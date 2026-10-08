#!/usr/bin/env bash
# Regression test for packaging/linux/apprun.sh (issue #3: the AppImage showed the generic
# Wayland placeholder icon). Builds a throwaway AppDir, runs AppRun's desktop-integration step
# against a throwaway HOME/XDG_DATA_HOME, and asserts:
#   * the launcher entry is installed and its Exec/TryExec point at the AppImage by absolute path;
#   * the hicolor icon is installed under the XDG data home so a bare Icon=$APP_ID resolves;
#   * a second run changes nothing (idempotent);
#   * XDG_DATA_DIRS ends up including the AppDir's own share dir.
# The stub "gridcraft" binary stands in for the real one, so no Rust build is needed.
#
# Usage: packaging/linux/apprun-selftest.sh   (run by CI: .github/workflows/packaging-lint.yml)
set -euo pipefail

HERE="$(cd -- "$(dirname -- "$0")" && pwd)"
ROOT="$(cd -- "$HERE/../.." && pwd)"
APP_ID=ai.storyteller.gridcraft

WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT

fails=0
ok() { echo "ok - $1"; }
fail() { echo "not ok - $1" >&2; fails=$((fails + 1)); }

# ---- a minimal AppDir -----------------------------------------------------------------------
APPDIR="$WORK/GridCraft.AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/icons/hicolor/256x256/apps" "$APPDIR/usr/share/applications"
# Stub app: records the args it was called with and the share dir it can see, so we can prove
# AppRun execs it, forwards arguments, and exports XDG_DATA_DIRS.
cat >"$APPDIR/usr/bin/gridcraft" <<'EOF'
#!/bin/sh
echo "gridcraft-ran $*"
echo "gridcraft-data-dirs $XDG_DATA_DIRS"
EOF
chmod +x "$APPDIR/usr/bin/gridcraft"
cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
cp "$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/$APP_ID.png"
install -m755 "$HERE/apprun.sh" "$APPDIR/AppRun"

# ---- an icon that is deliberately a different byte than the one in the tree -----------------
# (catches a bug where the wrong size/file is copied).
printf 'icon-256' >"$APPDIR/usr/share/icons/hicolor/256x256/apps/$APP_ID.png"

# ---- fake home + a fake AppImage file -------------------------------------------------------
export HOME="$WORK/home"
export XDG_DATA_HOME="$HOME/.local/share"
mkdir -p "$HOME"
# A space in the path exercises the desktop-entry Exec/TryExec quoting (spec: escape \ " ` $ and
# wrap in double quotes only when needed); a literal % exercises the Exec field-code doubling.
APPIMAGE="$WORK/Downloaded 100% Apps/gridcraft-0.1.0-linux-x86_64.AppImage"
mkdir -p -- "$(dirname -- "$APPIMAGE")"
: >"$APPIMAGE"
export APPDIR APPIMAGE

# ---- run integration (AppRun execs the app; capture both) -----------------------------------
out="$("$APPDIR/AppRun" --version)"
case $out in
  *"gridcraft-ran --version"*) ok "AppRun execs the app and forwards arguments" ;;
  *) fail "AppRun execs the app and forwards arguments (got: $out)" ;;
esac

ENTRY="$XDG_DATA_HOME/applications/$APP_ID.desktop"
ICON="$XDG_DATA_HOME/icons/hicolor/256x256/apps/$APP_ID.png"

if [ -f "$ENTRY" ]; then ok "launcher entry installed under XDG_DATA_HOME"; else fail "launcher entry installed under XDG_DATA_HOME"; fi
if [ -f "$ICON" ]; then ok "hicolor icon installed under XDG_DATA_HOME"; else fail "hicolor icon installed under XDG_DATA_HOME"; fi
if [ -f "$ICON" ] && cmp -s "$ICON" "$APPDIR/usr/share/icons/hicolor/256x256/apps/$APP_ID.png"; then
  ok "installed icon matches the AppDir icon"
else
  fail "installed icon matches the AppDir icon"
fi

# Exec/TryExec must be rewritten to the AppImage path, not the packaged `gridcraft %F`.
# (The path may be quoted or bare depending on reserved characters; accept either.) In Exec a
# literal % in the path must be doubled (%%), which is the field-code escape.
appimage_exec="$(printf '%s' "$APPIMAGE" | sed 's/%/%%/g')"
exec_line="$(sed -n 's/^Exec=//p' "$ENTRY" 2>/dev/null || true)"
exec_tok="${exec_line% %F}"      # drop the trailing file placeholder
exec_tok="${exec_tok%\"}"; exec_tok="${exec_tok#\"}"   # then strip surrounding quotes
if [ "$exec_tok" = "$appimage_exec" ]; then
  ok "Exec points at the AppImage by absolute path"
else
  fail "Exec points at the AppImage by absolute path (got: $exec_line, want: $appimage_exec)"
fi
case $exec_line in
  *'%F') ok "Exec preserves the %F file placeholder" ;;
  *) fail "Exec preserves the %F file placeholder (got: $exec_line)" ;;
esac
try_line="$(sed -n 's/^TryExec=//p' "$ENTRY" 2>/dev/null || true)"
try_line="${try_line%\"}"; try_line="${try_line#\"}"
if [ "$try_line" = "$APPIMAGE" ]; then
  ok "TryExec points at the AppImage by absolute path (no field-code doubling)"
else
  fail "TryExec points at the AppImage by absolute path (got: $try_line)"
fi
# Icon= must stay the bare app id (no extension), per the desktop-entry spec.
if [ -f "$ENTRY" ] && grep -qxF "Icon=$APP_ID" "$ENTRY"; then
  ok "Icon= stays the bare app id"
else
  fail "Icon= stays the bare app id"
fi

# ---- idempotency: a second run must rewrite nothing -----------------------------------------
before="$(cksum "$ENTRY" "$ICON")"
"$APPDIR/AppRun" --version >/dev/null
after="$(cksum "$ENTRY" "$ICON")"
if [ "$before" = "$after" ]; then ok "second run is idempotent (files unchanged)"; else fail "second run is idempotent (files unchanged)"; fi

# ---- the app sees the bundle's share dir ----------------------------------------------------
seen="$("$APPDIR/AppRun" --version)"
case $seen in
  *"gridcraft-data-dirs $APPDIR/usr/share"* | *"gridcraft-data-dirs "*":$APPDIR/usr/share"*)
    ok "XDG_DATA_DIRS includes the AppDir share dir" ;;
  *) fail "XDG_DATA_DIRS includes the AppDir share dir (got: $seen)" ;;
esac

# ---- opt-out works --------------------------------------------------------------------------
rm -rf -- "$XDG_DATA_HOME/applications" "$XDG_DATA_HOME/icons"
GRIDCRAFT_SKIP_DESKTOP_INTEGRATION=1 "$APPDIR/AppRun" --version >/dev/null
if [ ! -f "$ENTRY" ]; then ok "GRIDCRAFT_SKIP_DESKTOP_INTEGRATION=1 installs nothing"; else fail "GRIDCRAFT_SKIP_DESKTOP_INTEGRATION=1 installs nothing"; fi

if [ "$fails" -ne 0 ]; then
  echo "apprun self-test: $fails check(s) failed" >&2
  exit 1
fi
echo "apprun self-test: all checks passed"