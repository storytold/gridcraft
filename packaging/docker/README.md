# Running GridCraft with Docker

Two things run in a container here, and neither needs anything from a registry: the browser build
served by nginx (`compose.yaml`) and the headless `gridcraft-cli` (`cli.Dockerfile`). The desktop app
is deliberately not one of them — it installs into the system, opens a window, uses the tray and
starts other programs, none of which survives containerisation.

## The web app

```sh
packaging/web/package.sh                            # builds dist/web/ (trunk build --release)
docker compose -f packaging/docker/compose.yaml up  # official nginx image, no image to build
# open http://localhost:8771/
docker compose -f packaging/docker/compose.yaml down
```

No trunk at hand? An unpacked [release zip](https://github.com/storytold/gridcraft/releases) serves
the same way, since `index.html` loads everything through relative URLs:

```sh
GRIDCRAFT_WEB_DIR=/path/to/gridcraft-web-0.4.0 docker compose -f packaging/docker/compose.yaml up
```

`GRIDCRAFT_WEB_PORT` moves the published port.

`nginx/default.conf` is the nginx form of the hosting requirements in
[`packaging/web/README.md`](../web/README.md). `smoke-test.sh` checks a running server — this one or
your own — for them:

```sh
packaging/docker/smoke-test.sh http://localhost:8771
```

It fails on a wrong MIME type, a missing or wrong `Cache-Control`, an asset the page asks for that
404s, or a `.wasm` that is not gzipped. Mind the HTTPS rule from that README: WebGPU and the clipboard
need a secure context, so `http://localhost` is fine while a plain-HTTP LAN address such as
`http://192.168.1.5:8771` serves the app with the WebGL2 fallback.

## The CLI, and MCP over stdio

```sh
docker build -f packaging/docker/cli.Dockerfile -t gridcraft-cli .
docker run --rm -v "$PWD:/data" gridcraft-cli convert /data/book.xlsx /data/out.csv
docker run --rm -v "$PWD:/data" gridcraft-cli eval '=SUM(B2:B9)' --in /data/book.xlsx
docker run --rm -v "$PWD:/data" gridcraft-cli cat /data/book.xlsx --range A1:F20
docker run --rm -v "$PWD:/data" gridcraft-cli info /data/book.xlsx --json
```

The image builds from the committed `Cargo.lock` (`--locked`) and carries no fonts: `CRAFT_FONTS_DIR`
is read by the UI crate's build.rs (the web build bakes CJK faces into its wasm with it), while the
CLI links the engine and needs the repository alone. It runs as root so that a bind-mounted
directory stays writable — pass `--user "$(id -u):$(id -g)"` to run as yourself instead, which needs
the mount writable by that user.

`mcp` speaks JSON-RPC over stdio, so a client starts the container itself and must keep stdin open
(`-i`):

```sh
claude mcp add gridcraft -- docker run --rm -i gridcraft-cli mcp
```

or, for a client configured by JSON:

```json
{
  "mcpServers": {
    "gridcraft": {
      "command": "docker",
      "args": ["run", "--rm", "-i", "gridcraft-cli", "mcp"]
    }
  }
}
```

Give it a workbook with `-v "$PWD:/data"` and `--in /data/book.xlsx`, or `--sample budget`, as in
[docs/cli.md](../../docs/cli.md) with host paths rewritten to `/data`. The `send`/`--connect` control
channel is different: it talks to a running desktop app over TCP on the host, which a container cannot
reach by default — the file, formula and MCP paths need no such link.
