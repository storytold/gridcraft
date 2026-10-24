//! In-cell / formula-bar editing state: point mode, reference colouring, function
//! autocomplete and argument hints.

use egui::Color32;
use gridcraft_engine::core::{CellRef, RangeRef};

/// Colours for references in a formula being edited (our own palette).
pub const REF_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x2E, 0x6F, 0xD8),
    Color32::from_rgb(0xD0, 0x3C, 0x3C),
    Color32::from_rgb(0x7A, 0x4F, 0xC9),
    Color32::from_rgb(0x2B, 0x9A, 0x4A),
    Color32::from_rgb(0xC2, 0x5E, 0x1E),
    Color32::from_rgb(0x1D, 0x92, 0xA6),
    Color32::from_rgb(0xB8, 0x3B, 0x8E),
    Color32::from_rgb(0x8A, 0x7A, 0x1F),
];

#[derive(Clone, Debug)]
pub struct EditState {
    pub sheet: usize,
    /// Syntax of this edit, captured when it starts (prevents a language switch reinterpreting it).
    pub locale: gridcraft_engine::formula::FormulaLocale,
    pub cell: CellRef,
    pub text: String,
    /// Started by typing (Enter mode: arrows commit) rather than F2 (Edit mode: arrows move the caret).
    pub enter_mode: bool,
    pub from_formula_bar: bool,
    /// Caret position (char index), mirrored from the text widget.
    pub caret: usize,
    /// The reference most recently inserted by point mode (byte range) — arrows/clicks replace it.
    pub point: Option<(usize, usize)>,
    /// Where point mode's keyboard cursor is.
    pub point_cell: Option<CellRef>,
    /// The fixed range anchor and its sheet while pointing with the mouse or keyboard.
    pub point_anchor: Option<(usize, CellRef)>,
    pub autocomplete: Vec<String>,
    pub ac_index: usize,
    pub request_focus: bool,
    /// The text changed programmatically: push `caret` into the widget on the next frame.
    pub sync_caret: bool,
    /// Column AutoComplete: the full entry the typed text completes to.
    pub completion: Option<String>,
    /// Move the caret to the end on the next frame.
    pub caret_to_end: bool,
    /// The searchable list of a cell with an in-cell list validation dropdown.
    pub list: Option<ListSearch>,
    /// The user accepted the list's highlighted entry (Enter, Tab or a click on it). Any other
    /// commit (clicking elsewhere, a ribbon command, saving) keeps the typed text.
    pub list_accept: bool,
}

/// Entries of a list validation, searched as the user types (Excel's searchable dropdown).
#[derive(Clone, Debug, Default)]
pub struct ListSearch {
    pub items: Vec<String>,
    /// `items` folded for search once, so each keystroke only compares.
    folded: Vec<String>,
    /// Indices into `items` of the entries matching the typed text, in list order.
    pub matches: Vec<usize>,
    /// The highlighted entry, an index into `matches`.
    pub index: usize,
    /// Closed with Escape: hidden until the text changes again.
    pub closed: bool,
    /// First visible row of the popup (an index into `matches`).
    pub top: usize,
    /// The highlight moved: scroll the popup so it shows.
    pub reveal: bool,
}

impl ListSearch {
    /// Rows the popup shows at once.
    pub const VISIBLE: usize = 8;

    /// Scrolls `top` just enough to show the highlighted row.
    fn keep_visible(&mut self) {
        if self.index < self.top {
            self.top = self.index;
        } else if self.index >= self.top.saturating_add(Self::VISIBLE) {
            self.top = self.index.saturating_add(1).saturating_sub(Self::VISIBLE);
        }
        self.reveal = true;
    }
}

impl EditState {
    pub fn new(sheet: usize, cell: CellRef, text: String, enter_mode: bool, from_formula_bar: bool) -> EditState {
        let caret = text.chars().count();
        EditState {
            sheet,
            locale: Default::default(),
            cell,
            text,
            enter_mode,
            from_formula_bar,
            caret,
            point: None,
            point_cell: None,
            point_anchor: None,
            autocomplete: vec![],
            ac_index: 0,
            request_focus: true,
            sync_caret: true,
            caret_to_end: true,
            completion: None,
            list: None,
            list_accept: false,
        }
    }

    /// Gives this edit the entries of its cell's list validation. `search_now` filters them
    /// against the text right away (an edit started by typing); otherwise the list waits for
    /// the first change (F2 on an existing value).
    pub fn set_list_items(&mut self, items: Vec<String>, search_now: bool) {
        let folded = items.iter().map(|i| gridcraft_engine::cmd::data::search_fold(i)).collect();
        self.list = Some(ListSearch { items, folded, ..Default::default() });
        if search_now {
            self.update_list();
        }
    }

    /// Refilters the list against the typed text (call when the text changes).
    pub fn update_list(&mut self) {
        let typed = if self.is_formula() || self.text.trim().is_empty() { None } else { Some(self.text.as_str()) };
        let Some(list) = self.list.as_mut() else { return };
        list.matches.clear();
        list.index = 0;
        list.top = 0;
        list.reveal = true;
        list.closed = false;
        let Some(typed) = typed else { return };
        let term = gridcraft_engine::cmd::data::search_term(typed);
        let cap = gridcraft_engine::cmd::data::LIST_MATCH_CAP;
        // Text naming an entry exactly highlights that entry rather than the first match. The
        // kept matches stay the first `cap` in list order (as `data.validationList` returns), so
        // an exact entry past the cap isn't shown.
        let whole = gridcraft_engine::cmd::data::search_fold(typed.trim());
        let mut exact = None;
        for (i, f) in list.folded.iter().enumerate() {
            if list.matches.len() >= cap {
                break;
            }
            if !gridcraft_engine::cmd::data::search_matches(f, &term) {
                continue;
            }
            if exact.is_none() && *f == whole {
                exact = Some(list.matches.len());
            }
            list.matches.push(i);
        }
        if let Some(pos) = exact {
            list.index = pos;
            list.keep_visible();
        }
    }

    /// Whether the searched list is showing (it has matches and wasn't closed).
    pub fn list_open(&self) -> bool {
        self.list.as_ref().is_some_and(|l| !l.closed && !l.matches.is_empty())
    }

    /// The highlighted entry of the open list: what Enter or Tab commits (see `list_accept`).
    pub fn list_choice(&self) -> Option<&str> {
        let l = self.list.as_ref().filter(|_| self.list_open())?;
        l.matches.get(l.index).and_then(|&i| l.items.get(i)).map(String::as_str)
    }

    /// Moves the highlight down (or up), wrapping around.
    pub fn list_step(&mut self, down: bool) {
        if let Some(l) = self.list.as_mut()
            && !l.matches.is_empty()
        {
            let n = l.matches.len();
            l.index = if down { (l.index + 1) % n } else { (l.index + n - 1) % n };
            l.keep_visible();
        }
    }

    /// Hides the list until the text changes (Escape).
    pub fn close_list(&mut self) {
        if let Some(l) = self.list.as_mut() {
            l.closed = true;
        }
    }

    pub fn is_formula(&self) -> bool {
        self.text.starts_with('=')
            || (self.text.starts_with('+') && self.text.len() > 1)
            || (self.text.starts_with('-') && self.text.len() > 1 && !self.text[1..].starts_with(|c: char| c.is_ascii_digit()))
    }

    fn byte_caret(&self) -> usize {
        self.text.char_indices().nth(self.caret).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    /// Point mode: the caret follows an operator, `(`, `,` or `=` in a formula, so clicking a
    /// cell or pressing an arrow inserts a reference.
    pub fn can_point(&self) -> bool {
        if !self.is_formula() {
            return false;
        }
        if self.point_at_caret() {
            return true;
        }
        let before = self.text.get(..self.byte_caret()).unwrap_or("");
        let last = before.trim_end().chars().last();
        matches!(last, Some('=' | '(' | ',' | '+' | '-' | '*' | '/' | '^' | '&' | '<' | '>' | ':' | ';' | '{'))
    }

    pub fn point_at_caret(&self) -> bool {
        self.point.is_some_and(|(start, end)| end == self.byte_caret() && self.text.get(start..end).is_some())
    }

    /// Inserts (or replaces the last pointed) reference text at the caret.
    pub fn insert_ref(&mut self, r: &str) {
        let caret_b = self.byte_caret();
        let (start, end) = match self.point {
            Some((s, e)) if e == caret_b => (s, e),
            _ => (caret_b, caret_b),
        };
        let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(end..)) else { return };
        self.text = format!("{a}{r}{b}");
        let new_end = start + r.len();
        self.point = Some((start, new_end));
        self.caret = self.text.get(..new_end).map(|s| s.chars().count()).unwrap_or(0);
        self.caret_to_end = false;
        self.sync_caret = true;
    }

    /// Add another argument after the current pointed reference. Keep only the new reference
    /// replaceable, so a subsequent plain click does not remove earlier arguments or the separator.
    pub fn append_ref(&mut self, r: &str) {
        let separator = if self.point_at_caret() { self.locale.list_separator().to_string() } else { String::new() };
        self.point = None;
        self.insert_ref(&format!("{separator}{r}"));
        if let Some((start, end)) = self.point {
            self.point = Some((start + separator.len(), end));
        }
    }

    /// Updates function autocomplete for the word before the caret.
    pub fn update_autocomplete(&mut self, names: &[String]) {
        self.autocomplete.clear();
        if !self.is_formula() {
            return;
        }
        let before = self.text.get(..self.byte_caret()).unwrap_or("");
        let word: String =
            before.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_').collect::<String>().chars().rev().collect();
        if word.is_empty() || word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return;
        }
        let u = word.to_ascii_uppercase();
        // Not after a column letter of a reference like A1 (digits follow) – fine either way.
        self.autocomplete = names.iter().filter(|n| n.starts_with(&u)).take(12).cloned().collect();
        if self.autocomplete.len() == 1 && self.autocomplete[0] == u {
            self.autocomplete.clear();
        }
        self.ac_index = self.ac_index.min(self.autocomplete.len().saturating_sub(1));
    }

    /// Accepts the highlighted autocomplete entry: replaces the word and adds `(`.
    pub fn accept_autocomplete(&mut self) -> bool {
        let Some(name) = self.autocomplete.get(self.ac_index).cloned() else { return false };
        let caret_b = self.byte_caret();
        let before = self.text.get(..caret_b).unwrap_or("");
        let wlen: usize = before.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_').map(char::len_utf8).sum();
        let start = caret_b - wlen;
        let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(caret_b..)) else { return false };
        self.text = format!("{a}{name}({b}");
        self.caret = self.text.get(..start + name.len() + 1).map(|s| s.chars().count()).unwrap_or(0);
        self.autocomplete.clear();
        self.caret_to_end = false;
        self.sync_caret = true;
        true
    }

    /// Ctrl+Shift+A, as in Excel: right after a function name, or after its `(`, inserts the
    /// argument names (optional ones without their brackets) and the closing parenthesis, and
    /// the `(` too when missing. Anywhere else nothing changes and it returns false.
    pub fn insert_argument_names(&mut self, language: crate::i18n::Language) -> bool {
        if !self.is_formula() {
            return false;
        }
        let caret_b = self.byte_caret();
        let (Some(before), Some(after)) = (self.text.get(..caret_b), self.text.get(caret_b..)) else { return false };
        // An odd number of quotes before the caret: it is inside a text literal.
        if before.matches('"').count() % 2 == 1 {
            return false;
        }
        let open = before.ends_with('(');
        if !open && after.starts_with('(') {
            return false;
        }
        let head = before.strip_suffix('(').unwrap_or(before);
        let name: String =
            head.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_').collect::<String>().chars().rev().collect();
        if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
            return false;
        }
        let Some(signature) = crate::formula_locale::signature(&name, self.locale, language) else { return false };
        let sep = self.locale.list_separator();
        let inner = signature.split_once('(').map(|(_, rest)| rest.trim_end().trim_end_matches(')')).unwrap_or("");
        let args: Vec<&str> = inner.split(sep).map(|a| a.trim().trim_start_matches('[').trim_end_matches(']')).filter(|a| !a.is_empty()).collect();
        let close = if open && after.starts_with(')') { "" } else { ")" };
        let inserted = format!("{}{}{close}", if open { "" } else { "(" }, args.join(&sep.to_string()));
        self.text = format!("{before}{inserted}{after}");
        self.caret += inserted.chars().count();
        self.autocomplete.clear();
        self.completion = None;
        self.caret_to_end = false;
        self.sync_caret = true;
        true
    }

    /// The function call the caret is inside and the argument index (for the hint tooltip).
    pub fn current_function(&self) -> Option<(String, usize)> {
        if !self.is_formula() {
            return None;
        }
        let before = self.text.get(..self.byte_caret())?;
        let mut depth = 0i32;
        let mut arg = 0usize;
        let chars: Vec<char> = before.chars().collect();
        let mut in_str = false;
        let mut i = chars.len();
        while i > 0 {
            i -= 1;
            let c = chars[i];
            if c == '"' {
                in_str = !in_str;
                continue;
            }
            if in_str {
                continue;
            }
            match c {
                ')' | '}' | ']' => depth += 1,
                '{' | '[' => depth -= 1,
                '(' => {
                    if depth == 0 {
                        let name: String = chars[..i]
                            .iter()
                            .rev()
                            .take_while(|c| c.is_ascii_alphanumeric() || **c == '.' || **c == '_')
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect();
                        if name.is_empty() {
                            return None;
                        }
                        return Some((name.to_ascii_uppercase(), arg));
                    }
                    depth -= 1;
                }
                c if c == self.locale.list_separator() && depth == 0 => arg += 1,
                _ => {}
            }
        }
        None
    }
}

/// References in formula text with their byte spans and colour index (same reference text ⇒
/// same colour), for colouring the editor and outlining ranges on the grid.
pub fn formula_refs(text: &str) -> Vec<(usize, usize, RangeRef, Option<String>, usize)> {
    let body = text.strip_prefix('=').or_else(|| text.strip_prefix('+')).unwrap_or(text);
    let offset = text.len() - body.len();
    let Ok(toks) = gridcraft_engine::formula::lexer::tokenize(body) else { return vec![] };
    let mut out: Vec<(usize, usize, RangeRef, Option<String>, usize)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        use gridcraft_engine::formula::lexer::Tok;
        let (sheet, start, j) = match &toks[i].tok {
            Tok::Sheet(a, _) => (Some(a.clone()), toks[i].start, i + 1),
            _ => (None, toks[i].start, i),
        };
        if let Some(Tok::Word(w)) = toks.get(j).map(|t| &t.tok) {
            let range_end = if matches!(toks.get(j + 1).map(|t| &t.tok), Some(Tok::Colon | Tok::Trim(_))) {
                toks.get(j + 2).and_then(|token| {
                    let end = match &token.tok {
                        Tok::Word(w) => Some(w.clone()),
                        Tok::Number(n) if n.fract() == 0.0 && *n >= 1.0 => Some(format!("{n}")),
                        _ => None,
                    }?;
                    RangeRef::parse(&format!("{w}:{end}")).map(|range| (range, token.end))
                })
            } else {
                None
            };
            let (range, end) = if let Some(range_end) = range_end {
                i = j + 3;
                range_end
            } else if let Some(c) = CellRef::parse(w) {
                i = j + 1;
                (RangeRef::cell(c), toks[j].end)
            } else {
                i += 1;
                continue;
            };
            let key = body.get(start..end).unwrap_or("").replace('$', "").to_ascii_uppercase();
            let idx = match seen.iter().position(|k| *k == key) {
                Some(k) => k,
                None => {
                    seen.push(key);
                    seen.len() - 1
                }
            };
            out.push((start + offset, end + offset, range, sheet, idx % REF_COLORS.len()));
            continue;
        }
        i += 1;
    }
    out
}

/// Whether a reference of the formula being edited belongs on the sheet shown: a reference with a
/// sheet name is on that sheet, one without is on the sheet the edit started on, even while
/// pointing at another sheet.
pub fn ref_is_on_sheet(ref_sheet: Option<&str>, origin: &str, shown: &str) -> bool {
    ref_sheet.unwrap_or(origin).eq_ignore_ascii_case(shown)
}

/// Excel-style AutoComplete: the one distinct text entry in the same column (contiguous data
/// above and below) that starts with `typed` (case-insensitive). `None` when ambiguous.
pub fn column_completion(sheet: &gridcraft_engine::model::Sheet, at: CellRef, typed: &str) -> Option<String> {
    if typed.is_empty() || typed.starts_with('=') || typed.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
        return None;
    }
    let lower = typed.to_lowercase();
    let mut found: Option<String> = None;
    let mut ambiguous = false;
    let mut scan = |row: u32| -> bool {
        match sheet.value(CellRef::new(row, at.col)) {
            gridcraft_engine::core::Value::Empty => false,
            gridcraft_engine::core::Value::Text(t) => {
                let folded = t.to_lowercase();
                if folded.starts_with(&lower) && folded.len() > lower.len() {
                    match &found {
                        Some(f) if f.to_lowercase() != folded => ambiguous = true,
                        None => found = Some(t.to_string()),
                        _ => {}
                    }
                }
                true
            }
            _ => true,
        }
    };
    let mut r = at.row;
    let mut n = 0;
    while r > 0 && n < 5000 {
        r -= 1;
        n += 1;
        if !scan(r) {
            break;
        }
    }
    let mut r = at.row + 1;
    n = 0;
    while n < 5000 && scan(r) {
        r += 1;
        n += 1;
    }
    if ambiguous { None } else { found }
}

/// The part of `full` after the `typed` prefix, counted in characters: case folding can change
/// a character's byte length ("ß" is two bytes, "ẞ" three).
pub fn completion_rest<'a>(full: &'a str, typed: &str) -> Option<&'a str> {
    let start = full.char_indices().nth(typed.chars().count())?.0;
    full.get(start..)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_mode() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "=SUM(".into(), true, false);
        assert!(e.can_point());
        e.insert_ref("A1");
        assert_eq!(e.text, "=SUM(A1");
        e.insert_ref("A2");
        assert_eq!(e.text, "=SUM(A2");
        e.text.push(':');
        e.caret += 1;
        e.point = None;
        assert!(e.can_point());
        let e2 = EditState::new(0, CellRef::new(0, 0), "hello".into(), true, false);
        assert!(!e2.can_point());
    }

    #[test]
    fn autocomplete_and_hints() {
        let names: Vec<String> = ["SUM", "SUMIF", "SUMPRODUCT", "AVERAGE"].iter().map(|s| s.to_string()).collect();
        let mut e = EditState::new(0, CellRef::new(0, 0), "=su".into(), true, false);
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete.len(), 3);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=SUM(");
        let e = EditState::new(0, CellRef::new(0, 0), "=IF(A1>0,SUM(B1,".into(), true, false);
        assert_eq!(e.current_function(), Some(("SUM".into(), 1)));
    }

    #[test]
    fn ref_spans() {
        let r = formula_refs("=A1+B2:C3*a1+Sheet2!D4");
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].4, r[2].4); // A1 and a1 share a colour
        assert_eq!(r[1].2.a1(), "B2:C3");
        assert_eq!(r[3].3.as_deref(), Some("Sheet2"));
    }

    #[test]
    fn refs_are_outlined_on_their_own_sheet_only() {
        // Pointing at Sheet2 while the edit started on Sheet1: `A1` stays on Sheet1, `Sheet2!B2` shows on Sheet2.
        assert!(ref_is_on_sheet(None, "Sheet1", "Sheet1"));
        assert!(!ref_is_on_sheet(None, "Sheet1", "Sheet2"));
        assert!(ref_is_on_sheet(Some("Sheet2"), "Sheet1", "Sheet2"));
        assert!(!ref_is_on_sheet(Some("Sheet2"), "Sheet1", "Sheet1"));
        assert!(ref_is_on_sheet(Some("sheet2"), "Sheet1", "Sheet2"), "sheet names compare ignoring case");
    }

    fn cities() -> Vec<String> {
        ["Dubai - United Arab Emirates", "Paris - France", "London - United Kingdom", "Boston - United States", "Los Angeles - United States"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn typed(text: &str) -> EditState {
        let mut e = EditState::new(0, CellRef::new(0, 0), text.into(), true, false);
        e.set_list_items(cities(), true);
        e
    }

    #[test]
    fn list_search_filters_as_typed_and_picks_the_first_match() {
        let mut e = typed("u");
        assert!(e.list_open());
        e.text = "United".into();
        e.update_list();
        assert_eq!(e.list.as_ref().map(|l| l.matches.clone()), Some(vec![0, 2, 3, 4]));
        assert_eq!(e.list_choice(), Some("Dubai - United Arab Emirates"));
        e.text = "united st".into();
        e.update_list();
        assert_eq!(e.list_choice(), Some("Boston - United States"));
        e.list_step(true);
        assert_eq!(e.list_choice(), Some("Los Angeles - United States"));
        e.list_step(true);
        assert_eq!(e.list_choice(), Some("Boston - United States"), "the highlight wraps");
        e.list_step(false);
        assert_eq!(e.list_choice(), Some("Los Angeles - United States"));
    }

    #[test]
    fn list_search_hides_without_matches_formulas_or_text() {
        assert!(!typed("Tokyo").list_open());
        assert!(!typed("=A1").list_open());
        assert!(!typed("  ").list_open());
        assert_eq!(typed("Tokyo").list_choice(), None);
    }

    #[test]
    fn list_search_escape_closes_until_the_text_changes() {
        let mut e = typed("Par");
        e.close_list();
        assert!(!e.list_open());
        assert_eq!(e.list_choice(), None, "a closed list commits the typed text");
        e.text = "Pari".into();
        e.update_list();
        assert_eq!(e.list_choice(), Some("Paris - France"));
    }

    #[test]
    fn list_search_prefers_an_exact_entry() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "paris".into(), true, false);
        e.set_list_items(vec!["Paris - France".into(), "Paris".into()], true);
        assert_eq!(e.list_choice(), Some("Paris"));
    }

    #[test]
    fn list_search_waits_for_a_change_when_editing_an_existing_value() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "Paris - France".into(), false, false);
        e.set_list_items(cities(), false);
        assert!(!e.list_open(), "F2 doesn't pop the list up");
        e.text = "Paris".into();
        e.update_list();
        assert!(e.list_open());
    }

    #[test]
    fn list_search_keeps_the_highlight_in_view() {
        let items: Vec<String> = (0..20).map(|i| format!("Item {i}")).collect();
        let mut e = EditState::new(0, CellRef::new(0, 0), "item".into(), true, false);
        e.set_list_items(items, true);
        for _ in 0..10 {
            e.list_step(true);
        }
        let l = e.list.as_ref().map(|l| (l.index, l.top)).unwrap_or_default();
        assert_eq!(l, (10, 10 + 1 - ListSearch::VISIBLE));
        e.list_step(false);
        e.list_step(false);
        e.list_step(false);
        e.list_step(false);
        e.list_step(false);
        e.list_step(false);
        e.list_step(false);
        let l = e.list.as_ref().map(|l| (l.index, l.top)).unwrap_or_default();
        assert_eq!(l, (3, 3));
    }

    #[test]
    fn list_search_keeps_list_order_when_the_exact_entry_is_past_the_cap() {
        let cap = gridcraft_engine::cmd::data::LIST_MATCH_CAP;
        let mut items: Vec<String> = (0..cap + 5).map(|i| format!("Item {i}")).collect();
        items.push("Item".into());
        let mut e = EditState::new(0, CellRef::new(0, 0), "item".into(), true, false);
        e.set_list_items(items, true);
        let (matches, index) = e.list.as_ref().map(|l| (l.matches.clone(), l.index)).unwrap_or_default();
        assert_eq!(matches, (0..cap).collect::<Vec<_>>(), "the first `cap` matches, in list order");
        assert_eq!(index, 0, "the first match stays highlighted");
        assert_eq!(e.list_choice(), Some("Item 0"));
    }

    #[test]
    fn list_search_highlights_an_exact_entry_within_the_cap() {
        let items: Vec<String> = (0..20).map(|i| format!("Item {i}")).chain(["Item".to_string()]).collect();
        let mut e = EditState::new(0, CellRef::new(0, 0), "item".into(), true, false);
        e.set_list_items(items, true);
        let (index, top) = e.list.as_ref().map(|l| (l.index, l.top)).unwrap_or_default();
        assert_eq!(index, 20);
        assert_eq!(top, 21 - ListSearch::VISIBLE, "scrolled into view");
        assert_eq!(e.list_choice(), Some("Item"));
    }

    #[test]
    fn whole_row_and_column_ref_spans() {
        let refs = formula_refs("=SUM(C:C,4:6,$D:$E,Sheet2!7:8)");
        assert_eq!(refs.len(), 4);
        assert_eq!(refs[0].2.a1(), "C:C");
        assert_eq!(refs[1].2.a1(), "4:6");
        assert_eq!(refs[2].2.a1(), "D:E");
        assert_eq!(refs[3].2.a1(), "7:8");
        assert_eq!(refs[3].3.as_deref(), Some("Sheet2"));
    }

    #[test]
    fn inserting_a_union_keeps_each_reference_separately_pointed() {
        let mut edit = EditState::new(0, CellRef::new(0, 0), "=SUM(".into(), true, false);
        edit.insert_ref("C:C");
        edit.append_ref("E:E");
        assert_eq!(edit.text, "=SUM(C:C,E:E");
        assert_eq!(formula_refs(&edit.text).iter().map(|r| r.2.a1()).collect::<Vec<_>>(), ["C:C", "E:E"]);
        assert_eq!(edit.point, Some((9, 12)));
        assert!(edit.can_point());
    }
}
