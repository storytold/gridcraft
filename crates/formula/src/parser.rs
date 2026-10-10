//! Pratt parser: tokens → [`Expr`].

use std::sync::Arc;

use gridcraft_core::addr::{letters_to_col, parse_a1_prefix};
use gridcraft_core::{CellError, MAX_ROWS};

use crate::ast::*;
use crate::lexer::{Tok, Token, tokenize};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{msg}")]
pub struct ParseError {
    pub msg: String,
    pub pos: usize,
}

const MAX_DEPTH: usize = 256;

/// Parses formula text. A leading `=` (or `+`/`-` as Lotus users type) is accepted and skipped
/// for `=`.
pub fn parse(src: &str) -> Result<Expr, ParseError> {
    let body = src.strip_prefix('=').unwrap_or(src);
    let toks = tokenize(body).map_err(|e| ParseError { msg: e.msg, pos: e.pos })?;
    let toks = significant_spaces(toks);
    let mut p = Parser { toks, i: 0, depth: 0 };
    let e = p.expr(0, false)?;
    match p.peek() {
        Tok::Eof => Ok(e),
        t => Err(p.err(format!("unexpected {}", describe(t)))),
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::RParen => "`)`".into(),
        Tok::Comma => "`,`".into(),
        Tok::Eof => "end of formula".into(),
        other => format!("{other:?}"),
    }
}

/// Keeps a space token only where it is the intersection operator: between the end of an
/// operand and the start of another.
fn significant_spaces(toks: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(toks.len());
    for (i, t) in toks.iter().enumerate() {
        if t.tok == Tok::Space {
            let prev_end = out.last().is_some_and(|p| matches!(p.tok, Tok::Word(_) | Tok::RParen | Tok::Struct(_)));
            let next_start = toks.get(i + 1).is_some_and(|n| matches!(n.tok, Tok::Word(_) | Tok::Sheet(..) | Tok::LParen | Tok::Func(_)));
            if prev_end && next_start {
                out.push(t.clone());
            }
            continue;
        }
        out.push(t.clone());
    }
    out
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
    depth: usize,
}

fn word_is_cols(w: &str) -> Option<(u32, bool)> {
    let abs = w.starts_with('$');
    letters_to_col(w.trim_start_matches('$')).map(|c| (c, abs))
}

fn word_is_row(w: &str) -> Option<(u32, bool)> {
    let abs = w.starts_with('$');
    let n: u32 = w.trim_start_matches('$').parse().ok()?;
    if n == 0 || n > MAX_ROWS { None } else { Some((n - 1, abs)) }
}

fn word_is_cell(w: &str) -> Option<Anchor> {
    let (col, row, col_abs, row_abs, rest) = parse_a1_prefix(w)?;
    if !rest.is_empty() {
        return None;
    }
    Some(Anchor { row, col, row_abs, col_abs })
}

/// `_xlfn.XLOOKUP` → `XLOOKUP`.
pub fn normalize_function_name(name: &str) -> String {
    let u = name.to_ascii_uppercase();
    for p in ["_XLFN._XLWS.", "_XLFN.", "_XLWS.", "_XLUDF."] {
        if let Some(r) = u.strip_prefix(p) {
            return r.to_string();
        }
    }
    u
}

impl Parser {
    fn peek(&self) -> &Tok {
        self.toks.get(self.i).map(|t| &t.tok).unwrap_or(&Tok::Eof)
    }
    fn peek_at(&self, k: usize) -> &Tok {
        self.toks.get(self.i + k).map(|t| &t.tok).unwrap_or(&Tok::Eof)
    }
    fn pos(&self) -> usize {
        self.toks.get(self.i).map(|t| t.start).unwrap_or(0)
    }
    fn next(&mut self) -> Tok {
        let t = self.peek().clone();
        if self.i < self.toks.len() {
            self.i += 1;
        }
        t
    }
    fn err(&self, msg: impl Into<String>) -> ParseError {
        ParseError { msg: msg.into(), pos: self.pos() }
    }
    fn expect(&mut self, t: Tok) -> Result<(), ParseError> {
        if *self.peek() == t {
            self.next();
            Ok(())
        } else {
            Err(self.err(format!("expected {}, found {}", describe(&t), describe(self.peek()))))
        }
    }

    fn binop(&self, allow_union: bool) -> Option<BinOp> {
        Some(match self.peek() {
            Tok::Op("+") => BinOp::Add,
            Tok::Op("-") => BinOp::Sub,
            Tok::Op("*") => BinOp::Mul,
            Tok::Op("/") => BinOp::Div,
            Tok::Op("^") => BinOp::Pow,
            Tok::Op("&") => BinOp::Concat,
            Tok::Op("=") => BinOp::Eq,
            Tok::Op("<>") => BinOp::Ne,
            Tok::Op("<") => BinOp::Lt,
            Tok::Op(">") => BinOp::Gt,
            Tok::Op("<=") => BinOp::Le,
            Tok::Op(">=") => BinOp::Ge,
            Tok::Colon => BinOp::Range,
            Tok::Space => BinOp::Intersect,
            Tok::Comma if allow_union => BinOp::Union,
            _ => return None,
        })
    }

    fn expr(&mut self, min_bp: u8, allow_union: bool) -> Result<Expr, ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err("formula is nested too deeply"));
        }
        let mut lhs = self.prefix(allow_union)?;
        loop {
            if *self.peek() == Tok::Op("%") {
                if 70 < min_bp {
                    break;
                }
                self.next();
                lhs = Expr::Unary(UnOp::Percent, Box::new(lhs));
                continue;
            }
            if *self.peek() == Tok::LParen && matches!(lhs, Expr::Call(..) | Expr::Invoke(..) | Expr::Paren(_) | Expr::Name(_)) {
                // Invoking a LAMBDA result.
                if let Expr::Name(_) = lhs {
                    // A name followed by '(' was lexed as Func; this is `(name)(…)` only.
                }
                self.next();
                let args = self.args()?;
                lhs = Expr::Invoke(Box::new(lhs), args);
                continue;
            }
            let Some(op) = self.binop(allow_union) else { break };
            let bp = op.precedence();
            if bp < min_bp {
                break;
            }
            self.next();
            let rhs = self.expr(bp + 1, allow_union)?;
            lhs = combine(op, lhs, rhs);
        }
        self.depth -= 1;
        Ok(lhs)
    }

    fn args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if *self.peek() == Tok::RParen {
            self.next();
            return Ok(args);
        }
        loop {
            let a = match self.peek() {
                Tok::Comma | Tok::RParen => Expr::Missing,
                _ => self.expr(0, false)?,
            };
            args.push(a);
            match self.next() {
                Tok::Comma => continue,
                Tok::RParen => break,
                t => return Err(self.err(format!("expected `,` or `)`, found {}", describe(&t)))),
            }
        }
        if args.len() > 255 {
            return Err(self.err("too many arguments"));
        }
        Ok(args)
    }

    fn prefix(&mut self, allow_union: bool) -> Result<Expr, ParseError> {
        match self.next() {
            Tok::Number(n) => Ok(Expr::Number(n)),
            Tok::Text(s) => Ok(Expr::Text(Arc::from(s))),
            Tok::Bool(b) => Ok(Expr::Bool(b)),
            Tok::Error(e) => Ok(Expr::Error(e)),
            Tok::Op("-") => Ok(Expr::Unary(UnOp::Neg, Box::new(self.expr(60, allow_union)?))),
            Tok::Op("+") => Ok(Expr::Unary(UnOp::Plus, Box::new(self.expr(60, allow_union)?))),
            Tok::Op("@") => Ok(Expr::Unary(UnOp::At, Box::new(self.expr(95, allow_union)?))),
            Tok::LParen => {
                let e = self.expr(0, true)?;
                self.expect(Tok::RParen)?;
                Ok(Expr::Paren(Box::new(e)))
            }
            Tok::LBrace => self.array(),
            Tok::Func(name) => {
                let name = normalize_function_name(&name);
                let mut args = self.args()?;
                // Files write `A1#` as `_xlfn.ANCHORARRAY(A1)`.
                if name == "ANCHORARRAY"
                    && let [Expr::Ref(Reference { kind: RefKind::Cell(_), .. })] = args.as_slice()
                    && let Some(r) = args.pop()
                {
                    return Ok(Expr::Unary(UnOp::Spill, Box::new(r)));
                }
                Ok(Expr::Call(name, args))
            }
            Tok::Sheet(a, b) => {
                let sheet = match b {
                    Some(b) => SheetSel::Span(a, b),
                    None => SheetSel::Named(a),
                };
                match self.next() {
                    Tok::Word(w) => self.reference_from_word(sheet, &w),
                    Tok::Error(CellError::Ref) => Ok(Expr::Error(CellError::Ref)),
                    t => Err(self.err(format!("expected a reference after the sheet name, found {}", describe(&t)))),
                }
            }
            Tok::Word(w) => {
                if let Tok::Struct(_) = self.peek()
                    && word_is_cell(&w).is_none()
                {
                    let Tok::Struct(body) = self.next() else { return Err(self.err("internal")) };
                    return Ok(Expr::Struct(parse_struct(&w, &body)));
                }
                self.reference_from_word(SheetSel::Current, &w)
            }
            Tok::Struct(body) => Ok(Expr::Struct(parse_struct("", &body))),
            t => Err(self.err(format!("unexpected {}", describe(&t)))),
        }
    }

    /// A word after an optional sheet: a cell, a column/row range (`A:C`, `3:5`) or a name.
    fn reference_from_word(&mut self, sheet: SheetSel, w: &str) -> Result<Expr, ParseError> {
        if let Some(a) = word_is_cell(w) {
            // Cell, possibly `A1:B2` (merged here so the sheet applies to both ends).
            if *self.peek() == Tok::Colon
                && let Tok::Word(w2) = self.peek_at(1)
                && let Some(b) = word_is_cell(w2)
            {
                self.next();
                self.next();
                return Ok(Expr::Ref(Reference { sheet, kind: RefKind::Range(a, b) }));
            }
            let cell = Expr::Ref(Reference { sheet, kind: RefKind::Cell(a) });
            if *self.peek() == Tok::Op("#") {
                self.next();
                return Ok(Expr::Unary(UnOp::Spill, Box::new(cell)));
            }
            return Ok(cell);
        }
        let second = match self.peek_at(1) {
            Tok::Word(w2) => Some(w2.clone()),
            Tok::Number(n) if n.fract() == 0.0 && *n >= 1.0 => Some(format!("{n}")),
            _ => None,
        };
        if *self.peek() == Tok::Colon
            && let Some(w2) = second
        {
            if let (Some((c0, a0)), Some((c1, a1))) = (word_is_cols(w), word_is_cols(&w2)) {
                self.next();
                self.next();
                return Ok(Expr::Ref(Reference { sheet, kind: RefKind::Cols(c0, a0, c1, a1) }));
            }
            if let (Some((r0, a0)), Some((r1, a1))) = (word_is_row(w), word_is_row(&w2)) {
                self.next();
                self.next();
                return Ok(Expr::Ref(Reference { sheet, kind: RefKind::Rows(r0, a0, r1, a1) }));
            }
        }
        if sheet != SheetSel::Current {
            // Sheet-scoped name `Sheet1!MyName`.
            if let SheetSel::Named(s) = sheet {
                return Ok(Expr::Name(format!("{s}!{w}")));
            }
            return Err(self.err("invalid 3-D reference"));
        }
        if w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Err(self.err(format!("unexpected `{w}`")));
        }
        Ok(Expr::Name(w.to_string()))
    }

    fn array(&mut self) -> Result<Expr, ParseError> {
        let mut rows = vec![Vec::new()];
        loop {
            let el = match self.next() {
                Tok::Number(n) => Expr::Number(n),
                Tok::Op("-") => match self.next() {
                    Tok::Number(n) => Expr::Number(-n),
                    t => return Err(self.err(format!("expected a number in array, found {}", describe(&t)))),
                },
                Tok::Op("+") => match self.next() {
                    Tok::Number(n) => Expr::Number(n),
                    t => return Err(self.err(format!("expected a number in array, found {}", describe(&t)))),
                },
                Tok::Text(s) => Expr::Text(Arc::from(s)),
                Tok::Bool(b) => Expr::Bool(b),
                Tok::Error(e) => Expr::Error(e),
                t => return Err(self.err(format!("array constants may only contain constants, found {}", describe(&t)))),
            };
            if let Some(r) = rows.last_mut() {
                r.push(el);
            }
            match self.next() {
                Tok::Comma => {}
                Tok::Semicolon => rows.push(Vec::new()),
                Tok::RBrace => break,
                t => return Err(self.err(format!("expected `,`, `;` or `}}`, found {}", describe(&t)))),
            }
        }
        let w = rows.first().map_or(0, Vec::len);
        if rows.iter().any(|r| r.len() != w) {
            return Err(self.err("array rows must have the same length"));
        }
        Ok(Expr::Array(rows))
    }
}

/// Folds `A1:B2`-like range operations of two plain refs into a single reference.
fn combine(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    if op == BinOp::Range
        && let (Expr::Ref(a), Expr::Ref(b)) = (&lhs, &rhs)
        && b.sheet == SheetSel::Current
        && let (RefKind::Cell(x), RefKind::Cell(y)) = (&a.kind, &b.kind)
    {
        return Expr::Ref(Reference { sheet: a.sheet.clone(), kind: RefKind::Range(*x, *y) });
    }
    Expr::Binary(op, Box::new(lhs), Box::new(rhs))
}

/// Parses the inside of `Table[...]`.
pub fn parse_struct(table: &str, body: &str) -> StructRef {
    let mut specifiers = Vec::new();
    let mut cols: Vec<String> = Vec::new();
    let body = body.trim();
    let unescape = |s: &str| {
        let mut out = String::new();
        let mut it = s.chars();
        while let Some(c) = it.next() {
            if c == '\'' {
                if let Some(n) = it.next() {
                    out.push(n);
                }
            } else {
                out.push(c);
            }
        }
        out
    };
    let item = |s: &str| -> Option<StructItem> {
        match s.trim().to_ascii_lowercase().as_str() {
            "#all" => Some(StructItem::All),
            "#data" => Some(StructItem::Data),
            "#headers" => Some(StructItem::Headers),
            "#totals" => Some(StructItem::Totals),
            "#this row" => Some(StructItem::ThisRow),
            _ => None,
        }
    };
    if !body.starts_with('[') {
        // Simple: [Col], [@Col], [#Headers], [@]
        if let Some(rest) = body.strip_prefix('@') {
            specifiers.push(StructItem::ThisRow);
            let rest = rest.trim().trim_start_matches('[').trim_end_matches(']');
            if !rest.is_empty() {
                cols.push(unescape(rest));
            }
        } else if let Some(i) = item(body) {
            specifiers.push(i);
        } else if !body.is_empty() {
            cols.push(unescape(body));
        }
    } else {
        // [[#Headers],[Col1]:[Col3]] or [@[Col 1]]
        let mut parts = Vec::new();
        let mut depth = 0;
        let mut cur = String::new();
        let mut esc = false;
        for c in body.chars() {
            if esc {
                cur.push(c);
                esc = false;
                continue;
            }
            match c {
                '\'' => {
                    cur.push(c);
                    esc = true;
                }
                '[' => {
                    depth += 1;
                    if depth > 1 {
                        cur.push(c);
                    }
                }
                ']' => {
                    depth -= 1;
                    if depth > 0 {
                        cur.push(c);
                    } else {
                        parts.push(std::mem::take(&mut cur));
                    }
                }
                ',' if depth == 0 => {}
                ':' if depth == 0 => parts.push(":".into()),
                _ if depth > 0 => cur.push(c),
                _ => {}
            }
        }
        let mut range_next = false;
        for p in parts {
            if p == ":" {
                range_next = true;
                continue;
            }
            if let Some(i) = item(&p) {
                specifiers.push(i);
            } else if range_next {
                cols.push(unescape(&p));
                range_next = false;
            } else {
                cols.push(unescape(&p));
            }
        }
        if body.starts_with("@") {
            specifiers.push(StructItem::ThisRow);
        }
    }
    let mut it = cols.into_iter();
    let col_start = it.next();
    let col_end = it.next();
    StructRef { table: table.to_string(), specifiers, col_start, col_end }
}
