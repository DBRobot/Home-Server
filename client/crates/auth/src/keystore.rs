use zeroize::Zeroizing;

/// Where the ente key lives between runs.
///
/// A trait rather than a concrete type because there is a second
/// implementation coming: a passkey's PRF/hmac-secret output can unlock the
/// same value without a password, and unlike the OS keyring it follows a
/// synced credential across devices instead of being tied to one machine.
pub trait KeyStore {
    fn get(&self, account: &str) -> crate::Result<Option<Zeroizing<String>>>;
    fn set(&self, account: &str, secret: &str) -> crate::Result<()>;
    fn clear(&self, account: &str) -> crate::Result<()>;
}

/// The OS credential store. Encrypted at rest by the OS and unlocked by the
/// user's normal login, so there is no key material of ours on disk.
pub struct OsKeyring {
    service: String,
}

impl OsKeyring {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

impl KeyStore for OsKeyring {
    fn get(&self, account: &str) -> crate::Result<Option<Zeroizing<String>>> {
        let entry = keyring::Entry::new(&self.service, account)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn set(&self, account: &str, secret: &str) -> crate::Result<()> {
        keyring::Entry::new(&self.service, account)?.set_password(secret)?;
        Ok(())
    }

    fn clear(&self, account: &str) -> crate::Result<()> {
        match keyring::Entry::new(&self.service, account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ignored by default: this talks to the real OS credential store, which
    /// does not exist on a CI runner. Run it with
    ///   cargo test -p auth -- --ignored
    /// on a desktop session.
    #[test]
    #[ignore]
    fn round_trips_through_the_os_store() {
        let ks = OsKeyring::new("distributed-datacenter-test");
        let account = "roundtrip";

        ks.clear(account).unwrap();
        assert!(ks.get(account).unwrap().is_none(), "should start empty");

        ks.set(account, "hello").unwrap();
        assert_eq!(
            ks.get(account).unwrap().as_deref().map(|s| s.to_string()),
            Some("hello".to_string())
        );

        ks.clear(account).unwrap();
        assert!(ks.get(account).unwrap().is_none(), "clear should remove it");
    }
}
