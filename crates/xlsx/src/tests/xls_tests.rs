//! Legacy `.xls` (BIFF8) import. Every test file is assembled here from the [MS-CFB] and
//! [MS-XLS] structures; no Excel-produced file is used.

use gridcraft_core::{CellError, CellRef, DateSystem, RangeRef, Value};

use crate::{Format, IoError, read_xls, read_xlsx, sniff, write_xlsx};

const END: u32 = 0xFFFF_FFFE;
const FREE: u32 = 0xFFFF_FFFF;

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn dir_entry(name: &str, kind: u8, child: u32, start: u32, size: u32) -> [u8; 128] {
    let mut e = [0u8; 128];
    let units: Vec<u16> = name.encode_utf16().collect();
    for (i, u) in units.iter().enumerate() {
        e[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
    }
    if !name.is_empty() {
        e[64..66].copy_from_slice(&((units.len() as u16 + 1) * 2).to_le_bytes());
    }
    e[66] = kind;
    e[67] = 1; // black
    put32(&mut e, 68, FREE);
    put32(&mut e, 72, FREE);
    put32(&mut e, 76, child);
    put32(&mut e, 116, start);
    put32(&mut e, 120, size);
    e
}

/// A version 3 compound file (512-byte sectors) whose root holds one stream. Streams under
/// 4096 bytes go to the mini stream, like real files.
pub(crate) fn compound(name: &str, data: &[u8]) -> Vec<u8> {
    let mini = data.len() < 4096;
    let pad = |n: usize, to: usize| n.div_ceil(to) * to;
    // Sector payloads after the FAT: directory, mini FAT (if any), then data.
    let mut payload: Vec<Vec<u8>> = vec![];
    let mut chains: Vec<(usize, usize)> = vec![]; // (first payload index, sector count)
    let (dir_at, minifat_at, data_at);
    let mut dir = vec![];
    let mut minifat = vec![];
    let stream_sectors;
    if mini {
        let m = data.len().div_ceil(64).max(1);
        for i in 0..m {
            minifat.extend_from_slice(&(if i + 1 == m { END } else { i as u32 + 1 }).to_le_bytes());
        }
        minifat.resize(512, 0xFF);
        let mut container = data.to_vec();
        container.resize(pad(data.len().max(1), 64), 0);
        stream_sectors = container.len().div_ceil(512);
        container.resize(stream_sectors * 512, 0);
        dir_at = 0;
        minifat_at = Some(1);
        data_at = 2;
        payload.push(vec![]);
        payload.push(minifat);
        payload.extend(container.chunks(512).map(<[u8]>::to_vec));
        chains.push((data_at, stream_sectors));
        dir.extend_from_slice(&dir_entry("Root Entry", 5, 1, 0, (stream_sectors * 512) as u32));
        dir.extend_from_slice(&dir_entry(name, 2, FREE, 0, data.len() as u32));
    } else {
        let mut body = data.to_vec();
        stream_sectors = body.len().div_ceil(512);
        body.resize(stream_sectors * 512, 0);
        dir_at = 0;
        minifat_at = None;
        data_at = 1;
        payload.push(vec![]);
        payload.extend(body.chunks(512).map(<[u8]>::to_vec));
        chains.push((data_at, stream_sectors));
        dir.extend_from_slice(&dir_entry("Root Entry", 5, 1, END, 0));
        dir.extend_from_slice(&dir_entry(name, 2, FREE, 0, data.len() as u32));
    }
    dir.extend_from_slice(&dir_entry("", 0, FREE, FREE, 0));
    dir.extend_from_slice(&dir_entry("", 0, FREE, FREE, 0));
    // FAT sectors come first; their count depends on the total.
    let mut fat_n = 1;
    while (fat_n + payload.len()).div_ceil(128) > fat_n {
        fat_n += 1;
    }
    let sec = |i: usize| (fat_n + i) as u32;
    // Fix the directory's start sectors now that the layout is known.
    let first_data = sec(data_at);
    if mini {
        put32(&mut dir, 116, first_data); // root: mini stream container
    } else {
        put32(&mut dir, 128 + 116, first_data);
    }
    payload[dir_at] = dir;
    let total = fat_n + payload.len();
    let mut fat = vec![FREE; fat_n * 128];
    for f in fat.iter_mut().take(fat_n) {
        *f = 0xFFFF_FFFD;
    }
    fat[sec(dir_at) as usize] = END;
    if let Some(m) = minifat_at {
        fat[sec(m) as usize] = END;
    }
    for (first, n) in chains {
        for k in 0..n {
            fat[sec(first + k) as usize] = if k + 1 == n { END } else { sec(first + k + 1) };
        }
    }
    let mut out = vec![0u8; 512];
    out[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    out[24..26].copy_from_slice(&0x3Eu16.to_le_bytes());
    out[26..28].copy_from_slice(&3u16.to_le_bytes());
    out[28..30].copy_from_slice(&0xFFFEu16.to_le_bytes());
    out[30..32].copy_from_slice(&9u16.to_le_bytes());
    out[32..34].copy_from_slice(&6u16.to_le_bytes());
    put32(&mut out, 44, fat_n as u32);
    put32(&mut out, 48, sec(dir_at));
    put32(&mut out, 56, 4096);
    put32(&mut out, 60, minifat_at.map_or(END, sec));
    put32(&mut out, 64, u32::from(minifat_at.is_some()));
    put32(&mut out, 68, END);
    for i in 0..109 {
        put32(&mut out, 76 + i * 4, if i < fat_n { i as u32 } else { FREE });
    }
    for f in &fat {
        out.extend_from_slice(&f.to_le_bytes());
    }
    for p in payload {
        let mut p = p;
        p.resize(512, 0);
        out.extend_from_slice(&p);
    }
    assert_eq!(out.len(), 512 * (1 + total));
    out
}

fn rec(id: u16, payload: &[u8]) -> Vec<u8> {
    let mut v = id.to_le_bytes().to_vec();
    v.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn u16s(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Cell header: row, column, XF index.
fn cell(r: u16, c: u16, xf: u16) -> Vec<u8> {
    u16s(&[r, c, xf])
}

fn bof(dt: u16) -> Vec<u8> {
    rec(0x0809, &cat(&[&u16s(&[0x0600, dt, 0x0DBB, 0x07CC]), &[0; 8]]))
}

fn eof() -> Vec<u8> {
    rec(0x000A, &[])
}

/// XLUnicodeString with compressed characters.
fn xlstr(s: &str) -> Vec<u8> {
    cat(&[&(s.len() as u16).to_le_bytes(), &[0], s.as_bytes()])
}

/// A FORMULA record with a numeric cached result.
fn formula(r: u16, c: u16, cached: [u8; 8], rgce: &[u8]) -> Vec<u8> {
    let mut p = cell(r, c, 0);
    p.extend_from_slice(&cached);
    p.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // flags, chn
    p.extend_from_slice(&(rgce.len() as u16).to_le_bytes());
    p.extend_from_slice(rgce);
    rec(0x0006, &p)
}

fn num(n: f64) -> [u8; 8] {
    n.to_le_bytes()
}

fn special(kind: u8, v: u8) -> [u8; 8] {
    [kind, 0, v, 0, 0, 0, 0xFF, 0xFF]
}

/// PtgRef (value class) with relative row and column.
fn ptg_ref(r: u16, c: u16) -> Vec<u8> {
    cat(&[&[0x44], &u16s(&[r, c | 0xC000])])
}

fn ptg_int(n: u16) -> Vec<u8> {
    cat(&[&[0x1E], &n.to_le_bytes()])
}

fn exp(r: u16, c: u16) -> Vec<u8> {
    cat(&[&[0x01], &u16s(&[r, c])])
}

/// A workbook with two worksheets and a chart sheet covering the stage-1 features.
fn sample(date1904: bool) -> Vec<u8> {
    // SST: "Hello", then "Wörld" as UTF-16 split across a Continue record.
    let mut sst = cat(&[&2u32.to_le_bytes(), &2u32.to_le_bytes(), &xlstr("Hello")]);
    sst.extend_from_slice(&[5, 0, 0, b'W']);
    let sst_rec = cat(&[&rec(0x00FC, &sst), &rec(0x003C, &cat(&[&[1], &u16s(&[0xF6, 'r' as u16, 'l' as u16, 'd' as u16])]))]);
    let xf = |ifmt: u16| rec(0x00E0, &cat(&[&u16s(&[0, ifmt, 0]), &[0; 14]]));
    // Lbl "Rate" = Data!$A$1 (PtgRef3d, absolute).
    let rate = cat(&[&[0x3A], &1u16.to_le_bytes(), &u16s(&[0, 0])]);
    let lbl = rec(0x0018, &cat(&[&u16s(&[0]), &[0, 4], &u16s(&[rate.len() as u16, 0, 0]), &[0; 4], &[0], b"Rate", &rate]));
    // Print_Area on Sheet1 (built-in name 6, itab 1) = Sheet1!$A$1:$B$2.
    let pa = cat(&[&[0x3B], &0u16.to_le_bytes(), &u16s(&[0, 1, 0, 1])]);
    let print_area = rec(0x0018, &cat(&[&u16s(&[0x0020]), &[0, 1], &u16s(&[pa.len() as u16, 0, 1]), &[0; 4], &[0, 6], &pa]));
    let boundsheet = |pos: u32, kind: u8, name: &str| rec(0x0085, &cat(&[&pos.to_le_bytes(), &[0, kind, name.len() as u8, 0], name.as_bytes()]));
    let globals = |positions: [u32; 3]| {
        cat(&[
            &bof(0x0005),
            &rec(0x0022, &u16s(&[u16::from(date1904)])),
            &rec(0x041E, &cat(&[&164u16.to_le_bytes(), &xlstr("yyyy-mm-dd")])),
            &xf(0),
            &xf(164),
            &xf(10), // built-in 0.00%
            &boundsheet(positions[0], 0, "Sheet1"),
            &boundsheet(positions[1], 0, "Data"),
            &boundsheet(positions[2], 2, "Chart1"),
            &rec(0x01AE, &u16s(&[3, 0x0401])),
            &rec(0x0017, &cat(&[&u16s(&[2]), &u16s(&[0, 0, 0]), &u16s(&[0, 1, 1])])),
            &lbl,
            &print_area,
            &sst_rec,
            &eof(),
        ])
    };
    let mulrk = cat(&[&u16s(&[1, 2]), &u16s(&[0]), &((7u32 << 2) | 2).to_le_bytes(), &u16s(&[2]), &((150u32 << 2) | 3).to_le_bytes(), &u16s(&[3])]);
    let shared = cat(&[&[0x4C], &u16s(&[0, 0xFFFF]), &ptg_int(10), &[0x05]]);
    let array = cat(&[&[0x65], &u16s(&[4, 5, 0xC000, 0xC000]), &ptg_int(2), &[0x05]]);
    let sum = cat(&[&[0x25], &u16s(&[0, 0, 1 | 0xC000, 2 | 0xC000]), &[0x22, 1], &4u16.to_le_bytes()]);
    let concat = cat(&[&ptg_ref(0, 0), &[0x17, 1, 0, b'!'], &[0x08]]);
    let round = cat(&[&ptg_ref(0, 1), &ptg_int(0), &[0x41], &27u16.to_le_bytes()]);
    let name_ref = cat(&[&[0x43], &1u32.to_le_bytes(), &ptg_int(2), &[0x05]]);
    let ref3d = cat(&[&[0x5A], &1u16.to_le_bytes(), &u16s(&[0, 0xC000])]);
    let sheet1 = cat(&[
        &bof(0x0010),
        &rec(0x0225, &u16s(&[0, 255])),
        &rec(0x007D, &u16s(&[0, 0, 20 * 256, 0, 0x0002, 0])),
        &rec(0x0208, &cat(&[&u16s(&[0, 0, 4, 600, 0, 0]), &(0x0140u32).to_le_bytes()])),
        &rec(0x00FD, &cat(&[&cell(0, 0, 0), &0u32.to_le_bytes()])),
        &rec(0x0203, &cat(&[&cell(0, 1, 0), &1.5f64.to_le_bytes()])),
        &rec(0x027E, &cat(&[&cell(0, 2, 0), &((42u32 << 2) | 2).to_le_bytes()])),
        &rec(0x00FD, &cat(&[&cell(0, 3, 0), &1u32.to_le_bytes()])),
        &rec(0x0205, &cat(&[&cell(1, 0, 0), &[1, 0]])),
        &rec(0x0205, &cat(&[&cell(1, 1, 0), &[0x07, 1]])),
        &rec(0x00BD, &mulrk),
        &formula(2, 0, num(3.0), &cat(&[&ptg_ref(0, 1), &ptg_int(2), &[0x05]])),
        &formula(2, 1, num(43.5), &sum),
        &formula(2, 2, special(0, 0), &concat),
        &rec(0x0207, &xlstr("Hello!")),
        &formula(2, 3, special(1, 1), &cat(&[&ptg_ref(0, 1), &ptg_int(1), &[0x0D]])),
        &formula(2, 4, num(14.0), &name_ref),
        &formula(2, 5, num(2.0), &round),
        &formula(2, 6, num(7.0), &ref3d),
        &formula(2, 7, special(2, 0x2A), &[0x1C, 0x2A]),
        &rec(0x0203, &cat(&[&cell(3, 0, 1), &0.0f64.to_le_bytes()])),
        &rec(0x0203, &cat(&[&cell(3, 1, 2), &0.25f64.to_le_bytes()])),
        &rec(0x027E, &cat(&[&cell(4, 0, 0), &((1u32 << 2) | 2).to_le_bytes()])),
        &rec(0x027E, &cat(&[&cell(5, 0, 0), &((2u32 << 2) | 2).to_le_bytes()])),
        &formula(4, 1, num(10.0), &exp(4, 1)),
        &rec(0x04BC, &cat(&[&u16s(&[4, 5]), &[1, 1, 0, 2], &(shared.len() as u16).to_le_bytes(), &shared])),
        &formula(5, 1, num(20.0), &exp(4, 1)),
        &formula(4, 2, num(2.0), &exp(4, 2)),
        &rec(0x0221, &cat(&[&u16s(&[4, 5]), &[2, 2], &[0; 6], &(array.len() as u16).to_le_bytes(), &array])),
        &formula(5, 2, num(4.0), &exp(4, 2)),
        &rec(0x00E5, &cat(&[&u16s(&[1]), &u16s(&[6, 6, 0, 1])])),
        // An embedded chart substream: its records are not cells.
        &bof(0x0020),
        &rec(0x0203, &cat(&[&cell(9, 9, 0), &9.0f64.to_le_bytes()])),
        &eof(),
        &eof(),
    ]);
    let data = cat(&[&bof(0x0010), &rec(0x0203, &cat(&[&cell(0, 0, 0), &7.0f64.to_le_bytes()])), &eof()]);
    let chart = cat(&[&bof(0x0020), &eof()]);
    let len = globals([0; 3]).len() as u32;
    let p1 = len;
    let p2 = p1 + sheet1.len() as u32;
    let p3 = p2 + data.len() as u32;
    cat(&[&globals([p1, p2, p3]), &sheet1, &data, &chart])
}

fn v(wb: &gridcraft_model::Workbook, sheet: usize, a1: &str) -> Value {
    wb.sheets[sheet].value(CellRef::parse(a1).unwrap())
}

fn ftext(wb: &gridcraft_model::Workbook, a1: &str) -> Option<String> {
    wb.sheets[0].cell(CellRef::parse(a1).unwrap()).and_then(|c| c.formula.as_ref()).map(|f| f.text.clone())
}

#[test]
fn reads_values_formulas_formats_and_layout() {
    for big in [false, true] {
        let mut stream = sample(false);
        if big {
            // Past the mini stream cutoff: the stream lives in regular sectors.
            stream.extend(std::iter::repeat_n(0u8, 5000));
        }
        let bytes = compound("Workbook", &stream);
        assert_eq!(sniff(&bytes), Format::Xls);
        let (wb, report) = read_xls(&bytes).unwrap();
        assert!(report.warnings[0].contains("Excel 97-2003"), "{:?}", report.warnings);
        assert!(report.warnings.iter().any(|w| w.contains("Chart sheets")));
        assert_eq!(wb.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Sheet1", "Data"]);
        assert_eq!(v(&wb, 0, "A1"), Value::from("Hello"));
        assert_eq!(v(&wb, 0, "B1"), Value::Number(1.5));
        assert_eq!(v(&wb, 0, "C1"), Value::Number(42.0));
        assert_eq!(v(&wb, 0, "D1"), Value::from("Wörld"));
        assert_eq!(v(&wb, 0, "A2"), Value::Bool(true));
        assert_eq!(v(&wb, 0, "B2"), Value::Error(CellError::Div0));
        assert_eq!(v(&wb, 0, "C2"), Value::Number(7.0));
        assert_eq!(v(&wb, 0, "D2"), Value::Number(1.5));
        // Formulas: text plus the cached result.
        assert_eq!(ftext(&wb, "A3").as_deref(), Some("B1*2"));
        assert_eq!(v(&wb, 0, "A3"), Value::Number(3.0));
        assert_eq!(ftext(&wb, "B3").as_deref(), Some("SUM(B1:C1)"));
        assert_eq!(ftext(&wb, "C3").as_deref(), Some("A1&\"!\""));
        assert_eq!(v(&wb, 0, "C3"), Value::from("Hello!"));
        assert_eq!(ftext(&wb, "D3").as_deref(), Some("B1>1"));
        assert_eq!(v(&wb, 0, "D3"), Value::Bool(true));
        assert_eq!(ftext(&wb, "E3").as_deref(), Some("Rate*2"));
        assert_eq!(ftext(&wb, "F3").as_deref(), Some("ROUND(B1,0)"));
        assert_eq!(ftext(&wb, "G3").as_deref(), Some("Data!A1"));
        assert_eq!(v(&wb, 0, "H3"), Value::Error(CellError::NA));
        // Shared formula: relative offsets resolve per cell.
        assert_eq!(ftext(&wb, "B5").as_deref(), Some("A5*10"));
        assert_eq!(ftext(&wb, "B6").as_deref(), Some("A6*10"));
        // Array formula: the anchor holds it; the other cell's cached result is dropped so that
        // the formula fills the range on recalculation (as when an .xlsx is read, #156).
        let c5 = wb.sheets[0].cell(CellRef::parse("C5").unwrap()).unwrap().formula.clone().unwrap();
        assert_eq!((c5.text.as_str(), c5.array), ("A5:A6*2", Some(RangeRef::parse("C5:C6").unwrap())));
        assert_eq!(ftext(&wb, "C6"), None);
        assert_eq!(v(&wb, 0, "C6"), Value::Empty);
        // Number formats, merges, sizes, names, print area; embedded chart records ignored.
        let sh = &wb.sheets[0];
        assert_eq!(wb.styles.get(sh.style_id(CellRef::parse("A4").unwrap())).num_fmt.as_str(), "yyyy-mm-dd");
        assert_eq!(wb.styles.get(sh.style_id(CellRef::parse("B4").unwrap())).num_fmt.as_str(), "0.00%");
        assert_eq!(sh.merges, vec![RangeRef::parse("A7:B7").unwrap()]);
        assert!(sh.cols.get(&0).unwrap().size.unwrap() > sh.default_col_width);
        assert_eq!(sh.rows.get(&0).unwrap().size, Some(40.0));
        assert!(!sh.rows.get(&0).unwrap().hidden && sh.rows.get(&0).unwrap().custom);
        assert_eq!(sh.print.print_area, RangeRef::parse("A1:B2"));
        assert_eq!(v(&wb, 0, "J10"), Value::Empty);
        assert_eq!(wb.names.len(), 1);
        assert_eq!((wb.names[0].name.as_str(), wb.names[0].formula.as_str()), ("Rate", "Data!$A$1"));
        assert_eq!(v(&wb, 1, "A1"), Value::Number(7.0));
        assert_eq!(wb.date_system, DateSystem::D1900);
        // The import saves as .xlsx and reads back.
        let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
        assert_eq!(back.sheets[0].cell(CellRef::parse("A3").unwrap()).unwrap().formula.as_ref().unwrap().text, "B1*2");
        assert_eq!(back.sheets[0].value(CellRef::parse("C3").unwrap()), Value::from("Hello!"));
    }
    let (wb, _) = read_xls(&compound("Workbook", &sample(true))).unwrap();
    assert_eq!(wb.date_system, DateSystem::D1904);
}

#[test]
fn text_result_of_a_formula_kept_as_its_value() {
    // A what-if data table cell ({=TABLE(B1,)}) can't be imported as a formula, so its saved result
    // is kept. With the default XF the cell is blank until the String record brings the text.
    let table = cat(&[&[0x02], &u16s(&[0, 0])]);
    let sheet = cat(&[&bof(0x0010), &formula(0, 0, special(0, 0), &table), &rec(0x0207, &xlstr("103a")), &eof()]);
    let boundsheet = |pos: u32| rec(0x0085, &cat(&[&pos.to_le_bytes(), &[0, 0, 6, 0], b"Sheet1"]));
    let len = cat(&[&bof(0x0005), &boundsheet(0), &eof()]).len() as u32;
    let stream = cat(&[&bof(0x0005), &boundsheet(len), &eof(), &sheet]);
    let (wb, report) = read_xls(&compound("Workbook", &stream)).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("what-if data tables")), "{:?}", report.warnings);
    assert_eq!(ftext(&wb, "A1"), None);
    assert_eq!(v(&wb, 0, "A1"), Value::from("103a"));
}

#[test]
fn refuses_encrypted_old_and_foreign_files() {
    // FILEPASS in the globals: an encrypted workbook.
    let mut stream = bof(0x0005);
    stream.extend(rec(0x002F, &[1, 0, 1, 0]));
    stream.extend(eof());
    let err = read_xls(&compound("Workbook", &stream)).unwrap_err().to_string();
    assert!(err.contains("password-protected"), "{err}");
    // A BIFF5 (Excel 5.0/95) workbook keeps its data in a `Book` stream.
    let book = compound("Book", &stream);
    assert_eq!(sniff(&book), Format::Xls);
    assert!(read_xls(&book).unwrap_err().to_string().contains("BIFF5"));
    // Another compound file (a word processor document): not a workbook.
    let doc = compound("WordDocument", &[0u8; 100]);
    assert_eq!(sniff(&doc), Format::LegacyBinary);
    assert!(read_xls(&doc).is_err());
}

#[test]
fn hostile_xls_input_never_panics_or_hangs() {
    let stream = sample(false);
    let good = compound("Workbook", &stream);
    // Truncation anywhere.
    for cut in (0..good.len()).step_by(37) {
        let _ = read_xls(&good[..cut]);
        let _ = sniff(&good[..cut]);
    }
    // Byte mutations in the container and the stream.
    for at in (0..good.len()).step_by(7) {
        for x in [0x00u8, 0x01, 0x7F, 0xFF] {
            let mut b = good.clone();
            b[at] = x;
            let _ = read_xls(&b);
        }
    }
    for at in 0..stream.len() {
        let mut s = stream.clone();
        s[at] ^= 0xA5;
        let _ = read_xls(&compound("Workbook", &s));
    }
    // A FAT chain that loops on itself (big stream in regular sectors).
    let mut big = stream.clone();
    big.extend(std::iter::repeat_n(0u8, 5000));
    let mut b = compound("Workbook", &big);
    let fat_at = 512;
    let first_data = 2usize; // FAT, directory, then data
    put32(&mut b, fat_at + (first_data + 3) * 4, first_data as u32);
    assert!(read_xls(&b).is_err());
    // Huge declared counts: shared strings, merges, external sheets.
    let mut huge = bof(0x0005);
    huge.extend(rec(0x00FC, &cat(&[&u32::MAX.to_le_bytes(), &u32::MAX.to_le_bytes()])));
    huge.extend(eof());
    assert!(matches!(read_xls(&compound("Workbook", &huge)), Err(IoError::TooLarge(_))));
    let mut huge = bof(0x0005);
    huge.extend(rec(0x00FC, &cat(&[&9u32.to_le_bytes(), &1_000_000u32.to_le_bytes(), &xlstr("x")])));
    huge.extend(rec(0x0017, &u16s(&[0xFFFF])));
    huge.extend(eof());
    assert!(read_xls(&compound("Workbook", &huge)).is_err()); // no worksheets
    // A sheet pointing outside the stream and a stream size larger than the file.
    let mut bad = bof(0x0005);
    bad.extend(rec(0x0085, &cat(&[&u32::MAX.to_le_bytes(), &[0, 0, 1, 0], b"S"])));
    bad.extend(eof());
    assert!(read_xls(&compound("Workbook", &bad)).is_err());
    let mut b = compound("Workbook", &big);
    put32(&mut b, 512 * 2 + 128 + 120, 0x3FFF_FFFF);
    assert!(read_xls(&b).is_err());
    // Random bytes behind a compound-file signature.
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..200 {
        let mut b = good[..512].to_vec();
        for _ in 0..2048 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            b.push(seed as u8);
        }
        let _ = read_xls(&b);
    }
}

#[test]
fn dialog_sheets_are_skipped_not_read_as_worksheets() {
    let boundsheet = |pos: u32, name: &str| rec(0x0085, &cat(&[&pos.to_le_bytes(), &[0, 0, name.len() as u8, 0], name.as_bytes()]));
    let globals = |positions: [u32; 2]| cat(&[&bof(0x0005), &boundsheet(positions[0], "Dialog1"), &boundsheet(positions[1], "Sheet1"), &eof()]);
    let dialog = cat(&[&bof(0x0010), &rec(0x0081, &[0x10, 0x00]), &rec(0x0203, &cat(&[&cell(0, 0, 0), &1.0f64.to_le_bytes()])), &eof()]);
    let worksheet = cat(&[&bof(0x0010), &rec(0x0081, &[0x01, 0x04]), &rec(0x0203, &cat(&[&cell(0, 0, 0), &7.0f64.to_le_bytes()])), &eof()]);
    let first = globals([0; 2]).len() as u32;
    let stream = cat(&[&globals([first, first + dialog.len() as u32]), &dialog, &worksheet]);
    let (wb, report) = read_xls(&compound("Workbook", &stream)).unwrap();
    assert_eq!(wb.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Sheet1"]);
    assert_eq!(v(&wb, 0, "A1"), Value::Number(7.0));
    assert!(report.warnings.iter().any(|w| w.contains("Dialog sheets")), "{:?}", report.warnings);
}

#[test]
fn sheets_share_one_record_budget() {
    // Many BoundSheet records point into one long substream (at successive nested BOFs), so each
    // sheet would read the whole tail. The record allowance is shared, not per sheet.
    let n = 300u32;
    let tail: Vec<u8> = (0..2000u16).flat_map(|r| rec(0x0203, &cat(&[&cell(r, 0, 0), &1.0f64.to_le_bytes()]))).collect();
    let boundsheet = |pos: u32, name: &str| rec(0x0085, &cat(&[&pos.to_le_bytes(), &[0, 0, name.len() as u8, 0], name.as_bytes()]));
    let globals = |base: u32| {
        let mut g = bof(0x0005);
        for i in 0..n {
            g.extend(boundsheet(base + i * bof(0x0010).len() as u32, &format!("S{i}")));
        }
        g.extend(eof());
        g
    };
    let base = globals(0).len() as u32;
    let mut stream = globals(base);
    for _ in 0..n {
        stream.extend(bof(0x0010));
    }
    stream.extend(tail);
    stream.extend(eof());
    let file = compound("Workbook", &stream);
    // About n × 2,000 records in all: over a 100,000 shared allowance, though each sheet alone fits.
    assert!(matches!(crate::xls::read_xls_with(&file, crate::xls::Budget::with_max_records(100_000)), Err(IoError::TooLarge(_))));
    assert!(crate::xls::read_xls_with(&file, crate::xls::Budget::with_max_records(10_000_000)).is_ok());

    // Two sheets at the same offset are refused outright.
    let mut dup = bof(0x0005);
    let at = (dup.len() + 2 * boundsheet(0, "A").len() + eof().len()) as u32;
    dup.extend(boundsheet(at, "A"));
    dup.extend(boundsheet(at, "B"));
    dup.extend(eof());
    dup.extend(bof(0x0010));
    dup.extend(eof());
    let err = read_xls(&compound("Workbook", &dup)).unwrap_err().to_string();
    assert!(err.contains("same substream"), "{err}");
}

#[test]
fn formulas_using_a_skipped_name_keep_their_saved_results() {
    // Lbl "ExtRange" = [Source.xlsx]Sheet1!$A$1:$A$3 (PtgArea3d through an external SupBook),
    // "Local" = Sheet1!$A$1 and "Twice" = ExtRange*2. Only the first is skipped on import; a
    // name that uses it is still read as it was.
    let ext = cat(&[&[0x3B], &1u16.to_le_bytes(), &u16s(&[0, 2, 0, 0])]);
    let local = cat(&[&[0x3A], &0u16.to_le_bytes(), &u16s(&[0, 0])]);
    let twice = cat(&[&[0x23], &1u32.to_le_bytes(), &ptg_int(2), &[0x05]]);
    let lbl = |name: &str, rgce: &[u8]| {
        rec(0x0018, &cat(&[&u16s(&[0]), &[0, name.len() as u8], &u16s(&[rgce.len() as u16, 0, 0]), &[0; 4], &[0], name.as_bytes(), rgce]))
    };
    let boundsheet = |pos: u32| rec(0x0085, &cat(&[&pos.to_le_bytes(), &[0, 0, 6, 0], b"Sheet1"]));
    let globals = |pos: u32| {
        cat(&[
            &bof(0x0005),
            &boundsheet(pos),
            &rec(0x01AE, &u16s(&[1, 0x0401])),
            &rec(0x01AE, &cat(&[&u16s(&[1]), &xlstr("Source.xlsx"), &xlstr("Sheet1")])),
            &rec(0x0017, &cat(&[&u16s(&[2]), &u16s(&[0, 0, 0]), &u16s(&[1, 0, 0])])),
            &lbl("ExtRange", &ext),
            &lbl("Local", &local),
            &lbl("Twice", &twice),
            &eof(),
        ])
    };
    let sum_ext = cat(&[&[0x43], &1u32.to_le_bytes(), &[0x22, 1], &4u16.to_le_bytes()]);
    let local_x2 = cat(&[&[0x43], &2u32.to_le_bytes(), &ptg_int(2), &[0x05]]);
    let sheet = cat(&[
        &bof(0x0010),
        &rec(0x0203, &cat(&[&cell(0, 0, 0), &10.0f64.to_le_bytes()])),
        &formula(5, 1, num(60.0), &sum_ext),
        &formula(6, 1, num(20.0), &local_x2),
        &eof(),
    ]);
    let pos = globals(0).len() as u32;
    let (wb, report) = read_xls(&compound("Workbook", &cat(&[&globals(pos), &sheet]))).unwrap();
    let names: Vec<_> = wb.names.iter().map(|n| (n.name.as_str(), n.formula.as_str())).collect();
    assert_eq!(names, [("Local", "Sheet1!$A$1"), ("Twice", "ExtRange*2")]);
    assert_eq!(ftext(&wb, "B6"), None);
    assert_eq!(v(&wb, 0, "B6"), Value::Number(60.0));
    assert_eq!(ftext(&wb, "B7").as_deref(), Some("Local*2"));
    assert!(report.warnings.iter().any(|w| w.contains("defined names") && w.contains("saved results")), "{:?}", report.warnings);
}

#[test]
fn reads_iterative_calculation_settings() {
    // CalcMode, CalcCount, CalcIter and CalcDelta repeat in every worksheet substream; here only
    // the first sheet carries them, so the sheet after it must not reset them to the defaults.
    // CalcPrecision sits in the globals.
    let calc = cat(&[&rec(0x000D, &u16s(&[0])), &rec(0x000C, &u16s(&[50])), &rec(0x0011, &u16s(&[1])), &rec(0x0010, &0.01f64.to_le_bytes())]);
    let sheet1 = cat(&[&bof(0x0010), &calc, &eof()]);
    let sheet2 = cat(&[&bof(0x0010), &eof()]);
    let boundsheet = |pos: u32, name: &str| rec(0x0085, &cat(&[&pos.to_le_bytes(), &[0, 0, name.len() as u8, 0], name.as_bytes()]));
    let globals = |p1: u32, p2: u32| cat(&[&bof(0x0005), &rec(0x000E, &u16s(&[0])), &boundsheet(p1, "A"), &boundsheet(p2, "B"), &eof()]);
    let p1 = globals(0, 0).len() as u32;
    let p2 = p1 + sheet1.len() as u32;
    let (wb, _) = read_xls(&compound("Workbook", &cat(&[&globals(p1, p2), &sheet1, &sheet2]))).unwrap();
    assert!(wb.calc.iterative);
    assert_eq!((wb.calc.max_iterations, wb.calc.max_change), (50, 0.01));
    assert_eq!(wb.calc.mode, gridcraft_model::CalcMode::Manual);
    assert!(wb.calc.precision_as_displayed);
    // Without the records: Excel's defaults, iteration off.
    let (wb, _) = read_xls(&compound("Workbook", &sample(false))).unwrap();
    assert_eq!(wb.calc, gridcraft_model::CalcSettings::default());
}

#[test]
fn a_format_shorter_than_its_count_keeps_its_characters() {
    // A FORMAT whose cch is one more than the characters in the record (seen in real files):
    // Excel opens them, the record's characters are the format code.
    let short = rec(0x041E, &cat(&[&165u16.to_le_bytes(), &6u16.to_le_bytes(), &[0], b"0.000"]));
    let xf = |ifmt: u16| rec(0x00E0, &cat(&[&u16s(&[0, ifmt, 0]), &[0; 14]]));
    let globals = |pos: u32| {
        cat(&[
            &bof(0x0005),
            &short,
            &rec(0x041E, &cat(&[&164u16.to_le_bytes(), &xlstr("yyyy-mm-dd")])),
            &xf(0),
            &xf(164),
            &xf(165),
            &rec(0x0085, &cat(&[&pos.to_le_bytes(), &[0, 0, 6, 0], b"Sheet1"])),
            &eof(),
        ])
    };
    let sheet = cat(&[
        &bof(0x0010),
        &rec(0x0203, &cat(&[&cell(0, 0, 0), &7.0f64.to_le_bytes()])),
        &rec(0x0203, &cat(&[&cell(1, 0, 1), &0.0f64.to_le_bytes()])),
        &rec(0x0203, &cat(&[&cell(2, 0, 2), &1.25f64.to_le_bytes()])),
        &eof(),
    ]);
    let stream = cat(&[&globals(globals(0).len() as u32), &sheet]);
    let (wb, report) = read_xls(&compound("Workbook", &stream)).unwrap();
    let sh = &wb.sheets[0];
    let fmt = |a1: &str| wb.styles.get(sh.style_id(CellRef::parse(a1).unwrap())).num_fmt.as_str().to_string();
    assert_eq!(v(&wb, 0, "A1"), Value::Number(7.0));
    assert_eq!(v(&wb, 0, "A3"), Value::Number(1.25));
    assert_eq!(fmt("A2"), "yyyy-mm-dd");
    assert_eq!(fmt("A3"), "0.000");
    assert!(report.warnings.iter().any(|w| w.contains("declared length")), "{:?}", report.warnings);
}

fn one_sheet_with(sheet_records: &[u8]) -> Vec<u8> {
    let globals = |pos: u32| cat(&[&bof(0x0005), &rec(0x0085, &cat(&[&pos.to_le_bytes(), &[0, 0, 6, 0], b"Sheet1"])), &eof()]);
    let sheet = cat(&[&bof(0x0010), sheet_records, &rec(0x0203, &cat(&[&cell(0, 0, 0), &7.0f64.to_le_bytes()])), &eof()]);
    compound("Workbook", &cat(&[&globals(globals(0).len() as u32), &sheet]))
}

#[test]
fn xls_window2_right_to_left_sheet_opens_right_to_left() {
    let window2 = |flags: u16| rec(0x023E, &u16s(&[flags, 0, 0, 64, 0, 0, 0, 0, 0]));
    let view = |file: &[u8]| {
        let (wb, _) = read_xls(file).unwrap();
        let sh = &wb.sheets[0];
        (sh.right_to_left, sh.show_gridlines, sh.show_headings, sh.show_zeros, sh.show_formulas)
    };
    assert_eq!(view(&one_sheet_with(&window2(0x0040 | 0x0004))), (true, false, true, false, false));
    assert_eq!(view(&one_sheet_with(&window2(0x0001 | 0x0002 | 0x0010))), (false, true, false, true, true));
}

#[test]
fn xls_short_window2_is_ignored() {
    for payload in [&[][..], &[0x40][..], &[0x40, 0][..], &[0x40, 0, 0, 0, 0, 0, 0, 0, 0][..]] {
        let (wb, _) = read_xls(&one_sheet_with(&rec(0x023E, payload))).unwrap();
        let sh = &wb.sheets[0];
        assert_eq!((sh.right_to_left, sh.show_gridlines, sh.show_headings, sh.show_zeros), (false, true, true, true));
        assert_eq!(v(&wb, 0, "A1"), Value::Number(7.0));
    }
}
