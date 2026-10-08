#!/bin/sh
# AppRun — entry point for the GridCraft AppImage.
#
# Launches the app and, on a normal (writable, non-sandboxed) user account, installs this
# AppImage's launcher entry and icon theme into the user's XDG data directories — idempotently,
# doing nothing once they are up to date.
#
# Why: under Wayland the compositor ignores the window icon a client sets and instead matches the
# window's app id (ai.storyteller.gridcraft) against an *installed* desktop entry to pick the
# dock/taskbar icon. A directly-run AppImage exposes its .desktop and icons only inside the mounted
# image, so that lookup fails and GNOME/KDE fall back to a generic placeholder icon. A system
# package (deb/rpm/Flatpak) does not have this problem because it installs under /usr/share.
#
# Everything here is best effort: if any step fails the app still launches. Disable with
# GRIDCRAFT_SKIP_DESKTOP_INTEGRATION=1.
set -u

APP_ID=ai.storyteller.gridcraft

# Locate the AppDir: the runtime exports APPDIR on a real AppImage; otherwise use AppRun's own dir.
if [ -n "${APPDIR:-}" ]; then
  HERE=$APPDIR
else
  SELF=$0
  case $SELF in
    */*) ;;
    *) SELF=$(command -v -- "$SELF" 2>/dev/null || printf '%s' "$SELF") ;;
  esac
  LINK=$(readlink -f -- "$SELF" 2>/dev/null || printf '%s' "$SELF")
  HERE=$(cd -- "$(dirname -- "$LINK")" 2>/dev/null && pwd) || HERE=$(dirname -- "$SELF")
fi

EXE="$HERE/usr/bin/gridcraft"
# What the integrated desktop entry runs: the AppImage file itself when we have it, else AppRun.
ENTRY="${APPIMAGE:-$HERE/AppRun}"

# Let this process see the bundle's own share dir (bundled MIME types, icons).
export XDG_DATA_DIRS="$HERE/usr/share${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
export APPDIR="$HERE"

# Quote a path for a desktop-entry Exec/TryExec value, only when it needs it (spec: escape \ " ` $
# inside double quotes). Pass "fieldcodes" as $2 for Exec, where a literal % must be doubled; it
# is left alone for TryExec, which takes no field codes. The `$` in the sed program below is
# deliberately literal (it is the character being escaped), so the single-quoted expression is
# intended.
desktop_arg() {
  case $1 in
    *[!A-Za-z0-9/_.:+-]*)
      # shellcheck disable=SC2016
      q=$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/`/\\`/g' -e 's/\$/\\$/g')
      if [ "${2:-}" = fieldcodes ]; then q=$(printf '%s' "$q" | sed 's/%/%%/g'); fi
      printf '"%s"' "$q" ;;
    *) printf '%s' "$1" ;;
  esac
}

# Install $1 to $2, printing 'new' only if the contents changed (so callers can skip cache work).
install_file() {
  [ -f "$1" ] || return 0
  if [ -f "$2" ] && command -v cmp >/dev/null 2>&1 && cmp -s "$1" "$2"; then
    return 0
  fi
  mkdir -p -- "$(dirname -- "$2")" 2>/dev/null || return 0
  cp -f -- "$1" "$2" 2>/dev/null || return 0
  printf 'new'
}

integrate() {
  base="${XDG_DATA_HOME:-}"
  [ -n "$base" ] || base="${HOME:-}/.local/share"
  # Need a non-empty, absolute, writable base — otherwise there is nowhere to install to.
  [ -n "$base" ] || return 0
  case $base in /*) ;; *) return 0 ;; esac
  apps="$base/applications"
  icons="$base/icons"
  changed=

  # Desktop entry with Exec/TryExec pointing at this AppImage by absolute path (the packaged
  # Exec=gridcraft only works for a system install where gridcraft is on PATH).
  if [ -f "$HERE/$APP_ID.desktop" ] && command -v awk >/dev/null 2>&1; then
    tmp=$(mktemp 2>/dev/null) || tmp=
    if [ -n "$tmp" ]; then
      EXEC_LINE="Exec=$(desktop_arg "$ENTRY" fieldcodes) %F" \
        TRY_LINE="TryExec=$(desktop_arg "$ENTRY")" \
        awk '/^Exec=/ {print ENVIRON["EXEC_LINE"]; next}
             /^TryExec=/ {print ENVIRON["TRY_LINE"]; next}
             {print}' "$HERE/$APP_ID.desktop" >"$tmp" 2>/dev/null || true
      if [ -s "$tmp" ]; then changed="$changed$(install_file "$tmp" "$apps/$APP_ID.desktop")"; fi
      rm -f -- "$tmp"
    fi
  fi

  # The hicolor icon theme, so a bare Icon=$APP_ID resolves to the real artwork (dock, menus).
  for src in "$HERE"/usr/share/icons/hicolor/*/apps/"$APP_ID".*; do
    [ -f "$src" ] || continue
    rel=${src#"$HERE/usr/share/icons/"}
    changed="$changed$(install_file "$src" "$icons/$rel")"
  done

  # Refresh the caches that make the new entry/icon visible, only when something changed.
  if [ -n "$changed" ]; then
    if command -v update-desktop-database >/dev/null 2>&1; then
      update-desktop-database -q "$apps" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
      gtk-update-icon-cache -q -t -f "$icons/hicolor" 2>/dev/null || true
    fi
  fi
}

if [ "${GRIDCRAFT_SKIP_DESKTOP_INTEGRATION:-0}" != 1 ] && [ -z "${FLATPAK_ID:-}" ]; then
  integrate || true
fi

exec "$EXE" "$@"