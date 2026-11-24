#!/bin/sh
set -eu

real_bin="$(dirname "$0")/.gridcraft-nightly/gridcraft"
exec "$real_bin" "$@"
