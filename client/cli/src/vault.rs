//! What gets persisted between runs, and how.
//!
//! master_key decrypts everything the account owns, so it is handled as
//! carefully as the rest of this project handles sops values: never printed,
//! never written to a file of ours, and zeroized on drop. The OS credential
//! store holds it - encrypted at rest by the OS, unlocked by the user's
//! normal login.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Vault {
    pub user_id: i64,
    /// base64 because these are bytes and the keystore takes a string
    pub token: String,
    pub master_key: String,
    pub secret_key: String,
    pub public_key: String,
}

impl Vault {
    pub fn from_secrets(user_id: i64, s: &ente::AccountSecrets) -> Self {
        Self {
            user_id,
            token: B64.encode(&s.token),
            master_key: B64.encode(&s.master_key),
            secret_key: B64.encode(&s.secret_key),
            public_key: B64.encode(&s.public_key),
        }
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(raw: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(raw)?)
    }
}

/// Deliberately not derived: a stray {:?} on this type would put the master
/// key in a log or a terminal scrollback.
impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("user_id", &self.user_id)
            .field("token", &"[REDACTED]")
            .field("master_key", &"[REDACTED]")
            .field("secret_key", &"[REDACTED]")
            .field("public_key_len", &self.public_key.len())
            .finish()
    }
}
