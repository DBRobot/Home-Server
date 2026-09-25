//! Where this app keeps its things: its data directory as the platform
//! gives it (a folder under the home on desktop, the app's private
//! storage on a phone). Set once at start, read anywhere.

use std::path::PathBuf;
use std::sync::OnceLock;

#[cfg(target_os = "android")]
use tauri::Manager as _;

static DATA: OnceLock<PathBuf> = OnceLock::new();

pub fn init(app: &tauri::AppHandle) {
    // desktop: the folder dd's things sit next to; phone: what the
    // platform gives the app
    #[cfg(not(target_os = "android"))]
    let dir = std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".local/share/commonty"))
        .unwrap_or_else(|_| std::env::temp_dir().join("commonty"));
    #[cfg(target_os = "android")]
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("commonty"));
    #[cfg(not(target_os = "android"))]
    let _ = app;
    let _ = std::fs::create_dir_all(&dir);
    let _ = DATA.set(dir);
}

/// the app's data directory; before init, a temp folder
pub fn data() -> PathBuf {
    DATA.get()
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir().join("commonty"))
}
