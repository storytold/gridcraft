#!/usr/bin/env bash
# Build the browser version and zip it:  $DIST/gridcraft-web-<version>.zip
#
# Usage: packaging/web/package.sh [--skip-build]
#
# Needs: trunk (brew install trunk / cargo install trunk --locked) and the wasm32-unknown-unknown
# target. The zip holds a self-contained static site in gridcraft-web-<version>/ that works
# from any URL path and inside an <iframe>. Hosting notes: packaging/web/README.md.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/web"

SITE="$ROOT/dist/web"
if [ "${1:-}" != "--skip-build" ]; then
  command -v trunk >/dev/null || { echo "error: trunk not found (cargo install trunk --locked)" >&2; exit 1; }
  [ -f "$ROOT/apps/gridcraft-web/index.html" ] || { echo "error: apps/gridcraft-web/index.html missing (the web app isn't set up yet)" >&2; exit 1; }
  # With CRAFT_FONTS_DIR set the web build bakes fonts into the wasm (crates/ui-egui/build.rs), so
  # the CJK face is subset first: the full Noto Sans CJK SC is 15.7 MiB, over what hosts allow in
  # one file. Absolute path: build.rs resolves a relative one from the crate directory.
  if [ -n "${CRAFT_FONTS_DIR:-}" ]; then
    WEB_FONTS_DIR="$CARGO_TARGET_DIR/web-fonts"
    case "$WEB_FONTS_DIR" in /*) ;; *) WEB_FONTS_DIR="$ROOT/$WEB_FONTS_DIR" ;; esac
    command -v python3 >/dev/null || { echo "error: python3 not found; CRAFT_FONTS_DIR needs packaging/web/subset-fonts.py" >&2; exit 1; }
    rm -rf "$WEB_FONTS_DIR"
    python3 "$HERE/subset-fonts.py" "$CRAFT_FONTS_DIR" "$WEB_FONTS_DIR"
    CRAFT_FONTS_DIR="$WEB_FONTS_DIR"
    export CRAFT_FONTS_DIR
  fi
  # --dist/--public-url here, so the output doesn't depend on Trunk.toml (which should agree:
  # public_url = "./").
  (cd "$ROOT/apps/gridcraft-web" && trunk build --release --dist "$SITE" --public-url ./)
fi

[ -f "$SITE/index.html" ] || { echo "error: $SITE/index.html missing; run without --skip-build" >&2; exit 1; }
# Paths must be relative so the site works under any prefix (public_url = "./" in Trunk.toml).
if grep -Eq '(src|href)="/[^/]' "$SITE/index.html"; then
  echo "error: $SITE/index.html has root-absolute URLs; it would break when served from a sub-path" >&2
  exit 1
fi

NAME="gridcraft-web-$VERSION"
WORK="$CARGO_TARGET_DIR/web-package"
rm -rf "$WORK"
mkdir -p "$WORK/$NAME"
cp -R "$SITE/." "$WORK/$NAME/"
# Sample server configs (MIME type, caching, compression); harmless where unused.
cp "$HERE/_headers" "$HERE/.htaccess" "$WORK/$NAME/"
cp "$HERE/README.md" "$WORK/$NAME/HOSTING.md"
copy_docs "$WORK/$NAME"
rm -f "$DIST/$NAME.zip"
(cd "$WORK" && zip -qr9 "$DIST/$NAME.zip" "$NAME")
echo "wrote $DIST/$NAME.zip"
ls -lh "$DIST/$NAME.zip"
