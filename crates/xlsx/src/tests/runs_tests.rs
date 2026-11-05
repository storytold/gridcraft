//! Shared-string rich-text runs: splitting an `<si>` into text + run parts, reading `<rPr>`
//! deltas and assembling `CellRuns` under the caps, and writing them back (issue #774).

use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Cell, CellRuns, Color, MAX_RUNS_PER_CELL, Run, RunFont, Sheet, StyleId, Workbook};

use super::minimal;
use crate::read_xlsx;
use crate::runs::{Si, runs_fits};
use crate::styles::{read_run_font, run_font_set};
use crate::tables::INDEXED;
use crate::write_xlsx;

use crate::xml::{El, parse, si_parts};

fn si(xml: &str) -> El {
    parse(xml.as_bytes()).unwrap()
}

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}

/// A minimal one-sheet package with a sharedStrings part and the given sheet body.
fn sst_package(sst: &str, body: &str) -> Vec<u8> {
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#;
    minimal(body, &[("xl/sharedStrings.xml", sst)], rels, "")
}

fn none_font() -> RunFont {
    RunFont { name: None, size: None, bold: None, italic: None, underline: None, strike: None, color: None, vert: None }
}

#[test]
fn si_parts_plain() {
    let el = si("<si><t>plain text</t></si>");
    let (text, parts) = si_parts(&el);
    assert_eq!(text, "plain text");
    assert!(parts.is_empty());

    let mut budget = 1 << 20;
    let Si { text: t, runs } = Si::parse(&el, &INDEXED, &mut budget);
    assert_eq!(&*t, "plain text");
    assert!(runs.is_none());
    assert_eq!(budget, 1 << 20);
}

/// The #774 repro shape: three runs over "this is black, this is red, this is black" with the
/// three colour forms.
#[test]
fn si_parts_repro_colors() {
    let el = si(concat!(
        r#"<si><r><rPr><color theme="1"/></rPr><t>this is black,</t></r>"#,
        r#"<r><rPr><color indexed="2"/></rPr><t xml:space="preserve"> this is red,</t></r>"#,
        r#"<r><rPr><color rgb="FF00B050"/></rPr><t xml:space="preserve"> this is black</t></r></si>"#,
    ));
    let (text, parts) = si_parts(&el);
    assert_eq!(text, "this is black, this is red, this is black");
    assert_eq!(parts.len(), 3);
    assert_eq!((parts[0].start, parts[0].len), (0, 14));
    assert_eq!((parts[1].start, parts[1].len), (14, 13));
    assert_eq!((parts[2].start, parts[2].len), (27, 14));
    assert!(parts.iter().all(|p| p.rpr.is_some()));

    let mut budget = 1 << 20;
    let Si { text, runs } = Si::parse(&el, &INDEXED, &mut budget);
    assert_eq!(&*text, "this is black, this is red, this is black");
    let cr = runs.expect("runs parse");
    assert!(cr.valid_for(&text));
    assert_eq!(cr.run_text(&text, 0), Some("this is black,"));
    assert_eq!(cr.run_text(&text, 1), Some(" this is red,"));
    assert_eq!(cr.run_text(&text, 2), Some(" this is black"));
    assert_eq!(cr.runs[0].font.color, Some(Color::Auto));
    assert_eq!(cr.runs[1].font.color, Some(Color::Rgb(0xFF0000)));
    assert_eq!(cr.runs[2].font.color, Some(Color::Rgb(0x00B050)));
    assert_eq!(budget, (1 << 20) - 3);
}

#[test]
fn preserves_leading_space_offsets() {
    let el = si(concat!(
        r#"<si><r><rPr><b/></rPr><t>this is black,</t></r>"#,
        r#"<r><rPr><color rgb="FFFF0000"/></rPr><t xml:space="preserve"> this is red,</t></r></si>"#,
    ));
    let (text, parts) = si_parts(&el);
    assert_eq!(text, "this is black, this is red,");
    // The second run keeps its leading space: byte offsets, total == text length.
    assert_eq!((parts[1].start, parts[1].len), (14, 13));
    let total: usize = parts.iter().map(|p| p.len as usize).sum();
    assert_eq!(total, text.len());

    let mut budget = 1 << 20;
    let Si { runs, .. } = Si::parse(&el, &INDEXED, &mut budget);
    assert!(runs.expect("valid").valid_for(text.as_str()));
}

#[test]
fn rph_ignored() {
    let el = si(concat!(
        r#"<si><r><rPr><b/></rPr><t>ab</t></r>"#,
        r#"<rPh sb="0" eb="2"><t>phonetic</t></rPh>"#,
        r#"<r><rPr><i/></rPr><t>cd</t></r></si>"#,
    ));
    let (text, parts) = si_parts(&el);
    assert_eq!(text, "abcd");
    assert_eq!(parts.len(), 2);
    assert_eq!((parts[1].start, parts[1].len), (2, 2));
}

#[test]
fn over_cell_cap_drops_runs() {
    let mut xml = String::from("<si>");
    for _ in 0..=MAX_RUNS_PER_CELL {
        xml.push_str("<r><rPr><b/></rPr><t>x</t></r>");
    }
    xml.push_str("</si>");
    let el = si(&xml);
    assert_eq!(si_parts(&el).1.len(), MAX_RUNS_PER_CELL + 1);

    let mut budget = 1 << 20;
    let Si { text, runs } = Si::parse(&el, &INDEXED, &mut budget);
    assert_eq!(text.len(), MAX_RUNS_PER_CELL + 1);
    assert!(runs.is_none());
    assert_eq!(budget, 1 << 20);
}

#[test]
fn over_workbook_budget_drops() {
    let el = si(r#"<si><r><rPr><b/></rPr><t>ab</t></r><r><rPr><i/></rPr><t>cd</t></r></si>"#);
    let mut budget = 0u64;
    let Si { text, runs } = Si::parse(&el, &INDEXED, &mut budget);
    assert_eq!(&*text, "abcd");
    assert!(runs.is_none());
    assert_eq!(budget, 0);
    // Too small by one: still dropped, budget untouched.
    let mut budget = 1u64;
    let Si { runs, .. } = Si::parse(&el, &INDEXED, &mut budget);
    assert!(runs.is_none());
    assert_eq!(budget, 1);
}

#[test]
fn runs_without_rpr_dropped() {
    // Runs that carry no formatting delta hold no information: text only.
    let el = si(r#"<si><r><t>ab</t></r><r><t>cd</t></r></si>"#);
    let mut budget = 1 << 20;
    let Si { text, runs } = Si::parse(&el, &INDEXED, &mut budget);
    assert_eq!(&*text, "abcd");
    assert!(runs.is_none());
    assert_eq!(budget, 1 << 20);
}

/// The cliff [`runs_fits`] enforces for both parse-time and attachment-time charging: it
/// spends only when everything fits, and never touches the budget otherwise (issue #774).
#[test]
fn runs_fits_charges_at_the_cliff() {
    // Exact fit: charged, down to zero.
    let mut b = 3u64;
    assert!(runs_fits(&mut b, 3));
    assert_eq!(b, 0);
    // One over: refused, budget untouched.
    let mut b = 3u64;
    assert!(!runs_fits(&mut b, 4));
    assert_eq!(b, 3);
    // Zero always fits and charges nothing, even from an exhausted budget.
    let mut b = 0u64;
    assert!(runs_fits(&mut b, 0));
    assert_eq!(b, 0);
    // An exhausted budget refuses anything more.
    assert!(!runs_fits(&mut b, 1));
    assert_eq!(b, 0);
}

#[test]
fn run_font_delta_fields() {
    // Absent properties stay None (inherit); present ones override.
    let rpr = si(r#"<rPr><rFont val="Georgia"/><sz val="12"/><b/><strike val="0"/><u val="double"/><vertAlign val="superscript"/></rPr>"#);
    let f = read_run_font(&rpr, &INDEXED);
    assert_eq!(f.name.as_deref(), Some("Georgia"));
    assert_eq!(f.size, Some(12.0));
    assert_eq!(f.bold, Some(true));
    assert_eq!(f.italic, None);
    assert_eq!(f.strike, Some(false));
    assert_eq!(f.underline, Some(gridcraft_model::Underline::Double));
    assert_eq!(f.vert, Some(gridcraft_model::VertAlign::Superscript));
    assert_eq!(f.color, None);

    let empty = si("<rPr/>");
    assert_eq!(read_run_font(&empty, &INDEXED), none_font());

    // Both automatic forms: auto="1" and theme 1 (text 1) with no tint.
    let auto = si(r#"<rPr><color auto="1"/></rPr>"#);
    assert_eq!(read_run_font(&auto, &INDEXED).color, Some(Color::Auto));
    let theme1 = si(r#"<rPr><color theme="1"/></rPr>"#);
    assert_eq!(read_run_font(&theme1, &INDEXED).color, Some(Color::Auto));

    // Size clamps to Excel's 1..=409 pt; theme 1 with a tint is a real theme colour.
    let clamped = si(r#"<rPr><sz val="9999"/><color theme="1" tint="-0.5"/></rPr>"#);
    let f = read_run_font(&clamped, &INDEXED);
    assert_eq!(f.size, Some(409.0));
    assert_eq!(f.color, Some(Color::Theme(1, -500)));

    // A run without <rPr> inherits everything.
    let mut budget = 1 << 20;
    let Si { runs, .. } = Si::parse(&si(r#"<si><r><rPr><b/></rPr><t>a</t></r><r><t>bc</t></r></si>"#), &INDEXED, &mut budget);
    let cr = runs.expect("runs parse");
    assert_eq!(cr.runs[1].font, none_font());
    assert_eq!(cr.runs[0].font.bold, Some(true));
}

/// `run_font_set` compares against the default instead of listing fields, so a future
/// `RunFont` property cannot be silently missed: everything-unset inherits and is not-set;
/// any single field makes it set (issue #774).
#[test]
fn run_font_set_default_is_not_set() {
    assert!(!run_font_set(&RunFont::default()));
    assert!(run_font_set(&RunFont { bold: Some(true), ..RunFont::default() }));
}

// Reading packages: runs attach to shared and inline string cells (issue #774).

/// The #774 shape: four shared strings, A1-A4 referencing them; only the third and fourth carry
/// runs worth keeping. A3 holds the three colour forms with their exact offsets.
#[test]
fn read_attaches_shared_runs() {
    let sst = concat!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="4" uniqueCount="4">"#,
        r#"<si><t>plain shared string</t></si>"#,
        r#"<si><r><t>runs without </t></r><r><t>any formatting</t></r></si>"#,
        r#"<si><r><rPr><color theme="1"/></rPr><t>this is black,</t></r>"#,
        r#"<r><rPr><color indexed="2"/></rPr><t xml:space="preserve"> this is red,</t></r>"#,
        r#"<r><rPr><color rgb="FF00B050"/></rPr><t xml:space="preserve"> this is black</t></r></si>"#,
        r#"<si><r><rPr><b/><sz val="14"/></rPr><t>bold fourteen </t></r>"#,
        r#"<r><rPr><color rgb="FFFF0000"/></rPr><t xml:space="preserve">red tail</t></r></si>"#,
        r#"</sst>"#,
    );
    let body = concat!(
        r#"<sheetData>"#,
        r#"<row r="1"><c r="A1" t="s"><v>0</v></c></row>"#,
        r#"<row r="2"><c r="A2" t="s"><v>1</v></c></row>"#,
        r#"<row r="3"><c r="A3" t="s"><v>2</v></c></row>"#,
        r#"<row r="4"><c r="A4" t="s"><v>3</v></c></row>"#,
        r#"</sheetData>"#,
    );
    let (wb, rep) = read_xlsx(&sst_package(sst, body)).unwrap();
    let s = wb.sheet(0).unwrap();
    // Every cell keeps the plain text; the runs live only in the side-table.
    assert_eq!(s.value(at("A1")), Value::text("plain shared string"));
    assert_eq!(s.value(at("A2")), Value::text("runs without any formatting"));
    assert_eq!(s.value(at("A3")), Value::text("this is black, this is red, this is black"));
    assert_eq!(s.value(at("A4")), Value::text("bold fourteen red tail"));
    assert_eq!(s.cell_runs.len(), 2);
    assert!(!s.cell_runs.contains_key(&at("A1")));
    assert!(!s.cell_runs.contains_key(&at("A2")));

    let a3 = s.cell_runs.get(&at("A3")).unwrap();
    assert!(a3.valid_for("this is black, this is red, this is black"));
    assert_eq!(a3.runs.len(), 3);
    assert_eq!((a3.runs[0].start, a3.runs[0].len), (0, 14));
    assert_eq!((a3.runs[1].start, a3.runs[1].len), (14, 13));
    assert_eq!((a3.runs[2].start, a3.runs[2].len), (27, 14));
    assert_eq!(a3.runs[0].font.color, Some(Color::Auto));
    assert_eq!(a3.runs[1].font.color, Some(Color::Rgb(0xFF0000)));
    assert_eq!(a3.runs[2].font.color, Some(Color::Rgb(0x00B050)));

    let a4 = s.cell_runs.get(&at("A4")).unwrap();
    assert!(a4.valid_for("bold fourteen red tail"));
    assert_eq!((a4.runs[0].start, a4.runs[0].len), (0, 14));
    assert_eq!((a4.runs[1].start, a4.runs[1].len), (14, 8));
    assert_eq!(a4.runs[0].font.bold, Some(true));
    assert_eq!(a4.runs[0].font.size, Some(14.0));
    assert_eq!(a4.runs[1].font.color, Some(Color::Rgb(0xFF0000)));
    assert!(!rep.warnings.iter().any(|w| w.contains("shared strings")), "{:?}", rep.warnings);
}

/// Per-attachment budget charging must not change ordinary files: five cells sharing one rich
/// item each attach their own copy far under the workbook budget, and every cell still reads
/// its plain text (regression guard for issue #774).
#[test]
fn shared_runs_attach_per_reference() {
    let sst = concat!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="5" uniqueCount="1">"#,
        r#"<si><r><rPr><b/></rPr><t>bold </t></r><r><rPr><color rgb="FFFF0000"/></rPr><t>red</t></r></si>"#,
        r#"</sst>"#,
    );
    let body = concat!(
        r#"<sheetData>"#,
        r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>0</v></c><c r="C1" t="s"><v>0</v></c></row>"#,
        r#"<row r="2"><c r="A2" t="s"><v>0</v></c><c r="B2" t="s"><v>0</v></c></row>"#,
        r#"</sheetData>"#,
    );
    let (wb, rep) = read_xlsx(&sst_package(sst, body)).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.cell_runs.len(), 5, "every reference attaches its own runs");
    for a in ["A1", "B1", "C1", "A2", "B2"] {
        assert_eq!(s.value(at(a)), Value::text("bold red"), "{a}");
        assert!(s.cell_runs.get(&at(a)).is_some_and(|cr| cr.valid_for("bold red")), "{a}");
    }
    assert!(!rep.warnings.iter().any(|w| w.contains("shared strings")), "no warning storm: {:?}", rep.warnings);
}

/// An inline string with rich runs gets an entry; the plain `<is><t>` form does not.
#[test]
fn read_attaches_inline_runs() {
    let body = concat!(
        r#"<sheetData><row r="1">"#,
        r#"<c r="A1" t="inlineStr"><is><r><t>rich </t></r><r><rPr><b/></rPr><t>text</t></r><rPh><t>ignored</t></rPh></is></c>"#,
        r#"<c r="B1" t="inlineStr"><is><t>plain inline</t></is></c>"#,
        r#"</row></sheetData>"#,
    );
    let (wb, _) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.value(at("A1")), Value::text("rich text"));
    assert_eq!(s.value(at("B1")), Value::text("plain inline"));
    let cr = s.cell_runs.get(&at("A1")).unwrap();
    assert!(cr.valid_for("rich text"));
    assert_eq!(cr.runs.len(), 2);
    assert_eq!((cr.runs[0].start, cr.runs[0].len), (0, 5));
    assert_eq!((cr.runs[1].start, cr.runs[1].len), (5, 4));
    assert_eq!(cr.runs[0].font, none_font());
    assert_eq!(cr.runs[1].font.bold, Some(true));
    assert!(!s.cell_runs.contains_key(&at("B1")));
}

/// Cells whose text carries no formatting deltas get no side-table entry at all.
#[test]
fn plain_cells_have_no_entry() {
    let sst = concat!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">"#,
        r#"<si><t>shared plain</t></si>"#,
        r#"<si><r><t>runs but no rPr</t></r></si>"#,
        r#"</sst>"#,
    );
    let body = concat!(
        r#"<sheetData><row r="1">"#,
        r#"<c r="A1" t="s"><v>0</v></c>"#,
        r#"<c r="B1" t="s"><v>1</v></c>"#,
        r#"<c r="C1" t="inlineStr"><is><t>inline plain</t></is></c>"#,
        r#"<c r="D1"><v>7</v></c>"#,
        r#"<c r="E1" t="str"><f>1+1</f><v>computed</v></c>"#,
        r#"</row></sheetData>"#,
    );
    let (wb, _) = read_xlsx(&sst_package(sst, body)).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.value(at("A1")), Value::text("shared plain"));
    assert_eq!(s.value(at("B1")), Value::text("runs but no rPr"));
    assert!(s.cell_runs.is_empty());
}

/// A rich string cached inside a dynamic array's spill range is emptied so the formula can
/// spill again; no runs entry may survive for text that is no longer there. The anchor keeps
/// both its cached value and its runs.
#[test]
fn spill_override_leaves_no_stale_entry() {
    let sst = concat!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1">"#,
        r#"<si><r><rPr><color rgb="FFFF0000"/></rPr><t>spilled </t></r><r><rPr><b/></rPr><t>rich</t></r></si>"#,
        r#"</sst>"#,
    );
    let body = concat!(
        r#"<sheetData>"#,
        r#"<row r="1"><c r="A1" cm="1" t="s"><f t="array" ref="A1:A2">_xlfn.SEQUENCE(2)</f><v>0</v></c></row>"#,
        r#"<row r="2"><c r="A2" t="s"><v>0</v></c></row>"#,
        r#"</sheetData>"#,
    );
    let (wb, _) = read_xlsx(&sst_package(sst, body)).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.value(at("A1")), Value::text("spilled rich"));
    assert!(s.cell_runs.get(&at("A1")).is_some_and(|cr| cr.valid_for("spilled rich")));
    assert_eq!(s.value(at("A2")), Value::Empty, "the spilled cell's cached value is dropped");
    assert!(!s.cell_runs.contains_key(&at("A2")));
}

// Writing packages: rich runs go back to sharedStrings as `<si><r><rPr>…</rPr><t>…</t></r>…`
// items, deduplicated by text + runs (issue #774).

/// A workbook whose only sheet holds `(cell, text, runs)` triples.
fn runs_workbook(cells: &[(&str, &str, CellRuns)]) -> Workbook {
    let mut s = Sheet::new("Data");
    for (a, text, runs) in cells {
        s.cells.set(at(a), Cell { value: Value::text(*text), formula: None, style: StyleId::DEFAULT, py: None });
        s.cell_runs.insert(at(a), runs.clone());
    }
    let mut wb = Workbook::new();
    wb.sheets = vec![Arc::new(s)];
    wb
}

/// One part of a written package, as text.
fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = String::new();
    z.by_name(name).unwrap().read_to_string(&mut out).unwrap();
    out
}

/// The #774 repro written back: A3's three colour forms and A4's bold+size lead keep their
/// slices (leading spaces preserved), and everything reads back into an equal `cell_runs`.
#[test]
fn runs_roundtrip() {
    let a3 = CellRuns {
        runs: vec![
            Run { start: 0, len: 14, font: RunFont { color: Some(Color::Auto), ..none_font() } },
            Run { start: 14, len: 13, font: RunFont { color: Some(Color::Rgb(0xFF0000)), ..none_font() } },
            Run { start: 27, len: 14, font: RunFont { color: Some(Color::Rgb(0x00B050)), ..none_font() } },
        ],
    };
    let a4 = CellRuns {
        runs: vec![
            Run { start: 0, len: 14, font: RunFont { bold: Some(true), size: Some(14.0), ..none_font() } },
            Run { start: 14, len: 8, font: RunFont { color: Some(Color::Rgb(0xFF0000)), ..none_font() } },
        ],
    };
    let wb = runs_workbook(&[("A3", "this is black, this is red, this is black", a3.clone()), ("A4", "bold fourteen red tail", a4.clone())]);
    let bytes = write_xlsx(&wb).unwrap();
    let xml = part(&bytes, "xl/sharedStrings.xml");
    assert!(xml.contains("<r><rPr><color auto=\"1\"/></rPr><t>this is black,</t></r>"), "{xml}");
    assert!(xml.contains("<r><rPr><color rgb=\"FFFF0000\"/></rPr><t xml:space=\"preserve\"> this is red,</t></r>"), "{xml}");
    assert!(xml.contains("<r><rPr><color rgb=\"FF00B050\"/></rPr><t xml:space=\"preserve\"> this is black</t></r>"), "{xml}");
    assert!(xml.contains("<r><rPr><b/><sz val=\"14\"/></rPr><t xml:space=\"preserve\">bold fourteen </t></r>"), "{xml}");

    let (back, _) = read_xlsx(&bytes).unwrap();
    let s = back.sheet(0).unwrap();
    assert_eq!(s.value(at("A3")), Value::text("this is black, this is red, this is black"));
    assert_eq!(s.value(at("A4")), Value::text("bold fourteen red tail"));
    assert_eq!(s.cell_runs.get(&at("A3")), Some(&a3));
    assert_eq!(s.cell_runs.get(&at("A4")), Some(&a4));
}

/// The reporter's Wingdings shape (issue #774, comment 6131667259): a run in a symbol font
/// (`è` spells a check mark in Wingdings) followed by a normal-font run. Both `rFont` values
/// must survive the round-trip — pre-fix, run font families were dropped on write.
#[test]
fn wingdings_rfont_roundtrip() {
    // "è ok": the Wingdings `è` is 2 bytes, " ok" is 3.
    let runs = CellRuns {
        runs: vec![
            Run { start: 0, len: 2, font: RunFont { name: Some("Wingdings".into()), ..none_font() } },
            Run { start: 2, len: 3, font: RunFont { name: Some("Calibri".into()), ..none_font() } },
        ],
    };
    let wb = runs_workbook(&[("A1", "è ok", runs.clone())]);
    let bytes = write_xlsx(&wb).unwrap();
    let xml = part(&bytes, "xl/sharedStrings.xml");
    // `run_rpr_xml` emits `rFont` after any color/sz; with only a name set, it is the sole child.
    assert!(xml.contains("<r><rPr><rFont val=\"Wingdings\"/></rPr><t>è</t></r>"), "{xml}");
    assert!(xml.contains("<r><rPr><rFont val=\"Calibri\"/></rPr><t xml:space=\"preserve\"> ok</t></r>"), "{xml}");

    let (back, _) = read_xlsx(&bytes).unwrap();
    let s = back.sheet(0).unwrap();
    assert_eq!(s.value(at("A1")), Value::text("è ok"));
    assert_eq!(s.cell_runs.get(&at("A1")), Some(&runs));
    assert_eq!(s.cell_runs.get(&at("A1")).unwrap().runs[0].font.name.as_deref(), Some("Wingdings"));
    assert_eq!(s.cell_runs.get(&at("A1")).unwrap().runs[1].font.name.as_deref(), Some("Calibri"));
}

/// Identical text and runs share one `<si>`; the same text with different runs gets its own.
#[test]
fn sst_dedup_shares_and_splits() {
    let text = "red text";
    let colored = |c: Color| CellRuns {
        runs: vec![Run { start: 0, len: 3, font: RunFont { color: Some(c), ..none_font() } }, Run { start: 3, len: 5, font: none_font() }],
    };
    let (red, green) = (colored(Color::Rgb(0xFF0000)), colored(Color::Rgb(0x00B050)));

    let wb = runs_workbook(&[("A1", text, red.clone()), ("B1", text, red.clone())]);
    let xml = part(&write_xlsx(&wb).unwrap(), "xl/sharedStrings.xml");
    assert!(xml.contains("count=\"2\" uniqueCount=\"1\""), "{xml}");
    assert_eq!(xml.matches("<si>").count(), 1, "{xml}");

    let wb = runs_workbook(&[("A1", text, red.clone()), ("B1", text, green.clone())]);
    let bytes = write_xlsx(&wb).unwrap();
    let xml = part(&bytes, "xl/sharedStrings.xml");
    assert!(xml.contains("count=\"2\" uniqueCount=\"2\""), "{xml}");
    assert_eq!(xml.matches("<si>").count(), 2, "{xml}");
    assert!(xml.contains("FFFF0000") && xml.contains("FF00B050"), "{xml}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    let s = back.sheet(0).unwrap();
    assert_eq!(s.cell_runs.get(&at("A1")), Some(&red));
    assert_eq!(s.cell_runs.get(&at("B1")), Some(&green));
}

/// A side-table entry that does not line up with the cell's text is written as plain text.
#[test]
fn invalid_runs_write_plain() {
    let stale = CellRuns { runs: vec![Run { start: 0, len: 5, font: RunFont { bold: Some(true), ..none_font() } }] };
    let wb = runs_workbook(&[("A1", "hello world", stale)]);
    let bytes = write_xlsx(&wb).unwrap();
    let xml = part(&bytes, "xl/sharedStrings.xml");
    assert!(xml.contains("<si><t>hello world</t></si>"), "{xml}");
    assert!(!xml.contains("<r>"), "{xml}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    let s = back.sheet(0).unwrap();
    assert_eq!(s.value(at("A1")), Value::text("hello world"));
    assert!(s.cell_runs.is_empty());
}

/// `_xHHHH_` escapes decode before the offsets are measured, so a formatted run whose text holds
/// an escape keeps its formatting (the offsets are in the decoded text's coordinates).
#[test]
fn si_parts_offsets_follow_decoded_text() {
    // An escape in the unformatted run and one inside the formatted run: `_x000D_` is `\r`
    // (7 bytes -> 1). Excel writes unformatted parts as `<r>` without `<rPr>`.
    let el = si(concat!(r#"<si><r><t>a_x000D_b</t></r>"#, r#"<r><rPr><b/></rPr><t>c_x000D_d</t></r></si>"#,));
    let (text, parts) = si_parts(&el);
    assert_eq!(text, "a\rbc\rd");
    assert_eq!(parts.len(), 2);
    assert_eq!((parts[0].start, parts[0].len), (0, 3));
    assert_eq!((parts[1].start, parts[1].len), (3, 3));

    let mut budget = 1 << 20;
    let Si { text: t, runs } = Si::parse(&el, &INDEXED, &mut budget);
    assert_eq!(&*t, "a\rbc\rd");
    let cr = runs.expect("the escape does not drop the runs");
    assert!(cr.valid_for(&t));
    assert_eq!(cr.run_text(&t, 1), Some("c\rd"));
}

/// A literal escape sequence split across two `<t>` nodes is not a sequence in either node, so it
/// stays literal: the round trip through GridCraft's own writer keeps both the text and the runs.
#[test]
fn a_split_escape_sequence_round_trips_literal() {
    // Run 0 holds `_x00`, run 1 holds `41_`: neither slice carries a complete `_xHHHH_`.
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    let text = "_x0041_";
    let runs = CellRuns {
        runs: vec![Run { start: 0, len: 4, font: RunFont { bold: Some(true), ..none_font() } }, Run { start: 4, len: 3, font: none_font() }],
    };
    sh.cells.set(at("A1"), Cell { value: Value::text(text), formula: None, style: Default::default(), py: None });
    sh.cell_runs.insert(at("A1"), runs);
    let bytes = write_xlsx(&wb).unwrap();
    let (back, rep) = read_xlsx(&bytes).unwrap();
    assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
    let sh = back.sheet(0).unwrap();
    assert_eq!(sh.value(at("A1")), Value::text("_x0041_"), "the split sequence stays literal");
    let cr = sh.cell_runs.get(&at("A1")).expect("the runs survive the round trip");
    assert!(cr.valid_for("_x0041_"));
}
