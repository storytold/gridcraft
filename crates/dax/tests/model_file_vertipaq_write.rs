//! VertiPaq writing: round trips, idempotence, container legality and value edits.
//!
//! Set `GRIDCRAFT_VERTIPAQ_FIXTURE` to an extracted Excel `xl/model/item.data` to run these
//! tests; without it they are skipped so the suite stays machine independent.

use gridcraft_dax::model_file::{ModelData, is_model_file, read_model, write_model};
use gridcraft_dax::value::Scalar;

fn fixture() -> Option<Vec<u8>> {
    let path = std::env::var("GRIDCRAFT_VERTIPAQ_FIXTURE").ok()?;
    std::fs::read(path).ok()
}

fn find<'a>(model: &'a ModelData, table: &str, column: &str) -> &'a gridcraft_dax::model_file::ModelColumnData {
    model
        .tables
        .iter()
        .find(|candidate| candidate.name == table)
        .and_then(|table| table.columns.iter().find(|candidate| candidate.name == column))
        .unwrap_or_else(|| panic!("missing {table}[{column}]"))
}

#[test]
fn write_read_round_trip_is_identical() {
    let Some(base) = fixture() else { return };
    let model = read_model(&base).expect("read");
    let written = write_model(&base, &model).expect("write");
    assert!(is_model_file(&written));
    assert_eq!(written.len() % 4096, 0, "container must be page aligned");
    let back = read_model(&written).expect("read back");
    assert_eq!(back, model, "values, types, order, relationships and measures must round trip");

    let again = write_model(&written, &back).expect("second write");
    assert_eq!(again, written, "same inputs must produce byte-identical output");
}

#[test]
fn rewritten_values_are_visible_and_original_is_untouched() {
    let Some(base) = fixture() else { return };
    let mut model = read_model(&base).expect("read");
    let Some(table) = model.tables.iter_mut().find(|table| table.name == "Sales") else { return };
    let amount = table.columns.iter_mut().find(|column| column.name == "Amount").expect("Amount");
    let before = amount.values.clone();
    for value in amount.values.iter_mut() {
        if let Scalar::Integer(number) = value {
            *number *= 2;
        }
    }
    let written = write_model(&base, &model).expect("write");
    let back = read_model(&written).expect("read back");
    let changed = &find(&back, "Sales", "Amount").values;
    for (index, (old, new)) in before.iter().zip(changed.iter()).enumerate() {
        match (old, new) {
            (Scalar::Integer(value), Scalar::Integer(updated)) => assert_eq!(updated, &(value * 2), "row {index}"),
            (Scalar::Blank, Scalar::Blank) => {}
            other => panic!("unexpected row {index}: {other:?}"),
        }
    }
    assert_eq!(find(&back, "Sales", "Region"), find(&model, "Sales", "Region"));
    // The input buffer must not have been mutated in place.
    assert_eq!(read_model(&base).expect("re-read base"), {
        let mut original = model.clone();
        let table = original.tables.iter_mut().find(|table| table.name == "Sales").expect("Sales");
        let amount = table.columns.iter_mut().find(|column| column.name == "Amount").expect("Amount");
        amount.values = before;
        original
    });
}

#[test]
fn adding_a_row_keeps_all_columns_aligned() {
    let Some(base) = fixture() else { return };
    let mut model = read_model(&base).expect("read");
    let Some(table) = model.tables.iter_mut().find(|table| table.name == "Products") else { return };
    let extra: Vec<Scalar> = table
        .columns
        .iter()
        .map(|column| match column.data_type.as_str() {
            "String" => Scalar::String("Saddle".into()),
            "Int64" => Scalar::Integer(42),
            "Double" => Scalar::Decimal(4_200.0),
            "Currency" => Scalar::Currency(420_000),
            "DateTime" => Scalar::DateTime(45000.0),
            "Boolean" => Scalar::Boolean(true),
            _ => Scalar::Blank,
        })
        .collect();
    for (column, value) in table.columns.iter_mut().zip(extra) {
        column.values.push(value.clone());
    }
    let written = write_model(&base, &model).expect("write");
    let back = read_model(&written).expect("read back");
    assert_eq!(back.tables.iter().find(|table| table.name == "Products").map(|table| table.columns[0].values.len()), Some(4));
    assert_eq!(find(&back, "Products", "Product").values[3], Scalar::String("Saddle".into()));
    assert_eq!(find(&back, "Products", "Price").values[3], Scalar::Decimal(4_200.0));
}

#[test]
fn schema_changes_are_rejected_cleanly() {
    let Some(base) = fixture() else { return };
    let mut model = read_model(&base).expect("read");
    model.tables[0].columns.pop();
    let error = write_model(&base, &model).expect_err("column removal must be rejected");
    assert!(format!("{error}").contains("invalid model"), "{error}");

    let mut model = read_model(&base).expect("read");
    if model.relationships.pop().is_some() {
        let error = write_model(&base, &model).expect_err("relationship removal must be rejected");
        assert!(format!("{error}").contains("invalid model"), "{error}");
    }
}

#[test]
fn every_sampled_truncation_of_the_rewritten_stream_is_an_error() {
    let Some(base) = fixture() else { return };
    let model = read_model(&base).expect("read");
    let written = write_model(&base, &model).expect("write");
    let mut cuts: Vec<usize> = (0..written.len().min(120)).collect();
    cuts.extend((0..=16).map(|offset| written.len().saturating_sub(offset)));
    let mut position = 4096usize;
    while position < written.len() {
        cuts.extend([position.saturating_sub(1), position, position + 1]);
        position = position.saturating_add(4096);
    }
    cuts.sort_unstable();
    cuts.dedup();
    for cut in cuts.into_iter().filter(|cut| *cut < written.len()) {
        if written[cut..].iter().all(|byte| *byte == 0) {
            continue;
        }
        assert!(read_model(&written[..cut]).is_err(), "prefix {cut} decoded");
    }
}

/// Opt-in harness: writes the rewritten stream and/or a JSON dump for the Excel oracle run.
/// Set `GRIDCRAFT_VERTIPAQ_OUTPUT` to an output path and `GRIDCRAFT_VERTIPAQ_DUMP` to a JSON path.
#[test]
fn opt_in_export_for_the_excel_oracle() {
    let Some(base) = fixture() else { return };
    let model = read_model(&base).expect("read");
    if let Ok(output) = std::env::var("GRIDCRAFT_VERTIPAQ_OUTPUT") {
        let written = write_model(&base, &model).expect("write");
        std::fs::write(&output, &written).expect("write output");
        let back = read_model(&written).expect("read back");
        assert_eq!(back, model);
    }
    if let Ok(output) = std::env::var("GRIDCRAFT_VERTIPAQ_MODIFIED_OUTPUT") {
        let mut modified = model.clone();
        if let Some(table) = modified.tables.iter_mut().find(|table| table.name == "Sales")
            && let Some(column) = table.columns.iter_mut().find(|column| column.name == "Amount")
        {
            for value in column.values.iter_mut() {
                if let Scalar::Integer(number) = value {
                    *number *= 2;
                }
            }
        }
        let written = write_model(&base, &modified).expect("write modified");
        std::fs::write(&output, &written).expect("write modified output");
        let back = read_model(&written).expect("read modified back");
        assert_eq!(back, modified);
    }
    if let Ok(dump) = std::env::var("GRIDCRAFT_VERTIPAQ_DUMP") {
        std::fs::write(dump, serde_json::to_vec_pretty(&model).expect("serialize")).expect("write dump");
    }
}
