//! The pictures of Python cells in a file: linked floating pictures, and what hostile files do to
//! them. The pictures here are real PNGs, built in code.

use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Anchor, Cell, CellPicture, Image, PyCode, PyImage, PyOutput, Sheet, Workbook};

use super::make_zip;
use crate::{read_xlsx, write_xlsx};

pub(super) fn at(cell: &str) -> CellRef {
    CellRef::parse(cell).unwrap()
}

/// A PNG of `width` x `height` pixels. Only its header is read here, so its data can be short.
pub(super) fn png(width: u32, height: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    v.extend_from_slice(&width.to_be_bytes());
    v.extend_from_slice(&height.to_be_bytes());
    v.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    v
}

pub(super) fn text(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

pub(super) fn python_cell(sheet: &mut Sheet, cell: &str, py: PyCode, value: Value) {
    sheet.cells.set(at(cell), Cell { value, formula: None, style: Default::default(), py: Some(Arc::new(py)) });
}

fn linked_picture(id: u32, source: &str, alt: &str, data: &[u8]) -> Image {
    let anchor = Anchor { cell: at("B2"), width: 40.0, height: 30.0, ..Default::default() };
    Image { id, anchor, data: data.to_vec().into(), mime: "image/png".into(), alt: alt.into(), python_source: Some(source.into()), rendered: None }
}

pub(super) fn book(sheets: Vec<Sheet>) -> Workbook {
    Workbook { sheets: sheets.into_iter().map(Arc::new).collect(), ..Default::default() }
}

/// A one-sheet package, hand written, whose drawing holds pictures on B2.
struct Package<'a> {
    /// One picture for each description.
    descriptions: &'a [&'a str],
    /// The cell XML of row 2.
    cells: &'a str,
    /// Script 0 of its Python part.
    script: String,
    /// The bytes of its one media part.
    picture: Vec<u8>,
}

impl Package<'_> {
    fn bytes(&self) -> Vec<u8> {
        let workbook = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.microsoft.com/office/2023/09/relationships/Python" Target="python.xml"/></Relationships>"#;
        let python = format!(
            r#"<python xmlns="http://schemas.microsoft.com/office/spreadsheetml/2023/python"><pythonScripts><pythonScript><code>{}</code></pythonScript></pythonScripts></python>"#,
            self.script
        );
        let sheet = format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData><row r="2">{}</row></sheetData><drawing r:id="rId1"/></worksheet>"#,
            self.cells
        );
        let sheet_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
        let pictures: String = self
            .descriptions
            .iter()
            .map(|descr| {
                format!(
                    r#"<xdr:oneCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx="952500" cy="952500"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" name="P" descr="{}"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill><xdr:spPr/></xdr:pic><xdr:clientData/></xdr:oneCellAnchor>"#,
                    crate::xml::esc_attr(descr)
                )
            })
            .collect();
        let drawing = format!(
            r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{pictures}</xdr:wsDr>"#
        );
        let drawing_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#;
        make_zip(&[
            ("_rels/.rels", super::ROOT_RELS.as_bytes()),
            ("xl/workbook.xml", workbook.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/python.xml", python.as_bytes()),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
            ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels.as_bytes()),
            ("xl/drawings/drawing1.xml", drawing.as_bytes()),
            ("xl/drawings/_rels/drawing1.xml.rels", drawing_rels.as_bytes()),
            ("xl/media/image1.png", self.picture.as_slice()),
        ])
    }
}

/// A package with one picture, whose script is `1 + 1`.
fn package_with_picture(descr: &str, cells: &str) -> Vec<u8> {
    Package { descriptions: &[descr], cells, script: "1 + 1".into(), picture: png(40, 30) }.bytes()
}

/// A package as it is, with these parts replaced or added.
pub(super) fn repack(bytes: &[u8], changes: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = vec![];
    for i in 0..z.len() {
        let mut part = z.by_index(i).unwrap();
        let mut data = vec![];
        part.read_to_end(&mut data).unwrap();
        parts.push((part.name().to_string(), data));
    }
    for (name, data) in changes {
        parts.retain(|(n, _)| n != name);
        parts.push((name.to_string(), data.to_vec()));
    }
    let parts: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    make_zip(&parts)
}

/// A package with one part's text changed.
pub(super) fn with_text_changed(bytes: &[u8], part: &str, from: &str, to: &str) -> Vec<u8> {
    let was = text(bytes, part);
    assert!(was.contains(from), "{was}");
    repack(bytes, &[(part, was.replace(from, to).as_bytes())])
}

pub(super) fn image_of(data: &[u8], density: u32) -> PyImage {
    let (width, height, mime) = crate::richdata::picture_size(data).unwrap();
    let picture = CellPicture { data: data.to_vec(), mime: mime.to_string(), alt: "Image <plot> 40 x 30".to_string() };
    PyImage { picture: Arc::new(picture), width: width / density, height: height / density, density, floor: None, fitted: None }
}

/// A Python cell whose last run gave this picture, with a chosen return type.
pub(super) fn image_cell(sheet: &mut Sheet, cell: &str, object: bool, image: PyImage) {
    let output = PyOutput {
        value: Value::text("Image"),
        type_name: "Image".into(),
        preview: "Image".into(),
        from_run: true,
        image: Some(image),
        ..PyOutput::default()
    };
    python_cell(sheet, cell, PyCode { object, output_chosen: true, output: Some(output), ..PyCode::new("plot()") }, Value::text("Image"));
}

fn py_at<'a>(sheet: &'a Sheet, cell: &str) -> &'a PyCode {
    sheet.cell(at(cell)).unwrap().py.as_deref().unwrap()
}

fn media_parts(bytes: &[u8]) -> Vec<String> {
    let z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    z.file_names().filter(|n| n.starts_with("xl/media/")).map(str::to_string).collect()
}

pub(super) fn part_names(bytes: &[u8]) -> Vec<String> {
    let z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    z.file_names().map(str::to_string).collect()
}

/// The Python Object cell B2 of [`package_with_picture`], its code being its one script.
const OBJECT_CELL: &str = r#"<c r="B2" t="str"><f>_xlfn._xlws.PY(0,1)</f><v>Image</v></c>"#;

#[test]
fn a_linked_picture_keeps_its_source_through_a_file() {
    // A sheet name can hold every separator a marker might use.
    let reference = "'It''s a! test sheet'!$G$5";
    let mut source = Sheet::new("It's a! test sheet");
    python_cell(&mut source, "G5", PyCode { object: true, output_chosen: true, ..PyCode::new("1") }, Value::text("Image"));
    let mut other = Sheet::new("Other");
    let picture = png(40, 30);
    other.images.push(linked_picture(1, reference, "A \"plot\": <one>!", &picture));
    let bytes = write_xlsx(&book(vec![source, other])).unwrap();

    // Any other application sees a normal picture, with the marker in its description.
    let drawing = text(&bytes, "xl/drawings/drawing1.xml");
    assert!(drawing.contains("<xdr:pic>"), "{drawing}");
    assert!(drawing.contains(&format!("descr=\"gridcraft:python:{}:", reference.chars().count())), "{drawing}");

    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("picture")), "{:?}", report.warnings);
    let image = &back.sheets[1].images[0];
    assert_eq!(image.python_source.as_deref(), Some(reference));
    assert_eq!(image.alt, "A \"plot\": <one>!");
    assert_eq!(image.data.as_slice(), picture);

    // A second save says the same.
    let again = write_xlsx(&back).unwrap();
    assert_eq!(text(&again, "xl/drawings/drawing1.xml"), drawing);
}

#[test]
fn a_link_is_read_by_its_length_whatever_the_text_around_it_holds() {
    use crate::python_pictures::{linked_descr, read_link};
    for (reference, alt) in [
        ("Data!$G$5", ""),
        ("Data!$G$5", ":::!!!"),
        ("'a:b'!$A$1", "'x'!$B$2"),
        ("'a!b''c d'!$XFD$1048576", "\u{1F600} picture"),
        ("'\u{1F600}'!A1", "12:'x'!A1:y"),
    ] {
        let descr = linked_descr(reference, alt).unwrap();
        assert_eq!(read_link(&descr), Some((reference, alt)), "{descr}");
    }
    // What a file can't hold is saved as a plain picture.
    assert_eq!(linked_descr("#REF!", "x"), None);
    assert_eq!(linked_descr("Data", "x"), None);
    assert_eq!(linked_descr(&format!("'{}'!A1", "n".repeat(200)), "x"), None);
    // A character XML can't carry never reaches the file, and isn't counted.
    let descr = linked_descr("'a\u{1}b'!A1", "x").unwrap();
    assert_eq!(read_link(&descr), Some(("'ab'!A1", "x")));
}

#[test]
fn a_malformed_link_leaves_a_plain_picture_with_the_description_as_its_text() {
    for descr in [
        "gridcraft:python:",
        "gridcraft:python:9",
        "gridcraft:python::Data!A1:x",
        "gridcraft:python:+7:Data!B2:x",
        "gridcraft:python:-7:Data!B2:x",
        "gridcraft:python:007x:Data!B2:x",
        "gridcraft:python:99999999999999999999999:Data!B2:x",
        "gridcraft:python:999:Data!B2:x",
        "gridcraft:python:8:Data!B2",
        "gridcraft:python:3:Data!B2:x",
        "gridcraft:python:7:Data!B2x",
        "gridcraft:python:5:#REF!:x",
        "gridcraft:python:2:B2:x",
        "gridcraft:python:3:!B2:x",
        "gridcraft:python:9:Data!B2:C:x",
        "gridcraft:python:18:Data!ZZZZ999999999:x",
        "gridcraft:python:6:\u{1F600}!B2:x",
    ] {
        let bytes = package_with_picture(descr, OBJECT_CELL);
        let (back, _) = read_xlsx(&bytes).unwrap();
        let images = &back.sheets[0].images;
        assert_eq!(images.len(), 1, "{descr}");
        assert_eq!(images[0].python_source, None, "{descr}");
        assert_eq!(images[0].alt, descr);
    }
}

#[test]
fn opening_a_file_never_deletes_a_picture_whose_link_names_no_python_cell() {
    for (reference, cells) in [
        ("Data!$B$2", r#"<c r="B2"><v>5</v></c>"#),
        ("Data!$C$9", OBJECT_CELL),
        ("Nowhere!$B$2", OBJECT_CELL),
        ("'Da''ta'!$B$2", OBJECT_CELL),
        ("'\u{1F600}!$B$2", OBJECT_CELL),
    ] {
        let descr = format!("gridcraft:python:{}:{reference}:my text", reference.chars().count());
        let (back, report) = read_xlsx(&package_with_picture(&descr, cells)).unwrap();
        let image = &back.sheets[0].images[0];
        assert_eq!(image.python_source, None, "{reference}");
        assert_eq!(image.alt, "my text", "{reference}");
        assert_eq!(image.data.as_slice(), png(40, 30));
        assert!(report.warnings.iter().any(|w| w.contains("lost the link")), "{:?}", report.warnings);
    }
    // A Python cell keeps it, in whichever case the reference spells the sheet.
    let descr = "gridcraft:python:9:DATA!$B$2:my text";
    let (back, report) = read_xlsx(&package_with_picture(descr, OBJECT_CELL)).unwrap();
    assert_eq!(back.sheets[0].images[0].python_source.as_deref(), Some("DATA!$B$2"));
    assert!(!report.warnings.iter().any(|w| w.contains("lost the link")), "{:?}", report.warnings);
}

#[test]
fn a_picture_saved_with_a_link_a_file_cannot_hold_is_a_plain_picture() {
    let mut sheet = Sheet::new("Data");
    sheet.images.push(linked_picture(1, "#REF!", "my text", &png(40, 30)));
    let (back, _) = read_xlsx(&write_xlsx(&book(vec![sheet])).unwrap()).unwrap();
    assert_eq!(back.sheets[0].images[0].python_source, None);
    assert_eq!(back.sheets[0].images[0].alt, "my text");
}

#[test]
fn a_python_cell_keeps_its_picture_through_a_file_for_either_return_type() {
    let (object, value) = (png(80, 60), png(90, 70));
    let mut sheet = Sheet::new("Data");
    image_cell(&mut sheet, "G5", true, image_of(&object, 2));
    image_cell(&mut sheet, "H7", false, image_of(&value, 2));
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();

    // The cells: the text for a Python Object, the rich value of cell pictures for an Excel Value.
    let cells = text(&bytes, "xl/worksheets/sheet1.xml");
    assert!(cells.contains(r#"<c r="G5" t="str"><f>_xlfn._xlws.PY(0,1)</f><v>Image</v></c>"#), "{cells}");
    assert!(cells.contains(r#"<c r="H7" vm="1" t="e"><f>_xlfn._xlws.PY(1,0)</f><v>#VALUE!</v></c>"#), "{cells}");
    // The companions: hidden, on their cells, named for them, saying what the file holds nowhere else.
    let drawing = text(&bytes, "xl/drawings/drawing1.xml");
    assert_eq!(drawing.matches("<xdr:oneCellAnchor>").count(), 2, "{drawing}");
    assert!(drawing.contains(r#"<xdr:from><xdr:col>6</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>4</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx="381000" cy="285750"/>"#), "{drawing}");
    assert!(drawing.contains(r#"name="Python G5" descr="gridcraft:pythonCell:G5:2:Image &lt;plot&gt; 40 x 30" hidden="1""#), "{drawing}");
    assert!(drawing.contains(r#"name="Python H7" descr="gridcraft:pythonCell:H7:2:Image &lt;plot&gt; 40 x 30" hidden="1""#), "{drawing}");

    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("picture")), "{:?}", report.warnings);
    let sheet = &back.sheets[0];
    assert!(sheet.images.is_empty(), "a companion is no floating picture");
    assert!(sheet.cell_pictures.is_empty(), "a picture with code is no plain cell picture");
    for (cell, data, width, height, object) in [("G5", &object, 40, 30, true), ("H7", &value, 45, 35, false)] {
        let py = py_at(sheet, cell);
        assert_eq!((py.code.as_str(), py.object, py.shows_object()), ("plot()", object, object), "{cell}");
        let output = py.output.as_ref().unwrap();
        assert_eq!((output.value.clone(), output.type_name.as_str()), (Value::text("Image"), "Image"), "{cell}");
        assert!(!output.convertible && !output.from_run, "{cell}");
        let image = py.image().unwrap();
        assert_eq!((image.width, image.height, image.density), (width, height, 2), "{cell}");
        assert_eq!(image.picture.data, *data, "{cell}");
        assert_eq!((image.picture.mime.as_str(), image.picture.alt.as_str()), ("image/png", "Image <plot> 40 x 30"), "{cell}");
        assert_eq!(sheet.cell(at(cell)).unwrap().value, Value::text("Image"), "{cell}");
    }
    assert!(sheet.picture_at(at("G5")).is_none() && sheet.picture_at(at("H7")).is_some());
    assert!(sheet.input_text(at("H7")).contains("plot()"), "the formula bar has the code: {}", sheet.input_text(at("H7")));

    // A second save says the same.
    let again = write_xlsx(&back).unwrap();
    assert_eq!(text(&again, "xl/drawings/drawing1.xml"), drawing);
    assert_eq!(text(&again, "xl/worksheets/sheet1.xml"), cells);
}

/// The rich value and the companion have the same bytes, or the reader would float the companion and
/// lose the density: the copy fitted to the cell stays out of the file.
#[test]
fn an_excel_value_with_a_fitted_copy_keeps_its_picture_and_density_through_a_file() {
    let (full, small) = (png(80, 60), png(40, 30));
    let mut image = image_of(&full, 2);
    let copy = CellPicture { data: small, mime: "image/png".to_string(), alt: image.picture.alt.clone() };
    image.fitted = Some((Arc::new(copy), [40, 30]));
    let mut sheet = Sheet::new("Data");
    image_cell(&mut sheet, "H7", false, image);
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();

    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("picture")), "{:?}", report.warnings);
    let sheet = &back.sheets[0];
    assert!(sheet.images.is_empty() && sheet.cell_pictures.is_empty(), "a companion is no floating picture");
    let py = py_at(sheet, "H7");
    assert!(!py.shows_object());
    let image = py.image().unwrap();
    assert_eq!((image.width, image.height, image.density), (40, 30, 2));
    assert_eq!(image.picture.data, full);
}

/// The word `Image` from an Excel Value is a text the code gave, unless the file says it stands in for
/// a picture with no room for a rich value.
#[test]
fn an_excel_value_whose_result_is_the_word_keeps_it_and_the_old_companion_floats() {
    let mut sheet = Sheet::new("Data");
    image_cell(&mut sheet, "H7", false, image_of(&png(80, 60), 2));
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();
    let was = r#"<c r="H7" vm="1" t="e"><f>_xlfn._xlws.PY(0,0)</f><v>#VALUE!</v></c>"#;
    let bytes = with_text_changed(&bytes, "xl/worksheets/sheet1.xml", was, r#"<c r="H7" t="str"><f>_xlfn._xlws.PY(0,0)</f><v>Image</v></c>"#);
    let (back, report) = read_xlsx(&bytes).unwrap();
    let sheet = &back.sheets[0];
    assert!(py_at(sheet, "H7").image().is_none());
    assert_eq!(sheet.cell(at("H7")).unwrap().value, Value::text("Image"));
    assert_eq!(sheet.images.len(), 1, "the picture isn't lost");
    assert!(report.warnings.iter().any(|w| w.contains("don't go with what that cell holds")), "{:?}", report.warnings);
}

#[test]
fn a_follower_of_a_shared_python_formula_stays_a_python_cell() {
    let data = png(80, 60);
    let mut sheet = Sheet::new("Data");
    image_cell(&mut sheet, "H7", false, image_of(&data, 2));
    image_cell(&mut sheet, "H8", false, image_of(&data, 2));
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();
    let part = "xl/worksheets/sheet1.xml";
    let bytes = with_text_changed(&bytes, part, "<f>_xlfn._xlws.PY(0,0)</f>", r#"<f t="shared" ref="H7:H8" si="0">_xlfn._xlws.PY(0,0)</f>"#);
    for follower in [r#"<f t="shared" si="0"/>"#, r#"<f t="shared" si="0">  </f>"#] {
        let bytes = with_text_changed(&bytes, part, "<f>_xlfn._xlws.PY(1,0)</f>", follower);
        let (back, _) = read_xlsx(&bytes).unwrap();
        let sheet = &back.sheets[0];
        assert!(sheet.cell_pictures.is_empty(), "no cell became a static picture: {follower}");
        assert_eq!(py_at(sheet, "H8").code, "plot()", "{follower}");
        assert_eq!(py_at(sheet, "H8").image().unwrap().picture.data, data, "{follower}");
    }
}

/// The older `PY("…",0)` form is read from the formula's text. A follower reads the master's rather
/// than one printed from the parsed formula, which can say something else: `0.0` is printed `0`, so
/// the follower alone would become a Python cell.
#[test]
fn a_follower_of_a_shared_literal_python_formula_reads_the_masters_text() {
    let read = |master: &str| {
        let mut sheet = Sheet::new("Data");
        python_cell(&mut sheet, "A1", PyCode::new("x"), Value::text("x"));
        python_cell(&mut sheet, "A2", PyCode::new("y"), Value::text("y"));
        let bytes = write_xlsx(&book(vec![sheet])).unwrap();
        let part = "xl/worksheets/sheet1.xml";
        let master = format!("<f t=\"shared\" ref=\"A1:A2\" si=\"0\">_xlfn._xlws.{master}</f>");
        let bytes = with_text_changed(&bytes, part, "<f>_xlfn._xlws.PY(0,0)</f>", &master);
        let bytes = with_text_changed(&bytes, part, "<f>_xlfn._xlws.PY(1,0)</f>", r#"<f t="shared" si="0"/>"#);
        let (back, _) = read_xlsx(&bytes).unwrap();
        let code = |cell: &str| back.sheets[0].cell(at(cell)).and_then(|c| c.py.as_deref()).map(|py| py.code.clone());
        (code("A1"), code("A2"))
    };
    let both = Some("a = 1\nb = 2".to_string());
    assert_eq!(read("PY(\"a = 1\nb = 2\",0)"), (both.clone(), both));
    for strict in ["PY(\"a\",0.0)", "PY(\"a\",1.0)", "PY(\"a\",00)", "PY(\"a\",1E0)"] {
        assert_eq!(read(strict), (None, None), "neither cell is a Python cell: {strict}");
    }
}

/// A file past the limit on what reading is charged doesn't open, and linked copies of one plot are
/// charged once each.
#[test]
fn floating_pictures_come_out_of_the_room_for_python_cells_pictures() {
    let mut sheet = Sheet::new("Data");
    for id in 1..=3 {
        sheet.images.push(linked_picture(id, "Data!A1", "plot", &[0u8; 1024]));
    }
    let room = crate::write::python_image_room(&book(vec![sheet]));
    assert_eq!(room, crate::write::MAX_PYTHON_IMAGE_BYTES - 3 * 1024);
}

#[test]
fn a_picture_with_the_density_of_one_keeps_its_size() {
    let mut sheet = Sheet::new("Data");
    image_cell(&mut sheet, "A1", true, image_of(&png(80, 61), 1));
    // Odd pixels round down, as a run has it, and a side of one pixel still has a size.
    image_cell(&mut sheet, "A2", true, image_of(&png(1, 61), 2));
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    let size = |c: &str| {
        let i = py_at(&back.sheets[0], c).image().cloned().unwrap();
        (i.width, i.height, i.density)
    };
    assert_eq!(size("A1"), (80, 61, 1));
    assert_eq!(size("A2"), (1, 30, 2));
}

#[test]
fn the_bytes_of_a_python_picture_are_stored_once() {
    let picture = png(40, 30);
    let mut sheet = Sheet::new("Data");
    image_cell(&mut sheet, "A1", false, image_of(&picture, 1));
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();
    let media = media_parts(&bytes);
    assert_eq!(media.len(), 1, "{media:?}");
    let file = media[0].trim_start_matches("xl/media/");
    assert!(text(&bytes, "xl/drawings/_rels/drawing1.xml.rels").contains(file));
    assert!(text(&bytes, "xl/richData/_rels/richValueRel.xml.rels").contains(file));

    // Copies of the same result, a linked picture of it and a Python Object: one part again.
    let image = image_of(&picture, 1);
    let mut sheet = Sheet::new("Data");
    image_cell(&mut sheet, "A1", false, image.clone());
    image_cell(&mut sheet, "A2", false, image.clone());
    image_cell(&mut sheet, "A3", true, image);
    sheet.images.push(linked_picture(1, "Data!$A$1", "linked", &picture));
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();
    assert_eq!(media_parts(&bytes).len(), 1, "{:?}", media_parts(&bytes));
    assert_eq!(text(&bytes, "xl/richData/rdrichvalue.xml").matches("<rv ").count(), 1, "one rich value for one result");
    let (back, _) = read_xlsx(&bytes).unwrap();
    for cell in ["A1", "A2", "A3"] {
        assert_eq!(py_at(&back.sheets[0], cell).image().unwrap().picture.data, picture, "{cell}");
    }
    assert_eq!(back.sheets[0].images.len(), 1);

    // A Python Object has no rich value at all, and different pictures have parts of their own.
    let mut sheet = Sheet::new("Data");
    image_cell(&mut sheet, "A1", true, image_of(&picture, 1));
    image_cell(&mut sheet, "A2", true, image_of(&png(41, 30), 1));
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();
    assert_eq!(media_parts(&bytes).len(), 2);
    assert!(!part_names(&bytes).iter().any(|n| n.starts_with("xl/richData/")));
}

#[test]
fn a_python_picture_with_no_room_for_a_rich_value_never_fails_a_save() {
    // Past what a rich value can hold, each picture alone or all of them together, the cell holds what
    // a Python Object does, and its companion holds the picture. Plain cell pictures have nowhere else
    // to go, so their room is theirs.
    let big = |fill: u8| {
        let mut data = png(100 + u32::from(fill), 100);
        data.resize(15 * 1024 * 1024, fill);
        data
    };
    let mut sheet = Sheet::new("Data");
    for i in 0..8u8 {
        let picture = CellPicture { data: big(i), mime: "image/png".into(), alt: String::new() };
        sheet.set_picture(CellRef::new(u32::from(i), 0), Arc::new(picture));
    }
    image_cell(&mut sheet, "B1", false, image_of(&big(8), 1));
    image_cell(&mut sheet, "B2", false, image_of(&big(9), 1));
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();
    let cells = text(&bytes, "xl/worksheets/sheet1.xml");
    assert!(cells.contains(r#"<c r="B2" t="str"><f>_xlfn._xlws.PY(1,0)</f><v>Image</v></c>"#), "{}", &cells[..cells.len().min(800)]);
    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("picture")), "{:?}", report.warnings);
    let sheet = &back.sheets[0];
    assert_eq!(sheet.cell_pictures.len(), 8);
    for (cell, fill) in [("B1", 8), ("B2", 9)] {
        let py = py_at(sheet, cell);
        assert_eq!(py.image().unwrap().picture.data, big(fill), "{cell}");
        assert!(sheet.picture_at(at(cell)).is_some(), "{cell}: still an Excel Value");
    }
}

#[test]
fn bytes_a_rich_value_cannot_hold_are_kept_as_a_floating_picture() {
    // Too large for a cell picture: this can't be a run's, and the bytes still aren't lost.
    let mut sheet = Sheet::new("Data");
    let picture = CellPicture { data: png(9000, 10), mime: "image/png".into(), alt: "wide".into() };
    let image = PyImage { picture: Arc::new(picture), width: 9000, height: 10, density: 1, floor: None, fitted: None };
    image_cell(&mut sheet, "A1", false, image);
    let (back, report) = read_xlsx(&write_xlsx(&book(vec![sheet])).unwrap()).unwrap();
    let sheet = &back.sheets[0];
    assert_eq!(sheet.images.len(), 1);
    assert_eq!((sheet.images[0].data.as_slice(), sheet.images[0].alt.as_str()), (png(9000, 10).as_slice(), "wide"));
    assert_eq!(py_at(sheet, "A1").image(), None);
    assert!(report.warnings.iter().any(|w| w.contains("floating")), "{:?}", report.warnings);
}

#[test]
fn a_malformed_companion_marker_leaves_a_plain_picture() {
    for descr in [
        "gridcraft:pythonCell:",
        "gridcraft:pythonCell:B2",
        "gridcraft:pythonCell:B2:",
        "gridcraft:pythonCell:B2:1",
        "gridcraft:pythonCell::1:x",
        "gridcraft:pythonCell:B2:0:x",
        "gridcraft:pythonCell:B2:3:x",
        "gridcraft:pythonCell:B2:-1:x",
        "gridcraft:pythonCell:B2:01:x",
        "gridcraft:pythonCell:B2:99999999999999999999:x",
        "gridcraft:pythonCell:ZZZZ1:1:x",
        "gridcraft:pythonCell:B0:1:x",
        "gridcraft:pythonCell:B99999999:1:x",
        "gridcraft:pythonCell:\u{1F600}:1:x",
    ] {
        let (back, report) = read_xlsx(&package_with_picture(descr, OBJECT_CELL)).unwrap();
        let sheet = &back.sheets[0];
        assert_eq!(sheet.images.len(), 1, "{descr}");
        assert_eq!(sheet.images[0].alt, descr);
        assert_eq!(sheet.images[0].python_source, None, "{descr}");
        assert_eq!(py_at(sheet, "B2").image(), None, "{descr}");
        assert!(!report.warnings.iter().any(|w| w.contains("floating")), "{descr}: {:?}", report.warnings);
    }
}

#[test]
fn a_companion_with_no_python_cell_is_a_floating_picture() {
    let descr = "gridcraft:pythonCell:B2:2:my text";
    for cells in [
        "",
        r#"<c r="B2"><v>5</v></c>"#,
        r#"<c r="B2" t="str"><v>Image</v></c>"#,
        // A script the file doesn't hold, as after a save through openpyxl.
        r#"<c r="B2" t="str"><f>_xlfn._xlws.PY(7,1)</f><v>Image</v></c>"#,
    ] {
        let (back, report) = read_xlsx(&package_with_picture(descr, cells)).unwrap();
        let sheet = &back.sheets[0];
        assert_eq!(sheet.images.len(), 1, "{cells}");
        assert_eq!(sheet.images[0].alt, "my text", "{cells}");
        assert_eq!(sheet.images[0].anchor.cell, at("B2"), "{cells}");
        assert_eq!(sheet.images[0].data.as_slice(), png(40, 30), "{cells}");
        assert!(report.warnings.iter().any(|w| w.contains("floating")), "{cells}: {:?}", report.warnings);
    }
}

#[test]
fn a_companion_goes_by_its_anchor_which_moves_with_the_cell() {
    // Another application inserted a row above the cell: the anchor moved, the description didn't.
    let (back, report) = read_xlsx(&package_with_picture("gridcraft:pythonCell:B1:2:x", OBJECT_CELL)).unwrap();
    let sheet = &back.sheets[0];
    assert!(sheet.images.is_empty(), "{:?}", report.warnings);
    assert_eq!(py_at(sheet, "B2").image().unwrap().density, 2);
    // And a marker naming a Python cell elsewhere doesn't reach it.
    let cells = r#"<c r="A2" t="str"><f>_xlfn._xlws.PY(0,1)</f><v>Image</v></c>"#;
    let (back, _) = read_xlsx(&package_with_picture("gridcraft:pythonCell:A2:2:x", cells)).unwrap();
    let sheet = &back.sheets[0];
    assert_eq!(sheet.images.len(), 1);
    assert_eq!(py_at(sheet, "A2").image(), None);
}

#[test]
fn a_newer_result_keeps_its_value_and_the_old_companion_floats() {
    // An application that doesn't know the companion changed the code, and left it behind.
    let cells = r#"<c r="B2"><f>_xlfn._xlws.PY(0,1)</f><v>5</v></c>"#;
    let (back, _) = read_xlsx(&package_with_picture("gridcraft:pythonCell:B2:1:x", cells)).unwrap();
    let sheet = &back.sheets[0];
    assert_eq!(sheet.images.len(), 1);
    assert_eq!(sheet.value(at("B2")), Value::Number(5.0));
    assert_eq!(py_at(sheet, "B2").image(), None);
}

#[test]
fn a_second_companion_on_a_cell_is_a_floating_picture() {
    let descriptions = ["gridcraft:pythonCell:B2:1:first", "gridcraft:pythonCell:B2:2:second"];
    let package = Package { descriptions: &descriptions, cells: OBJECT_CELL, script: "1 + 1".into(), picture: png(40, 30) };
    let (back, _) = read_xlsx(&package.bytes()).unwrap();
    let sheet = &back.sheets[0];
    assert_eq!(py_at(sheet, "B2").image().unwrap().picture.alt, "first");
    assert_eq!(sheet.images.iter().map(|o| o.alt.as_str()).collect::<Vec<_>>(), ["second"]);
}

#[test]
fn a_companion_that_cannot_be_a_cell_picture_stays_a_floating_picture() {
    let descriptions = ["gridcraft:pythonCell:B2:1:x"];
    let mut oversized = png(40, 30);
    oversized.resize(16 * 1024 * 1024 + 1, 0);
    for picture in [png(0, 30), png(9000, 30), png(5000, 5000), b"not a picture".to_vec(), oversized] {
        let package = Package { descriptions: &descriptions, cells: OBJECT_CELL, script: "1 + 1".into(), picture: picture.clone() };
        let (back, _) = read_xlsx(&package.bytes()).unwrap();
        let sheet = &back.sheets[0];
        assert_eq!(sheet.images.len(), 1, "{} bytes", picture.len());
        assert_eq!(sheet.images[0].data.as_slice(), picture);
        assert_eq!(py_at(sheet, "B2").image(), None);
    }
}

#[test]
fn code_too_long_to_stay_is_no_place_for_a_picture() {
    // The engine takes such code away, and what would then hold the picture? So the picture stays out.
    let script = "x".repeat(gridcraft_model::python::MAX_FORMULA_CHARS + 1);
    let descriptions = ["gridcraft:pythonCell:B2:1:x", "gridcraft:python:9:Data!$B$2:y"];
    let package = Package { descriptions: &descriptions, cells: OBJECT_CELL, script, picture: png(40, 30) };
    let (back, _) = read_xlsx(&package.bytes()).unwrap();
    let sheet = &back.sheets[0];
    assert_eq!(py_at(sheet, "B2").image(), None);
    assert_eq!(sheet.images.len(), 2);
    assert!(sheet.images.iter().all(|o| o.python_source.is_none()));

    // The same with a rich value: the picture is what is left of the cell, and the code goes here, as
    // the engine would take it from a Python cell.
    let picture = png(80, 60);
    let long = "x".repeat(gridcraft_model::python::MAX_FORMULA_CHARS + 1);
    let bytes = with_text_changed(&excel_value_file(&picture), "xl/python.xml", "<code>plot()</code>", &format!("<code>{long}</code>"));
    let (back, report) = read_xlsx(&bytes).unwrap();
    let sheet = &back.sheets[0];
    assert_eq!(sheet.cell_pictures.get(&at("A1")).unwrap().data, picture);
    assert_eq!(sheet.cell(at("A1")).unwrap().py, None);
    assert_eq!(sheet.images.len(), 1);
    assert!(report.warnings.iter().any(|w| w.contains("static pictures")), "{:?}", report.warnings);
}

/// A workbook with an Excel Value picture on A1, as saved.
fn excel_value_file(picture: &[u8]) -> Vec<u8> {
    let mut sheet = Sheet::new("Data");
    image_cell(&mut sheet, "A1", false, image_of(picture, 2));
    write_xlsx(&book(vec![sheet])).unwrap()
}

#[test]
fn a_rich_value_is_a_python_cell_with_or_without_its_companion() {
    let picture = png(80, 60);
    let bytes = excel_value_file(&picture);
    // With no drawing the size comes from the pixels alone.
    let bare = with_text_changed(&bytes, "xl/worksheets/sheet1.xml", "<drawing r:id=\"rId1\"/>", "");
    let (back, _) = read_xlsx(&bare).unwrap();
    let sheet = &back.sheets[0];
    let image = py_at(sheet, "A1").image().cloned().unwrap();
    assert_eq!((image.width, image.height, image.density), (80, 60, 1));
    assert_eq!(image.picture.alt, "Image <plot> 40 x 30");
    assert_eq!(sheet.value(at("A1")), Value::text("Image"));
    assert!(sheet.cell_pictures.is_empty() && sheet.images.is_empty());
    // The next save gives it a companion.
    let again = write_xlsx(&back).unwrap();
    assert!(text(&again, "xl/drawings/drawing1.xml").contains("gridcraft:pythonCell:A1:1:"));
}

#[test]
fn a_companion_with_other_bytes_than_the_cells_rich_value_floats() {
    let bytes = repack(&excel_value_file(&png(80, 60)), &[("xl/media/image2.png", &png(70, 60))]);
    let bytes = with_text_changed(&bytes, "xl/drawings/_rels/drawing1.xml.rels", "media/image1.png", "media/image2.png");
    let (back, _) = read_xlsx(&bytes).unwrap();
    let sheet = &back.sheets[0];
    assert_eq!(py_at(sheet, "A1").image().unwrap().picture.data, png(80, 60));
    assert_eq!(py_at(sheet, "A1").image().unwrap().density, 1, "that companion says nothing about this picture");
    assert_eq!(sheet.images.len(), 1);
    assert_eq!(sheet.images[0].data.as_slice(), png(70, 60));
}

#[test]
fn a_rich_value_whose_code_is_gone_stays_a_plain_cell_picture() {
    // A formula naming a script the file lacks keeps only what it shows, which here is a picture, and
    // the companion, finding no Python cell, floats.
    let picture = png(80, 60);
    let python = r#"<python xmlns="http://schemas.microsoft.com/office/spreadsheetml/2023/python"/>"#;
    let bytes = repack(&excel_value_file(&picture), &[("xl/python.xml", python.as_bytes())]);
    let (back, report) = read_xlsx(&bytes).unwrap();
    let sheet = &back.sheets[0];
    assert_eq!(sheet.cell_pictures.get(&at("A1")).unwrap().data, picture);
    assert_eq!(sheet.cell(at("A1")).unwrap().py, None);
    assert_eq!(sheet.images.len(), 1);
    assert!(report.warnings.iter().any(|w| w.contains("Python cell(s)")), "{:?}", report.warnings);
}

#[test]
fn a_picture_is_the_whole_result_whatever_range_the_file_declares() {
    // A rich value: the range spills nothing, and the cells in it stay what they are.
    let bytes = excel_value_file(&png(80, 60));
    let sheet_part = "xl/worksheets/sheet1.xml";
    let was = text(&bytes, sheet_part);
    let declared = was
        .replace("<f>_xlfn._xlws.PY(0,0)</f>", r#"<f t="array" ref="A1:A3">_xlfn._xlws.PY(0,0)</f>"#)
        .replace("</row>", r#"</row><row r="2"><c r="A2"><v>7</v></c></row>"#);
    let (back, _) = read_xlsx(&repack(&bytes, &[(sheet_part, declared.as_bytes())])).unwrap();
    let sheet = &back.sheets[0];
    assert!(py_at(sheet, "A1").image().is_some());
    assert!(sheet.spill_ranges.is_empty(), "{:?}", sheet.spill_ranges);
    assert_eq!(sheet.value(at("A2")), Value::Number(7.0));

    // A Python Object whose file says it spills: a picture isn't what spilled.
    let cells = r#"<c r="B2" t="str"><f t="array" ref="B2:B3">_xlfn._xlws.PY(0,1)</f><v>Image</v></c>"#;
    let (back, _) = read_xlsx(&package_with_picture("gridcraft:pythonCell:B2:1:x", cells)).unwrap();
    let sheet = &back.sheets[0];
    assert_eq!(py_at(sheet, "B2").image(), None);
    assert_eq!(sheet.images.len(), 1);
}

#[test]
fn a_python_cell_with_no_picture_writes_no_companion() {
    let mut sheet = Sheet::new("Data");
    python_cell(&mut sheet, "A1", PyCode { object: true, output_chosen: true, ..PyCode::new("1") }, Value::text("Image"));
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();
    assert!(!part_names(&bytes).iter().any(|n| n.starts_with("xl/drawings/") || n.starts_with("xl/media/")));
    // The text alone says nothing about a picture.
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(py_at(&back.sheets[0], "A1").image(), None);
}

#[test]
fn more_python_pictures_than_a_file_can_reopen_are_left_for_the_cells_to_run_again() {
    // One result in many cells: one part, and each cell's picture is read when the file opens. Without
    // a limit this file would be refused as too large, with the person's work in it.
    let mut data = png(100, 100);
    data.resize(15 * 1024 * 1024, 0);
    let image = image_of(&data, 1);
    let cells = (crate::package::MAX_TOTAL as usize / data.len()) as u32 + 2;
    let mut sheet = Sheet::new("Data");
    for row in 0..cells {
        let cell = CellRef::new(row, 0).a1();
        image_cell(&mut sheet, &cell, row % 2 == 0, image.clone());
    }
    let bytes = write_xlsx(&book(vec![sheet])).unwrap();
    assert_eq!(media_parts(&bytes).len(), 1);
    let (back, report) = read_xlsx(&bytes).unwrap();
    let sheet = &back.sheets[0];
    let kept = crate::write::MAX_PYTHON_IMAGE_BYTES / data.len();
    assert!(kept < cells as usize);
    for row in 0..cells {
        let at = CellRef::new(row, 0);
        let py = sheet.cell(at).unwrap().py.as_deref().unwrap();
        assert_eq!(py.code, "plot()", "{row}");
        assert_eq!(py.image().is_some(), (row as usize) < kept, "{row}");
        assert_eq!(sheet.value(at), Value::text("Image"), "{row}");
    }
    assert!(sheet.images.is_empty(), "{:?}", report.warnings);
}
