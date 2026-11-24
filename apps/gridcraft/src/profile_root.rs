//! GridCraft's nightly profile fences: no stable fallback, no migration and no change to the
//! process environment. Every stable place a GridCraft build keeps user state is refused as a
//! nightly root, in both directions, so the two channels can never share a file.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// The folder the Windows build writes its control-channel token to (`gridcraft_mcp`).
const WINDOWS_TOKEN_DIR: &str = ".gridcraft";
/// The subfolder of the profile the nightly control-channel token goes to, when one runs.
pub const CONTROL_DIR: &str = "Control";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Windows,
    Linux,
    Mac,
}

impl Platform {
    fn current() -> Result<Self, String> {
        match std::env::consts::OS {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Mac),
            // FreeBSD, the BSDs and anything else have no nightly channel.
            _ => Err("nightly-v1 is not defined for this operating system".into()),
        }
    }

    fn ignores_case(self) -> bool {
        self != Self::Linux
    }
}

fn absolute(path: PathBuf, label: &str) -> Result<PathBuf, String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("{label} must be absolute and contain no parent traversal"));
    }
    Ok(path)
}

fn required_base(env: &impl Fn(&str) -> Option<OsString>, key: &str) -> Result<PathBuf, String> {
    let value = env(key).filter(|v| !v.is_empty()).ok_or_else(|| format!("cannot establish the stable {key} boundary"))?;
    absolute(PathBuf::from(value), key)
}

fn xdg_base(env: &impl Fn(&str) -> Option<OsString>, key: &str, fallback: &str) -> Result<PathBuf, String> {
    match env(key).filter(|v| !v.is_empty()) {
        Some(value) => absolute(PathBuf::from(value), key),
        None => Ok(required_base(env, "HOME")?.join(fallback)),
    }
}

/// Resolve existing ancestors before creating anything. A dangling link or an inaccessible
/// ancestor is not a missing directory that may be replaced or followed later.
fn canonical_future(path: &Path) -> Result<PathBuf, String> {
    let mut parent = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(parent) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(parent.file_name().ok_or("the profile root has no existing ancestor")?.to_os_string());
                parent = parent.parent().ok_or("the profile root has no existing ancestor")?;
            }
            Err(error) => return Err(format!("cannot inspect the profile boundary: {error}")),
        }
    }
    let mut resolved = std::fs::canonicalize(parent).map_err(|e| format!("cannot resolve the profile boundary: {e}"))?;
    for part in missing.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn overlaps(a: &Path, b: &Path, platform: Platform) -> bool {
    let parts = |p: &Path| {
        p.components()
            .map(|c| {
                let value = c.as_os_str().to_string_lossy().into_owned();
                if platform.ignores_case() { value.to_lowercase() } else { value }
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (parts(a), parts(b));
    a.starts_with(&b) || b.starts_with(&a)
}

fn installation_boundary(platform: Platform, exe: &Path) -> Result<PathBuf, String> {
    let parent = exe.parent().ok_or("cannot establish the installation boundary")?;
    if platform == Platform::Mac {
        // A profile in Resources is still inside the versioned install, not beside MacOS.
        if let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app"))) {
            return Ok(bundle.to_path_buf());
        }
    }
    Ok(parent.to_path_buf())
}

fn forbidden_roots(platform: Platform, env: &impl Fn(&str) -> Option<OsString>, exe: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = match platform {
        Platform::Windows => {
            // `apps/gridcraft/src/main.rs` (ui.json, prefs.json and logs) and `gridcraft_mcp`'s
            // token folder, plus both MSI install folders (`install-policy.wxi`).
            let mut roots = ["APPDATA", "LOCALAPPDATA"]
                .into_iter()
                .map(|key| required_base(env, key).map(|p| p.join("GridCraft")))
                .collect::<Result<Vec<_>, _>>()?;
            roots.push(required_base(env, "LOCALAPPDATA")?.join("Programs").join("ArtCraft").join("GridCraft"));
            roots.push(required_base(env, "USERPROFILE")?.join(WINDOWS_TOKEN_DIR));
            for key in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
                if let Some(base) = env(key).filter(|v| !v.is_empty()) {
                    roots.push(absolute(PathBuf::from(base), key)?.join("ArtCraft").join("GridCraft"));
                }
            }
            roots
        }
        Platform::Linux => {
            let mut roots =
                [("XDG_CONFIG_HOME", ".config"), ("XDG_DATA_HOME", ".local/share"), ("XDG_CACHE_HOME", ".cache"), ("XDG_STATE_HOME", ".local/state")]
                    .into_iter()
                    .map(|(key, fallback)| xdg_base(env, key, fallback).map(|p| p.join("gridcraft")))
                    .collect::<Result<Vec<_>, _>>()?;
            roots.extend(["/usr/bin/gridcraft", "/usr/local/bin/gridcraft", "/opt/gridcraft"].map(PathBuf::from));
            // The stable Flatpak keeps its data in its own per-app folder.
            roots.push(required_base(env, "HOME")?.join(".var/app/ai.storyteller.gridcraft"));
            // `gridcraft_mcp`'s token folder when no XDG runtime directory is set.
            roots.push(required_base(env, "HOME")?.join(".cache").join("gridcraft"));
            roots
        }
        Platform::Mac => {
            let home = required_base(env, "HOME")?;
            let library = home.join("Library");
            vec![
                library.join("Application Support/GridCraft"),
                library.join("Caches/GridCraft"),
                library.join("Caches/ai.storyteller.gridcraft"),
                library.join("Preferences/GridCraft"),
                library.join("Preferences/ai.storyteller.gridcraft.plist"),
                library.join("Saved Application State/ai.storyteller.gridcraft.savedState"),
                home.join("Applications/GridCraft.app"),
                PathBuf::from("/Applications/GridCraft.app"),
            ]
        }
    };
    roots.push(installation_boundary(platform, exe)?);
    roots.into_iter().map(|p| canonical_future(&p)).collect()
}

fn private_subdirectory(root: &Path, name: impl AsRef<Path>, label: &str) -> Result<PathBuf, String> {
    let chosen = canonical_future(&absolute(root.to_path_buf(), "the nightly profile root")?)?;
    let child = canonical_future(&chosen.join(name))?;
    if child == chosen || !child.starts_with(&chosen) {
        return Err(format!("the nightly {label} must stay inside its selected profile"));
    }
    Ok(child)
}

/// Every existing entry directly in the profile (settings, logs and so on) must resolve inside it,
/// so a link there cannot point Nightly's writes at the stable app's data.
fn entries_stay_inside(root: &Path) -> Result<(), String> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot inspect the nightly profile: {error}")),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot inspect the nightly profile: {e}"))?;
        private_subdirectory(root, entry.file_name(), "profile entry")?;
    }
    Ok(())
}

/// Where a nightly control-channel token goes: `<profile>/Control`, created lazily by
/// `gridcraft_mcp` if a control server is ever started. Resolving it never creates anything.
pub fn control_directory(root: &Path) -> Result<PathBuf, String> {
    private_subdirectory(root, CONTROL_DIR, "control folder")
}

/// The app's own exclusive writable probe: create a file inside the profile, then remove it.
pub fn ensure_writable(root: &Path) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let probe = root.join(format!(".gridcraft-nightly-writable-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let result = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map(drop);
    let _ = std::fs::remove_file(&probe);
    result
}

/// The writable hook is injected so filesystem boundary tests exercise the real policy without
/// starting the app or changing global state.
pub fn validate(
    root: &Path,
    env: impl Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    writable: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<PathBuf, String> {
    validate_for(Platform::current()?, root, env, exe, writable)
}

fn validate_for(
    platform: Platform,
    root: &Path,
    env: impl Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    writable: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<PathBuf, String> {
    absolute(root.to_path_buf(), "the nightly profile root")?;
    if env("GRIDCRAFT_CONFIG_DIR").is_some_and(|v| !v.is_empty()) {
        // The distro wrappers may point a stable build at its own data folder; a nightly launch
        // takes its root from the Launcher only, so there is never a second candidate.
        return Err("nightly-v1 cannot be combined with GRIDCRAFT_CONFIG_DIR".into());
    }
    if platform != Platform::Windows
        && let Some(id) = env("FLATPAK_ID").filter(|value| !value.is_empty())
        && id.to_string_lossy().as_ref() != gridcraft_ui_egui::channel::NIGHTLY_PACKAGE_ID
    {
        // `packaging/linux/flatpak-bundle.sh` builds GridCraft Nightly as its own Flatpak
        // (`ai.storyteller.gridcraft.nightly`), whose sandbox is already a private home, so that one
        // instance may carry a profile. Any other, the stable app's included, could point it at
        // another installation's tree; these rows are otherwise native tar/app-bundle launches.
        return Err(format!(
            "nightly-v1 can only run inside its own Flatpak instance ({}), not {}",
            gridcraft_ui_egui::channel::NIGHTLY_PACKAGE_ID,
            id.to_string_lossy()
        ));
    }
    let chosen = canonical_future(root)?;
    let exe = absolute(exe.ok_or("cannot establish the installation boundary")?.to_path_buf(), "the executable")?;
    let forbidden = forbidden_roots(platform, &env, &exe)?;
    if forbidden.iter().any(|stable| overlaps(&chosen, stable, platform)) {
        return Err("the nightly root must not overlap stable data or an installation".into());
    }
    // Existing aliases under the profile must not redirect a subfolder elsewhere.
    control_directory(&chosen)?;
    entries_stay_inside(&chosen)?;
    let mut create = std::fs::DirBuilder::new();
    create.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        create.mode(0o700);
    }
    create.create(&chosen).map_err(|e| format!("cannot create the nightly profile: {e}"))?;
    let confirmed = std::fs::canonicalize(&chosen).map_err(|e| format!("cannot resolve the created nightly profile: {e}"))?;
    if confirmed != chosen || forbidden.iter().any(|stable| overlaps(&confirmed, stable, platform)) {
        return Err("the nightly profile changed while it was being validated".into());
    }
    control_directory(&confirmed)?;
    entries_stay_inside(&confirmed)?;
    writable(&confirmed).map_err(|e| format!("the nightly profile is not writable: {e}"))?;
    Ok(confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        root: PathBuf,
        env: BTreeMap<String, OsString>,
        exe: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = loop {
                let candidate = std::env::temp_dir().join(format!("gridcraft-profile-policy-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
                match std::fs::create_dir(&candidate) {
                    Ok(()) => break std::fs::canonicalize(candidate).unwrap(),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create owned fixture: {error}"),
                }
            };
            let home = root.join("home");
            let exe = root.join("install/gridcraft");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, b"fixture executable, never run").unwrap();
            let env = [
                ("HOME", home.clone()),
                ("USERPROFILE", home),
                ("APPDATA", root.join("roaming")),
                ("LOCALAPPDATA", root.join("local")),
            ]
            .into_iter()
            .map(|(key, value)| (key.into(), value.into_os_string()))
            .collect();
            Self { root, env, exe }
        }

        fn get(&self, key: &str) -> Option<OsString> {
            self.env.get(key).cloned()
        }

        fn check(&self, platform: Platform, root: &Path) -> Result<PathBuf, String> {
            self.check_with(platform, root, |_| Ok(()))
        }

        fn check_with(&self, platform: Platform, root: &Path, writable: impl FnOnce(&Path) -> std::io::Result<()>) -> Result<PathBuf, String> {
            validate_for(platform, root, |key| self.get(key), Some(&self.exe), writable)
        }

        fn home(&self) -> PathBuf {
            PathBuf::from(self.get("HOME").unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // Only the directory successfully created by this fixture is owned for removal.
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn every_stable_data_root_is_refused_on_its_own_platform() {
        let mut fixture = Fixture::new();
        // Windows: APPDATA\GridCraft (settings and logs), LOCALAPPDATA\GridCraft, the per-user
        // and per-machine install folders and the control-token folder.
        for key in ["APPDATA", "LOCALAPPDATA"] {
            let stable = PathBuf::from(fixture.get(key).unwrap()).join("GridCraft");
            for candidate in [stable.clone(), stable.join("nightly")] {
                assert!(fixture.check(Platform::Windows, &candidate).unwrap_err().contains("overlap"), "{key}");
            }
            assert!(!stable.exists());
        }
        assert!(fixture.check(Platform::Windows, &PathBuf::from(fixture.get("LOCALAPPDATA").unwrap()).join("Programs/ArtCraft/GridCraft/nightly")).is_err());
        assert!(fixture.check(Platform::Windows, &fixture.home().join(".gridcraft/nightly")).is_err());
        for key in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
            let base = fixture.root.join(format!("machine {key}"));
            fixture.env.insert(key.into(), base.clone().into_os_string());
            assert!(fixture.check(Platform::Windows, &base.join("ArtCraft/GridCraft/nightly")).is_err(), "{key}");
            assert!(!base.join("ArtCraft/GridCraft").exists());
        }
        // Linux: the four XDG folders, then the stable Flatpak row and the token folder.
        for key in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"] {
            fixture.env.insert(key.into(), fixture.root.join(key).into_os_string());
        }
        for key in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"] {
            let stable = PathBuf::from(fixture.get(key).unwrap()).join("gridcraft");
            std::fs::create_dir_all(&stable).unwrap();
            std::fs::write(stable.join("stable-sentinel"), b"stable bytes").unwrap();
            for candidate in [stable.clone(), stable.join("nightly"), stable.parent().unwrap().to_path_buf()] {
                assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("overlap"), "{key}");
            }
            assert_eq!(std::fs::read(stable.join("stable-sentinel")).unwrap(), b"stable bytes");
        }
        assert!(fixture.check(Platform::Linux, &fixture.home().join(".var/app/ai.storyteller.gridcraft/data")).is_err());
        assert!(fixture.check(Platform::Linux, &fixture.home().join(".var/app/ai.storyteller.launcher/nightly")).is_ok());
        assert!(fixture.check(Platform::Linux, Path::new("/usr/bin/gridcraft/../gridcraft-nightly")).is_err(), "parent traversal is refused");
        // macOS: the four Library rows and the bundle itself.
        for suffix in ["Application Support/GridCraft", "Caches/GridCraft", "Preferences/GridCraft", "Saved Application State/ai.storyteller.gridcraft.savedState"] {
            assert!(fixture.check(Platform::Mac, &fixture.home().join("Library").join(suffix).join("nightly")).is_err(), "{suffix}");
        }
    }

    #[test]
    fn the_launcher_profile_row_is_accepted_on_every_platform() {
        let fixture = Fixture::new();
        let data = fixture.root.join("Launcher/nightly/profiles/gridcraft");
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            assert_eq!(fixture.check(platform, &data).unwrap(), data);
        }
        assert!(fixture.check(Platform::Linux, &PathBuf::from(fixture.get("LOCALAPPDATA").unwrap()).join("Programs/ArtCraft/ArtCraftLauncher/nightly/gridcraft")).is_ok(), "the Launcher's own folder is not GridCraft's install");
    }

    #[test]
    fn overlap_check_handles_ancestor_descendant_and_prefix_lookalikes() {
        let fixture = Fixture::new();
        let root = fixture.home().join("nightly/profiles/gridcraft");
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            assert!(overlaps(&root, &root.join("Control"), platform));
            assert!(overlaps(&root.join("Control"), &root, platform));
            assert!(!overlaps(&root, &root.with_file_name("gridcraft-other"), platform));
        }
    }

    #[test]
    fn the_install_folder_of_the_running_build_is_refused() {
        let fixture = Fixture::new();
        for platform in [Platform::Windows, Platform::Linux, Platform::Mac] {
            assert!(fixture.check(platform, &fixture.exe.parent().unwrap().join("profile")).is_err());
        }
    }

    #[test]
    fn mac_fences_the_whole_bundle_not_just_its_macos_folder() {
        let mut fixture = Fixture::new();
        fixture.exe = fixture.root.join("install/GridCraft Nightly.app/Contents/MacOS/GridCraft");
        std::fs::create_dir_all(fixture.exe.parent().unwrap()).unwrap();
        std::fs::write(&fixture.exe, b"fixture executable, never run").unwrap();
        let inside_resources = fixture.exe.parent().unwrap().parent().unwrap().join("Resources/profile");
        assert!(fixture.check(Platform::Mac, &inside_resources).unwrap_err().contains("overlap"));
        assert!(!inside_resources.exists());
        assert!(fixture.check(Platform::Mac, &fixture.home().join("Library/Application Support/ArtCraft/nightly/profiles/gridcraft")).is_ok());
    }

    #[test]
    fn ambiguous_environment_and_non_native_sandbox_are_refused_before_creation() {
        let mut fixture = Fixture::new();
        let candidate = fixture.home().join("nightly/profiles/gridcraft");
        fixture.env.insert("XDG_CACHE_HOME".into(), "relative/cache".into());
        assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("absolute"));
        fixture.env.remove("XDG_CACHE_HOME");
        fixture.env.insert("GRIDCRAFT_CONFIG_DIR".into(), fixture.home().join("custom-stable").into_os_string());
        assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("GRIDCRAFT_CONFIG_DIR"));
        fixture.env.remove("GRIDCRAFT_CONFIG_DIR");
        fixture.env.insert("FLATPAK_ID".into(), "ai.storyteller.gridcraft".into());
        assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("Flatpak"));
        fixture.env.remove("FLATPAK_ID");
        fixture.env.remove("HOME");
        assert!(fixture.check(Platform::Linux, &candidate).unwrap_err().contains("HOME"));
        assert!(!candidate.exists());
    }

    #[test]
    fn only_the_nightly_s_own_flatpak_instance_may_carry_a_profile() {
        let mut fixture = Fixture::new();
        let root = fixture.root.join("flatpak profile");
        for (id, accepted) in [
            (gridcraft_ui_egui::channel::NIGHTLY_PACKAGE_ID, true),
            // The stable Flatpak, and a lookalike that only shares the prefix, stay refused.
            ("ai.storyteller.gridcraft", false),
            ("ai.storyteller.gridcraft.nightly-v1", false),
            ("io.github.other.Gridcraft", false),
        ] {
            fixture.env.insert("FLATPAK_ID".into(), id.into());
            let result = fixture.check(Platform::Linux, &root);
            assert_eq!(result.is_ok(), accepted, "{id}");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn failed_write_probe_never_falls_back_to_stable() {
        let fixture = Fixture::new();
        let candidate = fixture.home().join("nightly/profiles/gridcraft");
        let result = fixture.check_with(Platform::Linux, &candidate, |_| Err(std::io::Error::other("read-only mount")));
        assert!(result.unwrap_err().contains("not writable"));
        assert!(candidate.is_dir(), "a created profile is not rolled back onto stable data");
        assert!(!fixture.home().join(".config/gridcraft").exists());
    }

    #[test]
    fn a_file_in_place_of_the_profile_is_refused() {
        let fixture = Fixture::new();
        let candidate = fixture.root.join("not-a-directory");
        std::fs::write(&candidate, b"a file, not a profile").unwrap();
        assert!(fixture.check(Platform::Linux, &candidate).is_err());
        assert_eq!(std::fs::read(&candidate).unwrap(), b"a file, not a profile");
    }

    #[cfg(unix)]
    #[test]
    fn profile_entry_aliases_must_stay_inside_the_profile() {
        let fixture = Fixture::new();
        let stable = fixture.home().join(".config/gridcraft");
        std::fs::create_dir_all(&stable).unwrap();
        std::fs::write(stable.join("ui.json"), b"stable settings").unwrap();
        for name in ["ui.json", "logs", "Control", "profile"] {
            let private = fixture.root.join(format!("nightly-{name}"));
            std::fs::create_dir(&private).unwrap();
            std::os::unix::fs::symlink(&stable, private.join(name)).unwrap();
            let probed = std::cell::Cell::new(false);
            let result = fixture.check_with(Platform::Linux, &private, |_| {
                probed.set(true);
                Ok(())
            });
            assert!(result.is_err(), "{name} must not lead outside the profile");
            assert!(!probed.get());
        }
        let private = fixture.root.join("nightly-inside");
        std::fs::create_dir_all(private.join("real-logs")).unwrap();
        std::fs::write(private.join("ui.json"), b"{}").unwrap();
        std::os::unix::fs::symlink(private.join("real-logs"), private.join("logs")).unwrap();
        assert!(fixture.check(Platform::Linux, &private).is_ok(), "links that stay inside are fine");
        assert_eq!(std::fs::read(stable.join("ui.json")).unwrap(), b"stable settings");
        assert_eq!(std::fs::read_dir(&stable).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_control_folder_alias_cannot_redirect_the_token_outside_the_profile() {
        let fixture = Fixture::new();
        let outside = fixture.home().join("shared-tokens");
        std::fs::create_dir_all(&outside).unwrap();
        let private = fixture.root.join("nightly-control");
        std::fs::create_dir(&private).unwrap();
        std::os::unix::fs::symlink(&outside, private.join(CONTROL_DIR)).unwrap();
        assert!(control_directory(&private).is_err());
        assert!(fixture.check(Platform::Linux, &private).is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[test]
    fn the_control_folder_is_a_private_lazy_subfolder() {
        let fixture = Fixture::new();
        let private = fixture.check(Platform::Linux, &fixture.home().join("nightly/profile")).unwrap();
        let control = control_directory(&private).unwrap();
        assert!(control.starts_with(&private) && control != private);
        assert!(!control.exists(), "resolving it must not create it");
        assert_eq!(control.file_name().unwrap(), CONTROL_DIR);
    }

    #[test]
    fn the_writable_probe_leaves_nothing_behind() {
        let fixture = Fixture::new();
        let private = fixture.check(Platform::Linux, &fixture.home().join("nightly/profile")).unwrap();
        ensure_writable(&private).unwrap();
        assert_eq!(std::fs::read_dir(&private).unwrap().count(), 0);
    }
}
