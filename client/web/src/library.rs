//! Libraries in the browser. A passkey's PRF secret is a key only that
//! passkey can make; from it this derives the keypair the member's
//! libraries are sealed to, opens a library key out of their entry, and
//! does the name and block crypto of the format (rclone's crypt, the same
//! code the app and `dd` use). Nothing here leaves the tab: the page asks
//! the gate for ciphertext and turns it into names and bytes itself.

use wasm_bindgen::prelude::*;
use zeroize::Zeroizing;

type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// The keypair a passkey makes: its PRF secret, hashed with a label of our
/// own so the photos key and this one are different keys, as an ed25519
/// seed. The public half goes in the entry (`dd passkey link`), the secret
/// half never exists outside this tab.
fn keypair(prf_secret_b64: &str) -> R<ed25519_dalek::SigningKey> {
    use base64::Engine as _;
    use sha2::Digest as _;
    let secret = base64::engine::general_purpose::STANDARD
        .decode(prf_secret_b64)
        .map_err(err)?;
    let mut h = sha2::Sha512::new();
    h.update(b"dd-library-device");
    h.update(&secret);
    let d = h.finalize();
    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&d[..32]);
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}

/// the public half, for `dd passkey link`
fn device_key(prf_secret_b64: &str) -> R<String> {
    Ok(identity::encode_public(
        &keypair(prf_secret_b64)?.verifying_key(),
    ))
}

/// A library this passkey can open: the entry as json, the library's id,
/// and the passkey's PRF secret. Returns the library key, base64, which
/// the calls below take. The page holds it for as long as the tab lives.
fn open(entry_json: &str, id: &str, prf_secret_b64: &str) -> R<String> {
    let entry: identity::SignedEntry = serde_json::from_str(entry_json).map_err(err)?;
    identity::verify(&entry).map_err(err)?;
    let lib = entry
        .entry
        .libraries
        .iter()
        .find(|l| l.id == id)
        .ok_or_else(|| "no such library in that entry".to_string())?;
    let sk = keypair(prf_secret_b64)?;
    let public = identity::encode_public(&sk.verifying_key());
    // the key sealed to this passkey's derived key, whichever entry named it
    for k in &lib.keys {
        if k.to.starts_with("passkey:")
            && let Ok(key) = library::open_with(&sk, &k.sealed)
        {
            let _ = &public;
            return Ok(library::crypt::password_of(&key));
        }
    }
    Err("no key in that library opens with this passkey: `dd passkey link` it once".into())
}

fn cipher(library_key_b64: &str, id: &str) -> R<library::crypt::Cipher> {
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(library_key_b64)
        .map_err(err)?;
    if raw.len() != 32 {
        return Err("that is not a library key".into());
    }
    let mut k = Zeroizing::new([0u8; 32]);
    k.copy_from_slice(&raw);
    Ok(library::crypt::Cipher::for_library(&k, id))
}

/// a plain path to the encrypted one the gate knows
fn encrypt_path(library_key_b64: &str, id: &str, plain: &str) -> R<String> {
    Ok(cipher(library_key_b64, id)?.encrypt_path(plain))
}

/// and back; an error means the name is not ours to read
fn decrypt_path(library_key_b64: &str, id: &str, enc: &str) -> R<String> {
    cipher(library_key_b64, id)?.decrypt_path(enc).map_err(err)
}

/// the plain bytes of a file the gate served, whole
fn open_file(library_key_b64: &str, id: &str, sealed: &[u8]) -> R<Vec<u8>> {
    let c = cipher(library_key_b64, id)?;
    if sealed.len() < library::crypt::HEADER {
        return Err("shorter than a header".into());
    }
    let d = c
        .decrypter(&sealed[..library::crypt::HEADER])
        .map_err(err)?;
    let mut out = Vec::with_capacity(sealed.len());
    for (n, block) in sealed[library::crypt::HEADER..]
        .chunks(library::crypt::SEALED_BLOCK)
        .enumerate()
    {
        out.extend(d.block(n as u64, block).map_err(err)?);
    }
    Ok(out)
}

/// a file the page is uploading: header and blocks, ready to PUT
fn seal_file(library_key_b64: &str, id: &str, plain: &[u8]) -> R<Vec<u8>> {
    let c = cipher(library_key_b64, id)?;
    let e = c.encrypter();
    let mut out = e.header();
    for (n, block) in plain.chunks(library::crypt::BLOCK).enumerate() {
        out.extend(e.block(n as u64, block));
    }
    Ok(out)
}

/// how many plain bytes a file of this sealed size holds
fn plain_of(sealed: f64) -> R<f64> {
    library::crypt::plain_size(sealed as u64)
        .map(|n| n as f64)
        .map_err(err)
}

// ---- what the page calls; the work is above, where a test can reach it

fn js<T>(r: R<T>) -> Result<T, JsValue> {
    r.map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn library_device_key(prf_secret_b64: &str) -> Result<String, JsValue> {
    js(device_key(prf_secret_b64))
}

#[wasm_bindgen]
pub fn library_open(entry_json: &str, id: &str, prf_secret_b64: &str) -> Result<String, JsValue> {
    js(open(entry_json, id, prf_secret_b64))
}

#[wasm_bindgen]
pub fn path_encrypt(library_key_b64: &str, id: &str, plain: &str) -> Result<String, JsValue> {
    js(encrypt_path(library_key_b64, id, plain))
}

#[wasm_bindgen]
pub fn path_decrypt(library_key_b64: &str, id: &str, enc: &str) -> Result<String, JsValue> {
    js(decrypt_path(library_key_b64, id, enc))
}

#[wasm_bindgen]
pub fn file_open(library_key_b64: &str, id: &str, sealed: &[u8]) -> Result<Vec<u8>, JsValue> {
    js(open_file(library_key_b64, id, sealed))
}

#[wasm_bindgen]
pub fn file_seal(library_key_b64: &str, id: &str, plain: &[u8]) -> Result<Vec<u8>, JsValue> {
    js(seal_file(library_key_b64, id, plain))
}

#[wasm_bindgen]
pub fn plain_size(sealed: f64) -> Result<f64, JsValue> {
    js(plain_of(sealed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// the key a passkey makes is the same every time and depends on the
    /// label, so the photos key and this one can never be the same key
    #[test]
    fn derived_key_is_stable_and_labelled() {
        use base64::Engine as _;
        let secret = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        let a = keypair(&secret).unwrap();
        let b = keypair(&secret).unwrap();
        assert_eq!(a.to_bytes(), b.to_bytes());
        let other = base64::engine::general_purpose::STANDARD.encode([8u8; 32]);
        assert_ne!(keypair(&other).unwrap().to_bytes(), a.to_bytes());
        // and the public half is what library_device_key prints
        assert_eq!(
            device_key(&secret).unwrap(),
            identity::encode_public(&a.verifying_key())
        );
    }

    /// what the page opens is what the fleet sealed
    #[test]
    fn a_passkey_opens_a_library_sealed_to_it() {
        use base64::Engine as _;
        let secret = base64::engine::general_purpose::STANDARD.encode([3u8; 32]);
        let device = device_key(&secret).unwrap();
        let root = identity::generate();
        let key = library::random_key();
        let id = library::random_id();
        let entry = identity::Entry {
            name: "sarah".into(),
            root: identity::encode_public(&root.verifying_key()),
            recovery: String::new(),
            devices: vec![],
            passkeys: vec![identity::Passkey {
                id: "pk1".into(),
                cred: serde_json::Value::Null,
                added: 0,
                library_key: Some(device),
            }],
            grant: None,
            libraries: vec![],
            version: 1,
            updated: 0,
        };
        let lib = identity::Library {
            id: id.clone(),
            keys: library::seal_for_entry(&entry, &key).unwrap(),
            readers: vec![],
            created: 0,
        };
        let entry = identity::Entry {
            libraries: vec![lib],
            ..entry
        };
        let signed = identity::sign(entry, &root).unwrap();
        let json = serde_json::to_string(&signed).unwrap();
        let opened = open(&json, &id, &secret).unwrap();
        assert_eq!(opened, library::crypt::password_of(&key));
        // a name written with it reads back
        let enc = encrypt_path(&opened, &id, "Files/a note.txt").unwrap();
        assert_eq!(
            decrypt_path(&opened, &id, &enc).unwrap(),
            "Files/a note.txt"
        );
        // and another passkey's secret does not open it
        let stranger = base64::engine::general_purpose::STANDARD.encode([9u8; 32]);
        assert!(open(&json, &id, &stranger).is_err());
    }
}
