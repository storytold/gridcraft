//! A small, strict XML reader for FILTERXML.
//!
//! It builds a tree of elements, attributes and text, and refuses anything it can't read
//! exactly: no DTDs (so no entity expansion), more than one root, mismatched tags, undefined
//! entities. Input comes from cells, so depth and node counts are capped. Whitespace-only text
//! between elements is dropped, as an XML DOM does when it isn't asked to preserve whitespace.

/// Nodes (elements and text) one document may hold.
const MAX_NODES: usize = 100_000;
/// Elements one document may nest.
const MAX_DEPTH: usize = 256;

#[derive(Debug)]
pub(crate) enum Kind {
    Root,
    Element {
        /// The qualified name as written (`ns:item`).
        name: String,
        /// Unprefixed, inside a non-empty default namespace: an XPath name test without a prefix
        /// does not select it, as in XPath 1.0.
        in_default_ns: bool,
    },
    Text(String),
}

#[derive(Debug)]
pub(crate) struct Node {
    pub kind: Kind,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub attrs: Vec<(String, String)>,
}

/// A parsed document. Nodes are stored in document order; node 0 is the root.
#[derive(Debug)]
pub(crate) struct Dom {
    pub nodes: Vec<Node>,
}

impl Dom {
    pub(crate) fn node(&self, i: usize) -> Option<&Node> {
        self.nodes.get(i)
    }

    /// Appends the text under `i` (an element's descendants, a text node itself) to `out`.
    pub(crate) fn push_text(&self, i: usize, out: &mut String) {
        let mut stack = vec![i];
        while let Some(n) = stack.pop() {
            let Some(node) = self.node(n) else { continue };
            match &node.kind {
                Kind::Text(t) => out.push_str(t),
                _ => stack.extend(node.children.iter().rev().copied()),
            }
        }
    }
}

/// Parses `src` as one well-formed XML document; `None` when it isn't or is too big.
pub(crate) fn parse(src: &str) -> Option<Dom> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut p = Parser {
        src,
        pos: 0,
        dom: Dom { nodes: vec![Node { kind: Kind::Root, parent: None, children: vec![], attrs: vec![] }] },
        open: vec![0],
        default_ns: vec![false],
        roots: 0,
    };
    p.document()?;
    Some(p.dom)
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    dom: Dom,
    /// Open elements, innermost last; the root node is always at the bottom.
    open: Vec<usize>,
    /// Whether a non-empty default namespace is in scope, per open element.
    default_ns: Vec<bool>,
    roots: usize,
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':'
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-' || c == '.' || c == '\u{b7}' || c.is_alphanumeric()
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.rest().starts_with(lit) {
            self.pos += lit.len();
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) {
        let rest = self.rest();
        self.pos += rest.len() - rest.trim_start_matches([' ', '\t', '\r', '\n']).len();
    }

    /// Consumes up to and including `end`, returning what came before it.
    fn until(&mut self, end: &str) -> Option<&'a str> {
        let rest = self.src.get(self.pos..)?;
        let at = rest.find(end)?;
        self.pos += at + end.len();
        rest.get(..at)
    }

    fn name(&mut self) -> Option<String> {
        let rest = self.rest();
        let mut chars = rest.char_indices();
        let (_, first) = chars.next()?;
        if !is_name_start(first) {
            return None;
        }
        let end = chars.find(|&(_, c)| !is_name_char(c)).map_or(rest.len(), |(i, _)| i);
        self.pos += end;
        rest.get(..end).map(str::to_string)
    }

    fn document(&mut self) -> Option<()> {
        // An XML declaration is only allowed first.
        if self.rest().starts_with("<?xml") && self.rest().get(5..).is_some_and(|r| r.starts_with([' ', '\t', '\r', '\n', '?'])) {
            self.until("?>")?;
        }
        loop {
            if self.pos >= self.src.len() {
                break;
            }
            if self.rest().starts_with('<') {
                self.markup()?;
            } else {
                self.text()?;
            }
        }
        (self.open.len() == 1 && self.roots == 1).then_some(())
    }

    fn markup(&mut self) -> Option<()> {
        if self.eat("<!--") {
            self.until("-->")?;
        } else if self.eat("<![CDATA[") {
            let data = self.until("]]>")?.to_string();
            self.add_text(&data, true)?;
        } else if self.eat("<?") {
            self.until("?>")?;
        } else if self.rest().starts_with("<!") {
            // DOCTYPE (and with it, entity declarations) is not accepted.
            return None;
        } else if self.eat("</") {
            let name = self.name()?;
            self.skip_space();
            if !self.eat(">") {
                return None;
            }
            let top = self.open.pop().filter(|_| self.open.last().is_some())?;
            self.default_ns.pop();
            match &self.dom.node(top)?.kind {
                Kind::Element { name: open, .. } if *open == name => {}
                _ => return None,
            }
        } else {
            self.eat("<");
            self.start_tag()?;
        }
        Some(())
    }

    fn start_tag(&mut self) -> Option<()> {
        let name = self.name()?;
        let mut attrs: Vec<(String, String)> = vec![];
        let self_closing = loop {
            let before = self.pos;
            self.skip_space();
            if self.eat("/>") {
                break true;
            }
            if self.eat(">") {
                break false;
            }
            // Attributes must be separated from the name and from each other.
            if self.pos == before {
                return None;
            }
            let key = self.name()?;
            self.skip_space();
            if !self.eat("=") {
                return None;
            }
            self.skip_space();
            let quote = if self.eat("\"") {
                "\""
            } else if self.eat("'") {
                "'"
            } else {
                return None;
            };
            let raw = self.until(quote)?.to_string();
            if raw.contains('<') || attrs.iter().any(|(k, _)| *k == key) {
                return None;
            }
            attrs.push((key, decode(&raw, true)?));
        };

        let parent_ns = self.default_ns.last().copied().unwrap_or(false);
        let ns = attrs.iter().rev().find(|(k, _)| k == "xmlns").map_or(parent_ns, |(_, v)| !v.is_empty());
        let parent = *self.open.last()?;
        if parent == 0 {
            self.roots += 1;
            if self.roots > 1 {
                return None;
            }
        }
        if self.open.len() > MAX_DEPTH || self.dom.nodes.len() >= MAX_NODES {
            return None;
        }
        let id = self.dom.nodes.len();
        let in_default_ns = ns && !name.contains(':');
        self.dom.nodes.push(Node { kind: Kind::Element { name, in_default_ns }, parent: Some(parent), children: vec![], attrs });
        self.dom.nodes.get_mut(parent)?.children.push(id);
        if !self_closing {
            self.open.push(id);
            self.default_ns.push(ns);
        }
        Some(())
    }

    fn text(&mut self) -> Option<()> {
        let rest = self.rest();
        let end = rest.find('<').unwrap_or(rest.len());
        let raw = rest.get(..end)?;
        self.pos += end;
        let decoded = decode(raw, false)?;
        self.add_text(&decoded, false)
    }

    fn add_text(&mut self, text: &str, cdata: bool) -> Option<()> {
        let parent = *self.open.last()?;
        if parent == 0 {
            // Outside the root element only whitespace is allowed.
            return text.trim_matches([' ', '\t', '\r', '\n']).is_empty().then_some(());
        }
        if !cdata && text.trim_matches([' ', '\t', '\r', '\n']).is_empty() {
            return Some(());
        }
        if text.is_empty() {
            return Some(());
        }
        // Text and CDATA next to each other are one text node.
        if let Some(&last) = self.dom.node(parent)?.children.last()
            && let Some(Node { kind: Kind::Text(t), .. }) = self.dom.nodes.get_mut(last)
        {
            t.push_str(text);
            return Some(());
        }
        if self.dom.nodes.len() >= MAX_NODES {
            return None;
        }
        let id = self.dom.nodes.len();
        self.dom.nodes.push(Node { kind: Kind::Text(text.to_string()), parent: Some(parent), children: vec![], attrs: vec![] });
        self.dom.nodes.get_mut(parent)?.children.push(id);
        Some(())
    }
}

/// Replaces the five predefined entities and numeric character references; `None` for any other
/// reference or a stray `&`. In attribute values, tabs and line breaks become spaces.
fn decode(raw: &str, attribute: bool) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at)?);
        let after = rest.get(at + 1..)?;
        let semi = after.find(';')?;
        let entity = after.get(..semi)?;
        match entity {
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "amp" => out.push('&'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let digits = entity.strip_prefix('#')?;
                let code = match digits.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => digits.parse::<u32>().ok()?,
                };
                let c = char::from_u32(code).filter(|&c| c == '\t' || c == '\n' || c == '\r' || c >= ' ')?;
                out.push(c);
            }
        }
        rest = after.get(semi + 1..)?;
    }
    out.push_str(rest);
    if attribute {
        out = out.replace(['\t', '\n', '\r'], " ");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(dom: &Dom) -> String {
        let mut s = String::new();
        dom.push_text(0, &mut s);
        s
    }

    #[test]
    fn reads_elements_attributes_and_text() {
        let dom = parse("<?xml version='1.0'?><!-- c --><r a='1' b=\"x &amp; y\"><p>one</p><p>two</p></r>").unwrap();
        assert_eq!(text_of(&dom), "onetwo");
        let root = dom.node(1).unwrap();
        assert_eq!(root.attrs, vec![("a".to_string(), "1".to_string()), ("b".to_string(), "x & y".to_string())]);
        assert_eq!(root.children.len(), 2);
    }

    #[test]
    fn decodes_entities_and_cdata() {
        assert_eq!(text_of(&parse("<a>&lt;&#65;&#x42;&gt;</a>").unwrap()), "<AB>");
        assert_eq!(text_of(&parse("<a>x<![CDATA[<&>]]>y</a>").unwrap()), "x<&>y");
    }

    #[test]
    fn drops_whitespace_between_elements() {
        let dom = parse("<a>\n  <b>x</b>\n  <c/>\n</a>").unwrap();
        assert_eq!(dom.node(1).unwrap().children.len(), 2);
        assert_eq!(text_of(&dom), "x");
    }

    #[test]
    fn rejects_what_is_not_well_formed() {
        for bad in [
            "",
            "text",
            "<a>",
            "<a></b>",
            "<a><b></a></b>",
            "<a/><b/>",
            "<a/>tail",
            "<a>&nope;</a>",
            "<a>&amp</a>",
            "<a b=1/>",
            "<a b='1' b='2'/>",
            "<a b='<'/>",
            "<a b='1'c='2'/>",
            "</a>",
            "<1a/>",
            "<!DOCTYPE a [<!ENTITY e 'x'>]><a>&e;</a>",
            "<a>&#0;</a>",
            "<a>&#xD800;</a>",
        ] {
            assert!(parse(bad).is_none(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn marks_elements_in_a_default_namespace() {
        let dom = parse("<a xmlns='urn:x'><b/><c xmlns=''/><p:d xmlns:p='urn:p'/></a>").unwrap();
        let flags: Vec<bool> = (1..=4)
            .map(|i| match &dom.node(i).unwrap().kind {
                Kind::Element { in_default_ns, .. } => *in_default_ns,
                _ => false,
            })
            .collect();
        assert_eq!(flags, vec![true, true, false, false]);
    }

    #[test]
    fn caps_depth_and_size() {
        let deep = format!("{}{}", "<a>".repeat(300), "</a>".repeat(300));
        assert!(parse(&deep).is_none());
        let wide = format!("<r>{}</r>", "<a/>".repeat(MAX_NODES));
        assert!(parse(&wide).is_none());
        assert!(parse(&format!("<r>{}</r>", "<a/>".repeat(1000))).is_some());
    }

    #[test]
    fn handles_multibyte_text_without_slicing_inside_a_char() {
        assert_eq!(text_of(&parse("<é>日本語 &amp; ✓</é>").unwrap()), "日本語 & ✓");
    }
}
