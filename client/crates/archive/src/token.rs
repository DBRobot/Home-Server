//! A bearer token that renews itself.
//!
//! The kanidm id token lives fifteen minutes; a disk image takes hours. rustic
//! asks this for a token before every request and gets a cached one until it
//! is about to expire, or immediately after the server has rejected it.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use auth::KeyStore;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use zeroize::Zeroizing;

pub struct TokenProvider<K: KeyStore> {
    issuer: String,
    client_id: String,
    keys: K,
    /// the keyring account holding the refresh token - the cli owns the name
    account: String,
    rt: tokio::runtime::Runtime,
    cached: Mutex<Option<Cached>>,
}

struct Cached {
    id_token: Zeroizing<String>,
    expires: u64,
}

impl<K: KeyStore + Send + Sync> TokenProvider<K> {
    pub fn new(issuer: &str, client_id: &str, keys: K, account: &str) -> Result<Self> {
        Ok(Self {
            issuer: issuer.to_string(),
            client_id: client_id.to_string(),
            keys,
            account: account.to_string(),
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
            cached: Mutex::new(None),
        })
    }

    pub fn token(&self) -> Result<Zeroizing<String>> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let mut slot = self.cached.lock().unwrap();
        if let Some(c) = slot.as_ref()
            && c.expires > now + 60
        {
            return Ok(c.id_token.clone());
        }
        // A registered device signs its own: no server round trip at all.
        if let (Some(kp), Some(user)) = (auth::device::load(&self.keys)?, self.keys.get("user")?) {
            let ttl = 3600;
            let tok = Zeroizing::new(auth::device::mint(
                &kp,
                &user,
                std::time::Duration::from_secs(ttl),
            )?);
            *slot = Some(Cached {
                id_token: tok.clone(),
                expires: now + ttl,
            });
            return Ok(tok);
        }
        let stored = self
            .keys
            .get(&self.account)?
            .context("not signed in to kanidm - run `dd login`")?;
        let session = self
            .rt
            .block_on(auth::refresh(&self.issuer, &self.client_id, &stored))
            .context("renewing the kanidm token failed - run `dd login` again")?;
        // Kanidm rotates: the refresh token just used is dead, so the
        // replacement has to be stored or this is the last renewal that works.
        if let Some(rt) = &session.refresh_token {
            self.keys.set(&self.account, rt)?;
        }
        let id_token = Zeroizing::new(
            session
                .id_token
                .context("kanidm returned no id token, so there is no identity to present")?,
        );
        let expires = jwt_exp(&id_token).unwrap_or(now + 300);
        *slot = Some(Cached {
            id_token: id_token.clone(),
            expires,
        });
        Ok(id_token)
    }

    /// The server said 401: whatever is cached is no good, refresh on next ask.
    pub fn invalidate(&self) {
        *self.cached.lock().unwrap() = None;
    }
}

/// `exp` out of a jwt payload. No verification - the server does that; this
/// only decides when to ask for a new one.
fn jwt_exp(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp")?.as_u64()
}
