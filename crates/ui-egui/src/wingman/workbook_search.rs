//! Read-only search of the open in-memory workbook. The continuation names a physical sparse
//! address and field, not a number of matches to replay. No selections or files are touched.
use std::borrow::Cow;
use std::hash::{DefaultHasher, Hash, Hasher};

use gridcraft_engine::DocState;
use gridcraft_engine::core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_engine::model::{Sheet, Visibility};
use regex::{Regex, RegexBuilder};
use serde_json::{Value as Json, json};

const WORK: usize = 3000;
const PAGE_BYTES: usize = 24 * 1024;
const SCAN_BYTES: usize = 512 * 1024;
// One Excel text cell (32,767 UTF-16 units) fits in this many UTF-8 bytes, even for CJK.
const FIELD_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy)]
struct Pos {
    sheet: usize,
    at: CellRef,
    field: usize,
}
impl Pos {
    fn token(self, tag: u64) -> String {
        format!("{tag:x}.{:x}.{:x}.{:x}.{:x}", self.sheet, self.at.row, self.at.col, self.field)
    }
    fn parse(token: &str, tag: u64) -> Option<Self> {
        if token.len() > 128 {
            return None;
        }
        let mut it = token.split('.');
        if u64::from_str_radix(it.next()?, 16).ok()? != tag {
            return None;
        }
        let sheet = usize::from_str_radix(it.next()?, 16).ok()?;
        let at = CellRef::new(u32::from_str_radix(it.next()?, 16).ok()?, u32::from_str_radix(it.next()?, 16).ok()?);
        let field = usize::from_str_radix(it.next()?, 16).ok()?;
        (it.next().is_none() && at.is_valid()).then_some(Self { sheet, at, field })
    }
}

// Restrict the first row with a B-tree seek even when the cursor is near XFD. Bound rows
// here; narrow the columns in the metered scan so a narrow search cannot skip an unbounded
// number of occupied rows inside an iterator between budget checks.
fn addresses<'a>(sh: &'a Sheet, from: CellRef, end: u32, which: &str) -> impl Iterator<Item = CellRef> + use<'a> {
    let max_col = MAX_COLS - 1;
    let hi = CellRef::new(end, max_col);
    let first = sh.cells.row(from.row, from.col, max_col).map(move |(c, _)| CellRef::new(from.row, c));
    let rest =
        (from.row < end).then(|| RangeRef::new(CellRef::new(from.row + 1, 0), hi)).into_iter().flat_map(|r| sh.cells.iter_range(r).map(|(c, _)| c));
    let mut cells = (which != "comment").then_some(first.chain(rest)).into_iter().flatten().peekable();
    let mut notes = matches!(which, "all" | "comment").then(|| sh.comments.range(from..=hi)).into_iter().flatten().map(|(at, _)| *at).peekable();
    let mut spills =
        matches!(which, "all" | "value" | "display" | "error").then(|| sh.spill.range(from..=hi)).into_iter().flatten().map(|(at, _)| *at).peekable();
    std::iter::from_fn(move || {
        let at = [cells.peek().copied(), notes.peek().copied(), spills.peek().copied()].into_iter().flatten().min()?;
        if cells.peek() == Some(&at) {
            cells.next();
        }
        if notes.peek() == Some(&at) {
            notes.next();
        }
        if spills.peek() == Some(&at) {
            spills.next();
        }
        Some(at)
    })
}

struct Field<'a> {
    kind: &'static str,
    text: Cow<'a, str>,
    reply: Option<usize>,
}
impl<'a> Field<'a> {
    fn new(kind: &'static str, text: impl Into<Cow<'a, str>>) -> Self {
        Self { kind, text: text.into(), reply: None }
    }
}

// Return Err(()) for an uncommonly large field; callers disclose skipped fields instead of
// silently matching a truncated prefix (notably incorrect for regex anchors). Excerpts alone
// are clipped; every ordinary Excel text cell is searched in full.
fn field<'a>(d: &DocState, sh: &'a Sheet, at: CellRef, index: usize, scope: &str) -> Result<Option<Field<'a>>, ()> {
    let value = sh.value_ref(at);
    let all = scope == "all";
    let value = match index {
        0 if scope == "error" => match value {
            Some(Value::Error(e)) => Some(Field::new("error", e.as_str())),
            _ => None,
        },
        0 if all || scope == "value" => match value {
            Some(Value::Text(t)) => Some(Field::new("value", t.as_ref())),
            Some(v @ (Value::Bool(_) | Value::Number(_) | Value::Error(_))) => Some(Field::new("value", v.display())),
            _ => None,
        },
        1 if all || scope == "display" => {
            let Some(v) = value.filter(|v| !matches!(v, Value::Empty | Value::Array(_))) else {
                return Ok(None);
            };
            let fmt = d.wb.styles.get(sh.style_id(at)).num_fmt.as_str();
            if fmt.len() > 1024 {
                return Err(());
            }
            if let Value::Text(t) = v {
                if t.len() > FIELD_BYTES {
                    return Err(());
                }
                if fmt == "General" || fmt == "@" {
                    return Ok((!all && !t.is_empty()).then(|| Field::new("display", t.as_ref())));
                }
                // A custom format can repeat '@'. Bound its possible expansion before invoking
                // the formatter rather than allocating a huge string and clipping it afterward.
                if t.len().saturating_mul(fmt.bytes().filter(|&b| b == b'@').count().max(1)).saturating_add(fmt.len()) > FIELD_BYTES {
                    return Err(());
                }
            }
            // Display formatting uses the same code as the grid, but never reveals a formula
            // when the user toggled Show Formulas: formula source is a separate guarded field.
            let rendered = gridcraft_engine::display::value_text_as(&d.wb, v, fmt);
            let duplicate = all
                && match v {
                    Value::Text(t) => rendered == t.as_ref(),
                    _ => rendered == v.display(),
                };
            (!duplicate && !rendered.is_empty()).then(|| Field::new("display", rendered))
        }
        2 if all || scope == "formula" => {
            let Some(f) = sh.cell(at).and_then(|v| v.formula.as_ref()) else {
                return Ok(None);
            };
            if gridcraft_engine::display::formula_hidden(&d.wb, sh, at) {
                return Ok(None);
            }
            if f.text.len() >= FIELD_BYTES {
                return Err(());
            }
            Some(Field::new("formula", format!("={}", f.text)))
        }
        3.. if all || scope == "comment" => {
            let Some(c) = sh.comments.get(&at) else {
                return Ok(None);
            };
            let n = index - 3;
            let (who, body, reply) = if n < 2 {
                (&c.author, &c.text, None)
            } else {
                let i = (n - 2) / 2;
                let Some((who, body)) = c.replies.get(i) else {
                    return Ok(None);
                };
                (who, body, Some(i + 1))
            };
            let (kind, text) = match (n % 2, reply) {
                (0, None) => ("comment", body),
                (0, Some(_)) => ("reply", body),
                (_, None) => ("commentAuthor", who),
                (_, Some(_)) => ("replyAuthor", who),
            };
            Some(Field { kind, text: text.as_str().into(), reply })
        }
        _ => None,
    };
    if value.as_ref().is_some_and(|f| f.text.len() > FIELD_BYTES) { Err(()) } else { Ok(value) }
}

fn excerpt(text: &str, at: usize) -> (String, bool) {
    let start = text[..at].char_indices().rev().nth(39).map_or(0, |(i, _)| i);
    let end = text[start..].char_indices().nth(160).map_or(text.len(), |(i, _)| start + i);
    (text[start..end].to_owned(), start != 0 || end != text.len())
}

struct Page {
    matches: Vec<Json>,
    scanned: usize,
    work: usize,
    text_bytes: usize,
    skipped: usize,
    bytes: usize,
}
impl Page {
    fn finish(self, cursor: Option<String>) -> Json {
        json!({"matches":self.matches,"nextCursor":cursor,"scanned":self.scanned,"skippedOversizeFields":self.skipped})
    }
}

pub(in crate::wingman) struct Matcher {
    pattern: String,
    literal: bool,
    sensitive: bool,
    regex: Regex,
}

pub(super) fn grep_slice(d: &DocState, p: &Json, matcher: &mut Option<Matcher>) -> Result<Json, String> {
    let pattern = p["pattern"]
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= super::PATTERN_CHARS)
        .ok_or("Pattern must contain 1–512 characters.")?;
    let literal = !p["regex"].as_bool().unwrap_or(false);
    let sensitive = p["caseSensitive"].as_bool().unwrap_or(false);
    let scope = p["scope"].as_str().unwrap_or(if p.get("sheet").is_some() { "sheet" } else { "workbook" });
    let target = match scope {
        "workbook" if p.get("sheet").is_none() => None,
        "sheet" => Some(if let Some(name) = p["sheet"].as_str() {
            d.wb.sheet_index(name)
                .filter(|i| d.wb.sheet(*i).is_some_and(|s| s.visibility == Visibility::Visible))
                .ok_or("No such visible worksheet")?
        } else {
            d.wb.active_sheet()
        }),
        _ => return Err("Specify a worksheet or scope: workbook, not both.".into()),
    };
    if target.is_some_and(|id| d.wb.sheet(id).is_none_or(|s| s.visibility != Visibility::Visible)) {
        return Err("Worksheet is not visible".into());
    }
    let which = p["searchIn"].as_str().unwrap_or("all");
    if !matches!(which, "all" | "value" | "display" | "formula" | "comment" | "error") {
        return Err("Unknown searchIn field".into());
    }
    let range = match p.get("range") {
        Some(v) => v.as_str().and_then(RangeRef::parse).ok_or("Expected an unqualified A1 range")?,
        None => RangeRef::new(CellRef::new(0, 0), CellRef::new(MAX_ROWS - 1, MAX_COLS - 1)),
    };
    let limit = p.get("limit").map_or(Some(20), Json::as_u64).filter(|v| (1..=50).contains(v)).ok_or("limit must be 1–50")? as usize;
    let mut hasher = DefaultHasher::new();
    (d.uid, d.revision, target, range, which, pattern, sensitive, literal).hash(&mut hasher);
    let tag = hasher.finish();
    let first = CellRef::new(range.start.row, 0);
    let mut pos = match p.get("cursor") {
        Some(v) => v
            .as_str()
            .and_then(|t| Pos::parse(t, tag))
            .filter(|p| (range.start.row..=range.end.row).contains(&p.at.row) && p.sheet < d.wb.sheets.len() && target.is_none_or(|i| p.sheet == i))
            .ok_or("Cursor expired or invalid; restart this search without a cursor.")?,
        None => Pos { sheet: target.unwrap_or(0), at: first, field: 0 },
    };
    // A single provider tool call may take many frames. Keep its compiled automaton on
    // that call, not globally; workbook visibility and cursor validity are rechecked above.
    let re = &match matcher {
        Some(m) if m.pattern == pattern && m.literal == literal && m.sensitive == sensitive => m,
        Some(_) => return Err("Search pattern changed; start a new search.".into()),
        None => {
            let escaped = literal.then(|| regex::escape(pattern));
            let regex = RegexBuilder::new(escaped.as_deref().unwrap_or(pattern))
                .case_insensitive(!sensitive)
                .size_limit(1024 * 1024)
                .dfa_size_limit(512 * 1024)
                .nest_limit(64)
                .build()
                .map_err(|_| "Invalid or overly complex regular expression")?;
            matcher.insert(Matcher { pattern: pattern.into(), literal, sensitive, regex })
        }
    }
    .regex;
    let mut page = Page { matches: vec![], scanned: 0, work: 0, text_bytes: 0, skipped: 0, bytes: 0 };
    for si in pos.sheet..target.map_or(d.wb.sheets.len(), |i| i + 1) {
        if si != pos.sheet {
            pos = Pos { sheet: si, at: first, field: 0 };
        }
        if page.work >= WORK {
            return Ok(page.finish(Some(pos.token(tag))));
        }
        page.work += 1;
        let sh = &d.wb.sheets[si];
        if sh.visibility != Visibility::Visible {
            continue;
        }
        for at in addresses(sh, pos.at, range.end.row, which) {
            let mut next = Pos { sheet: si, at, field: if pos.at == at { pos.field } else { 0 } };
            if page.work >= WORK {
                return Ok(page.finish(Some(next.token(tag))));
            }
            page.work += 1;
            if !range.contains(at) {
                continue;
            }
            page.scanned += 1;
            let fields = match which {
                "error" | "value" => 1,
                "display" => 2,
                "formula" => 3,
                _ => sh.comments.get(&at).map_or(3, |c| 5usize.saturating_add(c.replies.len().saturating_mul(2))),
            };
            while next.field < fields {
                if page.work >= WORK || page.text_bytes >= SCAN_BYTES || page.matches.len() >= limit || page.bytes >= PAGE_BYTES {
                    return Ok(page.finish(Some(next.token(tag))));
                }
                page.work += 1;
                let i = next.field;
                next.field += 1;
                let f = match field(d, sh, at, i, which) {
                    Ok(Some(f)) => f,
                    Ok(None) => continue,
                    Err(()) => {
                        page.skipped += 1;
                        continue;
                    }
                };
                page.text_bytes += f.text.len();
                if let Some(hit) = re.find(&f.text) {
                    let (excerpt, clipped) = excerpt(&f.text, hit.start());
                    let mut obj =
                        json!({"sheet":super::context::clip(&sh.name,128),"cell":at.a1(),"field":f.kind,"excerpt":excerpt,"clipped":clipped});
                    if let Some(n) = f.reply {
                        obj["reply"] = json!(n);
                    }
                    if f.kind == "error" {
                        obj["errorCode"] = json!(f.text);
                    }
                    page.bytes += obj.to_string().len();
                    page.matches.push(obj);
                }
            }
        }
    }
    Ok(page.finish(None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::{
        Session,
        core::CellError,
        model::{Cell, SheetProtection},
    };
    use std::sync::Arc;

    fn grep(d: &DocState, p: &Json) -> Result<Json, String> {
        grep_slice(d, p, &mut None)
    }

    fn session() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }

    #[test]
    fn regex_finds_last_sparse_address_after_empty_pages_and_cursor_rejects_edits() {
        let mut s = session();
        let doc = s.active_mut().unwrap();
        let sh = Arc::make_mut(&mut doc.wb).sheet_mut(0).unwrap();
        for row in 0..(WORK as u32 + 1) {
            sh.set_value(CellRef::new(row, 0), Value::text("irrelevant"));
        }
        sh.set_value(CellRef::new(MAX_ROWS - 1, MAX_COLS - 1), Value::text("prefix τέλος 4.25"));
        let mut args = json!({"pattern":r"ΤΈΛΟΣ\s+\d+\.\d+", "regex":true,"range":"XFD:XFD", "searchIn":"value"});
        let mut matcher = None;
        let first = grep_slice(doc, &args, &mut matcher).unwrap();
        assert!(first["matches"].as_array().unwrap().is_empty());
        args["cursor"] = first["nextCursor"].clone();
        let second = grep_slice(doc, &args, &mut matcher).unwrap();
        assert_eq!(second["matches"][0]["cell"], "XFD1048576");
        assert!(second["nextCursor"].is_null());
        let mut changed_query = args.clone();
        changed_query["regex"] = false.into();
        assert!(grep_slice(doc, &changed_query, &mut matcher).is_err());
        assert!(grep_slice(doc, &json!({"pattern":"different"}), &mut matcher).is_err());
        doc.revision += 1;
        assert!(grep_slice(doc, &args, &mut matcher).is_err());
    }

    #[test]
    fn hidden_sources_and_sheets_are_private_but_typed_spill_errors_are_searchable() {
        let mut s = session();
        s.run("cell.set", json!({"cell":"A1","input":"=IF(FALSE,\"secret-token\",7)"})).unwrap();
        s.run("home.insertSheet", json!({"name":"Other"})).unwrap();
        s.run("cell.set", json!({"cell":"A1","input":"secret-token"})).unwrap();
        s.run("sheet.hide", json!({"sheet":"Other"})).unwrap();
        let doc = s.active_mut().unwrap();
        let wb = Arc::make_mut(&mut doc.wb);
        let style = wb.styles.derive(Default::default(), |s| s.protection.hidden = true);
        let sh = wb.sheet_mut(0).unwrap();
        sh.cells.get_mut(CellRef::new(0, 0)).unwrap().style = style;
        sh.protection = Some(SheetProtection::default());
        sh.show_formulas = true;
        sh.set_cell(CellRef::new(1, 0), Cell::value(Value::text("#N/A")));
        sh.spill.insert(CellRef::new(2, 0), Value::Error(CellError::NA));
        let empty = grep(doc, &json!({"pattern":"secret-token","scope":"workbook"})).unwrap();
        assert!(empty["matches"].as_array().unwrap().is_empty());
        assert!(grep(doc, &json!({"pattern":"secret","sheet":"Other"})).is_err());
        let hits = grep(doc, &json!({"pattern":"#N/A","searchIn":"error"})).unwrap();
        assert_eq!(hits["matches"].as_array().unwrap().len(), 1);
        assert_eq!(hits["matches"][0]["cell"], "A3");
        assert_eq!(hits["matches"][0]["errorCode"], "#N/A");
    }

    #[test]
    fn comment_only_cells_resume_between_replies_and_search_beyond_preview() {
        let mut s = session();
        s.run("review.newComment", json!({"cell":"D5","author":"Mi","text":"root"})).unwrap();
        s.run("review.replyComment", json!({"cell":"D5","author":"Ed","text":format!("{}x.y","字".repeat(600))})).unwrap();
        s.run("review.replyComment", json!({"cell":"D5","author":"Jo","text":"x.y again"})).unwrap();
        s.run("review.replyComment", json!({"cell":"D5","author":"Jo","text":"x/y is different"})).unwrap();
        let doc = s.active().unwrap();
        let mut args = json!({"pattern":"x.y","searchIn":"comment","limit":1});
        assert!(!grep(doc, &json!({"pattern":"字".repeat(200),"searchIn":"comment"})).unwrap()["matches"].as_array().unwrap().is_empty());
        let page = grep(doc, &args).unwrap();
        assert_eq!(page["matches"][0]["reply"], 1);
        assert_eq!(page["matches"][0]["clipped"], true);
        assert!(page["matches"][0]["excerpt"].as_str().unwrap().contains("x.y"));
        args["cursor"] = page["nextCursor"].clone();
        let second = grep(doc, &args).unwrap();
        assert_eq!(second["matches"][0]["reply"], 2);
        args["cursor"] = second["nextCursor"].clone();
        let end = grep(doc, &args).unwrap();
        assert!(end["matches"].as_array().unwrap().is_empty() && end["nextCursor"].is_null());
    }
}
