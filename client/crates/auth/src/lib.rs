//! Kanidm login and the local key store.
//!
//! These are two separate tracks that happen to live in one crate because one
//! program uses both. Kanidm proves who you are to services. The ente key is
//! never derived from it - it comes from the ente password, is stored by the
//! OS, and kanidm never sees it.

pub mod keystore;
pub mod oidc;

pub use keystore::{KeyStore, OsKeyring};
pub use oidc::{Session, login};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("oidc discovery failed: {0}")]
    Discovery(String),
    #[error("oidc exchange failed: {0}")]
    Exchange(String),
    #[error("the browser redirect did not carry a code")]
    NoCode,
    #[error("state mismatch - the redirect did not come from the login we started")]
    StateMismatch,
    #[error("keystore: {0}")]
    Keystore(#[from] keyring::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
