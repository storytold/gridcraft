use super::*;

/// Maximum source length to scan on each drag frame for a fill-handle preview.
const MAX_FILL_PREVIEW_SOURCE_CELLS: u64 = 1_000;

/// Returns up to five predicted numeric fill values without changing the workbook.
///
/// Preview modes follow the engine's fill operation, including Ctrl/Cmd alternation and source
/// number formats. Oversized selections, formulas and calendar-dependent series omit the tooltip.
pub(super) fn fill_preview_text(wb: &Workbook, sh: &Sheet, src: RangeRef, target: RangeRef, alternate: bool) -> Option<String> {
    // A single-cell source has no intrinsic direction; infer it from the expanded target.
    let vertical = if src.height() > 1 {
        true
    } else if src.width() > 1 {
        false
    } else {
        target.height() > 1
    };
    if (vertical && src.width() != 1) || (!vertical && src.height() != 1) {
        return None;
    }
    let source_len = if vertical { src.height() } else { src.width() };
    if u64::from(source_len) > MAX_FILL_PREVIEW_SOURCE_CELLS {
        // This runs on every drag frame; avoid allocating and scanning huge selections for a tooltip.
        return None;
    }
    let source: Vec<CellRef> = if vertical {
        (src.start.row..=src.end.row).map(|row| CellRef::new(row, src.start.col)).collect()
    } else {
        (src.start.col..=src.end.col).map(|col| CellRef::new(src.start.row, col)).collect()
    };
    let numbers: Vec<f64> = source
        .iter()
        .map(|cell| {
            let stored = sh.cell(*cell)?;
            if stored.formula.is_some() || !stored.value.is_number() {
                return None;
            }
            stored.value.as_f64()
        })
        .collect::<Option<_>>()?;
    let style_cell = *source.first()?;
    let format = wb.styles.get(sh.style_id(style_cell)).num_fmt.as_str();
    let temporal = matches!(
        gridcraft_engine::numfmt::NumberFormat::parse(format).kind(),
        gridcraft_engine::numfmt::FormatKind::Date | gridcraft_engine::numfmt::FormatKind::Time
    );
    if !alternate && temporal {
        // The fill engine applies calendar/time-specific rules. Do not show a numeric guess that
        // would disagree with the values committed when the drag ends.
        return None;
    }
    let (forward, steps) = if vertical {
        let forward = target.end.row > src.end.row;
        let steps = if forward { target.end.row - src.end.row } else { src.start.row.saturating_sub(target.start.row) };
        (forward, steps)
    } else {
        let forward = target.end.col > src.end.col;
        let steps = if forward { target.end.col - src.end.col } else { src.start.col.saturating_sub(target.start.col) };
        (forward, steps)
    };
    if steps == 0 {
        return None;
    }
    // Match the engine's linear-series least-squares step, including flat numeric sequences.
    let (slope, fitted_first, fitted_last) = if numbers.len() == 1 {
        (0.0, numbers[0], numbers[0]) // a lone ordinary number is copied by the default fill operation
    } else {
        let n = numbers.len() as f64;
        let mean_x = (n - 1.0) / 2.0;
        let mean_y = numbers.iter().sum::<f64>() / n;
        let (numerator, denominator) = numbers
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(num, den), (i, y)| (num + (i as f64 - mean_x) * (y - mean_y), den + (i as f64 - mean_x).powi(2)));
        let slope = if denominator == 0.0 { 0.0 } else { numerator / denominator };
        let fitted_first = mean_y - slope * mean_x;
        let fitted_last = mean_y + slope * (n - 1.0 - mean_x);
        (slope, fitted_first, fitted_last)
    };
    let base = if forward { fitted_last } else { fitted_first };
    let preview = (1..=steps.min(5))
        .map(|step| {
            let sign = if forward { 1.0 } else { -1.0 };
            let source_index = if forward { (step as usize - 1) % source.len() } else { source.len() - 1 - ((step as usize - 1) % source.len()) };
            let raw = if alternate && numbers.len() == 1 && !temporal {
                // Ctrl/Cmd swaps the normal single-number copy behavior for a +1/-1 series.
                numbers[0] + f64::from(step) * sign
            } else if alternate {
                // Multi-cell sequences are copied in alternate mode rather than extrapolated.
                numbers[source_index]
            } else {
                base + slope * f64::from(step) * sign
            };
            let mag = if raw == 0.0 || !raw.is_finite() { 0 } else { raw.abs().log10().floor() as i32 };
            let precision = 10f64.powi(14 - mag);
            let rounded = if !precision.is_finite() || precision == 0.0 { raw } else { (raw * precision).round() / precision };
            let value = Value::number(rounded);
            let target_format = wb.styles.get(sh.style_id(source[source_index])).num_fmt.as_str();
            gridcraft_engine::display::value_text_as(wb, &value, target_format)
        })
        .collect::<Vec<_>>();
    Some(preview.join("\n"))
}

#[cfg(test)]
mod fill_preview_tests {
    use super::*;

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
}
