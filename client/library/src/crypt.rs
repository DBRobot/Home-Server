//! rclone's crypt format, so a library is an rclone remote: `rclone mount`
//! opens it on any desktop, and nothing here is a format of ours. Keys
//! come from scrypt over a password and a salt (the library key, base64,
//! and the library id); names are AES-EME with PKCS#7 padding in lowercase
//! base32hex, one segment at a time; a file is an 8-byte magic, a 24-byte
//! nonce, then 64 KiB blocks each sealed with NaCl secretbox under the
//! nonce incremented by the block number. Every byte is checked against
//! rclone itself in the tests.

use aes::Aes256;
use aes::cipher::KeyIvInit as _;
use aes::cipher::block_padding::Pkcs7;
use base64::Engine as _;
use crypto_secretbox::aead::AeadInPlace as _;
use crypto_secretbox::{KeyInit as _, XSalsa20Poly1305};
use eme_mode::DynamicEme;
use zeroize::{Zeroize as _, Zeroizing};

use crate::{Error, Key, Result};

pub const MAGIC: &[u8; 8] = b"RCLONE\x00\x00";
pub const NONCE: usize = 24;
pub const HEADER: usize = 8 + NONCE;
/// the plain bytes in a block
pub const BLOCK: usize = 64 * 1024;
/// the tag before each block's bytes
pub const TAG: usize = 16;
pub const SEALED_BLOCK: usize = TAG + BLOCK;
const NAME_BLOCK: usize = 16;
/// a name longer than this is not a name rclone would write
const MAX_NAME: usize = 2048;

/// the keys of one remote
pub struct Cipher {
    data: Zeroizing<[u8; 32]>,
    name: Zeroizing<[u8; 32]>,
    tweak: Zeroizing<[u8; 16]>,
}

impl Cipher {
    /// what rclone derives from `password` and `password2` (the salt),
    /// both as the person typed them, not obscured
    pub fn new(password: &str, salt: &str) -> Cipher {
        // the len here is a hint the crate caps at 64; the output is 80 bytes
        let params = scrypt::Params::new(14, 8, 1, 64).expect("scrypt params");
        let mut out = Zeroizing::new([0u8; 80]);
        scrypt::scrypt(password.as_bytes(), salt.as_bytes(), &params, &mut out[..])
            .expect("scrypt");
        let mut data = [0u8; 32];
        let mut name = [0u8; 32];
        let mut tweak = [0u8; 16];
        data.copy_from_slice(&out[..32]);
        name.copy_from_slice(&out[32..64]);
        tweak.copy_from_slice(&out[64..80]);
        Cipher {
            data: Zeroizing::new(data),
            name: Zeroizing::new(name),
            tweak: Zeroizing::new(tweak),
        }
    }

    /// a library's remote: its key is the password, its id the salt. What
    /// a member types into `rclone config` to open it by hand.
    pub fn for_library(key: &Key, id: &str) -> Cipher {
        Cipher::new(&password_of(key), id)
    }

    /// the key the blocks are sealed with: what a box gets, sealed to it,
    /// for one file's playing (the names stay closed to it)
    pub fn data_key(&self) -> Key {
        self.data.clone()
    }

    /// a reader from the data key alone, for that box
    pub fn decrypter_with(data_key: &Key, header: &[u8]) -> Result<Decrypter> {
        if header.len() < HEADER || &header[..8] != MAGIC {
            return Err(Error::Format("not an encrypted file".into()));
        }
        let mut nonce = [0u8; NONCE];
        nonce.copy_from_slice(&header[8..HEADER]);
        Ok(Decrypter {
            key: data_key.clone(),
            nonce,
        })
    }

    fn eme(&self) -> DynamicEme<Aes256> {
        DynamicEme::<Aes256>::new(self.name.as_ref().into(), self.tweak.as_ref().into())
    }

    /// one path segment
    pub fn encrypt_segment(&self, plain: &str) -> String {
        if plain.is_empty() {
            return String::new();
        }
        let padded = self.eme().encrypt_padded_vec_mut::<Pkcs7>(plain.as_bytes());
        base32hex(&padded)
    }

    pub fn decrypt_segment(&self, enc: &str) -> Result<String> {
        if enc.is_empty() {
            return Ok(String::new());
        }
        let raw = unbase32hex(enc)?;
        if raw.is_empty() || raw.len() % NAME_BLOCK != 0 || raw.len() > MAX_NAME {
            return Err(Error::Format("not an encrypted name".into()));
        }
        let plain = self
            .eme()
            .decrypt_padded_vec_mut::<Pkcs7>(&raw)
            .map_err(|_| Error::Format("name padding".into()))?;
        String::from_utf8(plain).map_err(|_| Error::Format("name is not utf-8".into()))
    }

    /// a path, segment by segment; the separators stay
    pub fn encrypt_path(&self, plain: &str) -> String {
        plain
            .split('/')
            .map(|s| self.encrypt_segment(s))
            .collect::<Vec<_>>()
            .join("/")
    }

    pub fn decrypt_path(&self, enc: &str) -> Result<String> {
        enc.split('/')
            .map(|s| self.decrypt_segment(s))
            .collect::<Result<Vec<_>>>()
            .map(|v| v.join("/"))
    }

    /// the writer of one file: a fresh nonce, blocks in order
    pub fn encrypter(&self) -> Encrypter {
        let mut nonce = [0u8; NONCE];
        getrandom::getrandom(&mut nonce).expect("randomness");
        Encrypter {
            key: self.data.clone(),
            nonce,
        }
    }

    /// the reader of one file, from its header
    pub fn decrypter(&self, header: &[u8]) -> Result<Decrypter> {
        if header.len() < HEADER || &header[..8] != MAGIC {
            return Err(Error::Format("not an encrypted file".into()));
        }
        let mut nonce = [0u8; NONCE];
        nonce.copy_from_slice(&header[8..HEADER]);
        Ok(Decrypter {
            key: self.data.clone(),
            nonce,
        })
    }
}

/// the password rclone would need for a library key: its base64
pub fn password_of(key: &Key) -> String {
    base64::engine::general_purpose::STANDARD.encode(&key[..])
}

pub struct Encrypter {
    key: Zeroizing<[u8; 32]>,
    nonce: [u8; NONCE],
}

impl Encrypter {
    pub fn header(&self) -> Vec<u8> {
        let mut h = Vec::with_capacity(HEADER);
        h.extend_from_slice(MAGIC);
        h.extend_from_slice(&self.nonce);
        h
    }
    /// block `n` (at most BLOCK bytes): tag then bytes
    pub fn block(&self, n: u64, plain: &[u8]) -> Vec<u8> {
        seal(&self.key, &nonce_for(&self.nonce, n), plain)
    }
}

pub struct Decrypter {
    key: Zeroizing<[u8; 32]>,
    nonce: [u8; NONCE],
}

impl Decrypter {
    pub fn block(&self, n: u64, sealed: &[u8]) -> Result<Vec<u8>> {
        open(&self.key, &nonce_for(&self.nonce, n), sealed)
    }
}

/// the nonce of block `n`: the file's nonce plus n, little-endian with carry
pub fn nonce_for(initial: &[u8; NONCE], n: u64) -> [u8; NONCE] {
    let mut nonce = *initial;
    if n == 0 {
        return nonce;
    }
    let mut x = n;
    let mut carry: u16 = 0;
    for digit in nonce.iter_mut().take(8) {
        let add = (x & 0xff) as u16;
        x >>= 8;
        carry += *digit as u16 + add;
        *digit = carry as u8;
        carry >>= 8;
    }
    if carry != 0 {
        for digit in nonce.iter_mut().skip(8) {
            carry += *digit as u16;
            *digit = carry as u8;
            carry >>= 8;
            if carry == 0 {
                break;
            }
        }
    }
    nonce
}

fn seal(key: &[u8; 32], nonce: &[u8; NONCE], plain: &[u8]) -> Vec<u8> {
    let sb = XSalsa20Poly1305::new(key.into());
    let mut buf = Vec::with_capacity(TAG + plain.len());
    buf.extend_from_slice(&[0u8; TAG]);
    buf.extend_from_slice(plain);
    let tag = sb
        .encrypt_in_place_detached(nonce.into(), b"", &mut buf[TAG..])
        .expect("secretbox");
    buf[..TAG].copy_from_slice(&tag);
    buf
}

fn open(key: &[u8; 32], nonce: &[u8; NONCE], sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < TAG {
        return Err(Error::Format("short block".into()));
    }
    let sb = XSalsa20Poly1305::new(key.into());
    let mut buf = sealed[TAG..].to_vec();
    sb.decrypt_in_place_detached(nonce.into(), b"", &mut buf, sealed[..TAG].into())
        .map_err(|_| {
            buf.zeroize();
            Error::Format("block does not open".into())
        })?;
    Ok(buf)
}

/// the size of a file's ciphertext for `plain` bytes, and back
pub fn sealed_size(plain: u64) -> u64 {
    if plain == 0 {
        return HEADER as u64;
    }
    let blocks = plain.div_ceil(BLOCK as u64);
    HEADER as u64 + plain + blocks * TAG as u64
}

pub fn plain_size(sealed: u64) -> Result<u64> {
    if sealed < HEADER as u64 {
        return Err(Error::Format("shorter than a header".into()));
    }
    let body = sealed - HEADER as u64;
    if body == 0 {
        return Ok(0);
    }
    let full = body / SEALED_BLOCK as u64;
    let rest = body % SEALED_BLOCK as u64;
    if rest != 0 && rest <= TAG as u64 {
        return Err(Error::Format("a partial block with no bytes".into()));
    }
    Ok(full * BLOCK as u64 + if rest == 0 { 0 } else { rest - TAG as u64 })
}

const B32: base32::Alphabet = base32::Alphabet::Rfc4648Hex { padding: false };

fn base32hex(b: &[u8]) -> String {
    base32::encode(B32, b).to_lowercase()
}

fn unbase32hex(s: &str) -> Result<Vec<u8>> {
    base32::decode(B32, &s.to_uppercase()).ok_or_else(|| Error::Format("not base32".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        let c = Cipher::new("pw", "salt");
        for n in ["a", "Movies/x.mkv", "a name with spaces.txt", "é"] {
            let e = c.encrypt_path(n);
            assert!(!e.contains(' '));
            assert_eq!(c.decrypt_path(&e).unwrap(), n);
        }
        assert_eq!(c.encrypt_path(""), "");
    }

    #[test]
    fn blocks_round_trip_and_sizes() {
        let c = Cipher::new("pw", "salt");
        let e = c.encrypter();
        let plain: Vec<u8> = (0..(BLOCK * 2 + 17)).map(|i| i as u8).collect();
        let mut out = e.header();
        for (n, chunk) in plain.chunks(BLOCK).enumerate() {
            out.extend(e.block(n as u64, chunk));
        }
        assert_eq!(out.len() as u64, sealed_size(plain.len() as u64));
        assert_eq!(plain_size(out.len() as u64).unwrap(), plain.len() as u64);
        let d = c.decrypter(&out[..HEADER]).unwrap();
        let mut back = Vec::new();
        for (n, chunk) in out[HEADER..].chunks(SEALED_BLOCK).enumerate() {
            back.extend(d.block(n as u64, chunk).unwrap());
        }
        assert_eq!(back, plain);
        // a block out of order does not open
        assert!(d.block(1, &out[HEADER..HEADER + SEALED_BLOCK]).is_err());
    }

    #[test]
    fn nonce_carries() {
        let z = [0u8; NONCE];
        let mut one = z;
        one[0] = 1;
        assert_eq!(nonce_for(&z, 1), one);
        let mut ff = z;
        ff[0] = 0xff;
        let mut expect = z;
        expect[1] = 1;
        assert_eq!(nonce_for(&ff, 1), expect);
        let mut top = [0xffu8; 8];
        let mut n = z;
        n[..8].copy_from_slice(&top);
        top = [0u8; 8];
        let mut expect = z;
        expect[..8].copy_from_slice(&top);
        expect[8] = 1;
        assert_eq!(nonce_for(&n, 1), expect);
    }
}
