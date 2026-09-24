//! Who this device is: its key, the name it signs for, and what the
//! directories say about that name. Signed in means the entry under the
//! name lists this device; the laptop's `dd device admit` puts it there.

use std::time::Duration;

use auth::KeyStore as _;
use serde::Serialize;
use tauri::State;

/// keyring account holding the name this device signs for
const USER: &str = "user";

pub struct Keys(pub auth::Store);

#[derive(Serialize)]
pub struct DeviceView {
    pub fingerprint: String,
    pub added: u64,
    pub this: bool,
}

#[derive(Serialize)]
pub struct EntryView {
    pub version: u64,
    pub updated: u64,
    pub devices: Vec<DeviceView>,
    pub passkeys: usize,
    pub libraries: usize,
}

#[derive(Serialize)]
pub struct Status {
    pub name: Option<String>,
    pub fingerprint: String,
    pub public_key: String,
    /// what each directory answered, for the page to say which is behind
    pub directories: Vec<(String, String)>,
    pub entry: Option<EntryView>,
    pub admitted: bool,
    /// the gate's answer to a token signed by this device, once admitted
    pub gate: Option<String>,
    /// this device holds the root: it made the account, it can admit others
    pub root: bool,
}

/// the directories this app asks: the gateway's, by name. A phone talks to
/// one directory; asking several is `dd`'s job. (The default list also
/// names a box by its address on the owner's tailnet, which means nothing
/// from the fleet's own network.)
pub fn dirs() -> Vec<String> {
    match std::env::var("COMMONTY_DIRECTORY") {
        Ok(s) if !s.is_empty() => s.split_whitespace().map(str::to_string).collect(),
        _ => directory::DEFAULT
            .iter()
            .filter(|d| d.starts_with("https://"))
            .map(|s| s.to_string())
            .collect(),
    }
}

/// the network's control server: on the gate's host
pub fn control_url() -> String {
    format!(
        "https://headscale.{}",
        gate_base()
            .trim_start_matches("https://")
            .trim_start_matches("files.")
    )
}

fn gate_base() -> String {
    dirs()
        .first()
        .map(|d| d.trim_end_matches("/_dd/directory").to_string())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn status(keys: State<'_, Keys>) -> Result<Status, String> {
    let (kp, _fresh) = auth::device::load_or_create(&keys.0).map_err(|e| e.to_string())?;
    let public_key = auth::device::public_b64(&kp);
    let fingerprint = identity::fingerprint(&public_key);
    let name = keys
        .0
        .get(USER)
        .map_err(|e| e.to_string())?
        .map(|s| s.to_string());
    let mut st = Status {
        name: name.clone(),
        fingerprint: fingerprint.clone(),
        public_key,
        directories: Vec::new(),
        entry: None,
        admitted: false,
        gate: None,
        root: keys
            .0
            .get(account::ROOT)
            .map(|r| r.is_some())
            .unwrap_or(false),
    };
    let Some(name) = name else { return Ok(st) };
    let found = directory::fetch(&dirs(), &name).await;
    for (d, r) in &found {
        st.directories.push((
            d.clone(),
            match r {
                Ok(Some(e)) => format!("version {}", e.entry.version),
                Ok(None) => "no such name".to_string(),
                Err(e) => format!("unreachable: {e:#}"),
            },
        ));
    }
    let Some(signed) = directory::newest(&found) else {
        return Ok(st);
    };
    if let Err(e) = identity::verify(&signed) {
        return Err(format!("the entry for {name} does not verify: {e}"));
    }
    let e = &signed.entry;
    st.admitted = e.devices.iter().any(|d| d.fingerprint == fingerprint);
    st.entry = Some(EntryView {
        version: e.version,
        updated: e.updated,
        devices: e
            .devices
            .iter()
            .map(|d| DeviceView {
                fingerprint: d.fingerprint.clone(),
                added: d.added,
                this: d.fingerprint == fingerprint,
            })
            .collect(),
        passkeys: e.passkeys.len(),
        libraries: e.libraries.len(),
    });
    if st.admitted {
        // a token this device signs, shown to the gate: the proof that
        // admission means something to a box, not only to a page
        st.gate = Some(match check_gate(&kp, &name, e).await {
            Ok(s) => s,
            Err(e) => format!("refused: {e}"),
        });
    }
    Ok(st)
}

async fn check_gate(
    kp: &auth::device::KeyPair,
    name: &str,
    entry: &identity::Entry,
) -> anyhow::Result<String> {
    let token = auth::device::mint(kp, name, Duration::from_secs(300))?;
    // the first library's record list is the cheapest gated thing there is;
    // without a library, a library that is nobody's: the gate answers 403
    // to a token it accepts and 401 to one it does not
    let (url, own) = match entry.libraries.first() {
        Some(l) => (
            format!("{}/_dd/library/{}/records", gate_base(), l.id),
            true,
        ),
        None => (
            format!("{}/_dd/library/{}/records", gate_base(), "0".repeat(32)),
            false,
        ),
    };
    let r = directory::http()?
        .get(&url)
        .bearer_auth(token)
        .send()
        .await?;
    let s = r.status();
    if s.is_success() || (!own && s == reqwest::StatusCode::FORBIDDEN) {
        Ok("accepted this device's token".to_string())
    } else {
        anyhow::bail!("{s}")
    }
}

#[tauri::command]
pub fn set_name(keys: State<'_, Keys>, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if !identity::valid_name(&name) {
        return Err("a name is lowercase letters, digits and dashes".to_string());
    }
    keys.0.set(USER, &name).map_err(|e| e.to_string())
}

/// A new member, from a code: the invite it derives is fetched from the
/// directory, a root and a recovery key are made here, the entry with its
/// grant is published, the root goes into the keystore. The recovery key
/// comes back once, for the page to show and never keep.
#[tauri::command]
pub async fn sign_up(keys: State<'_, Keys>, name: String, code: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if !identity::valid_name(&name) {
        return Err("a name is lowercase letters, digits and dashes".to_string());
    }
    if code.trim().is_empty() {
        return Err("the invite code is what lets you in".to_string());
    }
    let (kp, _) = auth::device::load_or_create(&keys.0).map_err(|e| e.to_string())?;
    let recovery = account::create(&keys.0, &dirs(), &name, &kp, Some(code.trim()))
        .await
        .map_err(|e| format!("{e:#}"))?;
    keys.0.set(USER, &name).map_err(|e| e.to_string())?;
    Ok(recovery.to_string())
}

/// leave: the name goes, the device key stays (a key is cheap to keep and
/// costly to re-admit)
#[tauri::command]
pub fn forget(keys: State<'_, Keys>) -> Result<(), String> {
    keys.0.clear(USER).map_err(|e| e.to_string())
}
