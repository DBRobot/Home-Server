//! The key a person actually holds.
//!
//! `login` proves who you are to kanidm once per device. After that, this
//! device signs its own tokens: an ed25519 key, generated here, private half in
//! the OS keyring, public half registered with the verifier on the server. No
//! server ever holds anything that can sign as you - the most a compromised
//! one can do is serve what it already serves.

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
            .map_err(|e| crate::Error::Discovery(format!("device key: {e}")))?,
    );
    let sk = PrivateKey::from_bytes(&bytes, Algorithm::Ed25519)
        .map_err(|e| crate::Error::Discovery(format!("device key: {e}")))?;
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
/// Nothing else is in it yet: attenuation (this copy is read-only, this copy
/// is for files only) is a block appended later, by whoever holds it.
pub fn mint(kp: &KeyPair, user: &str, ttl: Duration) -> Result<String> {
    let exp = (SystemTime::now() + ttl)
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let err = |e: biscuit_auth::error::Token| crate::Error::Discovery(format!("mint: {e}"));
    let token = Biscuit::builder()
        .fact(format!("user({:?})", user).as_str())
        .map_err(err)?
        .fact(format!("device({:?})", fingerprint(kp)).as_str())
        .map_err(err)?
        // the check is in the token itself, so even a verifier that forgot to
        // supply the time could not accept an expired one
        .check(format!("check if time($t), $t < {exp}").as_str())
        .map_err(err)?
        .build(kp)
        .map_err(err)?;
    token.to_base64().map_err(err)
}

/// A ten-minute token from a registered device saying "admit this one too".
/// The verifier accepts it in place of trusting the identity server twice.
pub fn vouch(kp: &KeyPair, user: &str, fingerprint: &str) -> Result<String> {
    let exp = (SystemTime::now() + Duration::from_secs(600))
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let err = |e: biscuit_auth::error::Token| crate::Error::Discovery(format!("vouch: {e}"));
    Biscuit::builder()
        .fact(format!("user({:?})", user).as_str())
        .map_err(err)?
        .fact(format!("vouch({:?})", fingerprint).as_str())
        .map_err(err)?
        .check(format!("check if time($t), $t < {exp}").as_str())
        .map_err(err)?
        .build(kp)
        .map_err(err)?
        .to_base64()
        .map_err(err)
}

/// Short, stable name for a key: the first bytes of its public half.
pub fn fingerprint(kp: &KeyPair) -> String {
    let b = kp.public().to_bytes();
    b.iter().take(6).map(|x| format!("{x:02x}")).collect()
}

/// Ask the verifier to record this device's public key for `user`. `id_token`
/// is the kanidm id token from the login that just happened: the one moment
/// a server's word is taken for who this is. Once a user has a device, the
/// verifier requires an existing device to vouch for the next one, and
/// `voucher` is that signature.
pub async fn register(
    verifier: &str,
    id_token: &str,
    kp: &KeyPair,
    voucher: Option<&str>,
) -> Result<()> {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| crate::Error::Discovery(e.to_string()))?;
    let body = serde_json::json!({
        "public_key": public_b64(kp),
        "fingerprint": fingerprint(kp),
        "voucher": voucher,
    });
    let r = http
        .post(format!("{}/register", verifier.trim_end_matches('/')))
        .bearer_auth(id_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| crate::Error::Discovery(format!("registering the device: {e}")))?;
    if !r.status().is_success() {
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        return Err(crate::Error::Discovery(format!(
            "the verifier refused this device: {status} {text}"
        )));
    }
    Ok(())
}
