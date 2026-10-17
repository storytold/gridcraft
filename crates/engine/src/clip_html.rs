//! Reading the HTML flavour of the system clipboard on paste.
//!
//! Spreadsheets put an HTML table on the clipboard next to the tab-separated text. The text holds
//! what each cell *shows* ("Rp10,000.00"), which en-US input parsing can't always turn back into a
//! number. The HTML of the common sources also carries the underlying number as a cell attribute:
//! `x:num="10000"` (Excel, Excel for the web) and `sdval="10000"` (LibreOffice Calc). Paste uses
//! it to keep such cells numeric (#257). Only the table structure, the cell text and these two
//! attributes are read; everything else in the markup is ignored.

/// Largest clipboard HTML we look at; past this the paste uses the plain text alone.
const MAX_HTML: usize = 16 * 1024 * 1024;
/// Widest row or `colspan` honoured (the sheet's column count).
const MAX_SPAN: usize = gridcraft_core::MAX_COLS as usize;

/// One table cell: its visible text and the number the source recorded for it, if any.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HtmlCell {
    pub text: String,
    pub num: Option<f64>,
}

/// The cells of the first table in `html`, limited to the tab-separated text's row widths.
/// A cell spanning columns is followed by empty cells so positions line up with the text.
/// Unused cells are not retained or decoded. Empty when there is no table or no text rows.
pub(crate) fn table_cells(html: &str, row_widths: impl IntoIterator<Item = usize>) -> Vec<Vec<HtmlCell>> {
    let mut rows: Vec<Vec<HtmlCell>> = Vec::new();
    let mut widths = row_widths.into_iter().peekable();
    if html.len() > MAX_HTML || widths.peek().is_none() {
        return rows;
    }
    let mut width = 0;
    let mut cell: Option<(HtmlCell, usize)> = None;
    let mut in_table = false;
    let mut rest = html;
    while !rest.is_empty() {
        if rows.last().is_some_and(|row| row.len() >= width) && widths.peek().is_none() {
            break;
        }
        let Some(lt) = rest.find('<') else {
            if let Some((c, _)) = cell.as_mut() {
                c.text.push_str(rest);
            }
            break;
        };
        if let Some((c, _)) = cell.as_mut() {
            c.text.push_str(rest.get(..lt).unwrap_or_default());
        }
        rest = rest.get(lt..).unwrap_or_default();
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").and_then(|e| after.get(e + 3..)).unwrap_or_default();
            continue;
        }
        let Some(gt) = tag_end(rest) else { break };
        let tag = rest.get(1..gt).unwrap_or_default();
        rest = rest.get(gt + 1..).unwrap_or_default();
        let closing = tag.starts_with('/');
        let body = tag.trim_start_matches('/');
        let name_end = body.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(body.len());
        let name = body.get(..name_end).unwrap_or_default().to_ascii_lowercase();
        let attrs = body.get(name_end..).unwrap_or_default();
        match (name.as_str(), closing) {
            ("style" | "script", false) => {
                // Raw text: skip to the closing tag.
                let end = format!("</{name}");
                rest = find_ci(rest, &end).and_then(|i| rest.get(i..)).unwrap_or_default();
            }
            ("table", false) => {
                if in_table {
                    break; // a nested table: the outer one is the clipboard grid, stop here
                }
                in_table = true;
            }
            ("table", true) => {
                finish(&mut rows, &mut cell, width);
                if !rows.is_empty() {
                    break;
                }
                in_table = false;
            }
            ("tr", false) if in_table => {
                finish(&mut rows, &mut cell, width);
                let Some(cols) = widths.next() else { break };
                width = cols.min(MAX_SPAN);
                rows.push(Vec::new());
            }
            ("td" | "th", false) if in_table => {
                finish(&mut rows, &mut cell, width);
                if rows.is_empty() {
                    let Some(cols) = widths.next() else { break };
                    width = cols.min(MAX_SPAN);
                    rows.push(Vec::new());
                }
                if rows.last().is_none_or(|row| row.len() >= width) {
                    continue;
                }
                let num = attr(attrs, "x:num").or_else(|| attr(attrs, "sdval")).and_then(|v| v.trim().parse::<f64>().ok()).filter(|n| n.is_finite());
                let span = attr(attrs, "colspan").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, MAX_SPAN);
                cell = Some((HtmlCell { text: String::new(), num }, span));
            }
            ("td" | "th", true) => finish(&mut rows, &mut cell, width),
            ("tr", true) => {
                finish(&mut rows, &mut cell, width);
                if !rows.is_empty() && widths.peek().is_none() {
                    break;
                }
            }
            ("br", _) => {
                if let Some((c, _)) = cell.as_mut() {
                    c.text.push(' ');
                }
            }
            _ => {}
        }
    }
    finish(&mut rows, &mut cell, width);
    rows
}

/// Closes the open cell (if any) into the last row, decoding its text.
fn finish(rows: &mut [Vec<HtmlCell>], cell: &mut Option<(HtmlCell, usize)>, width: usize) {
    let Some((mut c, span)) = cell.take() else { return };
    let Some(row) = rows.last_mut().filter(|row| row.len() < width) else { return };
    c.text = decode(&c.text);
    row.push(c);
    let room = width.saturating_sub(row.len());
    row.extend(std::iter::repeat_n(HtmlCell::default(), span.saturating_sub(1).min(room)));
}

/// Byte index of the `>` that closes the tag `rest` starts with; a `>` inside a quoted attribute
/// value (an inline `mso-number-format` condition such as `[>=1000]`) doesn't end it.
fn tag_end(rest: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in rest.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// Case-insensitive ASCII search.
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h.get(i..i + n.len()).is_some_and(|w| w.eq_ignore_ascii_case(n)))
}

/// The value of attribute `name` in a tag's attribute text (quoted or bare). A bare attribute
/// without `=` has no value.
fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut s = attrs;
    loop {
        s = s.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if s.is_empty() {
            return None;
        }
        let key_end = s.find(|c: char| c.is_whitespace() || c == '=' || c == '/').unwrap_or(s.len());
        let key = s.get(..key_end).unwrap_or_default();
        s = s.get(key_end..).unwrap_or_default().trim_start();
        let mut value = None;
        if let Some(after) = s.strip_prefix('=') {
            let after = after.trim_start();
            let (v, next) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let inner = after.get(1..).unwrap_or_default();
                    let end = inner.find(q).unwrap_or(inner.len());
                    (inner.get(..end).unwrap_or_default(), inner.get(end + 1..).unwrap_or_default())
                }
                _ => {
                    let end = after.find(char::is_whitespace).unwrap_or(after.len());
                    (after.get(..end).unwrap_or_default(), after.get(end..).unwrap_or_default())
                }
            };
            value = Some(v);
            s = next;
        }
        if key.eq_ignore_ascii_case(name) {
            return value;
        }
        if key.is_empty() && value.is_none() {
            // Stray character (e.g. a lone '='): skip it so the loop always advances.
            let mut chars = s.chars();
            chars.next();
            s = chars.as_str();
        }
    }
}

/// HTML text to plain text: entities decoded, whitespace runs (and `<br>`) collapsed to one
/// space, trimmed. The text only serves to match a cell with the tab-separated text.
fn decode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(rest.get(..amp).unwrap_or_default());
        rest = rest.get(amp..).unwrap_or_default();
        let semi = rest.get(..12).unwrap_or(rest).find(';');
        let entity = semi.and_then(|e| rest.get(1..e));
        let ch = entity.and_then(|e| match e {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => {
                let n = e
                    .strip_prefix("#x")
                    .or_else(|| e.strip_prefix("#X"))
                    .map(|h| u32::from_str_radix(h, 16))
                    .or_else(|| e.strip_prefix('#').map(str::parse::<u32>));
                n.and_then(Result::ok).and_then(char::from_u32)
            }
        });
        match (ch, semi) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = rest.get(e + 1..).unwrap_or_default();
            }
            _ => {
                out.push('&');
                rest = rest.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    let mut text = String::with_capacity(out.len());
    let mut space = false;
    for c in out.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            space = true;
            continue;
        }
        if space && !text.is_empty() {
            text.push(' ');
        }
        space = false;
        text.push(c);
    }
    text
}

/// Same-looking text: non-breaking spaces as spaces, whitespace runs collapsed, trimmed.
pub(crate) fn normalized(s: &str) -> String {
    s.replace('\u{a0}', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A number format that shows `n` exactly as `shown` ("Rp10,000.00" for 10000 →
/// `"Rp"#,##0.00`), inferred from the text around the digits. `None` unless the format renders
/// `shown` back character for character.
pub(crate) fn format_for(shown: &str, n: f64) -> Option<String> {
    let shown = normalized(shown);
    let start = shown.find(|c: char| c.is_ascii_digit())?;
    let end = shown.rfind(|c: char| c.is_ascii_digit()).map(|i| i + 1)?;
    let core = shown.get(start..end)?;
    let mut prefix = shown.get(..start)?.to_string();
    let suffix = shown.get(end..)?;
    if core.chars().any(|c| !(c.is_ascii_digit() || c == ',' || c == '.')) {
        return None;
    }
    if n < 0.0 {
        // A one-section format shows the minus sign itself, in front of everything.
        prefix = prefix.strip_prefix('-')?.to_string();
    }
    let decimals = core.split_once('.').map_or(0, |(_, f)| f.len());
    let mut code = String::new();
    let quote = |s: &str| if s.is_empty() { String::new() } else { format!("\"{}\"", s.replace('"', "")) };
    code.push_str(&quote(&prefix));
    code.push_str(if core.contains(',') { "#,##0" } else { "0" });
    if decimals > 0 {
        code.push('.');
        code.push_str(&"0".repeat(decimals.min(30)));
    }
    code.push_str(&quote(suffix));
    let rendered = gridcraft_numfmt::text_function(&gridcraft_core::Value::number(n), &code, gridcraft_core::date::DateSystem::D1900).ok()?;
    (normalized(&rendered) == shown).then_some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_excel_and_libreoffice_cell_values() {
        let excel = r#"<html><head><style>td{mso-number-format:General}</style></head><body><table>
            <!--StartFragment--><tr height=20><td class=xl65 x:num="10000">Rp10,000.00</td><td x:str>Name</td></tr>
            <tr><td colspan=2 x:num="2.5">2.50 &amp; up</td><td>x</td></tr><!--EndFragment--></table></body></html>"#;
        let rows = table_cells(excel, [2, 3]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], HtmlCell { text: "Rp10,000.00".into(), num: Some(10000.0) });
        assert_eq!(rows[0][1], HtmlCell { text: "Name".into(), num: None });
        assert_eq!(rows[1].len(), 3, "colspan keeps the next cell in its column");
        assert_eq!(rows[1][0].text, "2.50 & up");
        assert_eq!(rows[1][2].text, "x");

        let calc = r#"<table cellspacing="0" border="0"><colgroup width="85"></colgroup><tr>
            <td height="17" align="right" sdval="10000" sdnum="1033;0;[$Rp-421]#,##0.00"><font face="Liberation Sans">Rp10,000.00</font></td></tr></table>"#;
        assert_eq!(table_cells(calc, [1]), vec![vec![HtmlCell { text: "Rp10,000.00".into(), num: Some(10000.0) }]]);

        let quoted_gt = r#"<table><tr><td style='mso-number-format:"\[>=1000\]0"' x:num="1500">1500</td></tr></table>"#;
        assert_eq!(table_cells(quoted_gt, [1]), vec![vec![HtmlCell { text: "1500".into(), num: Some(1500.0) }]]);
    }

    #[test]
    fn limits_spans_and_following_cells_to_the_text_columns() {
        let html = r#"<table><tr><td colspan=8 x:num="5">Rp5</td><td x:num="7">unused</td></tr>
            <tr><td sdval="2.5">Rp2.50</td><td>unused</td></tr><tr><td>unused row</td></tr></table>"#;
        assert_eq!(
            table_cells(html, [2, 1]),
            vec![
                vec![HtmlCell { text: "Rp5".into(), num: Some(5.0) }, HtmlCell::default()],
                vec![HtmlCell { text: "Rp2.50".into(), num: Some(2.5) }]
            ]
        );
        let unfinished = r#"<table><tr><td colspan=8 x:num="5">Rp5"#;
        assert_eq!(table_cells(unfinished, [2]), vec![vec![HtmlCell { text: "Rp5".into(), num: Some(5.0) }, HtmlCell::default()]]);
    }

    #[test]
    fn respects_ragged_text_widths_including_empty_rows() {
        let html = r#"<table><tr><td x:num="1">Rp1</td><td>unused</td></tr>
            <tr><td x:num="2">unused</td></tr>
            <tr><td colspan=2 sdval="3">Rp3</td><td x:num="4">Rp4</td><td>unused</td></tr></table>"#;
        let rows = table_cells(html, [1, 0, 3]);
        assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [1, 0, 3]);
        assert_eq!(rows[0][0], HtmlCell { text: "Rp1".into(), num: Some(1.0) });
        assert_eq!(rows[2][0], HtmlCell { text: "Rp3".into(), num: Some(3.0) });
        assert_eq!(rows[2][1], HtmlCell::default());
        assert_eq!(rows[2][2], HtmlCell { text: "Rp4".into(), num: Some(4.0) });
    }

    #[test]
    fn bounds_implicit_rows_and_empty_targets() {
        let html = r#"<table><td colspan=8 x:num="1">Rp1</td><td>unused</td>
            <tr><td sdval="2">Rp2</td></tr><tr><td>unused row</td></tr></table>"#;
        assert_eq!(
            table_cells(html, [2, 1]),
            vec![vec![HtmlCell { text: "Rp1".into(), num: Some(1.0) }, HtmlCell::default()], vec![HtmlCell { text: "Rp2".into(), num: Some(2.0) }]]
        );
        assert_eq!(table_cells(html, [0, 1]), vec![vec![], vec![HtmlCell { text: "Rp2".into(), num: Some(2.0) }]]);
        assert!(table_cells(html, []).is_empty());
        assert_eq!(table_cells(html, [0]), vec![vec![]]);
        assert_eq!(table_cells("<table><tr><td>unused</td></tr></table>", [0]), vec![vec![]]);
    }

    #[test]
    fn hostile_markup_does_not_panic() {
        for html in [
            "",
            "<",
            "<td",
            "<table><td x:num=>",
            "<table><tr><td colspan=999999999>a",
            "&#xFFFFFFFF;&",
            "<table><td x:num='1e999'>1</td>",
            "<!--",
            "<style>",
            "<table><table><td>",
        ] {
            let _ = table_cells(html, [2]);
        }
        assert_eq!(table_cells("<table><td x:num='1e999'>1</td></table>", [1])[0][0].num, None);
        assert_eq!(table_cells("<table><tr><td colspan=999999999>a</td></tr></table>", [2])[0].len(), 2);
    }

    #[test]
    fn infers_a_format_that_shows_the_same_text() {
        assert_eq!(format_for("Rp10,000.00", 10000.0).as_deref(), Some("\"Rp\"#,##0.00"));
        assert_eq!(format_for("1 234 kr", 1234.0), None, "space grouping is not reproduced");
        assert_eq!(format_for("10.000,00 €", 10000.0), None, "comma decimals don't render back in en-US");
        assert_eq!(format_for("-Rp5", -5.0).as_deref(), Some("\"Rp\"0"));
        assert_eq!(format_for("12 EUR", 12.0).as_deref(), Some("0\" EUR\""));
    }
}
