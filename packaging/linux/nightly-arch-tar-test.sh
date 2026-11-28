#!/usr/bin/env bash
# Validate the staged Arch launcher without building or opening GridCraft.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/source"
dst="$work/dist"
base=gridcraft-0.0.1-nightly.1-linux-x86_64
mkdir -p "$stage/bin" "$dst" "$work/extract"

# A local argv recorder stands in for the compiled binaries. It never opens a UI.
cat >"$stage/bin/gridcraft" <<'RECORDER'
#!/bin/sh
printf '<%s>\n' "$@"
RECORDER
chmod 755 "$stage/bin/gridcraft"
cp "$stage/bin/gridcraft" "$stage/bin/gridcraft-cli"

bash "$HERE/nightly-arch-tar.sh" "$stage" "$work/stage" "$dst" "$base"
tar -xzf "$dst/$base.tar.gz" -C "$work/extract"
bin="$work/extract/$base/bin"
# Simulate the existing Arch bin-tree copy and public launcher rename.
mkdir -p "$work/installed"
cp -R "$bin" "$work/installed/bin"
bin="$work/installed/bin"
mv "$bin/gridcraft" "$bin/gridcraft-nightly"
mv "$bin/gridcraft-cli" "$bin/gridcraft-nightly-cli"

test ! -e "$stage/bin/.gridcraft-nightly"
cmp "$stage/bin/gridcraft" "$bin/.gridcraft-nightly/gridcraft"
cmp "$stage/bin/gridcraft-cli" "$bin/gridcraft-nightly-cli"

# The launcher starts the GUI through the flavor's launch profile, its only data root.
data="$work/data"
profile="$data/gridcraft-nightly/profile"
expect() {
  expected="$1"
  shift
  actual=$(XDG_DATA_HOME="$data" PATH="$bin:$PATH" gridcraft-nightly "$@")
  if [[ "$actual" != "$expected" ]]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
    exit 1
  fi
}
nightly_argv() {
  printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$profile" --
}
expect "$(nightly_argv)"
expect "$(nightly_argv; printf '<%s>\n' 'workbook with spaces.xlsx' '--other-name.xlsx')" \
  'workbook with spaces.xlsx' '--other-name.xlsx'
# A bare version query and the Launcher's own protocol pass straight through.
expect '<--version>' --version
expect '<-V>' -V
expect "$(printf '<%s>\n' --launch-profile nightly-v1 --profile-root "$work/launcher" -- 'book.xlsx')" \
  --launch-profile nightly-v1 --profile-root "$work/launcher" -- 'book.xlsx'
# A profile needs a data folder: the launcher refuses instead of writing into /.
status=0
env -u XDG_DATA_HOME -u HOME PATH="$bin:$PATH" gridcraft-nightly book.xlsx >/dev/null 2>&1 || status=$?
[[ "$status" -eq 2 ]] || { printf 'expected exit 2 without XDG_DATA_HOME/HOME, got %s\n' "$status" >&2; exit 1; }
test ! -e "$data/gridcraft-nightly"
test ! -e "$work/launcher"

echo "PASS: Arch Nightly handoff preserves launcher layout, profile selection and arguments"
