# Hosting GridCraft for the web

`gridcraft-web-<version>.zip` (from the GitHub release, or `packaging/web/package.sh`) holds a
static site in `gridcraft-web-<version>/`:

| File | What it is |
|---|---|
| `index.html` | The page. It loads everything through relative URLs. |
| `gridcraft-web-<hash>.js` | wasm-bindgen glue (generated, ES module) |
| `gridcraft-web-<hash>_bg.wasm` | The app (compresses to roughly a third with gzip/Brotli) |
| `_headers`, `.htaccess` | Sample header rules for Netlify/Cloudflare Pages and Apache |

There is no server-side code. Upload the folder's contents anywhere that serves static files.

## Any path works

All URLs in `index.html` are relative (`public_url = "./"` in `apps/gridcraft-web/Trunk.toml`),
so the site works at a domain root (`https://example.com/`), under a prefix
(`https://example.com/tools/gridcraft/`) and from a CDN bucket. The asset names carry a content
hash, so they can be cached forever. Only `index.html` needs revalidation.

## Required server settings

- **MIME type:** serve `.wasm` as `application/wasm`. Browsers refuse to stream-compile it under
  any other type, and the app then loads slowly or not at all. Serve `.js` as `text/javascript`.
  Most hosts already do both. For nginx, check that `mime.types` has `application/wasm wasm;`.
- **Compression:** turn on gzip or Brotli for `.wasm`, `.js` and `.html`. That cuts the
  download to roughly a third. You can also precompress (`brotli -k *.wasm`) and let
  the server send `Content-Encoding: br`.
- **Caching:** `Cache-Control: public, max-age=31536000, immutable` on the hashed `.wasm` and
  `.js` files, and `no-cache` on `index.html`.
- **HTTPS:** WebGPU (and the clipboard) only work in a secure context, which means `https://`
  or `http://localhost`. Over plain HTTP elsewhere, the app falls back to WebGL2.
- **No special isolation headers:** GridCraft doesn't use `SharedArrayBuffer`, so it doesn't
  need `Cross-Origin-Opener-Policy` or `Cross-Origin-Embedder-Policy`. If your site already sends
  COEP `require-corp`, also send `Cross-Origin-Resource-Policy: same-origin` (or `cross-origin`
  when the files live on a CDN) on the app's files.

## Fonts

The web build bakes its fonts into the wasm: a browser has no system fonts to fall back on, and
egui's own defaults carry no CJK glyphs, so Chinese, Japanese and Korean text would show as empty
boxes. A build made with the optional `CRAFT_FONTS_DIR` input (see
[`storytold/craft-fonts`](https://github.com/storytold/craft-fonts) `docs/integration.md`) embeds
the Simplified-Chinese face — `packaging/web/subset-fonts.py` cuts it down to the characters a UI
needs first (15.7 MiB → 3.4 MiB), since hosts cap a single file. Widen `WEB_FONTS` in
`crates/ui-egui/build.rs` (and the script) to embed more faces if the size budget allows. The
licence of each embedded font ships beside the site as `OFL-<family>.txt`.

nginx example:

```nginx
location /gridcraft/ {
    types { application/wasm wasm; text/javascript js; text/html html; }
    gzip on;
    # text/html is always compressed; naming it here logs "duplicate MIME type" at startup.
    gzip_types application/wasm text/javascript;
    location ~* \.(wasm|js)$ { add_header Cache-Control "public, max-age=31536000, immutable"; }
    location ~* index\.html$ { add_header Cache-Control "no-cache"; }
}
```

Local test: `python3 -m http.server 8765` inside the folder, then open http://localhost:8765/.
(WebGPU needs a secure context, so use `localhost` and not your machine's IP — see
[Renderer selection](#renderer-selection).)

Self-hosting with Docker: `packaging/docker/Dockerfile` builds one image — nginx serving this build
(with `nginx/default.conf`, the settings above written out for nginx) plus the `gridcraft-cli` — and
`packaging/docker/compose.yaml` runs it. `smoke-test.sh` checks the result. See
[`packaging/docker/README.md`](../docker/README.md).

## Embedding in a page (iframe)

```html
<iframe
  src="https://example.com/gridcraft/"
  title="GridCraft spreadsheet"
  style="width: 100%; height: 720px; border: 0;"
  allow="fullscreen; clipboard-read; clipboard-write"
  allowfullscreen>
</iframe>
```

- The app fills the iframe and follows its size, so size the iframe and not the app.
- Keyboard shortcuts go to the iframe after the user clicks into it, as with any embedded app.
- **Cross-origin embeds** work. Preferences are kept in the iframe's `localStorage`. Browsers
  that partition or block third-party storage may forget them between visits, and the app
  then starts with defaults.
- **Sandboxed iframes** need at least
  `sandbox="allow-scripts allow-same-origin allow-downloads allow-popups"`. Without
  `allow-same-origin` there's no storage. Without `allow-downloads`, Save and Export (browser
  downloads) are blocked.
- Don't send `X-Frame-Options: DENY` or a `frame-ancestors` CSP that excludes the embedding page.

## Renderer selection

GridCraft's web build is the same egui app as the desktop one, compiled to wasm
(`apps/gridcraft-web`). It renders with the backend `apps/gridcraft-web` selects (WebGPU
where available, otherwise WebGL2). If the web app adds URL flags to force a backend (as the
sibling Craft apps do with `?webgl` / `?cpu`), document them here; they also work on the
iframe `src`.

A browser with neither WebGPU nor WebGL2 gets a message in place of the app.
