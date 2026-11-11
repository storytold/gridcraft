//! Save refuses a package that `read_xlsx` would refuse to open (#3817).

use std::io::Read as _;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Cell, Workbook};

use crate::write::{ReadLimits, write_within};
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
