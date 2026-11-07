#!/usr/bin/env bash
# Build the gridcraft-harbor-base image that every task environment starts from.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
DOCKER_BUILDKIT=1 docker build \
  -f "$root/rl/harbor/docker/gridcraft.Dockerfile" \
  -t "${GRIDCRAFT_BASE_IMAGE:-gridcraft-harbor-base:latest}" \
  "$root"
