//! The Commonty app: the shell around the same crates `dd` is made of. This
//! process holds one device key in the OS keystore, reads the member's
//! entry from the directories, and shows its pages from `web/`. The pages
//! call the commands below; nothing else reaches them.

mod account;
mod media;
mod net;

/// the keystore this app keeps its device key in: its own, so an app beside
/// `dd` on one machine is a device of its own
const SERVICE: &str = "commonty-app";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(account::Keys(auth::open(SERVICE)))
        .manage(media::Media::default())
        .invoke_handler(tauri::generate_handler![
            account::status,
            account::set_name,
            account::sign_up,
            account::admit_device,
            account::remove_device,
            account::recover,
            account::forget,
            media::media_status,
            media::media_open,
            media::media_close,
            net::net_status,
            net::net_join
        ])
        .setup(|_app| {
            // on the network from the start, if this device has joined before
            net::resume(&crate::account::control_url());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the app window")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                media::stop_all(app);
                net::stop();
            }
        });
}
