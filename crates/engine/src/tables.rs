//! Table styles: how a cell inside a table looks (our own palettes, keyed by the familiar
//! `TableStyleLight1…21 / Medium1…28 / Dark1…11` names so files round-trip).

use gridcraft_core::CellRef;
use gridcraft_model::*;

/// (family, number) from a style name, e.g. `TableStyleMedium2` → ("Medium", 2).
pub fn parse_style_name(name: &str) -> (&'static str, u32) {
    let rest = name.strip_prefix("TableStyle").unwrap_or(name);
    for fam in ["Light", "Medium", "Dark"] {
        if let Some(n) = rest.strip_prefix(fam) {
            let k = n.parse::<u32>().unwrap_or(2).max(1); // the gallery starts at 1; 0 would underflow `n - 1`
            return (fam, k);
        }
    }
    ("Medium", 2)
}

/// Accent theme slot (4..=9) for a style number; 1 and every 7th style are neutral (dk1).
fn accent_for(n: u32) -> Option<u8> {
    let k = (n.saturating_sub(1)) % 7;
    if k == 0 { None } else { Some(3 + k as u8) }
}

/// Header/band/body colours for a table style.
pub struct TableLook {
    pub header_fill: Option<Color>,
    pub header_font: Color,
    pub header_bold: bool,
    pub band_fill: Option<Color>,
    pub body_fill: Option<Color>,
    pub line: Option<Color>,
    pub total_top: Option<Color>,
}

pub fn look(style: &str) -> TableLook {
    let (fam, n) = parse_style_name(style);
    let acc = accent_for(n);
    let base = |tint: i16| match acc {
        Some(a) => Color::Theme(a, tint),
        None => Color::Theme(1, if tint == 0 { 0 } else { tint.clamp(-1000, 1000) }),
    };
    let neutral_light = |tint: i16| match acc {
        Some(a) => Color::Theme(a, tint),
        None => Color::Theme(0, -((1000 - tint).clamp(0, 1000) / 6)),
    };
    match fam {
        "Light" => {
            let group = (n - 1) / 7; // 0: lines only, 1: banded header line, 2: grid
            TableLook {
                header_fill: None,
                header_font: if group == 0 { base(-250) } else { Color::Auto },
                header_bold: true,
                band_fill: Some(neutral_light(800)),
                body_fill: None,
                line: Some(base(0)),
                total_top: Some(base(0)),
            }
        }
        "Dark" => TableLook {
            header_fill: Some(Color::Theme(1, 0)),
            header_font: Color::Theme(0, 0),
            header_bold: true,
            band_fill: Some(base(-250)),
            body_fill: Some(base(0)),
            line: None,
            total_top: Some(Color::Theme(0, 0)),
        },
        _ => TableLook {
            header_fill: Some(base(0)),
            header_font: Color::Theme(0, 0),
            header_bold: true,
            band_fill: Some(neutral_light(800)),
            body_fill: None,
            line: if (n - 1) / 7 == 0 { None } else { Some(base(400)) },
            total_top: Some(base(0)),
        },
    }
}

/// The style a table gives a cell (merged over the cell's own style: explicit fills/fonts win).
pub fn table_cell_style(wb: &Workbook, sheet: usize, t: &Table, c: CellRef) -> Style {
    let own = wb.sheet(sheet).map(|sh| wb.styles.get(sh.style_id(c)).clone()).unwrap_or_default();
    let mut s = own.clone();
    let lk = look(&t.style);
    let is_header = t.header_row && c.row == t.range.start.row;
    let is_total = t.totals_row && c.row == t.range.end.row;
    let data_row = c.row.saturating_sub(t.range.start.row + t.header_row as u32);
    let data_col = c.col - t.range.start.col;
    let explicit_fill = own.fill.pattern != PatternType::None;
    if is_header {
        if let Some(f) = lk.header_fill
            && !explicit_fill
        {
            s.fill = Fill::solid(f);
        }
        if own.font.color == Color::Auto {
            s.font.color = lk.header_font;
        }
        s.font.bold = s.font.bold || lk.header_bold;
        if let Some(l) = lk.line {
            s.borders.bottom = BorderLine { style: BorderStyle::Thin, color: l };
        }
    } else if is_total {
        s.font.bold = true;
        if let Some(l) = lk.total_top {
            s.borders.top = BorderLine { style: BorderStyle::Double, color: l };
        }
    } else {
        let band = (t.banded_rows && data_row.is_multiple_of(2)) || (t.banded_cols && data_col.is_multiple_of(2));
        if !explicit_fill {
            if band {
                if let Some(f) = lk.band_fill {
                    s.fill = Fill::solid(f);
                }
            } else if let Some(f) = lk.body_fill {
                s.fill = Fill::solid(f);
            }
        }
        if let Some(l) = lk.line
            && t.style.contains("Light")
            && c.row == t.range.end.row
        {
            s.borders.bottom = BorderLine { style: BorderStyle::Thin, color: l };
        }
    }
    if (t.first_col && c.col == t.range.start.col) || (t.last_col && c.col == t.range.end.col) {
        s.font.bold = true;
    }
    if t.style.starts_with("TableStyleDark") && !is_header && own.font.color == Color::Auto {
        s.font.color = Color::Theme(0, 0);
    }
    s
}

/// Gallery list of style names.
pub fn gallery() -> Vec<String> {
    let mut v = Vec::new();
    for n in 1..=21 {
        v.push(format!("TableStyleLight{n}"));
    }
    for n in 1..=28 {
        v.push(format!("TableStyleMedium{n}"));
    }
    for n in 1..=11 {
        v.push(format!("TableStyleDark{n}"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_numbered_styles_do_not_underflow() {
        assert_eq!(parse_style_name("TableStyleLight0"), ("Light", 1));
        assert_eq!(parse_style_name("TableStyleMedium0"), ("Medium", 1));
        for name in ["TableStyleLight0", "TableStyleMedium0", "TableStyleDark0"] {
            let _ = look(name);
        }
    }
}
