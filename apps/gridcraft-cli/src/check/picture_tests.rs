//! Picture occurrence fidelity covers opaque media bytes and alternative text.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use gridcraft_engine::model::{Anchor, Image};

use super::*;

fn picture(id: u32, marker: u8) -> Image {
    // Existing XLSX tests use opaque synthetic PNG bytes; this does not test image decoding.
    let mut data = b"\x89PNG\r\n\x1a\n0000IHDRfake".to_vec();
    data.push(marker);
    Image {
        id,
        anchor: Anchor { width: 24.0, height: 24.0, ..Default::default() },
        data,
        mime: "image/png".into(),
        alt: "Original alternative text".into(),
    }
}

fn book(images: &[Vec<Image>]) -> Workbook {
    let mut wb = Workbook::new();
    wb.sheets = images
        .iter()
        .enumerate()
        .map(|(index, images)| {
            let mut sheet = Sheet::new(format!("Page {}", index + 1));
            sheet.images = images.clone();
            Arc::new(sheet)
        })
        .collect();
    wb
}

fn picture_diff(before: &Workbook, after: &Workbook, max_report: usize) -> Category {
    diff_workbooks(before, after, &Options { max_report, ..Default::default() }).into_iter().find(|c| c.name == "pictures").unwrap_or(Category {
        name: "pictures",
        diffs: 0,
        examples: vec![],
    })
}

#[test]
fn same_length_media_replacement_is_detected() {
    let original = picture(1, b'A');
    let changed = picture(1, b'B');
    assert_eq!(original.data.len(), changed.data.len());
    let category = picture_diff(&book(&[vec![original]]), &book(&[vec![changed]]), 20);
    assert_eq!(category.diffs, 1, "comparing only media size misses replacement: {category:?}");
}

#[test]
fn alternative_text_changes_are_detected_without_trimming_content() {
    let original = picture(1, b'A');
    for text in ["Changed alternative text", "Original alternative text ", ""] {
        let mut changed = original.clone();
        changed.alt = text.into();
        assert_eq!(picture_diff(&book(&[vec![original.clone()]]), &book(&[vec![changed]]), 20).diffs, 1);
    }
}

#[test]
fn duplicate_picture_occurrences_retain_multiplicity() {
    let a = picture(1, b'A');
    let b = picture(2, b'B');
    let before = book(&[vec![a.clone(), a.clone(), b.clone()]]);
    let after = book(&[vec![a, b.clone(), b]]);
    assert_eq!(picture_diff(&before, &after, 20).diffs, 1);
}

#[test]
fn picture_order_is_compared_even_when_the_media_set_is_unchanged() {
    let a = picture(1, b'A');
    let b = picture(2, b'B');
    let before = book(&[vec![a.clone(), b.clone()]]);
    let after = book(&[vec![b, a]]);
    assert_eq!(picture_diff(&before, &after, 20).diffs, 2);
}

#[test]
fn object_ids_mime_aliases_and_anchor_geometry_are_outside_this_comparison() {
    let mut original = picture(1, b'A');
    original.mime = "image/jpg".into();
    let mut changed = original.clone();
    changed.id = 900;
    changed.mime = "image/jpeg".into();
    changed.anchor.cell = CellRef::parse("C7").unwrap();
    changed.anchor.width = 48.0;
    assert_eq!(picture_diff(&book(&[vec![original]]), &book(&[vec![changed]]), 20).diffs, 0);
}

#[test]
fn media_and_escaped_alt_survive_two_saves_despite_reassigned_ids() {
    let mut first = picture(900, b'A');
    first.alt = "Logo <north> & \"south\"".into();
    let mut second = picture(1200, b'B');
    second.mime = "image/jpg".into();
    second.alt = "Second picture".into();
    let original = book(&[vec![first, second]]);
    let mut current = original.clone();
    for _ in 0..2 {
        let bytes = gridcraft_engine::io::save_bytes(&current, "pictures.xlsx").unwrap();
        current = gridcraft_engine::io::open_bytes("pictures.xlsx", &bytes).unwrap().0;
        assert_eq!(current.sheet(0).unwrap().images.len(), 2);
        let category = picture_diff(&original, &current, 20);
        assert_eq!(category.diffs, 0, "{category:?}");
        for (actual, expected) in current.sheet(0).unwrap().images.iter().zip(&original.sheet(0).unwrap().images) {
            assert_eq!(actual.data, expected.data);
            assert_eq!(actual.alt, expected.alt);
        }
        assert!(check_bytes("pictures.xlsx", &bytes, &Options::default()).is_clean());
    }
    assert_ne!(current.sheet(0).unwrap().images[0].id, 900);
    assert_eq!(current.sheet(0).unwrap().images[1].mime, "image/jpeg");
}

#[test]
fn added_or_removed_picture_is_reported_on_its_own_sheet() {
    let first = vec![picture(1, b'A')];
    let with = book(&[first.clone(), vec![picture(2, b'B')]]);
    let without = book(&[first, vec![]]);
    for (before, after) in [(&with, &without), (&without, &with)] {
        let category = picture_diff(before, after, 20);
        assert_eq!(category.diffs, 1);
        assert!(category.examples[0].contains("Page 2"), "{category:?}");
    }
}

#[test]
fn report_caps_do_not_hide_total_changed_picture_count() {
    let before = book(&[vec![picture(1, b'A'), picture(2, b'B'), picture(3, b'C')]]);
    let after = book(&[vec![picture(1, b'D'), picture(2, b'E'), picture(3, b'F')]]);
    for limit in [0, 1, 20] {
        let category = picture_diff(&before, &after, limit);
        assert_eq!(category.diffs, 3);
        assert_eq!(category.examples.len(), limit.min(3));
    }
}
