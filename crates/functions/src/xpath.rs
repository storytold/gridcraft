//! An XPath 1.0 evaluator over [`crate::xml::Dom`], for FILTERXML.
//!
//! Supported: absolute and relative location paths, every axis except `following`, `preceding` and
//! `namespace`, name / `*` / `prefix:*` / `text()` / `node()` tests, predicates (numeric position or
//! boolean), unions, the arithmetic, comparison and boolean operators with XPath's node-set
//! comparison rules, and the XPath 1.0 core function library except `id` and `lang`. Names match
//! as written, prefix included; a name without a prefix does not select an element that is inside a
//! default namespace (use `*[local-name()='x']`). Namespace declarations are not attributes.
//!
//! Expressions come from cells, so nesting is capped and evaluation spends a fixed amount of work
//! ([`FUEL`]); past it the evaluation gives up and the caller answers `#VALUE!`.

use gridcraft_core::Value;

use crate::xml::{Dom, Kind};

/// Work units one evaluation may spend (one per node visited by an axis or predicate run).
const FUEL: usize = 4_000_000;
/// Deepest nesting of parentheses, predicates and calls.
const MAX_DEPTH: usize = 64;
/// Binary and union operators one expression may hold in all. Each can add a level to the syntax
/// tree, which evaluation and dropping walk recursively; counting every operator (not only the
/// chain being parsed) bounds the height however groups nest around finished subtrees.
const MAX_OPS: usize = 128;

// ---------------------------------------------------------------------------------------------
// Lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Slash,
    DSlash,
    Dot,
    DotDot,
    At,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Pipe,
    Plus,
    Minus,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Star,
    Num(f64),
    Lit(String),
    Name(String),
    /// A name followed by `::`.
    Axis(String),
}

fn is_ncname_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ncname_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '\u{b7}')
}

/// Splits the expression into tokens; `None` for a character or number that XPath doesn't have.
fn lex(src: &str) -> Option<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut toks = vec![];
    let mut i = 0;
    while let Some(c) = at(i) {
        match c {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            '/' if at(i + 1) == Some('/') => {
                toks.push(Tok::DSlash);
                i += 2;
            }
            '/' => {
                toks.push(Tok::Slash);
                i += 1;
            }
            '.' if at(i + 1) == Some('.') => {
                toks.push(Tok::DotDot);
                i += 2;
            }
            '.' if !at(i + 1).is_some_and(|d| d.is_ascii_digit()) => {
                toks.push(Tok::Dot);
                i += 1;
            }
            '@' | '(' | ')' | '[' | ']' | ',' | '|' | '+' | '-' | '=' | '*' => {
                toks.push(match c {
                    '@' => Tok::At,
                    '(' => Tok::LParen,
                    ')' => Tok::RParen,
                    '[' => Tok::LBracket,
                    ']' => Tok::RBracket,
                    ',' => Tok::Comma,
                    '|' => Tok::Pipe,
                    '+' => Tok::Plus,
                    '-' => Tok::Minus,
                    '=' => Tok::Eq,
                    _ => Tok::Star,
                });
                i += 1;
            }
            '!' if at(i + 1) == Some('=') => {
                toks.push(Tok::Ne);
                i += 2;
            }
            '<' | '>' => {
                let eq = at(i + 1) == Some('=');
                toks.push(match (c, eq) {
                    ('<', true) => Tok::Le,
                    ('<', false) => Tok::Lt,
                    (_, true) => Tok::Ge,
                    _ => Tok::Gt,
                });
                i += if eq { 2 } else { 1 };
            }
            '"' | '\'' => {
                let body: String = chars.get(i + 1..)?.iter().take_while(|&&d| d != c).collect();
                let len = body.chars().count();
                if at(i + 1 + len) != Some(c) {
                    return None;
                }
                toks.push(Tok::Lit(body));
                i += len + 2;
            }
            d if d.is_ascii_digit() || d == '.' => {
                let start = i;
                while at(i).is_some_and(|d| d.is_ascii_digit()) {
                    i += 1;
                }
                if at(i) == Some('.') {
                    i += 1;
                    while at(i).is_some_and(|d| d.is_ascii_digit()) {
                        i += 1;
                    }
                }
                let s: String = chars.get(start..i)?.iter().collect();
                toks.push(Tok::Num(s.parse().ok()?));
            }
            n if is_ncname_start(n) => {
                let (name, next) = qname(&chars, i);
                i = next;
                let mut j = i;
                while matches!(at(j), Some(' ' | '\t' | '\r' | '\n')) {
                    j += 1;
                }
                if at(j) == Some(':') && at(j + 1) == Some(':') {
                    toks.push(Tok::Axis(name));
                    i = j + 2;
                } else {
                    toks.push(Tok::Name(name));
                }
            }
            _ => return None,
        }
    }
    Some(toks)
}

/// Reads `name`, `prefix:name` or `prefix:*` starting at `start`; returns it and the next index.
fn qname(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    let take = |i: &mut usize| {
        while chars.get(*i).copied().is_some_and(is_ncname_char) {
            *i += 1;
        }
    };
    take(&mut i);
    if chars.get(i) == Some(&':') && chars.get(i + 1) != Some(&':') {
        match chars.get(i + 1) {
            Some('*') => i += 2,
            Some(&c) if is_ncname_start(c) => {
                i += 1;
                take(&mut i);
            }
            _ => {}
        }
    }
    (chars.get(start..i).unwrap_or(&[]).iter().collect(), i)
}

// ---------------------------------------------------------------------------------------------
// Syntax tree and parser

#[derive(Debug, Clone, Copy, PartialEq)]
enum Axis {
    Child,
    Descendant,
    DescendantOrSelf,
    Parent,
    Ancestor,
    AncestorOrSelf,
    Same,
    Attribute,
    FollowingSibling,
    PrecedingSibling,
}

#[derive(Debug, Clone)]
enum Test {
    Node,
    Text,
    /// `comment()` and `processing-instruction()`: this tree holds neither.
    Never,
    AnyName,
    Name(String),
    /// `prefix:*`
    Prefix(String),
}

#[derive(Debug, Clone)]
struct Step {
    axis: Axis,
    test: Test,
    preds: Vec<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Func {
    Last,
    Position,
    Count,
    LocalName,
    Name,
    String,
    Concat,
    StartsWith,
    Contains,
    SubstringBefore,
    SubstringAfter,
    Substring,
    StringLength,
    NormalizeSpace,
    Translate,
    Boolean,
    Not,
    True,
    False,
    Number,
    Sum,
    Floor,
    Ceiling,
    Round,
}

/// (name, function, fewest arguments, most arguments)
const FUNCS: &[(&str, Func, usize, usize)] = &[
    ("last", Func::Last, 0, 0),
    ("position", Func::Position, 0, 0),
    ("count", Func::Count, 1, 1),
    ("local-name", Func::LocalName, 0, 1),
    ("name", Func::Name, 0, 1),
    ("string", Func::String, 0, 1),
    ("concat", Func::Concat, 2, usize::MAX),
    ("starts-with", Func::StartsWith, 2, 2),
    ("contains", Func::Contains, 2, 2),
    ("substring-before", Func::SubstringBefore, 2, 2),
    ("substring-after", Func::SubstringAfter, 2, 2),
    ("substring", Func::Substring, 2, 3),
    ("string-length", Func::StringLength, 0, 1),
    ("normalize-space", Func::NormalizeSpace, 0, 1),
    ("translate", Func::Translate, 3, 3),
    ("boolean", Func::Boolean, 1, 1),
    ("not", Func::Not, 1, 1),
    ("true", Func::True, 0, 0),
    ("false", Func::False, 0, 0),
    ("number", Func::Number, 0, 1),
    ("sum", Func::Sum, 1, 1),
    ("floor", Func::Floor, 1, 1),
    ("ceiling", Func::Ceiling, 1, 1),
    ("round", Func::Round, 1, 1),
];

#[derive(Debug, Clone)]
enum Start {
    Root,
    Context,
    Expr(Box<Expr>),
}

#[derive(Debug, Clone)]
enum Expr {
    Num(f64),
    Lit(String),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Union(Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
    Filter(Box<Expr>, Vec<Expr>),
    Path(Start, Vec<Step>),
}

/// A parsed XPath expression.
#[derive(Debug, Clone)]
pub(crate) struct XPath(Expr);

pub(crate) fn parse(src: &str) -> Option<XPath> {
    let mut p = Parser { toks: lex(src)?, pos: 0, depth: 0, ops: 0 };
    let e = p.expr()?;
    (p.pos == p.toks.len()).then_some(XPath(e))
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
    /// Binary and union operators parsed so far ([`MAX_OPS`]).
    ops: usize,
}

const NODE_TYPES: [&str; 4] = ["text", "node", "comment", "processing-instruction"];

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn peek_at(&self, ahead: usize) -> Option<&Tok> {
        self.toks.get(self.pos + ahead)
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        self.depth += 1;
        let r = if self.depth > MAX_DEPTH { None } else { f(self) };
        self.depth -= 1;
        r
    }

    fn expr(&mut self) -> Option<Expr> {
        self.nested(|p| p.binary(0))
    }

    /// Binary operators by precedence level: or, and, equality, relational, additive, multiplicative.
    fn binary(&mut self, level: usize) -> Option<Expr> {
        if level == 6 {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Some(op) = self.operator(level) {
            self.ops += 1;
            if self.ops > MAX_OPS {
                return None;
            }
            let right = self.binary(level + 1)?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn operator(&mut self, level: usize) -> Option<Op> {
        let op = match (level, self.peek()?) {
            (0, Tok::Name(n)) if n == "or" => Op::Or,
            (1, Tok::Name(n)) if n == "and" => Op::And,
            (2, Tok::Eq) => Op::Eq,
            (2, Tok::Ne) => Op::Ne,
            (3, Tok::Lt) => Op::Lt,
            (3, Tok::Le) => Op::Le,
            (3, Tok::Gt) => Op::Gt,
            (3, Tok::Ge) => Op::Ge,
            (4, Tok::Plus) => Op::Add,
            (4, Tok::Minus) => Op::Sub,
            (5, Tok::Star) => Op::Mul,
            (5, Tok::Name(n)) if n == "div" => Op::Div,
            (5, Tok::Name(n)) if n == "mod" => Op::Mod,
            _ => return None,
        };
        self.pos += 1;
        Some(op)
    }

    fn unary(&mut self) -> Option<Expr> {
        if self.eat(&Tok::Minus) {
            return self.nested(|p| p.unary()).map(|e| Expr::Neg(Box::new(e)));
        }
        let mut left = self.path()?;
        while self.eat(&Tok::Pipe) {
            self.ops += 1;
            if self.ops > MAX_OPS {
                return None;
            }
            let right = self.path()?;
            left = Expr::Union(Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn starts_step(&self) -> bool {
        matches!(self.peek(), Some(Tok::Name(_) | Tok::Star | Tok::At | Tok::Dot | Tok::DotDot | Tok::Axis(_)))
    }

    fn is_call(&self) -> bool {
        matches!((self.peek(), self.peek_at(1)), (Some(Tok::Name(n)), Some(Tok::LParen)) if !NODE_TYPES.contains(&n.as_str()))
    }

    fn path(&mut self) -> Option<Expr> {
        let primary = matches!(self.peek(), Some(Tok::LParen | Tok::Lit(_) | Tok::Num(_))) || self.is_call();
        if primary {
            let mut base = self.primary()?;
            let mut preds = vec![];
            while self.peek() == Some(&Tok::LBracket) {
                preds.push(self.predicate()?);
            }
            if !preds.is_empty() {
                base = Expr::Filter(Box::new(base), preds);
            }
            let mut steps = vec![];
            self.more_steps(&mut steps)?;
            return Some(if steps.is_empty() { base } else { Expr::Path(Start::Expr(Box::new(base)), steps) });
        }
        let mut steps = vec![];
        let start = if self.eat(&Tok::Slash) {
            if self.starts_step() {
                steps.push(self.step()?);
            }
            Start::Root
        } else if self.eat(&Tok::DSlash) {
            steps.push(descendant_or_self());
            steps.push(self.step()?);
            Start::Root
        } else {
            steps.push(self.step()?);
            Start::Context
        };
        self.more_steps(&mut steps)?;
        Some(Expr::Path(start, steps))
    }

    /// Steps introduced by `/` or `//`.
    fn more_steps(&mut self, steps: &mut Vec<Step>) -> Option<()> {
        loop {
            if self.eat(&Tok::Slash) {
                steps.push(self.step()?);
            } else if self.eat(&Tok::DSlash) {
                steps.push(descendant_or_self());
                steps.push(self.step()?);
            } else {
                return Some(());
            }
        }
    }

    fn predicate(&mut self) -> Option<Expr> {
        self.eat(&Tok::LBracket).then_some(())?;
        let e = self.expr()?;
        self.eat(&Tok::RBracket).then_some(e)
    }

    fn step(&mut self) -> Option<Step> {
        if self.eat(&Tok::Dot) {
            return Some(Step { axis: Axis::Same, test: Test::Node, preds: vec![] });
        }
        if self.eat(&Tok::DotDot) {
            return Some(Step { axis: Axis::Parent, test: Test::Node, preds: vec![] });
        }
        let axis = if self.eat(&Tok::At) {
            Axis::Attribute
        } else if let Some(Tok::Axis(name)) = self.peek().cloned() {
            self.pos += 1;
            match name.as_str() {
                "child" => Axis::Child,
                "descendant" => Axis::Descendant,
                "descendant-or-self" => Axis::DescendantOrSelf,
                "parent" => Axis::Parent,
                "ancestor" => Axis::Ancestor,
                "ancestor-or-self" => Axis::AncestorOrSelf,
                "self" => Axis::Same,
                "attribute" => Axis::Attribute,
                "following-sibling" => Axis::FollowingSibling,
                "preceding-sibling" => Axis::PrecedingSibling,
                _ => return None,
            }
        } else {
            Axis::Child
        };
        let test = match self.peek().cloned()? {
            Tok::Star => {
                self.pos += 1;
                Test::AnyName
            }
            Tok::Name(n) if NODE_TYPES.contains(&n.as_str()) && self.peek_at(1) == Some(&Tok::LParen) => {
                self.pos += 2;
                // processing-instruction() may carry a target literal.
                if n == "processing-instruction" && matches!(self.peek(), Some(Tok::Lit(_))) {
                    self.pos += 1;
                }
                self.eat(&Tok::RParen).then_some(())?;
                match n.as_str() {
                    "node" => Test::Node,
                    "text" => Test::Text,
                    _ => Test::Never,
                }
            }
            Tok::Name(n) => {
                self.pos += 1;
                match n.strip_suffix(":*") {
                    Some(prefix) => Test::Prefix(prefix.to_string()),
                    None => Test::Name(n),
                }
            }
            _ => return None,
        };
        let mut preds = vec![];
        while self.peek() == Some(&Tok::LBracket) {
            preds.push(self.predicate()?);
        }
        Some(Step { axis, test, preds })
    }

    fn primary(&mut self) -> Option<Expr> {
        match self.peek().cloned()? {
            Tok::LParen => {
                self.pos += 1;
                let e = self.expr()?;
                self.eat(&Tok::RParen).then_some(e)
            }
            Tok::Lit(s) => {
                self.pos += 1;
                Some(Expr::Lit(s))
            }
            Tok::Num(n) => {
                self.pos += 1;
                Some(Expr::Num(n))
            }
            Tok::Name(name) => {
                let &(_, f, min, max) = FUNCS.iter().find(|(n, ..)| *n == name)?;
                self.pos += 2;
                let mut args = vec![];
                if !self.eat(&Tok::RParen) {
                    loop {
                        args.push(self.expr()?);
                        if self.eat(&Tok::RParen) {
                            break;
                        }
                        self.eat(&Tok::Comma).then_some(())?;
                    }
                }
                (min..=max).contains(&args.len()).then_some(Expr::Call(f, args))
            }
            _ => None,
        }
    }
}

fn descendant_or_self() -> Step {
    Step { axis: Axis::DescendantOrSelf, test: Test::Node, preds: vec![] }
}

// ---------------------------------------------------------------------------------------------
// Evaluation

/// A node of the document: its index, and 0 for the node itself or *k* for its *k*-th attribute.
/// The derived order is document order.
type Nr = (usize, usize);

#[derive(Debug, Clone)]
enum Val {
    Nodes(Vec<Nr>),
    Num(f64),
    Str(String),
    Bool(bool),
}

#[derive(Clone, Copy)]
struct Cx {
    node: Nr,
    pos: usize,
    size: usize,
}

struct Eval<'d> {
    dom: &'d Dom,
    fuel: usize,
}

/// The attributes the XPath data model sees (namespace declarations are not among them).
fn visible_attrs(dom: &Dom, i: usize) -> impl Iterator<Item = (usize, &(String, String))> {
    dom.node(i).into_iter().flat_map(|n| n.attrs.iter().enumerate().filter(|(_, (k, _))| k != "xmlns" && !k.starts_with("xmlns:")))
}

impl Eval<'_> {
    fn spend(&mut self, n: usize) -> Option<()> {
        self.fuel = self.fuel.checked_sub(n)?;
        Some(())
    }

    fn string_value(&self, n: Nr) -> String {
        let (i, a) = n;
        if a > 0 {
            return self.dom.node(i).and_then(|node| node.attrs.get(a - 1)).map(|(_, v)| v.clone()).unwrap_or_default();
        }
        let mut s = String::new();
        self.dom.push_text(i, &mut s);
        s
    }

    fn qname(&self, n: Nr) -> String {
        let (i, a) = n;
        let Some(node) = self.dom.node(i) else { return String::new() };
        if a > 0 {
            return node.attrs.get(a - 1).map(|(k, _)| k.clone()).unwrap_or_default();
        }
        match &node.kind {
            Kind::Element { name, .. } => name.clone(),
            _ => String::new(),
        }
    }

    fn to_str(&self, v: &Val) -> String {
        match v {
            Val::Nodes(ns) => ns.first().map(|&n| self.string_value(n)).unwrap_or_default(),
            Val::Num(n) => num_to_string(*n),
            Val::Str(s) => s.clone(),
            Val::Bool(b) => b.to_string(),
        }
    }

    fn to_num(&self, v: &Val) -> f64 {
        match v {
            Val::Num(n) => *n,
            Val::Bool(b) => f64::from(u8::from(*b)),
            other => str_to_num(&self.to_str(other)),
        }
    }
}

fn to_bool(v: &Val) -> bool {
    match v {
        Val::Nodes(ns) => !ns.is_empty(),
        Val::Num(n) => *n != 0.0 && !n.is_nan(),
        Val::Str(s) => !s.is_empty(),
        Val::Bool(b) => *b,
    }
}

/// XPath's number-to-string: no exponent, no `.0`, `NaN`, `Infinity`.
fn num_to_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if n == 0.0 {
        "0".into()
    } else {
        n.to_string()
    }
}

/// XPath's string-to-number: optional sign, digits with an optional fraction, no exponent.
fn str_to_num(s: &str) -> f64 {
    let t = s.trim_matches([' ', '\t', '\r', '\n']);
    let body = t.strip_prefix('-').unwrap_or(t);
    let valid = !body.is_empty() && body != "." && body.chars().all(|c| c.is_ascii_digit() || c == '.') && body.matches('.').count() <= 1;
    if valid { t.parse().unwrap_or(f64::NAN) } else { f64::NAN }
}

fn cmp_num(op: Op, a: f64, b: f64) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        Op::Lt => a < b,
        Op::Le => a <= b,
        Op::Gt => a > b,
        _ => a >= b,
    }
}

impl Eval<'_> {
    fn run(&mut self, e: &Expr, cx: Cx) -> Option<Val> {
        match e {
            Expr::Num(n) => Some(Val::Num(*n)),
            Expr::Lit(s) => Some(Val::Str(s.clone())),
            Expr::Neg(e) => {
                let v = self.run(e, cx)?;
                Some(Val::Num(-self.to_num(&v)))
            }
            Expr::Bin(op, l, r) => self.binary(*op, l, r, cx),
            Expr::Union(l, r) => {
                let (Val::Nodes(mut a), Val::Nodes(b)) = (self.run(l, cx)?, self.run(r, cx)?) else { return None };
                a.extend(b);
                a.sort_unstable();
                a.dedup();
                Some(Val::Nodes(a))
            }
            Expr::Call(f, args) => self.call(*f, args, cx),
            Expr::Filter(base, preds) => {
                let Val::Nodes(mut ns) = self.run(base, cx)? else { return None };
                for p in preds {
                    ns = self.filter(ns, p)?;
                }
                Some(Val::Nodes(ns))
            }
            Expr::Path(start, steps) => {
                let mut set = match start {
                    Start::Root => vec![(0, 0)],
                    Start::Context => vec![cx.node],
                    Start::Expr(e) => match self.run(e, cx)? {
                        Val::Nodes(ns) => ns,
                        _ => return None,
                    },
                };
                for s in steps {
                    set = self.step(&set, s)?;
                }
                Some(Val::Nodes(set))
            }
        }
    }

    fn binary(&mut self, op: Op, l: &Expr, r: &Expr, cx: Cx) -> Option<Val> {
        match op {
            Op::Or => {
                let a = to_bool(&self.run(l, cx)?);
                Some(Val::Bool(a || to_bool(&self.run(r, cx)?)))
            }
            Op::And => {
                let a = to_bool(&self.run(l, cx)?);
                Some(Val::Bool(a && to_bool(&self.run(r, cx)?)))
            }
            Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                let (a, b) = (self.run(l, cx)?, self.run(r, cx)?);
                self.compare(op, &a, &b).map(Val::Bool)
            }
            _ => {
                let a = self.run(l, cx)?;
                let a = self.to_num(&a);
                let b = self.run(r, cx)?;
                let b = self.to_num(&b);
                Some(Val::Num(match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => a / b,
                    _ => a % b,
                }))
            }
        }
    }

    /// XPath 1.0 §3.4: a comparison with a node-set holds when it holds for some node.
    fn compare(&mut self, op: Op, a: &Val, b: &Val) -> Option<bool> {
        let eq_like = matches!(op, Op::Eq | Op::Ne);
        match (a, b) {
            (Val::Nodes(x), Val::Nodes(y)) => {
                self.spend(x.len().saturating_mul(y.len()))?;
                let ys: Vec<String> = y.iter().map(|&n| self.string_value(n)).collect();
                Some(x.iter().any(|&n| {
                    let s = self.string_value(n);
                    ys.iter().any(|t| if eq_like { cmp_str_eq(op, &s, t) } else { cmp_num(op, str_to_num(&s), str_to_num(t)) })
                }))
            }
            (Val::Nodes(ns), other) | (other, Val::Nodes(ns)) => {
                self.spend(ns.len())?;
                let swapped = matches!(b, Val::Nodes(_)) && !matches!(a, Val::Nodes(_));
                if let Val::Bool(x) = other {
                    let nodes = !ns.is_empty();
                    return Some(if swapped { cmp_bool(op, *x, nodes) } else { cmp_bool(op, nodes, *x) });
                }
                Some(ns.iter().any(|&n| {
                    let s = self.string_value(n);
                    match other {
                        Val::Num(x) => {
                            let v = str_to_num(&s);
                            if swapped { cmp_num(op, *x, v) } else { cmp_num(op, v, *x) }
                        }
                        Val::Str(x) if eq_like => cmp_str_eq(op, &s, x),
                        Val::Str(x) => {
                            let (v, w) = (str_to_num(&s), str_to_num(x));
                            if swapped { cmp_num(op, w, v) } else { cmp_num(op, v, w) }
                        }
                        _ => false,
                    }
                }))
            }
            _ if eq_like => Some(match (a, b) {
                (Val::Bool(_), _) | (_, Val::Bool(_)) => cmp_bool(op, to_bool(a), to_bool(b)),
                (Val::Num(_), _) | (_, Val::Num(_)) => cmp_num(op, self.to_num(a), self.to_num(b)),
                _ => cmp_str_eq(op, &self.to_str(a), &self.to_str(b)),
            }),
            _ => Some(cmp_num(op, self.to_num(a), self.to_num(b))),
        }
    }

    /// Keeps the nodes for which predicate `p` holds; a number selects by position.
    fn filter(&mut self, nodes: Vec<Nr>, p: &Expr) -> Option<Vec<Nr>> {
        let size = nodes.len();
        let mut out = Vec::with_capacity(size.min(1024));
        for (k, &node) in nodes.iter().enumerate() {
            self.spend(1)?;
            let v = self.run(p, Cx { node, pos: k + 1, size })?;
            let keep = match v {
                Val::Num(n) => n == (k + 1) as f64,
                other => to_bool(&other),
            };
            if keep {
                out.push(node);
            }
        }
        Some(out)
    }

    fn step(&mut self, input: &[Nr], step: &Step) -> Option<Vec<Nr>> {
        let mut out: Vec<Nr> = vec![];
        for &n in input {
            let mut cands = vec![];
            self.axis(n, step.axis, &mut cands)?;
            cands.retain(|&c| self.matches(c, step));
            for p in &step.preds {
                cands = self.filter(cands, p)?;
            }
            out.extend(cands);
        }
        out.sort_unstable();
        out.dedup();
        Some(out)
    }

    /// The nodes on `axis` from `n`, nearest first on reverse axes.
    fn axis(&mut self, n: Nr, axis: Axis, out: &mut Vec<Nr>) -> Option<()> {
        let (i, a) = n;
        let node = self.dom.node(i)?;
        let on_element = a == 0;
        match axis {
            Axis::Same => out.push(n),
            Axis::Child if on_element => out.extend(node.children.iter().map(|&c| (c, 0))),
            Axis::Descendant | Axis::DescendantOrSelf => {
                if axis == Axis::DescendantOrSelf {
                    out.push(n);
                }
                if on_element {
                    let mut stack: Vec<usize> = node.children.iter().rev().copied().collect();
                    while let Some(d) = stack.pop() {
                        self.spend(1)?;
                        out.push((d, 0));
                        stack.extend(self.dom.node(d)?.children.iter().rev().copied());
                    }
                }
            }
            Axis::Parent => {
                if !on_element {
                    out.push((i, 0));
                } else if let Some(p) = node.parent {
                    out.push((p, 0));
                }
            }
            Axis::Ancestor | Axis::AncestorOrSelf => {
                if axis == Axis::AncestorOrSelf {
                    out.push(n);
                }
                let mut cur = if on_element { node.parent } else { Some(i) };
                while let Some(p) = cur {
                    self.spend(1)?;
                    out.push((p, 0));
                    cur = self.dom.node(p)?.parent;
                }
            }
            Axis::Attribute if on_element => out.extend(visible_attrs(self.dom, i).map(|(k, _)| (i, k + 1))),
            Axis::FollowingSibling | Axis::PrecedingSibling if on_element => {
                let siblings = &self.dom.node(node.parent?)?.children;
                let at = siblings.iter().position(|&s| s == i)?;
                if axis == Axis::FollowingSibling {
                    out.extend(siblings.get(at + 1..)?.iter().map(|&s| (s, 0)));
                } else {
                    out.extend(siblings.get(..at)?.iter().rev().map(|&s| (s, 0)));
                }
            }
            _ => {}
        }
        self.spend(out.len())
    }

    fn matches(&self, n: Nr, step: &Step) -> bool {
        let (i, a) = n;
        let Some(node) = self.dom.node(i) else { return false };
        let attribute_axis = step.axis == Axis::Attribute;
        match &step.test {
            Test::Node => true,
            Test::Text => a == 0 && matches!(node.kind, Kind::Text(_)),
            Test::Never => false,
            Test::AnyName => {
                if attribute_axis {
                    a > 0
                } else {
                    a == 0 && matches!(node.kind, Kind::Element { .. })
                }
            }
            Test::Name(q) | Test::Prefix(q) => {
                let prefix_only = matches!(step.test, Test::Prefix(_));
                let name_ok = |name: &str| {
                    if prefix_only { name.strip_prefix(q.as_str()).is_some_and(|r| r.starts_with(':')) } else { name == q }
                };
                if attribute_axis {
                    a > 0 && node.attrs.get(a - 1).is_some_and(|(k, _)| name_ok(k))
                } else {
                    match &node.kind {
                        Kind::Element { name, in_default_ns } if a == 0 => name_ok(name) && !(*in_default_ns && !q.contains(':') && !prefix_only),
                        _ => false,
                    }
                }
            }
        }
    }

    fn arg_str(&mut self, args: &[Expr], i: usize, cx: Cx) -> Option<String> {
        match args.get(i) {
            Some(e) => {
                let v = self.run(e, cx)?;
                Some(self.to_str(&v))
            }
            None => Some(self.string_value(cx.node)),
        }
    }

    fn arg_num(&mut self, args: &[Expr], i: usize, cx: Cx) -> Option<f64> {
        let v = self.run(args.get(i)?, cx)?;
        Some(self.to_num(&v))
    }

    fn arg_nodes(&mut self, args: &[Expr], i: usize, cx: Cx) -> Option<Vec<Nr>> {
        match args.get(i) {
            Some(e) => match self.run(e, cx)? {
                Val::Nodes(ns) => Some(ns),
                _ => None,
            },
            None => Some(vec![cx.node]),
        }
    }

    fn call(&mut self, f: Func, args: &[Expr], cx: Cx) -> Option<Val> {
        let s = |v: String| Some(Val::Str(v));
        match f {
            Func::Last => Some(Val::Num(cx.size as f64)),
            Func::Position => Some(Val::Num(cx.pos as f64)),
            Func::Count => Some(Val::Num(self.arg_nodes(args, 0, cx)?.len() as f64)),
            Func::LocalName | Func::Name => {
                let mut ns = self.arg_nodes(args, 0, cx)?;
                ns.sort_unstable();
                let q = ns.first().map(|&n| self.qname(n)).unwrap_or_default();
                s(if f == Func::LocalName { q.rsplit(':').next().unwrap_or("").to_string() } else { q })
            }
            Func::String => s(self.arg_str(args, 0, cx)?),
            Func::Concat => {
                let mut out = String::new();
                for i in 0..args.len() {
                    out.push_str(&self.arg_str(args, i, cx)?);
                }
                s(out)
            }
            Func::StartsWith => Some(Val::Bool(self.arg_str(args, 0, cx)?.starts_with(&self.arg_str(args, 1, cx)?))),
            Func::Contains => Some(Val::Bool(self.arg_str(args, 0, cx)?.contains(&self.arg_str(args, 1, cx)?))),
            Func::SubstringBefore => {
                let (a, b) = (self.arg_str(args, 0, cx)?, self.arg_str(args, 1, cx)?);
                s(a.find(&b).and_then(|at| a.get(..at)).unwrap_or("").to_string())
            }
            Func::SubstringAfter => {
                let (a, b) = (self.arg_str(args, 0, cx)?, self.arg_str(args, 1, cx)?);
                s(a.find(&b).and_then(|at| a.get(at + b.len()..)).unwrap_or("").to_string())
            }
            Func::Substring => {
                let text = self.arg_str(args, 0, cx)?;
                let start = round(self.arg_num(args, 1, cx)?);
                let end = if args.len() > 2 { start + round(self.arg_num(args, 2, cx)?) } else { f64::INFINITY };
                s(text
                    .chars()
                    .enumerate()
                    .filter(|&(k, _)| {
                        let p = (k + 1) as f64;
                        p >= start && p < end
                    })
                    .map(|(_, c)| c)
                    .collect())
            }
            Func::StringLength => Some(Val::Num(self.arg_str(args, 0, cx)?.chars().count() as f64)),
            Func::NormalizeSpace => s(self.arg_str(args, 0, cx)?.split_whitespace().collect::<Vec<_>>().join(" ")),
            Func::Translate => {
                let text = self.arg_str(args, 0, cx)?;
                let from: Vec<char> = self.arg_str(args, 1, cx)?.chars().collect();
                let to: Vec<char> = self.arg_str(args, 2, cx)?.chars().collect();
                s(text
                    .chars()
                    .filter_map(|c| match from.iter().position(|&f| f == c) {
                        Some(k) => to.get(k).copied(),
                        None => Some(c),
                    })
                    .collect())
            }
            Func::Boolean => Some(Val::Bool(to_bool(&self.run(args.first()?, cx)?))),
            Func::Not => Some(Val::Bool(!to_bool(&self.run(args.first()?, cx)?))),
            Func::True => Some(Val::Bool(true)),
            Func::False => Some(Val::Bool(false)),
            Func::Number => match args.first() {
                Some(e) => {
                    let v = self.run(e, cx)?;
                    Some(Val::Num(self.to_num(&v)))
                }
                None => Some(Val::Num(str_to_num(&self.string_value(cx.node)))),
            },
            Func::Sum => {
                let ns = self.arg_nodes(args, 0, cx)?;
                self.spend(ns.len())?;
                Some(Val::Num(ns.iter().map(|&n| str_to_num(&self.string_value(n))).sum()))
            }
            Func::Floor => Some(Val::Num(self.arg_num(args, 0, cx)?.floor())),
            Func::Ceiling => Some(Val::Num(self.arg_num(args, 0, cx)?.ceil())),
            Func::Round => Some(Val::Num(round(self.arg_num(args, 0, cx)?))),
        }
    }
}

/// XPath's `round`: halves go toward positive infinity.
fn round(x: f64) -> f64 {
    if x.is_finite() { (x + 0.5).floor() } else { x }
}

fn cmp_str_eq(op: Op, a: &str, b: &str) -> bool {
    (a == b) == (op == Op::Eq)
}

fn cmp_bool(op: Op, a: bool, b: bool) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        _ => cmp_num(op, f64::from(u8::from(a)), f64::from(u8::from(b))),
    }
}

/// Text that reads as a plain number (`12`, `-3.5`, `1e3`) becomes a number; anything else stays text.
fn cell_value(s: String) -> Value {
    let t = s.trim();
    let plain = !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'));
    match t.parse::<f64>() {
        Ok(n) if plain && n.is_finite() => Value::from(n),
        _ => Value::from(s),
    }
}

/// Evaluates `xpath` on `dom` from the document node. A node-set gives one value per node in
/// document order (empty when nothing matches); anything else gives one value. `None` when the
/// evaluation fails or runs out of work, or a number result isn't finite.
pub(crate) fn evaluate(dom: &Dom, xpath: &XPath) -> Option<Vec<Value>> {
    let mut ev = Eval { dom, fuel: FUEL };
    let v = ev.run(&xpath.0, Cx { node: (0, 0), pos: 1, size: 1 })?;
    match v {
        Val::Nodes(ns) => Some(ns.into_iter().map(|n| cell_value(ev.string_value(n))).collect()),
        Val::Num(n) if n.is_finite() => Some(vec![Value::from(n)]),
        Val::Num(_) => None,
        Val::Str(s) => Some(vec![cell_value(s)]),
        Val::Bool(b) => Some(vec![Value::Bool(b)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    fn run(xml_src: &str, path: &str) -> Option<Vec<Value>> {
        let dom = xml::parse(xml_src)?;
        evaluate(&dom, &parse(path)?)
    }

    fn texts(xml_src: &str, path: &str) -> Vec<String> {
        run(xml_src, path)
            .unwrap_or_default()
            .into_iter()
            .map(|v| match v {
                Value::Text(t) => t.to_string(),
                Value::Number(n) => format!("#{n}"),
                Value::Bool(b) => b.to_string(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    const BOOKS: &str = "<lib><book id='1' lang='en'><t>A</t><p>10</p></book><book id='2' lang='de'><t>B</t><p>20.5</p></book><book id='3' lang='en'><t>C</t><p>5</p></book></lib>";

    #[test]
    fn selects_by_path_and_axis() {
        assert_eq!(texts(BOOKS, "/lib/book/t"), ["A", "B", "C"]);
        assert_eq!(texts(BOOKS, "//t"), ["A", "B", "C"]);
        assert_eq!(texts(BOOKS, "lib/book[2]/t"), ["B"]);
        assert_eq!(texts(BOOKS, "//book/@id"), ["#1", "#2", "#3"]);
        assert_eq!(texts(BOOKS, "//book[@lang='en']/t"), ["A", "C"]);
        assert_eq!(texts(BOOKS, "//t[.='B']/../p"), ["#20.5"]);
        assert_eq!(texts(BOOKS, "//book[t='C']/preceding-sibling::book/t"), ["A", "B"]);
        assert_eq!(texts(BOOKS, "//book[1]/following-sibling::*/t"), ["B", "C"]);
        assert_eq!(texts(BOOKS, "//p/ancestor::lib/book[1]/t"), ["A"]);
        assert_eq!(texts(BOOKS, "//t/descendant-or-self::t"), ["A", "B", "C"]);
        assert_eq!(texts(BOOKS, "//book[1]/*"), ["A", "#10"]);
        assert_eq!(texts(BOOKS, "//book[1]/@*"), ["#1", "en"]);
        assert_eq!(texts(BOOKS, "//t/text()"), ["A", "B", "C"]);
        assert_eq!(texts(BOOKS, "//book[3]/self::book/t"), ["C"]);
    }

    #[test]
    fn positions_and_last() {
        assert_eq!(texts(BOOKS, "//book[last()]/t"), ["C"]);
        assert_eq!(texts(BOOKS, "//book[position()>1]/t"), ["B", "C"]);
        assert_eq!(texts(BOOKS, "//book[position()=1 or position()=last()]/t"), ["A", "C"]);
        assert_eq!(texts(BOOKS, "(//book)[2]/t"), ["B"]);
        assert_eq!(texts(BOOKS, "//book[0]/t"), Vec::<String>::new());
        assert_eq!(texts(BOOKS, "//book[9]/t"), Vec::<String>::new());
    }

    #[test]
    fn unions_come_out_in_document_order() {
        assert_eq!(texts(BOOKS, "//p|//t"), ["A", "#10", "B", "#20.5", "C", "#5"]);
        assert_eq!(texts(BOOKS, "//t|//t"), ["A", "B", "C"]);
    }

    #[test]
    fn compares_node_sets_the_xpath_way() {
        assert_eq!(texts(BOOKS, "//book[p>15]/t"), ["B"]);
        assert_eq!(texts(BOOKS, "//book[p<=10]/t"), ["A", "C"]);
        assert_eq!(texts(BOOKS, "//book[p!=10]/t"), ["B", "C"]);
        assert_eq!(texts(BOOKS, "//book[@id=2]/t"), ["B"]);
        assert_eq!(texts(BOOKS, "//book[@id>1 and @lang='en']/t"), ["C"]);
        assert_eq!(texts(BOOKS, "//book[t=../book[2]/t]/@id"), ["#2"]);
        assert_eq!(texts(BOOKS, "//book[not(@lang='en')]/t"), ["B"]);
        assert_eq!(texts(BOOKS, "//book[@missing]/t"), Vec::<String>::new());
        assert_eq!(texts(BOOKS, "//book[p]/t"), ["A", "B", "C"]);
        assert_eq!(texts(BOOKS, "//book[15<p]/t"), ["B"]);
    }

    #[test]
    fn scalar_functions() {
        assert_eq!(texts(BOOKS, "count(//book)"), ["#3"]);
        assert_eq!(texts(BOOKS, "sum(//p)"), ["#35.5"]);
        assert_eq!(texts(BOOKS, "concat(//t[1],'-',(//t)[3])"), ["A-C"]);
        assert_eq!(texts(BOOKS, "string(//book[2]/@lang)"), ["de"]);
        assert_eq!(texts(BOOKS, "name(//book[1]/*[1])"), ["t"]);
        assert_eq!(texts(BOOKS, "local-name(/*)"), ["lib"]);
        assert_eq!(texts(BOOKS, "string-length(//t)"), ["#1"]);
        assert_eq!(texts(BOOKS, "contains(//t,'A')"), ["true"]);
        assert_eq!(texts(BOOKS, "starts-with('abc','ab')"), ["true"]);
        assert_eq!(texts(BOOKS, "substring-before('a-b','-')"), ["a"]);
        assert_eq!(texts(BOOKS, "substring-after('a-b','-')"), ["b"]);
        assert_eq!(texts(BOOKS, "substring('12345',2,3)"), ["#234"]);
        assert_eq!(texts(BOOKS, "substring('12345',2)"), ["#2345"]);
        assert_eq!(texts(BOOKS, "substring('12345',0)"), ["#12345"]);
        assert_eq!(texts(BOOKS, "substring('12345',1.5,2.6)"), ["#234"]);
        assert_eq!(texts(BOOKS, "normalize-space('  a  b ')"), ["a b"]);
        assert_eq!(texts(BOOKS, "translate('bar','abc','ABC')"), ["BAr"]);
        assert_eq!(texts(BOOKS, "translate('--aaa--','abc-','ABC')"), ["AAA"]);
        assert_eq!(texts(BOOKS, "number('12')+1"), ["#13"]);
        assert_eq!(texts(BOOKS, "floor(2.7)+ceiling(2.1)+round(2.5)"), ["#8"]);
        assert_eq!(texts(BOOKS, "round(-2.5)"), ["#-2"]);
        assert_eq!(texts(BOOKS, "7 mod 3 + 8 div 4 * 2"), ["#5"]);
        assert_eq!(texts(BOOKS, "-(2+3)"), ["#-5"]);
        assert_eq!(texts(BOOKS, "boolean(//zzz)"), ["false"]);
        assert_eq!(texts(BOOKS, "true() and not(false())"), ["true"]);
        assert_eq!(texts(BOOKS, "count(//book[1]/*) * 2"), ["#4"]);
    }

    #[test]
    fn numeric_text_becomes_a_number() {
        assert_eq!(run("<a>12</a>", "//a"), Some(vec![Value::from(12.0)]));
        assert_eq!(run("<a> 1.5e2 </a>", "//a"), Some(vec![Value::from(150.0)]));
        assert_eq!(run("<a>12abc</a>", "//a"), Some(vec![Value::from("12abc")]));
        assert_eq!(run("<a>inf</a>", "//a"), Some(vec![Value::from("inf")]));
        assert_eq!(run("<a>NaN</a>", "//a"), Some(vec![Value::from("NaN")]));
        assert_eq!(run("<a>e</a>", "//a"), Some(vec![Value::from("e")]));
        assert_eq!(run("<a>-</a>", "//a"), Some(vec![Value::from("-")]));
        assert_eq!(run("<a>007</a>", "//a"), Some(vec![Value::from(7.0)]));
    }

    #[test]
    fn empty_element_is_empty_text() {
        assert_eq!(run("<a><b/></a>", "//b"), Some(vec![Value::from("")]));
    }

    #[test]
    fn no_match_is_an_empty_list() {
        assert_eq!(run(BOOKS, "//zzz"), Some(vec![]));
    }

    #[test]
    fn rejects_bad_expressions() {
        for bad in [
            "",
            "//",
            "//a[",
            "//a[1",
            "a b",
            "nope(1)",
            "count()",
            "count(1,2)",
            "//@",
            "1 +",
            "//a/",
            "bogus::a",
            "'x",
            "//a]",
            "//a | ",
            "(//a",
            "$x",
            "//a!b",
            "//a[]",
            "concat('a')",
            "following::a",
            "//a::b",
        ] {
            assert!(parse(bad).is_none(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn type_errors_fail() {
        assert!(run(BOOKS, "count('x')").is_none());
        assert!(run(BOOKS, "'x'/a").is_none());
        assert!(run(BOOKS, "1|2").is_none());
        assert!(run(BOOKS, "1 div 0").is_none());
        assert!(run(BOOKS, "number('x')").is_none());
    }

    #[test]
    fn default_namespace_hides_unprefixed_names() {
        let ns = "<feed xmlns='urn:f'><entry>x</entry></feed>";
        assert_eq!(run(ns, "//entry"), Some(vec![]));
        assert_eq!(texts(ns, "//*[local-name()='entry']"), ["x"]);
        assert_eq!(texts(ns, "//*"), ["x", "x"]);
        let prefixed = "<a:r xmlns:a='urn:a'><a:i>1</a:i><a:j/></a:r>";
        assert_eq!(texts(prefixed, "//a:i"), ["#1"]);
        assert_eq!(texts(prefixed, "count(//a:*)"), ["#3"]);
        assert_eq!(texts(prefixed, "local-name(//a:i)"), ["i"]);
        assert_eq!(texts("<r xmlns:q='u' q:k='v'/>", "count(//@*)"), ["#1"]);
    }

    #[test]
    fn star_and_keywords_are_operators_or_names_by_position() {
        assert_eq!(texts("<r><div>3</div><mod>2</mod><and>1</and><or>0</or></r>", "//div * //mod"), ["#6"]);
        assert_eq!(texts("<r><div>3</div><mod>2</mod></r>", "//div mod //mod"), ["#1"]);
        assert_eq!(texts("<r><and>1</and><or>0</or></r>", "//and=1 and //or=1"), ["false"]);
        assert_eq!(texts("<r><and>1</and><or>0</or></r>", "//and=1 or //or=1"), ["true"]);
        assert_eq!(texts("<r><x>4</x></r>", "/r/*"), ["#4"]);
    }

    #[test]
    fn hostile_expressions_are_bounded() {
        // Nesting
        let deep = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
        assert!(parse(&deep).is_none());
        let chain = format!("1{}", "+1".repeat(15_000));
        assert!(parse(&chain).is_none());
        let union = format!("//a{}", "|//a".repeat(15_000));
        assert!(parse(&union).is_none());
        let mixed = format!("{}1{}", "(".repeat(30), ")".repeat(30));
        assert!(parse(&mixed).is_some());
        // Finished groups count too: 60 nested groups of 100 additions each is far past the limit.
        let mut grouped = "1".to_string();
        for _ in 0..60 {
            grouped = format!("({grouped}{})", "+1".repeat(100));
        }
        assert!(parse(&grouped).is_none());
        // A long chain that also groups still parses while within the limit.
        assert!(parse(&format!("1{}+(2+3)", "+1".repeat(100))).is_some());
        // The largest accepted expressions evaluate and drop on a small stack.
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let dom = xml::parse("<r><a>1</a></r>").unwrap();
                let sum = format!("1{}", "+1".repeat(MAX_OPS - 1));
                assert_eq!(evaluate(&dom, &parse(&sum).unwrap()), Some(vec![Value::from(MAX_OPS as f64)]));
                let union = format!("//a{}", "|//a".repeat(MAX_OPS - 1));
                assert_eq!(evaluate(&dom, &parse(&union).unwrap()), Some(vec![Value::from(1.0)]));
                let nested = format!("{}{sum}{}", "not(".repeat(60), ")".repeat(60));
                assert!(evaluate(&dom, &parse(&nested).unwrap()).is_some());
            })
            .unwrap()
            .join()
            .unwrap();
        let preds = format!("//a{}", "[1]".repeat(5000));
        assert!(parse(&preds).is_some());
        let nested_preds = format!("//a{}1{}", "[b".repeat(1000), "]".repeat(1000));
        assert!(parse(&nested_preds).is_none());
        // Work: a quadratic comparison over a wide document runs out of fuel instead of hanging.
        let wide = format!("<r>{}</r>", "<a>1</a>".repeat(40_000));
        let dom = xml::parse(&wide).unwrap();
        let q = parse("//a[//a=//a]").unwrap();
        assert!(evaluate(&dom, &q).is_none());
        // ... and a sane query over the same document is fine.
        let ok = parse("count(//a)").unwrap();
        assert_eq!(evaluate(&dom, &ok), Some(vec![Value::from(40_000.0)]));
    }

    #[test]
    fn long_literals_and_names_with_multibyte_characters() {
        assert_eq!(texts("<é>日本</é>", "//é"), ["日本"]);
        assert_eq!(texts("<a>x</a>", "concat('日本','語')"), ["日本語"]);
        assert_eq!(texts("<a>日本語</a>", "substring(//a,2,1)"), ["本"]);
    }
}
