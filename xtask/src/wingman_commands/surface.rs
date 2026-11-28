//! Follow literal menu tables and labels through Rust syntax rather than executing UI closures.
//! Unknown values (e.g. workbook sheet names) stay dynamic instead of becoming invented commands.
use super::Inventory;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use syn::{
    Expr, File, Item, Lit, Pat, Token,
    punctuated::Punctuated,
    visit::{self, Visit},
};
type Exprs = Punctuated<Expr, Token![,]>;
type Vars = BTreeMap<String, Expr>;

fn files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for p in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let p = p.map_err(|e| e.to_string())?.path();
        if p.is_dir() {
            files(&p, out)?
        } else if p.extension().is_some_and(|s| s == "rs") {
            out.push(p)
        }
    }
    Ok(())
}
fn parse(p: &Path) -> Result<File, String> {
    let s = fs::read_to_string(p).map_err(|e| e.to_string())?;
    syn::parse_file(&s).map_err(|e| format!("{}: {e}", p.display()))
}
fn path(e: &Expr) -> String {
    if let Expr::Path(p) = e { p.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default() } else { String::new() }
}
fn args(m: &syn::Macro) -> Vec<Expr> {
    m.parse_body_with(Exprs::parse_terminated).map(|a| a.into_iter().collect()).unwrap_or_default()
}
fn literal(e: &Expr) -> Option<String> {
    if let Expr::Lit(l) = e
        && let Lit::Str(s) = &l.lit
    {
        Some(s.value())
    } else {
        None
    }
}
fn resolve(e: &Expr, vars: &Vars, depth: u8) -> Option<String> {
    if depth > 12 {
        return None;
    }
    if let Some(s) = literal(e) {
        return Some(s);
    }
    let inner = match e {
        Expr::Reference(r) => &r.expr,
        Expr::Paren(r) => &r.expr,
        Expr::Unary(r) => &r.expr,
        Expr::Path(_) => vars.get(&path(e))?,
        Expr::MethodCall(m) if m.method == "tr" => m.args.first()?,
        Expr::Call(c) if matches!(&*c.func,Expr::Path(p) if p.path.segments.iter().any(|s|s.ident=="RichText")) => c.args.first()?,
        Expr::MethodCall(m)
            if ["as_str", "to_owned", "into", "replacen", "clone", "to_string", "font", "color", "strong", "italics", "small", "size"]
                .contains(&m.method.to_string().as_str()) =>
        {
            &m.receiver
        }
        _ => return None,
    };
    resolve(inner, vars, depth + 1)
}
fn elements(e: &Expr, vars: &Vars, depth: u8) -> Vec<Expr> {
    if depth > 10 {
        return Vec::new();
    }
    let again = |v: &Expr| elements(v, vars, depth + 1);
    match e {
        Expr::Array(a) => a.elems.iter().cloned().collect(),
        Expr::Macro(m) if m.mac.path.is_ident("vec") => args(&m.mac),
        Expr::Reference(r) => again(&r.expr),
        Expr::Paren(r) => again(&r.expr),
        Expr::Path(_) => vars.get(&path(e)).map(again).unwrap_or_default(),
        Expr::MethodCall(m) if ["iter", "into_iter", "copied", "cloned", "as_slice"].contains(&m.method.to_string().as_str()) => again(&m.receiver),
        Expr::MethodCall(m) if m.method == "enumerate" => again(&m.receiver).into_iter().map(|e| syn::parse_quote!((0,#e))).collect(),
        _ => Vec::new(),
    }
}
// egui builds the widget first, then attaches a keytip/handler to its Response. Follow that
// binding rather than treating a toolbar caption with the same spelling as a command.
fn widget_label(e: &Expr, vars: &Vars, depth: u8) -> Option<String> {
    if depth > 12 {
        return None;
    }
    let next = |e: &Expr| widget_label(e, vars, depth + 1);
    match e {
        Expr::Path(_) => next(vars.get(&path(e))?),
        Expr::Reference(r) => next(&r.expr),
        Expr::MethodCall(m) if ["add", "add_sized"].contains(&m.method.to_string().as_str()) => next(m.args.last()?),
        Expr::MethodCall(m) if ["button", "small_button", "menu_button"].contains(&m.method.to_string().as_str()) => {
            resolve(m.args.first()?, vars, 0)
        }
        Expr::MethodCall(m) => next(&m.receiver),
        Expr::Call(c) => {
            let k = path(&c.func);
            let i = match k.as_str() {
                "big_button" | "big_split_button" | "small_button" | "small_split_button" | "split_button" => 2,
                "toggle_button" | "icon_button" => 3,
                "new" if matches!(&*c.func,Expr::Path(p) if p.path.segments.iter().any(|s|s.ident=="Button")) => 0,
                _ => return None,
            };
            let label = resolve(c.args.get(i)?, vars, 0).unwrap_or_default();
            if label.is_empty() { resolve(c.args.get(i + 1)?, vars, 0) } else { Some(label) }
        }
        _ => None,
    }
}
fn bind(p: &Pat, e: &Expr, vars: &mut Vars) {
    match p {
        Pat::Ident(i) => {
            vars.insert(i.ident.to_string(), e.clone());
        }
        Pat::Tuple(t) => {
            if let Expr::Tuple(v) = e {
                for (p, e) in t.elems.iter().zip(&v.elems) {
                    bind(p, e, vars)
                }
            }
        }
        Pat::Reference(v) => bind(&v.pat, e, vars),
        Pat::Type(v) => bind(&v.pat, e, vars),
        _ => {}
    }
}

struct Adapters<'a>(&'a mut Inventory);
impl Adapters<'_> {
    fn pair(&mut self, name: Option<&Expr>, id: Option<&Expr>) {
        let (Some(name), Some(id)) = (name.and_then(literal), id) else { return };
        let id = if let Expr::Call(c) = id { c.args.first().and_then(literal) } else { literal(id) };
        if let Some(id) = id {
            self.0.adapters.entry(id).or_default().insert(name);
        }
    }
}
impl<'a> Visit<'a> for Adapters<'_> {
    fn visit_macro(&mut self, m: &syn::Macro) {
        if m.path.is_ident("tool") {
            let xs = args(m);
            self.pair(xs.first(), xs.get(2));
        }
    }
    fn visit_expr_call(&mut self, e: &syn::ExprCall) {
        if path(&e.func) == "control" {
            self.pair(e.args.first(), e.args.get(2));
        }
        visit::visit_expr_call(self, e);
    }
    fn visit_expr_struct(&mut self, e: &syn::ExprStruct) {
        if e.path.is_ident("Tool") {
            let get = |key: &str| e.fields.iter().find_map(|f| matches!(&f.member,syn::Member::Named(i) if i==key).then_some(&f.expr));
            self.pair(get("name"), get("command"));
        }
        visit::visit_expr_struct(self, e);
    }
}
pub(super) fn adapters(dir: &Path, index: &mut Inventory) -> Result<(), String> {
    let mut paths = Vec::new();
    files(dir, &mut paths)?;
    for p in paths {
        if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("workbook")) {
            Adapters(index).visit_file(&parse(&p)?);
        }
    }
    if !index.adapters.contains_key("cell.set") {
        return Err("could not read native Wingman tool declarations".into());
    }
    Ok(())
}

fn title(s: &str) -> String {
    s.split(['_', '/'])
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn section(file: &str, func: &str) -> String {
    let base = file.split('/').next_back().unwrap_or(file);
    match (file, base, func) {
        (_, _, f) if f.starts_with("pivot") => "PivotTable Analyze".into(),
        (_, "paper_view", _) => "Page Layout".into(),
        (_, "insert", "draw") => "Draw".into(),
        (_, "contextual", f) if f.starts_with("chart") => "Chart Design".into(),
        (_, "contextual", _) => "Table Design".into(),
        (_, "ribbon", "theme_choices") => "View".into(),
        (_, "ribbon", "file_menu") => "File".into(),
        (f, _, _) if f.starts_with("backstage") => "File".into(),
        (f, _, _) if f.starts_with("ribbon/") => match base {
            "page_layout" => "Page Layout".into(),
            "title_bar" => "Window".into(),
            "galleries" => "Home".into(),
            _ => title(base),
        },
        (f, _, _) if f.starts_with("dialogs") => "Dialogs".into(),
        (f, _, _) if f.starts_with("wingman") => "Wingman".into(),
        (_, "menu" | "menus", _) => "Grid context".into(),
        (_, "tabs", _) => "Sheet tabs".into(),
        (_, "macro_editor" | "macro_ui", _) => "Automate".into(),
        (_, "outline" | "quick_analysis", _) => "Data".into(),
        _ => "Workbook".into(),
    }
}

struct Surface<'a> {
    index: &'a mut Inventory,
    file: String,
    tab: String,
    group: String,
    id: String,
    vars: Vars,
}
impl Surface<'_> {
    fn add(&mut self, label: &Expr, id: Option<&str>) {
        if let Some(label) = resolve(label, &self.vars, 0) {
            self.index.add(&self.tab, &self.group, &label, id.unwrap_or(&self.id));
        }
    }
    fn entries(&mut self, e: &Expr) {
        for choice in elements(e, &self.vars, 0) {
            if let Expr::Tuple(t) = choice
                && let (Some(label), Some(id)) = (t.elems.first(), t.elems.get(1).and_then(|a| resolve(a, &self.vars, 0)))
            {
                self.add(label, Some(&id));
            }
        }
    }
    fn tuple(&mut self, e: &syn::ExprTuple) {
        if let (Some(a), Some(b)) = (e.elems.first().and_then(literal), e.elems.get(1).and_then(literal)) {
            if self.file == "control" && a.contains('.') && e.elems.len() == 3 && a.split('.').all(|s| s.chars().all(char::is_alphanumeric)) {
                if a.contains('.') && !b.is_empty() {
                    self.index.add("Desktop", "Control channel", &b, &a);
                }
            } else if self.file == "grid/menus" && (self.index.ids.contains(&b) || b.starts_with("dialog:") || b.starts_with("ui:")) {
                self.index.add(&self.tab, &self.group, a.split('|').next_back().unwrap_or(&a), &b);
            }
        }
    }
    fn fixed_choices(&mut self, e: &Expr, group: &str, id: &str) {
        for item in elements(e, &self.vars, 0) {
            let v = if let Expr::Tuple(t) = &item {
                t.elems.first().and_then(literal)
            } else if let Expr::Lit(l) = &item
                && let Lit::Float(n) = &l.lit
            {
                Some(n.base10_digits().trim_end_matches(".0").into())
            } else {
                literal(&item)
            };
            if let Some(v) = v {
                self.index.add(&self.tab, group, &v, id)
            }
        }
    }
    fn table_preset(&mut self) {
        let (Some(family), Some(Expr::Lit(n))) = (self.vars.get("fam").and_then(literal), self.vars.get("n")) else { return };
        let Lit::Int(n) = &n.lit else { return };
        if let Ok(count) = n.base10_parse::<usize>() {
            for i in 1..=count {
                let label = format!("TableStyle{family}{i}");
                self.index.add("Table Design", &format!("Table Styles / {family}"), &label, "table.style");
                self.index.add("Home", &format!("Format as Table / {family}"), &label, "home.formatAsTable");
            }
        }
    }
}
struct Targets<'s> {
    ids: BTreeSet<String>,
    vars: &'s Vars,
}
impl<'a> Visit<'a> for Targets<'_> {
    fn visit_expr_call(&mut self, c: &syn::ExprCall) {
        if path(&c.func) == "act"
            && let Some(id) = c.args.get(1).and_then(|a| resolve(a, self.vars, 0))
        {
            self.ids.insert(id);
        }
        visit::visit_expr_call(self, c);
    }
    fn visit_expr_method_call(&mut self, c: &syn::ExprMethodCall) {
        if matches!(c.method.to_string().as_str(), "run" | "run_or_alert" | "open_dialog")
            && let Some(id) = c.args.first().and_then(|a| resolve(a, self.vars, 0))
        {
            self.ids.insert(if c.method == "open_dialog" { format!("dialog:{id}") } else { id });
        }
        visit::visit_expr_method_call(self, c);
    }
}
impl<'a> Visit<'a> for Surface<'_> {
    fn visit_item_fn(&mut self, f: &'a syn::ItemFn) {
        if f.attrs.iter().any(|a| a.path().is_ident("test")) {
            return;
        }
        let name = f.sig.ident.to_string();
        self.tab = section(&self.file, &name);
        self.group = title(&name);
        if name == "cf_menu" {
            self.group = "Conditional Formatting".into();
        }
        if self.file == "ribbon/home" {
            for (name, group, id) in [("FONTS", "Font family", "home.fontName"), ("ACCOUNTING_SYMBOLS", "Accounting symbols", "home.numberFormat")] {
                if let Some(choice) = self.vars.get(name).cloned() {
                    self.fixed_choices(&choice, group, id);
                }
            }
        }
        let vars = self.vars.clone();
        self.visit_block(&f.block);
        self.vars = vars;
    }
    fn visit_item_mod(&mut self, m: &'a syn::ItemMod) {
        if !m.ident.to_string().contains("test") {
            visit::visit_item_mod(self, m)
        }
    }
    fn visit_impl_item_fn(&mut self, f: &'a syn::ImplItemFn) {
        if f.attrs.iter().any(|a| a.path().is_ident("test")) {
            return;
        }
        let name = f.sig.ident.to_string();
        self.tab = section(&self.file, &name);
        self.group = title(&name);
        let vars = self.vars.clone();
        self.visit_block(&f.block);
        self.vars = vars;
    }
    fn visit_local(&mut self, l: &'a syn::Local) {
        visit::visit_local(self, l);
        if let Some(i) = &l.init {
            if let Pat::Ident(p) = &l.pat
                && self.file == "ribbon/home"
            {
                if p.ident == "names" && self.group == "Number Combo" {
                    self.fixed_choices(&i.expr, "Number format", "home.numberFormat");
                }
                if p.ident == "sizes" && self.group == "Size Combo" {
                    self.fixed_choices(&i.expr, "Font size", "home.fontSize");
                }
            }
            bind(&l.pat, &i.expr, &mut self.vars)
        }
    }
    fn visit_expr_for_loop(&mut self, v: &'a syn::ExprForLoop) {
        let xs = elements(&v.expr, &self.vars, 0);
        if xs.is_empty() {
            visit::visit_expr_for_loop(self, v);
        } else {
            self.visit_expr(&v.expr);
            let previous = self.vars.clone();
            for item in xs {
                self.vars.clone_from(&previous);
                bind(&v.pat, &item, &mut self.vars);
                if self.file == "ribbon/galleries" && self.group == "Table Gallery" {
                    self.table_preset();
                }
                if self.file == "ribbon/galleries"
                    && self.group == "Cell Style Gallery"
                    && let Some(group) = self.vars.get("g").and_then(literal)
                    && let Some(names) = self.vars.get("names").cloned()
                {
                    self.fixed_choices(&names, &format!("Cell Styles / {group}"), "home.cellStyle");
                }
                self.visit_block(&v.body);
            }
            self.vars = previous;
        }
    }
    fn visit_expr_if(&mut self, v: &'a syn::ExprIf) {
        let mut target = Targets { ids: BTreeSet::new(), vars: &self.vars };
        target.visit_block(&v.then_branch);
        let id = if target.ids.len() == 1 { target.ids.into_iter().next().unwrap_or_default() } else { String::new() };
        let parent = std::mem::replace(&mut self.id, id);
        self.visit_expr(&v.cond);
        self.id = parent;
        self.visit_block(&v.then_branch);
        if let Some((_, e)) = &v.else_branch {
            self.visit_expr(e);
        }
    }
    fn visit_expr_tuple(&mut self, c: &'a syn::ExprTuple) {
        self.tuple(c);
        visit::visit_expr_tuple(self, c)
    }
    fn visit_expr_macro(&mut self, c: &'a syn::ExprMacro) {
        if c.mac.path.is_ident("vec") {
            for item in args(&c.mac) {
                self.visit_expr(&item)
            }
        }
    }
    fn visit_expr_call(&mut self, c: &'a syn::ExprCall) {
        let cmd = path(&c.func);
        if cmd == "control_keytip"
            && let Some(label) = c.args.get(2).and_then(|a| widget_label(a, &self.vars, 0))
            && let Some(id) = c.args.get(3).and_then(|a| resolve(a, &self.vars, 0))
        {
            self.index.add(&self.tab, &self.group, &label, &id);
        }
        let i = match cmd.as_str() {
            "menu_items" => {
                if let Some(items) = c.args.get(2) {
                    self.entries(items)
                }
                None
            }
            "big_button" | "big_split_button" | "small_button" | "small_split_button" | "split_button" => Some(2),
            "icon_button" | "toggle_button" => Some(3),
            "new" if matches!(c.func.as_ref(),Expr::Path(p) if p.path.segments.iter().any(|s|s.ident=="Button")) => Some(0),
            _ => None,
        };
        if let Some(i) = i
            && let Some(arg) = c.args.get(i)
        {
            if resolve(arg, &self.vars, 0).is_some_and(|s| s.is_empty()) {
                if let Some(tip) = c.args.get(i + 1) {
                    self.add(tip, None)
                }
            } else {
                self.add(arg, None)
            }
        }
        visit::visit_expr_call(self, c);
    }
    fn visit_expr_method_call(&mut self, c: &'a syn::ExprMethodCall) {
        let k = c.method.to_string();
        if k == "clicked"
            && !self.id.is_empty()
            && let Some(label) = widget_label(&c.receiver, &self.vars, 0)
        {
            self.index.add(&self.tab, &self.group, &label, &self.id);
        }
        let pos = match k.as_str() {
            "menu_button" | "button" | "small_button" | "link" => Some(0),
            "checkbox" | "selectable_label" | "toggle_value" => Some(1),
            "radio_value" | "selectable_value" => Some(2),
            _ => None,
        };
        if let Some(pos) = pos
            && let Some(label) = c.args.get(pos)
        {
            self.add(label, None);
            if k == "menu_button"
                && let Some(menu) = resolve(label, &self.vars, 0)
            {
                let old = self.group.clone();
                self.group += " / ";
                self.group += &menu;
                visit::visit_expr_method_call(self, c);
                self.group = old;
                return;
            }
        }
        visit::visit_expr_method_call(self, c);
    }
}
pub(super) fn controls(ui: &Path, index: &mut Inventory) -> Result<(), String> {
    let mut paths = Vec::new();
    files(ui, &mut paths)?;
    for p in paths {
        let file = p.strip_prefix(ui).map_err(|e| e.to_string())?.with_extension("").to_string_lossy().replace('\\', "/");
        if file.contains("test") || file.starts_with("keytips") || file.starts_with("i18n") || file.contains("generated") {
            continue;
        }
        let ast = parse(&p)?;
        if file == "icons"
            && let Some(Item::Const(library)) = ast.items.iter().find(|i| matches!(i,Item::Const(c) if c.ident=="LIBRARY"))
        {
            for item in elements(&library.expr, &Vars::new(), 0) {
                if let Expr::Tuple(t) = item
                    && let Some(label) = t.elems.first().and_then(literal)
                {
                    index.add("Insert", "Icons gallery", &label, "insert.icons");
                }
            }
        }
        let vars =
            ast.items.iter().filter_map(|i| if let Item::Const(c) = i { Some((c.ident.to_string(), (*c.expr).clone())) } else { None }).collect();
        Surface { index, file, tab: "Workbook".into(), group: "Controls".into(), id: String::new(), vars }.visit_file(&ast);
    }
    Ok(())
}
