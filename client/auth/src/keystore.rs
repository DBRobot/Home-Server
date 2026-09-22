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

/// A json file, 0600, for tests and for machines with no credential store.
/// DD_KEYRING_FILE selects it. Not encrypted at rest: that is the trade.
pub struct FileStore {
    path: std::path::PathBuf,
}

impl FileStore {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }
    fn read(&self) -> crate::Result<std::collections::BTreeMap<String, String>> {
        match std::fs::read(&self.path) {
            Ok(b) => Ok(serde_json::from_slice(&b).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
            Err(e) => Err(e.into()),
        }
    }
    fn write(&self, m: &std::collections::BTreeMap<String, String>) -> crate::Result<()> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(m).unwrap_or_default())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

impl KeyStore for FileStore {
    fn get(&self, account: &str) -> crate::Result<Option<Zeroizing<String>>> {
        Ok(self.read()?.remove(account).map(Zeroizing::new))
    }
    fn set(&self, account: &str, secret: &str) -> crate::Result<()> {
        let mut m = self.read()?;
        m.insert(account.to_string(), secret.to_string());
        self.write(&m)
    }
    fn clear(&self, account: &str) -> crate::Result<()> {
        let mut m = self.read()?;
        m.remove(account);
        self.write(&m)
    }
}

/// The store a cli run uses: the file DD_KEYRING_FILE names, else the OS
/// credential store under `service`.
pub enum Store {
    Os(OsKeyring),
    File(FileStore),
}

pub fn open(service: &str) -> Store {
    match std::env::var("DD_KEYRING_FILE") {
        Ok(p) if !p.is_empty() => Store::File(FileStore::new(p)),
        _ => Store::Os(OsKeyring::new(service)),
    }
}

impl KeyStore for Store {
    fn get(&self, account: &str) -> crate::Result<Option<Zeroizing<String>>> {
        match self {
            Store::Os(s) => s.get(account),
            Store::File(s) => s.get(account),
        }
    }
    fn set(&self, account: &str, secret: &str) -> crate::Result<()> {
        match self {
            Store::Os(s) => s.set(account, secret),
            Store::File(s) => s.set(account, secret),
        }
    }
    fn clear(&self, account: &str) -> crate::Result<()> {
        match self {
            Store::Os(s) => s.clear(account),
            Store::File(s) => s.clear(account),
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
