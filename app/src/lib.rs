//! The Commonty app: the shell around the same crates `dd` is made of. This
//! process holds one device key in the OS keystore, reads the member's
//! entry from the directories, and shows its pages from `web/`. The pages
//! call the commands below; nothing else reaches them.

mod account;

/// the keystore this app keeps its device key in: its own, so an app beside
/// `dd` on one machine is a device of its own
const SERVICE: &str = "commonty-app";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(account::Keys(auth::open(SERVICE)))
        .invoke_handler(tauri::generate_handler![
            account::status,
            account::set_name,
            account::forget
        ])
        .run(tauri::generate_context!())
        .expect("the app window");
}
