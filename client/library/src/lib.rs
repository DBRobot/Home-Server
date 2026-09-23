//! A member's encrypted library: the files a box holds but cannot read.
//!
//! One library is one bucket. A file is a random id, its content in
//! fixed-size chunks each encrypted with the file's own key, and a record
//! (name, size, chunk count, the file key) encrypted with the library key.
//! The library key is sealed to the member's device, root and recovery
//! keys and published in their directory entry: the keys live in the
//! identity, never on a box, never in a file on disk. Sharing a library is
//! sealing its key to one more reader; taking it back is a new key.
//!
//! What a box learns: how many chunks exist, how big they are, who fetches
//! which when. Names, contents and the shape of a file, never.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// a chunk of content before encryption; the last chunk of a file is shorter
pub const CHUNK: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("key: {0}")]
    Key(String),
    #[error("this key does not open it")]
    Sealed,
    #[error("ciphertext is damaged or not for this key")]
    Aead,
    #[error("record: {0}")]
    Record(String),
    #[error(transparent)]
    Identity(#[from] identity::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// a 32-byte symmetric key, wiped when dropped
pub type Key = Zeroizing<[u8; 32]>;

pub fn random_key() -> Key {
    let mut k = Zeroizing::new([0u8; 32]);
    fill(&mut k[..]);
    k
}

/// 32 hex characters: a library or file id, safe in a url, a bucket key
/// and a directory entry alike
pub fn random_id() -> String {
    let mut b = [0u8; 16];
    fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn fill(buf: &mut [u8]) {
    use std::io::Read as _;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .expect("/dev/urandom");
}

// ---------------------------------------------------------------- sealing

/// The x25519 keys that go with an identity's ed25519 keys, so a library
/// key can be sealed to a device, root or recovery key already in the
/// directory. Standard birational map (the same one libsodium uses).
pub fn seal_to(public_ed25519_b64: &str, secret: &[u8]) -> Result<String> {
    let vk = identity::decode_public(public_ed25519_b64)?;
    let ed = curve25519_dalek::edwards::CompressedEdwardsY(vk.to_bytes())
        .decompress()
        .ok_or_else(|| Error::Key("not a valid ed25519 point".into()))?;
    let mont = ed.to_montgomery();
    let pk = crypto_box::PublicKey::from(mont.to_bytes());
    let sealed = pk.seal(&mut rand_core(), secret).map_err(|_| Error::Aead)?;
    Ok(B64.encode(sealed))
}

/// open what was sealed to this ed25519 signing key
pub fn open_with(secret_ed25519: &ed25519_dalek::SigningKey, sealed_b64: &str) -> Result<Key> {
    let h = sha2::Sha512::digest(secret_ed25519.to_bytes());
    let mut scalar = [0u8; 32];
    scalar.copy_from_slice(&h[..32]);
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;
    let sk = crypto_box::SecretKey::from(scalar);
    let sealed = B64
        .decode(sealed_b64)
        .map_err(|e| Error::Key(e.to_string()))?;
    let opened = sk.unseal(&sealed).map_err(|_| Error::Sealed)?;
    let arr: [u8; 32] = opened[..]
        .try_into()
        .map_err(|_| Error::Key("sealed key is not 32 bytes".into()))?;
    Ok(Zeroizing::new(arr))
}

use sha2::Digest as _;

fn rand_core() -> Urandom {
    Urandom
}

/// /dev/urandom as a RngCore, for the sealing api that wants one
struct Urandom;
impl crypto_box::aead::rand_core::RngCore for Urandom {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        fill(&mut b);
        u32::from_le_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        fill(&mut b);
        u64::from_le_bytes(b)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        fill(dest)
    }
    fn try_fill_bytes(
        &mut self,
        dest: &mut [u8],
    ) -> std::result::Result<(), crypto_box::aead::rand_core::Error> {
        fill(dest);
        Ok(())
    }
}
impl crypto_box::aead::rand_core::CryptoRng for Urandom {}

// ------------------------------------------------------------- directory

pub use identity::{Library, Reader, SealedKey};

/// the library key sealed to every key an entry names for its owner
pub fn seal_for_entry(entry: &identity::Entry, key: &Key) -> Result<Vec<SealedKey>> {
    let mut out = Vec::new();
    for d in &entry.devices {
        out.push(SealedKey {
            to: format!("device:{}", d.fingerprint),
            sealed: seal_to(&d.public_key, &key[..])?,
        });
    }
    if !entry.root.starts_with("webauthn:") {
        out.push(SealedKey {
            to: "root".into(),
            sealed: seal_to(&entry.root, &key[..])?,
        });
    }
    if !entry.recovery.is_empty() {
        out.push(SealedKey {
            to: "recovery".into(),
            sealed: seal_to(&entry.recovery, &key[..])?,
        });
    }
    Ok(out)
}

/// open the library key with whichever of this machine's keys it was sealed to
pub fn open_library(
    lib: &Library,
    device: Option<(&str, &ed25519_dalek::SigningKey)>,
    root: Option<&ed25519_dalek::SigningKey>,
) -> Result<Key> {
    for k in &lib.keys {
        if let Some((fp, sk)) = device
            && k.to == format!("device:{fp}")
            && let Ok(key) = open_with(sk, &k.sealed)
        {
            return Ok(key);
        }
        if k.to == "root"
            && let Some(sk) = root
            && let Ok(key) = open_with(sk, &k.sealed)
        {
            return Ok(key);
        }
    }
    Err(Error::Sealed)
}

// ---------------------------------------------------------------- content

fn cipher(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

/// the nonce of chunk `n` of a file: the file key is used for one file
/// only, so a counter nonce is sound and the chunk index binds each
/// chunk to its place
fn chunk_nonce(n: u64) -> XNonce {
    let mut b = [0u8; 24];
    b[..8].copy_from_slice(&n.to_le_bytes());
    XNonce::from(b)
}

/// one chunk, encrypted with the file key at its index
pub fn seal_chunk(file_key: &Key, index: u64, plain: &[u8]) -> Result<Vec<u8>> {
    cipher(file_key)
        .encrypt(
            &chunk_nonce(index),
            Payload {
                msg: plain,
                aad: &index.to_le_bytes(),
            },
        )
        .map_err(|_| Error::Aead)
}

pub fn open_chunk(file_key: &Key, index: u64, sealed: &[u8]) -> Result<Vec<u8>> {
    cipher(file_key)
        .decrypt(
            &chunk_nonce(index),
            Payload {
                msg: sealed,
                aad: &index.to_le_bytes(),
            },
        )
        .map_err(|_| Error::Aead)
}

/// the size of a chunk on the box for a chunk of this many plain bytes
pub fn sealed_len(plain: usize) -> usize {
    plain + 16
}

// ---------------------------------------------------------------- records

/// What a file is, as the record encrypted with the library key says.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    /// a path inside the library, "/" separated, no leading slash
    pub name: String,
    pub size: u64,
    pub chunks: u64,
    /// the file key, base64
    pub key: String,
    pub modified: u64,
}

/// a record on the box: random nonce, then the ciphertext; the record id is
/// bound as associated data so a record cannot be moved under another id
pub fn seal_record(library_key: &Key, r: &Record) -> Result<Vec<u8>> {
    let plain = serde_json::to_vec(r).map_err(|e| Error::Record(e.to_string()))?;
    let mut nonce = [0u8; 24];
    fill(&mut nonce);
    let sealed = cipher(library_key)
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plain,
                aad: r.id.as_bytes(),
            },
        )
        .map_err(|_| Error::Aead)?;
    let mut out = nonce.to_vec();
    out.extend(sealed);
    Ok(out)
}

pub fn open_record(library_key: &Key, id: &str, bytes: &[u8]) -> Result<Record> {
    if bytes.len() < 24 {
        return Err(Error::Record("too short".into()));
    }
    let (nonce, sealed) = bytes.split_at(24);
    let plain = cipher(library_key)
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: sealed,
                aad: id.as_bytes(),
            },
        )
        .map_err(|_| Error::Aead)?;
    let r: Record = serde_json::from_slice(&plain).map_err(|e| Error::Record(e.to_string()))?;
    if r.id != id {
        return Err(Error::Record("id inside does not match".into()));
    }
    Ok(r)
}

pub fn file_key(r: &Record) -> Result<Key> {
    let b = B64.decode(&r.key).map_err(|e| Error::Key(e.to_string()))?;
    let arr: [u8; 32] = b[..]
        .try_into()
        .map_err(|_| Error::Key("file key is not 32 bytes".into()))?;
    Ok(Zeroizing::new(arr))
}

pub fn encode_key(k: &Key) -> String {
    B64.encode(&k[..])
}

/// the object names inside a library's bucket
pub fn record_object(id: &str) -> String {
    format!("records/{id}")
}
pub fn chunk_object(id: &str, n: u64) -> String {
    format!("chunks/{id}/{n:08}")
}
pub fn trash_prefix(id: &str) -> String {
    format!("trash/{id}/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        devices: &[&ed25519_dalek::SigningKey],
        root: &ed25519_dalek::SigningKey,
    ) -> identity::Entry {
        identity::Entry {
            name: "amy".into(),
            root: identity::encode_public(&root.verifying_key()),
            recovery: String::new(),
            devices: devices
                .iter()
                .map(|d| identity::Device {
                    fingerprint: identity::fingerprint(&identity::encode_public(
                        &d.verifying_key(),
                    )),
                    public_key: identity::encode_public(&d.verifying_key()),
                    added: 1,
                })
                .collect(),
            passkeys: vec![],
            grant: None,
            libraries: vec![],
            version: 1,
            updated: 1,
        }
    }

    #[test]
    fn a_library_key_opens_with_a_device_or_the_root_and_nothing_else() {
        let root = identity::generate();
        let phone = identity::generate();
        let stranger = identity::generate();
        let e = entry(&[&phone], &root);
        let key = random_key();
        let lib = Library {
            id: random_id(),
            keys: seal_for_entry(&e, &key).unwrap(),
            readers: vec![],
            created: 1,
        };
        assert_eq!(lib.keys.len(), 2);
        let fp = identity::fingerprint(&identity::encode_public(&phone.verifying_key()));
        assert_eq!(
            &open_library(&lib, Some((&fp, &phone)), None).unwrap()[..],
            &key[..]
        );
        assert_eq!(
            &open_library(&lib, None, Some(&root)).unwrap()[..],
            &key[..]
        );
        assert!(matches!(
            open_library(&lib, None, Some(&stranger)),
            Err(Error::Sealed)
        ));
    }

    #[test]
    fn chunks_open_only_at_their_index_with_their_key() {
        let k = random_key();
        let plain = b"the first chunk of a film";
        let sealed = seal_chunk(&k, 0, plain).unwrap();
        assert_eq!(sealed.len(), sealed_len(plain.len()));
        assert_eq!(open_chunk(&k, 0, &sealed).unwrap(), plain);
        assert!(
            open_chunk(&k, 1, &sealed).is_err(),
            "moved to another place"
        );
        assert!(
            open_chunk(&random_key(), 0, &sealed).is_err(),
            "another file's key"
        );
    }

    #[test]
    fn a_record_is_bound_to_its_id() {
        let lk = random_key();
        let fk = random_key();
        let r = Record {
            id: random_id(),
            name: "films/Heat (1995).mkv".into(),
            size: 12_345,
            chunks: 1,
            key: encode_key(&fk),
            modified: 7,
        };
        let sealed = seal_record(&lk, &r).unwrap();
        assert_eq!(open_record(&lk, &r.id, &sealed).unwrap(), r);
        assert!(open_record(&lk, "some-other-id", &sealed).is_err());
        assert_eq!(&file_key(&r).unwrap()[..], &fk[..]);
    }
}
