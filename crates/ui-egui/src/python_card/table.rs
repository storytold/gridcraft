//! The card's table: a virtualised grid over the bounded preview a Python Object cell keeps
//! ([`PyTable`]). Only the cells in view are laid out, so the worst table costs what a small one does.
//! A sheet that runs right to left has its columns from the right, like the card it is in.

use std::sync::Arc;

use egui::{Align, Label, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use gridcraft_engine::core::Value;
use gridcraft_engine::model::PyTable;

use crate::i18n::Language;
use crate::theme::{self, Tokens};

const ROW_HEIGHT: f32 = 18.0;
const MIN_COLUMN: f32 = 44.0;
const MAX_COLUMN: f32 = 220.0;
const CELL_PADDING: f32 = 5.0;
/// The rows measured for a column's width, and the characters of each text.
const MEASURED_ROWS: usize = 50;
const MEASURED_CHARS: usize = 40;
/// How much wider a column's name is, in bold, than the same text in a cell.
const NAME_WIDENING: f32 = 1.25;
/// The characters of a text the grid lays out: the rest of a cell of 4 KiB would never show.
const SHOWN_CHARS: usize = 400;

/// About how wide a text is at the grid's size, without laying it out: this runs for hundreds of
/// cells when a table first shows. A column isn't wider than [`MAX_COLUMN`] whatever it holds.
fn rough_width(text: &str) -> f32 {
    text.chars().take(MEASURED_CHARS).map(|c| if c.is_ascii() { 6.6 } else { 11.5 }).sum()
}

fn column_widths(table: &PyTable) -> Vec<f32> {
    (0..table.cols)
        .map(|col| {
            let name = table.columns.get(col).map_or(0.0, |n| rough_width(n) * NAME_WIDENING);
            let cells = (0..table.rows.min(MEASURED_ROWS)).map(|row| rough_width(&table.cell_text(row, col)));
            (cells.fold(name, f32::max) + 2.0 * CELL_PADDING).clamp(MIN_COLUMN, MAX_COLUMN)
        })
        .collect()
}

/// The widths, worked out once per table: the table is the same until the cell runs again.
fn widths(ui: &Ui, table: &Arc<PyTable>) -> Arc<Vec<f32>> {
    let id = egui::Id::unique(("python_card_widths", Arc::as_ptr(table) as usize));
    ui.ctx().data_mut(|d| d.get_temp_mut_or_insert_with(id, || Arc::new(column_widths(table))).clone())
}

/// A text on one line, at most [`SHOWN_CHARS`] characters: the work is bounded by that, and a line
/// break shows as a space.
fn one_line(text: &str) -> String {
    text.chars().take(SHOWN_CHARS).map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// One cell's text, inside `rect`, cut with an ellipsis when it doesn't fit; hovering shows more.
fn paint_cell(ui: &mut Ui, rect: Rect, text: &str, color: egui::Color32, font: egui::FontId, at_end: bool) {
    let layout = if at_end { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(vec2(CELL_PADDING, 0.0))).layout(layout));
    child.add(Label::new(RichText::new(one_line(text)).font(font).color(color)).truncate());
}

/// The grid of `table`, in a scroll area at most `max_height` tall.
pub(super) fn grid(ui: &mut Ui, table: &Arc<PyTable>, t: &Tokens, rtl: bool, max_height: f32) {
    let widths = widths(ui, table);
    let total_width: f32 = widths.iter().sum();
    let header = if table.columns.is_empty() { 0.0 } else { ROW_HEIGHT + 1.0 };
    let total_height = header + table.rows as f32 * ROW_HEIGHT;
    let scroll = egui::ScrollArea::both()
        .id_salt(("python_card_table", Arc::as_ptr(table) as usize))
        .max_height(max_height)
        .auto_shrink([false, true])
        // Its first column is at the right, so that is where it starts.
        .stick_to_right(rtl);
    scroll.show_viewport(ui, |ui, viewport| {
        let width = total_width.max(ui.available_width());
        let (rect, _) = ui.allocate_exact_size(vec2(width, total_height), Sense::hover());
        // Each column's left and right edges, in order; mirrored when the sheet runs right to left.
        let mut left = 0.0;
        let spans: Vec<(f32, f32)> = widths
            .iter()
            .map(|w| {
                let span = if rtl { (rect.right() - left - w, rect.right() - left) } else { (rect.left() + left, rect.left() + left + w) };
                left += w;
                span
            })
            .collect();
        let in_view = |span: &(f32, f32)| span.1 - rect.left() >= viewport.min.x && span.0 - rect.left() <= viewport.max.x;
        let columns: Vec<usize> = spans.iter().enumerate().filter(|(_, s)| in_view(s)).map(|(i, _)| i).collect();
        let first = (((viewport.min.y - header) / ROW_HEIGHT).floor().max(0.0)) as usize;
        let last = (((viewport.max.y - header) / ROW_HEIGHT).ceil().max(0.0) as usize).min(table.rows);
        let line = Stroke::new(1.0, t.separator);
        let font = theme::ui_font(12.0);

        for row in first..last {
            let top = rect.top() + header + row as f32 * ROW_HEIGHT;
            for &col in &columns {
                let Some(&(l, r)) = spans.get(col) else { continue };
                let cell = Rect::from_min_max(pos2(l, top), pos2(r, top + ROW_HEIGHT));
                let value = table.cell(row, col);
                let label = col == 0 && table.row_labels;
                let color = match value {
                    Some(Value::Error(_)) => t.danger,
                    _ if label => t.text_dim,
                    _ => t.text,
                };
                // Numbers sit at the far end of their cell, as on a sheet, which mirrors them too.
                let number = !label && matches!(value, Some(Value::Number(_)));
                paint_cell(ui, cell, &table.cell_text(row, col), color, font.clone(), number != rtl);
            }
            ui.painter().hline(rect.x_range(), top + ROW_HEIGHT, line);
        }
        for &col in &columns {
            let Some(&(l, r)) = spans.get(col) else { continue };
            let inner = if rtl { l } else { r };
            let y = rect.top()..=rect.bottom();
            ui.painter().vline(inner, y, line);
        }

        // The header stays in view: it is painted over the rows that scroll under it.
        if header > 0.0 {
            let top = rect.top() + viewport.min.y.max(0.0);
            let band = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), ROW_HEIGHT));
            ui.painter().rect_filled(band, 0.0, t.hover);
            ui.painter().hline(rect.x_range(), band.bottom(), line);
            for &col in &columns {
                let (Some(&(l, r)), Some(name)) = (spans.get(col), table.columns.get(col)) else { continue };
                let cell = Rect::from_min_max(pos2(l, top), pos2(r, top + ROW_HEIGHT));
                paint_cell(ui, cell, name, t.text, theme::ui_bold(12.0), rtl);
            }
        }
    });
}

/// What the person must know about a table that isn't all there, or `None` when it is.
pub(super) fn cut_note(lang: Language, table: &PyTable) -> Option<String> {
    if !table.cut() {
        return None;
    }
    let mut note = Vec::new();
    if table.rows_cut() {
        let text = lang.tr("Only the first {rows} of {total} rows are shown.");
        note.push(text.replace("{rows}", &table.rows.to_string()).replace("{total}", &table.total_rows.to_string()));
    }
    if table.cols_cut() {
        let text = lang.tr("Only the first {cols} of {total} columns are shown.");
        note.push(text.replace("{cols}", &table.data_cols().to_string()).replace("{total}", &table.total_cols.to_string()));
    }
    if table.text_cut {
        note.push(lang.tr("Some long text is cut.").to_string());
    }
    note.push(lang.tr("Copying and saving take only what is shown. Spill Here gives the whole table.").to_string());
    Some(note.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: usize, cols: usize, text: &str) -> PyTable {
        PyTable { rows, cols, data: vec![Value::text(text); rows * cols], total_rows: rows as u64, total_cols: cols as u64, ..PyTable::default() }
    }

    #[test]
    fn a_column_is_as_wide_as_it_needs_within_its_limits() {
        assert_eq!(column_widths(&table(1, 2, "")), vec![MIN_COLUMN, MIN_COLUMN]);
        assert_eq!(column_widths(&table(1, 1, &"x".repeat(100_000))), vec![MAX_COLUMN], "the measure stops early");
        let some = column_widths(&table(1, 1, "twenty characters at"));
        assert!(some[0] > MIN_COLUMN && some[0] < MAX_COLUMN, "{some:?}");
        // A name counts, and a row past those measured doesn't.
        let named = PyTable { columns: vec!["a much longer column name".into()], ..table(1, 1, "x") };
        assert!(column_widths(&named)[0] > 100.0);
        let mut late = table(MEASURED_ROWS + 1, 1, "");
        late.data[MEASURED_ROWS] = Value::text("x".repeat(200));
        assert_eq!(column_widths(&late), vec![MIN_COLUMN]);
        assert!(column_widths(&PyTable::default()).is_empty());
    }

    #[test]
    fn a_long_text_shows_on_one_line_with_its_length_bounded() {
        assert_eq!(one_line("a\nb\tc"), "a b c");
        assert_eq!(one_line(&"é".repeat(100_000)).chars().count(), SHOWN_CHARS);
    }

    #[test]
    fn the_note_says_what_is_missing() {
        let lang = Language::En;
        let whole = table(2, 2, "x");
        assert_eq!(cut_note(lang, &whole), None);
        let rows = PyTable { total_rows: 1000, ..whole.clone() };
        let note = cut_note(lang, &rows).unwrap();
        assert!(note.starts_with("Only the first 2 of 1000 rows are shown. Copying and saving"), "{note}");
        let both = PyTable { total_rows: 1000, total_cols: 50, text_cut: true, ..whole.clone() };
        let note = cut_note(lang, &both).unwrap();
        assert!(note.contains("2 of 1000 rows") && note.contains("2 of 50 columns") && note.contains("long text"), "{note}");
        // Row labels aren't one of the object's columns.
        let labels = PyTable { row_labels: true, cols: 3, data: vec![Value::Empty; 6], total_cols: 50, ..whole };
        assert!(cut_note(lang, &labels).unwrap().contains("first 2 of 50 columns"));
    }
}
