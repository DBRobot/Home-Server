//! An identity that no server issues.
//!
//! A person IS their root key. Their directory entry - name, root, recovery
//! key, the devices allowed to sign for them - is a document they sign
//! themselves. Boxes only store and relay it. A box can withhold an entry or
//! serve a stale one; it cannot produce a valid newer one, because it holds no
//! key that could sign it.
//!
//! Two keys, on purpose. The root key signs everything day to day and lives
//! on the person's devices. The recovery key lives on paper: it can sign
//! exactly one thing, an entry that installs a new root - so a lost or stolen
//! device is survivable without any server or person standing in for you.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bad key: {0}")]
    Key(String),
    #[error("bad signature")]
    Signature,
    #[error("{0}")]
    Rejected(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// What a person publishes about themselves. Field order is the canonical
/// form: serde keeps it, and the signature is over exactly these bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// base64 ed25519 public key that signs updates
    pub root: String,
    /// base64 ed25519 public key that may install a new root
    pub recovery: String,
    pub devices: Vec<Device>,
    /// strictly increasing; a box never accepts an older or equal one
    pub version: u64,
    pub updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub fingerprint: String,
    /// base64 ed25519 public key that signs this device's tokens
    pub public_key: String,
    pub added: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Signer_ {
    Root,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEntry {
    pub entry: Entry,
    pub signer: Signer_,
    /// base64 ed25519 signature over the canonical json of `entry`
    pub signature: String,
}

pub fn generate() -> SigningKey {
    use std::io::Read;
    let mut b = Zeroizing::new([0u8; 32]);
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut *b))
        .expect("/dev/urandom");
    SigningKey::from_bytes(&b)
}

pub fn encode_secret(k: &SigningKey) -> Zeroizing<String> {
    Zeroizing::new(B64.encode(k.to_bytes()))
}

pub fn decode_secret(s: &str) -> Result<SigningKey> {
    let b = Zeroizing::new(B64.decode(s.trim()).map_err(|e| Error::Key(e.to_string()))?);
    let arr: [u8; 32] = b[..].try_into().map_err(|_| Error::Key("secret is not 32 bytes".into()))?;
    Ok(SigningKey::from_bytes(&arr))
}

pub fn encode_public(k: &VerifyingKey) -> String {
    B64.encode(k.to_bytes())
}

pub fn decode_public(s: &str) -> Result<VerifyingKey> {
    let b = B64.decode(s.trim()).map_err(|e| Error::Key(e.to_string()))?;
    let arr: [u8; 32] = b[..].try_into().map_err(|_| Error::Key("public key is not 32 bytes".into()))?;
    VerifyingKey::from_bytes(&arr).map_err(|e| Error::Key(e.to_string()))
}

/// Short stable name for a public key: first six bytes, hex.
pub fn fingerprint(public_b64: &str) -> String {
    B64.decode(public_b64)
        .unwrap_or_default()
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn canonical(entry: &Entry) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(entry)?)
}

pub fn sign(entry: Entry, key: &SigningKey, signer: Signer_) -> Result<SignedEntry> {
    let sig: Signature = key.sign(&canonical(&entry)?);
    Ok(SignedEntry {
        entry,
        signer,
        signature: B64.encode(sig.to_bytes()),
    })
}

fn verify_with(signed: &SignedEntry, key: &VerifyingKey) -> Result<()> {
    let sig = B64.decode(&signed.signature).map_err(|_| Error::Signature)?;
    let sig = Signature::from_slice(&sig).map_err(|_| Error::Signature)?;
    key.verify(&canonical(&signed.entry)?, &sig).map_err(|_| Error::Signature)
}

/// The rule a box applies before storing `new` in place of `existing`.
///
/// - first sight: the entry must be signed by its own root
/// - otherwise: strictly newer version, and signed by the root ALREADY on
///   file - or by the recovery key already on file, in which case the root
///   may change (that is what recovery is)
///
/// Nothing here consults a server, an admin, or a name registry. A box that
/// wants to lie can only withhold or replay; it cannot make this pass.
pub fn accept(existing: Option<&SignedEntry>, new: &SignedEntry) -> Result<()> {
    if !valid_name(&new.entry.name) {
        return Err(Error::Rejected("bad name".into()));
    }
    if new.entry.devices.is_empty() {
        return Err(Error::Rejected("an entry needs at least one device".into()));
    }
    for d in &new.entry.devices {
        decode_public(&d.public_key)?;
    }
    match existing {
        None => {
            if new.entry.version != 1 {
                return Err(Error::Rejected("a first entry has version 1".into()));
            }
            if new.signer != Signer_::Root {
                return Err(Error::Rejected("a first entry is signed by its root".into()));
            }
            verify_with(new, &decode_public(&new.entry.root)?)
        }
        Some(old) => {
            if new.entry.name != old.entry.name {
                return Err(Error::Rejected("name mismatch".into()));
            }
            if new.entry.version <= old.entry.version {
                return Err(Error::Rejected(format!(
                    "version {} is not newer than {}",
                    new.entry.version, old.entry.version
                )));
            }
            match new.signer {
                Signer_::Root => {
                    if new.entry.root != old.entry.root {
                        return Err(Error::Rejected("only the recovery key may change the root".into()));
                    }
                    verify_with(new, &decode_public(&old.entry.root)?)
                }
                Signer_::Recovery => verify_with(new, &decode_public(&old.entry.recovery)?),
            }
        }
    }
}

pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
        && !s.starts_with('.')
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

    fn dev(k: &SigningKey) -> Device {
        let p = encode_public(&k.verifying_key());
        Device { fingerprint: fingerprint(&p), public_key: p, added: 1 }
    }

    #[test]
    fn birth_update_rotation_and_the_things_a_box_must_refuse() {
        let root = generate();
        let recovery = generate();
        let d1 = generate();
        let e1 = Entry {
            name: "sarah".into(),
            root: encode_public(&root.verifying_key()),
            recovery: encode_public(&recovery.verifying_key()),
            devices: vec![dev(&d1)],
            version: 1,
            updated: 1,
        };
        let s1 = sign(e1.clone(), &root, Signer_::Root).unwrap();
        accept(None, &s1).unwrap();

        // a stranger's key cannot produce version 2
        let stranger = generate();
        let mut e2 = e1.clone();
        e2.version = 2;
        e2.devices.push(dev(&generate()));
        assert!(accept(Some(&s1), &sign(e2.clone(), &stranger, Signer_::Root).unwrap()).is_err());
        // the root can
        let s2 = sign(e2.clone(), &root, Signer_::Root).unwrap();
        accept(Some(&s1), &s2).unwrap();
        // replaying the old one is refused
        assert!(accept(Some(&s2), &s1).is_err());
        // the root may not swap itself out
        let mut e3 = e2.clone();
        e3.version = 3;
        e3.root = encode_public(&stranger.verifying_key());
        assert!(accept(Some(&s2), &sign(e3.clone(), &root, Signer_::Root).unwrap()).is_err());
        // the recovery key may: that is recovery
        let s3 = sign(e3.clone(), &recovery, Signer_::Recovery).unwrap();
        accept(Some(&s2), &s3).unwrap();
        // and afterwards the OLD root is out
        let mut e4 = e3.clone();
        e4.version = 4;
        assert!(accept(Some(&s3), &sign(e4.clone(), &root, Signer_::Root).unwrap()).is_err());
        assert!(accept(Some(&s3), &sign(e4, &stranger, Signer_::Root).unwrap()).is_ok());
        // a first entry must be signed by its own root
        let mut e0 = e1.clone();
        e0.root = encode_public(&stranger.verifying_key());
        assert!(accept(None, &sign(e0, &root, Signer_::Root).unwrap()).is_err());
    }
}
