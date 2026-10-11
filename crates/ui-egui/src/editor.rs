//! In-cell / formula-bar editing state: point mode, reference colouring, function
//! autocomplete and argument hints.
//!
//! The text being edited is spelled in the session's formula language and region (`SOMA(1,5;A1)`
//! in pt-BR); [`EditState::locale`] carries the dialect that reads it.

use std::sync::Arc;

use egui::Color32;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_locale::{Dialect, Locale};

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

/// Whether `c` can be part of a function name being typed (local names have accents and dots).
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '.' || c == '_'
}

#[derive(Clone, Debug)]
pub struct EditState {
    pub sheet: usize,
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
    /// Function names (local spelling) the typed word completes to.
    pub autocomplete: Vec<String>,
    pub ac_index: usize,
    pub request_focus: bool,
    /// The text changed programmatically: push `caret` into the widget on the next frame.
    pub sync_caret: bool,
    /// Column AutoComplete: the full entry the typed text completes to.
    pub completion: Option<String>,
    /// Move the caret to the end on the next frame.
    pub caret_to_end: bool,
    /// Formula language and region the text is written in.
    pub locale: Arc<Locale>,
    /// Input method composition text, not part of `text` until the input method commits it.
    pub preedit: Option<String>,
}

impl EditState {
    pub fn new(sheet: usize, cell: CellRef, text: String, enter_mode: bool, from_formula_bar: bool, locale: Arc<Locale>) -> EditState {
        let caret = text.chars().count();
        EditState {
            sheet,
            cell,
            text,
            enter_mode,
            from_formula_bar,
            caret,
            point: None,
            point_cell: None,
            autocomplete: vec![],
            ac_index: 0,
            request_focus: true,
            sync_caret: true,
            caret_to_end: true,
            completion: None,
            locale,
            preedit: None,
        }
    }

    /// The dialect the text is written in.
    pub fn dialect(&self) -> Dialect<'_> {
        self.locale.dialect()
    }

    /// Moves the text to another formula language and region (the user switched while editing):
    /// a formula is read in the old dialect and written in the new one, with the caret at its
    /// end; plain text and formulas that do not parse yet stay as typed. `known` are the
    /// workbook names, which win over function names and booleans in both dialects.
    pub fn switch_locale(&mut self, new: Arc<Locale>, known: gridcraft_engine::formula::KnownNames<'_>) {
        use gridcraft_engine::locale::{from_local_formula, to_local_formula};
        if *self.locale != *new {
            let converted = self
                .text
                .starts_with('=')
                .then(|| from_local_formula(&self.text, &self.locale.dialect(), known).ok())
                .flatten()
                .map(|canonical| to_local_formula(&canonical, &new.dialect(), known));
            if let Some(text) = converted.filter(|t| *t != self.text) {
                self.text = text;
                self.caret = self.text.chars().count();
                self.sync_caret = true;
                self.point = None;
                self.autocomplete.clear();
            }
        }
        self.locale = new;
    }

    /// References in the text with their spans and colour indexes, read in the editor's dialect.
    pub fn refs(&self) -> Vec<(usize, usize, RangeRef, Option<String>, usize)> {
        formula_refs(&self.text, &self.dialect())
    }

    pub fn is_formula(&self) -> bool {
        let decimal = self.locale.regional.decimal;
        self.text.starts_with('=')
            || (self.text.starts_with('+') && self.text.len() > 1)
            || (self.text.starts_with('-') && self.text.len() > 1 && !self.text[1..].starts_with(|c: char| c.is_ascii_digit() || c == decimal))
    }

    fn byte_caret(&self) -> usize {
        self.text.char_indices().nth(self.caret).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    /// Point mode: the caret follows an operator, `(`, a separator or `=` in a formula, so clicking
    /// a cell or pressing an arrow inserts a reference.
    pub fn can_point(&self) -> bool {
        if !self.is_formula() {
            return false;
        }
        if let Some((_, e)) = self.point
            && e == self.byte_caret()
        {
            return true;
        }
        let before = self.text.get(..self.byte_caret()).unwrap_or("");
        let last = before.trim_end().chars().last();
        let r = &self.locale.regional;
        last.is_some_and(|c| {
            matches!(c, '=' | '(' | '+' | '-' | '*' | '/' | '^' | '&' | '<' | '>' | ':' | ';' | '{') || c == r.list || c == r.array_col
        })
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

    /// Updates function autocomplete for the word before the caret. `names` are the function names
    /// in the formula language (local spelling); only those are offered.
    pub fn update_autocomplete(&mut self, names: &[String]) {
        self.autocomplete.clear();
        if !self.is_formula() {
            return;
        }
        let before = self.text.get(..self.byte_caret()).unwrap_or("");
        if Scan::of(before, self.locale.regional.list).in_literal() {
            return;
        }
        let word: String = before.chars().rev().take_while(|c| is_name_char(*c)).collect::<String>().chars().rev().collect();
        if word.is_empty() || word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return;
        }
        let u = word.to_uppercase();
        // Not after a column letter of a reference like A1 (digits follow) – fine either way.
        self.autocomplete = names.iter().filter(|n| n.to_uppercase().starts_with(&u)).take(12).cloned().collect();
        if self.autocomplete.len() == 1 && self.autocomplete[0].to_uppercase() == u {
            self.autocomplete.clear();
        }
        self.ac_index = self.ac_index.min(self.autocomplete.len().saturating_sub(1));
    }

    /// Accepts the highlighted autocomplete entry: replaces the word and adds `(`.
    pub fn accept_autocomplete(&mut self) -> bool {
        let Some(name) = self.autocomplete.get(self.ac_index).cloned() else { return false };
        let caret_b = self.byte_caret();
        let before = self.text.get(..caret_b).unwrap_or("");
        let wlen: usize = before.chars().rev().take_while(|c| is_name_char(*c)).map(char::len_utf8).sum();
        let start = caret_b - wlen;
        let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(caret_b..)) else { return false };
        self.text = format!("{a}{name}({b}");
        self.caret = self.text.get(..start + name.len() + 1).map(|s| s.chars().count()).unwrap_or(0);
        self.autocomplete.clear();
        self.caret_to_end = false;
        self.sync_caret = true;
        true
    }

    /// The function call the caret is inside (name as typed, upper-cased) and the argument index
    /// (for the hint tooltip). Arguments are separated by the region's list separator; separators
    /// inside `{ }` array constants, `[ ]` table references, nested calls and quotes do not count.
    pub fn current_function(&self) -> Option<(String, usize)> {
        if !self.is_formula() {
            return None;
        }
        let scan = Scan::of(self.text.get(..self.byte_caret())?, self.locale.regional.list);
        // An unfinished array constant belongs to the call around it.
        match scan.stack.iter().rev().find(|f| !matches!(f, Frame::Array))? {
            Frame::Call { name, arg } => Some((name.to_uppercase(), *arg)),
            Frame::Group | Frame::Array => None,
        }
    }
}

/// One open bracket of a formula being scanned.
enum Frame {
    Call { name: String, arg: usize },
    Group,
    Array,
}

/// What a formula prefix leaves open at its end: brackets, a text literal, a quoted sheet name
/// or a table reference.
struct Scan {
    stack: Vec<Frame>,
    /// Inside `"…"` (a doubled quote closes and reopens it).
    in_string: bool,
    /// Inside `'…'` (a quoted sheet name; `''` is an escaped quote).
    in_quote: bool,
    /// Depth of `[ ]` table references; inside them `'` escapes the next character.
    brackets: usize,
}

impl Scan {
    fn of(text: &str, list: char) -> Scan {
        let mut scan = Scan { stack: Vec::new(), in_string: false, in_quote: false, brackets: 0 };
        let mut name = String::new();
        let mut escaped = false;
        for c in text.chars() {
            if scan.in_string {
                scan.in_string = c != '"';
                continue;
            }
            if scan.in_quote {
                scan.in_quote = c != '\'';
                continue;
            }
            if scan.brackets > 0 {
                if escaped {
                    escaped = false;
                } else {
                    match c {
                        '\'' => escaped = true,
                        '[' => scan.brackets += 1,
                        ']' => scan.brackets -= 1,
                        _ => {}
                    }
                }
                continue;
            }
            if is_name_char(c) {
                name.push(c);
                continue;
            }
            let word = std::mem::take(&mut name);
            match c {
                '"' => scan.in_string = true,
                '\'' => scan.in_quote = true,
                '[' => scan.brackets = 1,
                '(' if word.is_empty() => scan.stack.push(Frame::Group),
                '(' => scan.stack.push(Frame::Call { name: word, arg: 0 }),
                '{' => scan.stack.push(Frame::Array),
                ')' | '}' => {
                    scan.stack.pop();
                }
                c if c == list => {
                    // Separators inside an array constant or a bare group are not arguments.
                    if let Some(Frame::Call { arg, .. }) = scan.stack.last_mut() {
                        *arg += 1;
                    }
                }
                _ => {}
            }
        }
        scan
    }

    /// Whether the end of the text is inside a literal, a quoted name or a table reference,
    /// where no function name is being typed.
    fn in_literal(&self) -> bool {
        self.in_string || self.in_quote || self.brackets > 0
    }
}

/// References in formula text (written in `d`) with their byte spans and colour index (same
/// reference text ⇒ same colour), for colouring the editor and outlining ranges on the grid.
pub fn formula_refs(text: &str, d: &Dialect) -> Vec<(usize, usize, RangeRef, Option<String>, usize)> {
    let body = text.strip_prefix('=').or_else(|| text.strip_prefix('+')).unwrap_or(text);
    let offset = text.len() - body.len();
    let Ok(toks) = gridcraft_engine::formula::lexer::tokenize_local(body, d) else { return vec![] };
    let mut out: Vec<(usize, usize, RangeRef, Option<String>, usize)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        use gridcraft_engine::formula::lexer::Tok;
        let (sheet, start, j) = match &toks[i].tok {
            Tok::Sheet(a, _) => (Some(a.clone()), toks[i].start, i + 1),
            _ => (None, toks[i].start, i),
        };
        if let Some(Tok::Word(w)) = toks.get(j).map(|t| &t.tok)
            && let Some(c) = CellRef::parse(w)
        {
            let mut end = toks[j].end;
            let mut range = RangeRef::cell(c);
            if let (Some(Tok::Colon), Some(Tok::Word(w2))) = (toks.get(j + 1).map(|t| &t.tok), toks.get(j + 2).map(|t| &t.tok))
                && let Some(c2) = CellRef::parse(w2)
            {
                range = RangeRef::new(c, c2);
                end = toks[j + 2].end;
                i = j + 3;
            } else {
                i = j + 1;
            }
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

/// Excel-style AutoComplete: the one distinct text entry in the same column (contiguous data
/// above and below) that starts with `typed` (case-insensitive). `None` when ambiguous.
pub fn column_completion(sheet: &gridcraft_engine::model::Sheet, at: CellRef, typed: &str, loc: &gridcraft_locale::Locale) -> Option<String> {
    // Numbers (including `1,5` and `1.000` in a region that spells them so) are never completed.
    let numeric = gridcraft_engine::core::parse::parse_number_text_in(typed, gridcraft_engine::core::DateSystem::D1900, &loc.regional).is_some();
    if typed.is_empty() || typed.starts_with('=') || numeric {
        return None;
    }
    let lower = typed.to_lowercase();
    let mut found: Option<String> = None;
    let mut ambiguous = false;
    let mut scan = |row: u32| -> bool {
        match sheet.value(CellRef::new(row, at.col)) {
            gridcraft_engine::core::Value::Empty => false,
            gridcraft_engine::core::Value::Text(t) => {
                if t.to_lowercase().starts_with(&lower) && t.len() > typed.len() {
                    match &found {
                        Some(f) if !f.eq_ignore_ascii_case(&t) => ambiguous = true,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_mode() {
        let inv = Arc::new(Locale::default());
        let mut e = EditState::new(0, CellRef::new(0, 0), "=SUM(".into(), true, false, inv.clone());
        assert!(e.can_point());
        e.insert_ref("A1");
        assert_eq!(e.text, "=SUM(A1");
        e.insert_ref("A2");
        assert_eq!(e.text, "=SUM(A2");
        e.text.push(':');
        e.caret += 1;
        e.point = None;
        assert!(e.can_point());
        let e2 = EditState::new(0, CellRef::new(0, 0), "hello".into(), true, false, inv);
        assert!(!e2.can_point());
    }

    fn pt_br() -> Arc<Locale> {
        let lang = gridcraft_locale::language("pt-BR").expect("pt-BR language");
        let region = gridcraft_locale::region("pt-BR").expect("pt-BR region");
        Arc::new(Locale::new(lang, lang, *region))
    }

    #[test]
    fn point_mode_follows_the_regional_list_separator() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "=SOMA(A1;".into(), true, false, pt_br());
        assert!(e.can_point());
        e.text = "=SOMA(A1".into();
        e.caret = e.text.chars().count();
        e.point = None;
        assert!(!e.can_point());
        // `-,5` is a number in pt-BR, `-A1` a formula.
        assert!(!EditState::new(0, CellRef::new(0, 0), "-,5".into(), true, false, pt_br()).is_formula());
        assert!(EditState::new(0, CellRef::new(0, 0), "-A1".into(), true, false, pt_br()).is_formula());
    }

    #[test]
    fn arguments_are_counted_with_the_dialect_separator() {
        let e = EditState::new(0, CellRef::new(0, 0), "=SE(A1>1,5;SOMA(B1;B2);".into(), true, false, pt_br());
        assert_eq!(e.current_function(), Some(("SE".into(), 2)));
        let e = EditState::new(0, CellRef::new(0, 0), "=SOMA({1\\2;3\\4};".into(), true, false, pt_br());
        assert_eq!(e.current_function(), Some(("SOMA".into(), 1)));
        let e = EditState::new(0, CellRef::new(0, 0), "=ÉCÉL.VAZIA(".into(), true, false, pt_br());
        assert_eq!(e.current_function(), Some(("ÉCÉL.VAZIA".into(), 0)));
    }

    fn de_de() -> Arc<Locale> {
        let lang = gridcraft_locale::language("de-DE").expect("de-DE language");
        let region = gridcraft_locale::region("de-DE").expect("de-DE region");
        Arc::new(Locale::new(lang, lang, *region))
    }

    #[test]
    fn separators_inside_an_array_constant_are_not_arguments() {
        let at = |text: &str| EditState::new(0, CellRef::new(0, 0), text.into(), true, false, de_de()).current_function();
        // The array's own column and row separators (`.` and `;`) do not start an argument.
        assert_eq!(at("=SUMME({1.2;"), Some(("SUMME".into(), 0)));
        assert_eq!(at("=SUMME({1.2;3.4};"), Some(("SUMME".into(), 1)));
        assert_eq!(at("=SUMME({1.2;3.4};{5;"), Some(("SUMME".into(), 1)));
        // Quotes and groups hide separators too; a bare group is no call.
        assert_eq!(at("=SUMME(\"a;b\";"), Some(("SUMME".into(), 1)));
        assert_eq!(at("=(1;"), None);
        assert_eq!(at("=WENN(1;SUMME(2;3);"), Some(("WENN".into(), 2)));
    }

    #[test]
    fn quoted_names_and_table_references_hide_their_separators() {
        let at = |text: &str, loc: Arc<Locale>| EditState::new(0, CellRef::new(0, 0), text.into(), true, false, loc).current_function();
        assert_eq!(at("=SE('a;b'!A1;", pt_br()), Some(("SE".into(), 1)));
        assert_eq!(at("=SE('a(b'!A1;", pt_br()), Some(("SE".into(), 1)));
        assert_eq!(at("=SE('it''s;'!A1;", pt_br()), Some(("SE".into(), 1)));
        assert_eq!(at("=PROCV(A2;Tabela1[[#Tudo];[Col]];", pt_br()), Some(("PROCV".into(), 2)));
        let inv = Arc::new(Locale::default());
        assert_eq!(at("=VLOOKUP(A1,Table1[[#All],[Col]],", inv.clone()), Some(("VLOOKUP".into(), 2)));
        // An escaped bracket inside a column name does not close the reference.
        assert_eq!(at("=VLOOKUP(A1,Table1[Col'],x],", inv), Some(("VLOOKUP".into(), 2)));
        // Still inside the table reference: the caret is in the column name.
        assert_eq!(at("=VLOOKUP(A1,Table1[[#All],[Co", Arc::new(Locale::default())), Some(("VLOOKUP".into(), 1)));
    }

    #[test]
    fn no_autocomplete_inside_strings_quoted_names_or_table_references() {
        let names: Vec<String> = ["SUM", "SUMIF"].iter().map(|s| s.to_string()).collect();
        for text in ["=\"su", "=SUM(\"a\",\"su", "='su", "=Table1[su", "=Table1[[#All],[su"] {
            let mut e = EditState::new(0, CellRef::new(0, 0), text.into(), true, false, Arc::new(Locale::default()));
            e.update_autocomplete(&names);
            assert!(e.autocomplete.is_empty(), "{text}");
        }
        let mut e = EditState::new(0, CellRef::new(0, 0), "=\"a\"&su".into(), true, false, Arc::new(Locale::default()));
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete.len(), 2);
    }

    #[test]
    fn autocomplete_and_hints() {
        let names: Vec<String> = ["SUM", "SUMIF", "SUMPRODUCT", "AVERAGE"].iter().map(|s| s.to_string()).collect();
        let inv = Arc::new(Locale::default());
        let mut e = EditState::new(0, CellRef::new(0, 0), "=su".into(), true, false, inv.clone());
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete.len(), 3);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=SUM(");
        let e = EditState::new(0, CellRef::new(0, 0), "=IF(A1>0,SUM(B1,".into(), true, false, inv);
        assert_eq!(e.current_function(), Some(("SUM".into(), 1)));
    }

    #[test]
    fn autocomplete_matches_local_names_with_accents() {
        let names: Vec<String> = ["ÉCÉL.VAZIA", "ÉPAR", "SOMA"].iter().map(|s| s.to_string()).collect();
        let mut e = EditState::new(0, CellRef::new(0, 0), "=écé".into(), true, false, pt_br());
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete, vec!["ÉCÉL.VAZIA".to_string()]);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=ÉCÉL.VAZIA(");
    }

    #[test]
    fn ref_spans() {
        let r = formula_refs("=A1+B2:C3*a1+Sheet2!D4", &Dialect::INVARIANT);
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].4, r[2].4); // A1 and a1 share a colour
        assert_eq!(r[1].2.a1(), "B2:C3");
        assert_eq!(r[3].3.as_deref(), Some("Sheet2"));
    }

    #[test]
    fn ref_spans_in_a_localized_formula() {
        let loc = pt_br();
        let r = formula_refs("=SOMA(A1;B2:C3)+1,5*A1", &loc.dialect());
        assert_eq!(r.len(), 3);
        assert_eq!(r[1].2.a1(), "B2:C3");
        assert_eq!(r[0].4, r[2].4);
    }
}
