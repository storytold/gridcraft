# Vendored crates

Permissively licensed crates GridCraft carries small patches for. Each directory keeps the
upstream licence files unchanged. Every patch is marked `GridCraft patch:` in the source, is
covered by a GridCraft test, and should be proposed upstream. Remove the vendored copy once
upstream releases the fix. Vendoring copyleft code is never allowed.

| Crate | Version | Licence | Patches | Test |
|---|---|---|---|---|
| epaint | 0.36.2 | MIT OR Apache-2.0 | (1) a shaped cluster spans the text up to the next cluster start, so a right-to-left run no longer gets zero-width continuation glyphs for every character (`glyphs.len() == char_count` held only for left-to-right runs, so the caret and Backspace hit the wrong character in Hebrew); (2) `Glyph::rtl`, and `Row::x_offset` / `Row::char_at` place and hit the caret by screen position in a row whose glyphs a caller put into logical order at their screen positions (`crates/ui-egui/src/bidi.rs`). Rows without a right-to-left glyph behave as upstream. The same patch is carried by PdfCraft. | `crates/ui-egui/src/bidi.rs::egui_caret_follows_hebrew_reading_order` |
