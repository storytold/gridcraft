# Running GridCraft with Docker

One image holds both things that containerise well, and neither needs anything from a registry:
nginx serves the browser build, and the same image carries the headless `gridcraft-cli` (inspect,
convert, evaluate, script, and MCP over stdio). The desktop app is deliberately not one of them — it
installs into the system, opens a window, uses the tray and starts other programs, none of which
survives containerisation.

## The image

`packaging/docker/Dockerfile` builds it; the web build has to exist first, because the image copies
it in rather than building wasm itself:

```sh
packaging/web/package.sh                       # writes dist/web/ (trunk build --release)
docker build -f packaging/docker/Dockerfile -t gridcraft .
```

No trunk at hand? Unpack a [release
zip](https://github.com/storytold/gridcraft/releases) into `dist/web/` first — `index.html` loads
everything through relative URLs, so the published site is byte-for-byte what the image should serve:

```sh
mkdir -p dist/web && unzip -j gridcraft-web-0.4.0.zip 'gridcraft-web-0.4.0/*' -d dist/web
docker build -f packaging/docker/Dockerfile -t gridcraft .
```

One image, two jobs. It serves nginx by default; any other argument runs the CLI:

```sh
docker run --rm -p 8771:80 gridcraft            # the web app at http://localhost:8771/
docker run --rm gridcraft eval '=SUM(1,2,3)'    # the CLI
```

### Published images

Every release publishes this image to GitHub Container Registry
(`release.yml`'s `docker` jobs), as a multi-architecture `linux/amd64` + `linux/arm64` manifest.
Pull it instead of building:

```sh
docker run --rm -p 8771:80 ghcr.io/storytold/gridcraft:latest
docker run --rm ghcr.io/storytold/gridcraft:0.4 eval '=SUM(1,2,3)'
```

Tags: `<version>` (e.g. `0.4.0`) always, plus `0.4` and `latest` for a stable release — a
pre-release like `0.4.0-rc.1` gets its own tag only, so `latest` is never an unfinished build.
`packaging/docker/compose.yaml` still builds locally; point its `image:` at
`ghcr.io/storytold/gridcraft:<version>` and drop the `build:` block to run the published one.

## The web app

With compose, which builds and runs the same image:

```sh
packaging/web/package.sh                                       # dist/web/ must exist first
docker compose -f packaging/docker/compose.yaml up --build     # then open http://localhost:8771/
docker compose -f packaging/docker/compose.yaml down
```

`GRIDCRAFT_WEB_PORT` moves the published port.

`nginx/default.conf` is the nginx form of the hosting requirements in
[`packaging/web/README.md`](../web/README.md), baked into the image. `smoke-test.sh` checks a running
server — this one or your own — for them:

```sh
packaging/docker/smoke-test.sh http://localhost:8771
```

It fails on a wrong MIME type, a missing or wrong `Cache-Control`, an asset the page asks for that
404s, or a `.wasm` that is not gzipped. Mind the HTTPS rule from that README: WebGPU and the clipboard
need a secure context, so `http://localhost` is fine while a plain-HTTP LAN address such as
`http://192.168.1.5:8771` serves the app with the WebGL2 fallback.

## The CLI, and MCP over stdio

The CLI is a command on the image above, not a second one:

```sh
docker run --rm -v "$PWD:/data" gridcraft convert /data/book.xlsx /data/out.csv
docker run --rm -v "$PWD:/data" gridcraft eval '=SUM(B2:B9)' --in /data/book.xlsx
docker run --rm -v "$PWD:/data" gridcraft cat /data/book.xlsx --range A1:F20
docker run --rm -v "$PWD:/data" gridcraft info /data/book.xlsx --json
```

Through compose, run the same image's CLI branch with `docker compose run`:

```sh
docker compose -f packaging/docker/compose.yaml run --rm web eval '=SUM(1,2,3)'
```

The image builds the CLI from the committed `Cargo.lock` (`--locked`) and installs no fonts of its
own: the web build baked the CJK faces into its wasm (see
[`packaging/web/README.md`](../web/README.md)), and the CLI links the engine and needs none. It runs
as root so that a bind-mounted directory stays writable — pass `--user "$(id -u):$(id -g)"` to run as
yourself instead, which needs the mount writable by that user.

`mcp` speaks JSON-RPC over stdio, so a client starts the container itself and must keep stdin open
(`-i`):

```sh
claude mcp add gridcraft -- docker run --rm -i gridcraft mcp
```

or, for a client configured by JSON:

```json
{
  "mcpServers": {
    "gridcraft": {
      "command": "docker",
      "args": ["run", "--rm", "-i", "gridcraft", "mcp"]
    }
  }
}
```

Give it a workbook with `-v "$PWD:/data"` and `--in /data/book.xlsx`, or `--sample budget`, as in
[docs/cli.md](../../docs/cli.md) with host paths rewritten to `/data`. The `send`/`--connect` control
channel is different: it talks to a running desktop app over TCP on the host, which a container cannot
reach by default — the file, formula and MCP paths need no such link.
