# The Commonty app

The same crates as `dd`, behind a window: Tauri 2, Rust core, plain pages in
`web/`. The device key lives in the OS keystore under the service
`commonty-app`, so the app beside `dd` on one machine is a device of its own.

Build and run from the dev shell:

    cargo build -p commonty && target/debug/commonty

On Ubuntu with nix's toolchain the webview cannot find the host's GL driver;
point it at nix's mesa:

    M=$(nix build --no-link --print-out-paths nixpkgs#mesa)
    __EGL_VENDOR_LIBRARY_FILENAMES=$M/share/glvnd/egl_vendor.d/50_mesa.json \
    LIBGL_DRIVERS_PATH=$M/lib/dri LIBGL_ALWAYS_SOFTWARE=1 target/debug/commonty

`nix build .#app` makes the wrapped binary.

Milestone 1: sign in. Type a name, the app shows `dd device admit <key>`,
run that on a machine that is already yours, and the app is a device in
your entry with every library key sealed to it.

Milestone 2: Movies & TV. Open mounts every library you can open at
`~/Commonty` (client/media, FUSE) and starts Jellyfin on this machine
against it, with its data in `~/.local/share/commonty/jellyfin`, then shows
it in a window of its own, signed in. First start answers Jellyfin's setup:
you as its one user (a random password kept in the keystore, account
`jellyfin-password`) and the mount as one library. The packaged app brings
its own Jellyfin (`COMMONTY_JELLYFIN`); a dev build takes the one on PATH.
Jellyfin dies with the app; a mount left by a crash is cleared on the next
open.
