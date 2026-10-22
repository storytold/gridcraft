//! Pratt parser: tokens → [`Expr`] / [`Query`].
//!
//! Binding powers encode the DAX operator table from <https://dax.guide/operators/> (tighter
//! binds first): parentheses, function calls and `IN`; `^`; unary `+`/`-`; `*`/`/`; binary
//! `+`/`-`; `&`; comparisons; `NOT`; `&&`; `||`. All operators are left-associative except
//! `^`, which Excel evaluates right-associatively (`2^3^2` is 512).

use crate::ast::{
    Axis, AxisGroup, AxisKind, AxisOrderItem, BinaryOp, Call, ColumnDef, Define, Definition, Evaluate, Expr, ExprKind, FunctionDef, FunctionParam,
    Literal, MParameterDef, MeasureDef, Name, NameRef, OrderItem, ParameterSubtype, ParameterType, PassingMode, Query, SortDirection, Span,
    StartValue, TableDef, UnaryOp, VarDecl, VarDef, VisualShape,
};
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

/// Parses a DAX query: an optional `DEFINE` block followed by one or more `EVALUATE` statements.
///
/// For the simple form `EVALUATE <expression>` the result has `define: None` and a single
/// evaluation with empty `order_by`/`start_at`. An optional trailing `;` is accepted; it is not
/// part of the grammar, only a recovery extension.
///
/// Statement keywords (`DEFINE`, `MEASURE`, `COLUMN`, `TABLE`, `VAR`, `RETURN`, `FUNCTION`,
/// `MPARAMETER`, `ORDER BY`, `START AT`, `WITH VISUAL SHAPE`, `AXIS`, `GROUP`, `TOTAL`,
/// `DENSIFY`, `ASC`, `DESC`) are recognised case-insensitively at the positions where they are
/// expected. Definition bounds and statement bounds therefore come from those keywords, never
/// from newlines.
pub fn parse_query(src: &str) -> Result<Query, ParseError> {
    let tokens = tokenize(src)?;
    let mut parser = Parser { tokens, i: 0, depth: 0 };
    let query = parser.query()?;
    validate_query_depth(&query)?;
    Ok(query)
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

/// Applies the AST-depth bound to every expression in a query, not only to the last parsed
/// `EVALUATE` expression.
fn validate_query_depth(query: &Query) -> Result<(), ParseError> {
    if let Some(define) = &query.define {
        for item in &define.items {
            match item {
                Definition::Measure(def) => ensure_depth(&def.expr)?,
                Definition::Column(def) => ensure_depth(&def.expr)?,
                Definition::Table(def) => ensure_depth(&def.expr)?,
                Definition::Var(def) => ensure_depth(&def.expr)?,
                Definition::MParameter(def) => ensure_depth(&def.expr)?,
                Definition::Function(def) => {
                    ensure_depth(&def.body)?;
                    for param in &def.params {
                        if let Some(default) = &param.default {
                            ensure_depth(default)?;
                        }
                    }
                }
            }
        }
    }
    for evaluate in &query.evaluates {
        ensure_depth(&evaluate.table)?;
        for item in &evaluate.order_by {
            ensure_depth(&item.expr)?;
        }
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
        TokenKind::Colon => "`:`".into(),
        TokenKind::AtName(name) => format!("parameter `@{name}`"),
        TokenKind::Semicolon => "`;`".into(),
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

    fn expect_query_eof(&self) -> Result<(), ParseError> {
        match self.peek() {
            TokenKind::Eof => Ok(()),
            other => Err(self.error_here(format!("unexpected {} after the query", describe(other)))),
        }
    }

    /// The current token when it is a bare identifier, which is how statement keywords are
    /// lexed. Keywords are matched case-insensitively by their text.
    fn peek_word(&self) -> Option<&str> {
        match self.peek() {
            TokenKind::Ident(word) => Some(word),
            _ => None,
        }
    }

    fn keyword_at(&self, word: &str) -> bool {
        self.peek_word().is_some_and(|text| text.eq_ignore_ascii_case(word))
    }

    fn expect_keyword(&mut self, word: &str, context: &str) -> Result<Token, ParseError> {
        if self.keyword_at(word) {
            Ok(self.next())
        } else {
            Err(self.error_here(format!("expected `{word}`, found {}; {context}", describe(self.peek()))))
        }
    }

    // -------------------------------------------------------------------------------------
    // Query statements
    // -------------------------------------------------------------------------------------

    fn query(&mut self) -> Result<Query, ParseError> {
        let define = if self.keyword_at("DEFINE") { Some(self.define_block()?) } else { None };
        if !matches!(self.peek(), TokenKind::Evaluate) {
            return Err(self.error_here("a query needs at least one `EVALUATE` statement"));
        }
        let mut evaluates = Vec::new();
        while matches!(self.peek(), TokenKind::Evaluate) {
            evaluates.push(self.evaluate_statement()?);
        }
        // The documented grammar has no statement terminators. A trailing `;` is accepted as a
        // recovery extension only, never required.
        while matches!(self.peek(), TokenKind::Semicolon) {
            self.next();
        }
        self.expect_query_eof()?;
        Ok(Query { define, evaluates })
    }

    fn define_block(&mut self) -> Result<Define, ParseError> {
        let keyword = self.next();
        let mut items = Vec::new();
        loop {
            let Some(word) = self.peek_word().map(str::to_owned) else {
                match self.peek() {
                    TokenKind::Evaluate | TokenKind::Eof | TokenKind::Semicolon => break,
                    other => {
                        return Err(self.error_here(format!(
                            "expected a DEFINE entry (MEASURE, COLUMN, TABLE, VAR, FUNCTION or MPARAMETER), found {}",
                            describe(other)
                        )));
                    }
                }
            };
            if word.eq_ignore_ascii_case("MEASURE") {
                self.next();
                items.push(Definition::Measure(self.measure_definition()?));
            } else if word.eq_ignore_ascii_case("COLUMN") {
                self.next();
                items.push(Definition::Column(self.column_definition()?));
            } else if word.eq_ignore_ascii_case("TABLE") {
                self.next();
                items.push(Definition::Table(self.table_definition()?));
            } else if word.eq_ignore_ascii_case("VAR") {
                self.next();
                items.push(Definition::Var(self.var_definition()?));
            } else if word.eq_ignore_ascii_case("FUNCTION") {
                self.next();
                items.push(Definition::Function(self.function_definition()?));
            } else if word.eq_ignore_ascii_case("MPARAMETER") {
                self.next();
                items.push(Definition::MParameter(self.mparameter_definition()?));
            } else {
                return Err(self.error_here(format!("expected a DEFINE entry (MEASURE, COLUMN, TABLE, VAR, FUNCTION or MPARAMETER), found `{word}`")));
            }
        }
        if items.is_empty() {
            return Err(ParseError::new("DEFINE needs at least one definition before `EVALUATE`", keyword.span));
        }
        Ok(Define { items })
    }

    fn measure_definition(&mut self) -> Result<MeasureDef, ParseError> {
        let name = self.definition_name("measure")?;
        self.expect(TokenKind::Eq, "a MEASURE definition needs `=` before its expression")?;
        let expr = self.expr(0)?;
        Ok(MeasureDef { name, expr })
    }

    fn column_definition(&mut self) -> Result<ColumnDef, ParseError> {
        let name = self.definition_name("column")?;
        self.expect(TokenKind::Eq, "a COLUMN definition needs `=` before its expression")?;
        let expr = self.expr(0)?;
        Ok(ColumnDef { name, expr })
    }

    fn var_definition(&mut self) -> Result<VarDef, ParseError> {
        let name = self.variable_name()?;
        self.expect(TokenKind::Eq, "a query-scoped VAR needs `=` before its expression")?;
        let expr = self.expr(0)?;
        Ok(VarDef { name, expr })
    }

    fn table_definition(&mut self) -> Result<TableDef, ParseError> {
        let name = self.table_name("table")?;
        self.expect(TokenKind::Eq, "a TABLE definition needs `=` before its table expression")?;
        let expr = self.expr(0)?;
        let visual_shape = if self.keyword_at("WITH") { Some(self.visual_shape()?) } else { None };
        Ok(TableDef { name, expr, visual_shape })
    }

    fn mparameter_definition(&mut self) -> Result<MParameterDef, ParseError> {
        let name = self.table_name("M parameter")?;
        self.expect(TokenKind::Eq, "an MPARAMETER definition needs `=` before its expression")?;
        let expr = self.expr(0)?;
        Ok(MParameterDef { name, expr })
    }

    fn function_definition(&mut self) -> Result<FunctionDef, ParseError> {
        let name = self.table_name("function")?;
        self.expect(TokenKind::Eq, "a FUNCTION definition needs `=` before its parameter list")?;
        self.expect(TokenKind::LParen, "a FUNCTION definition needs a parenthesised parameter list")?;
        let mut params = Vec::new();
        if matches!(self.peek(), TokenKind::RParen) {
            self.next();
        } else {
            loop {
                params.push(self.function_param()?);
                let token = self.next();
                match token.kind {
                    TokenKind::Comma => {
                        if matches!(self.peek(), TokenKind::RParen) {
                            return Err(self.error_here("expected a parameter after `,`"));
                        }
                    }
                    TokenKind::RParen => break,
                    other => {
                        return Err(ParseError::new(format!("expected `,` or `)` in the parameter list, found {}", describe(&other)), token.span));
                    }
                }
            }
        }
        self.expect(TokenKind::Arrow, "a FUNCTION definition needs `=>` before its body")?;
        let body = self.expr(0)?;
        Ok(FunctionDef { name, params, body })
    }

    fn function_param(&mut self) -> Result<FunctionParam, ParseError> {
        let token = self.next();
        let name = match &token.kind {
            TokenKind::Ident(text) => Name::bare(text.clone()),
            other => return Err(ParseError::new(format!("expected a parameter name, found {}", describe(other)), token.span)),
        };
        let mut ty = None;
        let mut subtype = None;
        let mut passing = None;
        if matches!(self.peek(), TokenKind::Colon) {
            let colon = self.next();
            let mut annotated = false;
            while let Some(word) = self.peek_word().map(str::to_owned) {
                if let Some(parsed) = ParameterType::from_keyword(&word) {
                    if ty.is_some() {
                        return Err(self.error_here(format!("duplicate parameter type `{}`", parsed.keyword())));
                    }
                    if subtype.is_some() || passing.is_some() {
                        return Err(self.error_here("the parameter type must come before its subtype and passing mode"));
                    }
                    ty = Some(parsed);
                } else if let Some(parsed) = ParameterSubtype::from_keyword(&word) {
                    if subtype.is_some() {
                        return Err(self.error_here(format!("duplicate parameter subtype `{}`", parsed.keyword())));
                    }
                    if passing.is_some() {
                        return Err(self.error_here("the parameter subtype must come before its passing mode"));
                    }
                    subtype = Some(parsed);
                } else if let Some(parsed) = PassingMode::from_keyword(&word) {
                    if passing.is_some() {
                        return Err(self.error_here("duplicate parameter passing mode"));
                    }
                    passing = Some(parsed);
                } else {
                    return Err(self.error_here(format!("unknown function parameter type `{word}`")));
                }
                self.next();
                annotated = true;
            }
            if !annotated {
                return Err(ParseError::new("expected a parameter type, subtype or passing mode after `:`", colon.span));
            }
            if subtype.is_some() && ty.is_some_and(|ty| ty != ParameterType::Scalar) {
                return Err(self.error_here("only SCALAR parameters accept a subtype"));
            }
        }
        let default = if matches!(self.peek(), TokenKind::Eq) {
            self.next();
            Some(self.expr(0)?)
        } else {
            None
        };
        Ok(FunctionParam { name, ty, subtype, passing, default })
    }

    fn evaluate_statement(&mut self) -> Result<Evaluate, ParseError> {
        self.next(); // EVALUATE
        let table = self.expr(0)?;
        let order_by = if self.keyword_at("ORDER") {
            self.next();
            self.expect_keyword("BY", "ORDER must be followed by BY")?;
            self.order_items()?
        } else {
            Vec::new()
        };
        let start_at = if self.keyword_at("START") {
            let span = self.peek_span();
            self.next();
            self.expect_keyword("AT", "START must be followed by AT")?;
            if order_by.is_empty() {
                return Err(ParseError::new("START AT requires a preceding ORDER BY clause", span));
            }
            let values = self.start_values()?;
            if values.len() > order_by.len() {
                return Err(ParseError::new(format!("START AT has {} value(s) but ORDER BY has {} item(s)", values.len(), order_by.len()), span));
            }
            values
        } else {
            Vec::new()
        };
        Ok(Evaluate { table, order_by, start_at })
    }

    fn order_items(&mut self) -> Result<Vec<OrderItem>, ParseError> {
        let mut items = Vec::new();
        loop {
            let expr = self.expr(0)?;
            let direction = self.direction();
            items.push(OrderItem { expr, direction });
            if matches!(self.peek(), TokenKind::Comma) {
                self.next();
            } else {
                break;
            }
        }
        Ok(items)
    }

    fn direction(&mut self) -> SortDirection {
        if self.keyword_at("ASC") {
            self.next();
            SortDirection::Asc
        } else if self.keyword_at("DESC") {
            self.next();
            SortDirection::Desc
        } else {
            SortDirection::Asc
        }
    }

    fn start_values(&mut self) -> Result<Vec<StartValue>, ParseError> {
        let mut values = Vec::new();
        loop {
            values.push(self.start_value()?);
            if matches!(self.peek(), TokenKind::Comma) {
                self.next();
            } else {
                break;
            }
        }
        Ok(values)
    }

    fn start_value(&mut self) -> Result<StartValue, ParseError> {
        let token = self.next();
        let negative = match &token.kind {
            TokenKind::Minus => Some(true),
            TokenKind::Plus => Some(false),
            _ => None,
        };
        if let Some(negative) = negative {
            let literal = self.next();
            return match literal.kind {
                TokenKind::Integer(value) => Ok(StartValue::Literal(Literal::Integer(if negative { -value } else { value }))),
                TokenKind::Decimal(value) => Ok(StartValue::Literal(Literal::Decimal(if negative { -value } else { value }))),
                other => Err(ParseError::new(format!("a signed START AT value must be a numeric literal, found {}", describe(&other)), literal.span)),
            };
        }
        match token.kind {
            TokenKind::Integer(value) => Ok(StartValue::Literal(Literal::Integer(value))),
            TokenKind::Decimal(value) => Ok(StartValue::Literal(Literal::Decimal(value))),
            TokenKind::DateTime(value) => Ok(StartValue::Literal(Literal::DateTime(value))),
            TokenKind::String(value) => Ok(StartValue::Literal(Literal::String(value))),
            TokenKind::AtName(name) => Ok(StartValue::Parameter(Name::bare(name))),
            TokenKind::Ident(word) if word.eq_ignore_ascii_case("TRUE") => Ok(StartValue::Literal(Literal::Boolean(true))),
            TokenKind::Ident(word) if word.eq_ignore_ascii_case("FALSE") => Ok(StartValue::Literal(Literal::Boolean(false))),
            other => Err(ParseError::new(format!("expected a literal or @parameter in START AT, found {}", describe(&other)), token.span)),
        }
    }

    fn visual_shape(&mut self) -> Result<VisualShape, ParseError> {
        self.next(); // WITH
        self.expect_keyword("VISUAL", "WITH must be followed by VISUAL SHAPE")?;
        self.expect_keyword("SHAPE", "WITH VISUAL SHAPE is a single clause")?;
        let mut axes = Vec::new();
        while self.keyword_at("AXIS") {
            let span = self.peek_span();
            let axis = self.axis()?;
            if axes.iter().any(|existing: &Axis| existing.kind == axis.kind) {
                return Err(ParseError::new(format!("duplicate AXIS {} clause", axis.kind.keyword()), span));
            }
            axes.push(axis);
        }
        if axes.is_empty() {
            return Err(self.error_here("WITH VISUAL SHAPE needs at least one AXIS ROWS or AXIS COLUMNS clause"));
        }
        let densify = if self.keyword_at("DENSIFY") {
            self.next();
            let value = self.next();
            match value.kind {
                TokenKind::String(text) => Some(text),
                other => {
                    return Err(ParseError::new(format!("DENSIFY needs a double-quoted column name, found {}", describe(&other)), value.span));
                }
            }
        } else {
            None
        };
        Ok(VisualShape { axes, densify })
    }

    fn axis(&mut self) -> Result<Axis, ParseError> {
        self.next(); // AXIS
        let kind_token = self.next();
        let kind = match &kind_token.kind {
            TokenKind::Ident(word) if word.eq_ignore_ascii_case("ROWS") => AxisKind::Rows,
            TokenKind::Ident(word) if word.eq_ignore_ascii_case("COLUMNS") => AxisKind::Columns,
            other => {
                return Err(ParseError::new(format!("expected `ROWS` or `COLUMNS` after AXIS, found {}", describe(other)), kind_token.span));
            }
        };
        let mut groups = Vec::new();
        while self.keyword_at("GROUP") {
            self.next();
            let mut columns = vec![self.column_ref("GROUP")?];
            while matches!(self.peek(), TokenKind::Comma) {
                self.next();
                columns.push(self.column_ref("GROUP")?);
            }
            self.expect_keyword("TOTAL", "each GROUP needs a TOTAL column")?;
            let total = self.column_ref("TOTAL")?;
            groups.push(AxisGroup { columns, total });
        }
        if groups.is_empty() {
            return Err(self.error_here("AXIS needs at least one GROUP ... TOTAL level"));
        }
        self.expect_keyword("ORDER", "each AXIS needs an ORDER BY clause")?;
        self.expect_keyword("BY", "each AXIS needs an ORDER BY clause")?;
        let mut order_by = Vec::new();
        loop {
            let column = self.column_ref("ORDER BY")?;
            let direction = self.direction();
            order_by.push(AxisOrderItem { column, direction });
            if matches!(self.peek(), TokenKind::Comma) {
                self.next();
            } else {
                break;
            }
        }
        Ok(Axis { kind, groups, order_by })
    }

    /// A table name in a definition: a bare identifier or a single-quoted name.
    fn table_name(&mut self, what: &str) -> Result<Name, ParseError> {
        let token = self.next();
        match &token.kind {
            TokenKind::Ident(text) => Ok(Name::bare(text.clone())),
            TokenKind::QuotedIdent(text) => Ok(Name::quoted(text.clone())),
            other => Err(ParseError::new(format!("expected a {what} name, found {}", describe(other)), token.span)),
        }
    }

    /// `<table>[<name>]`, the name of a MEASURE or COLUMN definition.
    fn definition_name(&mut self, kind: &str) -> Result<NameRef, ParseError> {
        let table = self.table_name(kind)?;
        let token = self.next();
        match &token.kind {
            TokenKind::BracketIdent(text) => Ok(NameRef::qualified(table, Name::bracketed(text.clone()))),
            other => Err(ParseError::new(format!("expected a bracketed {kind} name after the table name, found {}", describe(other)), token.span)),
        }
    }

    /// A variable name: a plain identifier, never quoted or bracketed.
    fn variable_name(&mut self) -> Result<Name, ParseError> {
        let token = self.next();
        match &token.kind {
            TokenKind::Ident(text) => Ok(Name::bare(text.clone())),
            TokenKind::QuotedIdent(_) | TokenKind::BracketIdent(_) => {
                Err(ParseError::new("variable names cannot be quoted or bracketed", token.span))
            }
            other => Err(ParseError::new(format!("expected a variable name, found {}", describe(other)), token.span)),
        }
    }

    /// A column reference: `[Column]`, `Table[Column]` or `'Table'[Column]`.
    fn column_ref(&mut self, context: &str) -> Result<NameRef, ParseError> {
        let token = self.next();
        let table = match &token.kind {
            TokenKind::BracketIdent(text) => return Ok(NameRef::unqualified(Name::bracketed(text.clone()))),
            TokenKind::Ident(text) => Name::bare(text.clone()),
            TokenKind::QuotedIdent(text) => Name::quoted(text.clone()),
            other => {
                return Err(ParseError::new(format!("expected a column reference in {context}, found {}", describe(other)), token.span));
            }
        };
        let column = self.next();
        match &column.kind {
            TokenKind::BracketIdent(text) => Ok(NameRef::qualified(table, Name::bracketed(text.clone()))),
            other => Err(ParseError::new(format!("expected a bracketed column name after the table name, found {}", describe(other)), column.span)),
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
            TokenKind::Ident(word) => {
                if word.eq_ignore_ascii_case("VAR") {
                    return self.var_return(token.span);
                }
                if word.eq_ignore_ascii_case("RETURN") {
                    return Err(ParseError::new("RETURN without a preceding VAR declaration", token.span));
                }
                self.prefix_ident(word, token.span)
            }
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

    /// Parses `VAR <name> = <expr> ... RETURN <expr>`.
    ///
    /// The declaration values and the `RETURN` body stop at the next statement or definition
    /// keyword because a bare identifier is never an infix operator, so no newline or terminator
    /// is needed. A nested `VAR ... RETURN ...` as a declaration value or body parses recursively.
    fn var_return(&mut self, start: Span) -> Result<Expr, ParseError> {
        let mut vars = Vec::new();
        // The leading `VAR` has already been consumed by `prefix`; each iteration parses one
        // declaration and continues only when another `VAR` follows the value expression.
        loop {
            let name = self.variable_name()?;
            self.expect(TokenKind::Eq, "a VAR declaration needs `=` before its expression")?;
            let expr = self.expr(0)?;
            vars.push(VarDecl { name, expr });
            if !self.keyword_at("VAR") {
                break;
            }
            self.next();
        }
        if !self.keyword_at("RETURN") {
            return Err(self.error_here("VAR declarations must be followed by RETURN <expression>"));
        }
        self.next();
        let body = self.expr(0)?;
        let span = start.to(body.span);
        Ok(Expr::new(ExprKind::VarReturn { vars, body: Box::new(body) }, span))
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
