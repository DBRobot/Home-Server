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
    /// One hash over the whole closure: every path it depends on, with its
    /// own nar hash, sorted and digested.
    ///
    /// The top path's hash is not enough on its own. A nixos toplevel is a
    /// tree of symlinks, so its nar pins the *names* of what it points at
    /// and nothing about the contents. Everything underneath arrives from
    /// the cache bucket, which more than one machine can write, and a
    /// poisoned dependency would install unremarked. Absent on releases
    /// made before this existed; a box checks it when it is there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closure: Option<String>,
}

/// The digest a `closure` carries: every path in `nix path-info -r --json`,
/// as "<path> <narHash>" lines, sorted, sha256, hex.
pub fn closure_digest(path_info_json: &str) -> Result<String> {
    use sha2::Digest as _;
    let v: serde_json::Value = serde_json::from_str(path_info_json)?;
    let mut lines: Vec<String> = Vec::new();
    match &v {
        serde_json::Value::Object(m) => {
            for (path, e) in m {
                let h = e
                    .get("narHash")
                    .and_then(|h| h.as_str())
                    .ok_or_else(|| Error::Key(format!("no narHash for {path}")))?;
                lines.push(format!("{path} {h}"));
            }
        }
        serde_json::Value::Array(a) => {
            for e in a {
                let path = e
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| Error::Key("path-info entry has no path".into()))?;
                let h = e
                    .get("narHash")
                    .and_then(|h| h.as_str())
                    .ok_or_else(|| Error::Key(format!("no narHash for {path}")))?;
                lines.push(format!("{path} {h}"));
            }
        }
        _ => return Err(Error::Key("path-info is neither object nor array".into())),
    }
    if lines.is_empty() {
        return Err(Error::Key("path-info listed nothing".into()));
    }
    lines.sort();
    let mut h = sha2::Sha256::new();
    for l in &lines {
        h.update(l.as_bytes());
        h.update(b"\n");
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
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
                closure: None,
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

#[cfg(test)]
mod closure_tests {
    use super::*;

    /// The point of the digest: change anything anywhere under the top
    /// path and it stops matching. The top path's own hash would not.
    #[test]
    fn every_path_under_the_top_one_is_pinned() {
        let honest = r#"{
          "/nix/store/aaa-system": {"narHash": "sha256-top"},
          "/nix/store/bbb-openssl": {"narHash": "sha256-ssl"},
          "/nix/store/ccc-glibc": {"narHash": "sha256-libc"}
        }"#;
        let a = closure_digest(honest).unwrap();

        // the same closure, listed in another order and in the array shape
        let reordered = r#"[
          {"path": "/nix/store/ccc-glibc", "narHash": "sha256-libc"},
          {"path": "/nix/store/aaa-system", "narHash": "sha256-top"},
          {"path": "/nix/store/bbb-openssl", "narHash": "sha256-ssl"}
        ]"#;
        assert_eq!(
            a,
            closure_digest(reordered).unwrap(),
            "order must not matter"
        );

        // one dependency swapped for different contents at the same path
        let poisoned = honest.replace("sha256-ssl", "sha256-someone-elses");
        assert_ne!(a, closure_digest(&poisoned).unwrap());

        // a dependency added, the top path untouched
        let extra = r#"{
          "/nix/store/aaa-system": {"narHash": "sha256-top"},
          "/nix/store/bbb-openssl": {"narHash": "sha256-ssl"},
          "/nix/store/ccc-glibc": {"narHash": "sha256-libc"},
          "/nix/store/ddd-extra": {"narHash": "sha256-extra"}
        }"#;
        assert_ne!(a, closure_digest(extra).unwrap());

        assert!(
            closure_digest("{}").is_err(),
            "nothing listed is not a closure"
        );
    }
}
