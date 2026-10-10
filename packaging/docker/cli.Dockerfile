# syntax=docker/dockerfile:1
#
# gridcraft-cli in a container: inspect, convert, evaluate and script spreadsheets without a
# window, and serve MCP over stdio. Nothing is published to a registry; build it locally:
#
#   docker build -f packaging/docker/cli.Dockerfile -t gridcraft-cli .
#   docker run --rm -v "$PWD:/data" gridcraft-cli convert /data/book.xlsx /data/out.csv
#   docker run --rm -v "$PWD:/data" gridcraft-cli eval '=SUM(1,2,3)'
#   docker run -i --rm gridcraft-cli mcp          # stdio MCP server (see packaging/docker/README.md)
#
# Two things worth knowing:
#   - `CRAFT_FONTS_DIR` is not needed here. That optional input is read by the egui app's build.rs;
#     the CLI links the engine, so this image builds from the repository alone.
#   - The image runs as root, so a bind-mounted directory stays writable. Pass
#     `--user "$(id -u):$(id -g)"` to run as yourself instead; the mount then has to be writable
#     by that user (the usual choice when you want -out files to belong to you, not to root).
# 1.95 is the floor: Cargo.toml's rust-version, and what the dependencies need — egui/eframe
# 0.36.2 require rustc 1.95, and crates/functions/src/engineering.rs uses std's EULER_GAMMA (1.94).
FROM rust:1.95-bookworm AS build

WORKDIR /src
# Manifests first, then sources: a source-only change reuses the dependency build below.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps
COPY xtask ./xtask

# --locked: build the graph the committed Cargo.lock pins, like the release does.
# The caches keep target/ and the registry out of the image layers between builds.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p gridcraft-cli \
    && cp target/release/gridcraft-cli /usr/local/bin/gridcraft-cli

FROM debian:bookworm-slim

# libgcc_s: Rust binaries panic with unwinding, so they link it even in release builds.
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgcc-s1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /usr/local/bin/gridcraft-cli /usr/local/bin/gridcraft-cli

# Where a spreadsheet gets mounted, e.g. `-v "$PWD:/data"`, so the examples in docs/cli.md work
# as written once the host path is rewritten to /data.
WORKDIR /data
ENTRYPOINT ["gridcraft-cli"]
CMD ["version"]
