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

nginx example:

```nginx
location /gridcraft/ {
    types { application/wasm wasm; text/javascript js; text/html html; }
    gzip on;
    gzip_types application/wasm text/javascript text/html;
    location ~* \.(wasm|js)$ { add_header Cache-Control "public, max-age=31536000, immutable"; }
    location ~* index\.html$ { add_header Cache-Control "no-cache"; }
}
```

Local test: `python3 -m http.server 8765` inside the folder, then open http://localhost:8765/.

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

### Driving it from the page (`?host=parent`)

A page on the **same origin** can host GridCraft as its spreadsheet editor: add `?host=parent`
(and optionally `&author=<name>` for new notes and comments) to the iframe `src`. The app then
starts empty, opens what the page sends and hands every save back instead of downloading it.
Messages are plain objects posted with `postMessage` to/from the page's own origin:

| Message | Direction | Fields |
|---|---|---|
| `gridcraft:ready` | to the page | `version`: listening; send a workbook now |
| `gridcraft:open` | from the page | `name`, `bytes` (`ArrayBuffer` or `Uint8Array`): open it like File › Open |
| `gridcraft:save` | to the page | `name`, `bytes`: Save, Save As and Export as PDF, instead of a download |
| `gridcraft:dirty` | to the page | `dirty`: unsaved changes appeared or went away |
| `gridcraft:failed` | to the page | `error`: the app couldn't start |
| `gridcraft:command` | from the page | `id`, `method`, `params`: any method of the [control protocol](../../docs/control-protocol.md) |
| `gridcraft:result` | to the page | `id`, `ok`, `result` or `error`: one per command |
| `gridcraft:selection` | to the page | `sheet`, `active` (`B6`), `ranges` (`["B6:C9"]`): when a workbook opens and whenever the selection or the active sheet changes |

Commands run on the next frame, in order, as if the user had given them, so they are undoable and
mark the workbook dirty. For example, write a value and select the cell:

```js
frame.contentWindow.postMessage({ type: "gridcraft:command", id: 1, method: "engine.execute",
  params: { command: "cell.set", params: { cell: "B6", input: "5320984863" } } }, location.origin);
frame.contentWindow.postMessage({ type: "gridcraft:command", id: 2, method: "engine.execute",
  params: { command: "selection.set", params: { range: "B6" } } }, location.origin);
```

`ui.screenshot` isn't available in the browser (it answers with an error); everything else in
the control protocol is. At most 1,000 commands wait between two frames; more are refused.

## Renderer selection

GridCraft's web build is the same egui app as the desktop one, compiled to wasm
(`apps/gridcraft-web`). It renders with the backend `apps/gridcraft-web` selects (WebGPU
where available, otherwise WebGL2). If the web app adds URL flags to force a backend (as the
sibling Craft apps do with `?webgl` / `?cpu`), document them here; they also work on the
iframe `src`.

A browser with neither WebGPU nor WebGL2 gets a message in place of the app.
