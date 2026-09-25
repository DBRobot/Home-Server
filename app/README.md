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

Network: the fleet's own network (Headscale on the gateway box). Join asks
the gate for a single-use key in your name and hands it to the engine,
Tailscale's tsnet as a C archive (`app/net`, the one Go in the tree, built
by `nix build .#net` and linked by `build.rs` from `COMMONTY_NET_LIB_DIR`).
From then on every request the app makes goes through the engine's
loopback proxy, by the box's name, straight to the box; state lives in
`~/.local/share/commonty/net` and is resumed at the next start.

Sign-up: "New here? I have an invite code". The app fetches the invite the
code derives, makes the root and recovery keys on this device, publishes
the entry with its grant (`client/account`, shared with `dd identity new
--code`), shows the recovery key once, and joins the network.

Devices: the device that made the account (it holds the root) adds another
by pasting the key from that device's "Add this device" page, and removes
one; every library key follows. "Lost every device?" takes the recovery
key, installs a new root on this device, signs every old device out and
shows a new recovery key.

The engine's proxy is our own SOCKS5 (`app/net/socks.go`, `socks5h` from
the Rust side): its dialer resolves names through the network's DNS first,
where the fleet's names live. `COMMONTY_NET_DEBUG=1` turns the engine's
log on and prints the proxy credential for `curl -x socks5h://tsnet:…`.
