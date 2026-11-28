use super::show::{ok_cancel, take_first_focus};
use super::{Dialog, Json, SheetApp, json, vec2};
use gridcraft_engine::formula::FormulaLocale;
use gridcraft_engine::formula::lexer::{Tok, tokenize_locale};
use gridcraft_engine::functions::params::{self, Params};
use std::collections::HashMap;

/// A function call in formula text: its name as written, its arguments' texts, and the byte
/// span from the name to just after its `)` (the end of the text when it isn't closed yet).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Call {
    pub name: String,
    pub args: Vec<String>,
    pub start: usize,
    pub end: usize,
}

/// The innermost call around byte `caret` of formula `text`, in the edit's formula syntax.
pub(crate) fn call_at(text: &str, caret: usize, locale: FormulaLocale) -> Option<Call> {
    let tokens = tokenize_locale(text, locale).ok()?;
    let mut found = None;
    for i in 0..tokens.len() {
        if let Some(call) = scan(text, &tokens, i).filter(|c| c.start <= caret && caret <= c.end) {
            found = Some(call);
        }
    }
    found
}

/// The first call of formula `text` (the one Function Arguments opens on for a cell not being edited).
pub(crate) fn first_call(text: &str, locale: FormulaLocale) -> Option<Call> {
    let tokens = tokenize_locale(text, locale).ok()?;
    (0..tokens.len()).find_map(|i| scan(text, &tokens, i))
}

fn scan(text: &str, tokens: &[gridcraft_engine::formula::lexer::Token], i: usize) -> Option<Call> {
    let head = tokens.get(i)?;
    let Tok::Func(name) = &head.tok else { return None };
    let slice = |a: usize, b: usize| text.get(a..b).unwrap_or("").trim().to_string();
    let (mut depth, mut from, mut args) = (0usize, head.end, Vec::new());
    let (mut end, mut close) = (text.len(), text.len());
    for t in tokens.iter().skip(i + 1) {
        match t.tok {
            Tok::Func(_) | Tok::LParen | Tok::LBrace => depth += 1,
            Tok::RParen | Tok::RBrace if depth > 0 => depth -= 1,
            Tok::RParen => {
                (end, close) = (t.end, t.start);
                break;
            }
            Tok::Comma if depth == 0 => {
                args.push(slice(from, t.start));
                from = t.end;
            }
            _ => {}
        }
    }
    let last = slice(from, close.max(from));
    if !(args.is_empty() && last.is_empty()) {
        args.push(last);
    }
    Some(Call { name: name.clone(), args, start: head.start, end })
}

/// Opens Function Arguments. With `name`, for a new call of it at the caret of the cell edit (a
/// new edit when none is open); without, on the call at the caret of the edit, or on the first
/// call of the active cell's formula. `None` when there is no such call.
pub(crate) fn open(app: &mut SheetApp, params: &Json) -> Option<Dialog> {
    let restore = app.editor.as_ref().filter(|e| !e.python).map(|e| e.text.clone());
    let (name, args, before, after) = if let Some(name) = params.get("name").and_then(Json::as_str) {
        if restore.is_none() {
            app.begin_edit(Some("=".into()), false);
        }
        let ed = app.editor.as_ref()?;
        let caret = ed.text.char_indices().nth(ed.caret).map_or(ed.text.len(), |(i, _)| i);
        let (a, b) = ed.text.split_at(caret);
        let a = if ed.is_formula() { a.to_string() } else { "=".into() };
        let b = if ed.is_formula() { b.to_string() } else { String::new() };
        (name.to_string(), Vec::new(), a, b)
    } else {
        let call = match app.editor.as_ref() {
            Some(ed) if ed.is_formula() => {
                let caret = ed.text.char_indices().nth(ed.caret).map_or(ed.text.len(), |(i, _)| i);
                call_at(&ed.text, caret, ed.locale)?
            }
            Some(_) => return None,
            None => {
                let has_formula = app.session.active().and_then(|d| d.wb.active()?.cell(d.selection.active)?.formula.as_ref().map(|_| ())).is_some();
                if !has_formula {
                    return None;
                }
                app.begin_edit(None, false);
                let ed = app.editor.as_ref()?;
                match first_call(&ed.text, ed.locale) {
                    Some(c) => c,
                    None => {
                        app.cancel_edit();
                        return None;
                    }
                }
            }
        };
        let text = app.editor.as_ref().map(|e| e.text.clone()).unwrap_or_default();
        (call.name, call.args, text.get(..call.start)?.to_string(), text.get(call.end..)?.to_string())
    };
    let mut d = Dialog::custom(
        "functionArguments",
        "Function Arguments",
        json!({"function": name, "args": args, "given": args.len(), "before": before, "after": after, "restore": restore, "active": 0}),
    );
    d.focus_first_field = true;
    Some(d)
}

/// The function's parameters, from the catalog's signature, or `arg1`, `arg2`… for a function it
/// doesn't hold.
pub(crate) fn params_of(name: &str, locale: FormulaLocale) -> Params {
    crate::formula_bar::function_signature(locale.canonical_name(name)).map_or_else(Params::generic, |s| Params::parse(&s))
}

/// The dialog's argument texts, laid out on its fields.
fn args_of(d: &Dialog, p: &Params) -> Vec<String> {
    let mut args: Vec<String> =
        d.values.get("args").and_then(Json::as_array).map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    p.arrange(&mut args);
    args
}

fn text_value(d: &Dialog, key: &str) -> String {
    d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string()
}

/// The call the fields make (`SUM(A1:A3,B1)`) and the whole formula with it.
pub(crate) fn compose(d: &Dialog, locale: FormulaLocale) -> (String, String) {
    let name = text_value(d, "function");
    let p = params_of(&name, locale);
    let args = args_of(d, &p);
    let given = d.values.get("given").and_then(Json::as_u64).map_or(0, |g| g as usize);
    let call = format!("{name}({})", p.written_keeping(&args, given).join(&locale.list_separator().to_string()));
    let full = format!("{}{call}{}", text_value(d, "before"), text_value(d, "after"));
    (call, full)
}

/// Puts the formula the fields make into the cell edit, as the dialog shows it while open.
fn sync_editor(app: &mut SheetApp, call: &str, full: &str, before: &str) {
    let Some(ed) = app.editor.as_mut() else { return };
    if ed.text == full {
        return;
    }
    ed.text = full.to_string();
    ed.caret = before.chars().count() + call.chars().count();
    ed.anchor = ed.caret;
    ed.point = None;
    ed.autocomplete.clear();
    ed.completion = None;
    ed.caret_to_end = false;
    ed.sync_caret = true;
}

#[derive(Clone, Default)]
struct Cache {
    wb: usize,
    values: HashMap<String, Option<String>>,
}

/// What formula `text` (in the edit's syntax, with its `=`) evaluates to in the edited cell, as
/// the engine's `formulas.evaluate` gives it, for showing; `None` when it can't be evaluated.
pub(crate) fn preview(app: &mut SheetApp, ctx: &egui::Context, text: &str) -> Option<String> {
    let (locale, cell) = app.editor.as_ref().map(|e| (e.locale, e.cell))?;
    let doc = app.session.active()?;
    let sheet = doc.wb.active_sheet();
    let formula = crate::formula_locale::canonical(text, locale, &doc.wb, sheet, cell).ok()?;
    let wb = std::sync::Arc::as_ptr(&doc.wb) as usize;
    let id = egui::Id::unique("gridcraft.functionArguments.cache");
    let mut cache: Cache = ctx.data_mut(|m| m.get_temp(id)).unwrap_or_default();
    if cache.wb != wb || cache.values.len() > 256 {
        cache = Cache { wb, values: HashMap::new() };
    }
    let key = format!("{}|{sheet:?}|{}|{formula}", doc.revision, cell.a1());
    if let Some(v) = cache.values.get(&key) {
        return v.clone();
    }
    let shown = app.session.run("formulas.evaluate", json!({"formula": formula, "cell": cell.a1()})).ok().map(|v| show_value(&v, locale));
    cache.values.insert(key, shown.clone());
    ctx.data_mut(|m| m.insert_temp(id, cache));
    shown
}

/// A value as the dialog writes it: text in quotes, arrays in braces (the first items only).
pub(crate) fn show_value(v: &Json, locale: FormulaLocale) -> String {
    const MAX_ITEMS: usize = 20;
    let item = |v: &Json| match v {
        Json::Null => "0".to_string(),
        Json::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Json::Number(n) => {
            let text = gridcraft_engine::core::Value::Number(n.as_f64().unwrap_or(0.0)).display();
            text.replace('.', &locale.decimal_separator().to_string())
        }
        Json::String(s) => format!("\"{s}\""),
        Json::Object(o) => o.get("error").and_then(Json::as_str).unwrap_or("#VALUE!").to_string(),
        Json::Array(_) => String::new(),
    };
    let Json::Array(rows) = v else { return item(v) };
    let mut out = String::from("{");
    let mut count = 0;
    for (r, row) in rows.iter().enumerate() {
        if r > 0 {
            out.push(';');
        }
        for (c, x) in row.as_array().map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
            if count == MAX_ITEMS {
                out.push('…');
                out.push('}');
                return out;
            }
            if c > 0 {
                out.push(locale.array_column_separator());
            }
            out.push_str(&item(x));
            count += 1;
        }
    }
    out.push('}');
    out
}

/// Field labels: the parameter names in the interface language when it has all of them (else
/// English, so labels never mix two languages), capitalised.
fn labels(fields: &[params::Param], language: crate::i18n::Language) -> Vec<String> {
    let translated: Option<Vec<String>> =
        fields.iter().map(|p| language.function_argument(p.base()).map(|t| format!("{t}{}", p.name.get(p.base().len()..).unwrap_or("")))).collect();
    let names = translated.unwrap_or_else(|| fields.iter().map(|p| p.name.clone()).collect());
    names.into_iter().map(|n| capitalise(&n)).collect()
}

/// What argument `param` of `function` is for, in the interface language (English while untranslated).
fn help_text(language: crate::i18n::Language, function: &str, param: &str) -> String {
    params::describe(function, param).map(|english| language.tr(english).to_string()).unwrap_or_default()
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Function Arguments (Shift+F3, fx): one box per argument with what it evaluates to, the
/// function's result and the whole formula's, and what the focused argument is for. Optional
/// arguments are not bold; a repeating argument gets another box once its last one is filled in.
/// The cell edit shows the formula as it is built; OK enters it, Cancel puts the edit back.
pub(super) fn function_arguments(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let language = app.ui.language;
    let locale = app.editor.as_ref().map_or(language.formula_locale(), |e| e.locale);
    let name = text_value(d, "function");
    let p = params_of(&name, locale);
    let mut args = args_of(d, &p);
    let fields = p.fields(args.len());
    let shown = labels(&fields, language);
    let canonical = locale.canonical_name(&name).to_ascii_uppercase();
    ui.label(egui::RichText::new(locale.function_name(&canonical)).strong());
    let mut active = d.values.get("active").and_then(Json::as_u64).map_or(0, |a| a as usize);
    if fields.is_empty() {
        ui.label(language.tr("This function takes no arguments."));
    }
    egui::ScrollArea::vertical().max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("functionArgumentsGrid").num_columns(3).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
            for (i, (field, label)) in fields.iter().zip(&shown).enumerate() {
                let text = egui::RichText::new(label);
                let l = ui.label(if field.optional { text } else { text.strong() });
                let Some(value) = args.get_mut(i) else { continue };
                let r = ui
                    .add(egui::TextEdit::singleline(value).id(egui::Id::unique("gridcraft.functionArguments").with(i)).desired_width(220.0))
                    .labelled_by(l.id);
                if i == 0 {
                    take_first_focus(d, &r);
                }
                if r.has_focus() {
                    active = i;
                }
                let v = value.trim().to_string();
                let result = if v.is_empty() { None } else { preview(app, ui.ctx(), &format!("={v}")) };
                ui.label(result.map(|r| format!("= {r}")).unwrap_or_default());
                ui.end_row();
            }
        });
    });
    d.values.insert("args".into(), json!(args));
    d.values.insert("active".into(), json!(active));
    let (call, full) = compose(d, locale);
    let before = text_value(d, "before");
    sync_editor(app, &call, &full, &before);
    let result = preview(app, ui.ctx(), &format!("={call}"));
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(result.as_ref().map(|r| format!("= {r}")).unwrap_or_default());
        });
    });
    ui.separator();
    ui.label(crate::formula_locale::description(&canonical, FormulaLocale::En, language).unwrap_or_default());
    if let (Some(field), Some(label)) = (fields.get(active), shown.get(active)) {
        let help = help_text(language, &canonical, &field.name);
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("{label}:")).strong());
            ui.label(help);
        });
    }
    ui.add_space(4.0);
    let total = preview(app, ui.ctx(), &full);
    ui.label(format!("{} {}", language.tr("Formula result ="), total.unwrap_or_default()));
    ok_cancel(ui, confirm, open);
    if !*open {
        cancel(app, d);
    }
    if *confirm {
        sync_editor(app, &call, &full, &before);
        if !app.commit_edit(0, 0, false, false) {
            *confirm = false;
        }
    }
}

/// Escape or the close box closed a dialog: Function Arguments cancels.
pub(super) fn closed(app: &mut SheetApp, d: &Dialog) {
    if d.name == "functionArguments" {
        cancel(app, d);
    }
}

/// Cancel, Escape or the close box: the cell edit goes back to what it was before the dialog,
/// or ends when the dialog started it.
fn cancel(app: &mut SheetApp, d: &Dialog) {
    match d.values.get("restore").and_then(Json::as_str) {
        Some(text) => {
            if let Some(ed) = app.editor.as_mut() {
                ed.text = text.to_string();
                ed.caret = text.chars().count();
                ed.anchor = ed.caret;
                ed.sync_caret = true;
            }
        }
        None => app.cancel_edit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FormulaLocale::{En, Es};

    fn args(c: &Call) -> Vec<&str> {
        c.args.iter().map(String::as_str).collect()
    }

    #[test]
    fn the_argument_help_is_looked_up_in_the_translation_catalog() {
        use egui_kittest::kittest::Queryable as _;
        let english = params::describe("SUM", "number").unwrap();
        crate::i18n::set_test_catalog(&[(english, "Catalog help for the number argument")]);
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        let d = open(&mut app, &json!({"name": "SUM"})).unwrap();
        let mut h = egui_kittest::Harness::builder().with_size(vec2(520.0, 400.0)).build_ui_state(
            |ui, (app, d, confirm, open): &mut (SheetApp, Dialog, bool, bool)| function_arguments(app, ui, d, confirm, open),
            (app, d, false, true),
        );
        h.run();
        assert!(h.query_by_label("Catalog help for the number argument").is_some());
        assert!(h.query_by_label(english).is_none());
        crate::i18n::set_test_catalog(&[]);
    }

    #[test]
    fn the_call_at_the_caret_is_split_into_its_arguments() {
        let text = "=1+VLOOKUP(A1,Sheet2!A:C,MAX(1,2),FALSE)*2";
        let at = text.find("A:C").unwrap();
        let c = call_at(text, at, En).unwrap();
        assert_eq!(c.name, "VLOOKUP");
        assert_eq!(args(&c), ["A1", "Sheet2!A:C", "MAX(1,2)", "FALSE"]);
        assert_eq!(&text[..c.start], "=1+");
        assert_eq!(&text[c.end..], "*2");
        let inner = call_at(text, text.find("1,2").unwrap(), En).unwrap();
        assert_eq!((inner.name.as_str(), args(&inner)), ("MAX", vec!["1", "2"]));
        let quoted = call_at("=IF(A1=\"a,b\",{1,2},'My, Sheet'!B2)", 5, En).unwrap();
        assert_eq!(args(&quoted), ["A1=\"a,b\"", "{1,2}", "'My, Sheet'!B2"]);
        let open = call_at("=SUM(A1,", 7, En).unwrap();
        assert_eq!((args(&open), open.end), (vec!["A1", ""], 8));
        assert_eq!(args(&call_at("=TODAY()", 7, En).unwrap()), Vec::<&str>::new());
        assert_eq!(call_at("=A1+B1", 3, En), None);
        let es = call_at("=SUMA(A1;2,5)", 6, Es).unwrap();
        assert_eq!((es.name.as_str(), args(&es)), ("SUMA", vec!["A1", "2,5"]));
        assert_eq!(first_call("=ROUND(SUM(A1:A3),2)", En).map(|c| c.name), Some("ROUND".into()));
    }

    #[test]
    fn values_are_shown_as_the_formula_language_writes_them() {
        assert_eq!(show_value(&json!(6.5), En), "6.5");
        assert_eq!(show_value(&json!(6.5), Es), "6,5");
        assert_eq!(show_value(&json!("a"), En), "\"a\"");
        assert_eq!(show_value(&json!({"error": "#N/A"}), En), "#N/A");
        assert_eq!(show_value(&json!([[1, 2], [3, true]]), En), "{1,2;3,TRUE}");
        assert_eq!(show_value(&json!([[1, 2]]), Es), "{1\\2}");
        let long = show_value(&json!([(0..30).collect::<Vec<_>>()]), En);
        assert!(long.ends_with("…}") && long.matches(',').count() == 19, "{long}");
    }
}
