//! The repository key and what it does: encrypt packs, and be sealed to the
//! device keys of the people who may read them.

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use ed25519_dalek::{SigningKey, VerifyingKey};
use zeroize::Zeroizing;

pub type RepoKey = Zeroizing<[u8; 32]>;

pub fn new_repo_key() -> RepoKey {
    let mut k = Zeroizing::new([0u8; 32]);
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut *k);
    k
}

/// nonce || ciphertext. One message per pack; packs are at most a push.
pub fn encrypt(key: &RepoKey, plain: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(key.as_ref().into());
    let mut nonce = [0u8; 24];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut nonce);
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), plain)
        .map_err(|_| anyhow!("encrypt"))?;
    let mut out = nonce.to_vec();
    out.extend(ct);
    Ok(out)
}

pub fn decrypt(key: &RepoKey, blob: &[u8]) -> Result<Vec<u8>> {
    anyhow::ensure!(blob.len() > 24, "ciphertext too short");
    let cipher = XChaCha20Poly1305::new(key.as_ref().into());
    cipher
        .decrypt(XNonce::from_slice(&blob[..24]), &blob[24..])
        .map_err(|_| anyhow!("decrypt: wrong key or tampered pack"))
}

/// The device key as the two things it is here: an ed25519 signer, and the
/// same scalar as an x25519 secret for sealed boxes.
pub struct Device {
    pub signing: SigningKey,
}

impl Device {
    pub fn from_biscuit(kp: &biscuit_auth::KeyPair) -> Result<Self> {
        let bytes = Zeroizing::new(kp.private().to_bytes());
        let arr: [u8; 32] = bytes[..]
            .try_into()
            .map_err(|_| anyhow!("device key is not 32 bytes"))?;
        Ok(Self {
            signing: SigningKey::from_bytes(&arr),
        })
    }
    pub fn public_b64(&self) -> String {
        B64.encode(self.signing.verifying_key().to_bytes())
    }
    pub fn fingerprint(&self) -> String {
        identity::fingerprint(&self.public_b64())
    }
    fn box_secret(&self) -> crypto_box::SecretKey {
        crypto_box::SecretKey::from(self.signing.to_scalar_bytes())
    }
    pub fn sign(&self, msg: &[u8]) -> String {
        use ed25519_dalek::Signer as _;
        B64.encode(self.signing.sign(msg).to_bytes())
    }
    pub fn unseal(&self, sealed_b64: &str) -> Result<RepoKey> {
        let ct = B64.decode(sealed_b64).context("sealed key")?;
        let plain = self
            .box_secret()
            .unseal(&ct)
            .map_err(|_| anyhow!("this device cannot open the repository key"))?;
        let arr: [u8; 32] = plain[..]
            .try_into()
            .map_err(|_| anyhow!("repository key is not 32 bytes"))?;
        Ok(Zeroizing::new(arr))
    }
}

/// Seal the repository key to a device by its ed25519 public key.
pub fn seal_to(public_b64: &str, key: &RepoKey) -> Result<String> {
    let vk = identity::decode_public(public_b64).map_err(|e| anyhow!("{e}"))?;
    let pk = crypto_box::PublicKey::from(vk.to_montgomery().to_bytes());
    let ct = pk
        .seal(&mut rand_core::OsRng, key.as_ref())
        .map_err(|_| anyhow!("seal"))?;
    Ok(B64.encode(ct))
}

pub fn verify(public_b64: &str, msg: &[u8], sig_b64: &str) -> Result<()> {
    use ed25519_dalek::Verifier as _;
    let vk: VerifyingKey = identity::decode_public(public_b64).map_err(|e| anyhow!("{e}"))?;
    let sig = B64.decode(sig_b64).context("signature")?;
    let sig = ed25519_dalek::Signature::from_slice(&sig).context("signature")?;
    vk.verify(msg, &sig).map_err(|_| anyhow!("bad signature"))
}

pub fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(b))
}
