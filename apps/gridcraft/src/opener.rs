//! How a link is handed to the operating system's opener (#2176).
//!
//! The opener is spawned by absolute path where the platform has a fixed one: a bare program name
//! on Windows is searched for in the application's own directory (and the current directory)
//! before System32 and the Windows directory, so an `explorer.exe` planted next to a portable
//! GridCraft would run with the user's session. The target never becomes an option of the opener,
//! and on Windows the characters Explorer's own command-line parser treats specially are
//! percent-encoded. Pure functions, so the tests run on every OS.

use std::path::PathBuf;

/// Whose opener to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    /// Linux, the BSDs and other freedesktop systems: `xdg-open`.
    Other,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Other
        }
    }
}

/// Used when neither `SystemRoot` nor `windir` holds a usable path.
const DEFAULT_WINDOWS_DIR: &str = "C:\\Windows";

/// The Windows directory: `SystemRoot`, else `windir`, else `C:\Windows`. A value is used only
/// when it is a local drive path (`X:\…`) without `..` segments; relative or network values
/// would bring back the search this exists to avoid.
pub fn windows_dir(system_root: Option<&str>, windir: Option<&str>) -> String {
    [system_root, windir]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|d| is_local_drive_path(d))
        .map_or(DEFAULT_WINDOWS_DIR, |d| d.trim_end_matches(['\\', '/']))
        .to_string()
}

/// `X:\…` or `X:/…` (not the drive-relative `X:` or `X:dir`), with no `..` segment and no quote.
fn is_local_drive_path(d: &str) -> bool {
    let mut chars = d.chars();
    let drive = chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.next() == Some(':');
    let rest = chars.as_str();
    let rooted = rest.starts_with(['\\', '/']);
    drive && rooted && !d.contains('"') && !rest.split(['\\', '/']).any(|seg| seg == "..")
}

/// Percent-encodes the characters Explorer's command-line parser (comma-separated switches such
/// as `/select,`), the C runtime's quoting rules and `cmd` treat specially: space, `"`, `,` and
/// `^`. None of them may appear unencoded in a URL, so the address is unchanged in meaning.
pub fn escape_for_explorer(target: &str) -> String {
    let mut out = String::with_capacity(target.len());
    for c in target.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '"' => out.push_str("%22"),
            ',' => out.push_str("%2C"),
            '^' => out.push_str("%5E"),
            _ => out.push(c),
        }
    }
    out
}

/// The program and its single argument that open `target` (already accepted by
/// `links::classify`) on `os`. `None` when the target could be read as an option: it starts with
/// `-` or `/`, or is empty. `xdg-open` takes no `--`, so refusing is the only safe answer.
pub fn opener_command(os: Os, target: &str, system_root: Option<&str>, windir: Option<&str>) -> Option<(PathBuf, String)> {
    if target.is_empty() || target.starts_with(['-', '/']) {
        return None;
    }
    Some(match os {
        Os::Mac => (PathBuf::from("/usr/bin/open"), target.to_string()),
        Os::Windows => {
            let exe = format!("{}\\explorer.exe", windows_dir(system_root, windir));
            (PathBuf::from(exe), escape_for_explorer(target))
        }
        // Found on PATH: its location differs between distributions and the BSDs.
        Os::Other => (PathBuf::from("xdg-open"), target.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_opener_is_explorer_in_the_windows_directory() {
        let (exe, arg) = opener_command(Os::Windows, "https://a.example/x", Some("C:\\Windows"), None).unwrap_or_default();
        assert_eq!(exe, PathBuf::from("C:\\Windows\\explorer.exe"));
        assert_eq!(arg, "https://a.example/x");
        // Never a bare name, whatever the environment says.
        for (root, windir) in [(None, None), (Some(""), Some("")), (Some("Windows"), None), (Some(".\\"), Some("explorer"))] {
            let (exe, _) = opener_command(Os::Windows, "https://a.example", root, windir).unwrap_or_default();
            assert_eq!(exe, PathBuf::from("C:\\Windows\\explorer.exe"), "{root:?} {windir:?}");
        }
    }

    #[test]
    fn windows_dir_prefers_system_root_then_windir_then_default() {
        assert_eq!(windows_dir(Some("D:\\WINNT\\"), Some("E:\\Win")), "D:\\WINNT");
        assert_eq!(windows_dir(None, Some("E:\\Win")), "E:\\Win");
        assert_eq!(windows_dir(Some("relative\\dir"), Some("e:/win/")), "e:/win");
        assert_eq!(windows_dir(None, None), "C:\\Windows");
        assert_eq!(windows_dir(Some("D:\\"), None), "D:");
        for bad in ["\\\\host\\share", "//host/share", "C:", "C:Windows", "C:\\Windows\\..\\Users\\x", "C:\\\"x", "", "  ", "1:\\x"] {
            assert_eq!(windows_dir(Some(bad), None), "C:\\Windows", "{bad:?}");
        }
    }

    #[test]
    fn explorer_switch_characters_are_percent_encoded() {
        let (_, arg) = opener_command(Os::Windows, "https://x/,/select,C:\\Windows\\a b\"^", None, None).unwrap_or_default();
        assert_eq!(arg, "https://x/%2C/select%2CC:\\Windows\\a%20b%22%5E");
        assert!(!arg.contains([' ', '"', ',', '^']));
        assert_eq!(escape_for_explorer("mailto:a@b.example?subject=Hi there"), "mailto:a@b.example?subject=Hi%20there");
        // Other platforms get the address as it is.
        let (_, arg) = opener_command(Os::Other, "https://x/a,b", None, None).unwrap_or_default();
        assert_eq!(arg, "https://x/a,b");
    }

    #[test]
    fn mac_and_freedesktop_openers() {
        let (exe, arg) = opener_command(Os::Mac, "https://a.example", None, None).unwrap_or_default();
        assert_eq!((exe, arg.as_str()), (PathBuf::from("/usr/bin/open"), "https://a.example"));
        let (exe, _) = opener_command(Os::Other, "mailto:a@b.example", None, None).unwrap_or_default();
        assert_eq!(exe, PathBuf::from("xdg-open"));
    }

    #[test]
    fn a_target_never_becomes_an_option() {
        for os in [Os::Mac, Os::Windows, Os::Other] {
            for bad in ["-a", "--help", "-e calc", "/select,C:\\x", "/root,C:\\", ""] {
                assert_eq!(opener_command(os, bad, None, None), None, "{os:?} {bad:?}");
            }
        }
    }
}
