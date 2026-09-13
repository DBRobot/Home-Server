//! The person's own identity: a root key here, a recovery key on paper, and
//! the entry they sign with it, published to every directory they name.
//!
//! No server is asked for anything but storage. A directory that lies can
//! withhold or replay, and publishing to more than one is how that is caught.

use anyhow::{Context, Result, anyhow, bail};
use auth::KeyStore;
use identity::{Device, Entry, SignedEntry, Signer_};

/// keyring account holding the root secret, base64
pub const ROOT: &str = "identity-root";

pub fn load_root(keys: &impl KeyStore) -> Result<Option<ed25519_dalek::SigningKey>> {
    match keys.get(ROOT)? {
        Some(s) => Ok(Some(identity::decode_secret(&s)?)),
        None => Ok(None),
    }
}

fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()?)
}

/// What each directory has for `name`; a directory that cannot be reached is
/// an error entry, not a missing one, because the two mean different things.
pub async fn fetch(dirs: &[String], name: &str) -> Vec<(String, Result<Option<SignedEntry>>)> {
    let mut out = Vec::new();
    let http = match http() {
        Ok(h) => h,
        Err(e) => return vec![(String::new(), Err(e))],
    };
    for d in dirs {
        let url = format!("{}/{name}", d.trim_end_matches('/'));
        let r = async {
            let r = http.get(&url).send().await?;
            match r.status().as_u16() {
                404 => Ok(None),
                200 => Ok(Some(r.json::<SignedEntry>().await?)),
                s => Err(anyhow!("{s} {}", r.text().await.unwrap_or_default())),
            }
        }
        .await;
        out.push((d.clone(), r));
    }
    out
}

/// The newest entry any directory holds, checked against nothing yet: the
/// caller decides what it must match (its own root, or a recovery key).
pub fn newest(found: &[(String, Result<Option<SignedEntry>>)]) -> Option<SignedEntry> {
    found
        .iter()
        .filter_map(|(_, r)| r.as_ref().ok().and_then(|o| o.clone()))
        .max_by_key(|e| e.entry.version)
}

/// Push to every directory. Success is every directory accepting; a partial
/// result is reported line by line and still an error, because a directory
/// left behind is one that will later serve a stale entry as current.
pub async fn publish(dirs: &[String], signed: &SignedEntry) -> Result<()> {
    let http = http()?;
    let mut failed = 0;
    for d in dirs {
        let url = format!("{}/{}", d.trim_end_matches('/'), signed.entry.name);
        match http.put(&url).json(signed).send().await {
            Ok(r) if r.status().is_success() => {
                println!("  {d}: accepted version {}", signed.entry.version)
            }
            Ok(r) => {
                failed += 1;
                let s = r.status();
                println!(
                    "  {d}: refused ({s} {})",
                    r.text().await.unwrap_or_default().trim()
                );
            }
            Err(e) => {
                failed += 1;
                println!("  {d}: unreachable ({e})");
            }
        }
    }
    if failed > 0 {
        bail!(
            "{failed} of {} directories did not take the update",
            dirs.len()
        );
    }
    Ok(())
}

pub fn device_of(kp: &biscuit_auth::KeyPair) -> Device {
    let public_key = auth::device::public_b64(kp);
    Device {
        fingerprint: identity::fingerprint(&public_key),
        public_key,
        added: identity::now(),
    }
}

/// A brand new identity: root and recovery generated here, this device the
/// first and only one, version 1, signed by the root. Returns the recovery
/// secret exactly once, for paper.
pub async fn create(
    keys: &impl KeyStore,
    dirs: &[String],
    name: &str,
    kp: &biscuit_auth::KeyPair,
) -> Result<zeroize::Zeroizing<String>> {
    if keys.get(ROOT)?.is_some() {
        bail!("this machine already holds an identity - `dd identity show`");
    }
    if !identity::valid_name(name) {
        bail!("{name:?} is not a valid name: lowercase letters, digits, - _ .");
    }
    let found = fetch(dirs, name).await;
    for (d, r) in &found {
        match r {
            Ok(Some(_)) => bail!(
                "{d} already has an entry for {name}. If it is yours, `dd identity import` \
                 the root here or `dd identity recover` with the paper key."
            ),
            Ok(None) => {}
            Err(e) => bail!("{d}: {e} - not creating an identity blind"),
        }
    }
    let root = identity::generate();
    let recovery = identity::generate();
    let entry = Entry {
        name: name.to_string(),
        root: identity::encode_public(&root.verifying_key()),
        recovery: identity::encode_public(&recovery.verifying_key()),
        devices: vec![device_of(kp)],
        passkeys: vec![],
        version: 1,
        updated: identity::now(),
    };
    let signed = identity::sign(entry, &root, Signer_::Root)?;
    publish(dirs, &signed).await?;
    keys.set(ROOT, &identity::encode_secret(&root))?;
    Ok(identity::encode_secret(&recovery))
}

/// The entry as the directories have it, insisting it is ours: signed by the
/// root this machine holds. A directory serving somebody else's entry under
/// our name, or a forgery, fails here rather than getting a signature from us.
pub async fn ours(
    dirs: &[String],
    name: &str,
    root: &ed25519_dalek::SigningKey,
) -> Result<SignedEntry> {
    let found = fetch(dirs, name).await;
    let cur = newest(&found).with_context(|| {
        let why: Vec<String> = found
            .iter()
            .map(|(d, r)| match r {
                Ok(None) => format!("{d}: no entry"),
                Ok(Some(_)) => unreachable!(),
                Err(e) => format!("{d}: {e}"),
            })
            .collect();
        format!(
            "no directory has an entry for {name}:\n  {}",
            why.join("\n  ")
        )
    })?;
    let mine = identity::encode_public(&root.verifying_key());
    if cur.entry.root != mine {
        bail!(
            "the entry for {name} is under a different root ({}) than this machine's ({})",
            identity::fingerprint(&cur.entry.root),
            identity::fingerprint(&mine)
        );
    }
    if cur.signer == Signer_::Root {
        identity::verify(&cur, &root.verifying_key())
            .context("the entry under our root does not carry our signature")?;
    }
    Ok(cur)
}

/// Add a device by its public key, signed by the root here.
pub async fn admit(
    dirs: &[String],
    name: &str,
    root: &ed25519_dalek::SigningKey,
    public_key: &str,
) -> Result<SignedEntry> {
    identity::decode_public(public_key).context("that is not an ed25519 public key")?;
    let cur = ours(dirs, name, root).await?;
    let mut entry = cur.entry.clone();
    if entry.devices.iter().any(|d| d.public_key == public_key) {
        bail!("that device is already in the entry");
    }
    entry.devices.push(Device {
        fingerprint: identity::fingerprint(public_key),
        public_key: public_key.to_string(),
        added: identity::now(),
    });
    entry.version += 1;
    entry.updated = identity::now();
    let signed = identity::sign(entry, root, Signer_::Root)?;
    publish(dirs, &signed).await?;
    Ok(signed)
}

/// Recovery: the paper key installs a new root and a new recovery key, and
/// the device list starts over with this device alone. Everything a lost or
/// stolen device could sign is dead the moment a directory takes this.
pub async fn recover(
    keys: &impl KeyStore,
    dirs: &[String],
    name: &str,
    recovery_secret: &str,
    kp: &biscuit_auth::KeyPair,
) -> Result<zeroize::Zeroizing<String>> {
    let recovery = identity::decode_secret(recovery_secret).context("bad recovery key")?;
    let found = fetch(dirs, name).await;
    let cur = newest(&found).with_context(|| format!("no directory has an entry for {name}"))?;
    if cur.entry.recovery != identity::encode_public(&recovery.verifying_key()) {
        bail!("that recovery key does not match the entry for {name}");
    }
    let root = identity::generate();
    let next_recovery = identity::generate();
    let entry = Entry {
        name: name.to_string(),
        root: identity::encode_public(&root.verifying_key()),
        recovery: identity::encode_public(&next_recovery.verifying_key()),
        devices: vec![device_of(kp)],
        passkeys: vec![],
        version: cur.entry.version + 1,
        updated: identity::now(),
    };
    let signed = identity::sign(entry, &recovery, Signer_::Recovery)?;
    publish(dirs, &signed).await?;
    keys.set(ROOT, &identity::encode_secret(&root))?;
    Ok(identity::encode_secret(&next_recovery))
}

pub fn print_recovery(recovery: &str) {
    println!("\n  RECOVERY KEY - write this down, on paper, now.");
    println!("  It is the only way back in if every device is lost or stolen.");
    println!("  It is not stored anywhere, and nobody can issue another.\n");
    println!("    {recovery}\n");
}

/// Sign a passkey into the entry. The box that ran the browser ceremony hands
/// the credential to `dd enrol`; nothing on that box could add it itself.
pub async fn admit_passkey(
    dirs: &[String],
    name: &str,
    root: &ed25519_dalek::SigningKey,
    pk: identity::Passkey,
) -> Result<SignedEntry> {
    let cur = ours(dirs, name, root).await?;
    let mut entry = cur.entry.clone();
    if entry.passkeys.iter().any(|p| p.id == pk.id) {
        bail!("that passkey is already in the entry");
    }
    entry.passkeys.push(pk);
    entry.version += 1;
    entry.updated = identity::now();
    let signed = identity::sign(entry, root, Signer_::Root)?;
    publish(dirs, &signed).await?;
    Ok(signed)
}

pub async fn remove_passkey(
    dirs: &[String],
    name: &str,
    root: &ed25519_dalek::SigningKey,
    id: &str,
) -> Result<SignedEntry> {
    let cur = ours(dirs, name, root).await?;
    let mut entry = cur.entry.clone();
    let before = entry.passkeys.len();
    entry.passkeys.retain(|p| p.id != id);
    if entry.passkeys.len() == before {
        bail!("no passkey {id} in the entry");
    }
    entry.version += 1;
    entry.updated = identity::now();
    let signed = identity::sign(entry, root, Signer_::Root)?;
    publish(dirs, &signed).await?;
    Ok(signed)
}
