//! A release: what every box should run, signed by the one key allowed to
//! say so.
//!
//! The laptop that builds the fleet writes one of these per release: the
//! commit it built, a counter, and for every box the exact store path and
//! the hash of its contents. It signs the whole thing with the release key
//! and publishes it where every box can fetch it. A box checks the
//! signature against the public key baked into it, refuses a counter lower
//! than the last it applied, fetches its own path, checks the hash, and
//! switches. Nothing a box or the forge does can produce a valid one.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use serde::{Deserialize, Serialize};

pub use identity::{decode_public, decode_secret, encode_public, encode_secret, generate};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("signature does not verify")]
    Signature,
    #[error("signed by {0}, not the release key")]
    Signer(String),
    #[error("key: {0}")]
    Key(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// One box's share of a release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoxRelease {
    /// the system closure to run, /nix/store/...-nixos-system-<box>-...
    pub path: String,
    /// its nar hash as `nix path-info` prints it; what the box checks after
    /// fetching, since the store path alone does not pin the contents
    pub nar_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Payload {
    /// strictly increasing; a box never goes back
    pub counter: u64,
    /// the commit every path was built from
    pub rev: String,
    /// seconds since the epoch, when this was signed
    pub issued: u64,
    /// box name -> what it runs. Sorted, so the bytes signed are stable.
    pub boxes: BTreeMap<String, BoxRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signed {
    pub payload: Payload,
    /// public key, base64
    pub signer: String,
    /// over `canonical(payload)`, base64
    pub signature: String,
}

pub fn canonical(p: &Payload) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(p)?)
}

pub fn sign(payload: Payload, key: &SigningKey) -> Result<Signed> {
    let sig: Signature = key.sign(&canonical(&payload)?);
    Ok(Signed {
        payload,
        signer: encode_public(&key.verifying_key()),
        signature: B64.encode(sig.to_bytes()),
    })
}

/// Signed by exactly `trusted`? Any other signer, however valid, is refused.
pub fn verify(signed: &Signed, trusted: &VerifyingKey) -> Result<()> {
    if signed.signer != encode_public(trusted) {
        return Err(Error::Signer(signed.signer.clone()));
    }
    let sig = B64
        .decode(&signed.signature)
        .map_err(|_| Error::Signature)?;
    let sig = Signature::from_slice(&sig).map_err(|_| Error::Signature)?;
    trusted
        .verify(&canonical(&signed.payload)?, &sig)
        .map_err(|_| Error::Signature)
}

/// The nar hash `nix path-info --json` printed for `path`. Nix has printed
/// both a map keyed by path and a list of entries; take either.
pub fn nar_hash_from_path_info(json: &str, path: &str) -> Result<String> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let entry = v.get(path).cloned().or_else(|| {
        v.as_array()?
            .iter()
            .find(|e| e.get("path").and_then(|p| p.as_str()) == Some(path))
            .cloned()
    });
    entry
        .as_ref()
        .and_then(|e| e.get("narHash"))
        .and_then(|h| h.as_str())
        .map(str::to_owned)
        .ok_or_else(|| Error::Key(format!("path-info has no narHash for {path}")))
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(counter: u64) -> Payload {
        let mut boxes = BTreeMap::new();
        boxes.insert(
            "node2".into(),
            BoxRelease {
                path: "/nix/store/aaaa-nixos-system-node2".into(),
                nar_hash: "sha256-AAAA".into(),
            },
        );
        Payload {
            counter,
            rev: "deadbeef".into(),
            issued: 1,
            boxes,
        }
    }

    #[test]
    fn signed_by_the_release_key_verifies() {
        let k = generate();
        let s = sign(payload(1), &k).unwrap();
        verify(&s, &k.verifying_key()).unwrap();
    }

    #[test]
    fn another_key_is_refused_even_when_its_signature_is_valid() {
        let k = generate();
        let other = generate();
        let s = sign(payload(1), &other).unwrap();
        assert!(matches!(
            verify(&s, &k.verifying_key()),
            Err(Error::Signer(_))
        ));
    }

    #[test]
    fn a_changed_payload_is_refused() {
        let k = generate();
        let mut s = sign(payload(1), &k).unwrap();
        s.payload.counter = 2;
        assert!(matches!(
            verify(&s, &k.verifying_key()),
            Err(Error::Signature)
        ));
    }

    #[test]
    fn survives_a_json_round_trip() {
        let k = generate();
        let s = sign(payload(7), &k).unwrap();
        let back: Signed =
            serde_json::from_str(&serde_json::to_string_pretty(&s).unwrap()).unwrap();
        verify(&back, &k.verifying_key()).unwrap();
        assert_eq!(back.payload, payload(7));
    }
}
