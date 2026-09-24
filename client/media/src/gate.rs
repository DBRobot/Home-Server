//! A member's libraries from a device: the key out of the entry, the gate on
//! the box for one object at a time. `dd library`, `dd media` and the app
//! all come through here.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use auth::KeyStore;
use library::{Library, Record};

/// keyring account holding the name the device key signs for
pub const USER: &str = "user";
/// keyring account holding the root secret, base64 (a machine that made
/// the identity; a phone has none)
pub const ROOT: &str = "identity-root";

pub fn load_root(keys: &impl KeyStore) -> Result<Option<ed25519_dalek::SigningKey>> {
    match keys.get(ROOT)? {
        Some(s) => Ok(Some(identity::decode_secret(&s)?)),
        None => Ok(None),
    }
}

/// what this machine can open a library with
pub struct Opener {
    device: (String, ed25519_dalek::SigningKey),
    /// the root key, on a machine that made the identity
    pub root: Option<ed25519_dalek::SigningKey>,
}

impl Opener {
    pub fn load(keys: &impl KeyStore) -> Result<(Self, String, String)> {
        let kp = auth::device::load(keys)?.context("no device key here - `dd device show`")?;
        let user = keys
            .get(USER)?
            .context("no name on this machine - `dd identity new` or `dd identity import`")?
            .to_string();
        let public = auth::device::public_b64(&kp);
        let seed: [u8; 32] = kp.private().to_bytes()[..]
            .try_into()
            .context("device key is not 32 bytes")?;
        let token = auth::device::mint(&kp, &user, Duration::from_secs(3600))?;
        Ok((
            Opener {
                device: (
                    identity::fingerprint(&public),
                    ed25519_dalek::SigningKey::from_bytes(&seed),
                ),
                root: load_root(keys)?,
            },
            user,
            token,
        ))
    }
    fn open(&self, lib: &Library) -> Result<library::Key> {
        library::open_library(
            lib,
            Some((&self.device.0, &self.device.1)),
            self.root.as_ref(),
        )
        .with_context(|| format!("library {}: no key of this machine opens it", lib.id))
    }
}

/// the gate on the box, for one library
pub struct Gate {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl Gate {
    pub fn new(files_base: &str, lib: &str, token: &str) -> Self {
        Gate {
            base: format!("{}/_dd/library/{lib}", files_base.trim_end_matches('/')),
            token: token.to_string(),
            http: reqwest::Client::new(),
        }
    }
    pub async fn json(&self, method: reqwest::Method, path: &str) -> Result<serde_json::Value> {
        let r = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await?;
        let status = r.status();
        let body = r.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("gate: {status} {body}");
        }
        if body.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(serde_json::from_str(&body)?)
    }
    async fn url(&self, method: reqwest::Method, object: &str) -> Result<String> {
        let v = self.json(method, &format!("/url/{object}")).await?;
        v["url"]
            .as_str()
            .map(str::to_string)
            .context("gate gave no url")
    }
    async fn record_ids(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let path = match &after {
                Some(a) => format!("/records?after={a}"),
                None => "/records".to_string(),
            };
            let v = self.json(reqwest::Method::GET, &path).await?;
            for id in v["ids"].as_array().into_iter().flatten() {
                if let Some(s) = id.as_str() {
                    ids.push(s.to_string());
                }
            }
            if v["more"].as_bool() != Some(true) {
                break;
            }
            after = v["last"].as_str().map(str::to_string);
        }
        Ok(ids)
    }
    pub async fn fetch(&self, object: &str) -> Result<Vec<u8>> {
        let url = self.url(reqwest::Method::GET, object).await?;
        let r = self.http.get(url).send().await?;
        if !r.status().is_success() {
            bail!("fetching {object}: {}", r.status());
        }
        Ok(r.bytes().await?.to_vec())
    }
    pub async fn store(&self, object: &str, bytes: Vec<u8>) -> Result<()> {
        let url = self.url(reqwest::Method::PUT, object).await?;
        let r = self.http.put(url).body(bytes).send().await?;
        if !r.status().is_success() {
            bail!("storing {object}: {}", r.status());
        }
        Ok(())
    }
    pub async fn records(&self, key: &library::Key) -> Result<Vec<Record>> {
        let mut out = Vec::new();
        for id in self.record_ids().await? {
            let bytes = self.fetch(&library::record_object(&id)).await?;
            out.push(library::open_record(key, &id, &bytes)?);
        }
        Ok(out)
    }
}

/// every library this machine can open: the member's own, then those
/// shared with them by anyone whose entry names them
pub async fn openable(
    dirs: &[String],
    user: &str,
    opener: &Opener,
) -> Result<Vec<(String, Library, library::Key)>> {
    let mut out = Vec::new();
    let found = directory::fetch(dirs, user).await;
    let mine = directory::newest(&found).context("no directory has your entry")?;
    for lib in &mine.entry.libraries {
        if let Ok(k) = opener.open(lib) {
            out.push((user.to_string(), lib.clone(), k));
        }
    }
    // shared with us: a reader's keys are sealed the same way, under the
    // owner's entry. The gate lists every entry; a reader looks through
    // them once
    if let Some(dir) = dirs.first()
        && let Ok(names) = directory::names(dir).await
    {
        for name in names.into_iter().filter(|n| n != user) {
            let f = directory::fetch(dirs, &name).await;
            let Some(e) = directory::newest(&f) else {
                continue;
            };
            for lib in &e.entry.libraries {
                if let Some(r) = lib.readers.iter().find(|r| r.name == user) {
                    let as_mine = Library {
                        keys: r.keys.clone(),
                        ..lib.clone()
                    };
                    if let Ok(k) = opener.open(&as_mine) {
                        out.push((name.clone(), lib.clone(), k));
                    }
                }
            }
        }
    }
    Ok(out)
}

pub fn files_base(dirs: &[String]) -> Result<String> {
    // the gate is on the same host as the directory: https://files.<base>
    let d = dirs.first().context("no directory")?;
    Ok(d.trim_end_matches("/_dd/directory").to_string())
}
