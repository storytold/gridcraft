//! The Python part ([MS-XLSX] 2.1.23, 2.3.10): where a workbook keeps the code of its Python cells.
//!
//! A Python cell's formula is `_xlfn._xlws.PY(index, return_type, …)` ([MS-XLSX] 2.3.10.2): the
//! first argument is the zero-based index of a `pythonScript` element in this part, whose `code`
//! is the cell's Python code, and the second is 0 (an Excel value) or 1 (a Python object). Further
//! arguments are references the code names as `%Px%`, `x` being the argument's zero-based index
//! ([MS-XLSX] 2.3.10.4). The part's root is `python` ([MS-XLSX] 2.4.102, CT_Python 2.6.249), with
//! `pythonScripts` (2.6.253) holding the `pythonScript` elements (2.6.252).
//!
//! GridCraft writes one script per Python cell, in the order the cells are written, with the code
//! as it is (its `xl()` references stay text in the code, so no further arguments). Reading
//! resolves the index and puts each `%Px%` back as the text of argument `x`. A formula that names
//! no script the part has keeps only its cached value. Hostile input is bounded: the part's size,
//! the number of scripts, each script's length, and the code all cells take from the part
//! together.
//!
//! The part's `environmentDefinition` (2.6.250) is the workbook's Python environment: a GUID and,
//! in `initialization` (2.6.251), the code that runs before the cells and whether a person changed
//! it (`userModified`). What the part holds around these and GridCraft doesn't read (extensions,
//! other attributes and elements) is kept as raw XML and written back. A workbook that holds no
//! Python cells keeps its environment too: `pythonScripts` is optional, and is left out when there
//! are no scripts, since the format requires one `pythonScript` in it. The code is cut when read
//! ([`gridcraft_model::python::MAX_INIT_CHARS_KEPT`]), as a script is.
//!
//! An element is one of the part's only in the part's namespace, whatever its prefix, and an attribute
//! only without a prefix. So a `v:code` of another namespace beside the real `code` is kept with the
//! rest, and neither is taken for the other. A save writes one script per cell, so a script has no
//! place for anything else: what `pythonScripts` and `pythonScript` hold besides scripts and code goes,
//! with a warning.

use std::fmt::Write as _;

use gridcraft_formula::Expr;
use gridcraft_model::python::{FILE_PREFIX, MAX_FORMULA_CHARS, MAX_INIT_CHARS, MAX_INIT_CHARS_KEPT, environment_id, is_guid};
use gridcraft_model::{PyCode, PyEnvironment, PyEnvironmentXml};
use quick_xml::NsReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};

use crate::IoError;
use crate::read::Ctx;
use crate::xml::{self, El};

/// Namespace of the part.
pub const NS: &str = "http://schemas.microsoft.com/office/spreadsheetml/2023/python";
/// Relationship type from the workbook part.
pub const REL: &str = "http://schemas.microsoft.com/office/2023/09/relationships/Python";
/// Last segment of [`REL`], as [`crate::package::Rel::kind`] holds it.
const REL_KIND: &str = "Python";
/// Content type of the part.
pub const CONTENT_TYPE: &str = "application/vnd.ms-excel.python+xml";
/// Part name we write (the format names none; the relationship finds it).
pub const PART: &str = "xl/python.xml";

/// Largest Python part read.
const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;
/// Most scripts read from the part.
const MAX_SCRIPTS: usize = 1_000_000;
/// Most characters kept of one script: one more than a cell's code may have, so a longer script
/// still fails [`PyCode::fits`] and its cell keeps only its value, with the usual warning.
const MAX_SCRIPT_CHARS: usize = MAX_FORMULA_CHARS + 1;
/// Most bytes of code all the cells of a workbook take from the part together. Several formulas
/// may name one script, so without it a small part could fill memory.
pub const MAX_CODE_BYTES: usize = 64 * 1024 * 1024;
/// Most bytes of raw XML kept from around the environment. Extensions are small; more than this is
/// dropped with a warning.
const MAX_KEPT_XML_BYTES: usize = 4 * 1024 * 1024;

/// The scripts read from a workbook's Python part, and what the cells took from them.
#[derive(Default)]
pub struct ScriptsIn {
    scripts: Vec<String>,
    /// Bytes of code the cells may still take ([`MAX_CODE_BYTES`] in all).
    budget: usize,
    /// Python cells that kept only their value: no such script, or over the budget.
    missing: usize,
    /// The workbook's Python environment, when the part says anything about one.
    pub environment: Option<PyEnvironment>,
}

impl ScriptsIn {
    /// The warning for the Python cells that kept only their value, if any.
    pub fn warning(&self) -> Option<String> {
        (self.missing > 0).then(|| {
            format!(
                "{} Python cell(s) name code the file's Python part doesn't hold (or holds too much of); they kept only their values",
                self.missing
            )
        })
    }
}

/// Reads the workbook's Python part, if it has one. A damaged part is a warning: its cells keep
/// their values.
pub fn read(cx: &mut Ctx<'_>, rels: &[crate::package::Rel]) -> Result<ScriptsIn, IoError> {
    let mut out = ScriptsIn { budget: MAX_CODE_BYTES, ..ScriptsIn::default() };
    let Some(rel) = rels.iter().find(|r| !r.external && r.kind.eq_ignore_ascii_case(REL_KIND)) else { return Ok(out) };
    let bytes = match cx.pkg.read_optional_limited(&rel.target, MAX_PART_BYTES) {
        Ok(Some(b)) => b,
        Ok(None) => {
            cx.warn(format!("missing part {}", rel.target));
            return Ok(out);
        }
        Err(IoError::TooLarge(m)) => {
            cx.warn(format!("the Python part is too large ({m}); Python cells kept only their values"));
            return Ok(out);
        }
        // A damaged entry (a bad checksum or compressed stream) is a damaged part: its cells keep
        // their values, as when the part is missing.
        Err(e) => {
            cx.warn(format!("the Python part is damaged ({e}); Python cells kept only their values"));
            return Ok(out);
        }
    };
    let mut over = 0usize;
    let parsed = xml::parse_streaming_in(&bytes, NS, &["pythonScript"], &mut |el: El| {
        if out.scripts.len() < MAX_SCRIPTS {
            out.scripts.push(script_code(&el));
        } else {
            over += 1;
        }
        Ok(())
    });
    match parsed {
        Ok(root) if root.name != "python" => {
            cx.warn(format!("the Python part has the root <{}>; Python cells kept only their values", root.name));
            out.scripts.clear();
        }
        Ok(root) => out.environment = read_environment(cx, &root, &bytes),
        Err(e) => cx.warn(format!("the Python part is damaged ({e}); kept {} scripts", out.scripts.len())),
    }
    if over > 0 {
        cx.warn(format!("the Python part holds more than {MAX_SCRIPTS} scripts; {over} were not read"));
    }
    Ok(out)
}

/// Encoded characters that are decoded to get `max_chars` characters of code, whatever its escapes.
/// One character takes at most 14 encoded characters (a UTF-16 pair of `_xHHHH_` escapes) and a
/// `\r\n` line break becomes one character, so `2 × (max_chars + 1)` decoded characters cover the
/// code kept, and two more escapes' worth keep the last of them, and the line break after it, from
/// being cut.
const fn encoded_chars(max_chars: usize) -> usize {
    14 * (2 * (max_chars + 1) + 2)
}

/// A `code` element's text (ST_Xstring), with `\n` line breaks, cut after `max_chars` characters.
/// The limit is measured on the decoded code, so escapes in the file (`_x000D_`, `_x0009_`) don't
/// cut code that fits; decoding is linear and reads at most [`encoded_chars`] characters.
fn decoded_code(el: &El, max_chars: usize) -> String {
    let text = el.child("code").map(|c| c.text.as_str()).unwrap_or_default();
    let encoded = match text.char_indices().nth(encoded_chars(max_chars)) {
        Some((at, _)) => text.get(..at).unwrap_or_default(),
        None => text,
    };
    let code = gridcraft_model::python::normalize_code(&xml::decode_xstring(encoded));
    match code.char_indices().nth(max_chars) {
        Some((at, _)) => code.get(..at).unwrap_or_default().to_string(),
        None => code,
    }
}

/// A `pythonScript` element's code, cut after [`MAX_SCRIPT_CHARS`] characters.
fn script_code(el: &El) -> String {
    decoded_code(el, MAX_SCRIPT_CHARS)
}

/// The workbook's Python environment from the part's root: `None` when the part says nothing about
/// one, neither an `environmentDefinition` nor anything else to keep.
fn read_environment(cx: &mut Ctx<'_>, root: &El, bytes: &[u8]) -> Option<PyEnvironment> {
    let definition = root.child("environmentDefinition");
    let initialization = definition.and_then(|d| d.child("initialization"));
    let code = initialization.map(|i| decoded_code(i, MAX_INIT_CHARS_KEPT)).unwrap_or_default();
    if code.chars().nth(MAX_INIT_CHARS).is_some() {
        cx.warn(format!(
            "the Python initialization code is longer than {MAX_INIT_CHARS} characters: its start was kept, it doesn't run until it is shortened, and a save writes only what was kept"
        ));
    }
    let scanned = kept_xml(bytes);
    if scanned.is_none() {
        cx.warn("the Python part holds XML that isn't well formed around its environment; a save leaves what GridCraft doesn't read out");
    }
    let (kept, unkept) = scanned.map_or((None, 0), |(kept, unkept)| (Some(kept), unkept));
    if unkept > 0 {
        cx.warn(format!("the Python part holds {unkept} attribute(s) or element(s) on its code or among its scripts that a save leaves out"));
    }
    let size = kept.as_ref().map_or(0, |k| {
        [&k.root_attrs, &k.root_elements, &k.environment_attrs, &k.environment_elements, &k.initialization_attrs, &k.initialization_elements]
            .iter()
            .map(|s| s.len())
            .sum()
    });
    let kept = kept.filter(|k| *k != PyEnvironmentXml::default()).filter(|_| {
        let fits = size <= MAX_KEPT_XML_BYTES;
        if !fits {
            cx.warn(format!(
                "the Python part holds more than {MAX_KEPT_XML_BYTES} bytes GridCraft doesn't read around its environment; a save leaves them out"
            ));
        }
        fits
    });
    if definition.is_none() && kept.is_none() {
        return None;
    }
    Some(PyEnvironment {
        code,
        user_modified: initialization.is_some_and(|i| i.flag("userModified", false)),
        id: definition.and_then(|d| d.attr("id")).filter(|id| is_guid(id)).map(str::to_string),
        kept: kept.map(std::sync::Arc::new),
    })
}

/// What the part holds around what GridCraft reads, as it came, and how many attributes and elements
/// it holds where a save has no place for them. `None` for a part the scan can't follow, whose unread
/// content then goes with a save.
fn kept_xml(bytes: &[u8]) -> Option<(PyEnvironmentXml, usize)> {
    #[derive(Clone, Copy, PartialEq)]
    enum In {
        Part,
        Root,
        Environment,
        Initialization,
        Scripts,
        Script,
    }
    let data = xml::to_utf8(bytes);
    let text = std::str::from_utf8(&data).ok()?;
    let mut reader = NsReader::from_str(text);
    let mut kept = PyEnvironmentXml::default();
    let mut unkept = 0usize;
    // The default namespace each kept element takes from where it was.
    let (mut in_root, mut in_environment, mut in_initialization): (Option<String>, Option<String>, Option<String>) = (None, None, None);
    let mut place = In::Part;
    let (mut seen_environment, mut seen_initialization) = (false, false);
    loop {
        let start = usize::try_from(reader.buffer_position()).ok()?;
        let (namespace, event) = reader.read_resolved_event().ok()?;
        let in_part = matches!(namespace, ResolveResult::Bound(Namespace(n)) if n == NS.as_bytes());
        let (e, open) = match event {
            Event::Eof => break,
            Event::End(_) => {
                place = match place {
                    In::Part | In::Root => break,
                    In::Environment | In::Scripts => In::Root,
                    In::Initialization => In::Environment,
                    In::Script => In::Scripts,
                };
                continue;
            }
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            _ => continue,
        };
        // The part's own names are only names in the part's namespace: another's `code` isn't its code.
        let local = e.local_name();
        let name: &[u8] = if in_part { local.as_ref() } else { b"" };
        match (place, name) {
            (In::Part, _) => {
                kept.root_attrs = attrs_besides(&e, &[b"xmlns"]);
                in_root = declared_namespace(&e);
                if !open {
                    break;
                }
                place = In::Root;
                continue;
            }
            (In::Root, b"environmentDefinition") if !seen_environment => {
                seen_environment = true;
                kept.environment_attrs = attrs_besides(&e, &[b"id", b"xmlns"]);
                in_environment = declared_namespace(&e).or_else(|| in_root.clone());
                // A readable GUID says there is a definition; without one, only this does.
                kept.definition = !e
                    .attributes()
                    .flatten()
                    .any(|a| a.key.as_ref() == b"id" && std::str::from_utf8(&a.value).is_ok_and(gridcraft_model::python::is_guid));
                if open {
                    place = In::Environment;
                }
                continue;
            }
            (In::Environment, b"initialization") if !seen_initialization => {
                seen_initialization = true;
                kept.initialization_attrs = attrs_besides(&e, &[b"userModified", b"xmlns"]);
                in_initialization = declared_namespace(&e).or_else(|| in_environment.clone());
                if open {
                    place = In::Initialization;
                }
                continue;
            }
            (In::Root, b"pythonScripts") | (In::Scripts, b"pythonScript") => {
                unkept += usize::from(has_attributes(&e));
                if open {
                    place = if place == In::Root { In::Scripts } else { In::Script };
                }
                continue;
            }
            _ => {}
        }
        if open {
            reader.read_to_end(e.name()).ok()?;
        }
        if matches!((place, name), (In::Initialization | In::Script, b"code")) {
            unkept += usize::from(has_attributes(&e));
            continue;
        }
        // A second one, which the format doesn't allow.
        if matches!((place, name), (In::Root, b"environmentDefinition") | (In::Environment, b"initialization")) {
            continue;
        }
        if matches!(place, In::Scripts | In::Script) {
            unkept += 1;
            continue;
        }
        let fragment = text.get(start..usize::try_from(reader.buffer_position()).ok()?)?;
        let in_scope = match place {
            In::Environment => &in_environment,
            In::Initialization => &in_initialization,
            In::Root | In::Part | In::Scripts | In::Script => &in_root,
        };
        let fragment = with_namespace(fragment, &e, in_scope.as_deref());
        match place {
            In::Root => kept.root_elements.push_str(&fragment),
            In::Environment => kept.environment_elements.push_str(&fragment),
            In::Initialization => kept.initialization_elements.push_str(&fragment),
            In::Part | In::Scripts | In::Script => {}
        }
    }
    kept.no_initialization = seen_environment && !seen_initialization;
    Some((kept, unkept))
}

/// Whether an element has attributes that say something a save would lose. Names starting with `xml`
/// are the XML specification's own: namespace declarations, and `xml:space`, since code is read with
/// its spaces either way.
fn has_attributes(e: &BytesStart<'_>) -> bool {
    e.attributes().flatten().any(|a| !a.key.as_ref().starts_with(b"xml"))
}

/// The default namespace an element declares itself.
fn declared_namespace(e: &BytesStart<'_>) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == b"xmlns").map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

/// A kept element as written. Everything saved around it has the part's namespace as its default, so
/// an element whose default was anything else, or nothing, says so itself: for its own name when it
/// has no prefix, and for what it holds when it has one.
fn with_namespace<'a>(fragment: &'a str, e: &BytesStart<'_>, in_scope: Option<&str>) -> std::borrow::Cow<'a, str> {
    let name = e.name();
    let declares = declared_namespace(e).is_some();
    match std::str::from_utf8(name.as_ref()) {
        Ok(name) if !declares && in_scope != Some(NS) => match fragment.strip_prefix('<').and_then(|f| f.strip_prefix(name)) {
            Some(rest) => format!("<{name} xmlns=\"{}\"{rest}", in_scope.unwrap_or("").replace('"', "&quot;")).into(),
            None => fragment.into(),
        },
        _ => fragment.into(),
    }
}

/// An element's attributes as written, each with a space before it, leaving out `skip`.
fn attrs_besides(e: &BytesStart<'_>, skip: &[&[u8]]) -> String {
    let mut out = String::new();
    for a in e.attributes().flatten() {
        if skip.contains(&a.key.as_ref()) {
            continue;
        }
        if let (Ok(key), Ok(value)) = (std::str::from_utf8(a.key.as_ref()), std::str::from_utf8(&a.value)) {
            // The value is as the file escapes it, in whichever quotes it was in.
            let _ = write!(out, " {key}=\"{}\"", value.replace('"', "&quot;"));
        }
    }
    out
}

/// `PY(index, 0|1, params…)`, as a file's formula holds it: the index, whether the return type is
/// a Python object, and the parameters. `None` for anything else (the code in a string literal
/// is [`PyCode::from_formula`]'s form).
fn py_call(expr: &Expr) -> Option<(usize, bool, &[Expr])> {
    let Expr::Call(name, args) = expr else { return None };
    let base = name.trim_start_matches("_XLFN.").trim_start_matches("_XLWS.");
    if !base.eq_ignore_ascii_case("PY") {
        return None;
    }
    let index = match args.first()? {
        Expr::Number(n) if *n >= 0.0 && n.fract() == 0.0 && *n < MAX_SCRIPTS as f64 => *n as usize,
        _ => return None,
    };
    let object = match args.get(1)? {
        Expr::Number(n) if *n == 0.0 => false,
        Expr::Number(n) if *n == 1.0 => true,
        _ => return None,
    };
    Some((index, object, args.get(2..).unwrap_or_default()))
}

/// What a file formula is to Python.
#[derive(Debug, PartialEq)]
pub enum PyFormula {
    /// Not a `PY(…)` call: an ordinary formula.
    Not,
    /// A Python cell with this code.
    Code(PyCode),
    /// A `PY(index, …)` call naming a script the part doesn't hold (or over the budget): the
    /// cell keeps only its cached value, since no calculation can evaluate the formula.
    Missing,
}

/// What a file formula makes: a Python cell from the Python part when it names a script, or from
/// string literals ([`PyCode::from_formula`]). `formula` is the parsed formula, `raw` the
/// formula's own text, which keeps line breaks in literal code.
pub fn cell_code(scripts: &mut ScriptsIn, formula: &gridcraft_model::Formula, raw: &str) -> PyFormula {
    // The cheap test first: most formulas are not `PY(` calls.
    if !gridcraft_model::python::is_py_formula(&formula.text) {
        return PyFormula::Not;
    }
    let expr = formula.expr_arc();
    let Some((index, object, params)) = expr.as_deref().and_then(py_call) else {
        return PyCode::from_formula(raw).map_or(PyFormula::Not, PyFormula::Code);
    };
    let code = scripts.scripts.get(index).map(|code| with_params(code, params));
    match code.filter(|c| c.len() <= scripts.budget) {
        Some(code) => {
            scripts.budget -= code.len();
            PyFormula::Code(PyCode { object, output_chosen: true, ..PyCode::new(&code) })
        }
        None => {
            scripts.missing += 1;
            PyFormula::Missing
        }
    }
}

/// `code` with every `%Px%` (`x` at least 2, naming one of `params`) replaced by the parameter's
/// text. Stops growing past [`MAX_SCRIPT_CHARS`] characters: such code can't be a cell's.
fn with_params(code: &str, params: &[Expr]) -> String {
    if params.is_empty() || !code.contains("%P") {
        return code.to_string();
    }
    let texts: Vec<String> = params.iter().map(gridcraft_formula::print).collect();
    let mut out = String::with_capacity(code.len());
    let mut chars = 0usize;
    let mut push = |out: &mut String, piece: &str| {
        out.push_str(piece);
        chars = chars.saturating_add(piece.chars().count());
        chars > MAX_SCRIPT_CHARS
    };
    let mut rest = code;
    while let Some(at) = rest.find("%P") {
        if push(&mut out, rest.get(..at).unwrap_or_default()) {
            return out;
        }
        let after = rest.get(at + 2..).unwrap_or_default();
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let param = (1..=6)
            .contains(&digits)
            .then(|| after.get(..digits).and_then(|d| d.parse::<usize>().ok()))
            .flatten()
            .filter(|_| after.as_bytes().get(digits) == Some(&b'%'))
            .and_then(|x| x.checked_sub(2))
            .and_then(|x| texts.get(x));
        let full = match param {
            Some(text) => {
                rest = after.get(digits + 1..).unwrap_or_default();
                push(&mut out, text)
            }
            None => {
                rest = after;
                push(&mut out, "%P")
            }
        };
        if full {
            return out;
        }
    }
    out.push_str(rest);
    out
}

/// The formula a Python cell is saved as: `_xlfn._xlws.PY(index,0|1)`, its code being script
/// `index` of the part ([`write`]).
pub fn file_formula(index: usize, object: bool) -> String {
    format!("{FILE_PREFIX}PY({index},{})", u8::from(object))
}

/// Whether an environment has an `environmentDefinition` to write: anything but what a file's other
/// content made it hold.
fn defines_environment(env: &PyEnvironment) -> bool {
    env.has_code()
        || env.user_modified
        || env.id.is_some()
        || env.kept.as_deref().is_some_and(|k| {
            k.definition
                || !k.environment_attrs.is_empty()
                || !k.environment_elements.is_empty()
                || !k.initialization_attrs.is_empty()
                || !k.initialization_elements.is_empty()
        })
}

/// Whether the workbook needs a Python part: it has scripts, or an environment with something in it.
pub fn needs_part(scripts: &[String], env: Option<&PyEnvironment>) -> bool {
    !scripts.is_empty()
        || env.is_some_and(|e| defines_environment(e) || e.kept.as_deref().is_some_and(|k| !k.root_elements.is_empty() || !k.root_attrs.is_empty()))
}

/// The Python part holding the environment, when there is one, and `scripts`, in order. The format
/// requires one `pythonScript` in `pythonScripts`, so without scripts it has none.
pub fn write(scripts: &[String], env: Option<&PyEnvironment>) -> String {
    let kept = env.and_then(|e| e.kept.as_deref());
    let mut xml = format!("{}<python xmlns=\"{NS}\"{}>", crate::write::XML_DECL, kept.map_or("", |k| k.root_attrs.as_str()));
    if let Some(env) = env.filter(|e| defines_environment(e)) {
        write_environment(&mut xml, env);
    }
    if !scripts.is_empty() {
        xml.push_str("<pythonScripts>");
        for code in scripts {
            xml.push_str("<pythonScript><code>");
            xml.push_str(&xml::esc(&xml::encode_xstring(code)));
            xml.push_str("</code></pythonScript>");
        }
        xml.push_str("</pythonScripts>");
    }
    if let Some(k) = kept {
        xml.push_str(&k.root_elements);
    }
    xml.push_str("</python>");
    xml
}

/// `environmentDefinition`, in the order the schema has its content. The format requires a GUID, so
/// one is always there. Code a person emptied stays as an empty `code`; a file's own lack of one, or of
/// `initialization`, stays as it was.
fn write_environment(xml: &mut String, env: &PyEnvironment) {
    let kept = env.kept.as_deref();
    let id = env.id.as_deref().filter(|id| is_guid(id)).map_or_else(|| environment_id(&env.code), str::to_string);
    let _ = write!(xml, "<environmentDefinition id=\"{}\"{}>", xml::esc_attr(&id), kept.map_or("", |k| k.environment_attrs.as_str()));
    let code = !env.code.is_empty() || env.user_modified;
    if code || !kept.is_some_and(|k| k.no_initialization) {
        xml.push_str("<initialization");
        if env.user_modified {
            xml.push_str(" userModified=\"1\"");
        }
        xml.push_str(kept.map_or("", |k| k.initialization_attrs.as_str()));
        xml.push('>');
        if code {
            let _ = write!(xml, "<code>{}</code>", xml::esc(&xml::encode_xstring(&env.code)));
        }
        xml.push_str(kept.map_or("", |k| k.initialization_elements.as_str()));
        xml.push_str("</initialization>");
    }
    xml.push_str(kept.map_or("", |k| k.environment_elements.as_str()));
    xml.push_str("</environmentDefinition>");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formula(text: &str) -> gridcraft_model::Formula {
        gridcraft_model::Formula::new(&crate::fmla::from_file(text))
    }

    fn scripts(list: &[&str]) -> ScriptsIn {
        ScriptsIn { scripts: list.iter().map(|s| s.to_string()).collect(), budget: MAX_CODE_BYTES, ..ScriptsIn::default() }
    }

    #[test]
    fn reads_the_index_form_and_parameters() {
        let mut s = scripts(&["1 + 1", "df = xl(\"%P2%\", headers=True)\nxl('%P3%') + %P9% + %P1%"]);
        let code = |s: &mut ScriptsIn, text: &str| match cell_code(s, &formula(text), text) {
            PyFormula::Code(py) => Some(py),
            _ => None,
        };
        let a = code(&mut s, "_xlfn._xlws.PY(0,1)").unwrap();
        assert_eq!((a.code.as_str(), a.object), ("1 + 1", true));
        let b = code(&mut s, "_xlfn._xlws.PY(1,0,A1:B3,Sheet2!$C$4)").unwrap();
        assert_eq!(b.code, "df = xl(\"A1:B3\", headers=True)\nxl('Sheet2!$C$4') + %P9% + %P1%");
        assert!(!b.object);
        // The literal form still reads (other applications, older files).
        assert_eq!(code(&mut s, "_xlfn._xlws.PY(\"x = 1\",0)").unwrap().code, "x = 1");
        // A script the part doesn't hold: the cell keeps its value, and is counted for the warning.
        let gone = "_xlfn._xlws.PY(7,0)";
        assert_eq!(cell_code(&mut s, &formula(gone), gone), PyFormula::Missing);
        assert!(s.warning().is_some());
        for not in ["_xlfn._xlws.PY(-1,0)", "_xlfn._xlws.PY(0.5,0)", "_xlfn._xlws.PY(0,2)", "PY(A1,0)", "SUM(0,1)", "PYTHON_TYPE(A1)"] {
            assert_eq!(cell_code(&mut s, &formula(not), not), PyFormula::Not, "{not}");
        }
    }

    #[test]
    fn hostile_scripts_are_bounded() {
        // Many formulas naming one script take from one budget.
        let code = "x".repeat(1_000);
        let mut s = ScriptsIn { scripts: vec![code], budget: 2_500, ..ScriptsIn::default() };
        let f = formula("_xlfn._xlws.PY(0,0)");
        let taken = (0..5).filter(|_| matches!(cell_code(&mut s, &f, ""), PyFormula::Code(_))).count();
        assert_eq!(taken, 2);
        assert_eq!(s.missing, 3);
        // A parameter repeated many times stops growing the code just past a cell's limit.
        let many = "%P2%".repeat(5_000);
        let out = with_params(&many, &[Expr::Name("A_VERY_LONG_NAME_FOR_A_RANGE".into())]);
        assert!((MAX_SCRIPT_CHARS..MAX_SCRIPT_CHARS + 40).contains(&out.chars().count()), "{}", out.len());
        assert!(!PyCode::new(&out).fits());
        // Odd tokens stay as they are.
        assert_eq!(with_params("%P%P2%%P99999999%%P", &[Expr::Number(1.0)]), "%P1%P99999999%%P");
        // A long script is cut just past the limit, so it fails the cell's check.
        let el = El {
            name: "pythonScript".into(),
            children: vec![El { name: "code".into(), text: "y".repeat(20_000), ..El::default() }],
            ..El::default()
        };
        assert_eq!(script_code(&el).chars().count(), MAX_SCRIPT_CHARS);
    }

    /// The environment `write` puts in the part, read back through the part's own scan.
    fn read_back(xml: &str) -> (El, PyEnvironmentXml) {
        let root = xml::parse_streaming_in(xml.as_bytes(), NS, &["pythonScript"], &mut |_| Ok(())).unwrap();
        (root, kept_xml(xml.as_bytes()).unwrap().0)
    }

    /// The scripts' code a part's own reading finds.
    fn scripts_of(xml: &str) -> Vec<String> {
        let mut read = Vec::new();
        xml::parse_streaming_in(xml.as_bytes(), NS, &["pythonScript"], &mut |el| {
            read.push(script_code(&el));
            Ok(())
        })
        .unwrap();
        read
    }

    #[test]
    fn another_namespaces_element_with_a_python_name_stays_and_is_not_read() {
        const GUID: &str = "{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}";
        // Before and after the real one, prefixed and by a default of its own, at every level.
        let part = format!(
            "<python xmlns=\"{NS}\" xmlns:v=\"urn:vendor\">\
             <v:environmentDefinition id=\"v-e\"><initialization><code>not the environment</code></initialization></v:environmentDefinition>\
             <environmentDefinition id=\"{GUID}\">\
             <v:initialization userModified=\"1\"><code>not the initialization</code></v:initialization>\
             <initialization>\
             <v:code>before</v:code><code xmlns=\"urn:other\">other</code><code>x = 1</code><v:code>after</v:code>\
             </initialization></environmentDefinition>\
             <v:pythonScripts><pythonScript><code>not a script</code></pythonScript></v:pythonScripts>\
             <pythonScripts><pythonScript><code>2</code></pythonScript></pythonScripts></python>"
        );
        let (root, kept) = read_back(&part);
        let definition = root.child("environmentDefinition").unwrap();
        assert_eq!(definition.attr("id"), Some(GUID));
        let init = definition.child("initialization").unwrap();
        assert_eq!(decoded_code(init, MAX_INIT_CHARS_KEPT), "x = 1");
        assert!(!init.flag("userModified", false), "the flag was on another namespace's element");
        assert_eq!(scripts_of(&part), ["2"]);
        assert_eq!(
            kept.root_elements,
            "<v:environmentDefinition id=\"v-e\"><initialization><code>not the environment</code></initialization></v:environmentDefinition>\
             <v:pythonScripts><pythonScript><code>not a script</code></pythonScript></v:pythonScripts>"
        );
        assert_eq!(kept.environment_elements, "<v:initialization userModified=\"1\"><code>not the initialization</code></v:initialization>");
        assert_eq!(kept.initialization_elements, "<v:code>before</v:code><code xmlns=\"urn:other\">other</code><v:code>after</v:code>");
        // Written and read again, it says the same, twice over.
        let env = PyEnvironment { code: "x = 1".into(), id: Some(GUID.into()), kept: Some(std::sync::Arc::new(kept.clone())), ..Default::default() };
        let written = write(&["2".to_string()], Some(&env));
        let (again, kept_again) = read_back(&written);
        assert_eq!(kept_again, kept, "{written}");
        assert_eq!(decoded_code(again.path(&["environmentDefinition", "initialization"]).unwrap(), MAX_INIT_CHARS_KEPT), "x = 1");
        assert_eq!(scripts_of(&written), ["2"]);
        assert_eq!(write(&["2".to_string()], Some(&PyEnvironment { kept: Some(kept_again.into()), ..env })), written);
    }

    #[test]
    fn the_parts_own_names_read_whatever_prefix_they_have() {
        let part = format!(
            "<p:python xmlns:p=\"{NS}\" xmlns:v=\"urn:vendor\"><p:environmentDefinition><q:initialization xmlns:q=\"{NS}\">\
             <v:code>theirs</v:code><q:code>x = 1</q:code></q:initialization></p:environmentDefinition>\
             <p:pythonScripts><v:pythonScript><p:code>not a script</p:code></v:pythonScript><p:pythonScript><v:code>theirs</v:code>\
             <p:code>2</p:code></p:pythonScript></p:pythonScripts></p:python>"
        );
        let (root, kept) = read_back(&part);
        assert_eq!(decoded_code(root.path(&["environmentDefinition", "initialization"]).unwrap(), MAX_INIT_CHARS_KEPT), "x = 1");
        // The script of another namespace takes no index, so `PY(0,…)` still names the real one.
        assert_eq!(scripts_of(&part), ["2"]);
        // Under a saved root it would take the part's default, so it says it has none.
        assert_eq!(kept.initialization_elements, "<v:code xmlns=\"\">theirs</v:code>");
    }

    #[test]
    fn an_attribute_with_a_prefix_is_not_the_parts_attribute() {
        const GUID: &str = "{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}";
        const THEIRS: &str = "{11111111-2222-4333-8444-555555555555}";
        let part = format!(
            "<python xmlns=\"{NS}\" xmlns:v=\"urn:vendor\"><environmentDefinition v:id=\"{THEIRS}\" id=\"{GUID}\">\
             <initialization v:userModified=\"1\"/></environmentDefinition></python>"
        );
        let (root, kept) = read_back(&part);
        let definition = root.child("environmentDefinition").unwrap();
        assert_eq!(definition.attr("id"), Some(GUID));
        assert!(!definition.child("initialization").unwrap().flag("userModified", false));
        assert_eq!(kept.environment_attrs, format!(" v:id=\"{THEIRS}\""));
        assert_eq!(kept.initialization_attrs, " v:userModified=\"1\"");
    }

    #[test]
    fn a_root_in_another_namespace_is_not_the_python_part() {
        for part in [
            "<python><pythonScripts><pythonScript><code>1</code></pythonScript></pythonScripts></python>".to_string(),
            "<python xmlns=\"urn:other\"><pythonScripts><pythonScript><code>1</code></pythonScript></pythonScripts></python>".to_string(),
            format!(
                "<v:python xmlns:v=\"urn:other\" xmlns=\"{NS}\"><pythonScripts><pythonScript><code>1</code></pythonScript></pythonScripts></v:python>"
            ),
        ] {
            let mut scripts = 0;
            let parsed = xml::parse_streaming_in(part.as_bytes(), NS, &["pythonScript"], &mut |_| {
                scripts += 1;
                Ok(())
            });
            assert!(parsed.is_err(), "{part}");
            assert_eq!(scripts, 0, "{part}");
        }
    }

    #[test]
    fn what_a_save_has_no_place_for_is_counted() {
        let unkept = |part: String| kept_xml(part.as_bytes()).unwrap().1;
        // A script and its code as the format has them, and the declarations a part may carry.
        assert_eq!(
            unkept(format!(
                "<python xmlns=\"{NS}\"><pythonScripts xmlns:v=\"urn:v\"><pythonScript><code xml:space=\"preserve\">1</code></pythonScript></pythonScripts></python>"
            )),
            0
        );
        // One each: an attribute of `pythonScripts`, of a script and of its code, an element beside the
        // scripts and one beside the code, and an attribute of the initialization's code.
        let part = format!(
            "<python xmlns=\"{NS}\" xmlns:v=\"urn:v\"><environmentDefinition><initialization><code v:a=\"1\">x</code></initialization></environmentDefinition>\
             <pythonScripts v:a=\"1\"><v:note/><pythonScript v:a=\"1\"><code v:a=\"1\">1</code><v:code>theirs</v:code></pythonScript></pythonScripts></python>"
        );
        assert_eq!(unkept(part), 6);
    }

    #[test]
    fn another_namespaces_elements_nested_without_end_are_refused() {
        // The namespace reader's depth is a 16-bit count, which a part this deep would run past.
        let deep = format!("<python xmlns=\"{NS}\"><v:a xmlns:v=\"urn:v\">{}</v:a></python>", "<v:a>".repeat(70_000));
        let e = xml::parse_streaming_in(deep.as_bytes(), NS, &["pythonScript"], &mut |_| Ok(())).unwrap_err();
        assert!(e.to_string().contains("nested too deeply"), "{e}");
    }

    #[test]
    fn an_element_keeps_the_default_namespace_it_had_under_a_prefixed_root() {
        let part = format!("<p:python xmlns:p=\"{NS}\" xmlns=\"urn:other\"><extra a=\"1\"/><own xmlns=\"urn:own\"/><p:more/></p:python>");
        let (_, kept) = read_back(&part);
        assert_eq!(kept.root_elements, "<extra xmlns=\"urn:other\" a=\"1\"/><own xmlns=\"urn:own\"/><p:more xmlns=\"urn:other\"/>");
        // A prefixed element passes its default on to what it holds.
        let (_, kept) = read_back(&format!("<p:python xmlns:p=\"{NS}\"><p:wrap><x/></p:wrap></p:python>"));
        assert_eq!(kept.root_elements, "<p:wrap xmlns=\"\"><x/></p:wrap>");
        let (_, kept) = read_back(&format!("<p:python xmlns:p=\"{NS}\" xmlns=\"urn:other\"><p:wrap><x/></p:wrap></p:python>"));
        assert_eq!(kept.root_elements, "<p:wrap xmlns=\"urn:other\"><x/></p:wrap>");
        // No default at all is one too: under a saved root it would take the part's.
        let (_, kept) = read_back(&format!("<p:python xmlns:p=\"{NS}\"><extra/></p:python>"));
        assert_eq!(kept.root_elements, "<extra xmlns=\"\"/>");
        // Each element goes by the default where it was: the root's, or one a parent set.
        let guid = "{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}";
        let (_, kept) = read_back(&format!(
            "<p:python xmlns:p=\"{NS}\" xmlns=\"urn:other\"><p:environmentDefinition id=\"{guid}\" xmlns=\"{NS}\"><here/>\
             <p:initialization xmlns=\"urn:init\"><deep/></p:initialization></p:environmentDefinition></p:python>"
        ));
        assert_eq!(kept.environment_elements, "<here/>", "the part's own namespace needs nothing");
        assert_eq!(kept.initialization_elements, "<deep xmlns=\"urn:init\"/>");
        assert_eq!((kept.environment_attrs.as_str(), kept.initialization_attrs.as_str()), ("", ""), "a save declares none of these");
    }

    #[test]
    fn an_attribute_with_either_kind_of_quote_comes_through_whole() {
        let guid = "{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}";
        let (_, kept) = read_back(&format!(
            "<python xmlns=\"{NS}\" xmlns:a=\"urn:a\"><environmentDefinition id=\"{guid}\" a:e='x\"y'>\
             <initialization a:i='p\"q'/></environmentDefinition></python>"
        ));
        assert_eq!(kept.environment_attrs, " a:e=\"x&quot;y\"");
        assert_eq!(kept.initialization_attrs, " a:i=\"p&quot;q\"");
    }

    #[test]
    fn what_a_part_holds_besides_code_keeps_it_and_its_environment() {
        let env = |kept: PyEnvironmentXml| PyEnvironment { kept: Some(kept.into()), ..PyEnvironment::default() };
        let attributes = env(PyEnvironmentXml { root_attrs: " xmlns:a=\"urn:a\"".into(), ..Default::default() });
        assert!(needs_part(&[], Some(&attributes)));
        // A definition whose GUID couldn't be read, with an empty initialization.
        let definition = env(PyEnvironmentXml { definition: true, ..Default::default() });
        assert!(defines_environment(&definition));
        // Whatever an initialization holds besides code needs a definition around it.
        for kept in [
            PyEnvironmentXml { initialization_attrs: " a:i=\"1\"".into(), ..Default::default() },
            PyEnvironmentXml { initialization_elements: "<a:x/>".into(), ..Default::default() },
        ] {
            let held = kept.initialization_attrs.clone() + &kept.initialization_elements;
            let env = env(kept);
            assert!(needs_part(&[], Some(&env)), "{held}");
            let part = write(&[], Some(&env));
            assert!(part.contains("<environmentDefinition") && part.contains(held.trim()), "{part}");
        }
        assert!(!needs_part(&[], Some(&env(PyEnvironmentXml::default()))));
        // A readable GUID is enough by itself, and adds nothing to keep.
        let (_, kept) =
            read_back(&format!("<python xmlns=\"{NS}\"><environmentDefinition id=\"not a guid\"><initialization/></environmentDefinition></python>"));
        assert!(kept.definition);
        let (_, kept) = read_back(&format!(
            "<python xmlns=\"{NS}\"><environmentDefinition id=\"{{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}}\"><initialization/></environmentDefinition></python>"
        ));
        assert!(!kept.definition);
    }

    #[test]
    fn what_the_part_holds_around_the_environment_is_kept_as_it_came() {
        let part = format!(
            "<?xml version=\"1.0\"?><python xmlns=\"{NS}\" xmlns:a=\"urn:a\" a:r='x\"y'>\
             <a:before/>\
             <environmentDefinition id=\"{{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}}\" a:e=\"1\">\
             <initialization userModified=\"true\" a:i=\"&amp;\"><code>x = 1</code><a:inside k=\"1\">t</a:inside></initialization>\
             <extLst><ext uri=\"{{1}}\"><a:deep><a:er/></a:deep></ext></extLst>\
             </environmentDefinition>\
             <pythonScripts><pythonScript><code>2</code></pythonScript></pythonScripts>\
             <extLst><ext uri=\"{{2}}\"/></extLst></python>"
        );
        let (_, kept) = read_back(&part);
        assert_eq!(kept.root_attrs, " xmlns:a=\"urn:a\" a:r=\"x&quot;y\"");
        assert_eq!(kept.root_elements, "<a:before/><extLst><ext uri=\"{2}\"/></extLst>");
        assert_eq!(kept.environment_attrs, " a:e=\"1\"");
        assert_eq!(kept.environment_elements, "<extLst><ext uri=\"{1}\"><a:deep><a:er/></a:deep></ext></extLst>");
        assert_eq!(kept.initialization_attrs, " a:i=\"&amp;\"");
        assert_eq!(kept.initialization_elements, "<a:inside k=\"1\">t</a:inside>");
        assert!(!kept.no_initialization);
        // Written back in the schema's order, it says the same again.
        let env = PyEnvironment {
            code: "x = 1".into(),
            user_modified: true,
            id: Some("{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}".into()),
            kept: Some(std::sync::Arc::new(kept.clone())),
        };
        let written = write(&["2".to_string()], Some(&env));
        assert!(
            written.contains("<environmentDefinition id=\"{AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE}\" a:e=\"1\"><initialization userModified=\"1\" a:i=\"&amp;\"><code>x = 1</code><a:inside k=\"1\">t</a:inside></initialization><extLst>"),
            "{written}"
        );
        let (scripts, tail) = (written.find("<pythonScripts>").unwrap(), written.find("<a:before/>").unwrap());
        assert!(written.find("<environmentDefinition").unwrap() < scripts && scripts < tail, "{written}");
        let (root, again) = read_back(&written);
        assert_eq!(root.name, "python");
        assert_eq!(again, kept);
    }

    #[test]
    fn nothing_unasked_is_added_to_an_environment() {
        // No `initialization`: none is added while there is no code.
        let part = format!("<python xmlns=\"{NS}\"><environmentDefinition id=\"{{00000000-0000-0000-0000-000000000000}}\"/></python>");
        let (_, kept) = read_back(&part);
        assert!(kept.no_initialization);
        let env = PyEnvironment {
            id: Some("{00000000-0000-0000-0000-000000000000}".into()),
            kept: Some(std::sync::Arc::new(kept)),
            ..PyEnvironment::default()
        };
        let written = write(&[], Some(&env));
        assert!(
            written.ends_with("<environmentDefinition id=\"{00000000-0000-0000-0000-000000000000}\"></environmentDefinition></python>"),
            "{written}"
        );
        // A second `environmentDefinition`, which the format doesn't allow, isn't kept as if it were
        // something unknown.
        let two = format!("<python xmlns=\"{NS}\"><environmentDefinition/><environmentDefinition a=\"b\"><x/></environmentDefinition></python>");
        let (_, kept) = read_back(&two);
        assert_eq!((kept.root_elements.as_str(), kept.environment_attrs.as_str()), ("", ""));
        // An element that isn't closed loses what can't be followed, and nothing else.
        assert!(kept_xml(format!("<python xmlns=\"{NS}\"><a><b></a></python>").as_bytes()).is_none());
        // Only a person's emptied code is written empty.
        let emptied = PyEnvironment { user_modified: true, id: Some("{00000000-0000-0000-0000-000000000000}".into()), ..PyEnvironment::default() };
        assert!(write(&[], Some(&emptied)).contains("<initialization userModified=\"1\"><code></code></initialization>"));
        // A part is only needed for something to put in it.
        assert!(!needs_part(&[], None));
        assert!(!needs_part(&[], Some(&PyEnvironment::default())));
        assert!(needs_part(&["1".to_string()], None));
        assert!(needs_part(&[], Some(&PyEnvironment::from_person("x = 1"))));
    }

    #[test]
    fn initialization_code_survives_what_escapes_it() {
        let code = "a = \"<&>\"\nb = '_x0041_'\n\tc = 1\n# ends with spaces  ";
        let env = PyEnvironment { id: Some(environment_id(code)), ..PyEnvironment::from_person(code) };
        assert!(is_guid(env.id.as_deref().unwrap()));
        let written = write(&[], Some(&env));
        let (root, _) = read_back(&written);
        let init = root.child("environmentDefinition").and_then(|d| d.child("initialization")).unwrap();
        assert_eq!(decoded_code(init, MAX_INIT_CHARS_KEPT), code);
        assert_eq!(init.attr_bool("userModified"), Some(true));
        // Cut on the decoded code, so what escapes it doesn't cut code that fits.
        let long = "\u{1F600}".repeat(MAX_INIT_CHARS);
        let el = El {
            name: "initialization".into(),
            children: vec![El { name: "code".into(), text: xml::encode_xstring(&long), ..El::default() }],
            ..El::default()
        };
        assert_eq!(decoded_code(&el, MAX_INIT_CHARS_KEPT), long);
        let over = El { children: vec![El { name: "code".into(), text: "y".repeat(MAX_INIT_CHARS * 3), ..El::default() }], ..El::default() };
        assert_eq!(decoded_code(&over, MAX_INIT_CHARS_KEPT).chars().count(), MAX_INIT_CHARS_KEPT);
    }

    #[test]
    fn written_scripts_read_back() {
        let codes = vec!["a = 1\nb = \"two\" & 'three' <tag>".to_string(), "_x0041_ \u{1}".to_string(), String::new()];
        let xml = write(&codes, None);
        assert!(xml.contains(NS) && xml.contains("<pythonScripts><pythonScript><code>a = 1\nb = \"two\" &amp; 'three' &lt;tag&gt;</code>"), "{xml}");
        let mut read = Vec::new();
        let root = xml::parse_streaming(xml.as_bytes(), &["pythonScript"], &mut |el| {
            read.push(script_code(&el));
            Ok(())
        })
        .unwrap_or_default();
        assert_eq!(root.name, "python");
        assert_eq!(read, codes);
        assert_eq!(file_formula(3, true), "_xlfn._xlws.PY(3,1)");
    }
}
