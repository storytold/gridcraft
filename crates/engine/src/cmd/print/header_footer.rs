use super::layout::SheetLayout;
use super::{Font, Page, PdfDoc, Rect, Value, Workbook};

/// Side margin of headers and footers, in points, when Align with Page Margins is off.
pub(super) const HF_FIXED_SIDE_MARGIN: f32 = 0.7 * 72.0;

// ---------------------------------------------------------------- header / footer

/// Values for header/footer codes.
#[derive(Clone, Debug, Default)]
pub struct HfContext {
    pub page: usize,
    pub pages: usize,
    pub sheet: String,
    pub file: String,
    pub path: String,
    pub date: String,
    pub time: String,
}

/// Today's date and the current time as headers and footers show them (`&D`, `&T`).
pub fn hf_date_time(wb: &Workbook) -> (String, String) {
    let now = gridcraft_core::date::serial_in_system(wb.date_system, gridcraft_calc::now_serial());
    let date = crate::display::format(&Value::Number(now.floor()), "m/d/yyyy", wb).text;
    let time = crate::display::format(&Value::Number(now), "h:mm AM/PM", wb).text;
    (date, time)
}

/// Header/footer codes and the field names the Page Layout view shows for them while editing.
pub const HF_FIELDS: [(char, &str); 8] = [
    ('P', "&[Page]"),
    ('N', "&[Pages]"),
    ('D', "&[Date]"),
    ('T', "&[Time]"),
    ('Z', "&[Path]"),
    ('F', "&[File]"),
    ('A', "&[Tab]"),
    ('G', "&[Picture]"),
];

/// Splits a header/footer string into its raw left, centre and right sections (codes kept).
/// Text before the first `&L`/`&C`/`&R` belongs to the centre, as when printing.
pub fn split_header_footer(spec: &str) -> [String; 3] {
    let mut out: [String; 3] = Default::default();
    let mut cur = 1usize;
    let chars: Vec<char> = spec.chars().take(10_000).collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let next = chars.get(i + 1).copied();
        let sec = cur.min(2);
        match (c, next) {
            ('&', Some('L' | 'l')) => cur = 0,
            ('&', Some('C' | 'c')) => cur = 1,
            ('&', Some('R' | 'r')) => cur = 2,
            ('&', Some('"')) => {
                // &"Font,Style" is kept whole.
                let end = chars[i + 2..].iter().position(|ch| *ch == '"').map_or(chars.len(), |p| i + 3 + p);
                out[sec].extend(&chars[i..end.min(chars.len())]);
                i = end;
                continue;
            }
            ('&', Some(n)) => {
                out[sec].push('&');
                out[sec].push(n);
            }
            ('&', None) => {}
            (ch, _) => {
                out[sec].push(ch);
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    out
}

/// Joins raw left, centre and right sections into one header/footer string.
pub fn join_header_footer(sections: &[String; 3]) -> String {
    let mut s = String::new();
    for (code, text) in ["&L", "&C", "&R"].iter().zip(sections) {
        if !text.is_empty() {
            s.push_str(code);
            s.push_str(text);
        }
    }
    s
}

/// A raw section as the user edits it: `&P` shows as `&[Page]`, `&N` as `&[Pages]`, and so on.
pub fn codes_to_fields(raw: &str) -> String {
    let mut out = String::new();
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        // A dangling `&` at the end is dropped (printing ignores it too).
        if let Some(n) = it.next() {
            match HF_FIELDS.iter().find(|(code, _)| *code == n.to_ascii_uppercase()) {
                Some((_, field)) => out.push_str(field),
                None => {
                    out.push('&');
                    out.push(n);
                }
            }
        }
    }
    out
}

/// The inverse of [`codes_to_fields`]: field names become codes again; a dangling `&` at the end
/// becomes a literal ampersand so it can't swallow the next section's code.
pub fn fields_to_codes(text: &str) -> String {
    let chars: Vec<char> = text.chars().take(10_000).collect();
    let mut out = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c != '&' {
            out.push(c);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&'&') {
            out.push_str("&&");
            i += 2;
            continue;
        }
        let rest: String = chars[i..].iter().take(12).collect();
        if let Some((code, field)) = HF_FIELDS.iter().find(|(_, f)| rest.starts_with(f)) {
            out.push('&');
            out.push(*code);
            i += field.chars().count();
            continue;
        }
        if i + 1 == chars.len() {
            out.push_str("&&");
        } else {
            out.push('&');
        }
        i += 1;
    }
    out
}

/// One header/footer section: its text and whether `&B` made it bold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HfSection {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub size: Option<f32>,
    /// Where `&G` puts the section's picture: a char index into `text` (the first `&G` only).
    pub picture: Option<usize>,
}

/// Most bytes one section's expanded text may hold. A code like `&A` adds the whole sheet name
/// (or file path) each time it appears, and a JSON workbook's names have no length limit.
const MAX_SECTION_BYTES: usize = 64 * 1024;

/// Appends `s` unless the section would pass [`MAX_SECTION_BYTES`]; what doesn't fit is left out.
fn push_field(text: &mut String, s: &str) {
    if text.len().saturating_add(s.len()) <= MAX_SECTION_BYTES {
        text.push_str(s);
    }
}

/// Expands a header/footer string into its left, centre and right sections.
pub fn expand_header_footer(spec: &str, cx: &HfContext) -> [HfSection; 3] {
    let mut out: [HfSection; 3] = Default::default();
    let mut cur = 1usize;
    let chars: Vec<char> = spec.chars().take(10_000).collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '&' {
            if let Some(s) = out.get_mut(cur) {
                s.text.push(c);
            }
            i += 1;
            continue;
        }
        let Some(&code) = chars.get(i + 1) else { break };
        i += 2;
        let sec = &mut out[cur.min(2)];
        match code {
            'L' | 'l' => cur = 0,
            'C' | 'c' => cur = 1,
            'R' | 'r' => cur = 2,
            'P' | 'p' => {
                // &P, &P+n, &P-n
                let mut n = cx.page as i64;
                if let Some(&sign) = chars.get(i)
                    && (sign == '+' || sign == '-')
                {
                    let digits: String = chars[i + 1..].iter().take_while(|d| d.is_ascii_digit()).take(6).collect();
                    if let Ok(d) = digits.parse::<i64>() {
                        n = if sign == '+' { n + d } else { n - d };
                        i += 1 + digits.len();
                    }
                }
                push_field(&mut sec.text, &n.to_string());
            }
            'N' | 'n' => push_field(&mut sec.text, &cx.pages.to_string()),
            'D' | 'd' => push_field(&mut sec.text, &cx.date),
            'T' | 't' => push_field(&mut sec.text, &cx.time),
            'A' | 'a' => push_field(&mut sec.text, &cx.sheet),
            'F' | 'f' => push_field(&mut sec.text, &cx.file),
            'Z' | 'z' => push_field(&mut sec.text, &cx.path),
            '&' => sec.text.push('&'),
            'B' | 'b' => sec.bold = !sec.bold,
            'I' | 'i' => sec.italic = !sec.italic,
            '"' => {
                // &"Font,Style": skip the font name; honour Bold/Italic in the style.
                let rest: String = chars[i..].iter().take_while(|c| **c != '"').collect();
                let lower = rest.to_ascii_lowercase();
                if lower.contains("bold") {
                    sec.bold = true;
                }
                if lower.contains("italic") || lower.contains("oblique") {
                    sec.italic = true;
                }
                i += rest.chars().count() + 1;
            }
            d if d.is_ascii_digit() => {
                let digits: String = std::iter::once(d).chain(chars[i..].iter().copied().take_while(char::is_ascii_digit)).take(3).collect();
                i += digits.len() - 1;
                if let Ok(v) = digits.parse::<f32>() {
                    sec.size = Some(v.clamp(1.0, 409.0));
                }
            }
            'G' | 'g' => {
                if sec.picture.is_none() {
                    sec.picture = Some(sec.text.chars().count());
                }
            }
            // Underline, strike, super/subscript, colour: not rendered.
            'K' | 'k' => {
                i = (i + 6).min(chars.len());
            }
            _ => {}
        }
    }
    out
}

/// A header or footer picture ready to print: where it goes, its printed size in points (before
/// Scale with Document) and the document image it draws.
pub(super) struct HfImage {
    pub page: gridcraft_model::HfPage,
    pub footer: bool,
    pub section: usize,
    pub width: f32,
    pub height: f32,
    pub id: gridcraft_pdf::ImageId,
}

/// Largest header/footer picture decoded, in bytes of the encoded file and in pixels.
const HF_MAX_BYTES: usize = 16 * 1024 * 1024;
const HF_MAX_SIDE_PX: u32 = 8192;
/// Decoded header/footer pixels (RGBA bytes) one export may hold, all sheets together.
const HF_DECODE_BUDGET: usize = 128 * 1024 * 1024;

/// The header and footer pictures of a sheet (#1257), not decoded: where each goes, its printed
/// size in points and its encoded bytes.
pub fn hf_pictures(sh: &gridcraft_model::Sheet) -> &[gridcraft_model::HfPicture] {
    &sh.print.hf_pictures
}

/// Header and footer pictures decoded for one export (#1257): each distinct image is decoded
/// once and added once to the PDF, so every page and every sheet that shows it draws the same
/// image object, and all of them share one budget of decoded bytes.
pub(super) struct HfImageCache {
    /// Decoded bytes left for the export.
    budget: usize,
    /// What became of each image asked for: its encoded bytes, the size in pixels it was asked
    /// for, and the outcome.
    decoded: Vec<Decoded>,
}

/// A header/footer image asked for: its encoded bytes, the size in pixels, and the outcome.
type Decoded = (std::sync::Arc<Vec<u8>>, (u32, u32), Outcome);

/// What decoding a header/footer image for a size gave.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Outcome {
    /// Decoded and added to the PDF.
    Image(gridcraft_pdf::ImageId),
    /// It can't be decoded within the limits at any size: not tried again.
    Unreadable,
    /// Its decoded pixels wouldn't fit the budget left (checked before decoding): a smaller
    /// size may still fit.
    OverBudget,
}

impl HfImageCache {
    /// A cache for one export, with the whole decode budget.
    pub(super) fn new() -> HfImageCache {
        HfImageCache::with_budget(HF_DECODE_BUDGET)
    }

    /// A cache with `budget` decoded bytes to spend.
    pub(super) fn with_budget(budget: usize) -> HfImageCache {
        HfImageCache { budget, decoded: vec![] }
    }

    /// The PDF image of `data` decoded for at least `size` pixels: one already decoded that
    /// large, or at its own size (the same bytes, from this sheet or another), or a new decode
    /// within the budget.
    /// An image that can't be decoded is tried once; one that didn't fit the budget is tried
    /// again only for a smaller size (the budget only shrinks).
    pub(super) fn image(&mut self, doc: &mut PdfDoc, data: &std::sync::Arc<Vec<u8>>, size: (u32, u32)) -> Option<gridcraft_pdf::ImageId> {
        let same = |d: &std::sync::Arc<Vec<u8>>| std::sync::Arc::ptr_eq(d, data) || d == data;
        let covers = |(w, h): (u32, u32)| w >= size.0 && h >= size.1;
        for (d, at, outcome) in &self.decoded {
            if !same(d) {
                continue;
            }
            match outcome {
                Outcome::Image(id) if covers(*at) => return Some(*id),
                Outcome::Unreadable => return None,
                // Asked for at most this size before and it didn't fit: neither will this.
                Outcome::OverBudget if at.0 <= size.0 && at.1 <= size.1 => return None,
                _ => {}
            }
        }
        let mut at = size;
        let outcome = match decode_limited(data, size.0, size.1, self.budget) {
            Err(failed) => failed,
            Ok((rgba, pw, ph)) => match self.budget.checked_sub(rgba.len()) {
                None => Outcome::OverBudget,
                Some(left) => match doc.add_image(&rgba, [pw, ph]) {
                    Some(id) => {
                        self.budget = left;
                        // Not shrunk: the source's own pixels, which serve any larger size too
                        // (images are never enlarged).
                        if crate::io::image_size(data) == Some((pw, ph)) {
                            at = (u32::MAX, u32::MAX);
                        }
                        Outcome::Image(id)
                    }
                    None => Outcome::Unreadable,
                },
            },
        };
        self.decoded.push((data.clone(), at, outcome));
        match outcome {
            Outcome::Image(id) => Some(id),
            _ => None,
        }
    }

    /// How many decodes it has tried.
    #[cfg(test)]
    pub(super) fn tries(&self) -> usize {
        self.decoded.len()
    }
}

/// The header and footer pictures sheet `sh` prints (#1257), from `cache`. Only pictures a page
/// prints are decoded: their section has `&G` in the header or footer of a page kind one of the
/// sheet's printed pages uses (`kinds`). An image several sections show is decoded once, at the
/// largest size it prints. Pictures that can't be decoded within the limits, or past the
/// export's budget, are left out, as if their `&G` weren't there.
pub(super) fn hf_images(
    sh: &gridcraft_model::Sheet,
    scale: f32,
    kinds: &[gridcraft_model::HfPage],
    doc: &mut PdfDoc,
    cache: &mut HfImageCache,
) -> Vec<HfImage> {
    use gridcraft_model::HfPage;
    let pr = &sh.print;
    let has_g = |page: HfPage, footer: bool, section: usize| {
        let spec = pr.hf(page, footer);
        !spec.is_empty() && expand_header_footer(spec, &HfContext::default()).get(section).is_some_and(|s| s.picture.is_some())
    };
    let pictures: Vec<_> = hf_pictures(sh).iter().filter(|p| kinds.contains(&p.page) && has_g(p.page, p.footer, usize::from(p.section))).collect();
    // About 150 dpi at the printed size is plenty.
    let px = |pt: f32| ((pt * scale.max(0.01) / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32;
    let mut out = Vec::with_capacity(pictures.len());
    // The pixels a picture needs: its box, shrunk to the image's proportions (an image is never
    // enlarged, so one smaller than its box needs its own size).
    let need = |width: f32, height: f32, data: &[u8]| {
        let fit = (px(width), px(height));
        match crate::io::image_size(data) {
            Some(src) if src.0 > 0 && src.1 > 0 => {
                if src.0 <= fit.0 && src.1 <= fit.1 {
                    src
                } else {
                    thumbnail_size(src, fit)
                }
            }
            _ => fit,
        }
    };
    for p in pictures.iter().copied() {
        // Equal bytes are one image, as the cache counts them, even from separate parts: decoded
        // once, for the largest size any of them needs (a wide box and a tall one for the same
        // square image need two small squares, not one large one).
        let users = pictures.iter().filter(|q| std::sync::Arc::ptr_eq(&q.data, &p.data) || q.data[..] == p.data[..]);
        let size = users.fold((1, 1), |(w, h), q| {
            let (qw, qh) = need(q.width, q.height, &q.data);
            (w.max(qw), h.max(qh))
        });
        // When that largest size doesn't fit the budget left, this picture's own size may.
        let own = need(p.width, p.height, &p.data);
        if let Some(id) = cache.image(doc, &p.data, size).or_else(|| if own == size { None } else { cache.image(doc, &p.data, own) }) {
            out.push(HfImage { page: p.page, footer: p.footer, section: usize::from(p.section), width: p.width, height: p.height, id });
        }
    }
    out
}

/// The decoded pixels of `data`, shrunk to fit `max_w` × `max_h`, within the decode limits and
/// `budget` bytes. `Unreadable` when it can't be decoded within the limits; `OverBudget`, before
/// any decoding, when its pixels at that size wouldn't fit `budget`.
pub(super) fn decode_limited(data: &[u8], max_w: u32, max_h: u32, budget: usize) -> Result<(Vec<u8>, u32, u32), Outcome> {
    if data.len() > HF_MAX_BYTES {
        return Err(Outcome::Unreadable);
    }
    let (iw, ih) = crate::io::image_size(data).ok_or(Outcome::Unreadable)?;
    if iw == 0 || ih == 0 || iw > HF_MAX_SIDE_PX || ih > HF_MAX_SIDE_PX || u64::from(iw) * u64::from(ih) > 16_000_000 {
        return Err(Outcome::Unreadable);
    }
    let (max_w, max_h) = (max_w.max(1), max_h.max(1));
    let (ow, oh) = if iw > max_w || ih > max_h { thumbnail_size((iw, ih), (max_w, max_h)) } else { (iw, ih) };
    let bytes = u64::from(ow).checked_mul(u64::from(oh)).and_then(|p| p.checked_mul(4));
    if bytes.is_none_or(|b| usize::try_from(b).map_or(true, |b| b > budget)) {
        return Err(Outcome::OverBudget);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().map_err(|_| Outcome::Unreadable)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(HF_MAX_SIDE_PX);
    limits.max_image_height = Some(HF_MAX_SIDE_PX);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|_| Outcome::Unreadable)?;
    let img = if img.width() > max_w || img.height() > max_h { img.thumbnail(max_w, max_h) } else { img };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok((rgba.into_raw(), w, h))
}

/// The size `image`'s `thumbnail` gives an `iw` × `ih` image shrunk to fit `max_w` × `max_h`:
/// the aspect ratio kept, each side rounded to the nearest pixel and at least 1.
pub(super) fn thumbnail_size((iw, ih): (u32, u32), (max_w, max_h): (u32, u32)) -> (u32, u32) {
    let ratio = (f64::from(max_w) / f64::from(iw.max(1))).min(f64::from(max_h) / f64::from(ih.max(1)));
    // Both sides are at most HF_MAX_SIDE_PX here, so the rounded values fit u32.
    let side = |v: u32| ((f64::from(v) * ratio).round().clamp(1.0, f64::from(u32::MAX))) as u32;
    (side(iw), side(ih))
}

/// One line of a section: its text around the picture (all of it in `before` when the line has
/// none), and whether the picture sits between the two.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HfLine {
    pub before: String,
    pub after: String,
    pub picture: bool,
}

/// A section's lines (at most 20). With `with_picture`, the line where `&G` was is split
/// around it; without (no picture to draw), `&G` takes no room. Printing and the Page Layout
/// view lay sections out from these.
pub fn section_lines(sec: &HfSection, with_picture: bool) -> Vec<HfLine> {
    let mut start = 0usize;
    let mut out = vec![];
    for line in sec.text.split('\n').take(20) {
        let n = line.chars().count();
        match sec.picture.filter(|p| with_picture && *p >= start && *p <= start + n).map(|p| p - start) {
            Some(k) => {
                let (b, a) = line.split_at(line.char_indices().nth(k).map_or(line.len(), |(j, _)| j));
                out.push(HfLine { before: b.to_string(), after: a.to_string(), picture: true });
            }
            None => out.push(HfLine { before: line.to_string(), after: String::new(), picture: false }),
        }
        start += n + 1;
    }
    out
}

/// The page geometry headers and footers are laid out in, in points: the paper, the header and
/// footer margins, the side margins sections line up with, and the text scale (the print scale
/// with Scale with Document on, else 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HfFrame {
    pub page_w: f32,
    pub page_h: f32,
    pub header_margin: f32,
    pub footer_margin: f32,
    pub sides: (f32, f32),
    pub text_scale: f32,
}

/// One thing to draw in a header or footer, in points from the page's top-left corner.
#[derive(Clone, Debug, PartialEq)]
pub enum HfItem {
    /// A run of text: `baseline` is the text's baseline; `size` is in points (scaled).
    Text { x: f32, baseline: f32, size: f32, bold: bool, italic: bool, text: String },
    /// The picture of a section, drawn in the box.
    Picture { x: f32, y: f32, width: f32, height: f32, footer: bool, section: usize },
}

/// Lays out the header and footer of a page of kind `which` (#1257): each section's lines, the
/// picture inline where its `&G` is, the header growing down from the header margin and the
/// footer up from the footer margin. `picture(footer, section)` gives a section's picture size
/// in points (before the text scale), or `None` when it has none to draw (its `&G` then takes no
/// room). Printing draws these items; the print preview draws them in miniature.
pub fn layout_header_footer(
    frame: &HfFrame,
    settings: &gridcraft_model::PrintSettings,
    which: gridcraft_model::HfPage,
    cx: &HfContext,
    picture: impl Fn(bool, usize) -> Option<(f32, f32)>,
) -> Vec<HfItem> {
    let HfFrame { page_w: pw, page_h: ph, header_margin: hm, footer_margin: fm, sides: (l, r), text_scale } = *frame;
    let mut out = vec![];
    for (spec, header) in [(settings.hf(which, false), true), (settings.hf(which, true), false)] {
        if spec.is_empty() {
            continue;
        }
        let secs = expand_header_footer(spec, cx);
        for (i, sec) in secs.iter().enumerate() {
            let pic = sec
                .picture
                .and_then(|_| picture(!header, i))
                .filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0)
                .map(|(w, h)| (w * text_scale, h * text_scale));
            if sec.text.trim().is_empty() && pic.is_none() {
                continue;
            }
            let size = sec.size.unwrap_or(10.0).clamp(1.0, 72.0) * text_scale;
            let font = Font::styled(gridcraft_pdf::Family::Sans, sec.bold, sec.italic);
            let lines = section_lines(sec, pic.is_some());
            // Text sits this far above the bottom of its line (as it always has).
            let descent = if header { 0.4 } else { 0.22 } * size;
            let line_h = |ln: &HfLine| match pic {
                Some((_, h)) if ln.picture => h.max(size * 1.2),
                _ => size * 1.2,
            };
            let total: f32 = lines.iter().map(line_h).sum();
            // Headers grow down from the header margin, footers up from the footer margin.
            let mut top = if header { hm } else { ph - fm - total };
            let text = |x: f32, baseline: f32, t: &str| HfItem::Text { x, baseline, size, bold: sec.bold, italic: sec.italic, text: t.to_string() };
            for ln in &lines {
                let lh = line_h(ln);
                let bw = gridcraft_pdf::text_width(font, size, &ln.before);
                let shown = pic.filter(|_| ln.picture);
                let pic_w = shown.map_or(0.0, |(w, _)| w);
                let lw = bw + pic_w + if ln.after.is_empty() { 0.0 } else { gridcraft_pdf::text_width(font, size, &ln.after) };
                let x = match i {
                    0 => l,
                    1 => (pw - lw) / 2.0,
                    _ => pw - r - lw,
                };
                let base = top + lh - descent;
                if !ln.before.is_empty() {
                    out.push(text(x, base, &ln.before));
                }
                if let Some((w, h)) = shown {
                    out.push(HfItem::Picture { x: x + bw, y: top + lh - h, width: w, height: h, footer: !header, section: i });
                }
                if !ln.after.is_empty() {
                    out.push(text(x + bw + pic_w, base, &ln.after));
                }
                top += lh;
            }
        }
    }
    out
}

/// Draws the header and footer of page kind `which` on `pg`, as `layout_header_footer` lays
/// them out, each picture from `images`.
pub(super) fn draw_header_footer(pg: &mut Page, lay: &SheetLayout, which: gridcraft_model::HfPage, cx: &HfContext, images: &[HfImage]) {
    let image = |footer: bool, section: usize| images.iter().find(|im| im.page == which && im.footer == footer && im.section == section);
    let items = layout_header_footer(&lay.hf_frame(), &lay.settings, which, cx, |f, s| image(f, s).map(|im| (im.width, im.height)));
    for item in items {
        match item {
            HfItem::Text { x, baseline, size, bold, italic, text } => {
                pg.text(x, baseline, size, Font::styled(gridcraft_pdf::Family::Sans, bold, italic), [0, 0, 0], &text);
            }
            HfItem::Picture { x, y, width, height, footer, section } => {
                if let Some(im) = image(footer, section) {
                    pg.image_shared(Rect::new(x, y, width, height), im.id);
                }
            }
        }
    }
}
