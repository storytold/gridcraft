#!/usr/bin/env bash
# Build, sign and (optionally) notarize the macOS release artifacts:
#
#   $DIST/gridcraft-<version>-macos-<arch>.dmg          GridCraft.app on a drag-to-Applications DMG
#   $DIST/gridcraft-cli-<version>-macos-<arch>.zip      the headless CLI
#
# Usage: packaging/macos/package.sh [--arch universal|aarch64|x86_64] [--skip-build]
#
# Signing (env):
#   MACOS_SIGN_IDENTITY   codesign identity (name or SHA-1). Default "-" = ad-hoc (local testing;
#                         Gatekeeper will reject the result on other Macs). In CI, import-cert.sh sets it.
#   MACOS_KEYCHAIN        keychain holding the identity (optional)
# Notarization (env; all three needed, and a real identity):
#   APPLE_ID, APPLE_PASSWORD (app-specific password), APPLE_TEAM_ID
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/macos"

ARCH=universal
SKIP_BUILD=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$ARCH" in
  universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  aarch64) TARGETS=(aarch64-apple-darwin) ;;
  x86_64) TARGETS=(x86_64-apple-darwin) ;;
  *) echo "unknown --arch $ARCH" >&2; exit 2 ;;
esac

# Keep in sync with LSMinimumSystemVersion in Info.plist.in.
export MACOSX_DEPLOYMENT_TARGET=11.0
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
SHORT_VERSION="${VERSION%%-*}"
WORK="$CARGO_TARGET_DIR/macos-package"
APP="$WORK/GridCraft.app"
DMG="$DIST/gridcraft-$VERSION-macos-$ARCH.dmg"
CLI_ZIP="$DIST/gridcraft-cli-$VERSION-macos-$ARCH.zip"

NOTARIZE=0
if [[ "$IDENTITY" = "-" ]]; then
  warn "macOS: ad-hoc signing (no MACOS_SIGN_IDENTITY); artifacts are not notarized"
elif [[ -n "${APPLE_ID:-}" ]] && [[ -n "${APPLE_PASSWORD:-}" ]] && [[ -n "${APPLE_TEAM_ID:-}" ]]; then
  NOTARIZE=1
else
  warn "macOS: APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID incomplete; signed but not notarized"
fi

echo "==> GridCraft $VERSION for macOS ($ARCH), identity: $IDENTITY, notarize: $NOTARIZE"

# ---- build -------------------------------------------------------------------------------------
if [[ "$SKIP_BUILD" = 0 ]]; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  (cd "$ROOT" && cargo build --release --locked -p gridcraft -p gridcraft-cli "${args[@]}")
fi

rm -rf "$WORK"
mkdir -p "$WORK/bin"
for bin in gridcraft gridcraft-cli; do
  inputs=()
  for t in "${TARGETS[@]}"; do inputs+=("$CARGO_TARGET_DIR/$t/release/$bin"); done
  lipo -create -output "$WORK/bin/$bin" "${inputs[@]}"
  lipo -info "$WORK/bin/$bin"
done

# ---- signing helpers ---------------------------------------------------------------------------
sign() {
  # Hardened runtime + secure timestamp for a real identity; ad-hoc can't be timestamped.
  local ts=(--timestamp)
  [[ "$IDENTITY" = "-" ]] && ts=(--timestamp=none)
  local kc=()
  [[ -n "${MACOS_KEYCHAIN:-}" ]] && kc=(--keychain "$MACOS_KEYCHAIN")
  codesign --force --sign "$IDENTITY" ${kc[@]+"${kc[@]}"} "${ts[@]}" "$@"
}

# stapler fetches the ticket from Apple's CloudKit, which sometimes drops the connection ("The
# network connection was lost", Error 68). Stapling and validating are idempotent, so retry.
stapler_retry() {
  local action="$1" attempt
  for attempt in 1 2 3 4 5; do
    if xcrun stapler "$@"; then
      return 0
    fi
    [[ "$attempt" = 5 ]] && { echo "stapler $action failed 5 times" >&2; return 1; }
    warn "stapler $action failed (attempt $attempt of 5); retrying in $((attempt * 15))s"
    sleep $((attempt * 15))
  done
}

notarize() {
  local file="$1" out id status
  echo "==> notarizing $(basename "$file") (this can take a few minutes)"
  out="$(xcrun notarytool submit "$file" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait --timeout 1h --output-format json)" || true
  echo "$out"
  id="$(printf '%s' "$out" | plutil -extract id raw -o - - 2>/dev/null || true)"
  status="$(printf '%s' "$out" | plutil -extract status raw -o - - 2>/dev/null || true)"
  if [[ "$status" != "Accepted" ]]; then
    if [[ -n "$id" ]]; then
      xcrun notarytool log "$id" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" || true
    fi
    echo "error: notarization of $(basename "$file") failed (status: ${status:-unknown})" >&2
    exit 1
  fi
}

# ---- GridCraft.app ----------------------------------------------------------------------------
echo "==> assembling $APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# Executable and icon carry the display name (CFBundleExecutable / CFBundleIconFile).
cp "$WORK/bin/gridcraft" "$APP/Contents/MacOS/GridCraft"
cp "$ROOT/assets/app-icon/gridcraft.icns" "$APP/Contents/Resources/GridCraft.icns"
# Bundled dependency font notices are installed before the app is signed.
cp "$ROOT/NOTICE" "$ROOT/ATTRIBUTION.md" "$APP/Contents/Resources/"
# Licences of the embedded craft-fonts fonts (only when built with CRAFT_FONTS_DIR).
copy_font_licences "$APP/Contents/Resources"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" \
  -e "s/@BUILD_SHA@/${GRIDCRAFT_BUILD_SHA:-unknown}/g" \
  "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"

# Resource forks and Finder metadata invalidate strict bundle verification. Clear them before signing.
xattr -cr "$APP"
# Sign inside-out: nested code first, then the bundle itself (no --deep on the final signature).
# Today the only nested code is the main executable; frameworks/helpers would be signed here too.
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/GridCraft"
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"

if [[ "$NOTARIZE" = 1 ]]; then
  ditto -c -k --keepParent "$APP" "$WORK/GridCraft-notarize.zip"
  notarize "$WORK/GridCraft-notarize.zip"
  stapler_retry staple "$APP"
  stapler_retry validate "$APP"
  spctl --assess --type execute -vvv "$APP"
fi

# ---- DMG ---------------------------------------------------------------------------------------
echo "==> building $DMG"
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/GridCraft.app"
ln -s /Applications "$STAGE/Applications"
# Finder window layout: background, icon size and positions (packaging/macos/dmg/README.md).
mkdir -p "$STAGE/.background"
cp "$HERE/dmg/background.tiff" "$STAGE/.background/background.tiff"
cp "$HERE/dmg/dmg-layout.DS_Store" "$STAGE/.DS_Store"
rm -f "$DMG"
# Retries an hdiutil verb that attaches a device: on CI runners it fails now and then with
# "Resource busy". Usage: hdiutil_retry <verb> <args...>; five tries with growing pauses.
hdiutil_retry() {
  local verb="$1"
  local attempt
  for attempt in 1 2 3 4 5; do
    if hdiutil "$@"; then
      return 0
    fi
    if [[ "$attempt" = 5 ]]; then
      echo "error: hdiutil $verb failed 5 times" >&2
      return 1
    fi
    warn "hdiutil $verb failed (attempt $attempt of 5); retrying in $((attempt * 10))s"
    sleep $((attempt * 10))
  done
  return 1
}
# `hdiutil create -srcfolder` copies the staged files as they are. The previous
# `makehybrid -hfs` + `convert` gave every file and folder in the image a com.apple.FinderInfo
# (Finder location -1,-1), so the signed app failed `codesign --verify --deep --strict` inside the
# mounted DMG and after copying it out. The volume name has no version: .DS_Store finds the
# background through an alias that includes it.
hdiutil_retry create -srcfolder "$STAGE" -volname "GridCraft" -fs HFS+ -format UDZO \
  -imagekey zlib-level=9 -ov "$DMG"
sign "$DMG"
codesign --verify --strict --verbose=2 "$DMG"

# Gate: the app inside the finished image, and a copy installed from it with ditto, must pass the
# same strict check as the app before packaging, so a container signature can't hide a damaged app
# signature. Returns non-zero (set -e then stops the build) after detaching the image.
verify_dmg_app() {
  local dmg="$1" mount="$WORK/dmg-check" copy="$WORK/dmg-check-copy" rc=0
  mkdir -p "$mount" "$copy"
  hdiutil_retry attach -readonly -nobrowse -noautoopen -mountpoint "$mount" "$dmg" >/dev/null
  echo "==> strict signature check of GridCraft.app inside $(basename "$dmg")"
  codesign --verify --deep --strict --verbose=2 "$mount/GridCraft.app" || rc=1
  if [[ "$rc" = 0 ]]; then
    ditto "$mount/GridCraft.app" "$copy/GridCraft.app" || rc=1
  fi
  if [[ "$rc" = 0 ]]; then
    echo "==> strict signature check of a ditto copy of it"
    codesign --verify --deep --strict --verbose=2 "$copy/GridCraft.app" || rc=1
  fi
  if [[ "$rc" = 0 ]] && [[ "$NOTARIZE" = 1 ]]; then
    spctl --assess --type execute -vvv "$copy/GridCraft.app" || rc=1
  fi
  if [[ "$rc" != 0 ]]; then
    echo "error: GridCraft.app in $(basename "$dmg") fails strict verification; extended attributes:" >&2
    xattr -lr "$mount/GridCraft.app" >&2 || true
  fi
  hdiutil_retry detach "$mount" >/dev/null || hdiutil detach "$mount" -force >/dev/null || rc=1
  rm -rf "$copy"
  return "$rc"
}
verify_dmg_app "$DMG"
if [[ "$NOTARIZE" = 1 ]]; then
  notarize "$DMG"
  stapler_retry staple "$DMG"
  stapler_retry validate "$DMG"
  spctl --assess --type open --context context:primary-signature -vvv "$DMG"
fi

# ---- CLI ---------------------------------------------------------------------------------------
echo "==> building $CLI_ZIP"
CLI_DIR="$WORK/gridcraft-cli-$VERSION-macos-$ARCH"
mkdir -p "$CLI_DIR"
cp "$WORK/bin/gridcraft-cli" "$CLI_DIR/"
copy_docs "$CLI_DIR"
sign --options runtime "$CLI_DIR/gridcraft-cli"
codesign --verify --strict --verbose=2 "$CLI_DIR/gridcraft-cli"
rm -f "$CLI_ZIP"
ditto -c -k --keepParent "$CLI_DIR" "$CLI_ZIP"
# A bare Mach-O can't carry a stapled ticket; Gatekeeper looks the notarization up online.
if [[ "$NOTARIZE" = 1 ]]; then notarize "$CLI_ZIP"; fi

"$WORK/bin/gridcraft-cli" --version
echo "==> done"
ls -lh "$DMG" "$CLI_ZIP"
