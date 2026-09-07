//! Client library for a self-hosted ente instance.
//!
//! Nothing here does I/O of its own: no printing, no reading argv. Callers pass
//! data in and get data out, so the same code runs in a CLI, a browser via
//! wasm, or a phone.

/// Placeholder so the crate compiles and the wiring can be tested.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
