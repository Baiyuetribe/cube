#[cfg(target_os = "windows")]
#[path = "src/app_icon/ico.rs"]
mod ico;

fn main() {
    // `CUBE` is not a reserved DOS device name (unlike the project's previous
    // `con` name), so there is exactly one binary target and no platform-
    // specific feature gating.
    //
    // `app_display_version()` captures `CUBE_RELEASE_VERSION` via
    // `option_env!` on non-macOS builds. Cargo does not track env reads
    // inside macros, so a cached build would pin a stale version string
    // across a tag change — declare the dep to force invalidation.
    println!("cargo:rerun-if-env-changed=CUBE_RELEASE_VERSION");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        cc::Build::new()
            .file("src/objc/sparkle_trampoline.m")
            .file("src/objc/global_hotkey_trampoline.m")
            .file("src/objc/quick_terminal_trampoline.m")
            .file("src/objc/ghostty_surface_trampoline.m")
            .flag("-fobjc-arc")
            .flag("-fmodules")
            .compile("cube_objc_trampolines");

        println!("cargo:rerun-if-changed=src/objc/sparkle_trampoline.m");
        println!("cargo:rerun-if-changed=src/objc/global_hotkey_trampoline.m");
        println!("cargo:rerun-if-changed=src/objc/quick_terminal_trampoline.m");
        println!("cargo:rerun-if-changed=src/objc/ghostty_surface_trampoline.m");
        println!("cargo:rustc-link-lib=framework=Carbon");
    }

    #[cfg(target_os = "windows")]
    {
        // Ship as `cube.ico`.
        let source = "../../assets/app-icons/cube-raccoon-tail-A6.png";
        println!("cargo:rerun-if-changed={source}");
        println!("cargo:rerun-if-changed=src/app_icon/ico.rs");
        let icon = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("cube.ico");
        let png = std::fs::read(source).expect("read default app icon");
        std::fs::write(&icon, ico::encode(&png).expect("encode default app icon"))
            .expect("write app icon resource");

        let mut res = winresource::WindowsResource::new();
        // Some locked-down hosts (and the GitHub Actions windows-2022
        // image on occasion) can't auto-discover rc.exe; let CI point
        // us at the toolkit explicitly.
        if let Ok(toolkit) = std::env::var("CUBE_RC_TOOLKIT_PATH") {
            res.set_toolkit_path(&toolkit);
        }
        res.set_icon(icon.to_str().expect("app icon output path"));
        res.set("FileDescription", "con");
        res.set("ProductName", "con");
        if let Err(e) = res.compile() {
            eprintln!("winresource failed to embed icon: {e}");
            std::process::exit(1);
        }
    }
}
