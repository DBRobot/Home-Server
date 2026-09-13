//! One memorised password, several encryption keys.
//!
//! The photo password has to reach ente as typed - its mobile apps log in
//! with it - but nothing else needs the raw string. Every other end-to-end
//! secret this client holds is derived from it, so a person remembers one
//! thing and a new machine needs `dd unlock`, never a second password.
//!
//! HKDF-SHA256 from RustCrypto; the salt and label are fixed so the result is
//! the same on every machine for the same person.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

const SALT: &[u8] = b"distributed-datacenter";

fn derive(master: &str, label: &str) -> Zeroizing<String> {
    let hk = Hkdf::<Sha256>::new(Some(SALT), master.as_bytes());
    let mut out = Zeroizing::new([0u8; 32]);
    hk.expand(label.as_bytes(), out.as_mut())
        .expect("32 bytes is within hkdf-sha256's limit");
    Zeroizing::new(URL_SAFE_NO_PAD.encode(out.as_ref()))
}

/// The restic repository password for `dd image`. Only dd ever uses it, so
/// it can be a different secret from the one that was typed.
pub fn archive_password(master: &str) -> Zeroizing<String> {
    derive(master, "archive password v1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_distinct() {
        let a = archive_password("correct horse");
        assert_eq!(*a, *archive_password("correct horse"));
        assert_ne!(*a, *archive_password("correct horse "));
        assert_ne!(*a, *derive("correct horse", "something else"));
        assert_eq!(a.len(), 43, "32 bytes, base64url, unpadded");
    }
}
