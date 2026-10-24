#!/usr/bin/env bash
# Build and package GridCraft for FreeBSD (<arch> is x86_64 or aarch64):
#
#   $DIST/gridcraft-<version>-freebsd-<arch>.tar.gz   bin/ + share/ (desktop entry, icons,
#                                                      AppStream, MIME), extract under /usr/local
#
# Usage: packaging/freebsd/package.sh [--skip-build]
#
# Runs on FreeBSD itself (CI: .github/workflows/freebsd.yml, in a FreeBSD VM). Needs cargo and
# the build deps listed in that workflow. Runtime deps on the user's system: libxkbcommon,
# wayland or libX11/libXcursor/libXrandr/libXi, mesa-libs or vulkan-loader.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
APP_ID=ai.storyteller.gridcraft
LINUX="$ROOT/packaging/linux"

SKIP_BUILD=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

release_arch() {
  local arch="$1"
  case "$arch" in
    amd64 | x86_64) echo x86_64 ;;
    arm64 | aarch64) echo aarch64 ;;
    *) echo "unsupported architecture $arch" >&2; return 2 ;;
  esac
  return
}
# The release workflow cross-compiles aarch64 in an x86_64 VM with CARGO_BUILD_TARGET set to
# aarch64-unknown-freebsd (craft-common-actions#39): name the archive after the target.
case "${CARGO_BUILD_TARGET:-}" in
  "") ARCH="$(release_arch "$(uname -m)")" ;;
  aarch64-unknown-freebsd | x86_64-unknown-freebsd) ARCH="$(release_arch "${CARGO_BUILD_TARGET%%-*}")" ;;
  *) echo "CARGO_BUILD_TARGET=$CARGO_BUILD_TARGET is not a supported FreeBSD target" >&2; exit 2 ;;
esac
# Only binaries for this machine run here (the --version check at the end); a FreeBSD target built
# on another OS never does.
HOST_ARCH="$(release_arch "$(uname -m)" 2>/dev/null)" || HOST_ARCH="$(uname -m)"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]] && [[ "$(uname -s)" != FreeBSD ]]; then
  HOST_ARCH="$(uname -s) $HOST_ARCH"
fi
BASENAME="gridcraft-$VERSION-freebsd-$ARCH"

echo "==> GridCraft $VERSION for FreeBSD $ARCH"

if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$ROOT" && cargo build --release --locked -p gridcraft -p gridcraft-cli)
fi
# Cargo puts a cross build under target/<triple>/release.
BIN="$CARGO_TARGET_DIR/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release"
WORK="$CARGO_TARGET_DIR/freebsd-package"
TREE="$WORK/$BASENAME"
rm -rf "$WORK"

install -d "$TREE/bin" "$TREE/share/applications" "$TREE/share/metainfo" "$TREE/share/mime/packages" \
  "$TREE/share/icons" "$TREE/share/doc/gridcraft"
install -m 755 "$BIN/gridcraft" "$BIN/gridcraft-cli" "$TREE/bin/"
strip "$TREE/bin/gridcraft" "$TREE/bin/gridcraft-cli" 2>/dev/null || true
install -m 644 "$LINUX/$APP_ID.desktop" "$TREE/share/applications/$APP_ID.desktop"
install -m 644 "$LINUX/$APP_ID.mime.xml" "$TREE/share/mime/packages/$APP_ID.xml"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$GRIDCRAFT_BUILD_DATE/g" \
  "$LINUX/$APP_ID.metainfo.xml.in" >"$TREE/share/metainfo/$APP_ID.metainfo.xml"
cp -R "$ROOT/assets/app-icon/hicolor" "$TREE/share/icons/"
copy_docs "$TREE/share/doc/gridcraft"

tar -C "$WORK" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
echo "wrote $DIST/$BASENAME.tar.gz"

if [[ "$ARCH" = "$HOST_ARCH" ]]; then
  "$TREE/bin/gridcraft-cli" --version
else
  # They cannot run here, so check what they are instead: executables, not libraries or objects.
  # An older file(1) calls a PIE a shared object; its interpreter still marks it as a program.
  want="$ARCH"; [[ "$ARCH" != x86_64 ]] || want=x86-64
  for b in "$TREE"/bin/*; do
    case "$(file -b "$b")" in
      ELF*" executable, "*"$want"*FreeBSD* | ELF*"$want"*FreeBSD*interpreter*) ;;
      *) echo "error: $b is not a FreeBSD $ARCH executable: $(file -b "$b")" >&2; exit 1 ;;
    esac
  done
  echo "skipping --version: an $ARCH binary does not run on this $HOST_ARCH machine"
fi
echo "==> done"
ls -lh "$DIST/$BASENAME.tar.gz"
