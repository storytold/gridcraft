//! Fill › Justify: the text of a column's cells, reflowed to the width of the selected columns.

use gridcraft_core::{CellRef, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{Cell, Font, StyleId};

use crate::cmd::Ctx;
use crate::{EngineError, Result, TextWidth};

/// Room a cell keeps beside its text, as AutoFit counts it.
const PADDING: f32 = 6.0;
/// Lines measured exactly; past them the front end may answer with its cheaper measure.
const EXACT_WIDTHS: usize = 2_000;

/// What [`justify`] wrote.
pub struct Justified {
    /// Lines of text written.
    pub lines: usize,
    /// The cells of the first column it wrote or cleared.
    pub range: RangeRef,
}

/// One row of the result: a line of a paragraph and the style its paragraph started with, or a
/// blank row between paragraphs.
type Row = Option<(String, StyleId)>;

/// Reflows the text in the first column of `r` into lines as wide as all of `r`'s columns, from
/// `r`'s top row down. Each run of filled cells is a paragraph and blank cells stay between them;
/// a word wider than the line keeps a line of its own. The other columns of `r` only give width.
/// Lines past the bottom of `r` go on down the column; they replace cells there that hold
/// something (spilled values too) only with `overwrite`, else the error asks first. Cells holding
/// numbers, formulas or spilled values can't be justified, and no merged area may meet the cells
/// written.
pub fn justify(cx: &mut Ctx, sheet: usize, r: RangeRef, measure: Option<&TextWidth>, overwrite: bool) -> Result<Justified> {
    let sh = cx.wb.sheet(sheet).ok_or_else(|| EngineError::Other("There's no such sheet.".into()))?;
    let col = r.start.col;
    // Only rows up to the last stored one can hold text.
    let last = sh.used_range().map_or(r.start.row, |u| u.end.row.min(r.end.row)).max(r.start.row);
    let width = (r.start.col..=r.end.col).map(|c| sh.col_width(c)).sum::<f32>() - PADDING;
    // The merges in the column, found once: an empty merge holds no cell and is past the used range.
    let merged: Vec<RangeRef> = sh.merges.iter().filter(|m| !m.is_single() && m.start.col <= col && col <= m.end.col).copied().collect();

    // ---- the paragraphs: (text of each cell, style of its first cell)
    let mut source: Vec<Option<(Vec<String>, StyleId)>> = Vec::new();
    for row in r.start.row..=last {
        let at = CellRef::new(row, col);
        if sh.spill.get(&at).is_some_and(|v| !v.is_empty()) {
            return Err(not_text());
        }
        let text = match sh.cell(at) {
            Some(c) if c.formula.is_some() || c.py.is_some() => return Err(not_text()),
            Some(Cell { value: Value::Text(t), .. }) if !t.trim().is_empty() => t.to_string(),
            Some(Cell { value: Value::Empty, .. }) | None => String::new(),
            Some(Cell { value: Value::Text(_), .. }) => String::new(),
            Some(_) => return Err(not_text()),
        };
        match (source.last_mut(), text.is_empty()) {
            (_, true) => source.push(None),
            (Some(Some((texts, _))), false) => texts.push(text),
            (_, false) => source.push(Some((vec![text], sh.style_id(at)))),
        }
    }
    while matches!(source.last(), Some(None)) {
        source.pop();
    }

    // ---- the lines
    let mut measured = 0usize;
    let mut fits = |line: &str, font: &Font| {
        measured += 1;
        let w = match measure {
            Some(m) => m(line, font, measured <= EXACT_WIDTHS),
            None => crate::cmd::format::approx_text_width(line, font.size, font.bold),
        };
        w <= width
    };
    let mut rows: Vec<Row> = Vec::new();
    for paragraph in source {
        let Some((texts, style)) = paragraph else {
            rows.push(None);
            continue;
        };
        let font = cx.wb.styles.get(style).font.clone();
        let mut line = String::new();
        for word in texts.iter().flat_map(|t| t.split_whitespace()) {
            if line.is_empty() {
                line.push_str(word);
                continue;
            }
            let longer = format!("{line} {word}");
            if fits(&longer, &font) {
                line = longer;
            } else {
                rows.push(Some((std::mem::take(&mut line), style)));
                line.push_str(word);
            }
        }
        rows.push(Some((line, style)));
    }
    let lines = rows.iter().filter(|row| row.is_some()).count();
    if lines == 0 {
        return Ok(Justified { lines: 0, range: RangeRef::cell(r.start) });
    }

    // ---- where they go: from the top of the selection; the old text's rows below them clear
    let end = u64::from(r.start.row) + rows.len() as u64 - 1;
    if end >= u64::from(MAX_ROWS) {
        return Err(EngineError::Other("The justified text doesn't fit above the bottom of the sheet.".into()));
    }
    let end = end as u32;
    if end > r.end.row {
        let below = RangeRef::new(CellRef::new(r.end.row + 1, col), CellRef::new(end, col));
        let stored = sh.cells.iter_range(below).find(|(_, c)| c.has_content()).map(|(at, _)| at);
        let spilled = || sh.spill.range(below.start..=below.end).find(|(at, v)| below.contains(**at) && !v.is_empty()).map(|(at, _)| *at);
        if let Some(at) = stored.or_else(spilled).filter(|_| !overwrite) {
            return Err(EngineError::Confirm(format!(
                "The text will extend below the selection and replace what is in {}. Do you want to replace it?",
                at.a1()
            )));
        }
    }
    let written = RangeRef::new(r.start, CellRef::new(end.max(last), col));
    if merged.iter().any(|m| m.intersects(&written)) {
        return Err(EngineError::Other("Merged cells can't be justified.".into()));
    }

    let shm = cx.sheet_mut(sheet)?;
    for (i, at) in written.iter().enumerate() {
        match rows.get(i).cloned().flatten() {
            Some((text, style)) => shm.set_cell(at, Cell { value: Value::text(text.as_str()), style, ..Cell::default() }),
            // A row between paragraphs, or below the last line, keeps its format.
            None if shm.cell(at).is_some_and(Cell::has_content) => {
                let style = shm.style_id(at);
                shm.set_cell(at, Cell { style, ..Cell::default() });
            }
            None => {}
        }
    }
    for at in written.iter() {
        cx.touch(sheet, at);
    }
    Ok(Justified { lines, range: written })
}

fn not_text() -> EngineError {
    EngineError::Other("Only text can be justified: the first column of the selection holds numbers or formulas.".into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Session;

    const SENTENCE: &str = "The quick brown fox jumps over the lazy dog and keeps running far away";

    fn session() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }
    fn text(s: &Session, a: &str) -> String {
        s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(a).unwrap()).display()
    }
    /// The first column's text from row 1 down to the last filled row.
    fn column(s: &Session) -> Vec<String> {
        let mut v: Vec<String> = (1..=40).map(|r| text(s, &format!("A{r}"))).collect();
        while v.last().is_some_and(String::is_empty) {
            v.pop();
        }
        v
    }
    /// The width a line of the default font takes, as headless justify measures it.
    fn width(s: &Session, line: &str) -> f32 {
        let font = s.doc().unwrap().wb.styles.get(StyleId::DEFAULT).font.clone();
        crate::cmd::format::approx_text_width(line, font.size, font.bold)
    }

    #[test]
    fn lines_fill_the_width_of_the_selected_columns_and_keep_every_word() {
        let mut s = session();
        s.execute("cell.set", json!({"cell": "A1", "input": SENTENCE})).unwrap();
        let mut counts = vec![];
        for (range, cols) in [("A1:C6", 3.0), ("A1:A2", 1.0)] {
            s.execute("cell.set", json!({"cell": "A1", "input": SENTENCE})).unwrap();
            s.execute("edit.fillJustify", json!({"range": range})).unwrap();
            let lines = column(&s);
            let room = cols * s.doc().unwrap().wb.active().unwrap().col_width(0) - PADDING;
            for pair in lines.windows(2) {
                assert!(width(&s, &pair[0]) <= room, "{range}: {:?} is wider than the columns", pair[0]);
                let next = pair[1].split(' ').next().unwrap();
                assert!(width(&s, &format!("{} {next}", pair[0])) > room, "{range}: {next:?} would fit after {:?}", pair[0]);
            }
            assert_eq!(lines.join(" "), SENTENCE, "{range}");
            counts.push(lines.len());
            for r in 2..=lines.len() {
                s.execute("cell.set", json!({"cell": format!("A{r}"), "input": ""})).unwrap();
            }
        }
        assert!(counts[1] > counts[0], "one column takes more lines than three: {counts:?}");
    }

    #[test]
    fn blank_cells_separate_paragraphs_and_other_columns_stay() {
        let mut s = session();
        let rows = json!([["alpha beta", null], ["gamma", "in column B"], [null, null], ["delta"]]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("edit.fillJustify", json!({"range": "A1:C6"})).unwrap();
        let lines = column(&s);
        let gap = lines.iter().position(String::is_empty).expect("a blank row between the paragraphs");
        assert_eq!(lines[..gap].join(" "), "alpha beta gamma");
        assert_eq!(lines[gap + 1..], ["delta"]);
        assert_eq!(text(&s, "B2"), "in column B");
    }

    #[test]
    fn numbers_and_formulas_are_refused() {
        let mut s = session();
        for input in ["42", "=1+1"] {
            s.execute("cell.set", json!({"cell": "A1", "input": "alpha"})).unwrap();
            s.execute("cell.set", json!({"cell": "A2", "input": input})).unwrap();
            let err = s.execute("edit.fillJustify", json!({"range": "A1:C3"})).unwrap_err().to_string();
            assert!(err.contains("Only text"), "{input}: {err}");
            assert_eq!(text(&s, "A1"), "alpha");
        }
    }

    #[test]
    fn writing_over_filled_cells_below_the_selection_asks_first() {
        let mut s = session();
        s.execute("cell.set", json!({"cell": "A1", "input": SENTENCE})).unwrap();
        s.execute("cell.set", json!({"cell": "A6", "input": "keep"})).unwrap();
        let undo = s.doc().unwrap().undo.len();
        let err = s.execute("edit.fillJustify", json!({"range": "A1"})).unwrap_err();
        assert!(matches!(err, EngineError::Confirm(_)), "{err}");
        assert_eq!((text(&s, "A1"), text(&s, "A6")), (SENTENCE.to_string(), "keep".to_string()));
        let done = s.execute("edit.fillJustify", json!({"range": "A1", "overwrite": true})).unwrap();
        assert!(done["lines"].as_u64().unwrap() >= 6);
        assert_ne!(text(&s, "A6"), "keep");
        assert_eq!(s.doc().unwrap().undo.len(), undo + 1, "one undo step");
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!((text(&s, "A1"), text(&s, "A6")), (SENTENCE.to_string(), "keep".to_string()));
    }

    /// An empty merge holds no cell, so only the cells to be written show it in the way.
    #[test]
    fn a_merge_where_lines_would_go_is_refused_even_when_empty() {
        let mut s = session();
        s.execute("cell.set", json!({"cell": "A1", "input": SENTENCE})).unwrap();
        s.execute("home.mergeCells", json!({"range": "A3:B4"})).unwrap();
        let err = s.execute("edit.fillJustify", json!({"range": "A1:A10"})).unwrap_err().to_string();
        assert!(err.contains("Merged"), "{err}");
        assert_eq!(text(&s, "A1"), SENTENCE);
    }

    /// Spilled values are no stored cells, yet they are filled: in the selection they can't be
    /// justified, and below it they are asked about before lines replace them.
    #[test]
    fn spilled_values_count_as_filled() {
        let mut s = session();
        s.execute("cell.set", json!({"cell": "A3", "input": "=SEQUENCE(3,2)"})).unwrap();
        s.execute("cell.set", json!({"cell": "B1", "input": SENTENCE})).unwrap();
        let err = s.execute("edit.fillJustify", json!({"range": "B1:B6"})).unwrap_err().to_string();
        assert!(err.contains("Only text"), "{err}");
        let err = s.execute("edit.fillJustify", json!({"range": "B1"})).unwrap_err();
        assert!(matches!(err, EngineError::Confirm(_)), "{err}");
        assert_eq!(text(&s, "B1"), SENTENCE);
    }
}
