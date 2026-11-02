//! Cached values of linked workbooks (`xl/externalLinks/externalLinkN.xml`).
//!
//! A formula reaching into another workbook (`[1]Prices!B2`, `'[1]P&L'!A7:A99`) names it by its
//! position in `<externalReferences>`. Excel stores the values it last read from each linked
//! workbook in the link part (`<externalBook><sheetDataSet>`) and calculates with them while the
//! source is closed. The parts themselves are carried through unchanged by `passthrough`.

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{ExternalBook, ExternalName, ExternalSheet};

use crate::IoError;
use crate::package::Rel;
use crate::read::Ctx;
use crate::xml::{self, El};

/// One book per `<externalReference>`, in order. A link that isn't to a workbook (DDE, OLE), or
/// whose part is missing or can't be read, is an opaque book, so that the later ones keep their
/// positions and formulas through it keep their saved values; a DDE link's book holds the values
/// saved for its items.
pub(crate) fn read(cx: &mut Ctx<'_>, wb_xml: &El, wb_rels: &[Rel]) -> Result<Vec<ExternalBook>, IoError> {
    let Some(refs) = wb_xml.child("externalReferences") else { return Ok(vec![]) };
    let mut books = vec![];
    for r in refs.kids("externalReference") {
        let target = r.attr("id").and_then(|id| wb_rels.iter().find(|x| x.id == id)).map(|x| x.target.clone());
        let part = match target {
            Some(t) => cx.optional_xml(&t)?,
            None => None,
        };
        let book = match part.as_ref() {
            Some(x) => match (x.child("externalBook"), x.child("ddeLink")) {
                (Some(b), _) => book(b),
                (None, Some(d)) => ExternalBook { opaque: true, items: dde_items(d), ..Default::default() },
                (None, None) => ExternalBook { opaque: true, ..Default::default() },
            },
            None => ExternalBook { opaque: true, ..Default::default() },
        };
        books.push(book);
    }
    Ok(books)
}

/// Whether a formula is saved as the legacy formula it was read as, whatever it calculates to: it
/// was read from a file without dynamic-array metadata (and isn't a Ctrl+Shift+Enter array, which
/// keeps its range), and reads a linked workbook (`[1]Data!B:B`, `[1]!Rates`). Excel reads such a
/// cell with implicit intersection; writing it as a dynamic array, because a linked range in it
/// spills here, would change what Excel shows. Formulas typed over a linked workbook are saved
/// like any other.
pub(crate) fn saved_as_read(formula: &gridcraft_model::Formula) -> bool {
    use gridcraft_formula::{Expr, SheetSel};
    let Some(expr) = formula.expr_arc().filter(|_| formula.legacy && formula.array.is_none()) else { return false };
    let mut linked = false;
    expr.walk(&mut |e| {
        linked |= match e {
            Expr::Ref(r) => matches!(&r.sheet, SheetSel::Named(s) | SheetSel::Span(s, _) if s.starts_with('[')),
            Expr::Name(n) => n.starts_with('['),
            Expr::LinkedRefError(_) => true,
            _ => false,
        }
    });
    linked
}

/// The items of a `<ddeLink>` that were saved with values (`<values rows cols>`, row-major): one
/// value, or an array.
fn dde_items(x: &El) -> Vec<(String, Value)> {
    let items = x.child("ddeItems").into_iter().flat_map(|i| i.kids("ddeItem"));
    items
        .filter_map(|item| {
            let name = item.attr("name").map(xml::decode_xstring)?;
            let values = item.child("values")?;
            let data: Vec<Value> = values.kids("value").map(dde_value).collect();
            let (rows, cols) = (values.attr_u32("rows").unwrap_or(1) as usize, values.attr_u32("cols").unwrap_or(1) as usize);
            let v = match data.as_slice() {
                [one] if rows == 1 && cols == 1 => one.clone(),
                _ => Value::from(gridcraft_core::Array::new(rows, cols, data)?),
            };
            Some((name, v))
        })
        .collect()
}

/// One saved DDE value (`<value t="n|str|b|e|nil"><val>…</val></value>`, number by default).
fn dde_value(v: &El) -> Value {
    let text = v.child("val").map(|x| x.text.as_str()).unwrap_or_default();
    match v.attr("t").unwrap_or("n") {
        "nil" => Value::Empty,
        "str" => Value::text(xml::decode_xstring(text)),
        "b" => Value::Bool(matches!(text.trim(), "1" | "true" | "TRUE")),
        "e" => Value::Error(CellError::parse(text.trim()).unwrap_or(CellError::Value)),
        _ => match text.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Value::Number(if n == 0.0 { 0.0 } else { n }),
            _ => Value::Error(CellError::Value),
        },
    }
}

fn book(x: &El) -> ExternalBook {
    let mut sheets: Vec<ExternalSheet> = x
        .child("sheetNames")
        .map(|n| {
            n.kids("sheetName")
                .map(|s| ExternalSheet { name: s.attr("val").map(xml::decode_xstring).unwrap_or_default(), ..Default::default() })
                .collect()
        })
        .unwrap_or_default();
    for data in x.child("sheetDataSet").into_iter().flat_map(|d| d.kids("sheetData")) {
        let Some(sheet) = data.attr_u32("sheetId").and_then(|i| sheets.get_mut(i as usize)) else { continue };
        sheet.refresh_error = data.flag("refreshError", false);
        for c in data.kids("row").flat_map(|r| r.kids("cell")) {
            let Some(pos) = c.attr("r").and_then(CellRef::parse).filter(|p| p.row < MAX_ROWS && p.col < MAX_COLS) else { continue };
            let v = value(c);
            if !v.is_empty() {
                sheet.cells.push((pos, v));
            }
        }
        sheet.cells.sort_by_key(|(p, _)| *p);
        sheet.cells.dedup_by_key(|(p, _)| *p);
    }
    let names = x
        .child("definedNames")
        .into_iter()
        .flat_map(|d| d.kids("definedName"))
        .filter_map(|d| {
            // A name saved without `refersTo` is still defined: the cells that use it keep the
            // values Excel saved for them.
            let refers_to = d.attr("refersTo").unwrap_or_default();
            Some(ExternalName {
                name: d.attr("name")?.to_string(),
                sheet: d.attr_u32("sheetId").map(|i| i as usize),
                refers_to: refers_to.strip_prefix('=').unwrap_or(refers_to).to_string(),
            })
        })
        .collect();
    ExternalBook { sheets, names, ..Default::default() }
}

fn value(c: &El) -> Value {
    let Some(v) = c.child("v").map(|v| v.text.as_str()) else { return Value::Empty };
    match c.attr("t").unwrap_or("n") {
        "str" | "s" | "inlineStr" => Value::text(xml::decode_xstring(v)),
        "b" => Value::Bool(matches!(v.trim(), "1" | "true" | "TRUE")),
        "e" => Value::Error(CellError::parse(v.trim()).unwrap_or(CellError::Value)),
        _ => match v.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Value::Number(if n == 0.0 { 0.0 } else { n }),
            _ => Value::Empty,
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_saved_without_a_definition_are_kept() {
        // Excel writes `<definedName name="X"/>` for a linked name it couldn't read: still a name
        // of the link, which formulas use with the values Excel saved for them.
        let x = br##"<externalBook><sheetNames><sheetName val="Personal"/></sheetNames><definedNames><definedName name="HelloWorld"/><definedName name="Broken" refersTo="#REF!"/><definedName name="Rate" refersTo="=Personal!$A$1"/></definedNames></externalBook>"##;
        let book = super::book(&crate::xml::parse(x).unwrap());
        let names: Vec<_> = book.names.iter().map(|n| (n.name.as_str(), n.refers_to.as_str())).collect();
        assert_eq!(names, [("HelloWorld", ""), ("Broken", "#REF!"), ("Rate", "Personal!$A$1")]);
    }

    #[test]
    fn dde_ole_and_missing_links_are_opaque_and_keep_positions() {
        use gridcraft_core::{Array, Value};
        const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;
        let dde = format!(
            r#"<externalLink {NS}><ddeLink ddeService="REUTER" ddeTopic="IDN"><ddeItems><ddeItem name="ENE" advise="1"><values><value><val>85.33</val></value></values></ddeItem><ddeItem name="Name"><values><value t="str"><val>Enron</val></value></values></ddeItem><ddeItem name="Grid"><values rows="1" cols="2"><value><val>1</val></value><value t="e"><val>#N/A</val></value></values></ddeItem><ddeItem name="Unsaved"/></ddeItems></ddeLink></externalLink>"#
        );
        let ole = format!(r#"<externalLink {NS}><oleLink progId="Word.Document.8"/></externalLink>"#);
        let book = format!(r#"<externalLink {NS}><externalBook><sheetNames><sheetName val="Data"/></sheetNames></externalBook></externalLink>"#);
        let rel = |id: u32, n: u32| {
            format!(
                r#"<Relationship Id="rId{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/externalLink" Target="externalLinks/externalLink{n}.xml"/>"#
            )
        };
        let rels = [rel(11, 1), rel(12, 2), rel(13, 3), rel(14, 4)].concat();
        let refs = r#"<externalReferences><externalReference r:id="rId11"/><externalReference r:id="rId12"/><externalReference r:id="rId13"/><externalReference r:id="rId14"/></externalReferences>"#;
        // Link 3's part is missing from the package.
        let parts = [
            ("xl/externalLinks/externalLink1.xml", dde.as_str()),
            ("xl/externalLinks/externalLink2.xml", ole.as_str()),
            ("xl/externalLinks/externalLink4.xml", book.as_str()),
        ];
        let (wb, _) = crate::read_xlsx(&crate::tests::minimal("<sheetData/>", &parts, &rels, refs)).unwrap();
        let books = &wb.external_books;
        assert_eq!(books.iter().map(|b| b.opaque).collect::<Vec<_>>(), [true, true, true, false]);
        let grid = Value::from(Array::new(1, 2, vec![Value::Number(1.0), Value::Error(gridcraft_core::CellError::NA)]).unwrap());
        assert_eq!(books[0].items, [("ENE".to_string(), Value::Number(85.33)), ("Name".into(), Value::text("Enron")), ("Grid".into(), grid)]);
        assert_eq!(books[0].item("ene"), Some(&Value::Number(85.33)));
        assert!(books[1].items.is_empty() && books[2].items.is_empty());
        assert_eq!(books[3].sheet_index("Data"), Some(0));
    }
}
