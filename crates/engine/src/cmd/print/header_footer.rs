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
                sec.text.push_str(&n.to_string());
            }
            'N' | 'n' => sec.text.push_str(&cx.pages.to_string()),
            'D' | 'd' => sec.text.push_str(&cx.date),
            'T' | 't' => sec.text.push_str(&cx.time),
            'A' | 'a' => sec.text.push_str(&cx.sheet),
            'F' | 'f' => sec.text.push_str(&cx.file),
            'Z' | 'z' => sec.text.push_str(&cx.path),
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
pub(super) const HF_DECODE_BUDGET: usize = 128 * 1024 * 1024;

/// The header and footer pictures a sheet keeps from the file it was read from (#1257), not
/// decoded: where each goes, its printed size in points and its encoded bytes.
pub fn hf_pictures(sh: &gridcraft_model::Sheet) -> Vec<gridcraft_xlsx::HfPicture> {
    sh.preserved.as_deref().map(gridcraft_xlsx::header_footer_pictures).unwrap_or_default()
}

/// The header and footer pictures sheet `sh` prints (#1257), each distinct image decoded once
/// and added once to `doc`, so every page draws the same image object. Only pictures a page
/// prints are decoded: their section has `&G` in the header or footer of a page kind the sheet
/// uses (odd pages always; the first page and even pages when their options are on). Pictures
/// that can't be decoded within the limits, or past `budget` (decoded bytes left for the
/// export), are left out, as if their `&G` weren't there.
pub(super) fn hf_images(sh: &gridcraft_model::Sheet, scale: f32, doc: &mut PdfDoc, budget: &mut usize) -> Vec<HfImage> {
    use gridcraft_model::HfPage;
    let pr = &sh.print;
    let used = |page: HfPage| match page {
        HfPage::Odd => true,
        HfPage::First => pr.different_first,
        HfPage::Even => pr.different_odd_even,
    };
    let has_g = |page: HfPage, footer: bool, section: usize| {
        let spec = pr.hf(page, footer);
        !spec.is_empty() && expand_header_footer(spec, &HfContext::default()).get(section).is_some_and(|s| s.picture.is_some())
    };
    let pictures: Vec<_> = hf_pictures(sh).into_iter().filter(|p| used(p.page) && has_g(p.page, p.footer, p.section)).collect();
    // About 150 dpi at the printed size is plenty.
    let px = |pt: f32| ((pt * scale.max(0.01) / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32;
    let same = |a: &gridcraft_xlsx::HfPicture, b: &gridcraft_xlsx::HfPicture| std::sync::Arc::ptr_eq(&a.data, &b.data);
    let mut ids: Vec<Option<gridcraft_pdf::ImageId>> = Vec::with_capacity(pictures.len());
    for (i, p) in pictures.iter().enumerate() {
        // An image several sections show is decoded once, at the largest size it prints.
        if let Some(first) = pictures.iter().position(|q| same(p, q))
            && first < i
        {
            ids.push(ids.get(first).copied().flatten());
            continue;
        }
        let users = pictures.iter().filter(|q| same(p, q));
        let (w, h) = users.fold((1, 1), |(w, h), q| (w.max(px(q.width)), h.max(px(q.height))));
        let id = decode_limited(&p.data, w, h).and_then(|(rgba, pw, ph)| {
            *budget = budget.checked_sub(rgba.len())?;
            doc.add_image(&rgba, [pw, ph])
        });
        ids.push(id);
    }
    pictures
        .iter()
        .zip(ids)
        .filter_map(|(p, id)| Some(HfImage { page: p.page, footer: p.footer, section: p.section, width: p.width, height: p.height, id: id? }))
        .collect()
}

/// Decodes an encoded picture to RGBA within fixed limits, downscaled to at most `max_w`×`max_h`.
fn decode_limited(data: &[u8], max_w: u32, max_h: u32) -> Option<(Vec<u8>, u32, u32)> {
    if data.len() > HF_MAX_BYTES {
        return None;
    }
    let (iw, ih) = crate::io::image_size(data)?;
    if iw == 0 || ih == 0 || iw > HF_MAX_SIDE_PX || ih > HF_MAX_SIDE_PX || u64::from(iw) * u64::from(ih) > 16_000_000 {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(HF_MAX_SIDE_PX);
    limits.max_image_height = Some(HF_MAX_SIDE_PX);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let img = if img.width() > max_w.max(1) || img.height() > max_h.max(1) { img.thumbnail(max_w.max(1), max_h.max(1)) } else { img };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Some((rgba.into_raw(), w, h))
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

pub(super) fn draw_header_footer(pg: &mut Page, lay: &SheetLayout, which: gridcraft_model::HfPage, cx: &HfContext, images: &[HfImage]) {
    let [_, _, _, _, hm, fm] = lay.margins;
    let (l, r) = lay.hf_sides();
    let text_scale = lay.hf_text_scale();
    let pw = lay.page_w;
    let ph = lay.page_h;
    for (spec, header) in [(lay.settings.hf(which, false), true), (lay.settings.hf(which, true), false)] {
        if spec.is_empty() {
            continue;
        }
        let secs = expand_header_footer(spec, cx);
        for (i, sec) in secs.iter().enumerate() {
            let image = sec.picture.and_then(|_| images.iter().find(|im| im.page == which && im.footer == !header && im.section == i));
            if sec.text.trim().is_empty() && image.is_none() {
                continue;
            }
            let size = sec.size.unwrap_or(10.0).clamp(1.0, 72.0) * text_scale;
            let font = Font::styled(gridcraft_pdf::Family::Sans, sec.bold, sec.italic);
            let lines = section_lines(sec, image.is_some());
            let pic = image.map(|im| (im, im.width * text_scale, im.height * text_scale));
            // Text sits this far above the bottom of its line (as it always has).
            let descent = if header { 0.4 } else { 0.22 } * size;
            let line_h = |ln: &HfLine| match pic {
                Some((_, _, h)) if ln.picture => h.max(size * 1.2),
                _ => size * 1.2,
            };
            let total: f32 = lines.iter().map(line_h).sum();
            // Headers grow down from the header margin, footers up from the footer margin.
            let mut top = if header { hm } else { ph - fm - total };
            for ln in &lines {
                let lh = line_h(ln);
                let bw = gridcraft_pdf::text_width(font, size, &ln.before);
                let shown = pic.filter(|_| ln.picture);
                let pic_w = shown.map_or(0.0, |(_, w, _)| w);
                let lw = bw + pic_w + if ln.after.is_empty() { 0.0 } else { gridcraft_pdf::text_width(font, size, &ln.after) };
                let x = match i {
                    0 => l,
                    1 => (pw - lw) / 2.0,
                    _ => pw - r - lw,
                };
                let base = top + lh - descent;
                if !ln.before.is_empty() {
                    pg.text(x, base, size, font, [0, 0, 0], &ln.before);
                }
                if let Some((im, w, h)) = shown {
                    pg.image_shared(Rect::new(x + bw, top + lh - h, w, h), im.id);
                }
                if !ln.after.is_empty() {
                    pg.text(x + bw + pic_w, base, size, font, [0, 0, 0], &ln.after);
                }
                top += lh;
            }
        }
    }
}
