//! Windows only: embed the app icon and version info (VERSIONINFO) into `gridcraft.exe`, so it
//! shows in Explorer, the taskbar, the Start menu and Alt-Tab.
//!
//! On every other target this does nothing but the channel work. A missing resource compiler is a
//! warning, so a cross-compile from macOS or Linux still links, unless `GRIDCRAFT_REQUIRE_WINRES=1`
//! turns it into an error (for release builds).
//!
//! The nightly channel is a build-time decision: `GRIDCRAFT_BUILD_CHANNEL=nightly-v1` sets
//! `cfg(gridcraft_nightly_v1)` and requires the prepared `GRIDCRAFT_VERSION` feed identity, which
//! becomes the executable's `ProductVersion` (the Launcher checks that, not `FileVersion`) and the
//! version the app answers `--version` with. Anything else than `stable`, empty or `nightly-v1`
//! is a build error, so a typo cannot silently ship the wrong channel.

#[path = "src/build_channel.rs"]
mod build_channel;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=GRIDCRAFT_BUILD_CHANNEL");
    println!("cargo:rustc-check-cfg=cfg(gridcraft_nightly_v1)");
    let nightly = match std::env::var("GRIDCRAFT_BUILD_CHANNEL").as_deref() {
        Ok("nightly-v1") => {
            println!("cargo:rustc-cfg=gridcraft_nightly_v1");
            true
        }
        // An empty value (an unset CI input) means the default stable build.
        Ok("stable" | "") | Err(std::env::VarError::NotPresent) => false,
        _ => {
            eprintln!("unsupported GRIDCRAFT_BUILD_CHANNEL (expected stable or nightly-v1)");
            std::process::exit(1);
        }
    };
    println!("cargo:rerun-if-changed=../../assets/app-icon/gridcraft.ico");
    println!("cargo:rerun-if-changed=../../assets/app-icon/gridcraft-nightly.ico");
    println!("cargo:rerun-if-env-changed=GRIDCRAFT_REQUIRE_WINRES");
    println!("cargo:rerun-if-env-changed=GRIDCRAFT_VERSION");
    let built_version = if nightly {
        match std::env::var("GRIDCRAFT_VERSION") {
            Ok(version) if build_channel::is_nightly_version(&version) => version,
            _ => {
                eprintln!("nightly-v1 requires an explicit GRIDCRAFT_VERSION feed identity");
                std::process::exit(1);
            }
        }
    } else {
        std::env::var("CARGO_PKG_VERSION").unwrap_or_default()
    };
    println!("cargo:rustc-env=GRIDCRAFT_BUILT_VERSION={built_version}");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(build_channel::windows_icon_path(nightly))
        .set("ProductName", if nightly { "GridCraft Nightly" } else { "GridCraft" })
        .set("FileDescription", if nightly { "GridCraft Nightly spreadsheet" } else { "GridCraft spreadsheet" })
        .set("CompanyName", "Learning Machines LLC")
        .set("LegalCopyright", "Copyright (c) the GridCraft authors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "gridcraft.exe")
        .set("InternalName", "gridcraft");
    if nightly {
        // The Launcher compares the PE ProductVersion with the exact feed identity.
        res.set("ProductVersion", &built_version);
    }
    if let Err(e) = res.compile() {
        if std::env::var_os("GRIDCRAFT_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=gridcraft.exe built without icon/version resources: {e}");
    }
}
