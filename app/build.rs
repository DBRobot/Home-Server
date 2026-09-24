fn main() {
    // the network engine: a static archive of Go (app/net), where the
    // build environment says it is
    println!("cargo:rerun-if-env-changed=COMMONTY_NET_LIB_DIR");
    let dir = std::env::var("COMMONTY_NET_LIB_DIR")
        .expect("COMMONTY_NET_LIB_DIR: where libcommontynet.a is (nix build .#net)");
    println!("cargo:rustc-link-search=native={dir}");
    println!("cargo:rustc-link-lib=static=commontynet");
    #[cfg(target_os = "linux")]
    {
        println!("cargo:rustc-link-lib=dylib=pthread");
        println!("cargo:rustc-link-lib=dylib=resolv");
    }
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=dylib=resolv");
    }
    tauri_build::build()
}
