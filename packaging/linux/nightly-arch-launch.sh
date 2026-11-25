#!/bin/sh
# GridCraft Nightly for the Arch handoff: the GUI lives in .gridcraft-nightly/ beside this
# launcher. The launch profile is the only data root a nightly build accepts, and it holds the
# settings, logs, the control token and the session record, so no XDG variable has to move.
set -eu

real_bin="$(dirname "$0")/.gridcraft-nightly/gridcraft"
# The Launcher's protocol and a bare version query name their own root and never read the default
# one, so they are dispatched before the guard that a default-root launch needs.
case "${1-}" in
  --launch-profile | --profile-root) exec "$real_bin" "$@" ;;
esac
if [ "$#" -eq 1 ] && { [ "${1-}" = "--version" ] || [ "${1-}" = "-V" ]; }; then
  exec "$real_bin" "$@"
fi
if [ -z "${XDG_DATA_HOME:-}" ] && [ -z "${HOME:-}" ]; then
  echo "gridcraft-nightly: XDG_DATA_HOME and HOME are both unset, so there is no Nightly data folder" >&2
  exit 2
fi
exec "$real_bin" --launch-profile nightly-v1 \
  --profile-root "${XDG_DATA_HOME:-$HOME/.local/share}/gridcraft-nightly/profile" -- "$@"
