//! The directory: a person's signed entry, stored and served by any box.
//!
//! No credential admits an update. What admits it is a signature by the key
//! already on file, so a box running only this has nothing to lose but the
//! copies, and a box that wants to lie can only withhold or replay.
//!
//! Boxes share it. Each pulls its peers on a timer and runs what arrives
//! through the same rule a client's PUT gets, so a peer cannot push a
//! forgery either. The one moment the rule has nothing to check against is a
//! name this box has never seen, and that is where the peers come in: a
//! first sight from the network is taken only once this box has pulled every
//! peer at least once, and only if no peer knows the name under another root.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};

pub struct Directory {
    dir: PathBuf,
    /// the other boxes' directory urls, e.g. https://files.example/_dd/directory
    peers: Vec<String>,
    http: reqwest::Client,
    /// every peer answered a full pull at least once; until then no first
    /// sight from the network is taken
    synced: AtomicBool,
}

#[derive(Serialize, Deserialize)]
pub struct Listed {
    pub name: String,
    pub version: u64,
}

impl Directory {
    pub fn open(dir: PathBuf, peers: Vec<String>) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let synced = AtomicBool::new(peers.is_empty());
        Ok(Self {
            dir,
            peers,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            synced,
        })
    }

    /// The person's own signed entry, as last accepted by this box.
    pub fn entry(&self, name: &str) -> Result<Option<identity::SignedEntry>> {
        let p = self.dir.join(format!("{name}.json"));
        match std::fs::read(&p) {
            Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn list(&self) -> Result<Vec<Listed>> {
        let mut out = Vec::new();
        for f in std::fs::read_dir(&self.dir)? {
            let f = f?;
            let Some(name) = f
                .file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".json"))
                .map(str::to_string)
            else {
                continue;
            };
            if !identity::valid_name(&name) {
                continue;
            }
            if let Ok(Some(e)) = self.entry(&name) {
                out.push(Listed {
                    name,
                    version: e.entry.version,
                });
            }
        }
        Ok(out)
    }

    fn store(&self, signed: &identity::SignedEntry) -> Result<()> {
        let name = &signed.entry.name;
        let p = self.dir.join(format!("{name}.json"));
        let tmp = self.dir.join(format!(".{name}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(signed)?)?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }

    /// A first sight from the network: may this box take it? Every peer has
    /// to answer, and none may hold the name under a different root. A peer
    /// that holds it under the same root is fine - the client publishes to
    /// all of them in turn, so that is the normal case.
    async fn first_sight_allowed(
        &self,
        new: &identity::SignedEntry,
    ) -> std::result::Result<(), (StatusCode, String)> {
        if !self.synced.load(Ordering::Relaxed) {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "this box has not yet pulled every peer; new names wait".into(),
            ));
        }
        for p in &self.peers {
            let url = format!("{}/{}", p.trim_end_matches('/'), new.entry.name);
            let r = self.http.get(&url).send().await;
            match r {
                Ok(r) if r.status() == StatusCode::NOT_FOUND => {}
                Ok(r) if r.status().is_success() => {
                    let theirs: identity::SignedEntry = r
                        .json()
                        .await
                        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("peer {p}: {e}")))?;
                    if theirs.entry.root != new.entry.root {
                        return Err((
                            StatusCode::CONFLICT,
                            format!("{} is already taken (known to {p})", new.entry.name),
                        ));
                    }
                }
                Ok(r) => {
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        format!("peer {p} answered {}", r.status()),
                    ));
                }
                Err(e) => {
                    return Err((
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("cannot confirm the name is free: peer {p} unreachable ({e})"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// One pull of one peer: everything it has that is newer than ours,
    /// through the accept rule.
    async fn pull(&self, peer: &str) -> Result<usize> {
        let base = peer.trim_end_matches('/');
        let listed: Vec<Listed> = self
            .http
            .get(base)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("listing")?;
        let mut taken = 0;
        for l in listed {
            if !identity::valid_name(&l.name) {
                continue;
            }
            let ours = self.entry(&l.name)?;
            if ours.as_ref().is_some_and(|o| o.entry.version >= l.version) {
                continue;
            }
            let theirs: identity::SignedEntry = self
                .http
                .get(format!("{base}/{}", l.name))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            if theirs.entry.name != l.name {
                continue;
            }
            match identity::accept(ours.as_ref(), &theirs) {
                Ok(()) => {
                    self.store(&theirs)?;
                    taken += 1;
                    eprintln!(
                        "directory: {} version {} from {peer}",
                        theirs.entry.name, theirs.entry.version
                    );
                }
                Err(e) => eprintln!("directory: refused {} from {peer}: {e}", l.name),
            }
        }
        Ok(taken)
    }

    /// Forever: pull every peer, every five minutes (VERIFY_SYNC_SECS to
    /// change it). The first time every peer has answered, the box starts
    /// taking new names from the network.
    pub async fn sync_forever(self: Arc<Self>) {
        if self.peers.is_empty() {
            return;
        }
        let every = std::env::var("VERIFY_SYNC_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(300);
        let mut ok = vec![false; self.peers.len()];
        loop {
            for (i, p) in self.peers.iter().enumerate() {
                match self.pull(p).await {
                    Ok(_) => ok[i] = true,
                    Err(e) => eprintln!("directory: pull from {p} failed: {e:#}"),
                }
            }
            if ok.iter().all(|b| *b) && !self.synced.swap(true, Ordering::Relaxed) {
                eprintln!("directory: every peer pulled once; new names accepted from now");
            }
            tokio::time::sleep(Duration::from_secs(every)).await;
        }
    }
}

pub fn router(d: Arc<Directory>) -> Router {
    Router::new()
        .route("/_dd/directory", get(list))
        .route("/_dd/directory/{name}", get(get_entry).put(put_entry))
        .with_state(d)
}

async fn list(State(d): State<Arc<Directory>>) -> Response {
    match d.list() {
        Ok(l) => Json(l).into_response(),
        Err(e) => {
            eprintln!("directory: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Public: an entry holds only public keys.
async fn get_entry(State(d): State<Arc<Directory>>, Path(name): Path<String>) -> Response {
    if !identity::valid_name(&name) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match d.entry(&name) {
        Ok(Some(e)) => Json(e).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            eprintln!("directory: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn put_entry(
    State(d): State<Arc<Directory>>,
    Path(name): Path<String>,
    Json(signed): Json<identity::SignedEntry>,
) -> Response {
    if signed.entry.name != name {
        return (StatusCode::BAD_REQUEST, "name in path and entry differ").into_response();
    }
    let existing = match d.entry(&name) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("directory: {e:#}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if let Err(e) = identity::accept(existing.as_ref(), &signed) {
        eprintln!("directory: refused update for {name}: {e}");
        return (StatusCode::FORBIDDEN, format!("refused: {e}")).into_response();
    }
    if existing.is_none()
        && let Err((status, why)) = d.first_sight_allowed(&signed).await
    {
        eprintln!("directory: refused new name {name}: {why}");
        return (status, why).into_response();
    }
    if let Err(e) = d.store(&signed) {
        eprintln!("directory: {e:#}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    eprintln!(
        "directory: {name} version {} ({} device(s), {} passkey(s){})",
        signed.entry.version,
        signed.entry.devices.len(),
        signed.entry.passkeys.len(),
        if signed.recovery_signature.is_some() {
            ", recovered"
        } else {
            ""
        }
    );
    Json(serde_json::json!({ "accepted": true, "version": signed.entry.version })).into_response()
}
