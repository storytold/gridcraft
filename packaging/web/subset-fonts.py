#!/usr/bin/env python3
"""Shrink craft-fonts to what the web build bakes into its wasm.

The web build embeds its fonts (crates/ui-egui/build.rs), and hosts cap a single file
(Cloudflare/Workers: 25 MiB), so the whole `Noto Sans CJK SC` (15.7 MiB) cannot go in. This cuts it
down to the characters a Simplified-Chinese UI and its documents need, and writes the result as a
craft-fonts checkout in miniature, so build.rs reads it unchanged.

Usage: packaging/web/subset-fonts.py <craft-fonts checkout> <output dir>

Needs: fontTools (`pip install fonttools`). Like all of craft-fonts this only runs when
CRAFT_FONTS_DIR is set; without it no build embeds any font.
"""

import os
import shutil
import sys

from fontTools import subset


def gb2312_hanzi() -> set:
    """The 6,763 hanzi of GB2312: the everyday Simplified-Chinese repertoire."""
    codes = set()
    for hi in range(0xB0, 0xF8):
        for lo in range(0xA1, 0xFF):
            try:
                codes.add(ord(bytes((hi, lo)).decode("gb2312")))
            except UnicodeDecodeError:
                continue
    return codes


def wanted_unicodes() -> set:
    """What a spreadsheet UI shows: text, numbers, formatting symbols and their punctuation."""
    codes = gb2312_hanzi()
    codes.add(0x3007)  # 〇
    for lo, hi in (
        (0x0020, 0x007E),  # ASCII
        (0x00A0, 0x00FF),  # Latin-1
        (0x2000, 0x206F),  # general punctuation — – … “ ”
        (0x20A0, 0x20BF),  # currency symbols € ¥ £
        (0x2100, 0x214F),  # letterlike ™ № ℃
        (0x2190, 0x21FF),  # arrows → ⇧
        (0x2200, 0x22FF),  # maths ± × ÷ ≤ ≥ ≈
        (0x2460, 0x24FF),  # enclosed alphanumerics ① ⑵
        (0x25A0, 0x25FF),  # geometric shapes ▪ ● ▲
        (0x2600, 0x27BF),  # symbols and dingbats ★ ☑ ✔
        (0x3000, 0x303F),  # CJK punctuation 、。 「」
        (0x3040, 0x30FF),  # kana, so a Japanese label still reads
        (0x31C0, 0x31EF),  # CJK strokes
        (0x3200, 0x33FF),  # enclosed CJK ㊙ ㍿
        (0xFE10, 0xFE4F),  # CJK compatibility forms
        (0xFF00, 0xFFEF),  # full-width forms
    ):
        codes.update(range(lo, hi + 1))
    return codes


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    src_dir = os.path.abspath(sys.argv[1])
    dst_dir = os.path.abspath(sys.argv[2])
    manifest = os.path.join(src_dir, "fonts", "manifest.txt")
    if not os.path.isfile(manifest):
        print(f"error: {manifest} not found; is {src_dir} a craft-fonts checkout?", file=sys.stderr)
        return 1

    # Keep in sync with crates/ui-egui/build.rs.
    web_fonts = {("Noto Sans CJK SC", "Regular")}
    codes = wanted_unicodes()
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.notdef_outline = True

    out_lines = [
        "# Written by packaging/web/subset-fonts.py from a full craft-fonts checkout: only the\n",
        "# faces the web build bakes in, cut down to the characters a UI and its documents need.\n",
    ]
    kept = 0
    with open(manifest, encoding="utf-8") as fh:
        for line in fh:
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            fields = [part.strip() for part in stripped.split(" | ")]
            if len(fields) < 6:
                print(f"error: malformed manifest line: {stripped}", file=sys.stderr)
                return 1
            family, style, rel, scripts, licence, licence_file = fields[:6]
            if (family, style) not in web_fonts:
                continue

            src_file = os.path.join(src_dir, rel)
            stem, ext = os.path.splitext(rel)
            out_rel = f"{stem}.subset{ext}"
            dst_file = os.path.join(dst_dir, out_rel)
            os.makedirs(os.path.dirname(dst_file), exist_ok=True)

            font = subset.load_font(src_file, options)
            subsetter = subset.Subsetter(options=options)
            subsetter.populate(unicodes=codes)
            subsetter.subset(font)
            subset.save_font(font, dst_file, options)
            font.close()

            # The licence travels with the font (craft-fonts ATTRIBUTION.md).
            shutil.copyfile(os.path.join(src_dir, licence_file), os.path.join(dst_dir, licence_file))
            out_lines.append(f"{family} | {style} | {out_rel} | {scripts} | {licence} | {licence_file} | - | subset of the craft-fonts checkout\n")
            before = os.path.getsize(src_file) / 1048576
            after = os.path.getsize(dst_file) / 1048576
            print(f"{family} {style}: {before:.1f} MiB -> {after:.1f} MiB ({out_rel})")
            kept += 1

    if kept == 0:
        print(f"error: none of {sorted(web_fonts)} is in {manifest}", file=sys.stderr)
        return 1

    with open(os.path.join(dst_dir, "fonts", "manifest.txt"), "w", encoding="utf-8") as fh:
        fh.writelines(out_lines)
    print(f"wrote {kept} subset font(s) to {dst_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
