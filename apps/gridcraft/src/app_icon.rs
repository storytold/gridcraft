//! The runtime window icon. Inside a macOS app bundle the Dock must show the bundle's own icon,
//! so GridCraft only sets a runtime icon outside one (same approach as PhotoCraft's `app_icon`).

/// Window, taskbar and (when running unbundled) Dock icon. macOS gets the padded render on
/// Apple's icon grid; elsewhere the 256 px hicolor render of the full tile.
pub fn window_icon() -> egui::IconData {
    #[cfg(target_os = "macos")]
    let bytes: &[u8] = include_bytes!("../../../assets/app-icon/gridcraft-macos-512.png");
    #[cfg(not(target_os = "macos"))]
    let bytes: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.gridcraft.png");
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let img = img.to_rgba8();
            egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
        }
        Err(e) => {
            log::warn!("app icon: {e}");
            egui::IconData::default()
        }
    }
}

/// Whether the window gets [`window_icon`] at runtime: everywhere except inside a macOS app
/// bundle. There the Dock shows the bundle's icon, which macOS renders in its own style
/// (Liquid Glass, dark and tinted) and which users can customize in Finder; a runtime PNG
/// would replace it while the app runs. `exe` is the running executable. The platform is a
/// parameter so tests cover the macOS case on every CI platform.
fn sets_runtime_icon_on(macos: bool, exe: Option<&std::path::Path>) -> bool {
    !(macos && exe.is_some_and(in_app_bundle))
}

/// Value for [`egui::ViewportBuilder::with_icon`]: [`window_icon`] when the window gets a
/// runtime icon; otherwise [`egui::IconData::default`], so eframe does not substitute its own
/// default glyph on the Dock after the window opens (storytold/photocraft#7253).
pub fn viewport_icon(exe: Option<&std::path::Path>) -> egui::IconData {
    viewport_icon_on(cfg!(target_os = "macos"), exe)
}

/// [`viewport_icon`] for the given platform; see [`sets_runtime_icon_on`].
fn viewport_icon_on(macos: bool, exe: Option<&std::path::Path>) -> egui::IconData {
    if sets_runtime_icon_on(macos, exe) { window_icon() } else { egui::IconData::default() }
}

/// Whether `exe` is an app bundle's executable: `…/Name.app/Contents/MacOS/binary`.
fn in_app_bundle(exe: &std::path::Path) -> bool {
    let name = |p: Option<&std::path::Path>| p.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
    let macos = exe.parent();
    let contents = macos.and_then(std::path::Path::parent);
    let bundle = contents.and_then(std::path::Path::parent);
    name(macos).as_deref() == Some("MacOS")
        && name(contents).as_deref() == Some("Contents")
        && name(bundle).is_some_and(|n| n.len() > 4 && n.to_ascii_lowercase().ends_with(".app"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Only an executable inside a macOS `.app` skips the runtime icon; unbundled runs and other
    /// platforms (Linux and Windows builds) still set it.
    #[test]
    fn only_a_macos_app_bundle_skips_the_runtime_icon() {
        assert!(in_app_bundle(Path::new("/Applications/GridCraft.app/Contents/MacOS/gridcraft")));
        assert!(in_app_bundle(Path::new("/Users/a/Downloads/GridCraft Beta.APP/Contents/MacOS/gridcraft")));
        assert!(!in_app_bundle(Path::new("/Users/a/src/gridcraft/target/release/gridcraft")));
        assert!(!in_app_bundle(Path::new("/opt/Contents/MacOS/gridcraft")), "not inside a .app");
        assert!(!in_app_bundle(Path::new("/.app/Contents/MacOS/gridcraft")), "a bare `.app` is no bundle name");
        assert!(!in_app_bundle(Path::new("gridcraft")));
        for macos in [true, false] {
            assert!(sets_runtime_icon_on(macos, None));
            assert!(sets_runtime_icon_on(macos, Some(Path::new("/usr/bin/gridcraft"))));
        }
        let bundled = Some(Path::new("/Applications/GridCraft.app/Contents/MacOS/gridcraft"));
        assert!(!sets_runtime_icon_on(true, bundled), "macOS keeps the bundle's icon");
        assert!(sets_runtime_icon_on(false, bundled), "other platforms always set the runtime icon");
    }

    /// Inside a bundle on macOS the viewport gets an empty `IconData`, so eframe neither sets a
    /// runtime icon nor paints its default glyph on the Dock (storytold/photocraft#7253). Other
    /// platforms get the full icon.
    #[test]
    fn viewport_icon_blocks_eframe_default_when_runtime_icon_is_skipped() {
        let bundled = Some(Path::new("/Applications/GridCraft.app/Contents/MacOS/gridcraft"));
        assert_eq!(viewport_icon_on(true, bundled), egui::IconData::default());
        let icon = viewport_icon_on(false, bundled);
        assert!(icon.width >= 256 && icon.width == icon.height);
    }

    /// The embedded PNG decodes to a square, non-blank icon of at least 256 px.
    #[test]
    fn window_icon_decodes() {
        let icon = window_icon();
        assert!(icon.width >= 256 && icon.width == icon.height);
        assert_eq!(icon.rgba.len(), icon.width as usize * icon.height as usize * 4);
        assert!(icon.rgba.iter().skip(3).step_by(4).any(|&alpha| alpha > 0), "the icon is not blank");
    }
}
