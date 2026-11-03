//! LaTeX to OMML, the open Office Math Markup of ECMA-376 Part 1 §22.1, for the common subset:
//! runs, fractions and binomials, roots, scripts, big operators with limits, functions and
//! limits, delimiters, matrices, accents, bars, braces, font styles and text colour (`\color`,
//! `\textcolor`). Anything else is an error, and the caller saves the equation as text only.

use ratex_parser::ParseNode;
use ratex_parser::parse_node::Mode;

use crate::MathError;

/// Deepest tree walked (RaTeX already refuses input nested deeper than 32).
const MAX_WALK: usize = 96;

/// `latex` as an `<m:oMath>` element (prefix `m` for the OMML namespace and `a` for DrawingML,
/// declared by the caller), with runs at `size_pt` points in the `ink` colour (none: the
/// paragraph's own).
pub fn to_omml(latex: &str, size_pt: f32, ink: Option<[u8; 3]>) -> Result<String, MathError> {
    write(latex, size_pt, ink, true)
}

fn write(latex: &str, size_pt: f32, ink: Option<[u8; 3]>, merge: bool) -> Result<String, MathError> {
    crate::check_caps(latex)?;
    crate::guarded(|| {
        let nodes = crate::parse(latex)?;
        let sz = if size_pt.is_finite() { (size_pt * 100.0).round().clamp(100.0, 400_000.0) as u32 } else { 1100 };
        let fill = ink.map(fill_of).unwrap_or_default();
        let mut w = Writer { merge, last: None, out: String::from("<m:oMath>"), sz, fill };
        w.seq(&nodes, &Style::default(), 0)?;
        w.out.push_str("</m:oMath>");
        Ok(w.out)
    })
}

#[derive(Clone, Default)]
struct Style {
    /// `m:scr` (script) value: double-struck, script, fraktur, sans-serif, monospace.
    scr: Option<&'static str>,
    /// `m:sty` value: p (plain), b (bold), i, bi.
    sty: Option<&'static str>,
    /// Ordinary text (`\text`): `m:nor`.
    normal: bool,
    /// An explicit text colour (`\color`), which wins over the ink colour. Black is a colour
    /// like another here: `{\color{black}x}` stays black on a dark theme (the layout maps black
    /// to "automatic", which a painter can follow; a file has no such second chance). Only runs
    /// carry it: fraction bars, radical signs and brackets take the paragraph's colour.
    color: Option<[u8; 3]>,
}

/// The run written last, so that the next one can join it.
struct LastRun {
    /// Where its `<m:r>` starts, and where its `<m:t` does (the head is what must match).
    start: usize,
    text_at: usize,
    /// The end of the output when it was written.
    end: usize,
    /// Its text, unescaped.
    text: String,
    /// Whether its `<m:t>` carries `xml:space="preserve"`.
    marked: bool,
}

const SPACE_MARK: &str = " xml:space=\"preserve\"";

fn needs_space_mark(text: &str) -> bool {
    text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace)
}

struct Writer {
    /// Join runs with the same properties (off only in tests that compare with the unjoined form).
    merge: bool,
    last: Option<LastRun>,
    out: String,
    sz: u32,
    /// The DrawingML fill of every run (`<a:solidFill>…</a:solidFill>`), or empty.
    fill: String,
}

/// The DrawingML fill for a colour.
fn fill_of([r, g, b]: [u8; 3]) -> String {
    format!("<a:solidFill><a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\"/></a:solidFill>")
}

fn unsupported(what: &str) -> MathError {
    MathError::new(format!("{what} can't be saved as an Office equation."), None)
}

/// Whether XML 1.0 can carry `c`: the same set the xlsx crate's escapes keep, so the OMML is
/// well formed wherever the drawing part embeds it.
fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// `s` escaped for element text; characters XML cannot carry (C0 controls other than tab, newline
/// and carriage return, and U+FFFE and U+FFFF) are dropped, as the xlsx crate's `esc` drops them.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\r' => o.push_str("&#13;"),
            c if is_xml_char(c) => o.push(c),
            _ => {}
        }
    }
    o
}

/// The character a symbol name (`\alpha`, `(`, `\langle`) stands for.
fn symbol(name: &str, mode: Mode) -> Option<String> {
    let m = if mode == Mode::Text { ratex_font::symbols::Mode::Text } else { ratex_font::symbols::Mode::Math };
    if let Some(c) = ratex_font::symbols::get_symbol(name, m).and_then(|s| s.codepoint) {
        return Some(c.to_string());
    }
    if !name.starts_with('\\') {
        return Some(name.to_string());
    }
    match name {
        "\\{" | "\\lbrace" => Some("{".into()),
        "\\}" | "\\rbrace" => Some("}".into()),
        "\\|" | "\\Vert" => Some("‖".into()),
        "\\vert" => Some("|".into()),
        "\\langle" => Some("⟨".into()),
        "\\rangle" => Some("⟩".into()),
        "\\lfloor" => Some("⌊".into()),
        "\\rfloor" => Some("⌋".into()),
        "\\lceil" => Some("⌈".into()),
        "\\rceil" => Some("⌉".into()),
        _ => None,
    }
}

/// A delimiter's character; `.` is no delimiter.
fn delim(d: &str) -> Result<String, MathError> {
    if d == "." {
        return Ok(String::new());
    }
    if d.len() == 1 && d.is_ascii() {
        return Ok(d.to_string());
    }
    symbol(d, Mode::Math).ok_or_else(|| unsupported(&format!("The delimiter {d}")))
}

fn is_big_op(n: &ParseNode) -> Option<String> {
    match n {
        ParseNode::Op { symbol: true, name: Some(name), .. } => symbol(name, Mode::Math),
        _ => None,
    }
}

fn is_rel(n: &ParseNode) -> bool {
    matches!(n, ParseNode::Atom { family: ratex_parser::parse_node::AtomFamily::Rel, .. })
}

impl Writer {
    fn run(&mut self, text: &str, st: &Style) {
        if text.is_empty() {
            return;
        }
        let mut head = String::from("<m:r>");
        if st.scr.is_some() || st.sty.is_some() || st.normal {
            head.push_str("<m:rPr>");
            if st.normal {
                head.push_str("<m:nor/>");
            }
            if let Some(s) = st.scr {
                head.push_str(&format!("<m:scr m:val=\"{s}\"/>"));
            }
            if let Some(s) = st.sty {
                head.push_str(&format!("<m:sty m:val=\"{s}\"/>"));
            }
            head.push_str("</m:rPr>");
        }
        let own_fill;
        let fill = match st.color {
            Some(rgb) => {
                own_fill = fill_of(rgb);
                &own_fill
            }
            None => &self.fill,
        };
        head.push_str(&format!("<a:rPr lang=\"en-US\" sz=\"{}\">{}<a:latin typeface=\"Cambria Math\"/></a:rPr>", self.sz, fill));
        // Characters in a row with the same properties are one run, as Word writes them. The
        // previous run is still the last thing in the output only if nothing was written since.
        // Upright runs stay apart: a function name (`\sin`) is one run of its own, and the xlsx
        // reader finds it by that.
        if self.merge
            && st.sty != Some("p")
            && let Some(last) = self.last.as_mut()
            && last.end == self.out.len()
            && self.out.get(last.start..last.text_at) == Some(head.as_str())
        {
            last.text.push_str(text);
            let was_marked = last.marked;
            last.marked = needs_space_mark(&last.text);
            if last.marked == was_marked {
                self.out.truncate(last.end - "</m:t></m:r>".len());
                self.out.push_str(&esc(text));
            } else {
                self.out.truncate(last.text_at);
                self.out.push_str(&format!("<m:t{}>{}", if last.marked { SPACE_MARK } else { "" }, esc(&last.text)));
            }
            self.out.push_str("</m:t></m:r>");
            last.end = self.out.len();
            return;
        }
        let start = self.out.len();
        self.out.push_str(&head);
        let text_at = self.out.len();
        // Word trims a run's text unless it is marked, so a run that is or ends in whitespace
        // (the space after `\text{if }`) says so.
        let marked = needs_space_mark(text);
        self.out.push_str(&format!("<m:t{}>{}</m:t></m:r>", if marked { SPACE_MARK } else { "" }, esc(text)));
        self.last = Some(LastRun { start, text_at, end: self.out.len(), text: text.to_string(), marked });
    }

    /// `<tag>…</tag>` around the nodes.
    fn wrap(&mut self, tag: &str, nodes: &[ParseNode], st: &Style, depth: usize) -> Result<(), MathError> {
        self.out.push_str(&format!("<{tag}>"));
        self.seq(nodes, st, depth)?;
        self.out.push_str(&format!("</{tag}>"));
        Ok(())
    }

    fn wrap1(&mut self, tag: &str, node: Option<&ParseNode>, st: &Style, depth: usize) -> Result<(), MathError> {
        match node {
            Some(n) => self.wrap(tag, std::slice::from_ref(n), st, depth),
            None => {
                self.out.push_str(&format!("<{tag}/>"));
                Ok(())
            }
        }
    }

    fn seq(&mut self, nodes: &[ParseNode], st: &Style, depth: usize) -> Result<(), MathError> {
        if depth > MAX_WALK {
            return Err(unsupported("An equation this deep"));
        }
        let mut i = 0;
        while let Some(n) = nodes.get(i) {
            i += 1;
            // A big operator takes what follows, up to the next relation, as its operand.
            let (op, sub, sup) = match n {
                ParseNode::SupSub { base: Some(b), sub, sup, .. } => (is_big_op(b), sub.as_deref(), sup.as_deref()),
                other => (is_big_op(other), None, None),
            };
            if let Some(chr) = op {
                let end = nodes.get(i..).and_then(|rest| rest.iter().position(is_rel)).map_or(nodes.len(), |p| i + p);
                let limits = match n {
                    ParseNode::SupSub { base: Some(b), .. } => matches!(**b, ParseNode::Op { limits: true, .. }),
                    _ => false,
                };
                self.out.push_str(&format!(
                    "<m:nary><m:naryPr><m:chr m:val=\"{}\"/><m:limLoc m:val=\"{}\"/>{}{}</m:naryPr>",
                    esc(&chr),
                    if limits { "undOvr" } else { "subSup" },
                    if sub.is_none() { "<m:subHide m:val=\"1\"/>" } else { "" },
                    if sup.is_none() { "<m:supHide m:val=\"1\"/>" } else { "" },
                ));
                self.wrap1("m:sub", sub, st, depth + 1)?;
                self.wrap1("m:sup", sup, st, depth + 1)?;
                self.wrap("m:e", nodes.get(i..end).unwrap_or_default(), st, depth + 1)?;
                self.out.push_str("</m:nary>");
                i = end;
                continue;
            }
            self.node(n, st, depth + 1)?;
        }
        Ok(())
    }

    fn node(&mut self, n: &ParseNode, st: &Style, depth: usize) -> Result<(), MathError> {
        if depth > MAX_WALK {
            return Err(unsupported("An equation this deep"));
        }
        match n {
            ParseNode::Atom { text, mode, .. }
            | ParseNode::MathOrd { text, mode, .. }
            | ParseNode::TextOrd { text, mode, .. }
            | ParseNode::OpToken { text, mode, .. } => {
                let t = symbol(text, *mode).ok_or_else(|| unsupported(text))?;
                self.run(&t, st);
            }
            ParseNode::SpacingNode { text, .. } => {
                let t = match text.as_str() {
                    // A literal space in text mode (`\text{if } x`) is a spacing node of " ".
                    " " | "\\ " | "~" | "\\space" | "\\nobreakspace" => " ",
                    "\\quad" => "\u{2003}",
                    "\\qquad" => "\u{2003}\u{2003}",
                    "\\," | "\\thinspace" => "\u{2009}",
                    "\\:" | "\\>" | "\\medspace" => "\u{205F}",
                    "\\;" | "\\thickspace" => "\u{2004}",
                    _ => "",
                };
                self.run(t, st);
            }
            ParseNode::Kern { .. } => {}
            ParseNode::OrdGroup { body, .. } | ParseNode::MClass { body, .. } => self.seq(body, st, depth)?,
            // `\displaystyle` and `\textstyle` are Office Math's own call; the script styles shrink
            // what follows, which a run here cannot carry.
            ParseNode::Styling { style, body, .. } => {
                if matches!(style, ratex_parser::parse_node::StyleStr::Script | ratex_parser::parse_node::StyleStr::Scriptscript) {
                    return Err(unsupported("A script style command"));
                }
                self.seq(body, st, depth)?;
            }
            // Text in a box is ordinary text, and a poor man's bold is bold.
            ParseNode::HBox { body, .. } => self.seq(body, &Style { normal: true, ..st.clone() }, depth)?,
            ParseNode::Pmb { body, .. } => self.seq(body, &Style { sty: Some("b"), ..st.clone() }, depth)?,
            ParseNode::Color { color, body, .. } => {
                let c = ratex_types::color::Color::parse(color).ok_or_else(|| unsupported(&format!("The colour {color}")))?;
                // A run carries RGB only; `transparent` and alpha hex would be written opaque.
                if c.a < 1.0 {
                    return Err(unsupported(&format!("The translucent colour {color}")));
                }
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let s = Style { color: Some([q(c.r), q(c.g), q(c.b)]), ..st.clone() };
                self.seq(body, &s, depth)?;
            }
            // These change the size or make a link, which a run here cannot carry; saying so
            // beats saving the equation without them.
            ParseNode::Sizing { .. } => return Err(unsupported("A text size command")),
            ParseNode::Href { .. } => return Err(unsupported("A link")),
            ParseNode::Text { body, font, .. } => {
                let mut s = Style { normal: true, ..st.clone() };
                if let Some(f) = font {
                    s.sty = match f.as_str() {
                        "\\textbf" => Some("b"),
                        "\\textit" => Some("i"),
                        _ => s.sty,
                    };
                }
                self.seq(body, &s, depth)?;
            }
            ParseNode::Font { font, body, .. } => {
                let mut s = st.clone();
                match font.trim_start_matches('\\') {
                    "mathbb" | "Bbb" => s.scr = Some("double-struck"),
                    "mathcal" | "mathscr" => s.scr = Some("script"),
                    "mathfrak" | "frak" => s.scr = Some("fraktur"),
                    "mathsf" | "textsf" => s.scr = Some("sans-serif"),
                    "mathtt" | "texttt" => s.scr = Some("monospace"),
                    "mathbf" | "bf" | "textbf" => s.sty = Some("b"),
                    "boldsymbol" | "bm" => s.sty = Some("bi"),
                    "mathrm" | "rm" | "textrm" | "mathnormal" => s.sty = Some("p"),
                    "mathit" | "it" | "textit" => s.sty = Some("i"),
                    other => return Err(unsupported(&format!("\\{other}"))),
                }
                self.node(body, &s, depth + 1)?;
            }
            ParseNode::SupSub { base, sup, sub, .. } => {
                // A function with limits under it (\lim_{x\to 0}, \max_i).
                if let Some(b) = base.as_deref()
                    && let ParseNode::Op { limits: true, symbol: false, .. } | ParseNode::OperatorName { limits: true, .. } = b
                    && sup.is_none()
                {
                    self.out.push_str("<m:limLow><m:e>");
                    self.node(b, st, depth + 1)?;
                    self.out.push_str("</m:e>");
                    self.wrap1("m:lim", sub.as_deref(), st, depth + 1)?;
                    self.out.push_str("</m:limLow>");
                    return Ok(());
                }
                // A brace with a label over or under it (`\overbrace{x}^{n}`, `\underbrace{x}_{n}`):
                // the label is the limit on the brace's side. A script on the other side
                // (`\underbrace{x}_{n}^{2}`) is a script around the whole, `m:sSup` or `m:sSub`,
                // so that it reaches Office Math too; without a label it goes around the bare
                // brace, with no empty limit.
                if let Some(b @ ParseNode::HorizBrace { is_over, .. }) = base.as_deref() {
                    let (lim_tag, label, (script_tag, script_arg), script) = if *is_over {
                        ("m:limUpp", sup.as_deref(), ("m:sSub", "m:sub"), sub.as_deref())
                    } else {
                        ("m:limLow", sub.as_deref(), ("m:sSup", "m:sup"), sup.as_deref())
                    };
                    // Each wrapper around the brace is a level of its own.
                    let inner = if script.is_some() { depth + 1 } else { depth };
                    if script.is_some() {
                        self.out.push_str(&format!("<{script_tag}><m:e>"));
                    }
                    if label.is_some() {
                        self.out.push_str(&format!("<{lim_tag}><m:e>"));
                        self.node(b, st, inner + 1)?;
                        self.out.push_str("</m:e>");
                        self.wrap1("m:lim", label, st, inner + 1)?;
                        self.out.push_str(&format!("</{lim_tag}>"));
                    } else {
                        self.node(b, st, inner)?;
                    }
                    if script.is_some() {
                        self.out.push_str("</m:e>");
                        self.wrap1(script_arg, script, st, depth + 1)?;
                        self.out.push_str(&format!("</{script_tag}>"));
                    }
                    return Ok(());
                }
                let tag = match (sub.is_some(), sup.is_some()) {
                    (true, true) => "m:sSubSup",
                    (true, false) => "m:sSub",
                    _ => "m:sSup",
                };
                self.out.push_str(&format!("<{tag}>"));
                self.wrap1("m:e", base.as_deref(), st, depth + 1)?;
                if sub.is_some() {
                    self.wrap1("m:sub", sub.as_deref(), st, depth + 1)?;
                }
                if sup.is_some() || sub.is_none() {
                    self.wrap1("m:sup", sup.as_deref(), st, depth + 1)?;
                }
                self.out.push_str(&format!("</{tag}>"));
            }
            ParseNode::GenFrac { numer, denom, has_bar_line, left_delim, right_delim, .. } => {
                let delims = left_delim.is_some() || right_delim.is_some();
                if delims {
                    self.out.push_str(&format!(
                        "<m:d><m:dPr><m:begChr m:val=\"{}\"/><m:endChr m:val=\"{}\"/></m:dPr><m:e>",
                        esc(&delim(left_delim.as_deref().unwrap_or("."))?),
                        esc(&delim(right_delim.as_deref().unwrap_or("."))?)
                    ));
                }
                self.out.push_str("<m:f>");
                if !has_bar_line {
                    self.out.push_str("<m:fPr><m:type m:val=\"noBar\"/></m:fPr>");
                }
                self.wrap1("m:num", Some(numer), st, depth + 1)?;
                self.wrap1("m:den", Some(denom), st, depth + 1)?;
                self.out.push_str("</m:f>");
                if delims {
                    self.out.push_str("</m:e></m:d>");
                }
            }
            ParseNode::Sqrt { body, index, .. } => {
                self.out.push_str("<m:rad>");
                match index {
                    Some(ix) => {
                        self.wrap1("m:deg", Some(ix), st, depth + 1)?;
                    }
                    None => self.out.push_str("<m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/>"),
                }
                self.wrap1("m:e", Some(body), st, depth + 1)?;
                self.out.push_str("</m:rad>");
            }
            ParseNode::Op { name: Some(name), symbol: false, .. } => {
                let t = name.trim_start_matches('\\');
                self.run(t, &Style { sty: Some("p"), ..st.clone() });
            }
            ParseNode::Op { name: Some(name), .. } => {
                let t = symbol(name, Mode::Math).ok_or_else(|| unsupported(name))?;
                self.run(&t, st);
            }
            ParseNode::Op { body: Some(body), .. } => self.seq(body, st, depth)?,
            ParseNode::OperatorName { body, .. } => self.seq(body, &Style { sty: Some("p"), ..st.clone() }, depth)?,
            ParseNode::LeftRight { body, left, right, .. } => {
                self.out.push_str(&format!(
                    "<m:d><m:dPr><m:begChr m:val=\"{}\"/><m:endChr m:val=\"{}\"/></m:dPr>",
                    esc(&delim(left)?),
                    esc(&delim(right)?)
                ));
                // \middle splits the contents into separated elements.
                let mut parts: Vec<&[ParseNode]> = Vec::new();
                let mut start = 0;
                for (k, b) in body.iter().enumerate() {
                    if matches!(b, ParseNode::Middle { .. }) {
                        parts.push(body.get(start..k).unwrap_or_default());
                        start = k + 1;
                    }
                }
                parts.push(body.get(start..).unwrap_or_default());
                if parts.len() > 1 {
                    let sep =
                        body.iter().find_map(|b| if let ParseNode::Middle { delim: d, .. } = b { delim(d).ok() } else { None }).unwrap_or_default();
                    // Rewrite the dPr with the separator: drop what was pushed and start again.
                    let cut = self.out.rfind("<m:d>").unwrap_or(self.out.len());
                    self.out.truncate(cut);
                    self.out.push_str(&format!(
                        "<m:d><m:dPr><m:begChr m:val=\"{}\"/><m:sepChr m:val=\"{}\"/><m:endChr m:val=\"{}\"/></m:dPr>",
                        esc(&delim(left)?),
                        esc(&sep),
                        esc(&delim(right)?)
                    ));
                }
                for p in parts {
                    self.wrap("m:e", p, st, depth + 1)?;
                }
                self.out.push_str("</m:d>");
            }
            ParseNode::DelimSizing { delim: d, .. } => {
                let t = delim(d)?;
                self.run(&t, st);
            }
            ParseNode::Array { body, is_cd: None | Some(false), .. } => {
                let cols = body.iter().map(Vec::len).max().unwrap_or(0).max(1);
                self.out.push_str(&format!(
                    "<m:m><m:mPr><m:mcs><m:mc><m:mcPr><m:count m:val=\"{cols}\"/><m:mcJc m:val=\"center\"/></m:mcPr></m:mc></m:mcs></m:mPr>"
                ));
                // Every row the parser gives is written: RaTeX itself drops the empty last row a
                // trailing `\\` leaves, and an empty row anywhere else is the author's blank line.
                for row in body {
                    self.out.push_str("<m:mr>");
                    for k in 0..cols {
                        self.wrap1("m:e", row.get(k), st, depth + 1)?;
                    }
                    self.out.push_str("</m:mr>");
                }
                self.out.push_str("</m:m>");
            }
            ParseNode::Accent { label, base, .. } => {
                let chr = match label.as_str() {
                    "\\hat" | "\\widehat" => "\u{0302}",
                    "\\check" | "\\widecheck" => "\u{030C}",
                    "\\tilde" | "\\widetilde" => "\u{0303}",
                    "\\acute" => "\u{0301}",
                    "\\grave" => "\u{0300}",
                    "\\dot" => "\u{0307}",
                    "\\ddot" => "\u{0308}",
                    "\\breve" => "\u{0306}",
                    "\\bar" => "\u{0305}",
                    "\\vec" | "\\overrightarrow" => "\u{20D7}",
                    "\\overleftarrow" => "\u{20D6}",
                    "\\mathring" => "\u{030A}",
                    other => return Err(unsupported(other)),
                };
                self.out.push_str(&format!("<m:acc><m:accPr><m:chr m:val=\"{chr}\"/></m:accPr>"));
                self.wrap1("m:e", Some(base), st, depth + 1)?;
                self.out.push_str("</m:acc>");
            }
            ParseNode::Overline { body, .. } | ParseNode::Underline { body, .. } => {
                let pos = if matches!(n, ParseNode::Overline { .. }) { "top" } else { "bot" };
                self.out.push_str(&format!("<m:bar><m:barPr><m:pos m:val=\"{pos}\"/></m:barPr>"));
                self.wrap1("m:e", Some(body), st, depth + 1)?;
                self.out.push_str("</m:bar>");
            }
            ParseNode::HorizBrace { is_over, base, .. } => {
                let (chr, pos, vert) = if *is_over { ("\u{23DE}", "top", "bot") } else { ("\u{23DF}", "bot", "top") };
                self.out.push_str(&format!(
                    "<m:groupChr><m:groupChrPr><m:chr m:val=\"{chr}\"/><m:pos m:val=\"{pos}\"/><m:vertJc m:val=\"{vert}\"/></m:groupChrPr>"
                ));
                self.wrap1("m:e", Some(base), st, depth + 1)?;
                self.out.push_str("</m:groupChr>");
            }
            ParseNode::Enclose { label, body, .. } if label == "\\boxed" || label == "\\fbox" => {
                self.out.push_str("<m:borderBox>");
                self.wrap1("m:e", Some(body), st, depth + 1)?;
                self.out.push_str("</m:borderBox>");
            }
            ParseNode::Phantom { body, .. } => {
                self.out.push_str("<m:phant><m:phantPr><m:show m:val=\"0\"/></m:phantPr>");
                self.wrap("m:e", body, st, depth + 1)?;
                self.out.push_str("</m:phant>");
            }
            ParseNode::Internal { .. } | ParseNode::NoNumber { .. } | ParseNode::ColorToken { .. } => {}
            other => return Err(unsupported(&format!("\"{}\"", node_name(other)))),
        }
        Ok(())
    }
}

/// A short name for an unsupported construct, for the error.
fn node_name(n: &ParseNode) -> String {
    let dbg = format!("{n:?}");
    dbg.split([' ', '{', '(']).next().unwrap_or("This construct").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn om(s: &str) -> String {
        to_omml(s, 11.0, None).unwrap()
    }

    /// The markup without the per-run DrawingML properties, to keep assertions readable.
    fn bare(s: &str) -> String {
        let mut o = om(s);
        while let Some(a) = o.find("<a:rPr") {
            let Some(b) = o[a..].find("</a:rPr>") else { break };
            o.replace_range(a..a + b + "</a:rPr>".len(), "");
        }
        o
    }

    #[test]
    fn runs_fractions_and_roots() {
        assert_eq!(bare("x+1"), "<m:oMath><m:r><m:t>x+1</m:t></m:r></m:oMath>");
        assert!(om("x").contains("sz=\"1100\""));
        assert_eq!(
            bare("\\frac{a}{b}"),
            "<m:oMath><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>"
        );
        assert!(
            bare("\\binom{n}{k}")
                .contains("<m:d><m:dPr><m:begChr m:val=\"(\"/><m:endChr m:val=\")\"/></m:dPr><m:e><m:f><m:fPr><m:type m:val=\"noBar\"/>")
        );
        assert!(bare("\\sqrt{x}").starts_with("<m:oMath><m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e>"));
        assert!(bare("\\sqrt[3]{x}").contains("<m:deg><m:r><m:t>3</m:t></m:r></m:deg>"));
        assert!(bare("\\alpha \\le \\infty").contains("<m:t>α≤∞</m:t>"));
    }

    #[test]
    fn scripts_operators_and_limits() {
        assert_eq!(bare("x^2"), "<m:oMath><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:oMath>");
        assert!(bare("a_{i}^{2}").starts_with("<m:oMath><m:sSubSup><m:e>"));
        let s = bare("\\sum_{k=0}^{n} k = m");
        assert!(s.starts_with("<m:oMath><m:nary><m:naryPr><m:chr m:val=\"∑\"/><m:limLoc m:val=\"undOvr\"/></m:naryPr><m:sub>"), "{s}");
        assert!(s.contains("<m:e><m:r><m:t>k</m:t></m:r></m:e></m:nary><m:r><m:t>=m</m:t>"), "the operand stops at the relation: {s}");
        assert!(bare("\\int x").contains("<m:subHide m:val=\"1\"/><m:supHide m:val=\"1\"/>"));
        let l = bare("\\lim_{n\\to\\infty} a");
        assert!(l.starts_with("<m:oMath><m:limLow><m:e><m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>lim</m:t>"), "{l}");
        assert!(bare("\\sin x").contains("<m:sty m:val=\"p\"/></m:rPr><m:t>sin</m:t>"));
    }

    #[test]
    fn delimiters_matrices_accents_and_fonts() {
        let p = bare("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}");
        assert!(p.contains("<m:d><m:dPr><m:begChr m:val=\"(\"/><m:endChr m:val=\")\"/></m:dPr><m:e><m:m>"), "{p}");
        assert_eq!(p.matches("<m:mr>").count(), 2);
        assert_eq!(p.matches("<m:e>").count(), 5);
        assert!(bare("\\left\\{ x \\right.").contains("<m:begChr m:val=\"{\"/><m:endChr m:val=\"\"/>"));
        assert!(bare("\\left\\langle a \\middle| b \\right\\rangle").contains("<m:sepChr m:val=\"|\"/>"));
        assert!(bare("\\hat{x}").contains("<m:acc><m:accPr><m:chr m:val=\"\u{0302}\"/>"));
        assert!(bare("\\overline{z}").contains("<m:bar><m:barPr><m:pos m:val=\"top\"/>"));
        assert!(bare("\\mathbb{R}").contains("<m:rPr><m:scr m:val=\"double-struck\"/></m:rPr>"));
        assert!(bare("\\text{if } x").contains("<m:rPr><m:nor/></m:rPr>"));
        assert!(bare("\\overbrace{a+b}^{n}").contains("<m:limUpp><m:e><m:groupChr>"));
        assert!(bare("a < b \\& c").contains("<m:t>a&lt;b&amp;c</m:t>") && bare("a \\& b").contains("<m:t>a&amp;b</m:t>"));
    }

    #[test]
    fn an_underbrace_with_both_scripts_keeps_its_exponent() {
        // The label under the brace is its limit; the exponent on the other side is a
        // superscript around the whole, so that an application reading the Office Math sees it
        // (the source in `descr` hid its loss in a GridCraft-to-GridCraft round trip).
        let brace = |chr: &str, pos: &str, vert: &str| {
            format!(
                "<m:groupChr><m:groupChrPr><m:chr m:val=\"{chr}\"/><m:pos m:val=\"{pos}\"/><m:vertJc m:val=\"{vert}\"/></m:groupChrPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:groupChr>"
            )
        };
        let under = brace("\u{23DF}", "bot", "top");
        let over = brace("\u{23DE}", "top", "bot");
        assert_eq!(
            bare("\\underbrace{x}_{n}^{2}"),
            format!(
                "<m:oMath><m:sSup><m:e><m:limLow><m:e>{under}</m:e><m:lim><m:r><m:t>n</m:t></m:r></m:lim></m:limLow></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:oMath>"
            )
        );
        // The mirror: the label over the brace, the subscript around the whole.
        assert_eq!(
            bare("\\overbrace{x}^{n}_{m}"),
            format!(
                "<m:oMath><m:sSub><m:e><m:limUpp><m:e>{over}</m:e><m:lim><m:r><m:t>n</m:t></m:r></m:lim></m:limUpp></m:e><m:sub><m:r><m:t>m</m:t></m:r></m:sub></m:sSub></m:oMath>"
            )
        );
        // Nested braces keep every script, one wrapper per level.
        let nested = bare("\\underbrace{\\underbrace{x}_{a}^{b}}_{c}^{d}");
        assert_eq!(nested.matches("<m:limLow>").count(), 2, "{nested}");
        assert_eq!(nested.matches("<m:sup>").count(), 2, "{nested}");
        for t in ["a", "b", "c", "d"] {
            assert!(nested.contains(&format!("<m:t>{t}</m:t>")), "{nested}");
        }
    }

    #[test]
    fn an_underbrace_without_a_label_keeps_its_superscript() {
        // No label: the script goes around the bare brace, and there is no empty limit.
        let under = bare("\\underbrace{x}^{2}");
        assert!(under.starts_with("<m:oMath><m:sSup><m:e><m:groupChr>"), "{under}");
        assert!(under.ends_with("</m:groupChr></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:oMath>"), "{under}");
        assert!(!under.contains("<m:lim"), "{under}");
        let over = bare("\\overbrace{x}_{m}");
        assert!(over.starts_with("<m:oMath><m:sSub><m:e><m:groupChr>"), "{over}");
        assert!(over.ends_with("</m:groupChr></m:e><m:sub><m:r><m:t>m</m:t></m:r></m:sub></m:sSub></m:oMath>"), "{over}");
        assert!(!over.contains("<m:lim"), "{over}");
        // A brace with neither script, and one with its label alone, are as they were.
        let plain = bare("\\underbrace{x}");
        assert!(plain.starts_with("<m:oMath><m:groupChr>") && !plain.contains("<m:sSup>") && !plain.contains("<m:lim"), "{plain}");
        let labelled = bare("\\underbrace{x}_{n}");
        assert!(labelled.starts_with("<m:oMath><m:limLow><m:e><m:groupChr>") && !labelled.contains("<m:sSup>"), "{labelled}");
    }

    #[test]
    fn the_unsupported_subset_is_an_error() {
        for s in ["\\begin{CD} A @>>> B \\end{CD}", "\\rule{1em}{1em}", "\\xrightarrow{f}", "\\verb|x|"] {
            assert!(to_omml(s, 11.0, None).is_err(), "{s}");
        }
        assert!(to_omml("\\frac{1}{", 11.0, None).is_err());
        assert!(to_omml(&"x".repeat(5000), 11.0, None).is_err());
        assert!(om("x").starts_with("<m:oMath>"));
        assert!(to_omml("x", f32::NAN, None).unwrap().contains("sz=\"1100\""));
    }

    #[test]
    fn colours_are_kept_and_size_and_links_are_errors() {
        let red = "<a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill>";
        let o = om("{\\color{red}x}y\\textcolor{blue}{z}");
        assert_eq!(o.matches(red).count(), 1, "{o}");
        assert_eq!(o.matches("<a:solidFill>").count(), 2, "only x and z are coloured: {o}");
        assert!(o.contains("val=\"0000FF\""), "{o}");
        // A colour applies to what follows it in the group, through scripts and fractions.
        let o = om("\\color{#00ff00}\\frac{a}{b}^2");
        assert_eq!(o.matches("val=\"00FF00\"").count(), o.matches("<m:r>").count(), "{o}");
        // An explicit black is black, not the paragraph's colour, and wins over the ink.
        let o = to_omml("{\\color{black}x}y", 11.0, Some([0xFF, 0xFF, 0xFF])).unwrap();
        assert!(o.find("000000").is_some_and(|b| o.find("FFFFFF").is_some_and(|w| b < w)), "{o}");
        assert!(om("{\\color{black}x}y").contains("val=\"000000\""), "black without an ink colour is still written");
        // The inner colour wins and the outer one returns after the group.
        let o = om("\\color{red}a{\\color{blue}b}c");
        let fills: Vec<&str> = o.matches("srgbClr val=\"").map(|_| "").collect();
        assert_eq!(fills.len(), 3, "{o}");
        let (r1, b, r2) = (o.find("FF0000"), o.find("0000FF"), o.rfind("FF0000"));
        assert!(r1 < b && b < r2, "{o}");
        for (s, what) in [
            ("\\large x", "text size"),
            ("\\frac{\\tiny a}{b}", "text size"),
            ("\\scriptstyle x", "script style"),
            ("\\href{https://example.org}{x}", "link"),
            ("\\color{nocolour}x", "colour"),
            ("\\color{transparent}x", "translucent colour"),
            ("\\textcolor{#ff000080}{x}", "translucent colour"),
        ] {
            let e = to_omml(s, 11.0, None).unwrap_err();
            assert!(e.message.contains(what), "{s}: {e}");
        }
        // Boxed text is ordinary text, `\pmb` is bold.
        assert!(bare("\\hbox{ab}").contains("<m:nor/>"));
        assert!(bare("\\pmb{x}").contains("<m:sty m:val=\"b\"/>"));
        assert!(to_omml("\\displaystyle x", 11.0, None).is_ok());
    }

    /// The runs of OMML as (properties, text), with neighbours of the same properties joined.
    fn joined_runs(omml: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut rest = omml;
        let mut adjacent = false;
        while let Some(at) = rest.find("<m:r>") {
            // A run directly after the previous one (nothing written between them).
            adjacent = adjacent && at == 0;
            let body = &rest[at + "<m:r>".len()..];
            let t = body.find("<m:t").unwrap();
            let head = body[..t].to_string();
            let text_from = t + body[t..].find('>').unwrap() + 1;
            let end = body.find("</m:t></m:r>").unwrap();
            let text = body[text_from..end].to_string();
            // The mark must be there exactly when the run starts or ends in whitespace.
            let marked = body[t..text_from].contains("xml:space=\"preserve\"");
            assert_eq!(marked, needs_space_mark(&text), "{omml}");
            match out.last_mut() {
                Some((h, tx)) if adjacent && *h == head => tx.push_str(&text),
                _ => out.push((head, text)),
            }
            rest = &body[end + "</m:t></m:r>".len()..];
            adjacent = true;
        }
        out
    }

    #[test]
    fn joined_runs_read_as_the_same_text_per_style() {
        for s in [
            "x+1=2",
            "\\text{if } x \\ge 0",
            "{\\color{red}ab}c\\textcolor{blue}{de}",
            "\\mathbb{R}\\mathbf{xyz}\\mathrm{sin}x\\mathit{ab}",
            "\\frac{12}{34}+\\sqrt{abc}+x^{12}_{ab}",
            "\\begin{pmatrix} 12 & ab \\\\ c & d \\end{pmatrix}",
            "\\left(ab\\middle|cd\\right)",
            "\\sum_{i=1}^{n} ab + \\lim_{x\\to 0} f",
            "\\text{a } \\text{ b}\\text{c d}",
            "\\hbox{ab}\\pmb{cd}",
            "\\text{a }x\\text{ a}\\text{a} \\text{b} ",
            "\\sin\\cos x\\ln\\sin",
        ] {
            let old = write(s, 11.0, None, false).unwrap();
            let new = write(s, 11.0, None, true).unwrap();
            assert_eq!(joined_runs(&old), joined_runs(&new), "{s}");
            assert!(new.len() <= old.len(), "{s}");
        }
    }

    #[test]
    fn long_runs_are_one_run() {
        let digits = "1234567890".repeat(409);
        let (old, new) = (write(&digits, 11.0, None, false).unwrap(), to_omml(&digits, 11.0, None).unwrap());
        assert_eq!(new.matches("<m:r>").count(), 1);
        assert!(new.contains(&digits));
        assert!(new.len() < 5_000 && old.len() > 300_000, "{} against {}", new.len(), old.len());
        let text = format!("\\text{{{}}}", "ab ".repeat(1300));
        assert_eq!(to_omml(&text, 11.0, None).unwrap().matches("<m:r>").count(), 1);
        assert_eq!(bare("\\sin\\cos").matches("<m:r>").count(), 2, "function names stay separate runs");
        // Another property, or any other element between two runs, keeps them apart.
        assert_eq!(bare("a\\mathbf{b}c").matches("<m:r>").count(), 3);
        assert_eq!(bare("a^b c").matches("<m:r>").count(), 3);
    }

    #[test]
    fn a_literal_space_in_text_mode_is_kept() {
        // RaTeX gives a text-mode space as a spacing node whose text is " "; it is a run of its
        // own, marked `xml:space="preserve"` so that Word keeps it, and `\text{if } x` does not
        // come out as `ifx`.
        let o = bare("\\text{a b}");
        assert!(o.contains("<m:t>a b</m:t>"), "text with the space inside needs no mark: {o}");
        let o = bare("\\text{if } x");
        assert!(o.contains("<m:t xml:space=\"preserve\">if </m:t></m:r><m:r><m:t>x</m:t>"), "{o}");
        let o = bare("\\text{a } \\text{ b}");
        assert!(o.contains("<m:t>a  b</m:t>"), "{o}");
        assert!(!bare("x+1").contains("xml:space"), "runs without surrounding whitespace are not marked");
    }

    #[test]
    fn runs_carry_the_ink_colour_when_one_is_given() {
        let o = to_omml("x+1", 11.0, Some([0x1F, 0x4E, 0x79])).unwrap();
        let run =
            "<a:rPr lang=\"en-US\" sz=\"1100\"><a:solidFill><a:srgbClr val=\"1F4E79\"/></a:solidFill><a:latin typeface=\"Cambria Math\"/></a:rPr>";
        assert_eq!(o.matches(run).count(), 1, "{o}");
        assert!(!om("x+1").contains("<a:solidFill>"), "no ink: the paragraph's colour applies");
    }

    #[test]
    fn characters_xml_cannot_carry_are_dropped_from_runs() {
        // U+FFFE and U+FFFF are not XML characters, nor are C0 controls other than tab, newline
        // and carriage return; the xlsx escapes drop them and this writer does the same, so the
        // OMML it hands the drawing part is well formed. U+FDD0 is a Unicode noncharacter but an
        // XML character, kept as the xlsx escapes keep it.
        assert_eq!(esc("a\u{FFFE}b\u{FFFF}c\u{1}d\u{B}e\u{7F}f"), "abcde\u{7F}f");
        assert_eq!(esc("a\tb\nc\rd\u{FDD0}e"), "a\tb\nc&#13;d\u{FDD0}e");
        for (src, bad) in [("x\u{FFFE}y", '\u{FFFE}'), ("x\u{FFFF}y", '\u{FFFF}'), ("\\text{a\u{FFFF}b}", '\u{FFFF}')] {
            if let Ok(o) = to_omml(src, 11.0, None) {
                assert!(!o.contains(bad), "{src:?}: {o}");
            }
        }
    }

    #[test]
    fn only_a_trailing_empty_row_is_dropped() {
        // RaTeX drops the empty last row a trailing `\\` leaves (its `environments.rs`), and the
        // writer keeps every row it is given, so a blank row in the middle, written as `\\ \\` or
        // as `{}`, reaches Office as the sheet shows it. A RaTeX release that stopped dropping the
        // trailing row would show up here as a third row.
        assert_eq!(bare("\\begin{matrix} a \\\\ b \\\\ \\end{matrix}").matches("<m:mr>").count(), 2);
        assert_eq!(bare("\\begin{cases} x \\\\ \\\\ y \\end{cases}").matches("<m:mr>").count(), 3);
        assert_eq!(bare("\\begin{matrix} a \\\\ {} \\\\ b \\end{matrix}").matches("<m:mr>").count(), 3);
        assert_eq!(bare("\\begin{matrix} \\\\ \\end{matrix}").matches("<m:mr>").count(), 1, "one blank row is still a row");
    }

    #[test]
    fn every_gallery_example_converts() {
        for s in [
            "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}",
            "a^2 + b^2 = c^2",
            "(x + a)^n = \\sum_{k=0}^{n} \\binom{n}{k} x^k a^{n-k}",
            "A = \\pi r^2",
            "e^{i\\pi} + 1 = 0",
            "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
            "f(x) = \\begin{cases} x & x \\ge 0 \\\\ -x & x < 0 \\end{cases}",
        ] {
            assert!(to_omml(s, 11.0, None).is_ok(), "{s}: {:?}", to_omml(s, 11.0, None));
        }
    }
}
