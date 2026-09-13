//! The directory: a person's signed entry, stored and served by any box.
//!
//! No credential admits an update. What admits it is a signature by the key
//! already on file, so a box running only this has nothing to lose but the
//! copies, and a box that wants to lie can only withhold or replay. It is
//! what a box with no other role runs.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};

pub struct Directory {
    dir: PathBuf,
}

impl Directory {
    pub fn open(dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
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

    fn store(&self, signed: &identity::SignedEntry) -> Result<()> {
        let name = &signed.entry.name;
        let p = self.dir.join(format!("{name}.json"));
        let tmp = self.dir.join(format!(".{name}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(signed)?)?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }
}

pub fn router(d: Arc<Directory>) -> Router {
    Router::new()
        .route("/_dd/directory/{name}", get(get_entry).put(put_entry))
        .with_state(d)
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
    if let Err(e) = d.store(&signed) {
        eprintln!("directory: {e:#}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    eprintln!(
        "directory: {name} version {} ({} device(s), signed by {:?})",
        signed.entry.version,
        signed.entry.devices.len(),
        signed.signer
    );
    Json(serde_json::json!({ "accepted": true, "version": signed.entry.version })).into_response()
}
