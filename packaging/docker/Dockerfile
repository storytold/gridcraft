# syntax=docker/dockerfile:1
#
# GridCraft in one image: the browser build served by nginx, with the headless gridcraft-cli
# (inspect, convert, evaluate, script spreadsheets, and serve MCP over stdio) alongside it.
# Nothing is published to a registry; build it locally after making the web build:
#
#   packaging/web/package.sh                      # writes dist/web/ (trunk build --release)
#   docker build -f packaging/docker/Dockerfile -t gridcraft .
#   docker run --rm -p 8771:80 gridcraft          # the web app at http://localhost:8771/
#   docker run --rm gridcraft eval '=SUM(1,2,3)'  # the CLI
#   docker run --rm -v "$PWD:/data" gridcraft convert /data/book.xlsx /data/out.csv
#   docker run -i --rm gridcraft mcp              # stdio MCP server (see packaging/docker/README.md)
#
# The image serves nginx by default; any other argument runs the CLI (the entrypoint below).
#
# Two things worth knowing:
#   - The web build (dist/web/) is made on the host by packaging/web/package.sh, which bakes the
#     optional craft-fonts face into the wasm. So this image builds from the repository plus
#     dist/web, and installs no fonts of its own: the CLI links the engine and needs none.
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

# Debian, not Alpine: the CLI binary is built against glibc, and this variant provides it (the
# Alpine image is musl and would not run it). It also carries libgcc_s (Rust binaries unwind) and
# curl (compose's healthcheck); being the official nginx image, it keeps nginx's own entrypoint.
FROM nginx:stable-bookworm

# The static site, made by packaging/web/package.sh. It already carries the baked-in CJK face, and
# index.html loads everything through relative URLs, so the site works under any path.
COPY dist/web/ /usr/share/nginx/html/
# The server settings packaging/web/README.md requires, written out for nginx (MIME types, caching,
# compression). Replaces the image's default.conf; mount over it to tweak without rebuilding.
COPY packaging/docker/nginx/default.conf /etc/nginx/conf.d/default.conf

COPY --from=build /usr/local/bin/gridcraft-cli /usr/local/bin/gridcraft-cli

# One image, two jobs. No argument — or a web-ish one — starts nginx (through the image's own
# entrypoint, so its /docker-entrypoint.d tuning still runs) and serves the site; anything else is
# a gridcraft-cli subcommand, so `docker run <image> eval '=SUM(1,2,3)'` reads as it did with the
# CLI-only image. `--entrypoint sh` gets a shell.
COPY <<'EOF' /usr/local/bin/gridcraft-entrypoint
#!/bin/sh
set -e
case "${1:-}" in
    "" | nginx | web | serve)
        exec /docker-entrypoint.sh nginx -g "daemon off;"
        ;;
    *)
        exec gridcraft-cli "$@"
        ;;
esac
EOF
RUN chmod 0755 /usr/local/bin/gridcraft-entrypoint

# Where a spreadsheet gets mounted, e.g. `-v "$PWD:/data"`, so the examples in docs/cli.md work as
# written once the host path is rewritten to /data.
WORKDIR /data
EXPOSE 80
ENTRYPOINT ["/usr/local/bin/gridcraft-entrypoint"]
CMD ["web"]
