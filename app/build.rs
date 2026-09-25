fn main() {
    // the network engine, Go (app/net): a static archive on desktop, a
    // shared library on Android (Go makes no archives there), found where
    // the build environment says. On Android the same .so goes into the
    // apk (gen/android/.../jniLibs/<abi>/), which is where the
    // COMMONTY_NET_LIB_DIR for that target points.
    println!("cargo:rerun-if-env-changed=COMMONTY_NET_LIB_DIR");
    let target = std::env::var("TARGET").unwrap_or_default();
    let dir = std::env::var("COMMONTY_NET_LIB_DIR")
        .expect("COMMONTY_NET_LIB_DIR: where libcommontynet is (nix build .#net)");
    if target.contains("android") {
        let abi = if target.starts_with("aarch64") {
            "arm64-v8a"
        } else if target.starts_with("x86_64") {
            "x86_64"
        } else {
            panic!("no engine for {target}")
        };
        println!("cargo:rustc-link-search=native={dir}/{abi}");
        println!("cargo:rustc-link-lib=dylib=commontynet");
    } else {
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
    }
    tauri_build::build()
}
