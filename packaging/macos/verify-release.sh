#!/usr/bin/env bash
# CI gate: refuse to ship macOS artifacts that are not Developer-ID signed and notarized.
#
# Usage: verify-release.sh [dir]          (default: $DIST)
#
# Checks every GridCraft macOS artifact in the directory:
#   DMG — the app inside must be Developer-ID signed (never ad-hoc) and spctl-accepted,
#         and the DMG must carry a valid stapled notarization ticket.
#   ZIP — the CLI binary must be Developer-ID signed and spctl-accepted. A bare Mach-O
#         cannot carry a staple; the zip's notarization ticket lives online and spctl
#         consults it there.
#
# Exits non-zero with one line per problem. Local ad-hoc builds fail this check by
# design — the guard runs in CI only, after packaging and before upload.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

DIR="${1:-$DIST}"
shopt -s nullglob

errors=()
any=false

is_adhoc() { # prints nothing; returns 0 when the signature is ad-hoc
  # No `grep -q`: it would exit early, SIGPIPE the producer and fail under pipefail.
  codesign -dv "$1" 2>&1 | grep 'Signature=adhoc' >/dev/null
}

for dmg in "$DIR"/gridcraft-*-macos-*.dmg; do
  any=true
  echo "==> $dmg"
  if xcrun stapler validate "$dmg" >/dev/null 2>&1; then
    echo "    stapled notarization ticket: ok"
  else
    errors+=("$dmg: notarization ticket missing or invalid (stapler validate failed)")
  fi
  mp="$(mktemp -d)"
  if ! hdiutil attach -readonly -nobrowse -mountpoint "$mp" "$dmg" >/dev/null 2>&1; then
    errors+=("$dmg: could not mount for inspection")
    rmdir "$mp"
    continue
  fi
  app="$(find "$mp" -maxdepth 2 -name '*.app' -print -quit)"
  if [ -z "$app" ]; then
    errors+=("$dmg: no .app inside")
  elif is_adhoc "$app"; then
    errors+=("$dmg: app is ad-hoc signed, not Developer ID")
  elif ! spctl --assess --type execute -vv "$app" >/dev/null 2>&1; then
    errors+=("$dmg: app rejected by Gatekeeper policy (spctl)")
  else
    echo "    app signature + Gatekeeper: ok"
  fi
  hdiutil detach "$mp" >/dev/null
  rmdir "$mp"
done

for zip in "$DIR"/gridcraft-cli-*-macos-*.zip; do
  any=true
  echo "==> $zip"
  tmp="$(mktemp -d)"
  unzip -q "$zip" -d "$tmp"
  bin="$(find "$tmp" -type f -name gridcraft-cli -print -quit)"
  if [ -z "$bin" ]; then
    errors+=("$zip: no gridcraft-cli binary inside")
  elif is_adhoc "$bin"; then
    errors+=("$zip: binary is ad-hoc signed, not Developer ID")
  elif ! spctl --assess --type execute -vv "$bin" >/dev/null 2>&1; then
    errors+=("$zip: binary rejected by Gatekeeper policy (spctl; notarization ticket not found online)")
  else
    echo "    binary signature + notarization lookup: ok"
  fi
  rm -rf "$tmp"
done

if [ "$any" = false ]; then
  errors+=("$DIR: no macOS artifacts found (gridcraft-*-macos-*.dmg, gridcraft-cli-*-macos-*.zip)")
fi

if [ "${#errors[@]}" -ne 0 ]; then
  echo "verify-release: FAILED"
  for e in "${errors[@]+"${errors[@]}"}"; do
    echo "  $e"
  done
  exit 1
fi
echo "verify-release: all macOS artifacts Developer-ID signed and notarized"
