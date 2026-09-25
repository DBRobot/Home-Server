//! Media on a device: the gate to a library (gate), the libraries as
//! folders (fs), and Jellyfin pointed at them (jellyfin). Shared by `dd`
//! and the app.

pub mod gate;
pub mod jellyfin;
#[cfg(not(target_os = "android"))]
pub mod mount;
