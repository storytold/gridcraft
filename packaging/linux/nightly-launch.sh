#!/bin/sh
# GridCraft Nightly launcher for a moved tree: an AppImage (AppRun and usr/bin), the plain nightly
# tar, or a distro tarball's usr/bin. The launch profile is the only data root a nightly build
# accepts, so this is what keeps a Nightly run out of the stable app's settings, prefs and logs.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
# Where each placement keeps the GUI it starts: the AppImage (and any installed tree) has it in
# usr/bin/gridcraft; the flat nightly tar names it gridcraft-bin beside this launcher; a plain
# tree tarball keeps it beside its bin/gridcraft-nightly. A candidate that is this same file is
# never started, so a launcher can not recurse into itself.
self="$here/$(basename "$0")"
real=""
for candidate in "$here/usr/bin/gridcraft" "$here/gridcraft-bin" "$here/gridcraft"; do
  if [ -x "$candidate" ] && [ ! "$candidate" -ef "$self" ]; then
    real="$candidate"
    break
  fi
done
if [ -z "$real" ]; then
  echo "gridcraft-nightly: no GridCraft binary beside $0 (usr/bin/gridcraft, gridcraft-bin)" >&2
  exit 2
fi
# A bare version query and the Launcher's own protocol pass straight through: neither needs a data
# folder of this script's own, so neither may be refused for a HOME it never reads.
case "${1-}" in
  --launch-profile | --profile-root) exec "$real" "$@" ;;
esac
if [ "$#" -eq 1 ] && { [ "${1-}" = "--version" ] || [ "${1-}" = "-V" ]; }; then
  exec "$real" "$@"
fi
if [ -z "${XDG_DATA_HOME:-}" ] && [ -z "${HOME:-}" ]; then
  echo "gridcraft-nightly: XDG_DATA_HOME and HOME are both unset, so there is no Nightly data folder" >&2
  exit 2
fi
exec "$real" --launch-profile nightly-v1 --profile-root "${XDG_DATA_HOME:-$HOME/.local/share}/gridcraft-nightly/profile" -- "$@"
