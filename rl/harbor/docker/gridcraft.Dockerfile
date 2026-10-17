# syntax=docker/dockerfile:1
# Base image for GridCraft Harbor tasks: the headless `gridcraft-cli` (inspect, eval, run commands,
# MCP server) on Ubuntu, with Python for verifiers. Build from the repository root:
#   rl/harbor/build-base.sh
# Task environments start `FROM gridcraft-harbor-base:latest`.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p gridcraft-cli \
    && cp target/release/gridcraft-cli /usr/local/bin/gridcraft-cli

FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git python3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/gridcraft-cli /usr/local/bin/gridcraft-cli
WORKDIR /app
