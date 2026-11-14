use gridcraft_engine::core::RangeRef;
use gridcraft_engine::model::{Sheet, Workbook};

/// Formats the engine's bounded, read-only numeric predictions for the drag tooltip.
pub(super) fn fill_preview_text(wb: &Workbook, sh: &Sheet, src: RangeRef, target: RangeRef, alternate: bool) -> Option<String> {
    let preview = gridcraft_engine::fill::numeric_preview(wb, sh, src, target, alternate)?;
    Some(
        preview
            .iter()
            .map(|cell| {
                let format = wb.styles.get(sh.style_id(cell.source)).num_fmt.as_str();
                gridcraft_engine::display::value_text_as(wb, &cell.value, format)
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[cfg(test)]
mod fill_preview_tests {
    use super::fill_preview_text;
    use gridcraft_engine::core::{CellRef, RangeRef, Value};
    use gridcraft_engine::model::{Sheet, Style, Workbook};

    fn preview(values: &[f64], target_end: u32, alternate: bool) -> Option<String> {
        let wb = Workbook::default();
        let mut sh = Sheet::new("Sheet1");
        for (row, value) in values.iter().enumerate() {
            sh.set_value(CellRef::new(row as u32, 0), Value::number(*value));
        }
        fill_preview_text(
            &wb,
            &sh,
            RangeRef::new(CellRef::new(0, 0), CellRef::new(values.len() as u32 - 1, 0)),
            RangeRef::new(CellRef::new(0, 0), CellRef::new(target_end, 0)),
            alternate,
        )
    }

    #[test]
    fn alternate_preview_copies_multi_cell_sequences() {
        assert_eq!(preview(&[1.0, 2.0, 3.0], 5, true).as_deref(), Some("1\n2\n3"));
    }

    #[test]
    fn alternate_preview_counts_up_from_a_single_number() {
        assert_eq!(preview(&[1.0], 3, true).as_deref(), Some("2\n3\n4"));
        assert_eq!(preview(&[1.0], 3, false).as_deref(), Some("1\n1\n1"));
    }

    #[test]
    fn formats_engine_predictions_in_backward_source_order() {
        let mut wb = Workbook::default();
        let mut sh = Sheet::new("Sheet1");
        for (row, number) in [(5, 1.0), (6, 2.0), (7, 4.0)] {
            sh.set_value(CellRef::new(row, 0), Value::number(number));
        }
        let style = wb.styles.intern(Style { num_fmt: gridcraft_engine::model::NumFmt::new("0.00%"), ..Style::default() });
        sh.set_style(CellRef::new(6, 0), style);
        let src = RangeRef::new(CellRef::new(5, 0), CellRef::new(7, 0));
        let target = RangeRef::new(CellRef::new(0, 0), CellRef::new(7, 0));
        for alternate in [false, true] {
            let predictions = gridcraft_engine::fill::numeric_preview(&wb, &sh, src, target, alternate).unwrap();
            let expected = predictions
                .iter()
                .map(|p| gridcraft_engine::display::value_text_as(&wb, &p.value, wb.styles.get(sh.style_id(p.source)).num_fmt.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(fill_preview_text(&wb, &sh, src, target, alternate).as_deref(), Some(expected.as_str()));
            assert_eq!(predictions.len(), 5);
        }
        assert_eq!(fill_preview_text(&wb, &sh, src, target, true).as_deref(), Some("4\n200.00%\n1\n4\n200.00%"));
    }

    #[test]
    fn alternate_preview_copies_a_single_temporal_value() {
        let mut wb = Workbook::default();
        let mut sh = Sheet::new("Sheet1");
        let cell = CellRef::new(0, 0);
        let value = 45_292.0;
        sh.set_value(cell, Value::number(value));
        let style = wb.styles.intern(Style { num_fmt: gridcraft_engine::model::NumFmt::new("yyyy-mm-dd"), ..Style::default() });
        sh.set_style(cell, style);

        let expected = gridcraft_engine::display::value_text_as(&wb, &Value::number(value), "yyyy-mm-dd");
        assert_eq!(
            fill_preview_text(&wb, &sh, RangeRef::new(cell, cell), RangeRef::new(cell, CellRef::new(1, 0)), true).as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn oversized_source_does_not_create_a_preview() {
        let wb = Workbook::default();
        let sh = Sheet::new("Sheet1");
        let whole_column = RangeRef::cols(0, 0);
        assert!(fill_preview_text(&wb, &sh, whole_column, whole_column, false).is_none());
    }

    /// A protected sheet never promises a fill result that destination permissions could reject.
    #[test]
    fn protected_sheets_omit_numeric_previews() {
        let wb = Workbook::default();
        let mut sh = Sheet::new("Sheet1");
        sh.set_value(CellRef::new(0, 0), Value::number(1.0));
        sh.set_value(CellRef::new(1, 0), Value::number(2.0));
        let src = RangeRef::new(CellRef::new(0, 0), CellRef::new(1, 0));
        let target = RangeRef::new(CellRef::new(0, 0), CellRef::new(3, 0));
        assert_eq!(fill_preview_text(&wb, &sh, src, target, false).as_deref(), Some("3\n4"));
        sh.protection = Some(Default::default());
        assert!(fill_preview_text(&wb, &sh, src, target, false).is_none());
        assert!(fill_preview_text(&wb, &sh, src, target, true).is_none());
    }
}
