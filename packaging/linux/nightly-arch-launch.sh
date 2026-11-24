#!/bin/sh
set -eu

: "${XDG_CONFIG_HOME:=${HOME:-}/.config}"
: "${XDG_DATA_HOME:=${HOME:-}/.local/share}"
: "${XDG_STATE_HOME:=${HOME:-}/.local/state}"
: "${XDG_CACHE_HOME:=${HOME:-}/.cache}"
export XDG_CONFIG_HOME="$XDG_CONFIG_HOME/gridcraft-nightly"
export XDG_DATA_HOME="$XDG_DATA_HOME/gridcraft-nightly"
export XDG_STATE_HOME="$XDG_STATE_HOME/gridcraft-nightly"
export XDG_CACHE_HOME="$XDG_CACHE_HOME/gridcraft-nightly"

real_bin="$(dirname "$0")/.gridcraft-nightly/gridcraft"
exec "$real_bin" "$@"
