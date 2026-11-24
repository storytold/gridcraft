#!/bin/sh
# GridCraft Nightly launcher for a moved tree: an AppImage (AppRun and usr/bin), the plain nightly
# tar, or a distro tarball's usr/bin. The launch profile is the only data root a nightly build
# accepts, so this is what keeps a Nightly run out of the stable app's settings, prefs and logs.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
real="$here/usr/bin/gridcraft"
[ -x "$real" ] || real="$here/gridcraft"
if [ ! -x "$real" ]; then
  echo "gridcraft-nightly: no GridCraft binary beside $0" >&2
  exit 2
fi
if [ -z "${XDG_DATA_HOME:-}" ] && [ -z "${HOME:-}" ]; then
  echo "gridcraft-nightly: XDG_DATA_HOME and HOME are both unset, so there is no Nightly data folder" >&2
  exit 2
fi
# A bare version query and the Launcher's own protocol pass straight through.
case "${1-}" in
  --launch-profile | --profile-root) exec "$real" "$@" ;;
esac
if [ "$#" -eq 1 ] && { [ "${1-}" = "--version" ] || [ "${1-}" = "-V" ]; }; then
  exec "$real" "$@"
fi
exec "$real" --launch-profile nightly-v1 --profile-root "${XDG_DATA_HOME:-$HOME/.local/share}/gridcraft-nightly/profile" -- "$@"
