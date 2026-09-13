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
    /// Browser passkeys, as the box that enrolled them serialised the
    /// credential. In the entry rather than on a box, so no box can add one
    /// for you and any box can check a login against them. Absent when
    /// empty, so entries signed before the field existed still verify.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passkeys: Vec<Passkey>,
    /// strictly increasing; a box never accepts an older or equal one
    pub version: u64,
    pub updated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Passkey {
    /// the credential id, base64url, as the browser presents it
    pub id: String,
    /// the whole credential as webauthn-rs serialises it; opaque here
    pub cred: serde_json::Value,
    pub added: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub fingerprint: String,
    /// base64 ed25519 public key that signs this device's tokens
    pub public_key: String,
    pub added: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEntry {
    pub entry: Entry,
    /// base64 ed25519 signature over the canonical json of `entry`, by the
    /// root named IN the entry. Always present, so an entry at any version
    /// stands on its own: a box that has never seen the name can still tell
    /// the entry is internally honest.
    pub signature: String,
    /// When the root changed: the same bytes signed by the recovery key that
    /// was on file before. The box that holds the previous entry checks it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_signature: Option<String>,
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
    let b = Zeroizing::new(
        B64.decode(s.trim())
            .map_err(|e| Error::Key(e.to_string()))?,
    );
    let arr: [u8; 32] = b[..]
        .try_into()
        .map_err(|_| Error::Key("secret is not 32 bytes".into()))?;
    Ok(SigningKey::from_bytes(&arr))
}

pub fn encode_public(k: &VerifyingKey) -> String {
    B64.encode(k.to_bytes())
}

pub fn decode_public(s: &str) -> Result<VerifyingKey> {
    let b = B64
        .decode(s.trim())
        .map_err(|e| Error::Key(e.to_string()))?;
    let arr: [u8; 32] = b[..]
        .try_into()
        .map_err(|_| Error::Key("public key is not 32 bytes".into()))?;
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

/// Sign with the root the entry names.
pub fn sign(entry: Entry, root: &SigningKey) -> Result<SignedEntry> {
    if entry.root != encode_public(&root.verifying_key()) {
        return Err(Error::Key("that is not the root the entry names".into()));
    }
    let sig: Signature = root.sign(&canonical(&entry)?);
    Ok(SignedEntry {
        entry,
        signature: B64.encode(sig.to_bytes()),
        recovery_signature: None,
    })
}

/// Recovery: a new root signs, and the old recovery key co-signs to hand over.
pub fn sign_recovery(
    entry: Entry,
    new_root: &SigningKey,
    recovery: &SigningKey,
) -> Result<SignedEntry> {
    let mut signed = sign(entry, new_root)?;
    let sig: Signature = recovery.sign(&canonical(&signed.entry)?);
    signed.recovery_signature = Some(B64.encode(sig.to_bytes()));
    Ok(signed)
}

fn check(sig_b64: &str, entry: &Entry, key: &VerifyingKey) -> Result<()> {
    let sig = B64.decode(sig_b64).map_err(|_| Error::Signature)?;
    let sig = Signature::from_slice(&sig).map_err(|_| Error::Signature)?;
    key.verify(&canonical(entry)?, &sig)
        .map_err(|_| Error::Signature)
}

/// Is the entry signed by the root it names? True of every valid entry.
pub fn verify(signed: &SignedEntry) -> Result<()> {
    check(
        &signed.signature,
        &signed.entry,
        &decode_public(&signed.entry.root)?,
    )
}

/// The rule a box applies before storing `new` in place of `existing`.
///
/// - always: signed by the root it names
/// - first sight: nothing more. Whether a first sight may be trusted at all
///   is the box's question (has any peer seen the name?), not this one's.
/// - otherwise: strictly newer version, and either the same root as on
///   file, or a different root co-signed by the recovery key on file - that
///   is what recovery is
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
    verify(new)?;
    let Some(old) = existing else {
        return Ok(());
    };
    if new.entry.name != old.entry.name {
        return Err(Error::Rejected("name mismatch".into()));
    }
    if new.entry.version <= old.entry.version {
        return Err(Error::Rejected(format!(
            "version {} is not newer than {}",
            new.entry.version, old.entry.version
        )));
    }
    if new.entry.root == old.entry.root {
        return Ok(());
    }
    let Some(rs) = &new.recovery_signature else {
        return Err(Error::Rejected(
            "only the recovery key may change the root".into(),
        ));
    };
    check(rs, &new.entry, &decode_public(&old.entry.recovery)?)
}

pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
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
        Device {
            fingerprint: fingerprint(&p),
            public_key: p,
            added: 1,
        }
    }

    #[test]
    fn birth_update_rotation_and_the_things_a_box_must_refuse() {
        let root = generate();
        let recovery = generate();
        let d1 = generate();
        let e1 = Entry {
            passkeys: vec![],
            name: "sarah".into(),
            root: encode_public(&root.verifying_key()),
            recovery: encode_public(&recovery.verifying_key()),
            devices: vec![dev(&d1)],
            version: 1,
            updated: 1,
        };
        let s1 = sign(e1.clone(), &root).unwrap();
        accept(None, &s1).unwrap();

        // a stranger's key cannot produce version 2: sign() refuses a key the
        // entry does not name, and a forged signature fails accept
        let stranger = generate();
        let mut e2 = e1.clone();
        e2.version = 2;
        e2.devices.push(dev(&generate()));
        assert!(sign(e2.clone(), &stranger).is_err());
        let mut forged = sign(e2.clone(), &root).unwrap();
        forged.signature = B64.encode([0u8; 64]);
        assert!(accept(Some(&s1), &forged).is_err());
        // the root can
        let s2 = sign(e2.clone(), &root).unwrap();
        accept(Some(&s1), &s2).unwrap();
        // replaying the old one is refused
        assert!(accept(Some(&s2), &s1).is_err());
        // a new root on its own is refused, however well it signs itself
        let mut e3 = e2.clone();
        e3.version = 3;
        e3.root = encode_public(&stranger.verifying_key());
        assert!(accept(Some(&s2), &sign(e3.clone(), &stranger).unwrap()).is_err());
        // co-signed by the recovery key on file it is recovery
        let s3 = sign_recovery(e3.clone(), &stranger, &recovery).unwrap();
        accept(Some(&s2), &s3).unwrap();
        // co-signed by the wrong recovery key it is not
        assert!(
            accept(
                Some(&s2),
                &sign_recovery(e3.clone(), &stranger, &generate()).unwrap()
            )
            .is_err()
        );
        // afterwards the OLD root is out and the new one is in
        let mut e4 = e3.clone();
        e4.version = 4;
        assert!(sign(e4.clone(), &root).is_err());
        assert!(accept(Some(&s3), &sign(e4, &stranger).unwrap()).is_ok());
        // a box seeing the name for the first time takes any version that
        // stands on its own - the peer check is the box's job, not this one's
        accept(None, &s3).unwrap();
        // but never an entry whose signature is not by the root it names
        let mut e0 = e1.clone();
        e0.root = encode_public(&stranger.verifying_key());
        let mut bad = sign(e0, &stranger).unwrap();
        bad.signature = s1.signature.clone();
        assert!(accept(None, &bad).is_err());
    }
}
