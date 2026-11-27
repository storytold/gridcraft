#!/usr/bin/env python3
"""Render the GridCraft Nightly icon set from assets/app-icon/gridcraft-nightly.svg.

The stable set is made by packaging/icons.sh (resvg + `cargo xtask ico` + iconutil). This does the
same for the Nightly channel, with the same geometry: Windows and Linux use the full-bleed tile,
macOS uses the 824/1024 padded tile. Every frame is rendered at its own size; nothing is upscaled.

Needs one vector renderer: `resvg` (what icons.sh uses), else a headless Chrome
(`chrome-headless-shell` or `chromium`/`google-chrome`) drawing the SVG on a transparent page at
device scale 1. The `.ico` and `.icns` containers are written here so the set needs no macOS.

Usage: packaging/nightly-icons.py [--renderer auto|resvg|chrome] [--check]

--check verifies the committed files still match assets/app-icon/nightly-provenance.json and that
each PNG/ICO/ICNS member has the size its slot says, without rendering anything.
"""
import argparse
import hashlib
import html
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
ICONS = ROOT / "assets" / "app-icon"
SVG = ICONS / "gridcraft-nightly.svg"
# The macOS tile is the full-bleed 512-square viewBox widened so the body fills 824 of 1024 px.
MAC_VIEWBOX = 'viewBox="-62.136 -62.136 636.272 636.272"'
ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)
# The ICNS slots iconutil writes, and macOS still reads.
ICNS_SLOTS = (
    ("icp4", 16), ("icp5", 32), ("ic11", 32), ("icp6", 64), ("ic12", 64), ("ic07", 128),
    ("ic08", 256), ("ic13", 256), ("ic09", 512), ("ic14", 512), ("ic10", 1024),
)
MAC_PNG_SIZES = (16, 32, 64, 128, 256, 512, 1024)
CHROME_NAMES = ("chrome-headless-shell", "chromium", "chromium-browser", "google-chrome", "google-chrome-stable")


def die(message):
    raise SystemExit(f"nightly-icons: {message}")


def renderer_argv(kind):
    if kind in ("auto", "resvg"):
        found = shutil.which("resvg")
        if found:
            return ("resvg", found)
        if kind == "resvg":
            die("resvg not found (brew install resvg / cargo install resvg)")
    for name in CHROME_NAMES:
        found = shutil.which(name)
        if found:
            return ("chrome", found)
    die("no vector renderer found: install resvg, or a headless Chrome")


def render_resvg(resvg, svg_text, size, out):
    with tempfile.TemporaryDirectory() as work:
        source = Path(work) / "icon.svg"
        source.write_text(svg_text)
        subprocess.run([resvg, "-w", str(size), "-h", str(size), str(source), str(out)], check=True, stdin=subprocess.DEVNULL)


def render_chrome(chrome, svg_text, size, out):
    # The SVG's width/height attributes fix the document size; the page is exactly that.
    start = svg_text.index("<svg")
    end = svg_text.index(">", start)
    tag = svg_text[start:end]
    for attribute in (' width="512"', ' height="512"'):
        tag = tag.replace(attribute, "")
    tag = f'{tag} width="{size}" height="{size}"'
    page = (
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>"
        "html,body{margin:0;padding:0;background:transparent;overflow:hidden}svg{display:block}"
        "</style></head><body>"
        + svg_text[:start] + tag + svg_text[end:]
        + "</body></html>"
    )
    with tempfile.TemporaryDirectory() as work:
        document = Path(work) / "icon.html"
        document.write_text(page)
        subprocess.run(
            [chrome, "--headless", "--disable-gpu", "--hide-scrollbars", "--no-first-run",
             "--default-background-color=00000000", "--force-device-scale-factor=1",
             f"--window-size={size},{size}", f"--screenshot={out}", document.as_uri()],
            check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )


def png_size(data):
    if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        die("renderer wrote something that is not a PNG")
    return struct.unpack(">II", data[16:24])


def has_alpha(data):
    # IHDR colour type 6 is RGBA; a renderer that dropped alpha would flatten the padding.
    return data[25] == 6


def pack_ico(pngs):
    body = bytearray(struct.pack("<HHH", 0, 1, len(pngs)))
    offset = 6 + 16 * len(pngs)
    for data in pngs:
        width, height = png_size(data)
        if width != height or width > 256:
            die(f"ico frames must be square and at most 256 px, got {width}x{height}")
        body += struct.pack("<BBBBHHII", 0 if width == 256 else width, 0 if width == 256 else width, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    for data in pngs:
        body += data
    return bytes(body)


def pack_icns(members):
    body = bytearray()
    for kind, data in members:
        body += kind.encode("ascii") + struct.pack(">I", len(data) + 8) + data
    return b"icns" + struct.pack(">I", len(body) + 8) + bytes(body)


def mac_svg(svg_text):
    if 'viewBox="0 0 512 512"' not in svg_text:
        die("gridcraft-nightly.svg must keep the full-bleed 0 0 512 512 viewBox")
    return svg_text.replace('viewBox="0 0 512 512"', MAC_VIEWBOX)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def render_all(kind):
    renderer, program = renderer_argv(kind)
    svg_text = SVG.read_text()
    padded = mac_svg(svg_text)
    with tempfile.TemporaryDirectory() as work:
        work = Path(work)

        def draw(source, size, name):
            if not source.strip():
                die(f"empty SVG for {name}")
            out = work / "tile.svg"
            out.write_text(source)
            (render_resvg if renderer == "resvg" else render_chrome)(program, source, size, work / name)
            data = (work / name).read_bytes()
            if png_size(data) != (size, size):
                die(f"{name}: expected {size}x{size}, got {png_size(data)}")
            if not has_alpha(data):
                die(f"{name}: the render has no alpha channel")
            return data

        # Full-bleed tile: the Linux tar icon (256) and the Windows ICO frames.
        tile = {size: draw(svg_text, size, f"tile-{size}.png") for size in sorted({256} | set(ICO_SIZES))}
        (ICONS / "gridcraft-nightly.png").write_bytes(tile[256])
        (ICONS / "gridcraft-nightly.ico").write_bytes(pack_ico([tile[size] for size in ICO_SIZES]))
        # Padded macOS tile: the 1024 master, the ICNS slots and the runtime Dock render.
        mac = {size: draw(padded, size, f"mac-{size}.png") for size in MAC_PNG_SIZES}
        (ICONS / "gridcraft-nightly-1024.png").write_bytes(mac[1024])
        (ICONS / "gridcraft-nightly.icns").write_bytes(pack_icns([(kind, mac[size]) for kind, size in ICNS_SLOTS]))
    write_provenance(renderer)


def write_provenance(renderer):
    outputs = ("gridcraft-nightly.png", "gridcraft-nightly-1024.png", "gridcraft-nightly.ico", "gridcraft-nightly.icns")
    provenance = {
        "title": "GridCraft nightly deep-background icons",
        "author": "@skylartaylor",
        "license": "MIT OR Apache-2.0",
        "inputs": [
            {"path": "assets/app-icon/gridcraft-nightly.svg", "sha256": digest(SVG), "purpose": "vector master for every frame"},
            {"path": "assets/app-icon/gridcraft.svg", "sha256": digest(ICONS / "gridcraft.svg"), "purpose": "stable artwork this is derived from"},
        ],
        "method": {
            "kind": "vector render of the project-owned artwork with only its field colour deepened",
            "stable_field": "#1f9d55",
            "nightly_field": "#14532d",
            "unchanged": "ink #0b0b0c, paper #efe9dc, every path, stroke width and the 824/1024 Mac padding",
            "sizes": "each frame rendered at its own size, never upscaled",
            "renderer": renderer,
            "generator": "packaging/nightly-icons.py",
        },
        "outputs": {name: digest(ICONS / name) for name in outputs},
        "ico_frame_sizes": list(ICO_SIZES),
        "icns_png_slots": {kind: size for kind, size in ICNS_SLOTS},
    }
    (ICONS / "nightly-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")


def check():
    provenance = json.loads((ICONS / "nightly-provenance.json").read_text())
    ok = True
    for name, expected in provenance["outputs"].items():
        actual = digest(ICONS / name)
        if actual != expected:
            print(f"mismatch: assets/app-icon/{name} is {actual}, provenance says {expected}", file=sys.stderr)
            ok = False
    ico = (ICONS / "gridcraft-nightly.ico").read_bytes()
    count = struct.unpack("<H", ico[4:6])[0]
    if count != len(ICO_SIZES):
        print(f"mismatch: the ICO holds {count} frames, expected {len(ICO_SIZES)}", file=sys.stderr)
        ok = False
    icns = (ICONS / "gridcraft-nightly.icns").read_bytes()
    if icns[:4] != b"icns" or struct.unpack(">I", icns[4:8])[0] != len(icns):
        print("mismatch: the ICNS header does not cover the file", file=sys.stderr)
        ok = False
    return 0 if ok else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--renderer", choices=("auto", "resvg", "chrome"), default="auto")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        return check()
    render_all(args.renderer)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
