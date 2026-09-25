//! Movies & TV in the app: the libraries mounted as folders on this
//! machine and Jellyfin started against them, in a window of its own.
//! Everything Jellyfin knows lives in this device's data dir; the box only
//! ever sees ciphertext.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::Mutex;

use crate::account::Keys;

pub struct Running {
    mount: media::fs::Mount,
    player: media::jellyfin::Player,
}

#[derive(Default)]
pub struct Media(pub Mutex<Option<Running>>);

#[derive(Serialize)]
pub struct MediaStatus {
    pub running: bool,
    pub at: Option<String>,
    pub url: Option<String>,
    pub libraries: usize,
    pub files: usize,
    pub jellyfin: Option<String>,
}

/// the jellyfin this app starts: named by the packaging, or on PATH
fn jellyfin_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("COMMONTY_JELLYFIN")
        && !p.is_empty()
    {
        return Some(PathBuf::from(p));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("jellyfin"))
        .find(|p| p.is_file())
}

fn home() -> Result<PathBuf, String> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "no HOME".to_string())
}

#[tauri::command]
pub async fn media_status(media: State<'_, Media>) -> Result<MediaStatus, String> {
    let m = media.0.lock().await;
    Ok(match m.as_ref() {
        Some(r) => MediaStatus {
            running: true,
            at: Some(r.mount.at.to_string_lossy().into_owned()),
            url: Some(r.player.url.clone()),
            libraries: r.mount.libraries.len(),
            files: r.mount.libraries.iter().map(|l| l.files).sum(),
            jellyfin: jellyfin_bin().map(|p| p.to_string_lossy().into_owned()),
        },
        None => MediaStatus {
            running: false,
            at: None,
            url: None,
            libraries: 0,
            files: 0,
            jellyfin: jellyfin_bin().map(|p| p.to_string_lossy().into_owned()),
        },
    })
}

#[tauri::command]
pub async fn media_open(
    app: AppHandle,
    keys: State<'_, Keys>,
    media: State<'_, Media>,
) -> Result<MediaStatus, String> {
    let mut m = media.0.lock().await;
    if m.is_none() {
        let bin = jellyfin_bin().ok_or("no jellyfin on this machine")?;
        let home = home()?;
        let at = home.join("Commonty");
        let data = crate::paths::data().join("jellyfin");
        let mount = media::fs::mount(&keys.0, &crate::account::dirs(), at)
            .await
            .map_err(|e| format!("{e:#}"))?;
        let (_, user, _) = media::gate::Opener::load(&keys.0).map_err(|e| e.to_string())?;
        let password = media::jellyfin::password(&keys.0).map_err(|e| e.to_string())?;
        let player = media::jellyfin::start(&bin, &data, &mount.at, &user, &password)
            .await
            .map_err(|e| format!("{e:#}"))?;
        *m = Some(Running { mount, player });
    }
    let (url, session) = m
        .as_ref()
        .map(|r| (r.player.url.clone(), r.player.session.clone()))
        .ok_or("no player")?;
    drop(m);
    show(&app, &url, &session)?;
    media_status(media).await
}

/// the player in its own window; a second open brings it to the front
fn show(app: &AppHandle, url: &str, session: &media::jellyfin::Session) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("player") {
        return w.set_focus().map_err(|e| e.to_string());
    }
    // jellyfin's web client reads its sign-in from local storage; the
    // window arrives signed in as the member, nobody types a password
    let creds = serde_json::json!({
        "Servers": [{
            "ManualAddress": url,
            "Id": session.server_id,
            "AccessToken": session.token,
            "UserId": session.user_id,
            "DateLastAccessed": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            "LastConnectionMode": 2
        }]
    });
    let init = format!(
        "try {{ localStorage.setItem('jellyfin_credentials', {}); localStorage.setItem('enableAutoLogin', 'true'); }} catch (e) {{}}",
        serde_json::to_string(&creds.to_string()).unwrap_or_default()
    );
    let url = tauri::Url::parse(url).map_err(|e| e.to_string())?;
    WebviewWindowBuilder::new(app, "player", WebviewUrl::External(url))
        .initialization_script(init)
        .title("Movies & TV")
        .inner_size(1100.0, 720.0)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn media_close(app: AppHandle, media: State<'_, Media>) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("player") {
        let _ = w.close();
    }
    let mut m = media.0.lock().await;
    *m = None;
    Ok(())
}

/// on exit: jellyfin stopped, the folders unmounted
pub fn stop_all(app: &AppHandle) {
    if let Some(media) = app.try_state::<Media>()
        && let Ok(mut m) = media.0.try_lock()
    {
        *m = None;
    }
}
