//! The keys a person holds: the device key that signs tokens, and the OS
//! credential store that keeps it and everything else the cli remembers.

pub mod device;
pub mod keystore;

pub use keystore::{FileStore, KeyStore, OsKeyring, Store, open};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Token(String),
    #[error("keystore: {0}")]
    Keystore(#[from] keyring::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
