//! A saved workbook with Python pictures, read by a real openpyxl outside GridCraft, which is how a
//! script or an agent reads one (a cell's own Python can't open a file). It runs on the interpreter in
//! `GRIDCRAFT_TEST_PYTHON`, not sandboxed, and passes with a note when that is unset or lacks openpyxl
//! or Pillow. The file is built in code through a `Session`, with pictures that are real PNGs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_engine::model::{Anchor, Cell, CellPicture, Image, PyCode, PyImage, PyOutput};
use serde_json::{Value as Json, json};

const SHEET: &str = "It's a! sheet";
const REFERENCE: &str = "'It''s a! sheet'!$G$5";
const WARNING: &str = "Unknown type for GridCraft: this workbook has Python cells. openpyxl cannot read their code or in-cell pictures, and wb.save() removes them. See wb.custom_doc_props.";

/// What openpyxl gives, as JSON on its standard output.
const SCRIPT: &str = r#"
import json, sys, warnings, zipfile
from openpyxl import load_workbook
from openpyxl.utils import get_column_letter

path, saved, saved_after_data = sys.argv[1:4]

def images(ws):
    out = []
    for image in ws._images:
        anchor = image.anchor
        props = anchor.pic.nvPicPr.cNvPr
        out.append({
            "cell": f"{get_column_letter(anchor._from.col + 1)}{anchor._from.row + 1}",
            "anchor": type(anchor).__name__,
            "hidden": bool(props.hidden),
            "name": props.name,
            "descr": props.descr,
        })
    return sorted(out, key=lambda i: i["cell"])

def loaded(path, **options):
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        wb = load_workbook(path, **options)
    return wb, [str(w.message) for w in caught]

report = {}
wb, report["warnings"] = loaded(path)
ws = wb.worksheets[0]
report["sheet"] = ws.title
report["notice"] = wb.custom_doc_props["GridCraft.Python.Notice"].value
report["properties"] = wb.custom_doc_props.names
report["images"] = images(ws)
report["formulas"] = {cell: ws[cell].value for cell in ("G5", "H7")}
values, _ = loaded(path, data_only=True)
report["values"] = {cell: values.worksheets[0][cell].value for cell in ("G5", "H7")}
report["read_only_warnings"] = loaded(path, read_only=True)[1]
report["code"] = zipfile.ZipFile(path).read("xl/python.xml").decode()
try:
    import pandas
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        pandas.read_excel(path)
    report["pandas_warnings"] = [str(w.message) for w in caught]
except ImportError:
    pass

wb.save(saved)
after, report["after_save_warnings"] = loaded(saved)
report["after_save_properties"] = after.custom_doc_props.names
report["after_save_images"] = images(after.worksheets[0])
report["after_save_parts"] = sorted(n for n in zipfile.ZipFile(saved).namelist() if "python" in n or "richData" in n or "metadata" in n)

spent, _ = loaded(path)
spent.worksheets[0]._images[0]._data()
try:
    spent.save(saved_after_data)
    report["save_after_data"] = "saved"
except ValueError as e:
    report["save_after_data"] = f"ValueError: {e}"

print(json.dumps(report))
"#;

/// A real PNG of one colour: a stored zlib stream, with its checksums.
fn png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut crc = !0u32;
        for &byte in kind.iter().chain(body) {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        out.extend_from_slice(&(!crc).to_be_bytes());
    }
    let mut row = vec![0];
    row.extend(rgb.repeat(width as usize));
    let raw = row.repeat(height as usize);
    assert!(raw.len() <= 65_535, "one stored block");
    let mut zlib = vec![0x78, 0x01, 1];
    zlib.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    zlib.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    zlib.extend_from_slice(&raw);
    let (mut a, mut b) = (1u32, 0u32);
    for &n in &raw {
        a = (a + u32::from(n)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = vec![];
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn at(cell: &str) -> CellRef {
    CellRef::parse(cell).unwrap()
}

/// The result a cell keeps from a run that gave this picture, at a density of 2.
fn picture_result(data: Vec<u8>, width: u32, height: u32) -> PyOutput {
    let alt = format!("Image {width} x {height}");
    let picture = Arc::new(CellPicture { data, mime: "image/png".into(), alt });
    let image = PyImage { picture, width, height, density: 2, floor: None, fitted: None };
    PyOutput {
        value: Value::text("Image"),
        type_name: "Image".into(),
        preview: "Image".into(),
        from_run: true,
        image: Some(image),
        ..PyOutput::default()
    }
}

fn object_picture() -> Vec<u8> {
    png(80, 60, [10, 120, 200])
}

fn value_picture() -> Vec<u8> {
    png(90, 70, [200, 120, 10])
}

/// A workbook with a Python Object picture on G5, an Excel Value picture on H7 and a floating picture
/// on J2 linked to G5, on a sheet whose name holds what a marker must get past.
fn workbook() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    let doc = s.doc_mut().unwrap();
    let sheet = Arc::make_mut(&mut doc.wb).sheet_mut(0).unwrap();
    sheet.name = SHEET.into();
    let cell = |object: bool, output: PyOutput| Cell {
        value: Value::text("Image"),
        formula: None,
        style: Default::default(),
        py: Some(Arc::new(PyCode { object, output_chosen: true, output: Some(output), ..PyCode::new("plot()") })),
    };
    sheet.cells.set(at("G5"), cell(true, picture_result(object_picture(), 40, 30)));
    sheet.cells.set(at("H7"), cell(false, picture_result(value_picture(), 45, 35)));
    sheet.images.push(Image {
        id: 100,
        anchor: Anchor { cell: at("J2"), width: 40.0, height: 30.0, ..Default::default() },
        data: Arc::new(object_picture()),
        mime: "image/png".into(),
        alt: "Python result of G5".into(),
        python_source: Some(REFERENCE.into()),
        rendered: None,
    });
    s
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("gridcraft-python-openpyxl-{}-{name}", std::process::id()))
}

fn save_as(s: &mut Session, path: &Path) {
    s.execute("file.saveAs", json!({"path": path})).unwrap();
}

/// A session without Python that has opened the file: what the file holds.
fn opened(path: &Path) -> (Session, Json) {
    let mut s = Session::new();
    let reply = s.execute("file.open", json!({"path": path})).unwrap();
    (s, reply)
}

fn image_reply(s: &mut Session, cell: &str) -> (u64, u64, u64, String) {
    let r = s.execute("python.image", json!({"cell": cell})).unwrap();
    (r["width"].as_u64().unwrap(), r["height"].as_u64().unwrap(), r["density"].as_u64().unwrap(), r["base64"].as_str().unwrap().to_string())
}

#[test]
fn a_reopened_workbook_shows_its_saved_pictures_without_running_code() {
    let path = temp("reopened.xlsx");
    save_as(&mut workbook(), &path);
    let (mut s, reply) = opened(&path);
    assert_eq!(reply["warnings"], json!([]), "{reply}");
    let base64 = gridcraft_engine::io::base64_encode;
    assert_eq!(image_reply(&mut s, "G5"), (40, 30, 2, base64(&object_picture())));
    assert_eq!(image_reply(&mut s, "H7"), (45, 35, 2, base64(&value_picture())));
    let doc = s.doc().unwrap();
    let sheet = doc.wb.sheet(0).unwrap();
    assert_eq!(sheet.name, SHEET);
    assert_eq!(sheet.value(at("G5")), Value::text("Image"));
    assert_eq!(sheet.value(at("H7")), Value::text("Image"));
    assert!(sheet.picture_at(at("G5")).is_none());
    assert_eq!(sheet.picture_at(at("H7")).unwrap().data, value_picture());
    assert!(sheet.cell_pictures.is_empty());
    assert_eq!(sheet.images.len(), 1, "the companions aren't floating pictures");
    assert_eq!(sheet.images[0].python_source.as_deref(), Some(REFERENCE));
    assert_eq!(sheet.images[0].alt, "Python result of G5");
    assert_eq!(s.execute("cell.get", json!({"cell": "H7"})).unwrap()["python"]["output"], "value");
    // The package is sound: content types, relationships and what they name.
    let check = Command::new(env!("CARGO_BIN_EXE_gridcraft-cli")).arg("check").arg(&path).arg("--json").output().unwrap();
    assert!(check.status.success(), "{}{}", String::from_utf8_lossy(&check.stdout), String::from_utf8_lossy(&check.stderr));
    std::fs::remove_file(&path).ok();
}

#[test]
fn openpyxl_reads_what_a_saved_workbook_holds_and_a_save_through_it_loses_no_picture() {
    let Some(python) = std::env::var_os("GRIDCRAFT_TEST_PYTHON") else {
        eprintln!("GRIDCRAFT_TEST_PYTHON isn't set; skipping the test that needs openpyxl");
        return;
    };
    let has = Command::new(&python).args(["-I", "-c", "import openpyxl, PIL"]).output().is_ok_and(|o| o.status.success());
    if !has {
        eprintln!("GRIDCRAFT_TEST_PYTHON has no openpyxl or Pillow; skipping");
        return;
    }
    let (path, saved, spent) = (temp("source.xlsx"), temp("saved.xlsx"), temp("spent.xlsx"));
    save_as(&mut workbook(), &path);
    let run = Command::new(&python).args(["-I", "-c", SCRIPT]).arg(&path).arg(&saved).arg(&spent).output().unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{stderr}");
    let out = String::from_utf8_lossy(&run.stdout);
    eprintln!("openpyxl: {out}");
    let report: Json = serde_json::from_str(out.trim()).unwrap();

    // At load, and however it loads, the warning says what it can't do.
    assert_eq!(report["warnings"], json!([WARNING]), "{report}");
    assert_eq!(report["read_only_warnings"], json!([WARNING]), "{report}");
    if !report["pandas_warnings"].is_null() {
        assert_eq!(report["pandas_warnings"], json!([WARNING]), "{report}");
    }
    assert_eq!(report["sheet"], SHEET);

    // The text says where things are, and only it is left in the list.
    assert_eq!(report["properties"], json!(["GridCraft.Python.Notice"]), "{report}");
    let notice = report["notice"].as_str().unwrap();
    assert!(notice.chars().count() <= 255, "{notice}");
    for word in ["ws._images", "gridcraft:python:", "gridcraft:pythonCell:", "xl/python.xml", "wb.save()"] {
        assert!(notice.contains(word), "{word}: {notice}");
    }

    // The pictures, on their cells, and the markers that say what each is.
    assert_eq!(
        report["images"],
        json!([
            {
                "cell": "G5",
                "anchor": "OneCellAnchor",
                "hidden": true,
                "name": "Python G5",
                "descr": "gridcraft:pythonCell:G5:2:Image 40 x 30"
            },
            {
                "cell": "H7",
                "anchor": "OneCellAnchor",
                "hidden": true,
                "name": "Python H7",
                "descr": "gridcraft:pythonCell:H7:2:Image 45 x 35"
            },
            {
                "cell": "J2",
                "anchor": "TwoCellAnchor",
                "hidden": false,
                "name": "Picture 1",
                "descr": format!("gridcraft:python:{}:{REFERENCE}:Python result of G5", REFERENCE.chars().count())
            }
        ]),
        "{report}"
    );

    // The cells: the formula, and with `data_only` what each return type caches.
    assert_eq!(report["formulas"], json!({"G5": "=_xlfn._xlws.PY(0,1)", "H7": "=_xlfn._xlws.PY(1,0)"}), "{report}");
    assert_eq!(report["values"], json!({"G5": "Image", "H7": "#VALUE!"}), "{report}");
    assert!(report["code"].as_str().unwrap().contains("<code>plot()</code>"), "{report}");

    // What a save through openpyxl keeps: the pictures, with their markers, and the text.
    assert_eq!(report["after_save_warnings"], json!([]), "{report}");
    assert_eq!(report["after_save_properties"], json!(["GridCraft.Python.Notice"]), "{report}");
    assert_eq!(report["after_save_images"], report["images"], "{report}");
    assert_eq!(report["after_save_parts"], json!([]), "the code and the rich values are gone: {report}");
    assert!(report["save_after_data"].as_str().unwrap().starts_with("ValueError"), "{report}");

    // Without Pillow openpyxl drops every picture, with no word about it.
    let no_pillow = Command::new(&python)
        .args([
            "-I",
            "-c",
            "import sys\nsys.modules['PIL'] = None\nfrom openpyxl import load_workbook\nprint(len(load_workbook(sys.argv[1]).worksheets[0]._images))",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&no_pillow.stdout).trim(), "0", "{}", String::from_utf8_lossy(&no_pillow.stderr));

    // Reopened here, no picture was lost, and with no Python cell there is no link and no notice.
    let (mut s, reply) = opened(&saved);
    let warnings = reply["warnings"].to_string();
    assert!(warnings.contains("don't go with what that cell holds") && warnings.contains("lost the link"), "{warnings}");
    let doc = s.doc().unwrap();
    let sheet = doc.wb.sheet(0).unwrap();
    assert!(sheet.cell(at("G5")).is_none_or(|c| c.py.is_none()) && sheet.cell(at("H7")).is_none_or(|c| c.py.is_none()));
    let mut pictures: Vec<(String, Vec<u8>, bool)> =
        sheet.images.iter().map(|o| (o.anchor.cell.a1(), o.data.to_vec(), o.python_source.is_some())).collect();
    pictures.sort();
    assert_eq!(
        pictures,
        [("G5".to_string(), object_picture(), false), ("H7".to_string(), value_picture(), false), ("J2".to_string(), object_picture(), false)]
    );
    let saved_again = temp("again.xlsx");
    save_as(&mut s, &saved_again);
    let (s, _) = opened(&saved_again);
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().images.len(), 3);
    let keep = Command::new(&python)
        .args(["-I", "-c", "import sys, warnings; from openpyxl import load_workbook\nwarnings.simplefilter('error')\nprint(load_workbook(sys.argv[1]).custom_doc_props.names)"])
        .arg(&saved_again)
        .output()
        .unwrap();
    assert!(keep.status.success(), "{}", String::from_utf8_lossy(&keep.stderr));
    assert_eq!(String::from_utf8_lossy(&keep.stdout).trim(), "[]", "a workbook with no Python cells has no notices");

    for file in [&path, &saved, &spent, &saved_again] {
        std::fs::remove_file(file).ok();
    }
}

/// A package with a part's bytes replaced.
fn with_part(path: &Path, part: &str, data: &[u8]) -> Vec<u8> {
    use std::io::{Read, Write};
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(std::fs::read(path).unwrap())).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut entry = z.by_index(i).unwrap();
        let mut bytes = vec![];
        entry.read_to_end(&mut bytes).unwrap();
        out.start_file(entry.name(), zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(if entry.name() == part { data } else { &bytes }).unwrap();
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn openpyxl_reads_the_notices_beside_a_persons_own_properties() {
    let Some(python) = std::env::var_os("GRIDCRAFT_TEST_PYTHON") else {
        eprintln!("GRIDCRAFT_TEST_PYTHON isn't set; skipping the test that needs openpyxl");
        return;
    };
    let has = Command::new(&python).args(["-I", "-c", "import openpyxl, PIL"]).output().is_ok_and(|o| o.status.success());
    if !has {
        eprintln!("GRIDCRAFT_TEST_PYTHON has no openpyxl or Pillow; skipping");
        return;
    }
    const READ: &str = "import json, sys, warnings\nfrom openpyxl import load_workbook\nwith warnings.catch_warnings(record=True) as caught:\n    warnings.simplefilter('always')\n    wb = load_workbook(sys.argv[1])\nprint(json.dumps({'warnings': [str(w.message) for w in caught], 'properties': {p.name: p.value for p in wb.custom_doc_props}}))";
    // A person's property in the shape openpyxl itself writes: the root declares no prefix for the types.
    let part = r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties"><property name="Client &amp; year" pid="2" fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}"><vt:lpwstr xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">A &lt;b&gt;</vt:lpwstr></property></Properties>"#;
    let (source, merged) = (temp("persons-source.xlsx"), temp("persons-merged.xlsx"));
    save_as(&mut workbook(), &source);
    std::fs::write(&source, with_part(&source, "docProps/custom.xml", part.as_bytes())).unwrap();
    let (mut s, _) = opened(&source);
    save_as(&mut s, &merged);
    let run = Command::new(&python).args(["-I", "-c", READ]).arg(&merged).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let report: Json = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(report["warnings"], json!([WARNING]), "{report}");
    assert_eq!(report["properties"]["Client & year"], "A <b>", "{report}");
    assert!(report["properties"]["GridCraft.Python.Notice"].as_str().is_some_and(|n| n.contains("<ref>")), "{report}");
    assert_eq!(report["properties"].as_object().unwrap().len(), 2, "{report}");
    for file in [&source, &merged] {
        std::fs::remove_file(file).ok();
    }
}
