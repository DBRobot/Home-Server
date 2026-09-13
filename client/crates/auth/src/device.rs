//! The key a person actually holds.
//!
//! An ed25519 key, generated here, private half in the OS keyring. Its public
//! half is admitted to the person's own signed entry (the identity crate),
//! and from then on this device signs its own tokens. No server ever holds
//! anything that can sign as you - the most a compromised one can do is
//! serve what it already serves.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use biscuit_auth::{Biscuit, KeyPair, PrivateKey, builder::Algorithm};
use zeroize::Zeroizing;

use crate::{KeyStore, Result};

/// keyring account holding the private key, base64
pub const ACCOUNT: &str = "device-key";

pub fn generate() -> KeyPair {
    KeyPair::new_with_algorithm(Algorithm::Ed25519)
}

pub fn encode_private(kp: &KeyPair) -> Zeroizing<String> {
    Zeroizing::new(B64.encode(kp.private().to_bytes().as_slice()))
}

pub fn decode_private(s: &str) -> Result<KeyPair> {
    let bytes = Zeroizing::new(
        B64.decode(s.trim())
            .map_err(|e| crate::Error::Token(format!("device key: {e}")))?,
    );
    let sk = PrivateKey::from_bytes(&bytes, Algorithm::Ed25519)
        .map_err(|e| crate::Error::Token(format!("device key: {e}")))?;
    Ok(KeyPair::from(&sk))
}

/// The public half, as sent to the verifier and stored in its directory.
pub fn public_b64(kp: &KeyPair) -> String {
    B64.encode(kp.public().to_bytes())
}

/// Load the device key from the keyring, if this device has one.
pub fn load(keys: &impl KeyStore) -> Result<Option<KeyPair>> {
    match keys.get(ACCOUNT)? {
        Some(s) => Ok(Some(decode_private(&s)?)),
        None => Ok(None),
    }
}

/// Load or create, storing a new key in the keyring.
pub fn load_or_create(keys: &impl KeyStore) -> Result<(KeyPair, bool)> {
    if let Some(kp) = load(keys)? {
        return Ok((kp, false));
    }
    let kp = generate();
    keys.set(ACCOUNT, &encode_private(&kp))?;
    Ok((kp, true))
}

/// A token good for `ttl` from now, signed by this device, naming the user.
pub fn mint(kp: &KeyPair, user: &str, ttl: Duration) -> Result<String> {
    mint_for(kp, user, ttl, None)
}

/// The same, restricted to one operation. The verifier states what a request
/// is for ("access", "enrol") and a token that names one is refused for the
/// other: an enrol link in a terminal scrollback opens no file.
pub fn mint_for(
    kp: &KeyPair,
    user: &str,
    ttl: Duration,
    operation: Option<&str>,
) -> Result<String> {
    let exp = (SystemTime::now() + ttl)
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let err = |e: biscuit_auth::error::Token| crate::Error::Token(format!("mint: {e}"));
    let token = Biscuit::builder()
        .fact(format!("user({:?})", user).as_str())
        .map_err(err)?
        .fact(format!("device({:?})", fingerprint(kp)).as_str())
        .map_err(err)?
        // the check is in the token itself, so even a verifier that forgot to
        // supply the time could not accept an expired one
        .check(format!("check if time($t), $t < {exp}").as_str())
        .map_err(err)?;
    let token = match operation {
        Some(op) => token
            .check(format!("check if operation({op:?})").as_str())
            .map_err(err)?,
        None => token,
    };
    token.build(kp).map_err(err)?.to_base64().map_err(err)
}

/// Short, stable name for a key, the same one the directory entry shows.
pub fn fingerprint(kp: &KeyPair) -> String {
    let b = kp.public().to_bytes();
    b.iter().take(6).map(|x| format!("{x:02x}")).collect()
}
