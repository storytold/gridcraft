//! In-cell / formula-bar editing state: point mode, reference colouring, function
//! autocomplete and argument hints.

use gridcraft_engine::core::{CellRef, RangeRef};

/// How many distinct reference colours a formula cycles through (`Tokens::ref_colors`).
pub const REF_COLOR_COUNT: usize = 8;

#[derive(Clone, Debug)]
pub struct EditState {
    /// The workbook (`DocState::uid`) the draft belongs to: it is drawn and committed only there.
    pub doc: u64,
    /// The window it was typed in: switching to another window, even on the same workbook,
    /// commits it there first (`None` until the app sets it).
    pub window: Option<u64>,
    /// The sheet's index when the draft was last resolved; [`EditState::sheet_in`] finds it again
    /// by `sheet_uid` after sheets are inserted, deleted or moved.
    pub sheet: usize,
    /// The sheet's identity while the app runs (`model::SheetUid`), recorded when the draft is
    /// created ([`EditState::on_sheet`]); 0 (never given out) when none was.
    pub sheet_uid: u64,
    /// The draft began on a Python cell, with its code as the text. Kept so the code stays out of
    /// view once the cell can't be looked at, its sheet gone.
    pub py_cell: bool,
    /// The order of the workbook's sheets ([`sheet_order`]) when the draft was last resolved: a
    /// change after a command points the draft at its sheet again and lets go of
    /// `point_anchor`, which holds a sheet index.
    pub sheet_order: u64,
    /// Syntax of this edit, captured when it starts (prevents a language switch reinterpreting it).
    pub locale: gridcraft_engine::formula::FormulaLocale,
    pub cell: CellRef,
    pub text: String,
    /// Started by typing (Enter mode: arrows commit) rather than F2 (Edit mode: arrows move the caret).
    pub enter_mode: bool,
    pub from_formula_bar: bool,
    /// Caret position (char index), mirrored from the text widget.
    pub caret: usize,
    /// The other end of the selected text (char index), mirrored like `caret`; equal to `caret`
    /// when nothing is selected.
    pub anchor: usize,
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
    /// A dialog opened over the edit has the keyboard: when the editor takes it back without a
    /// change to the caret, the selected text (`anchor`..`caret`) stays selected.
    pub keep_selection: bool,
    /// Column AutoComplete: the full entry the typed text completes to.
    pub completion: Option<String>,
    /// Move the caret to the end on the next frame.
    pub caret_to_end: bool,
    /// The cell's text when an F2-style edit began. Committing it unchanged leaves the cell alone
    /// (no undo step, the workbook stays clean). `None` for an edit started by typing.
    pub original: Option<String>,
    /// Python mode: the text is a Python cell's code, edited in the formula bar's code editor and
    /// committed with Ctrl+Enter (Cmd+Enter on macOS) through `python.set`.
    pub python: bool,
}

/// A digest of the order of `wb`'s sheets by identity: it changes when a sheet is inserted,
/// deleted or moved, not when one is renamed or edited.
pub fn sheet_order(wb: &gridcraft_engine::model::Workbook) -> u64 {
    wb.sheets.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, sh| (h ^ sh.uid.get()).wrapping_mul(0x0000_0100_0000_01b3))
}

impl EditState {
    pub fn new(doc: u64, sheet: usize, cell: CellRef, text: String, enter_mode: bool, from_formula_bar: bool) -> EditState {
        let caret = text.chars().count();
        let original = (!enter_mode).then(|| text.clone());
        EditState {
            doc,
            window: None,
            sheet,
            sheet_uid: 0,
            py_cell: false,
            sheet_order: 0,
            locale: Default::default(),
            cell,
            text,
            enter_mode,
            from_formula_bar,
            caret,
            anchor: caret,
            point: None,
            point_cell: None,
            point_anchor: None,
            autocomplete: vec![],
            ac_index: 0,
            request_focus: true,
            sync_caret: true,
            keep_selection: false,
            caret_to_end: true,
            completion: None,
            original,
            python: false,
        }
    }

    /// An edit of a cell's Python code, in the formula bar.
    pub fn python(doc: u64, sheet: usize, cell: CellRef, code: String) -> EditState {
        let mut ed = EditState::new(doc, sheet, cell, code, false, true);
        ed.python = true;
        ed
    }

    /// Records the identity of the draft's sheet, from the workbook it is typed in.
    pub fn on_sheet(mut self, wb: &gridcraft_engine::model::Workbook) -> EditState {
        self.sheet_uid = wb.sheet(self.sheet).map_or(0, |sh| sh.uid.get());
        self.sheet_order = sheet_order(wb);
        self
    }

    /// The draft's sheet's index now, found by its identity, since commands insert, delete and
    /// move sheets under an open draft; `None` once the sheet is gone (or none was recorded).
    pub fn sheet_in(&self, wb: &gridcraft_engine::model::Workbook) -> Option<usize> {
        if self.sheet_uid == 0 {
            return None;
        }
        if wb.sheet(self.sheet).is_some_and(|sh| sh.uid.get() == self.sheet_uid) {
            return Some(self.sheet);
        }
        wb.sheets.iter().position(|sh| sh.uid.get() == self.sheet_uid)
    }

    /// Whether the text is the `=PY` that Tab turns into Python mode, as in Excel (`=PY(` too,
    /// in any letter case).
    pub fn is_py_trigger(&self) -> bool {
        let t = self.text.trim();
        !self.python && (t.eq_ignore_ascii_case("=PY") || t.eq_ignore_ascii_case("=PY("))
    }

    /// Switches the edit to Python mode, in the formula bar. The `=PY` trigger becomes empty code;
    /// other text is kept as the code, so the shortcut pressed mid-edit loses nothing.
    pub fn enter_python(&mut self) {
        if self.is_py_trigger() {
            self.text.clear();
        }
        self.python = true;
        self.from_formula_bar = true;
        self.enter_mode = false;
        self.point = None;
        self.point_cell = None;
        self.point_anchor = None;
        self.autocomplete.clear();
        self.completion = None;
        self.caret = self.text.chars().count();
        self.anchor = self.caret;
        self.caret_to_end = false;
        self.request_focus = true;
        self.sync_caret = true;
    }

    /// Leaves Python mode: the edit goes on as an ordinary entry of the same text.
    pub fn leave_python(&mut self) {
        self.python = false;
        self.caret = self.text.chars().count();
        self.anchor = self.caret;
        self.request_focus = true;
        self.sync_caret = true;
    }

    /// Whether the text is a formula (never in Python mode: there it is code).
    pub fn is_formula(&self) -> bool {
        !self.python
            && (self.text.starts_with('=')
                || (self.text.starts_with('+') && self.text.len() > 1)
                || (self.text.starts_with('-') && self.text.len() > 1 && !self.text[1..].starts_with(|c: char| c.is_ascii_digit())))
    }

    fn byte_caret(&self) -> usize {
        self.text.char_indices().nth(self.caret).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    /// Point mode: the caret follows an operator, `(`, `,` or `=` in a formula, so clicking a
    /// cell or pressing an arrow inserts a reference. In Python mode the caret may also follow
    /// whitespace or start a line, outside a string or comment, and the reference goes in as an
    /// `xl("…")` call.
    pub fn can_point(&self) -> bool {
        if self.python {
            return self.point_at_caret() || self.text.get(..self.byte_caret()).is_some_and(python_can_point);
        }
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

    /// Inserts (or replaces the last pointed) reference text at the caret. In Python mode the
    /// reference goes in as an `xl("…")` call ([`python_xl`]).
    pub fn insert_ref(&mut self, r: &str) {
        if self.python {
            self.insert_pointed(&python_xl(r));
        } else {
            self.insert_pointed(r);
        }
    }

    /// Puts `r` at the caret, in place of the pointed text there, as the new pointed text.
    fn insert_pointed(&mut self, r: &str) {
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

    /// Inserts `t` at the caret as typed text (Paste Names), in place of the selected text: the
    /// caret ends after it, and point mode starts afresh.
    pub fn insert_at_caret(&mut self, t: &str) {
        // When code moved the caret this frame (`sync_caret`), the widget has not yet stored it, so `anchor` is out of
        // date and the name goes in at the caret.
        let (from, to) = if self.sync_caret { (self.caret, self.caret) } else { (self.anchor.min(self.caret), self.anchor.max(self.caret)) };
        let byte = |c: usize| self.text.char_indices().nth(c).map(|(i, _)| i).unwrap_or(self.text.len());
        let (Some(a), Some(b)) = (self.text.get(..byte(from)), self.text.get(byte(to)..)) else { return };
        self.text = format!("{a}{t}{b}");
        self.point = None;
        self.caret = from + t.chars().count();
        self.anchor = self.caret;
        // A function suggestion shown before the name went in no longer fits the text.
        self.autocomplete.clear();
        self.ac_index = 0;
        self.completion = None;
        self.caret_to_end = false;
        self.sync_caret = true;
        self.request_focus = true;
    }

    /// Add another argument after the current pointed reference. Keep only the new reference
    /// replaceable, so a subsequent plain click does not remove earlier arguments or the separator.
    /// In Python mode the separator is Python's `, `, whatever the formula locale.
    pub fn append_ref(&mut self, r: &str) {
        let separator = match (self.point_at_caret(), self.python) {
            (false, _) => String::new(),
            (true, true) => ", ".to_string(),
            (true, false) => self.locale.list_separator().to_string(),
        };
        self.point = None;
        let r = if self.python { python_xl(r) } else { r.to_string() };
        self.insert_pointed(&format!("{separator}{r}"));
        if let Some((start, end)) = self.point {
            self.point = Some((start + separator.len(), end));
        }
    }

    /// The reference text this pointing session inserted, without Python mode's `xl("…")`.
    pub fn pointed_text(&self) -> Option<String> {
        let (start, end) = self.point?;
        let text = self.text.get(start..end)?;
        if self.python { python_xl_reference(text) } else { Some(text.to_string()) }
    }

    /// The range and sheet name of the reference this pointing session inserted.
    pub fn pointed_range(&self) -> Option<(RangeRef, Option<String>)> {
        let (start, end) = self.point?;
        if self.python {
            let text = self.pointed_text()?;
            formula_refs(&text).into_iter().find(|(s, e, ..)| (*s, *e) == (0, text.len())).map(|(_, _, range, sheet, _)| (range, sheet))
        } else {
            formula_refs(&self.text).into_iter().find(|(s, e, ..)| (*s, *e) == (start, end)).map(|(_, _, range, sheet, _)| (range, sheet))
        }
    }

    /// Ends the pointing session: the next click or arrow inserts a new reference, and arrows
    /// start again from the edited cell.
    pub fn end_pointing(&mut self) {
        self.point = None;
        self.point_anchor = None;
        self.point_cell = None;
    }

    /// Updates function autocomplete for the word before the caret.
    pub fn update_autocomplete(&mut self, names: &[String]) {
        self.autocomplete.clear();
        if self.python || !self.is_formula() {
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
        if self.python || !self.is_formula() {
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

/// A reference as Python mode inserts it: a call of `xl` with the reference as a Python string
/// literal (`xl("'My Sheet'!A1:B3")`). A `"` or `\` in a sheet name is escaped, and so is a
/// control character, so the literal stays on one line and the code stays valid Python.
pub fn python_xl(reference: &str) -> String {
    let mut out = String::with_capacity(reference.len() + 6);
    out.push_str("xl(\"");
    for c in reference.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // `char::is_control` is Unicode's Cc category, all below U+0100.
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push_str("\")");
    out
}

/// The reference inside an `xl("…")` call [`python_xl`] wrote; `None` for other text.
pub fn python_xl_reference(call: &str) -> Option<String> {
    let inner = call.strip_prefix("xl(\"")?.strip_suffix("\")")?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'x' => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// Whether Python code ending in `before` leaves room for a reference at its end: after `(`, `,`,
/// `=`, an operator or whitespace, or at the start, and never inside a string or a comment, where
/// the `xl("…")` call would break the code.
fn python_can_point(before: &str) -> bool {
    let Some(depth) = python_code_depth(before) else { return false };
    let code = before.trim_end();
    let Some(last) = code.chars().last() else { return true };
    let space = before.get(code.len()..).unwrap_or_default();
    // A new line outside brackets starts a statement (the last one is the cell's result).
    if depth == 0 && space.contains('\n') {
        return true;
    }
    if python_expects_value_after(last) {
        return true;
    }
    // After a completed operand (`value `, `f(a)`) the next token can't be a value: a reference
    // there would make invalid code. Only a keyword that takes a value (`return `, `x if c else `)
    // lets one follow.
    !space.is_empty() && PY_VALUE_KEYWORDS.contains(&python_trailing_word(code))
}

/// Python keywords that a value follows.
const PY_VALUE_KEYWORDS: &[&str] = &["return", "yield", "in", "not", "and", "or", "if", "elif", "else", "while", "assert", "raise", "is", "await"];

/// Whether a value can follow `c`: an opening bracket, a separator or an operator.
fn python_expects_value_after(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | ',' | '=' | '+' | '-' | '*' | '/' | '%' | '@' | '&' | '|' | '^' | '~' | '<' | '>' | ':' | ';')
}

/// The identifier or keyword `code` ends with ("" when it ends with something else).
fn python_trailing_word(code: &str) -> &str {
    let start = code.char_indices().rev().take_while(|(_, c)| c.is_alphanumeric() || *c == '_').last().map_or(code.len(), |(i, _)| i);
    code.get(start..).unwrap_or_default()
}

/// How many brackets are open at the end of the Python code `before`, or `None` when it ends
/// inside a string literal (single, double or triple quoted) or a comment.
fn python_code_depth(before: &str) -> Option<usize> {
    #[derive(PartialEq)]
    enum At {
        Code,
        Comment,
        Text { quote: char, triple: bool },
    }
    let chars: Vec<char> = before.chars().collect();
    let mut at = At::Code;
    let mut depth = 0usize;
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let tripled = |q: char| chars.get(i + 1) == Some(&q) && chars.get(i + 2) == Some(&q);
        match at {
            At::Code if c == '#' => at = At::Comment,
            At::Code if matches!(c, '(' | '[' | '{') => depth = depth.saturating_add(1),
            At::Code if matches!(c, ')' | ']' | '}') => depth = depth.saturating_sub(1),
            At::Code if c == '"' || c == '\'' => {
                let triple = tripled(c);
                at = At::Text { quote: c, triple };
                if triple {
                    i += 2;
                }
            }
            At::Comment if c == '\n' => at = At::Code,
            At::Text { .. } if c == '\\' => i += 1,
            At::Text { triple: false, .. } if c == '\n' => at = At::Code,
            At::Text { quote, triple } if c == quote && (!triple || tripled(c)) => {
                at = At::Code;
                if triple {
                    i += 2;
                }
            }
            _ => {}
        }
        i += 1;
    }
    (at == At::Code).then_some(depth)
}

/// References in formula text with their byte spans and colour index (same reference text ⇒
/// same colour), for colouring the editor and outlining ranges on the grid.
pub fn formula_refs(text: &str) -> Vec<(usize, usize, RangeRef, Option<String>, usize)> {
    let body = text.strip_prefix('=').or_else(|| text.strip_prefix('+')).unwrap_or(text);
    let offset = text.len() - body.len();
    let Ok(toks) = gridcraft_engine::formula::lexer::tokenize(body) else { return vec![] };
    let mut out: Vec<(usize, usize, RangeRef, Option<String>, usize)> = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
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
            let next = seen.len();
            let idx = *seen.entry(key).or_insert(next);
            out.push((start + offset, end + offset, range, sheet, idx % REF_COLOR_COUNT));
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
        let mut e = EditState::new(1, 0, CellRef::new(0, 0), "=SUM(".into(), true, false);
        assert!(e.can_point());
        e.insert_ref("A1");
        assert_eq!(e.text, "=SUM(A1");
        e.insert_ref("A2");
        assert_eq!(e.text, "=SUM(A2");
        e.text.push(':');
        e.caret += 1;
        e.point = None;
        assert!(e.can_point());
        let e2 = EditState::new(1, 0, CellRef::new(0, 0), "hello".into(), true, false);
        assert!(!e2.can_point());
    }

    #[test]
    fn a_pasted_name_replaces_the_selected_text() {
        let mut e = EditState::new(1, 0, CellRef::new(0, 0), "=OldName+1".into(), false, false);
        e.sync_caret = false;
        (e.anchor, e.caret) = (8, 1);
        e.autocomplete = vec!["SUM".into()];
        e.insert_at_caret("Sales");
        assert_eq!((e.text.as_str(), e.caret, e.anchor), ("=Sales+1", 6, 6));
        assert!(e.autocomplete.is_empty(), "the earlier function suggestion is gone");
        // Nothing selected: the name goes in at the caret.
        e.sync_caret = false;
        e.insert_at_caret("Rate");
        assert_eq!((e.text.as_str(), e.caret), ("=SalesRate+1", 10));
    }

    #[test]
    fn autocomplete_and_hints() {
        let names: Vec<String> = ["SUM", "SUMIF", "SUMPRODUCT", "AVERAGE"].iter().map(|s| s.to_string()).collect();
        let mut e = EditState::new(1, 0, CellRef::new(0, 0), "=su".into(), true, false);
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete.len(), 3);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=SUM(");
        let e = EditState::new(1, 0, CellRef::new(0, 0), "=IF(A1>0,SUM(B1,".into(), true, false);
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
        let mut edit = EditState::new(1, 0, CellRef::new(0, 0), "=SUM(".into(), true, false);
        edit.insert_ref("C:C");
        edit.append_ref("E:E");
        assert_eq!(edit.text, "=SUM(C:C,E:E");
        assert_eq!(formula_refs(&edit.text).iter().map(|r| r.2.a1()).collect::<Vec<_>>(), ["C:C", "E:E"]);
        assert_eq!(edit.point, Some((9, 12)));
        assert!(edit.can_point());
    }

    fn python_edit(code: &str) -> EditState {
        EditState::python(1, 0, CellRef::new(0, 0), code.into())
    }

    #[test]
    fn python_mode_points_after_an_operator_whitespace_or_a_line_start() {
        for code in ["", "df = ", "f(", "f(a,", "x =", "[", "a +", "x = 1\n", "y = f(1,\n    ", "return ", "x if c else ", "a and ", "y = 1\n  "] {
            assert!(python_edit(code).can_point(), "{code:?}");
        }
        for code in [
            "abc",
            "x = 1",
            "f(a)",
            "df.",
            "x = 'a ",
            "s = \"(",
            "# note ",
            "'''doc\n",
            "x = 1  # see ",
            "value ",
            "x = 1 ",
            "f(a) ",
            "f(a\n    ",
            "return",
            "returns ",
        ] {
            assert!(!python_edit(code).can_point(), "{code:?}");
        }
        assert!(python_edit("s = 'it''s' + ").can_point(), "after two strings the code goes on");
        assert!(python_edit("s = '''a'''\n").can_point(), "after a closed triple-quoted string");
        assert!(!python_edit("s = 'a\\' + ").can_point(), "an escaped quote doesn't close the string");
        assert!(python_edit("# note\n").can_point(), "a comment ends with its line");
    }

    #[test]
    fn python_mode_inserts_xl_calls_and_replaces_the_pointed_one() {
        let mut e = python_edit("df = ");
        e.insert_ref("B2");
        assert_eq!(e.text, "df = xl(\"B2\")");
        assert!(e.can_point(), "right after its own reference");
        e.insert_ref("B2:C4");
        assert_eq!(e.text, "df = xl(\"B2:C4\")", "the session's reference is replaced, not stacked");
        assert_eq!(e.pointed_text().as_deref(), Some("B2:C4"));
        assert_eq!(e.pointed_range().map(|(r, sheet)| (r.a1(), sheet)), Some(("B2:C4".to_string(), None)));
        e.append_ref("E5");
        assert_eq!(e.text, "df = xl(\"B2:C4\"), xl(\"E5\")", "Ctrl+click adds a Python argument");
        assert_eq!(e.pointed_text().as_deref(), Some("E5"));
        e.end_pointing();
        assert!(!e.can_point(), "after `)` with the session over");
        // A locale whose formulas separate arguments with `;` still writes Python's `, `.
        let mut f = python_edit("f(1, ");
        f.locale = gridcraft_engine::formula::FormulaLocale::Es;
        f.insert_ref("A1");
        f.append_ref("B1");
        assert_eq!(f.text, "f(1, xl(\"A1\"), xl(\"B1\")");
    }

    #[test]
    fn python_xl_quotes_sheet_names_as_a_python_string() {
        assert_eq!(python_xl("A:A"), r#"xl("A:A")"#);
        assert_eq!(python_xl("1:3"), r#"xl("1:3")"#);
        assert_eq!(python_xl("'My Sheet'!A1"), r#"xl("'My Sheet'!A1")"#);
        assert_eq!(python_xl("'Bob''s'!A1"), r#"xl("'Bob''s'!A1")"#);
        assert_eq!(python_xl(r#"'Say "hi"'!A1"#), r#"xl("'Say \"hi\"'!A1")"#);
        assert_eq!(python_xl("'a\\b\nc\u{1}'!A1"), r#"xl("'a\\b\nc\x01'!A1")"#);
        for reference in ["B2:C4", "'My Sheet'!A1", "'Bob''s'!A1", r#"'Say "hi"'!A1"#, "'a\\b\nc\u{1}\u{85}'!A1"] {
            let call = python_xl(reference);
            assert_eq!(python_xl_reference(&call).as_deref(), Some(reference), "{reference:?}");
            // The literal stays on one line and closes where the call does.
            assert!(!call.contains('\n') && !call.contains('\r'), "{call:?}");
            assert!(python_can_point(&format!("{call} + ")), "{call:?}");
        }
        assert_eq!(python_xl_reference(r#"f("A1")"#), None);
        assert_eq!(python_xl_reference(r#"xl("A1\")"#), None);
        let mut e = python_edit("x = ");
        e.insert_ref(r#"'Say "hi"'!B2:C3"#);
        assert_eq!(e.pointed_range().map(|(r, sheet)| (r.a1(), sheet)), Some(("B2:C3".to_string(), Some(r#"Say "hi""#.to_string()))));
    }

    #[test]
    fn formula_point_mode_inserts_no_xl_call() {
        let mut e = EditState::new(1, 0, CellRef::new(0, 0), "=SUM(".into(), true, false);
        e.insert_ref("'My Sheet'!A1");
        assert_eq!(e.text, "=SUM('My Sheet'!A1");
        assert_eq!(e.pointed_text().as_deref(), Some("'My Sheet'!A1"));
        assert!(!EditState::new(1, 0, CellRef::new(0, 0), "x = ".into(), true, false).can_point(), "text that isn't a formula");
    }
}
