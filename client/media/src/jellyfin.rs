//! Jellyfin on the device, pointed at the mount. The box never runs it: it
//! reads files, and files are plain only here. First start finishes its
//! setup with the member's name and the mount as one library, so the
//! window that opens is a library, not a wizard.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

pub struct Player {
    pub url: String,
    pub data: PathBuf,
    /// what a web client needs to be signed in as the member
    pub session: Session,
    child: std::process::Child,
}

/// a signed-in session with jellyfin, for the window that shows it
#[derive(Clone, serde::Serialize)]
pub struct Session {
    pub server_id: String,
    pub user_id: String,
    pub token: String,
}

/// keyring account holding the password jellyfin knows the member by: it
/// refuses a user without one, and nobody types it
pub const PASSWORD: &str = "jellyfin-password";

pub fn password(keys: &impl auth::KeyStore) -> Result<String> {
    if let Some(p) = keys.get(PASSWORD)? {
        return Ok(p.to_string());
    }
    let p = library::random_id();
    keys.set(PASSWORD, &p)?;
    Ok(p)
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const PORT: u16 = 8096;

/// what the app says it is, to jellyfin's auth
const CLIENT: &str =
    r#"MediaBrowser Client="Commonty", Device="this device", DeviceId="commonty", Version="0.1""#;

pub async fn start(
    bin: &Path,
    data: &Path,
    mount: &Path,
    user: &str,
    password: &str,
) -> Result<Player> {
    std::fs::create_dir_all(data)?;
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("--datadir")
        .arg(data)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // if the app dies without saying goodbye, jellyfin goes with it
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt as _;
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    let child = cmd
        .spawn()
        .with_context(|| format!("starting {}", bin.display()))?;
    let url = format!("http://127.0.0.1:{PORT}");
    let mut child = child;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    // up to a minute for the first start; /health answers before the
    // server does, so it is the public info that says ready
    let mut info = None;
    for _ in 0..120 {
        if let Some(status) = child.try_wait()? {
            bail!("jellyfin exited on start: {status}");
        }
        if let Ok(r) = http.get(format!("{url}/System/Info/Public")).send().await
            && let Ok(v) = r.json::<serde_json::Value>().await
            && v.get("StartupWizardCompleted").is_some()
        {
            info = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let Some(info) = info else {
        let _ = child.kill();
        bail!("jellyfin did not come up on {url}");
    };
    let server_id = info["Id"].as_str().unwrap_or_default().to_string();
    let session = async {
        if info["StartupWizardCompleted"].as_bool() != Some(true) {
            set_up(&http, &url, user, password).await?;
        }
        let (token, user_id) = sign_in(&http, &url, user, password).await?;
        if info["StartupWizardCompleted"].as_bool() != Some(true) {
            add_library(&http, &url, &token, mount).await?;
        }
        Ok::<_, anyhow::Error>(Session {
            server_id,
            user_id,
            token,
        })
    }
    .await;
    let session = match session {
        Ok(s) => s,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    Ok(Player {
        url,
        data: data.to_path_buf(),
        session,
        child,
    })
}

fn ok(r: reqwest::Response, what: &str) -> Result<reqwest::Response> {
    if r.status().is_success() {
        Ok(r)
    } else {
        bail!("jellyfin, {what}: {}", r.status())
    }
}

/// the first-run wizard, answered: language, the member as the one user
async fn set_up(http: &reqwest::Client, url: &str, user: &str, password: &str) -> Result<()> {
    ok(
        http.post(format!("{url}/Startup/Configuration"))
            .json(&serde_json::json!({
                "UICulture": "en-US",
                "MetadataCountryCode": "US",
                "PreferredMetadataLanguage": "en"
            }))
            .send()
            .await?,
        "configuration",
    )?;
    // the user endpoint must be read once before it is written
    http.get(format!("{url}/Startup/User")).send().await?;
    ok(
        http.post(format!("{url}/Startup/User"))
            .json(&serde_json::json!({ "Name": user, "Password": password }))
            .send()
            .await?,
        "user",
    )?;
    ok(
        http.post(format!("{url}/Startup/RemoteAccess"))
            .json(&serde_json::json!({
                "EnableRemoteAccess": false,
                "EnableAutomaticPortMapping": false
            }))
            .send()
            .await?,
        "remote access",
    )?;
    ok(
        http.post(format!("{url}/Startup/Complete")).send().await?,
        "complete",
    )?;
    Ok(())
}

async fn sign_in(
    http: &reqwest::Client,
    url: &str,
    user: &str,
    password: &str,
) -> Result<(String, String)> {
    let auth: serde_json::Value = ok(
        http.post(format!("{url}/Users/AuthenticateByName"))
            .header("Authorization", CLIENT)
            .json(&serde_json::json!({ "Username": user, "Pw": password }))
            .send()
            .await?,
        "sign in (the player's data and this device's password disagree)",
    )?
    .json()
    .await?;
    let token = auth["AccessToken"]
        .as_str()
        .context("jellyfin gave no token")?
        .to_string();
    let user_id = auth["User"]["Id"]
        .as_str()
        .context("jellyfin gave no user id")?
        .to_string();
    Ok((token, user_id))
}

/// the mount as the one library, of everything
async fn add_library(http: &reqwest::Client, url: &str, token: &str, mount: &Path) -> Result<()> {
    ok(
        http.post(format!("{url}/Library/VirtualFolders"))
            .header("Authorization", format!("{CLIENT}, Token=\"{token}\""))
            .query(&[("name", "Commonty"), ("refreshLibrary", "true")])
            .json(&serde_json::json!({
                "LibraryOptions": {
                    "PathInfos": [{ "Path": mount.to_string_lossy() }],
                    "EnableRealtimeMonitor": false
                }
            }))
            .send()
            .await?,
        "library",
    )?;
    Ok(())
}
