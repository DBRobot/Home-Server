//! The directories, as a client sees them: storage for signed entries and
//! nothing more. A directory that lies can withhold or replay, and asking
//! more than one is how that is caught. `dd` and the app share this.

use anyhow::{Result, anyhow, bail};
use identity::SignedEntry;

/// the directories a client asks unless told otherwise
pub const DEFAULT: [&str; 2] = [
    "https://files.commonty.org/_dd/directory",
    "http://100.95.10.10:4181/_dd/directory",
];

/// a proxy every client this process builds goes through: the app's way
/// into the fleet's own network (app/src/net.rs), a SOCKS5 one that
/// resolves names on its side (the network's own names). None: straight out.
#[derive(Clone)]
pub struct Proxy {
    /// host:port
    pub addr: String,
    pub user: String,
    pub password: String,
}

static VIA: std::sync::RwLock<Option<Proxy>> = std::sync::RwLock::new(None);

pub fn via(p: Option<Proxy>) {
    if let Ok(mut v) = VIA.write() {
        *v = p;
    }
}

pub fn proxy() -> Option<Proxy> {
    VIA.read().ok().and_then(|v| v.clone())
}

/// a client: no redirects, a short timeout, through the network's proxy
/// when this process is on it
pub fn http() -> Result<reqwest::Client> {
    builder()?
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(Into::into)
}

/// the same, for callers that set their own limits
pub fn builder() -> Result<reqwest::ClientBuilder> {
    let mut b = reqwest::Client::builder();
    if let Some(p) = proxy() {
        // socks5h: the name goes to the proxy unresolved
        let url = format!("socks5h://{}:{}@{}", p.user, p.password, p.addr);
        b = b.proxy(reqwest::Proxy::all(&url)?);
    }
    Ok(b)
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

/// every name a directory lists
pub async fn names(dir: &str) -> Result<Vec<String>> {
    let v: Vec<serde_json::Value> = http()?
        .get(dir)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(v.into_iter()
        .filter_map(|l| l["name"].as_str().map(str::to_string))
        .collect())
}

/// what one directory said to a publish
pub enum Took {
    Accepted,
    Refused(String),
    Unreachable(String),
}

/// Push to every directory, one verdict each. Every directory accepting is
/// success; a directory left behind is one that will later serve a stale
/// entry as current, so the caller treats anything less as an error.
pub async fn publish(dirs: &[String], signed: &SignedEntry) -> Result<Vec<(String, Took)>> {
    let http = http()?;
    let mut out = Vec::new();
    for d in dirs {
        let url = format!("{}/{}", d.trim_end_matches('/'), signed.entry.name);
        let took = match http.put(&url).json(signed).send().await {
            Ok(r) if r.status().is_success() => Took::Accepted,
            Ok(r) => {
                let s = r.status();
                Took::Refused(format!("{s} {}", r.text().await.unwrap_or_default().trim()))
            }
            Err(e) => Took::Unreachable(e.to_string()),
        };
        out.push((d.clone(), took));
    }
    Ok(out)
}

/// publish, and fail unless every directory took it
pub async fn publish_all(dirs: &[String], signed: &SignedEntry) -> Result<Vec<(String, Took)>> {
    let out = publish(dirs, signed).await?;
    let failed = out
        .iter()
        .filter(|(_, t)| !matches!(t, Took::Accepted))
        .count();
    if failed > 0 {
        bail!(
            "{failed} of {} directories did not take the update",
            dirs.len()
        );
    }
    Ok(out)
}
