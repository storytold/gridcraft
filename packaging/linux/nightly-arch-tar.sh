#!/usr/bin/env bash
# Stage the GridCraft Nightly-only handoff for the Arch packager.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
source_tree="$1"
work="$2"
destination="$3"
basename="$4"
stage="$work/$basename"

mkdir -p "$work"
cp -R "$source_tree" "$stage"
mkdir "$stage/bin/.gridcraft-nightly"
mv "$stage/bin/gridcraft" "$stage/bin/.gridcraft-nightly/gridcraft"
cp "$here/nightly-arch-launch.sh" "$stage/bin/gridcraft"
chmod 755 "$stage/bin/gridcraft"
tar -C "$work" -czf "$destination/$basename.tar.gz" "$basename"
echo "wrote $destination/$basename.tar.gz"
