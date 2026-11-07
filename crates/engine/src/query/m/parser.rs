//! The M parser: recursive descent over the tokens, with the specification's operator
//! precedence (`??`, `or`, `and`, `is`, `as`, equality, relational, additive, multiplicative,
//! `meta`, unary) and nesting bounded so hostile formulas can't exhaust the stack.

use std::rc::Rc;

use super::ast::{BinOp, Expr, FuncDef, ListItem, PRIMITIVE_TYPES, Param, TypeExpr, UnOp};
use super::lexer::{Tok, Token, lex};
use super::value::MError;

/// Deepest nesting of expressions the parser follows.
const MAX_DEPTH: usize = 200;

/// Parses one M expression (a query's formula), optionally followed by `;`.
pub fn parse(src: &str) -> Result<Expr, MError> {
    let toks = lex(src)?;
    let mut p = Parser { src, toks, pos: 0, depth: 0 };
    let e = p.expr()?;
    p.eat_p(";");
    if !matches!(p.peek(), Tok::Eof) {
        return Err(p.unexpected("the end of the formula"));
    }
    Ok(e)
}

struct Parser<'s> {
    src: &'s str,
    toks: Vec<Token>,
    pos: usize,
    depth: usize,
}

fn syntax(msg: impl Into<String>) -> MError {
    MError::new("Expression.SyntaxError", msg)
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("'{s}'"),
        Tok::Quoted(s) => format!("#\"{s}\""),
        Tok::Text(_) => "a text value".into(),
        Tok::Number(n) => format!("{n}"),
        Tok::Kw(k) => format!("'{k}'"),
        Tok::Hash(h) => format!("'#{h}'"),
        Tok::P(p) => format!("'{p}'"),
        Tok::Eof => "the end of the formula".into(),
    }
}

impl Parser<'_> {
    fn peek(&self) -> &Tok {
        self.peek_at(0)
    }
    fn peek_at(&self, n: usize) -> &Tok {
        self.toks.get(self.pos + n).or_else(|| self.toks.last()).map(|t| &t.tok).unwrap_or(&Tok::Eof)
    }
    fn bump(&mut self) -> Tok {
        let t = self.peek().clone();
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
        t
    }
    fn is_p(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::P(x) if *x == p)
    }
    fn eat_p(&mut self, p: &str) -> bool {
        let yes = self.is_p(p);
        if yes {
            self.pos += 1;
        }
        yes
    }
    fn expect_p(&mut self, p: &str) -> Result<(), MError> {
        if self.eat_p(p) { Ok(()) } else { Err(self.unexpected(&format!("'{p}'"))) }
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Kw(x) if *x == k)
    }
    fn eat_kw(&mut self, k: &str) -> bool {
        let yes = self.is_kw(k);
        if yes {
            self.pos += 1;
        }
        yes
    }
    fn expect_kw(&mut self, k: &str) -> Result<(), MError> {
        if self.eat_kw(k) { Ok(()) } else { Err(self.unexpected(&format!("'{k}'"))) }
    }
    fn unexpected(&self, wanted: &str) -> MError {
        syntax(format!("Expected {wanted}, found {}.", describe(self.peek())))
    }
    fn enter(&mut self) -> Result<(), MError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(syntax("The formula is nested too deeply.")) } else { Ok(()) }
    }
    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    // ------------------------------------------------------------ expressions

    fn expr(&mut self) -> Result<Expr, MError> {
        self.enter()?;
        let r = self.expr_inner();
        self.leave();
        r
    }

    fn expr_inner(&mut self) -> Result<Expr, MError> {
        match self.peek() {
            Tok::Kw("let") => {
                self.bump();
                let mut binds: Vec<(Rc<str>, Rc<Expr>)> = Vec::new();
                loop {
                    let name = self.variable_name()?;
                    self.expect_p("=")?;
                    let value = self.expr()?;
                    binds.push((name, Rc::new(value)));
                    if !self.eat_p(",") || self.is_kw("in") {
                        break;
                    }
                }
                self.expect_kw("in")?;
                let body = self.expr()?;
                Ok(Expr::Let(binds.into(), Box::new(body)))
            }
            Tok::Kw("if") => {
                self.bump();
                let c = self.expr()?;
                self.expect_kw("then")?;
                let a = self.expr()?;
                self.expect_kw("else")?;
                let b = self.expr()?;
                Ok(Expr::If(Box::new(c), Box::new(a), Box::new(b)))
            }
            Tok::Kw("each") => {
                self.bump();
                let body = self.expr()?;
                Ok(Expr::Func(Rc::new(FuncDef { params: vec![Param { name: "_".into(), optional: false }], body })))
            }
            Tok::Kw("try") => {
                self.bump();
                let body = Box::new(self.expr()?);
                if self.eat_kw("otherwise") {
                    let o = self.expr()?;
                    return Ok(Expr::Try { body, otherwise: Some(Box::new(o)), catch: None });
                }
                if self.eat_kw("catch") {
                    let f = self.try_function()?.ok_or_else(|| self.unexpected("a function after 'catch'"))?;
                    return Ok(Expr::Try { body, otherwise: None, catch: Some(f) });
                }
                Ok(Expr::Try { body, otherwise: None, catch: None })
            }
            Tok::Kw("error") => {
                self.bump();
                Ok(Expr::Error(Box::new(self.expr()?)))
            }
            Tok::P("(") => match self.try_function()? {
                Some(f) => Ok(Expr::Func(f)),
                None => self.coalesce(),
            },
            _ => self.coalesce(),
        }
    }

    /// `(a, optional b as text) as number => body`, or `None` (position unchanged) when the
    /// parenthesis starts something else.
    fn try_function(&mut self) -> Result<Option<Rc<FuncDef>>, MError> {
        let save = self.pos;
        let Some(params) = self.function_head() else {
            self.pos = save;
            return Ok(None);
        };
        let body = self.expr()?;
        Ok(Some(Rc::new(FuncDef { params, body })))
    }

    fn function_head(&mut self) -> Option<Vec<Param>> {
        if !self.eat_p("(") {
            return None;
        }
        let mut params = Vec::new();
        if !self.eat_p(")") {
            loop {
                let optional = matches!(self.peek(), Tok::Ident(s) if s == "optional") && matches!(self.peek_at(1), Tok::Ident(_) | Tok::Quoted(_));
                if optional {
                    self.bump();
                }
                let name: Rc<str> = match self.bump() {
                    Tok::Ident(s) | Tok::Quoted(s) => s.into(),
                    _ => return None,
                };
                if self.eat_kw("as") {
                    self.type_expr().ok()?;
                }
                params.push(Param { name, optional });
                if self.eat_p(",") {
                    continue;
                }
                if !self.eat_p(")") {
                    return None;
                }
                break;
            }
        }
        if self.eat_kw("as") {
            self.type_expr().ok()?;
        }
        if !self.eat_p("=>") {
            return None;
        }
        Some(params)
    }

    fn variable_name(&mut self) -> Result<Rc<str>, MError> {
        match self.peek().clone() {
            Tok::Ident(s) | Tok::Quoted(s) => {
                self.bump();
                Ok(s.into())
            }
            _ => Err(self.unexpected("a name")),
        }
    }

    fn coalesce(&mut self) -> Result<Expr, MError> {
        let l = self.or()?;
        if self.eat_p("??") {
            self.enter()?;
            let r = self.coalesce();
            self.leave();
            return Ok(Expr::Binary(BinOp::Coalesce, Box::new(l), Box::new(r?)));
        }
        Ok(l)
    }

    fn or(&mut self) -> Result<Expr, MError> {
        let mut l = self.and()?;
        while self.eat_kw("or") {
            let r = self.and()?;
            l = Expr::Binary(BinOp::Or, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn and(&mut self) -> Result<Expr, MError> {
        let mut l = self.is_as()?;
        while self.eat_kw("and") {
            let r = self.is_as()?;
            l = Expr::Binary(BinOp::And, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn is_as(&mut self) -> Result<Expr, MError> {
        let mut l = self.equality()?;
        loop {
            if self.eat_kw("is") {
                let t = self.type_expr()?;
                l = Expr::Is(Box::new(l), Rc::new(t));
            } else if self.eat_kw("as") {
                let t = self.type_expr()?;
                l = Expr::As(Box::new(l), Rc::new(t));
            } else {
                return Ok(l);
            }
        }
    }

    fn equality(&mut self) -> Result<Expr, MError> {
        let mut l = self.relational()?;
        loop {
            let op = if self.eat_p("=") {
                BinOp::Eq
            } else if self.eat_p("<>") {
                BinOp::Ne
            } else {
                return Ok(l);
            };
            let r = self.relational()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
    }

    fn relational(&mut self) -> Result<Expr, MError> {
        let mut l = self.additive()?;
        loop {
            let op = if self.eat_p("<") {
                BinOp::Lt
            } else if self.eat_p("<=") {
                BinOp::Le
            } else if self.eat_p(">") {
                BinOp::Gt
            } else if self.eat_p(">=") {
                BinOp::Ge
            } else {
                return Ok(l);
            };
            let r = self.additive()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
    }

    fn additive(&mut self) -> Result<Expr, MError> {
        let mut l = self.multiplicative()?;
        loop {
            let op = if self.eat_p("+") {
                BinOp::Add
            } else if self.eat_p("-") {
                BinOp::Sub
            } else if self.eat_p("&") {
                BinOp::Concat
            } else {
                return Ok(l);
            };
            let r = self.multiplicative()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, MError> {
        let mut l = self.metadata()?;
        loop {
            let op = if self.eat_p("*") {
                BinOp::Mul
            } else if self.eat_p("/") {
                BinOp::Div
            } else {
                return Ok(l);
            };
            let r = self.metadata()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
    }

    fn metadata(&mut self) -> Result<Expr, MError> {
        let mut l = self.unary()?;
        while self.eat_kw("meta") {
            let r = self.unary()?;
            l = Expr::Meta(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn unary(&mut self) -> Result<Expr, MError> {
        self.enter()?;
        let r = self.unary_inner();
        self.leave();
        r
    }

    fn unary_inner(&mut self) -> Result<Expr, MError> {
        match self.peek() {
            Tok::P("+") => {
                self.bump();
                Ok(Expr::Unary(UnOp::Plus, Box::new(self.unary()?)))
            }
            Tok::P("-") => {
                self.bump();
                Ok(Expr::Unary(UnOp::Minus, Box::new(self.unary()?)))
            }
            Tok::Kw("not") => {
                self.bump();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?)))
            }
            Tok::Kw("type") => {
                self.bump();
                Ok(Expr::Type(Rc::new(self.type_expr()?)))
            }
            // Leniency: these are expressions of their own, written without parentheses.
            Tok::Kw("let" | "if" | "each" | "try" | "error") => self.expr(),
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Result<Expr, MError> {
        let mut e = self.primary()?;
        loop {
            if self.is_p("[") {
                self.bump();
                if self.is_p("[") {
                    let names = self.projection()?;
                    let optional = self.eat_p("?");
                    e = Expr::Project { target: Some(Box::new(e)), names, optional };
                } else {
                    let name = self.field_name()?;
                    self.expect_p("]")?;
                    let optional = self.eat_p("?");
                    e = Expr::Field { target: Some(Box::new(e)), name, optional };
                }
            } else if self.eat_p("{") {
                let key = self.expr()?;
                self.expect_p("}")?;
                let optional = self.eat_p("?");
                e = Expr::Item { target: Box::new(e), key: Box::new(key), optional };
            } else if self.eat_p("(") {
                let mut args = Vec::new();
                if !self.eat_p(")") {
                    loop {
                        args.push(self.expr()?);
                        if self.eat_p(",") {
                            continue;
                        }
                        self.expect_p(")")?;
                        break;
                    }
                }
                e = Expr::Call(Box::new(e), args);
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, MError> {
        match self.peek().clone() {
            Tok::Number(n) => {
                self.bump();
                Ok(Expr::Num(n))
            }
            Tok::Text(s) => {
                self.bump();
                Ok(Expr::Text(s.into()))
            }
            Tok::Kw("true") => {
                self.bump();
                Ok(Expr::Bool(true))
            }
            Tok::Kw("false") => {
                self.bump();
                Ok(Expr::Bool(false))
            }
            Tok::Kw("null") => {
                self.bump();
                Ok(Expr::Null)
            }
            Tok::Ident(s) | Tok::Quoted(s) => {
                self.bump();
                Ok(Expr::Ident(s.into()))
            }
            Tok::P("@") => {
                self.bump();
                match self.bump() {
                    Tok::Ident(s) | Tok::Quoted(s) => Ok(Expr::Ident(s.into())),
                    _ => Err(syntax("Expected a name after '@'.")),
                }
            }
            Tok::Hash(h) => {
                self.bump();
                match h.as_str() {
                    "infinity" => Ok(Expr::Num(f64::INFINITY)),
                    "nan" => Ok(Expr::Num(f64::NAN)),
                    "date" | "datetime" | "datetimezone" | "time" | "duration" | "table" | "binary" | "shared" | "sections" => {
                        Ok(Expr::Ident(format!("#{h}").into()))
                    }
                    _ => Err(syntax(format!("'#{h}' isn't an M keyword."))),
                }
            }
            Tok::P("(") => {
                self.bump();
                let e = self.expr()?;
                self.expect_p(")")?;
                Ok(e)
            }
            Tok::P("{") => {
                self.bump();
                let mut items = Vec::new();
                if !self.eat_p("}") {
                    loop {
                        let a = self.expr()?;
                        if self.eat_p("..") {
                            let b = self.expr()?;
                            items.push(ListItem::Range(a, b));
                        } else {
                            items.push(ListItem::One(a));
                        }
                        if self.eat_p(",") {
                            continue;
                        }
                        self.expect_p("}")?;
                        break;
                    }
                }
                Ok(Expr::List(items))
            }
            Tok::P("[") => {
                self.bump();
                if self.eat_p("]") {
                    return Ok(Expr::Record(Rc::from(Vec::new())));
                }
                if self.is_p("[") {
                    let names = self.projection()?;
                    let optional = self.eat_p("?");
                    return Ok(Expr::Project { target: None, names, optional });
                }
                let first = self.field_name()?;
                if self.eat_p("=") {
                    let mut fields: Vec<(Rc<str>, Rc<Expr>)> = vec![(first, Rc::new(self.expr()?))];
                    while self.eat_p(",") {
                        let name = self.field_name()?;
                        self.expect_p("=")?;
                        fields.push((name, Rc::new(self.expr()?)));
                    }
                    self.expect_p("]")?;
                    return Ok(Expr::Record(fields.into()));
                }
                self.expect_p("]")?;
                let optional = self.eat_p("?");
                Ok(Expr::Field { target: None, name: first, optional })
            }
            Tok::P("...") => {
                self.bump();
                Ok(Expr::NotImplemented)
            }
            _ => Err(self.unexpected("an expression")),
        }
    }

    /// `[A], [B]]` after the projection's first `[`: the names (both brackets consumed).
    fn projection(&mut self) -> Result<Vec<Rc<str>>, MError> {
        let mut names = Vec::new();
        loop {
            self.expect_p("[")?;
            names.push(self.field_name()?);
            self.expect_p("]")?;
            if self.eat_p(",") {
                continue;
            }
            self.expect_p("]")?;
            return Ok(names);
        }
    }

    /// A field name: a quoted identifier or a generalized identifier (`Order Date`,
    /// `Column1.1`), taken from the formula text as written.
    fn field_name(&mut self) -> Result<Rc<str>, MError> {
        if let Tok::Quoted(s) = self.peek().clone() {
            self.bump();
            return Ok(s.into());
        }
        let first = self.pos;
        while matches!(self.peek(), Tok::Ident(_) | Tok::Kw(_) | Tok::Number(_)) {
            self.pos += 1;
        }
        if self.pos == first {
            return Err(self.unexpected("a field name"));
        }
        let start = self.toks.get(first).map(|t| t.start).unwrap_or(0);
        let end = self.toks.get(self.pos - 1).map(|t| t.end).unwrap_or(start);
        Ok(self.src.get(start..end).unwrap_or("").trim().into())
    }

    // ------------------------------------------------------------ types

    fn type_expr(&mut self) -> Result<TypeExpr, MError> {
        self.enter()?;
        let r = self.type_inner();
        self.leave();
        r
    }

    fn type_inner(&mut self) -> Result<TypeExpr, MError> {
        match self.peek().clone() {
            Tok::Ident(s) if s == "nullable" => {
                self.bump();
                Ok(TypeExpr::Nullable(Box::new(self.type_expr()?)))
            }
            Tok::Ident(s) if s == "table" && matches!(self.peek_at(1), Tok::P("[")) => {
                self.bump();
                self.bump();
                Ok(TypeExpr::Table(self.field_types()?))
            }
            Tok::Ident(s) if s == "function" && matches!(self.peek_at(1), Tok::P("(")) => {
                self.bump();
                let mut level = 0usize;
                loop {
                    match self.bump() {
                        Tok::P("(") => level += 1,
                        Tok::P(")") => {
                            level = level.saturating_sub(1);
                            if level == 0 {
                                break;
                            }
                        }
                        Tok::Eof => return Err(self.unexpected("')'")),
                        _ => {}
                    }
                }
                if self.eat_kw("as") {
                    self.type_expr()?;
                }
                Ok(TypeExpr::Function)
            }
            Tok::Ident(s) => match PRIMITIVE_TYPES.iter().find(|p| **p == s) {
                Some(p) => {
                    self.bump();
                    Ok(TypeExpr::Prim(p))
                }
                None => Ok(TypeExpr::Expr(Box::new(self.postfix()?))),
            },
            Tok::Kw("null") => {
                self.bump();
                Ok(TypeExpr::Prim("null"))
            }
            Tok::Kw("type") => {
                self.bump();
                Ok(TypeExpr::Prim("type"))
            }
            Tok::P("{") => {
                self.bump();
                let t = self.type_expr()?;
                self.expect_p("}")?;
                Ok(TypeExpr::List(Box::new(t)))
            }
            Tok::P("[") => {
                self.bump();
                Ok(TypeExpr::Record(self.field_types()?))
            }
            _ => Ok(TypeExpr::Expr(Box::new(self.postfix()?))),
        }
    }

    /// `A = text, optional B = number, C, ...]` after a record or table type's `[`.
    fn field_types(&mut self) -> Result<Vec<(Rc<str>, TypeExpr)>, MError> {
        let mut fields = Vec::new();
        if self.eat_p("]") {
            return Ok(fields);
        }
        loop {
            if self.eat_p("...") {
                self.expect_p("]")?;
                return Ok(fields);
            }
            if matches!(self.peek(), Tok::Ident(s) if s == "optional") && !matches!(self.peek_at(1), Tok::P("=" | "," | "]")) {
                self.bump();
            }
            let name = self.field_name()?;
            let t = if self.eat_p("=") { self.type_expr()? } else { TypeExpr::Prim("any") };
            fields.push((name, t));
            if self.eat_p(",") {
                continue;
            }
            self.expect_p("]")?;
            return Ok(fields);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excel_generated_queries_parse() {
        let q = r#"let
    Source = Excel.CurrentWorkbook(){[Name="Sales"]}[Content],
    #"Changed Type" = Table.TransformColumnTypes(Source,{{"Region", type text}, {"Amount", Int64.Type}}),
    #"Filtered Rows" = Table.SelectRows(#"Changed Type", each ([Region] = "North") and [Amount] > 100),
    #"Added Custom" = Table.AddColumn(#"Filtered Rows", "Big", each if [Amount] >= 1000 then "yes" else "no", type text),
    #"Kept" = Table.SelectColumns(#"Added Custom", {"Region", "Order Date"}),
    Typed = #table(type table [A = text, B = nullable number], {{"x", 1}}),
    Enter = Table.FromRows(Json.Document(Binary.Decompress(Binary.FromText("i45W", BinaryEncoding.Base64), Compression.Deflate)), let _t = ((type nullable text) meta [Serialized.Text = true]) in type table [Column1 = _t, #"Column 2" = _t]),
    f = (x as number, optional y as nullable text) as number => x * 2,
    r = [a = 1, b = a + 1][b],
    l = {1..3, 5},
    t = try 1/0 otherwise null
in
    #"Kept""#;
        let e = parse(q).unwrap();
        let Expr::Let(binds, _) = e else { panic!("not a let") };
        assert_eq!(binds.len(), 11);
        assert_eq!(&*binds[1].0, "Changed Type");
    }

    #[test]
    fn implicit_fields_and_generalized_names() {
        let Expr::Func(f) = parse("each [Order Date] & [Column1.1] & _[x]?").unwrap() else { panic!() };
        let Expr::Binary(BinOp::Concat, l, _) = &f.body else { panic!("{:?}", f.body) };
        let Expr::Binary(BinOp::Concat, a, b) = &**l else { panic!() };
        assert!(matches!(&**a, Expr::Field { target: None, name, .. } if &**name == "Order Date"));
        assert!(matches!(&**b, Expr::Field { target: None, name, .. } if &**name == "Column1.1"));
    }

    #[test]
    fn precedence() {
        let Expr::Binary(BinOp::Or, _, r) = parse("a or b and c = 1 + 2 * 3").unwrap() else { panic!() };
        let Expr::Binary(BinOp::And, _, r) = *r else { panic!() };
        let Expr::Binary(BinOp::Eq, _, r) = *r else { panic!() };
        assert!(matches!(*r, Expr::Binary(BinOp::Add, _, _)));
        assert!(matches!(parse("(x) => x").unwrap(), Expr::Func(_)));
        assert!(matches!(parse("(x)").unwrap(), Expr::Ident(_)));
    }

    #[test]
    fn hostile_input_fails_cleanly() {
        let deep = "(".repeat(100_000);
        assert!(parse(&deep).is_err());
        let deep_list = "{".repeat(100_000);
        assert!(parse(&deep_list).is_err());
        let minus = "-".repeat(100_000) + "1";
        assert!(parse(&minus).is_err());
        for s in ["let", "let a = 1 in", "if a then", "[a=", "x{", "f(1,", "type table [", "each", "try", "(a, b) =>", "[[a]", "#foo", "a b"] {
            assert!(parse(s).is_err(), "{s}");
        }
    }
}
