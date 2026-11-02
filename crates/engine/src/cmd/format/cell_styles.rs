use super::{
    ACCOUNTING, BorderLine, BorderStyle, Borders, COMMA, Color, Fill, Json, NumFmt, Result, Session, Style, Theme, apply_style, bad, str_param,
};

/// Built-in cell styles (our own palette).
pub fn builtin_cell_style(name: &str, theme: &Theme) -> Option<Style> {
    let _ = theme;
    let mut s = Style::default();
    let accent = |i: u8, tint: i16| Color::Theme(i, tint);
    match name {
        "Normal" => {}
        "Good" => {
            s.fill = Fill::solid(Color::rgb(0xC6, 0xEF, 0xCE));
            s.font.color = Color::rgb(0x00, 0x61, 0x00);
        }
        "Bad" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xC7, 0xCE));
            s.font.color = Color::rgb(0x9C, 0x00, 0x06);
        }
        "Neutral" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xEB, 0x9C));
            s.font.color = Color::rgb(0x9C, 0x57, 0x00);
        }
        "Calculation" => {
            s.fill = Fill::solid(Color::rgb(0xF2, 0xF2, 0xF2));
            s.font.color = Color::rgb(0xFA, 0x7D, 0x00);
            s.font.bold = true;
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0x7F, 0x7F, 0x7F) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Check Cell" => {
            s.fill = Fill::solid(Color::rgb(0xA5, 0xA5, 0xA5));
            s.font.color = Color::rgb(0xFF, 0xFF, 0xFF);
            s.font.bold = true;
            let l = BorderLine { style: BorderStyle::Double, color: Color::rgb(0x3F, 0x3F, 0x3F) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Explanatory Text" => {
            s.font.italic = true;
            s.font.color = Color::rgb(0x7F, 0x7F, 0x7F);
        }
        "Input" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xCC, 0x99));
            s.font.color = Color::rgb(0x3F, 0x3F, 0x76);
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0x7F, 0x7F, 0x7F) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Linked Cell" => {
            s.font.color = Color::rgb(0xFA, 0x7D, 0x00);
            s.borders.bottom = BorderLine { style: BorderStyle::Double, color: Color::rgb(0xFF, 0x80, 0x01) };
        }
        "Note" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xFF, 0xCC));
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0xB2, 0xB2, 0xB2) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Output" => {
            s.fill = Fill::solid(Color::rgb(0xF2, 0xF2, 0xF2));
            s.font.color = Color::rgb(0x3F, 0x3F, 0x3F);
            s.font.bold = true;
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0x3F, 0x3F, 0x3F) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Warning Text" => s.font.color = Color::rgb(0xFF, 0x00, 0x00),
        "Title" => {
            s.font.size = 18.0;
            s.font.color = Color::Theme(3, 0);
            s.font.name = theme.major_font.clone();
        }
        "Heading 1" => {
            s.font.size = 15.0;
            s.font.bold = true;
            s.font.color = Color::Theme(3, 0);
            s.borders.bottom = BorderLine { style: BorderStyle::Thick, color: accent(4, 0) };
        }
        "Heading 2" => {
            s.font.size = 13.0;
            s.font.bold = true;
            s.font.color = Color::Theme(3, 0);
            s.borders.bottom = BorderLine { style: BorderStyle::Thick, color: accent(4, 500) };
        }
        "Heading 3" => {
            s.font.bold = true;
            s.font.color = Color::Theme(3, 0);
            s.borders.bottom = BorderLine { style: BorderStyle::Medium, color: accent(4, 400) };
        }
        "Heading 4" => {
            s.font.bold = true;
            s.font.color = Color::Theme(3, 0);
        }
        "Total" => {
            s.font.bold = true;
            s.borders.top = BorderLine { style: BorderStyle::Thin, color: accent(4, 0) };
            s.borders.bottom = BorderLine { style: BorderStyle::Double, color: accent(4, 0) };
        }
        "Currency" => s.num_fmt = NumFmt::new(ACCOUNTING),
        "Comma" => s.num_fmt = NumFmt::new(COMMA),
        "Percent" => s.num_fmt = NumFmt::new("0%"),
        "Currency [0]" => s.num_fmt = NumFmt::new("_(\"$\"* #,##0_);_(\"$\"* \\(#,##0\\);_(\"$\"* \"-\"_);_(@_)"),
        "Comma [0]" => s.num_fmt = NumFmt::new("_(* #,##0_);_(* \\(#,##0\\);_(* \"-\"_);_(@_)"),
        n => {
            // AccentN, 20%/40%/60% - AccentN
            let (pct, rest) = match n.split_once("% - ") {
                Some((p, r)) => (p.parse::<i16>().ok(), r),
                None => (None, n),
            };
            let i: u8 = rest.strip_prefix("Accent")?.parse().ok()?;
            if !(1..=6).contains(&i) {
                return None;
            }
            match pct {
                None => {
                    s.fill = Fill::solid(accent(3 + i, 0));
                    s.font.color = Color::Theme(0, 0);
                }
                Some(p) => {
                    s.fill = Fill::solid(accent(3 + i, (1000 - p * 10).clamp(0, 1000)));
                    s.font.color = if p >= 60 { Color::Theme(0, 0) } else { Color::Theme(1, 0) };
                }
            }
        }
    }
    Some(s)
}

pub const CELL_STYLE_NAMES: &[&str] = &[
    "Normal",
    "Bad",
    "Good",
    "Neutral",
    "Calculation",
    "Check Cell",
    "Explanatory Text",
    "Input",
    "Linked Cell",
    "Note",
    "Output",
    "Warning Text",
    "Heading 1",
    "Heading 2",
    "Heading 3",
    "Heading 4",
    "Title",
    "Total",
    "20% - Accent1",
    "20% - Accent2",
    "20% - Accent3",
    "20% - Accent4",
    "20% - Accent5",
    "20% - Accent6",
    "40% - Accent1",
    "40% - Accent2",
    "40% - Accent3",
    "40% - Accent4",
    "40% - Accent5",
    "40% - Accent6",
    "60% - Accent1",
    "60% - Accent2",
    "60% - Accent3",
    "60% - Accent4",
    "60% - Accent5",
    "60% - Accent6",
    "Accent1",
    "Accent2",
    "Accent3",
    "Accent4",
    "Accent5",
    "Accent6",
    "Comma",
    "Comma [0]",
    "Currency",
    "Currency [0]",
    "Percent",
];

/// The built-in style called `name` (in any case), as [`CELL_STYLE_NAMES`] spells it.
pub fn builtin_style_name(name: &str) -> Option<&'static str> {
    CELL_STYLE_NAMES.iter().find(|b| b.eq_ignore_ascii_case(name)).copied()
}

/// The cell style called `name` in `wb`: the workbook's own definition (a custom style, or a
/// built-in it changed), else the built-in one. Names are compared ignoring case.
pub fn named_cell_style(wb: &gridcraft_model::Workbook, name: &str) -> Option<Style> {
    wb.cell_styles.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, s)| s.clone()).or_else(|| builtin_cell_style(name, &wb.theme))
}

/// What Merge Styles makes of a workbook's named styles ([`merge_cell_styles`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleMerge {
    /// The target's named styles after the merge.
    pub styles: Vec<(String, Style)>,
    /// Custom styles the target didn't have.
    pub added: Vec<String>,
    /// Styles whose definition in the target became the source's (a built-in the source leaves
    /// as it comes goes back to the built-in definition).
    pub replaced: Vec<String>,
    /// Styles both define differently that the target kept (`keep_same`).
    pub kept: Vec<String>,
}

impl StyleMerge {
    /// Whether the merge changes the target's styles.
    pub fn changes(&self) -> bool {
        !self.added.is_empty() || !self.replaced.is_empty()
    }
}

/// Home › Cell Styles › Merge Styles: the source's named styles merged into the target's.
///
/// Every style of the source comes over. A style both define (names compared ignoring case) takes
/// the source's definition, or keeps the target's with `keep_same`; styles only the target defines
/// stay. Each workbook has every built-in style, so the source's version of each built-in
/// replaces the target's: one the source changed comes over, and one the target changed but the
/// source didn't goes back to the built-in definition, as Excel does it. A style whose definition
/// is the same in both counts as neither replaced nor kept.
///
/// `theme` is the target's, for the built-in definitions. The target's styles keep their order,
/// and the ones it gains follow in the source's.
pub fn merge_cell_styles(target: &[(String, Style)], source: &[(String, Style)], theme: &Theme, keep_same: bool) -> StyleMerge {
    let key = |name: &str| name.to_ascii_lowercase();
    // The first of several styles with the same name is the one the workbook uses.
    let index = |list: &[(String, Style)]| {
        let mut map = std::collections::HashMap::with_capacity(list.len());
        for (i, (name, _)) in list.iter().enumerate() {
            map.entry(key(name)).or_insert(i);
        }
        map
    };
    let builtin = |name: &str| builtin_style_name(name).and_then(|b| builtin_cell_style(b, theme));
    let (in_target, in_source) = (index(target), index(source));
    let mut m = StyleMerge::default();
    for (i, (name, style)) in target.iter().enumerate() {
        let k = key(name);
        if in_target.get(&k) != Some(&i) {
            continue;
        }
        let theirs = in_source.get(&k).and_then(|&j| source.get(j));
        let Some(their_style) = theirs.map(|(_, s)| s.clone()).or_else(|| builtin(name)) else {
            // A custom style only the target has.
            m.styles.push((name.clone(), style.clone()));
            continue;
        };
        if their_style == *style {
            m.styles.push((name.clone(), style.clone()));
        } else if keep_same {
            m.kept.push(name.clone());
            m.styles.push((name.clone(), style.clone()));
        } else {
            m.replaced.push(name.clone());
            // Without a definition of its own in the source, the built-in definition comes back.
            m.styles.extend(theirs.cloned());
        }
    }
    for (j, (name, style)) in source.iter().enumerate() {
        let k = key(name);
        if in_source.get(&k) != Some(&j) || in_target.contains_key(&k) {
            continue;
        }
        match builtin(name) {
            None => {
                m.added.push(name.clone());
                m.styles.push((name.clone(), style.clone()));
            }
            Some(ours) if ours == *style => {}
            Some(_) if keep_same => m.kept.push(name.clone()),
            Some(_) => {
                m.replaced.push(name.clone());
                m.styles.push((name.clone(), style.clone()));
            }
        }
    }
    m
}

pub(super) fn cell_style(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").unwrap_or("Normal").to_string();
    let d = s.doc()?;
    let style = named_cell_style(&d.wb, &name);
    let Some(style) = style else { return Err(bad("home.cellStyle", format!("unknown cell style `{name}`"))) };
    // Number-format styles only change the number format.
    let only_fmt = matches!(name.as_str(), "Comma" | "Comma [0]" | "Currency" | "Currency [0]" | "Percent");
    apply_style(s, p, move |st| {
        if only_fmt {
            st.num_fmt = style.num_fmt.clone();
        } else {
            let keep_fmt = st.num_fmt.clone();
            *st = style.clone();
            if style.num_fmt.as_str() == "General" {
                st.num_fmt = keep_fmt;
            }
        }
    })
}
