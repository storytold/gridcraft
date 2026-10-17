//! A small XML tree and an XPath 1.0 evaluator for FILTERXML.
//!
//! Covers what worksheets use: location paths (`/a/b`, `//b`, `.`, `..`, `*`, `@attr`, `@*`,
//! `text()`, `node()`), the common axes (`child::`, `descendant::`, `parent::`, `ancestor::`,
//! `following-sibling::`, `preceding-sibling::`, `self::`, `attribute::`…), predicates (`[2]`,
//! `[last()]`, `[@id='x']`, `[price>35]`, `[not(@x)]`), unions, comparisons, arithmetic and the
//! core string, number and boolean functions. Whitespace-only text between elements is dropped,
//! as Excel's XML parser does. Every size is bounded: nodes, nesting, expression depth and the
//! work one evaluation may do.

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

/// Most nodes kept from one document.
const MAX_NODES: usize = 200_000;
/// Deepest element nesting accepted.
const MAX_DEPTH: usize = 256;
/// Deepest expression nesting accepted.
const MAX_EXPR_DEPTH: usize = 64;
/// Node visits one evaluation may make before it gives up.
const MAX_WORK: usize = 20_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Root,
    Element,
    Text,
}

#[derive(Clone, Debug)]
struct Node {
    kind: Kind,
    name: String,
    parent: Option<usize>,
    children: Vec<usize>,
    attrs: Vec<(String, String)>,
    text: String,
}

/// A parsed document; node 0 is the root (above the document element).
#[derive(Clone, Debug)]
pub struct Doc {
    nodes: Vec<Node>,
}

/// A node of a node-set: a tree node, or attribute `i` of element `e`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeRef {
    Node(usize),
    Attr(usize, usize),
}

impl NodeRef {
    /// Document order key: an element's attributes come right after it.
    fn order(self) -> (usize, usize) {
        match self {
            NodeRef::Node(n) => (n, 0),
            NodeRef::Attr(e, i) => (e, i + 1),
        }
    }
}

impl Doc {
    /// Parses XML text. Errors (malformed XML, too large or too deep) are `None`.
    pub fn parse(xml: &str) -> Option<Doc> {
        let mut nodes = vec![Node { kind: Kind::Root, name: String::new(), parent: None, children: vec![], attrs: vec![], text: String::new() }];
        let mut stack: Vec<usize> = vec![0];
        let mut reader = Reader::from_str(xml);
        {
            let cfg = reader.config_mut();
            cfg.trim_text_start = false;
            cfg.trim_text_end = false;
        }
        let mut seen_root = false;
        let add = |nodes: &mut Vec<Node>, stack: &[usize], node: Node| -> Option<usize> {
            if nodes.len() >= MAX_NODES {
                return None;
            }
            let parent = *stack.last()?;
            let id = nodes.len();
            nodes.push(Node { parent: Some(parent), ..node });
            nodes.get_mut(parent)?.children.push(id);
            Some(id)
        };
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    if stack.len() == 1 {
                        if seen_root {
                            return None;
                        }
                        seen_root = true;
                    }
                    let el = element(&e)?;
                    let id = add(&mut nodes, &stack, el)?;
                    if stack.len() > MAX_DEPTH {
                        return None;
                    }
                    stack.push(id);
                }
                Ok(Event::Empty(e)) => {
                    if stack.len() == 1 {
                        if seen_root {
                            return None;
                        }
                        seen_root = true;
                    }
                    let el = element(&e)?;
                    add(&mut nodes, &stack, el)?;
                }
                Ok(Event::End(e)) => {
                    let id = stack.pop()?;
                    if stack.is_empty() || nodes.get(id)?.name.as_bytes() != e.name().as_ref() {
                        return None;
                    }
                }
                Ok(Event::Text(t)) => {
                    let text = t.xml_content(XmlVersion::Explicit1_0).ok()?.into_owned();
                    push_text(&mut nodes, &stack, text, &add)?;
                }
                Ok(Event::GeneralRef(r)) => {
                    let c = match r.resolve_char_ref() {
                        Ok(Some(c)) => c,
                        _ => entity(&String::from_utf8_lossy(&r))?,
                    };
                    push_text(&mut nodes, &stack, c.to_string(), &add)?;
                }
                Ok(Event::CData(t)) => {
                    let text = String::from_utf8_lossy(t.as_ref()).into_owned();
                    push_text(&mut nodes, &stack, text, &add)?;
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        if stack.len() != 1 || !seen_root {
            return None;
        }
        // Whitespace-only text between elements isn't data.
        let blank: Vec<usize> = nodes.iter().enumerate().filter(|(_, n)| n.kind == Kind::Text && n.text.trim().is_empty()).map(|(i, _)| i).collect();
        if !blank.is_empty() {
            for n in nodes.iter_mut() {
                n.children.retain(|c| blank.binary_search(c).is_err());
            }
        }
        Some(Doc { nodes })
    }

    fn node(&self, i: usize) -> Option<&Node> {
        self.nodes.get(i)
    }

    /// A node's string value: an element's descendant text, an attribute's or text node's value.
    pub fn string_value(&self, r: NodeRef) -> String {
        match r {
            NodeRef::Attr(e, i) => self.node(e).and_then(|n| n.attrs.get(i)).map(|(_, v)| v.clone()).unwrap_or_default(),
            NodeRef::Node(n) => {
                let Some(node) = self.node(n) else { return String::new() };
                if node.kind == Kind::Text {
                    return node.text.clone();
                }
                let mut out = String::new();
                let mut stack: Vec<usize> = node.children.iter().rev().copied().collect();
                while let Some(c) = stack.pop() {
                    let Some(child) = self.node(c) else { continue };
                    if child.kind == Kind::Text {
                        out.push_str(&child.text);
                    } else {
                        stack.extend(child.children.iter().rev());
                    }
                }
                out
            }
        }
    }

    fn name(&self, r: NodeRef) -> String {
        match r {
            NodeRef::Attr(e, i) => self.node(e).and_then(|n| n.attrs.get(i)).map(|(k, _)| k.clone()).unwrap_or_default(),
            NodeRef::Node(n) => self.node(n).filter(|n| n.kind == Kind::Element).map(|n| n.name.clone()).unwrap_or_default(),
        }
    }
}

fn push_text(nodes: &mut Vec<Node>, stack: &[usize], text: String, add: &dyn Fn(&mut Vec<Node>, &[usize], Node) -> Option<usize>) -> Option<()> {
    if stack.len() == 1 {
        // Text outside the document element must be blank.
        return text.trim().is_empty().then_some(());
    }
    let parent = *stack.last()?;
    // Adjacent text (text, a reference, CDATA) is one text node.
    if let Some(&last) = nodes.get(parent)?.children.last()
        && let Some(n) = nodes.get_mut(last)
        && n.kind == Kind::Text
    {
        if n.text.len() < 1 << 20 {
            n.text.push_str(&text);
        }
        return Some(());
    }
    add(nodes, stack, Node { kind: Kind::Text, name: String::new(), parent: None, children: vec![], attrs: vec![], text })?;
    Some(())
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix('x') {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

fn element(e: &quick_xml::events::BytesStart<'_>) -> Option<Node> {
    let name = String::from_utf8(e.name().as_ref().to_vec()).ok()?;
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.ok()?;
        let key = String::from_utf8(a.key.as_ref().to_vec()).ok()?;
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let value = a.normalized_value(XmlVersion::Explicit1_0).ok()?.into_owned();
        attrs.push((key, value));
    }
    Some(Node { kind: Kind::Element, name, parent: None, children: vec![], attrs, text: String::new() })
}

// ---------------------------------------------------------------- expressions

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Child,
    Descendant,
    DescendantOrSelf,
    Itself,
    Parent,
    Ancestor,
    AncestorOrSelf,
    Attribute,
    FollowingSibling,
    PrecedingSibling,
    Following,
    Preceding,
}

#[derive(Clone, Debug, PartialEq)]
enum Test {
    /// `*`, or `prefix:*`.
    Any(Option<String>),
    Name(String),
    Text,
    Node,
}

#[derive(Clone, Debug, PartialEq)]
struct Step {
    axis: Axis,
    test: Test,
    preds: Vec<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
enum Expr {
    /// A location path; `absolute` starts at the root.
    Path {
        absolute: bool,
        steps: Vec<Step>,
    },
    /// A filter expression followed by a relative path (`(…)[1]/b`).
    Filter {
        base: Box<Expr>,
        preds: Vec<Expr>,
        steps: Vec<Step>,
    },
    Num(f64),
    Str(String),
    Call(String, Vec<Expr>),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    Union,
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Slash,
    DSlash,
    LParen,
    RParen,
    LBracket,
    RBracket,
    At,
    Comma,
    Pipe,
    Dot,
    DDot,
    ColonColon,
    Star,
    Op(Op),
    Name(String),
    Num(f64),
    Str(String),
}

fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let cs: Vec<char> = s.chars().collect();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    // `*` and operator names are operators unless they start an expression (XPath 3.7).
    let operator_context = |out: &Vec<Tok>| {
        !matches!(
            out.last(),
            None | Some(Tok::At | Tok::ColonColon | Tok::LParen | Tok::LBracket | Tok::Comma | Tok::Op(_) | Tok::Slash | Tok::DSlash | Tok::Pipe)
        )
    };
    while let Some(&c) = cs.get(i) {
        let next = cs.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '/' if next == Some('/') => {
                out.push(Tok::DSlash);
                i += 2;
            }
            '/' => {
                out.push(Tok::Slash);
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '[' => {
                out.push(Tok::LBracket);
                i += 1;
            }
            ']' => {
                out.push(Tok::RBracket);
                i += 1;
            }
            '@' => {
                out.push(Tok::At);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '|' => {
                out.push(Tok::Pipe);
                i += 1;
            }
            ':' if next == Some(':') => {
                out.push(Tok::ColonColon);
                i += 2;
            }
            '.' if next == Some('.') => {
                out.push(Tok::DDot);
                i += 2;
            }
            '.' if !next.is_some_and(|n| n.is_ascii_digit()) => {
                out.push(Tok::Dot);
                i += 1;
            }
            '=' => {
                out.push(Tok::Op(Op::Eq));
                i += 1;
            }
            '!' if next == Some('=') => {
                out.push(Tok::Op(Op::Ne));
                i += 2;
            }
            '<' if next == Some('=') => {
                out.push(Tok::Op(Op::Le));
                i += 2;
            }
            '<' => {
                out.push(Tok::Op(Op::Lt));
                i += 1;
            }
            '>' if next == Some('=') => {
                out.push(Tok::Op(Op::Ge));
                i += 2;
            }
            '>' => {
                out.push(Tok::Op(Op::Gt));
                i += 1;
            }
            '+' => {
                out.push(Tok::Op(Op::Add));
                i += 1;
            }
            '-' => {
                out.push(Tok::Op(Op::Sub));
                i += 1;
            }
            '*' => {
                out.push(if operator_context(&out) { Tok::Op(Op::Mul) } else { Tok::Star });
                i += 1;
            }
            '"' | '\'' => {
                let end = cs.get(i + 1..)?.iter().position(|&x| x == c)? + i + 1;
                out.push(Tok::Str(cs.get(i + 1..end)?.iter().collect()));
                i = end + 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while cs.get(i).is_some_and(|x| x.is_ascii_digit() || *x == '.') {
                    i += 1;
                }
                let text: String = cs.get(start..i)?.iter().collect();
                out.push(Tok::Num(text.parse().ok()?));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while cs.get(i).is_some_and(|x| x.is_alphanumeric() || matches!(x, '_' | '-' | '.')) {
                    i += 1;
                }
                // A prefixed name (`ns:item`) or `prefix:*`, but not an axis (`child::`).
                if cs.get(i) == Some(&':') && cs.get(i + 1) != Some(&':') {
                    if cs.get(i + 1) == Some(&'*') {
                        i += 2;
                    } else {
                        i += 1;
                        while cs.get(i).is_some_and(|x| x.is_alphanumeric() || matches!(x, '_' | '-' | '.')) {
                            i += 1;
                        }
                    }
                }
                let name: String = cs.get(start..i)?.iter().collect();
                let op = match name.as_str() {
                    "and" => Some(Op::And),
                    "or" => Some(Op::Or),
                    "div" => Some(Op::Div),
                    "mod" => Some(Op::Mod),
                    _ => None,
                };
                match op {
                    Some(op) if operator_context(&out) => out.push(Tok::Op(op)),
                    _ => out.push(Tok::Name(name)),
                }
            }
            _ => return None,
        }
        if out.len() > 10_000 {
            return None;
        }
    }
    Some(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn peek_at(&self, k: usize) -> Option<&Tok> {
        self.toks.get(self.pos + k)
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn enter(&mut self) -> Option<()> {
        self.depth += 1;
        (self.depth <= MAX_EXPR_DEPTH).then_some(())
    }

    fn expr(&mut self) -> Option<Expr> {
        self.enter()?;
        let e = self.binary(0);
        self.depth -= 1;
        e
    }

    /// Precedence climbing over the binary operators, loosest first.
    fn binary(&mut self, level: usize) -> Option<Expr> {
        const LEVELS: &[&[Op]] =
            &[&[Op::Or], &[Op::And], &[Op::Eq, Op::Ne], &[Op::Lt, Op::Le, Op::Gt, Op::Ge], &[Op::Add, Op::Sub], &[Op::Mul, Op::Div, Op::Mod]];
        let Some(ops) = LEVELS.get(level) else { return self.unary() };
        let mut left = self.binary(level + 1)?;
        while let Some(Tok::Op(op)) = self.peek().cloned() {
            if !ops.contains(&op) {
                break;
            }
            self.pos += 1;
            let right = self.binary(level + 1)?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<Expr> {
        if self.eat(&Tok::Op(Op::Sub)) {
            self.enter()?;
            let e = self.unary();
            self.depth -= 1;
            return Some(Expr::Neg(Box::new(e?)));
        }
        let mut left = self.path()?;
        while self.eat(&Tok::Pipe) {
            let right = self.path()?;
            left = Expr::Bin(Op::Union, Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn path(&mut self) -> Option<Expr> {
        match self.peek()? {
            Tok::Slash => {
                self.pos += 1;
                let steps = if self.starts_step() { self.rel_path()? } else { vec![] };
                Some(Expr::Path { absolute: true, steps })
            }
            Tok::DSlash => {
                self.pos += 1;
                let mut steps = vec![Step { axis: Axis::DescendantOrSelf, test: Test::Node, preds: vec![] }];
                steps.extend(self.rel_path()?);
                Some(Expr::Path { absolute: true, steps })
            }
            Tok::LParen | Tok::Str(_) | Tok::Num(_) => self.filter(),
            Tok::Name(n)
                if self.peek_at(1) == Some(&Tok::LParen) && !matches!(n.as_str(), "text" | "node" | "comment" | "processing-instruction") =>
            {
                self.filter()
            }
            _ => Some(Expr::Path { absolute: false, steps: self.rel_path()? }),
        }
    }

    fn filter(&mut self) -> Option<Expr> {
        let base = match self.peek()?.clone() {
            Tok::LParen => {
                self.pos += 1;
                let e = self.expr()?;
                if !self.eat(&Tok::RParen) {
                    return None;
                }
                e
            }
            Tok::Str(s) => {
                self.pos += 1;
                Expr::Str(s)
            }
            Tok::Num(n) => {
                self.pos += 1;
                Expr::Num(n)
            }
            Tok::Name(n) => {
                self.pos += 2;
                let mut args = vec![];
                if !self.eat(&Tok::RParen) {
                    loop {
                        args.push(self.expr()?);
                        if self.eat(&Tok::RParen) {
                            break;
                        }
                        if !self.eat(&Tok::Comma) {
                            return None;
                        }
                    }
                }
                Expr::Call(n, args)
            }
            _ => return None,
        };
        let preds = self.predicates()?;
        let steps = match self.peek() {
            Some(Tok::Slash) => {
                self.pos += 1;
                self.rel_path()?
            }
            Some(Tok::DSlash) => {
                self.pos += 1;
                let mut s = vec![Step { axis: Axis::DescendantOrSelf, test: Test::Node, preds: vec![] }];
                s.extend(self.rel_path()?);
                s
            }
            _ => vec![],
        };
        if preds.is_empty() && steps.is_empty() {
            return Some(base);
        }
        Some(Expr::Filter { base: Box::new(base), preds, steps })
    }

    fn starts_step(&self) -> bool {
        matches!(self.peek(), Some(Tok::Name(_) | Tok::Star | Tok::At | Tok::Dot | Tok::DDot))
    }

    fn rel_path(&mut self) -> Option<Vec<Step>> {
        let mut steps = vec![self.step()?];
        loop {
            match self.peek() {
                Some(Tok::Slash) => {
                    self.pos += 1;
                    steps.push(self.step()?);
                }
                Some(Tok::DSlash) => {
                    self.pos += 1;
                    steps.push(Step { axis: Axis::DescendantOrSelf, test: Test::Node, preds: vec![] });
                    steps.push(self.step()?);
                }
                _ => break,
            }
            if steps.len() > 256 {
                return None;
            }
        }
        Some(steps)
    }

    fn step(&mut self) -> Option<Step> {
        if self.eat(&Tok::Dot) {
            return Some(Step { axis: Axis::Itself, test: Test::Node, preds: vec![] });
        }
        if self.eat(&Tok::DDot) {
            return Some(Step { axis: Axis::Parent, test: Test::Node, preds: vec![] });
        }
        let mut axis = Axis::Child;
        if self.eat(&Tok::At) {
            axis = Axis::Attribute;
        } else if let (Some(Tok::Name(n)), Some(Tok::ColonColon)) = (self.peek().cloned(), self.peek_at(1)) {
            axis = match n.as_str() {
                "child" => Axis::Child,
                "descendant" => Axis::Descendant,
                "descendant-or-self" => Axis::DescendantOrSelf,
                "self" => Axis::Itself,
                "parent" => Axis::Parent,
                "ancestor" => Axis::Ancestor,
                "ancestor-or-self" => Axis::AncestorOrSelf,
                "attribute" => Axis::Attribute,
                "following-sibling" => Axis::FollowingSibling,
                "preceding-sibling" => Axis::PrecedingSibling,
                "following" => Axis::Following,
                "preceding" => Axis::Preceding,
                _ => return None,
            };
            self.pos += 2;
        }
        let test = match self.peek()?.clone() {
            Tok::Star => {
                self.pos += 1;
                Test::Any(None)
            }
            Tok::Name(n) if self.peek_at(1) == Some(&Tok::LParen) => {
                self.pos += 2;
                if !self.eat(&Tok::RParen) {
                    return None;
                }
                match n.as_str() {
                    "text" => Test::Text,
                    "node" => Test::Node,
                    _ => return None,
                }
            }
            Tok::Name(n) => {
                self.pos += 1;
                match n.strip_suffix(":*") {
                    Some(prefix) => Test::Any(Some(prefix.to_string())),
                    None => Test::Name(n),
                }
            }
            _ => return None,
        };
        let preds = self.predicates()?;
        Some(Step { axis, test, preds })
    }

    fn predicates(&mut self) -> Option<Vec<Expr>> {
        let mut preds = vec![];
        while self.eat(&Tok::LBracket) {
            preds.push(self.expr()?);
            if !self.eat(&Tok::RBracket) {
                return None;
            }
        }
        Some(preds)
    }
}

/// An XPath value.
#[derive(Clone, Debug, PartialEq)]
pub enum XValue {
    Nodes(Vec<NodeRef>),
    Num(f64),
    Str(String),
    Bool(bool),
}

struct Eval<'d> {
    doc: &'d Doc,
    work: usize,
}

#[derive(Clone, Copy)]
struct Ctx {
    node: NodeRef,
    pos: usize,
    size: usize,
}

impl Eval<'_> {
    fn tick(&mut self, n: usize) -> Option<()> {
        self.work = self.work.saturating_add(n);
        (self.work <= MAX_WORK).then_some(())
    }

    fn string(&self, v: &XValue) -> String {
        match v {
            XValue::Nodes(ns) => ns.first().map(|n| self.doc.string_value(*n)).unwrap_or_default(),
            XValue::Num(n) => num_to_string(*n),
            XValue::Str(s) => s.clone(),
            XValue::Bool(b) => b.to_string(),
        }
    }
    fn number(&self, v: &XValue) -> f64 {
        match v {
            XValue::Num(n) => *n,
            XValue::Bool(b) => f64::from(u8::from(*b)),
            other => str_to_num(&self.string(other)),
        }
    }
    fn boolean(&self, v: &XValue) -> bool {
        match v {
            XValue::Nodes(ns) => !ns.is_empty(),
            XValue::Num(n) => *n != 0.0 && !n.is_nan(),
            XValue::Str(s) => !s.is_empty(),
            XValue::Bool(b) => *b,
        }
    }

    fn eval(&mut self, e: &Expr, cx: Ctx) -> Option<XValue> {
        self.tick(1)?;
        Some(match e {
            Expr::Num(n) => XValue::Num(*n),
            Expr::Str(s) => XValue::Str(s.clone()),
            Expr::Neg(x) => {
                let v = self.eval(x, cx)?;
                XValue::Num(-self.number(&v))
            }
            Expr::Path { absolute, steps } => {
                let start = if *absolute { NodeRef::Node(0) } else { cx.node };
                XValue::Nodes(self.steps(vec![start], steps)?)
            }
            Expr::Filter { base, preds, steps } => {
                let XValue::Nodes(mut ns) = self.eval(base, cx)? else { return None };
                for p in preds {
                    ns = self.filter(ns, p)?;
                }
                XValue::Nodes(self.steps(ns, steps)?)
            }
            Expr::Call(name, args) => self.call(name, args, cx)?,
            Expr::Bin(op, a, b) => {
                let (x, y) = (self.eval(a, cx)?, self.eval(b, cx)?);
                match op {
                    Op::Or => XValue::Bool(self.boolean(&x) || self.boolean(&y)),
                    Op::And => XValue::Bool(self.boolean(&x) && self.boolean(&y)),
                    Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => XValue::Bool(self.compare(*op, &x, &y)),
                    Op::Add => XValue::Num(self.number(&x) + self.number(&y)),
                    Op::Sub => XValue::Num(self.number(&x) - self.number(&y)),
                    Op::Mul => XValue::Num(self.number(&x) * self.number(&y)),
                    Op::Div => XValue::Num(self.number(&x) / self.number(&y)),
                    Op::Mod => XValue::Num(self.number(&x) % self.number(&y)),
                    Op::Union => match (x, y) {
                        (XValue::Nodes(mut a), XValue::Nodes(b)) => {
                            a.extend(b);
                            a.sort_by_key(|n| n.order());
                            a.dedup();
                            XValue::Nodes(a)
                        }
                        _ => return None,
                    },
                }
            }
        })
    }

    /// XPath 3.4 comparisons, node-sets compared value by value.
    fn compare(&self, op: Op, x: &XValue, y: &XValue) -> bool {
        let strings = |v: &XValue| -> Vec<String> {
            match v {
                XValue::Nodes(ns) => ns.iter().map(|n| self.doc.string_value(*n)).collect(),
                other => vec![self.string(other)],
            }
        };
        let atom = |a: &str, b: &str, numeric: bool| -> bool {
            match op {
                Op::Eq | Op::Ne if !numeric => (a == b) == (op == Op::Eq),
                _ => {
                    let (p, q) = (str_to_num(a), str_to_num(b));
                    match op {
                        Op::Eq => p == q,
                        Op::Ne => p != q,
                        Op::Lt => p < q,
                        Op::Le => p <= q,
                        Op::Gt => p > q,
                        _ => p >= q,
                    }
                }
            }
        };
        match (x, y) {
            (XValue::Bool(_), _) | (_, XValue::Bool(_)) if matches!(op, Op::Eq | Op::Ne) => (self.boolean(x) == self.boolean(y)) == (op == Op::Eq),
            (XValue::Num(_), _) | (_, XValue::Num(_)) => {
                let numeric = |v: &XValue| -> Vec<f64> {
                    match v {
                        XValue::Nodes(ns) => ns.iter().map(|n| str_to_num(&self.doc.string_value(*n))).collect(),
                        other => vec![self.number(other)],
                    }
                };
                let (a, b) = (numeric(x), numeric(y));
                a.iter().any(|p| {
                    b.iter().any(|q| match op {
                        Op::Eq => p == q,
                        Op::Ne => p != q,
                        Op::Lt => p < q,
                        Op::Le => p <= q,
                        Op::Gt => p > q,
                        _ => p >= q,
                    })
                })
            }
            _ => {
                let numeric = !matches!(op, Op::Eq | Op::Ne);
                let (a, b) = (strings(x), strings(y));
                a.iter().any(|p| b.iter().any(|q| atom(p, q, numeric)))
            }
        }
    }

    fn steps(&mut self, mut ns: Vec<NodeRef>, steps: &[Step]) -> Option<Vec<NodeRef>> {
        for s in steps {
            let mut out: Vec<NodeRef> = Vec::new();
            for n in &ns {
                let mut found = self.axis(*n, s)?;
                for p in &s.preds {
                    found = self.filter(found, p)?;
                }
                out.extend(found);
                if out.len() > 2_000_000 {
                    return None;
                }
            }
            out.sort_by_key(|n| n.order());
            out.dedup();
            ns = out;
        }
        Some(ns)
    }

    /// The nodes of an axis that pass the step's node test, in axis order (reverse axes nearest
    /// first, so positions in predicates count outward).
    fn axis(&mut self, n: NodeRef, s: &Step) -> Option<Vec<NodeRef>> {
        let doc = self.doc;
        let mut out = Vec::new();
        let elem_of = |r: NodeRef| match r {
            NodeRef::Node(i) => Some(i),
            NodeRef::Attr(..) => None,
        };
        let parent_of = |r: NodeRef| match r {
            NodeRef::Node(i) => doc.node(i).and_then(|x| x.parent),
            NodeRef::Attr(e, _) => Some(e),
        };
        match s.axis {
            Axis::Attribute => {
                if let Some(e) = elem_of(n).and_then(|i| doc.node(i).map(|x| (i, x))) {
                    for (ai, (name, _)) in e.1.attrs.iter().enumerate() {
                        if self.matches_name(&s.test, name, true) {
                            out.push(NodeRef::Attr(e.0, ai));
                        }
                    }
                }
                return Some(out);
            }
            Axis::Child => {
                for &c in elem_of(n).and_then(|i| doc.node(i)).map(|x| x.children.as_slice()).unwrap_or(&[]) {
                    self.push_if(&mut out, c, &s.test);
                }
            }
            Axis::Descendant | Axis::DescendantOrSelf => {
                if s.axis == Axis::DescendantOrSelf {
                    match n {
                        NodeRef::Node(i) => self.push_if(&mut out, i, &s.test),
                        NodeRef::Attr(..) if s.test == Test::Node => out.push(n),
                        NodeRef::Attr(..) => {}
                    }
                }
                let mut stack: Vec<usize> =
                    elem_of(n).and_then(|i| doc.node(i)).map(|x| x.children.iter().rev().copied().collect()).unwrap_or_default();
                while let Some(c) = stack.pop() {
                    self.tick(1)?;
                    self.push_if(&mut out, c, &s.test);
                    if let Some(x) = doc.node(c) {
                        stack.extend(x.children.iter().rev());
                    }
                }
            }
            Axis::Itself => match n {
                NodeRef::Node(i) => self.push_if(&mut out, i, &s.test),
                NodeRef::Attr(e, a) => {
                    let name = doc.node(e).and_then(|x| x.attrs.get(a)).map(|(k, _)| k.as_str()).unwrap_or("");
                    if s.test == Test::Node || self.matches_name(&s.test, name, true) {
                        out.push(n);
                    }
                }
            },
            Axis::Parent => {
                if let Some(p) = parent_of(n) {
                    self.push_if(&mut out, p, &s.test);
                }
            }
            Axis::Ancestor | Axis::AncestorOrSelf => {
                if s.axis == Axis::AncestorOrSelf
                    && let NodeRef::Node(i) = n
                {
                    self.push_if(&mut out, i, &s.test);
                }
                let mut cur = parent_of(n);
                while let Some(p) = cur {
                    self.tick(1)?;
                    self.push_if(&mut out, p, &s.test);
                    cur = doc.node(p).and_then(|x| x.parent);
                }
            }
            Axis::FollowingSibling | Axis::PrecedingSibling => {
                let Some(i) = elem_of(n) else { return Some(out) };
                let sibs = doc.node(i).and_then(|x| x.parent).and_then(|p| doc.node(p)).map(|p| p.children.as_slice()).unwrap_or(&[]);
                let at = sibs.iter().position(|&c| c == i).unwrap_or(0);
                if s.axis == Axis::FollowingSibling {
                    for &c in sibs.iter().skip(at + 1) {
                        self.push_if(&mut out, c, &s.test);
                    }
                } else {
                    for &c in sibs.iter().take(at).rev() {
                        self.push_if(&mut out, c, &s.test);
                    }
                }
            }
            Axis::Following | Axis::Preceding => {
                let Some(i) = elem_of(n).or_else(|| parent_of(n)) else { return Some(out) };
                let mut ancestors = vec![];
                let mut cur = Some(i);
                while let Some(p) = cur {
                    ancestors.push(p);
                    cur = doc.node(p).and_then(|x| x.parent);
                }
                let last_desc = {
                    let mut d = i;
                    while let Some(&c) = doc.node(d).and_then(|x| x.children.last()) {
                        d = c;
                    }
                    d
                };
                if s.axis == Axis::Following {
                    for c in last_desc + 1..doc.nodes.len() {
                        self.tick(1)?;
                        self.push_if(&mut out, c, &s.test);
                    }
                } else {
                    for c in (1..i).rev() {
                        self.tick(1)?;
                        if !ancestors.contains(&c) {
                            self.push_if(&mut out, c, &s.test);
                        }
                    }
                }
            }
        }
        Some(out)
    }

    fn push_if(&self, out: &mut Vec<NodeRef>, i: usize, t: &Test) {
        let Some(n) = self.doc.node(i) else { return };
        let ok = match (t, n.kind) {
            (Test::Node, _) => true,
            (Test::Text, Kind::Text) => true,
            (Test::Text, _) => false,
            (_, Kind::Element) => self.matches_name(t, &n.name, false),
            _ => false,
        };
        if ok {
            out.push(NodeRef::Node(i));
        }
    }

    fn matches_name(&self, t: &Test, name: &str, _attr: bool) -> bool {
        match t {
            Test::Any(None) | Test::Node => true,
            Test::Any(Some(prefix)) => name.split_once(':').is_some_and(|(p, _)| p == prefix),
            Test::Name(n) => n == name,
            Test::Text => false,
        }
    }

    fn filter(&mut self, ns: Vec<NodeRef>, p: &Expr) -> Option<Vec<NodeRef>> {
        let size = ns.len();
        let mut out = Vec::new();
        for (i, n) in ns.into_iter().enumerate() {
            let v = self.eval(p, Ctx { node: n, pos: i + 1, size })?;
            let keep = match v {
                XValue::Num(k) => k == (i + 1) as f64,
                other => self.boolean(&other),
            };
            if keep {
                out.push(n);
            }
        }
        Some(out)
    }

    fn call(&mut self, name: &str, args: &[Expr], cx: Ctx) -> Option<XValue> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.eval(a, cx)?);
        }
        let s = |i: usize, me: &Self| vals.get(i).map(|v| me.string(v)).unwrap_or_else(|| me.doc.string_value(cx.node));
        let n = |i: usize, me: &Self| vals.get(i).map(|v| me.number(v)).unwrap_or(f64::NAN);
        let argc = |lo: usize, hi: usize| (lo..=hi).contains(&vals.len()).then_some(());
        Some(match name {
            "last" => {
                argc(0, 0)?;
                XValue::Num(cx.size as f64)
            }
            "position" => {
                argc(0, 0)?;
                XValue::Num(cx.pos as f64)
            }
            "count" => match vals.first() {
                Some(XValue::Nodes(ns)) if vals.len() == 1 => XValue::Num(ns.len() as f64),
                _ => return None,
            },
            "name" | "local-name" => {
                argc(0, 1)?;
                let node = match vals.first() {
                    Some(XValue::Nodes(ns)) => ns.first().copied(),
                    Some(_) => return None,
                    None => Some(cx.node),
                };
                let full = node.map(|r| self.doc.name(r)).unwrap_or_default();
                XValue::Str(if name == "local-name" { full.rsplit(':').next().unwrap_or("").to_string() } else { full })
            }
            "string" => {
                argc(0, 1)?;
                XValue::Str(s(0, self))
            }
            "concat" => {
                if vals.len() < 2 {
                    return None;
                }
                XValue::Str(vals.iter().map(|v| self.string(v)).collect())
            }
            "starts-with" => {
                argc(2, 2)?;
                XValue::Bool(s(0, self).starts_with(&s(1, self)))
            }
            "ends-with" => {
                argc(2, 2)?;
                XValue::Bool(s(0, self).ends_with(&s(1, self)))
            }
            "contains" => {
                argc(2, 2)?;
                XValue::Bool(s(0, self).contains(&s(1, self)))
            }
            "substring-before" => {
                argc(2, 2)?;
                let (a, b) = (s(0, self), s(1, self));
                XValue::Str(a.split_once(&b).map(|(x, _)| x.to_string()).unwrap_or_default())
            }
            "substring-after" => {
                argc(2, 2)?;
                let (a, b) = (s(0, self), s(1, self));
                XValue::Str(a.split_once(&b).map(|(_, x)| x.to_string()).unwrap_or_default())
            }
            "substring" => {
                argc(2, 3)?;
                let text: Vec<char> = s(0, self).chars().collect();
                let start = round(n(1, self));
                let end = if vals.len() == 3 { start + round(n(2, self)) } else { f64::INFINITY };
                XValue::Str(text.iter().enumerate().filter(|(i, _)| ((*i + 1) as f64) >= start && ((*i + 1) as f64) < end).map(|(_, c)| *c).collect())
            }
            "string-length" => {
                argc(0, 1)?;
                XValue::Num(s(0, self).chars().count() as f64)
            }
            "normalize-space" => {
                argc(0, 1)?;
                XValue::Str(s(0, self).split_whitespace().collect::<Vec<_>>().join(" "))
            }
            "translate" => {
                argc(3, 3)?;
                let (from, to): (Vec<char>, Vec<char>) = (s(1, self).chars().collect(), s(2, self).chars().collect());
                XValue::Str(
                    s(0, self)
                        .chars()
                        .filter_map(|c| match from.iter().position(|f| *f == c) {
                            Some(i) => to.get(i).copied(),
                            None => Some(c),
                        })
                        .collect(),
                )
            }
            "not" => {
                argc(1, 1)?;
                XValue::Bool(!self.boolean(vals.first()?))
            }
            "true" => XValue::Bool(true),
            "false" => XValue::Bool(false),
            "boolean" => {
                argc(1, 1)?;
                XValue::Bool(self.boolean(vals.first()?))
            }
            "number" => {
                argc(0, 1)?;
                XValue::Num(match vals.first() {
                    Some(v) => self.number(v),
                    None => str_to_num(&self.doc.string_value(cx.node)),
                })
            }
            "sum" => match vals.first() {
                Some(XValue::Nodes(ns)) if vals.len() == 1 => XValue::Num(ns.iter().map(|r| str_to_num(&self.doc.string_value(*r))).sum()),
                _ => return None,
            },
            "floor" => XValue::Num(n(0, self).floor()),
            "ceiling" => XValue::Num(n(0, self).ceil()),
            "round" => XValue::Num(round(n(0, self))),
            _ => return None,
        })
    }
}

fn round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// XPath's number(): optional whitespace, an optional minus, digits with an optional point.
fn str_to_num(s: &str) -> f64 {
    let t = s.trim();
    let body = t.strip_prefix('-').unwrap_or(t);
    let valid = !body.is_empty() && body.chars().all(|c| c.is_ascii_digit() || c == '.') && body.matches('.').count() <= 1 && body != ".";
    if valid { t.parse().unwrap_or(f64::NAN) } else { f64::NAN }
}

fn num_to_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Parses an XPath expression; `None` when it is invalid or too deep.
fn parse_xpath(s: &str) -> Option<Expr> {
    let toks = tokenize(s)?;
    if toks.is_empty() {
        return None;
    }
    let mut p = Parser { toks, pos: 0, depth: 0 };
    let e = p.expr()?;
    (p.pos == p.toks.len()).then_some(e)
}

/// Evaluates `xpath` against `doc` from the root.
pub fn evaluate(doc: &Doc, xpath: &str) -> Option<XValue> {
    let e = parse_xpath(xpath)?;
    let mut ev = Eval { doc, work: 0 };
    ev.eval(&e, Ctx { node: NodeRef::Node(0), pos: 1, size: 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(xml: &str, xpath: &str) -> Option<Vec<String>> {
        let doc = Doc::parse(xml)?;
        match evaluate(&doc, xpath)? {
            XValue::Nodes(ns) => Some(ns.iter().map(|n| doc.string_value(*n)).collect()),
            other => Some(vec![format!("{other:?}")]),
        }
    }

    const BOOKS: &str = r#"<?xml version="1.0"?>
<catalog>
  <book id="b1" lang="en"><title>Rust</title><price>35</price></book>
  <book id="b2" lang="de"><title>XML &amp; XPath</title><price>20</price></book>
  <book id="b3"><title><![CDATA[<Tags>]]></title><price>50</price></book>
</catalog>"#;

    #[test]
    fn location_paths_and_predicates() {
        assert_eq!(strings(BOOKS, "//title").unwrap(), vec!["Rust", "XML & XPath", "<Tags>"]);
        assert_eq!(strings(BOOKS, "/catalog/book/@id").unwrap(), vec!["b1", "b2", "b3"]);
        assert_eq!(strings(BOOKS, "//book[2]/title").unwrap(), vec!["XML & XPath"]);
        assert_eq!(strings(BOOKS, "//book[last()]/@id").unwrap(), vec!["b3"]);
        assert_eq!(strings(BOOKS, "//book[@lang='de']/price").unwrap(), vec!["20"]);
        assert_eq!(strings(BOOKS, "//book[price>30]/title").unwrap(), vec!["Rust", "<Tags>"]);
        assert_eq!(strings(BOOKS, "//book[not(@lang)]/@id").unwrap(), vec!["b3"]);
        assert_eq!(strings(BOOKS, "//book[@lang]").unwrap().len(), 2);
        assert_eq!(strings(BOOKS, "//title/text()").unwrap(), vec!["Rust", "XML & XPath", "<Tags>"]);
        assert_eq!(strings(BOOKS, "/catalog/*[1]/*[2]").unwrap(), vec!["35"]);
        assert_eq!(strings(BOOKS, "//price/..//title").unwrap().len(), 3);
        assert_eq!(strings(BOOKS, "//book[title='Rust']/following-sibling::book/@id").unwrap(), vec!["b2", "b3"]);
        assert_eq!(strings(BOOKS, "//book[3]/preceding-sibling::book[1]/@id").unwrap(), vec!["b2"]);
        assert_eq!(strings(BOOKS, "//title | //price").unwrap().len(), 6);
        assert_eq!(strings(BOOKS, "//book[contains(title,'XML')]/@id").unwrap(), vec!["b2"]);
        assert_eq!(strings(BOOKS, "//book[position()<=2 and price > 25]/@id").unwrap(), vec!["b1"]);
        assert_eq!(strings(BOOKS, "(//price)[2]").unwrap(), vec!["20"]);
        assert_eq!(strings(BOOKS, "//@*").unwrap().len(), 5);
        assert_eq!(strings(BOOKS, "//nothing").unwrap(), Vec::<String>::new());
        assert_eq!(strings(BOOKS, "count(//book)").unwrap(), vec!["Num(3.0)"]);
        assert_eq!(strings(BOOKS, "sum(//price) div 3").unwrap(), vec![format!("{:?}", XValue::Num(35.0))]);
    }

    #[test]
    fn invalid_input_is_refused() {
        assert!(Doc::parse("<a><b></a>").is_none());
        assert!(Doc::parse("<a/><b/>").is_none());
        assert!(Doc::parse("text").is_none());
        assert!(Doc::parse("").is_none());
        let doc = Doc::parse("<a><b>1</b></a>").unwrap();
        for bad in ["", "//", "//b[", "b)", "@", "//b[1", "foo(", "//b/bad::x", "'unterminated", "#"] {
            assert!(evaluate(&doc, bad).is_none(), "{bad}");
        }
        let deep = format!("{}1{}", "(".repeat(500), ")".repeat(500));
        assert!(evaluate(&doc, &deep).is_none());
        let nested = "<a>".repeat(1000) + &"</a>".repeat(1000);
        assert!(Doc::parse(&nested).is_none());
    }
}
