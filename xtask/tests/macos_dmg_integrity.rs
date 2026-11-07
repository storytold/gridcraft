#![cfg(unix)]

use std::process::{Command, Output};

const SCRIPT: &str = include_str!("../../packaging/macos/package.sh");

fn section(start: &str, end: &str) -> &'static str {
    let offset = SCRIPT.find(start).expect("packaging section start");
    let rest = &SCRIPT[offset..];
    &rest[..rest.find(end).expect("packaging section end")]
}

fn bash(script: &str) -> Output {
    let path = std::env::var_os("PATH").unwrap_or_default();
    Command::new("bash").env_clear().env("PATH", path).args(["-euc", script]).output().unwrap()
}

#[test]
fn assembled_bundle_metadata_is_cleared_before_any_signature() {
    let script = section("xattr -cr \"$APP\"", "\nif [[ \"$NOTARIZE\" = 1 ]]");
    let out = bash(&format!(
        r#"
APP='Synthetic App.app'; HERE='Synthetic packaging'; cleared=0
xattr() {{ [[ "$#" = 2 && "$1" = -cr && "$2" = "$APP" ]]; cleared=1; printf 'CLEAR\n'; }}
sign() {{ [[ "$cleared" = 1 ]]; printf 'SIGN\n'; }}
codesign() {{ [[ "$cleared" = 1 ]]; printf 'VERIFY\n'; }}
{script}
"#
    ));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "CLEAR\nSIGN\nSIGN\nVERIFY\n");
}

fn run(fail_at: &str, failures: u8) -> Output {
    let setup = r#"
export PATH=/usr/bin:/bin
exec 3>&1
WORK='Synthetic work'; STAGE='Synthetic stage'; DMG='Synthetic image.dmg'; NOTARIZE=1
create_calls=0
log() { printf '%s' "$1" >&3; shift; printf '|%s' "$@" >&3; printf '\n' >&3; }
warn() { printf '%s\n' "$*" >&2; }
sleep() { log PAUSE "$1"; }
mkdir() { :; }
rm() { log CLEAN "$@"; }
xattr() { :; }
sign() { log SIGN "$@"; }
notarize() { log NOTARIZE "$@"; }
stapler_retry() { log STAPLE "$@"; }
staple() { log STAPLE "$@"; }
hdiutil() {
  log HDI "$@"
  case "$1" in
    create)
      [[ "$#" = 13 && "$2" = -srcfolder && "$3" = "$STAGE" && "$4" = -volname && "$5" = GridCraft && "$6" = -fs && "$7" = HFS+ && "$8" = -format && "$9" = UDZO && "${10}" = -imagekey && "${11}" = zlib-level=9 && "${12}" = -ov && "${13}" = "$DMG" ]] || return 71
      create_calls=$((create_calls + 1))
      [[ "$create_calls" -gt "$FAILURES" ]] || return 1
      ;;
    attach)
      [[ "$#" = 7 && "$2" = -readonly && "$3" = -nobrowse && "$4" = -noautoopen && "$5" = -mountpoint && "$6" = "$WORK/dmg-check" && "$7" = "$DMG" ]] || return 72
      ;;
    detach) [[ "$2" = "$WORK/dmg-check" ]] || return 73 ;;
    *) return 74 ;;
  esac
}
codesign() {
  local last; for last in "$@"; do :; done
  log VERIFY "$last"
  [[ "$FAIL_AT" != mounted || "$last" != "$WORK/dmg-check/GridCraft.app" ]] &&
    [[ "$FAIL_AT" != copied || "$last" != "$WORK/dmg-check-copy/GridCraft.app" ]]
}
ditto() {
  log COPY "$@"
  [[ "$1" = "$WORK/dmg-check/GridCraft.app" && "$2" = "$WORK/dmg-check-copy/GridCraft.app" && "$FAIL_AT" != ditto ]]
}
spctl() {
  local last; for last in "$@"; do :; done
  log ASSESS "$last"
  [[ "$FAIL_AT" != assessment ]]
}
"#;
    let body = section("hdiutil_retry() {", "\n# ---- CLI");
    let path = std::env::var_os("PATH").unwrap_or_default();
    Command::new("bash")
        .env_clear()
        .env("PATH", path)
        .env("FAIL_AT", fail_at)
        .env("FAILURES", failures.to_string())
        .args(["-euc", &format!("{setup}\n{body}")])
        .output()
        .unwrap()
}

#[test]
fn image_copies_staged_metadata_and_gates_both_mounted_and_installed_app() {
    let out = run("none", 0);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    let mut previous = 0;
    for marker in [
        "HDI|create|-srcfolder|Synthetic stage",
        "SIGN|Synthetic image.dmg",
        "HDI|attach|-readonly|-nobrowse|-noautoopen",
        "VERIFY|Synthetic work/dmg-check/GridCraft.app",
        "COPY|Synthetic work/dmg-check/GridCraft.app|Synthetic work/dmg-check-copy/GridCraft.app",
        "VERIFY|Synthetic work/dmg-check-copy/GridCraft.app",
        "ASSESS|Synthetic work/dmg-check-copy/GridCraft.app",
        "HDI|detach|Synthetic work/dmg-check",
        "NOTARIZE|Synthetic image.dmg",
    ] {
        let next = text.find(marker).unwrap_or_else(|| panic!("missing {marker}: {text}"));
        assert!(next >= previous, "wrong order: {text}");
        previous = next;
    }
    assert!(!text.contains("makehybrid"));
}

#[test]
fn damaged_apps_are_detached_and_stop_before_notarizing_the_container() {
    for fail in ["mounted", "copied"] {
        let out = run(fail, 0);
        assert!(!out.status.success(), "{fail}");
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("HDI|detach|Synthetic work/dmg-check"), "{text}");
        assert!(!text.contains("NOTARIZE|"), "{text}");
    }
}

#[test]
fn copy_or_gatekeeper_failure_still_detaches_and_stops_the_release() {
    for fail in ["ditto", "assessment"] {
        let out = run(fail, 0);
        assert!(!out.status.success(), "{fail}");
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("HDI|detach|Synthetic work/dmg-check"), "{text}");
        assert!(!text.contains("NOTARIZE|"), "{text}");
    }
}

#[test]
fn image_creation_has_bounded_retries_and_never_accepts_a_persistent_failure() {
    let out = run("none", 2);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.matches("HDI|create|").count(), 3);
    assert!(text.contains("PAUSE|10") && text.contains("PAUSE|20"));
    let out = run("none", 5);
    assert!(!out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.matches("HDI|create|").count(), 5);
    assert!(!text.contains("SIGN|"));
    assert!(!text.contains("NOTARIZE|"));
}
