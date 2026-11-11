//! Save refuses a package that `read_xlsx` would refuse to open (#3817).

use std::io::Read as _;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Cell, Workbook};

use crate::write::{ReadLimits, check_reopenable, write_within};
use crate::{FunctionInfo, IoError, XlsxKind, read_xlsx, write_xlsx};

/// Distinct strings made of `&`, which the writer escapes to `&amp;`.
fn ampersands() -> Workbook {
    let mut wb = Workbook::new();
    let sheet = wb.sheet_mut(0).unwrap();
    for row in 0..200u32 {
        let text = format!("{row:03}{}", "&".repeat(500));
        sheet.cells.set(CellRef::new(row, 0), Cell::value(Value::text(text.as_str())));
    }
    wb
}

/// Each part's unpacked size, by name.
fn part_sizes(bytes: &[u8]) -> Vec<(String, u64)> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            (f.name().to_string(), data.len() as u64)
        })
        .collect()
}

fn write(wb: &Workbook, part: u64, total: u64) -> Result<Vec<u8>, IoError> {
    write_within(wb, XlsxKind::Workbook, None, FunctionInfo::default(), ReadLimits { part, total })
}

#[test]
fn escaped_text_is_what_makes_the_shared_strings_part_large() {
    let wb = ampersands();
    let sizes = part_sizes(&write_xlsx(&wb).unwrap());
    let (largest, size) = sizes.iter().max_by_key(|(_, s)| *s).unwrap();
    assert_eq!(largest, "xl/sharedStrings.xml");
    // 200 strings of 500 `&`: five bytes each once written.
    assert!(*size > 200 * 500 * 5, "{size}");
}

#[test]
fn a_part_over_the_reader_limit_is_refused_and_one_at_it_saves() {
    let wb = ampersands();
    let sizes = part_sizes(&write_xlsx(&wb).unwrap());
    let largest = sizes.iter().map(|(_, s)| *s).max().unwrap();

    let saved = write(&wb, largest, u64::MAX).unwrap();
    assert!(read_xlsx(&saved).is_ok());

    let err = write(&wb, largest - 1, u64::MAX).unwrap_err();
    assert!(matches!(err, IoError::TooLarge(_)), "{err}");
    let message = err.to_string();
    assert!(message.contains("xl/sharedStrings.xml") && message.contains("one part") && message.contains("not saved"), "{message}");
}

#[test]
fn a_package_over_the_reader_total_is_refused_and_one_at_it_saves() {
    let wb = ampersands();
    let total: u64 = part_sizes(&write_xlsx(&wb).unwrap()).iter().map(|(_, s)| *s).sum();

    assert!(write(&wb, u64::MAX, total).is_ok());

    let err = write(&wb, u64::MAX, total - 1).unwrap_err();
    assert!(matches!(err, IoError::TooLarge(_)), "{err}");
    assert!(err.to_string().contains("one file"), "{err}");
}

#[test]
fn the_check_uses_the_limits_the_reader_applies() {
    assert_eq!(ReadLimits::READER.part, crate::MAX_PART);
    assert_eq!(ReadLimits::READER.total, crate::package::MAX_TOTAL);
}

/// Parts read while opening `bytes`, each named once per read.
fn reads(bytes: &[u8]) -> (Workbook, Vec<String>) {
    crate::package::READS.with(|r| r.borrow_mut().clear());
    let (wb, _) = read_xlsx(bytes).unwrap();
    (wb, crate::package::READS.with(|r| std::mem::take(&mut *r.borrow_mut())))
}

fn read_twice(reads: &[String]) -> Vec<&String> {
    let mut seen = std::collections::HashSet::new();
    reads.iter().filter(|p| !seen.insert(*p)).collect()
}

/// The check counts each part once, so the reader may charge each part once: a picture the writer
/// stores once for several drawings is read once, and they share its bytes.
#[test]
fn a_picture_on_several_sheets_is_read_once() {
    use gridcraft_model::{Anchor, Image, Sheet};
    let data = std::sync::Arc::new([b"\x89PNG\r\n\x1a\n".as_slice(), &[7; 4096]].concat());
    let sheet = |name: &str| {
        let mut s = Sheet::new(name);
        for id in 1..=2 {
            let anchor = Anchor { cell: CellRef::new(id, 1), width: 40.0, height: 30.0, ..Default::default() };
            s.images.push(Image { id, anchor, data: data.clone(), mime: "image/png".into(), alt: String::new() });
        }
        std::sync::Arc::new(s)
    };
    let wb = Workbook { sheets: vec![sheet("One"), sheet("Two"), sheet("Three")], ..Default::default() };
    let (back, reads) = reads(&write_xlsx(&wb).unwrap());
    assert!(read_twice(&reads).is_empty(), "{:?}", read_twice(&reads));
    let images: Vec<_> = back.sheets.iter().flat_map(|s| s.images.iter()).collect();
    assert_eq!(images.len(), 6);
    assert!(images.iter().all(|im| std::sync::Arc::ptr_eq(&im.data, &images[0].data) && *im.data == *data));
}

/// A sheet's relationships are looked up for its cells, its PivotTables and pass-through, and a
/// sheet with controls or kept objects is scanned again by pass-through: each part is still read
/// once, for every kind of workbook GridCraft saves.
#[test]
fn reopening_a_saved_workbook_reads_each_part_once() {
    let kept = read_xlsx(&super::passthrough_tests::rich_package()).unwrap().0;
    let books = [
        ("pivots", super::pivot_tests::workbook()),
        ("sample", super::roundtrip::sample()),
        ("controls", super::controls_tests::book()),
        ("kept content", kept),
    ];
    for (what, wb) in books {
        let (_, reads) = reads(&write_xlsx(&wb).unwrap());
        assert!(read_twice(&reads).is_empty(), "{what}: {:?}", read_twice(&reads));
    }
}

/// A size one byte over a limit reads as more than it, not as the limit itself.
#[test]
fn sizes_in_the_message_round_up() {
    let limits = ReadLimits::READER;
    let over = (limits.part + 1) as usize;
    let err = check_reopenable([("xl/sharedStrings.xml", over)], limits).unwrap_err().to_string();
    assert!(err.contains("would unpack to 513 MB, over the 512 MB"), "{err}");
    assert!(check_reopenable([("xl/sharedStrings.xml", limits.part as usize)], limits).is_ok());
    let half = (limits.total / 2) as usize;
    let err = check_reopenable([("a", half), ("b", half), ("c", 1)], limits).unwrap_err().to_string();
    assert!(err.contains("would unpack to 1025 MB, over the 1024 MB"), "{err}");
}
