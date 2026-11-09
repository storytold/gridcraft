//! Self-contained extensions (`<extLst><ext>`) of the workbook and its sheets, kept on save.
//!
//! An extension is kept only when its URI is on an allowlist of entries that are one element
//! holding attributes and nothing else: no children, no text, no relationship ids, no references
//! to cells, names or parts GridCraft renumbers. They are rebuilt from the parsed element with the
//! schema's attributes only, in source order. Extensions that point at other content (slicer
//! caches and lists, timelines, the data model, PivotTable caches, Google Sheets custom data with
//! its `r:id`) are left out: GridCraft rewrites or drops what they point at. Extensions GridCraft
//! models itself (x14 conditional formats, data validations, sparklines) are read into the model
//! and written by the sheet writer, never kept here. Every other extension is dropped and counted
//! in a read warning.
//!
//! Kept extensions go into the owner's [`Preserved`] as elements of slot `"ext"`: the workbook's
//! are written by `write.rs`, a sheet's after the extensions the sheet writer emits, skipping any
//! URI it already wrote.

use std::fmt::Write as _;
use std::sync::Arc;

use gridcraft_model::{Preserved, PreservedXml, Sheet};

use crate::read::Ctx;
use crate::xml::{El, esc_attr};

/// One allowlisted extension: its URI, the single element it holds and that element's attributes.
struct Allowed {
    uri: &'static str,
    prefix: &'static str,
    ns: &'static str,
    el: &'static str,
    attrs: &'static [&'static str],
}

const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const NS_X15: &str = "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main";
const NS_XLWCV: &str = "http://schemas.microsoft.com/office/spreadsheetml/2024/workbookCompatibilityVersion";
/// Excel for Mac 2011 extensions; older files use the namespace itself as the URI.
const NS_MX: &str = "http://schemas.microsoft.com/office/mac/excel/2008/main";

/// The workbook extension listing the calculation features the saved values were computed with.
/// Excel needs it, with `<fileVersion>`, to trust the saved values (#799).
pub(crate) const CALC_FEATURES_URI: &str = "{B58B0392-4F1F-4190-BB64-5DF3571DCE5F}";
const NS_CALC_FEATURES: &str = "http://schemas.microsoft.com/office/spreadsheetml/2018/calcfeatures";

/// Kept workbook extensions.
const WORKBOOK: &[Allowed] = &[
    // x15:workbookPr (CT_WorkbookPr in the 2010/11 schema).
    Allowed { uri: "{140A7094-0E35-4892-8432-C4D2E57EDEB5}", prefix: "x15", ns: NS_X15, el: "workbookPr", attrs: &["chartTrackingRefBase"] },
    // x14:workbookPr (CT_WorkbookPr in the 2009/9 schema).
    Allowed {
        uri: "{79F54976-1DA5-4618-B147-4CDE4B953A38}",
        prefix: "x14",
        ns: NS_X14,
        el: "workbookPr",
        attrs: &["defaultImageDpi", "discardImageEditData", "accuracyVersion"],
    },
    // The workbook compatibility version Excel 365 stamps.
    Allowed { uri: "{D14903EA-33C4-47F7-8F05-3474C54BE107}", prefix: "xlwcv", ns: NS_XLWCV, el: "version", attrs: &["setVersion"] },
    // Excel for Mac architecture flags.
    Allowed { uri: "{7523E5D3-25F3-A5E0-1632-64F254C22452}", prefix: "mx", ns: NS_MX, el: "ArchID", attrs: &["Flags"] },
    Allowed { uri: NS_MX, prefix: "mx", ns: NS_MX, el: "ArchID", attrs: &["Flags"] },
];

/// Kept sheet extensions.
const SHEET: &[Allowed] = &[
    // Excel for Mac page layout view settings.
    Allowed { uri: "{64002731-A6B0-56B0-2670-7721B7C09600}", prefix: "mx", ns: NS_MX, el: "PLV", attrs: &["Mode", "OnePage", "WScale"] },
    Allowed { uri: NS_MX, prefix: "mx", ns: NS_MX, el: "PLV", attrs: &["Mode", "OnePage", "WScale"] },
];

/// Sheet extension elements the sheet reader models (x14 conditional formats, data validations,
/// sparklines).
const SHEET_MODELLED: &[&str] = &["conditionalFormattings", "dataValidations", "sparklineGroups"];

/// Longest attribute value kept (the kept attributes are flags and small numbers).
const MAX_VALUE: usize = 32;
/// Most dropped URIs named in one warning.
const MAX_NAMED: usize = 5;

/// The `uri` attribute of `ext`, trimmed.
fn uri(ext: &El) -> &str {
    ext.attr("uri").unwrap_or("").trim()
}

/// `ext` rebuilt from `table` when it is allowlisted and self-contained.
fn rebuild(ext: &El, table: &[Allowed]) -> Option<String> {
    let u = uri(ext);
    let a = table.iter().find(|a| a.uri.eq_ignore_ascii_case(u))?;
    // Exactly one element, the expected one, holding attributes only.
    let [el] = ext.children.as_slice() else { return None };
    if el.name != a.el || !el.children.is_empty() || !el.text.trim().is_empty() || !ext.text.trim().is_empty() {
        return None;
    }
    let mut x = format!("<ext uri=\"{}\" xmlns:{p}=\"{}\"><{p}:{}", a.uri, a.ns, a.el, p = a.prefix);
    for name in a.attrs {
        if let Some(v) = el.attr(name).map(str::trim)
            && !v.is_empty()
            && v.len() <= MAX_VALUE
            && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            let _ = write!(x, " {name}=\"{}\"", esc_attr(v));
        }
    }
    x.push_str("/></ext>");
    Some(x)
}

/// The calcFeatures extension rebuilt: the feature names only.
fn calc_features(ext: &El) -> Option<String> {
    let c = ext.child("calcFeatures")?;
    let mut x = format!("<ext uri=\"{CALC_FEATURES_URI}\" xmlns:xcalcf=\"{NS_CALC_FEATURES}\"><xcalcf:calcFeatures>");
    for name in c.kids("feature").filter_map(|f| f.attr("name")) {
        let _ = write!(x, "<xcalcf:feature name=\"{}\"/>", esc_attr(name));
    }
    x.push_str("</xcalcf:calcFeatures></ext>");
    Some(x)
}

/// Adds `uri` to the dropped list once.
fn dropped(list: &mut Vec<String>, uri: &str) {
    let u = if uri.is_empty() { "(no URI)".to_string() } else { uri.to_string() };
    if !list.iter().any(|d| d.eq_ignore_ascii_case(&u)) {
        list.push(u);
    }
}

/// The read warning naming the dropped extension URIs of `owner` ("the workbook", "sheet \"A\"").
fn warn_dropped(cx: &mut Ctx<'_>, owner: &str, list: &[String]) {
    if list.is_empty() {
        return;
    }
    let mut named = list.iter().take(MAX_NAMED).cloned().collect::<Vec<_>>().join(", ");
    if list.len() > MAX_NAMED {
        let _ = write!(named, " and {} more", list.len() - MAX_NAMED);
    }
    let n = list.len();
    cx.warn(format!(
        "{owner}: {n} Excel extension{} GridCraft does not model {} dropped ({named})",
        if n == 1 { "" } else { "s" },
        if n == 1 { "was" } else { "were" }
    ));
}

/// Keeps the workbook's calcFeatures and allowlisted extensions from `ext_lst` (the workbook's
/// `<extLst>`), in source order, as elements of slot `"ext"` of `p`; warns about the others.
pub(crate) fn keep_workbook(cx: &mut Ctx<'_>, ext_lst: Option<&El>, p: &mut Preserved) {
    let Some(list) = ext_lst else { return };
    let mut kept: Vec<String> = vec![];
    let mut lost = vec![];
    for ext in list.kids("ext") {
        let u = uri(ext);
        if kept.iter().any(|k| k.eq_ignore_ascii_case(u)) {
            continue;
        }
        let x = if u.eq_ignore_ascii_case(CALC_FEATURES_URI) { calc_features(ext) } else { rebuild(ext, WORKBOOK) };
        match x {
            Some(x) => {
                kept.push(u.to_string());
                p.elements.push(PreservedXml { slot: "ext".into(), xml: x });
            }
            None => dropped(&mut lost, u),
        }
    }
    warn_dropped(cx, "the workbook", &lost);
}

/// While a sheet is read: keeps the allowlisted extension `ext` of its `<extLst>` until
/// [`keep_sheet`] attaches it, or counts it as dropped. Modelled extensions are left to the
/// sheet reader.
pub(crate) fn read_sheet_ext(cx: &mut Ctx<'_>, ext: &El) {
    if ext.children.iter().any(|c| SHEET_MODELLED.contains(&c.name.as_str())) {
        // The sheet reader keeps its modelled children; any other child of this extension is lost.
        if ext.children.iter().any(|c| !SHEET_MODELLED.contains(&c.name.as_str())) {
            dropped(&mut cx.sheet_exts_dropped, uri(ext));
        }
        return;
    }
    let u = uri(ext);
    if cx.sheet_exts.iter().any(|(k, _)| k.eq_ignore_ascii_case(u)) {
        return;
    }
    match rebuild(ext, SHEET) {
        Some(x) => cx.sheet_exts.push((u.to_string(), x)),
        None => dropped(&mut cx.sheet_exts_dropped, u),
    }
}

/// After a sheet is read: adds the extensions [`read_sheet_ext`] kept to `preserved` (the sheet's
/// other kept content), when unmodelled content is kept at all (`keep`), and warns about the
/// dropped ones.
pub(crate) fn keep_sheet(cx: &mut Ctx<'_>, name: &str, preserved: Option<Arc<Preserved>>, keep: bool) -> Option<Arc<Preserved>> {
    let exts = std::mem::take(&mut cx.sheet_exts);
    let mut lost = std::mem::take(&mut cx.sheet_exts_dropped);
    if !keep {
        for (u, _) in &exts {
            dropped(&mut lost, u);
        }
    }
    warn_dropped(cx, &format!("sheet \"{name}\""), &lost);
    if !keep || exts.is_empty() {
        return preserved;
    }
    let mut p = preserved.map(Arc::unwrap_or_clone).unwrap_or_default();
    p.elements.extend(exts.into_iter().map(|(_, xml)| PreservedXml { slot: "ext".into(), xml }));
    Some(Arc::new(p))
}

/// The kept extensions of `sheet`, minus those whose URI is in `written` (the `<ext>` elements the
/// sheet writer already emitted).
pub(crate) fn sheet_exts(sheet: &Sheet, written: &str) -> String {
    let Some(p) = sheet.preserved.as_deref() else { return String::new() };
    let lower = written.to_ascii_lowercase();
    let mut s = String::new();
    for e in p.elements.iter().filter(|e| e.slot == "ext") {
        let uri = e.xml.split('"').nth(1).unwrap_or("");
        if uri.is_empty() || !lower.contains(&format!("uri=\"{}\"", uri.to_ascii_lowercase())) {
            s.push_str(&e.xml);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    fn ext(x: &str) -> El {
        parse(x.as_bytes()).unwrap()
    }

    #[test]
    fn rebuilds_allowlisted_entries_only() {
        let x15 =
            ext(r#"<ext uri="{140a7094-0e35-4892-8432-c4d2e57edeb5}" xmlns:x15="x"><x15:workbookPr chartTrackingRefBase="1" other="2"/></ext>"#);
        assert_eq!(
            rebuild(&x15, WORKBOOK).as_deref(),
            Some(
                format!(
                    r#"<ext uri="{{140A7094-0E35-4892-8432-C4D2E57EDEB5}}" xmlns:x15="{NS_X15}"><x15:workbookPr chartTrackingRefBase="1"/></ext>"#
                )
                .as_str()
            )
        );
        // A known URI with unexpected content: children, text, another element, or nothing.
        for bad in [
            r#"<ext uri="{140A7094-0E35-4892-8432-C4D2E57EDEB5}"><x15:workbookPr><x:y/></x15:workbookPr></ext>"#,
            r#"<ext uri="{140A7094-0E35-4892-8432-C4D2E57EDEB5}"><x15:workbookPr>text</x15:workbookPr></ext>"#,
            r#"<ext uri="{140A7094-0E35-4892-8432-C4D2E57EDEB5}"><x15:other/></ext>"#,
            r#"<ext uri="{140A7094-0E35-4892-8432-C4D2E57EDEB5}"><x15:workbookPr/><x15:workbookPr/></ext>"#,
            r#"<ext uri="{140A7094-0E35-4892-8432-C4D2E57EDEB5}"/>"#,
        ] {
            assert_eq!(rebuild(&ext(bad), WORKBOOK), None, "{bad}");
        }
        // Not allowlisted: the data model and slicer caches point at other parts.
        for uri in ["{FCE2AD5D-F65C-4FA6-A056-5C36A1767C68}", "{BBE1A952-AA13-448e-AADC-164F8A28A991}", "GOOGLESHEETSCUSTOMDATAVERSION2"] {
            assert_eq!(rebuild(&ext(&format!(r#"<ext uri="{uri}"><x:workbookPr/></ext>"#)), WORKBOOK), None);
        }
        // Sheet entries are not workbook entries.
        let plv = ext(r#"<ext uri="{64002731-A6B0-56B0-2670-7721B7C09600}"><mx:PLV Mode="1" OnePage="0" WScale="0"/></ext>"#);
        assert_eq!(rebuild(&plv, WORKBOOK), None);
        assert!(rebuild(&plv, SHEET).unwrap().contains(r#"<mx:PLV Mode="1" OnePage="0" WScale="0"/>"#));
    }

    #[test]
    fn values_that_are_not_flags_or_numbers_are_left_out() {
        let e =
            ext(r#"<ext uri="{79F54976-1DA5-4618-B147-4CDE4B953A38}"><x14:workbookPr defaultImageDpi="&quot;&gt;&lt;x" accuracyVersion="1"/></ext>"#);
        assert_eq!(
            rebuild(&e, WORKBOOK).as_deref(),
            Some(
                format!(r#"<ext uri="{{79F54976-1DA5-4618-B147-4CDE4B953A38}}" xmlns:x14="{NS_X14}"><x14:workbookPr accuracyVersion="1"/></ext>"#)
                    .as_str()
            )
        );
    }

    #[test]
    fn written_uris_are_not_repeated() {
        let mut sheet = Sheet::new("A".to_string());
        let plv = rebuild(&ext(r#"<ext uri="{64002731-A6B0-56B0-2670-7721B7C09600}"><mx:PLV Mode="0"/></ext>"#), SHEET).unwrap();
        sheet.preserved = Some(Arc::new(Preserved { elements: vec![PreservedXml { slot: "ext".into(), xml: plv.clone() }], ..Default::default() }));
        assert_eq!(sheet_exts(&sheet, ""), plv);
        assert_eq!(sheet_exts(&sheet, r#"<ext uri="{64002731-a6b0-56b0-2670-7721b7c09600}">"#), "");
    }
}
