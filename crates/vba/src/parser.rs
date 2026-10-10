//! Finds `Sub`s in a module's source text, and parses one `Sub`'s body into a [`Stmt`] block.

use crate::ast::{BinOp, CellExpr, Expr, Place, Stmt, UnOp, Value};
use crate::lexer::{self, Token};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VBA parse error: {}", self.0)
    }
}

/// Finds every zero-argument `Sub NAME()` ... `End Sub` block in `source` (top level only — VBA
/// doesn't nest `Sub`s). Returns `(name, body source)` pairs; `body source` excludes the `Sub`
/// and `End Sub` lines themselves.
pub fn subs(source: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some((name, args)) = sub_header(lines[i].trim()) {
            let mut j = i + 1;
            while j < lines.len() && !lines[j].trim().eq_ignore_ascii_case("end sub") {
                j += 1;
            }
            if args.trim().is_empty() {
                out.push((name, lines[(i + 1).min(j)..j].join("\n")));
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn sub_header(line: &str) -> Option<(String, String)> {
    let mut s = line;
    loop {
        let mut advanced = false;
        for prefix in ["Public ", "Private ", "Static "] {
            if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
                s = s[prefix.len()..].trim_start();
                advanced = true;
            }
        }
        if !advanced {
            break;
        }
    }
    if s.len() < 3 || !s.as_bytes()[..3].eq_ignore_ascii_case(b"sub") {
        return None;
    }
    let rest = s.get(3..)?.trim_start();
    let open = rest.find('(')?;
    let name = rest[..open].trim();
    if name.is_empty() || !name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') {
        return None;
    }
    let close = rest.get(open..)?.find(')')? + open;
    let args = rest.get(open + 1..close)?;
    Some((name.to_string(), args.to_string()))
}

/// Parses one `Sub`'s body (as returned by [`subs`]) into statements.
pub fn parse_block(body_source: &str) -> Result<Vec<Stmt>, ParseError> {
    let tokens = lexer::lex(body_source);
    let mut p = Parser { tokens, pos: 0 };
    let block = p.block(&[])?;
    Ok(block)
}

/// An unresolved reference: either a plain variable, or a (possibly sheet-qualified) cell.
enum Ref {
    Var(String),
    Cell { sheet: Option<Expr>, cell: CellExpr },
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        t
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Token::Ident(s) if s.eq_ignore_ascii_case(kw))
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_kw(&mut self, kw: &str) -> Result<(), ParseError> {
        if self.eat_kw(kw) { Ok(()) } else { Err(ParseError(format!("expected `{kw}`, found {:?}", self.peek()))) }
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek(), Token::Newline | Token::Colon) {
            self.pos += 1;
        }
    }

    /// True when the parser is at a block terminator (one of `terminators`, matched
    /// case-insensitively) or end of input — i.e. the current `block` should stop.
    fn at_terminator(&self, terminators: &[&str]) -> bool {
        matches!(self.peek(), Token::Eof) || terminators.iter().any(|t| self.is_kw(t))
    }

    fn block(&mut self, terminators: &[&str]) -> Result<Vec<Stmt>, ParseError> {
        let mut out = Vec::new();
        loop {
            self.skip_separators();
            if self.at_terminator(terminators) {
                return Ok(out);
            }
            out.push(self.statement()?);
        }
    }

    fn statement(&mut self) -> Result<Stmt, ParseError> {
        if self.eat_kw("dim") {
            let name = self.ident()?;
            if self.eat_kw("as") {
                self.ident()?; // Type name: not enforced, VBA variants hold anything.
            }
            return Ok(Stmt::Dim(name));
        }
        if self.eat_kw("if") {
            return self.if_stmt();
        }
        if self.eat_kw("for") {
            return self.for_stmt();
        }
        if self.eat_kw("msgbox") {
            let wrapped = self.eat(Token::LParen);
            let e = self.expr()?;
            if wrapped {
                self.expect(Token::RParen)?;
            }
            return Ok(Stmt::MsgBox(e));
        }
        // Anything else is an assignment: `<place> = <expr>`.
        let place = self.place()?;
        self.expect(Token::Eq)?;
        let rhs = self.expr()?;
        Ok(Stmt::Assign(place, rhs))
    }

    fn if_stmt(&mut self) -> Result<Stmt, ParseError> {
        let cond = self.expr()?;
        self.expect_kw("then")?;
        let mut branches = Vec::new();
        if matches!(self.peek(), Token::Newline) {
            // Block form.
            self.skip_separators();
            let body = self.block(&["elseif", "else", "end"])?;
            branches.push((cond, body));
            while self.eat_kw("elseif") {
                let c = self.expr()?;
                self.expect_kw("then")?;
                self.skip_separators();
                let b = self.block(&["elseif", "else", "end"])?;
                branches.push((c, b));
            }
            let else_body = if self.eat_kw("else") {
                self.skip_separators();
                self.block(&["end"])?
            } else {
                Vec::new()
            };
            self.expect_kw("end")?;
            self.expect_kw("if")?;
            Ok(Stmt::If { branches, else_body })
        } else {
            // Single-line form: `If x Then stmt [: stmt...] [Else stmt [: stmt...]]`.
            let mut then_body = Vec::new();
            loop {
                then_body.push(self.statement()?);
                if self.eat(Token::Colon) {
                    continue;
                }
                break;
            }
            let else_body = if self.eat_kw("else") {
                let mut v = Vec::new();
                loop {
                    v.push(self.statement()?);
                    if self.eat(Token::Colon) {
                        continue;
                    }
                    break;
                }
                v
            } else {
                Vec::new()
            };
            branches.push((cond, then_body));
            Ok(Stmt::If { branches, else_body })
        }
    }

    fn for_stmt(&mut self) -> Result<Stmt, ParseError> {
        let var = self.ident()?;
        self.expect(Token::Eq)?;
        let start = self.expr()?;
        self.expect_kw("to")?;
        let end = self.expr()?;
        let step = if self.eat_kw("step") { Some(self.expr()?) } else { None };
        self.skip_separators();
        let body = self.block(&["next"])?;
        self.expect_kw("next")?;
        // `Next i` — the loop variable name after `Next` is optional; consume it if present.
        if let Token::Ident(_) = self.peek() {
            self.pos += 1;
        }
        Ok(Stmt::For { var, start, end, step, body })
    }

    // ---- references (variables and cells) ----

    fn parse_ref(&mut self) -> Result<Ref, ParseError> {
        // A sheet qualifier: `Sheets("Name")`, `Worksheets("Name")`, or `ActiveSheet`, each
        // followed by `.`.
        let sheet = if self.is_kw("sheets") || self.is_kw("worksheets") {
            self.pos += 1;
            self.expect(Token::LParen)?;
            let e = self.expr()?;
            self.expect(Token::RParen)?;
            self.expect(Token::Dot)?;
            Some(e)
        } else if self.is_kw("activesheet") {
            self.pos += 1;
            self.expect(Token::Dot)?;
            None
        } else {
            None
        };
        if self.eat_kw("range") {
            self.expect(Token::LParen)?;
            let e = self.expr()?;
            self.expect(Token::RParen)?;
            return Ok(Ref::Cell { sheet, cell: CellExpr::A1(Box::new(e)) });
        }
        if self.eat_kw("cells") {
            self.expect(Token::LParen)?;
            let row = self.expr()?;
            self.expect(Token::Comma)?;
            let col = self.expr()?;
            self.expect(Token::RParen)?;
            return Ok(Ref::Cell { sheet, cell: CellExpr::RowCol(Box::new(row), Box::new(col)) });
        }
        if sheet.is_some() {
            return Err(ParseError("expected `Range(...)` or `Cells(...)` after sheet qualifier".into()));
        }
        let name = self.ident()?;
        Ok(Ref::Var(name))
    }

    /// Consumes a trailing `.Value` if present — VBA allows `Range("A1")` bare to mean the same
    /// thing as `Range("A1").Value`.
    fn eat_dot_value(&mut self) {
        if matches!(self.peek(), Token::Dot) {
            let save = self.pos;
            self.pos += 1;
            if self.eat_kw("value") {
                return;
            }
            self.pos = save;
        }
    }

    fn place(&mut self) -> Result<Place, ParseError> {
        let r = self.parse_ref()?;
        self.eat_dot_value();
        Ok(match r {
            Ref::Var(name) => Place::Var(name),
            Ref::Cell { sheet, cell } => Place::Cell { sheet: sheet.map(Box::new), cell: Box::new(cell) },
        })
    }

    fn ref_as_expr(&mut self) -> Result<Expr, ParseError> {
        let r = self.parse_ref()?;
        self.eat_dot_value();
        Ok(match r {
            Ref::Var(name) => Expr::Var(name),
            Ref::Cell { sheet, cell } => Expr::CellRead { sheet: sheet.map(Box::new), cell: Box::new(cell) },
        })
    }

    // ---- expressions (lowest to highest precedence) ----

    fn expr(&mut self) -> Result<Expr, ParseError> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.and_expr()?;
        while self.eat_kw("or") {
            lhs = Expr::Bin(BinOp::Or, Box::new(lhs), Box::new(self.and_expr()?));
        }
        Ok(lhs)
    }

    fn and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.not_expr()?;
        while self.eat_kw("and") {
            lhs = Expr::Bin(BinOp::And, Box::new(lhs), Box::new(self.not_expr()?));
        }
        Ok(lhs)
    }

    fn not_expr(&mut self) -> Result<Expr, ParseError> {
        if self.eat_kw("not") {
            return Ok(Expr::Unary(UnOp::Not, Box::new(self.not_expr()?)));
        }
        self.compare_expr()
    }

    fn compare_expr(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.concat_expr()?;
        let op = match self.peek() {
            Token::Eq => BinOp::Eq,
            Token::Ne => BinOp::Ne,
            Token::Lt => BinOp::Lt,
            Token::Le => BinOp::Le,
            Token::Gt => BinOp::Gt,
            Token::Ge => BinOp::Ge,
            _ => return Ok(lhs),
        };
        self.pos += 1;
        let rhs = self.concat_expr()?;
        Ok(Expr::Bin(op, Box::new(lhs), Box::new(rhs)))
    }

    fn concat_expr(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.add_expr()?;
        while self.eat(Token::Amp) {
            lhs = Expr::Bin(BinOp::Concat, Box::new(lhs), Box::new(self.add_expr()?));
        }
        Ok(lhs)
    }

    fn add_expr(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(self.mul_expr()?));
        }
        Ok(lhs)
    }

    fn mul_expr(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => break,
            };
            self.pos += 1;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(self.unary_expr()?));
        }
        Ok(lhs)
    }

    fn unary_expr(&mut self) -> Result<Expr, ParseError> {
        if self.eat(Token::Minus) {
            return Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary_expr()?)));
        }
        self.pow_expr()
    }

    fn pow_expr(&mut self) -> Result<Expr, ParseError> {
        let base = self.primary()?;
        if self.eat(Token::Caret) {
            let exp = self.unary_expr()?;
            return Ok(Expr::Bin(BinOp::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            Token::Number(n) => {
                self.pos += 1;
                Ok(Expr::Lit(Value::Number(n)))
            }
            Token::Str(s) => {
                self.pos += 1;
                Ok(Expr::Lit(Value::Str(s)))
            }
            Token::LParen => {
                self.pos += 1;
                let e = self.expr()?;
                self.expect(Token::RParen)?;
                Ok(e)
            }
            Token::Ident(ref s) if s.eq_ignore_ascii_case("true") => {
                self.pos += 1;
                Ok(Expr::Lit(Value::Bool(true)))
            }
            Token::Ident(ref s) if s.eq_ignore_ascii_case("false") => {
                self.pos += 1;
                Ok(Expr::Lit(Value::Bool(false)))
            }
            Token::Ident(_) => self.ref_as_expr(),
            other => Err(ParseError(format!("unexpected token {other:?}"))),
        }
    }

    // ---- token helpers ----

    fn ident(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            other => Err(ParseError(format!("expected an identifier, found {other:?}"))),
        }
    }

    fn eat(&mut self, t: Token) -> bool {
        if *self.peek() == t {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: Token) -> Result<(), ParseError> {
        if self.eat(t.clone()) { Ok(()) } else { Err(ParseError(format!("expected {t:?}, found {:?}", self.peek()))) }
    }
}
