//! Pratt parser: tokens → [`Expr`] / [`Query`].
//!
//! Binding powers encode the DAX operator table from <https://dax.guide/operators/> (tighter
//! binds first): parentheses, function calls and `IN`; `^`; unary `+`/`-`; `*`/`/`; binary
//! `+`/`-`; `&`; comparisons; `NOT`; `&&`; `||`. All operators are left-associative except
//! `^`, which Excel evaluates right-associatively (`2^3^2` is 512).

use crate::ast::{BinaryOp, Call, Expr, ExprKind, Literal, Name, NameRef, Query, Span, UnaryOp};
use crate::lexer::{LexError, Token, TokenKind, tokenize};

/// Deepest expression nesting accepted. Bounds parser recursion (and the tree that later
/// evaluation walks) at 256 levels.
pub const MAX_DEPTH: usize = 256;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{message} at bytes {}..{}", span.start, span.end)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl ParseError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        ParseError { message: message.into(), span }
    }
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError { message: e.message, span: e.span }
    }
}

/// Parses a DAX expression. A single leading `=` (as typed in some editors) is accepted and
/// ignored.
pub fn parse_expression(src: &str) -> Result<Expr, ParseError> {
    let tokens = tokenize(src)?;
    let mut parser = Parser { tokens, i: 0, depth: 0 };
    if matches!(parser.peek(), TokenKind::Eq) {
        parser.next();
    }
    let expr = parser.expr(0)?;
    parser.expect_eof()?;
    ensure_depth(&expr)?;
    Ok(expr)
}

/// Parses a DAX query. This PR accepts only `EVALUATE <expression>`; the statements PR will
/// extend [`Query`] with `DEFINE`/`VAR` clauses.
pub fn parse_query(src: &str) -> Result<Query, ParseError> {
    let tokens = tokenize(src)?;
    let mut parser = Parser { tokens, i: 0, depth: 0 };
    if !matches!(parser.peek(), TokenKind::Evaluate) {
        return Err(parser.error_here("expected `EVALUATE`"));
    }
    parser.next();
    let evaluate = parser.expr(0)?;
    parser.expect_eof()?;
    ensure_depth(&evaluate)?;
    Ok(Query { evaluate })
}

/// Rejects an AST deeper than [`MAX_DEPTH`] even when parser recursion stayed shallow (for
/// example a function call wrapped around a grouped chain).
fn ensure_depth(expr: &Expr) -> Result<(), ParseError> {
    let depth = expr.depth();
    if depth > MAX_DEPTH {
        return Err(ParseError::new(format!("expression is nested too deeply (limit {MAX_DEPTH})"), expr.span));
    }
    Ok(())
}

fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Integer(n) => format!("integer literal `{n}`"),
        TokenKind::Decimal(n) => format!("decimal literal `{n}`"),
        TokenKind::DateTime(value) => format!("date/time literal `dt\"{value}\"`"),
        TokenKind::String(_) => "string literal".into(),
        TokenKind::Ident(s) => format!("`{s}`"),
        TokenKind::QuotedIdent(_) => "quoted identifier".into(),
        TokenKind::BracketIdent(_) => "bracketed identifier".into(),
        TokenKind::Not => "`NOT`".into(),
        TokenKind::In => "`IN`".into(),
        TokenKind::Evaluate => "`EVALUATE`".into(),
        TokenKind::Plus => "`+`".into(),
        TokenKind::Minus => "`-`".into(),
        TokenKind::Star => "`*`".into(),
        TokenKind::Slash => "`/`".into(),
        TokenKind::Caret => "`^`".into(),
        TokenKind::Amp => "`&`".into(),
        TokenKind::Eq => "`=`".into(),
        TokenKind::StrictEq => "`==`".into(),
        TokenKind::NotEq => "`<>`".into(),
        TokenKind::Lt => "`<`".into(),
        TokenKind::Gt => "`>`".into(),
        TokenKind::Le => "`<=`".into(),
        TokenKind::Ge => "`>=`".into(),
        TokenKind::AndAnd => "`&&`".into(),
        TokenKind::OrOr => "`||`".into(),
        TokenKind::Arrow => "`=>`".into(),
        TokenKind::LParen => "`(`".into(),
        TokenKind::RParen => "`)`".into(),
        TokenKind::LBrace => "`{`".into(),
        TokenKind::RBrace => "`}`".into(),
        TokenKind::Comma => "`,`".into(),
        TokenKind::Eof => "end of input".into(),
    }
}

struct Parser {
    tokens: Vec<Token>,
    i: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> &TokenKind {
        self.tokens.get(self.i).map(|t| &t.kind).unwrap_or(&TokenKind::Eof)
    }

    fn peek_span(&self) -> Span {
        self.tokens.get(self.i).map(|t| t.span).unwrap_or(Span::empty())
    }

    fn next(&mut self) -> Token {
        let token = self.tokens.get(self.i).cloned().unwrap_or(Token { kind: TokenKind::Eof, span: Span::empty() });
        if self.i < self.tokens.len() {
            self.i += 1;
        }
        token
    }

    fn error_here(&self, message: impl Into<String>) -> ParseError {
        ParseError::new(message, self.peek_span())
    }

    fn expect(&mut self, kind: TokenKind, context: &str) -> Result<Token, ParseError> {
        if *self.peek() == kind {
            Ok(self.next())
        } else {
            Err(self.error_here(format!("expected {}, found {}; {context}", describe(&kind), describe(self.peek()))))
        }
    }

    fn expect_eof(&self) -> Result<(), ParseError> {
        match self.peek() {
            TokenKind::Eof => Ok(()),
            other => Err(self.error_here(format!("unexpected {} after expression", describe(other)))),
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error_here(format!("expression is nested too deeply (limit {MAX_DEPTH})")));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn binary_op(&self) -> Option<BinaryOp> {
        match self.peek() {
            TokenKind::Plus => Some(BinaryOp::Add),
            TokenKind::Minus => Some(BinaryOp::Sub),
            TokenKind::Star => Some(BinaryOp::Mul),
            TokenKind::Slash => Some(BinaryOp::Div),
            TokenKind::Caret => Some(BinaryOp::Pow),
            TokenKind::Amp => Some(BinaryOp::Concat),
            TokenKind::Eq => Some(BinaryOp::Eq),
            TokenKind::StrictEq => Some(BinaryOp::StrictEq),
            TokenKind::NotEq => Some(BinaryOp::Ne),
            TokenKind::Lt => Some(BinaryOp::Lt),
            TokenKind::Gt => Some(BinaryOp::Gt),
            TokenKind::Le => Some(BinaryOp::Le),
            TokenKind::Ge => Some(BinaryOp::Ge),
            TokenKind::AndAnd => Some(BinaryOp::And),
            TokenKind::OrOr => Some(BinaryOp::Or),
            TokenKind::In => Some(BinaryOp::In),
            _ => None,
        }
    }

    fn expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        self.enter()?;
        let mut lhs = self.prefix()?;
        while let Some(op) = self.binary_op() {
            let bp = op.precedence();
            if bp < min_bp {
                break;
            }
            self.next();
            // `^` is right-associative; every other binary operator is left-associative.
            let next_min = if op.is_right_associative() { bp } else { bp + 1 };
            let rhs = self.expr(next_min)?;
            // Grouped expressions return their inner tree without a parenthesis node, so the
            // depth must come from the AST itself, not from the parser's recursion frames.
            if lhs.depth().max(rhs.depth()) >= MAX_DEPTH {
                return Err(self.error_here(format!("expression is nested too deeply (limit {MAX_DEPTH})")));
            }
            let span = lhs.span.to(rhs.span);
            lhs = Expr::new(ExprKind::Binary { op, left: Box::new(lhs), right: Box::new(rhs) }, span);
        }
        self.leave();
        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Expr, ParseError> {
        let token = self.next();
        match token.kind {
            TokenKind::Integer(n) => Ok(Expr::literal(Literal::Integer(n), token.span)),
            TokenKind::Decimal(n) => Ok(Expr::literal(Literal::Decimal(n), token.span)),
            TokenKind::DateTime(value) => Ok(Expr::literal(Literal::DateTime(value), token.span)),
            TokenKind::String(s) => Ok(Expr::literal(Literal::String(s), token.span)),
            TokenKind::Ident(word) => self.prefix_ident(word, token.span),
            TokenKind::QuotedIdent(word) => {
                let name = Name::quoted(word);
                if let TokenKind::BracketIdent(_) = self.peek() {
                    self.qualified_name(name, token.span)
                } else {
                    Ok(Expr::new(ExprKind::Name(NameRef::unqualified(name)), token.span))
                }
            }
            TokenKind::BracketIdent(word) => Ok(Expr::new(ExprKind::Name(NameRef::unqualified(Name::bracketed(word))), token.span)),
            TokenKind::Not => {
                let operand = self.expr(UnaryOp::Not.precedence())?;
                let span = token.span.to(operand.span);
                Ok(Expr::new(ExprKind::Unary { op: UnaryOp::Not, expr: Box::new(operand) }, span))
            }
            TokenKind::Plus => {
                let operand = self.expr(UnaryOp::Plus.precedence())?;
                let span = token.span.to(operand.span);
                Ok(Expr::new(ExprKind::Unary { op: UnaryOp::Plus, expr: Box::new(operand) }, span))
            }
            TokenKind::Minus => {
                let operand = self.expr(UnaryOp::Neg.precedence())?;
                let span = token.span.to(operand.span);
                Ok(Expr::new(ExprKind::Unary { op: UnaryOp::Neg, expr: Box::new(operand) }, span))
            }
            TokenKind::LParen => self.paren(token.span),
            TokenKind::LBrace => self.table(token.span),
            _ => Err(ParseError::new(format!("expected expression, found {}", describe(&token.kind)), token.span)),
        }
    }

    fn prefix_ident(&mut self, word: String, span: Span) -> Result<Expr, ParseError> {
        if let TokenKind::BracketIdent(_) = self.peek() {
            return self.qualified_name(Name::bare(word), span);
        }
        if matches!(self.peek(), TokenKind::LParen) {
            return self.call(Name::bare(word), span);
        }
        if word.eq_ignore_ascii_case("TRUE") {
            return Ok(Expr::literal(Literal::Boolean(true), span));
        }
        if word.eq_ignore_ascii_case("FALSE") {
            return Ok(Expr::literal(Literal::Boolean(false), span));
        }
        Ok(Expr::new(ExprKind::Name(NameRef::unqualified(Name::bare(word))), span))
    }

    /// Consumes a bracketed column that follows a table name.
    fn qualified_name(&mut self, table: Name, table_span: Span) -> Result<Expr, ParseError> {
        let column = self.next();
        let TokenKind::BracketIdent(text) = column.kind else {
            return Err(ParseError::new("expected bracketed column name", column.span));
        };
        let span = table_span.to(column.span);
        Ok(Expr::new(ExprKind::Name(NameRef::qualified(table, Name::bracketed(text))), span))
    }

    fn call(&mut self, name: Name, name_span: Span) -> Result<Expr, ParseError> {
        self.expect(TokenKind::LParen, "function calls need `(` after the name")?;
        let mut args = Vec::new();
        if matches!(self.peek(), TokenKind::RParen) {
            let close = self.next();
            return Ok(Expr::new(ExprKind::Call(Call { name, args }), name_span.to(close.span)));
        }
        let close_span = loop {
            if matches!(self.peek(), TokenKind::RParen) {
                return Err(self.error_here("expected an argument after `,`"));
            }
            args.push(self.expr(0)?);
            let token = self.next();
            match token.kind {
                TokenKind::Comma => {}
                TokenKind::RParen => break token.span,
                _ => {
                    return Err(ParseError::new(
                        format!("expected `,` or `)` in call to `{}`, found {}", name.text, describe(&token.kind)),
                        token.span,
                    ));
                }
            }
        };
        Ok(Expr::new(ExprKind::Call(Call { name, args }), name_span.to(close_span)))
    }

    /// `( expr )` is grouping (parentheses are not AST nodes), `( a, b, ... )` is a row
    /// constructor.
    fn paren(&mut self, open: Span) -> Result<Expr, ParseError> {
        if matches!(self.peek(), TokenKind::RParen) {
            let close = self.next();
            return Err(ParseError::new("empty parentheses are not an expression", open.to(close.span)));
        }
        let first = self.expr(0)?;
        if matches!(self.peek(), TokenKind::Comma) {
            let mut items = vec![first];
            while matches!(self.peek(), TokenKind::Comma) {
                self.next();
                if matches!(self.peek(), TokenKind::RParen | TokenKind::Eof) {
                    return Err(self.error_here("expected an expression after `,` in a row constructor"));
                }
                items.push(self.expr(0)?);
            }
            let close = self.expect(TokenKind::RParen, "row constructors end with `)`")?;
            return Ok(Expr::new(ExprKind::Tuple(items), open.to(close.span)));
        }
        let close = self.expect(TokenKind::RParen, "parentheses end with `)`")?;
        Ok(Expr::new(first.kind, open.to(close.span)))
    }

    fn table(&mut self, open: Span) -> Result<Expr, ParseError> {
        let mut items = Vec::new();
        if matches!(self.peek(), TokenKind::RBrace) {
            let close = self.next();
            return Ok(Expr::new(ExprKind::Table(items), open.to(close.span)));
        }
        loop {
            if matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                return Err(self.error_here("expected a value in the table constructor"));
            }
            items.push(self.expr(0)?);
            match self.next().kind {
                TokenKind::Comma => {
                    if matches!(self.peek(), TokenKind::RBrace) {
                        return Err(self.error_here("expected a value after `,` in the table constructor"));
                    }
                }
                TokenKind::RBrace => break,
                _ => {
                    return Err(ParseError::new(
                        "expected `,` or `}` in the table constructor",
                        self.tokens.get(self.i.saturating_sub(1)).map(|t| t.span).unwrap_or(open),
                    ));
                }
            }
        }
        let close_span = self.tokens.get(self.i.saturating_sub(1)).map(|t| t.span).unwrap_or(open);
        Ok(Expr::new(ExprKind::Table(items), open.to(close_span)))
    }
}
