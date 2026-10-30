//! The offline XPath subset used by FILTERXML.

use std::collections::HashSet;

use gridcraft_core::parse::parse_input;
use gridcraft_core::{Array, CellError, Value};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::Event;
use quick_xml::name::QName;

use crate::util::{MAX_TEXT, R, text};
use crate::{Arg, Ctx};

const MAX_DEPTH: usize = 256;
const MAX_NODES: usize = 16_384;
const MAX_WORK: usize = 1_000_000;

#[derive(Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<usize>,
}
#[derive(Clone, Copy)]
enum Axis {
    Child,
    Descendant,
}

#[derive(Clone)]
enum Test {
    Name(String),
    Any,
    Attribute(String),
}

#[derive(Clone)]
enum Predicate {
    Position(usize),
    AttributeEquals(String, String),
}

#[derive(Clone)]
struct Step {
    axis: Axis,
    test: Test,
    predicate: Option<Predicate>,
}

pub(crate) fn filterxml(args: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
    let xml = text(args, 0)?;
    let xpath = text(args, 1)?;
    if xml.is_empty() || xpath.is_empty() || xml.chars().count() > MAX_TEXT {
        return Err(CellError::Value);
    }
    let steps = parse_xpath(&xpath)?;
    let (nodes, root) = parse_xml(&xml)?;
    let mut selected = vec![root];
    let mut work = 0usize;
    for (step_index, step) in steps.iter().enumerate() {
        let mut next = Vec::new();
        let mut seen = HashSet::new();
        if let Test::Attribute(name) = &step.test {
            if step.predicate.is_some() {
                return Err(CellError::Value);
            }
            for current in &selected {
                work = work.checked_add(1).ok_or(CellError::Value)?;
                if work > MAX_WORK {
                    return Err(CellError::Value);
                }
                if nodes.get(*current).is_some_and(|node| node.attrs.iter().any(|(key, _)| key == name)) && seen.insert(*current) {
                    next.push(*current);
                }
            }
            selected = next;
            continue;
        }
        for current in &selected {
            let candidate_groups = match step.axis {
                Axis::Child if step_index == 0 => vec![vec![*current]],
                Axis::Child => vec![nodes.get(*current).ok_or(CellError::Value)?.children.clone()],
                Axis::Descendant => descendant_child_groups(&nodes, *current, step_index == 0)?,
            };
            work = work.checked_add(candidate_groups.iter().map(Vec::len).sum::<usize>()).ok_or(CellError::Value)?;
            if work > MAX_WORK {
                return Err(CellError::Value);
            }
            for candidates in candidate_groups {
                let mut position = 0usize;
                for candidate in candidates {
                    if matches_test(nodes.get(candidate).ok_or(CellError::Value)?, &step.test) {
                        position = position.checked_add(1).ok_or(CellError::Value)?;
                        if predicate_matches(nodes.get(candidate).ok_or(CellError::Value)?, step.predicate.as_ref(), position)
                            && seen.insert(candidate)
                        {
                            next.push(candidate);
                        }
                    }
                }
            }
        }
        next.sort_unstable();
        selected = next;
        if selected.len() > MAX_NODES {
            return Err(CellError::Value);
        }
    }

    let last = steps.last().ok_or(CellError::Value)?;
    let mut values = Vec::with_capacity(selected.len());
    for index in selected {
        let node = nodes.get(index).ok_or(CellError::Value)?;
        let raw = match &last.test {
            Test::Attribute(name) => node.attrs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str()).ok_or(CellError::Value)?,
            _ if !node.children.is_empty() => return Err(CellError::Value),
            _ => node.text.trim(),
        };
        if raw.is_empty() {
            return Err(CellError::Value);
        }
        let parsed = parse_input(raw, ctx.date_system()).value;
        if matches!(parsed, Value::Empty | Value::Error(_)) {
            return Err(CellError::Value);
        }
        values.push(parsed);
    }
    match values.len() {
        0 => Err(CellError::Value),
        1 => values.into_iter().next().ok_or(CellError::Value),
        _ => Array::new(values.len(), 1, values).map(Value::from).ok_or(CellError::Value),
    }
}

fn parse_xml(xml: &str) -> R<(Vec<Node>, usize)> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut nodes = Vec::new();
    let mut stack = Vec::new();
    let mut root = None;
    let mut text_len = 0usize;
    loop {
        match reader.read_event().map_err(|_| CellError::Value)? {
            Event::Start(element) => {
                if stack.len() >= MAX_DEPTH || nodes.len() >= MAX_NODES {
                    return Err(CellError::Value);
                }
                let index = add_node(&mut nodes, &stack, &mut root, &element)?;
                stack.push(index);
            }
            Event::Empty(element) => {
                if nodes.len() >= MAX_NODES {
                    return Err(CellError::Value);
                }
                add_node(&mut nodes, &stack, &mut root, &element)?;
            }
            Event::End(_) => {
                stack.pop().ok_or(CellError::Value)?;
            }
            Event::Text(event) => {
                let decoded = event.xml10_content().map_err(|_| CellError::Value)?;
                if stack.is_empty() {
                    if !decoded.trim().is_empty() {
                        return Err(CellError::Value);
                    }
                    continue;
                }
                if let Some(index) = stack.last().copied() {
                    text_len = text_len.checked_add(decoded.chars().count()).ok_or(CellError::Value)?;
                    if text_len > MAX_TEXT {
                        return Err(CellError::Value);
                    }
                    nodes.get_mut(index).ok_or(CellError::Value)?.text.push_str(&decoded);
                }
            }
            Event::GeneralRef(reference) => {
                if stack.is_empty() {
                    return Err(CellError::Value);
                }
                if let Some(index) = stack.last().copied() {
                    let decoded = reference.resolve_char_ref().map_err(|_| CellError::Value)?;
                    let ch = decoded
                        .or_else(|| match reference.as_ref() {
                            b"amp" => Some('&'),
                            b"lt" => Some('<'),
                            b"gt" => Some('>'),
                            b"quot" => Some('"'),
                            b"apos" => Some('\''),
                            _ => None,
                        })
                        .ok_or(CellError::Value)?;
                    if !is_xml10_char(ch) {
                        return Err(CellError::Value);
                    }
                    text_len = text_len.checked_add(1).ok_or(CellError::Value)?;
                    if text_len > MAX_TEXT {
                        return Err(CellError::Value);
                    }
                    nodes.get_mut(index).ok_or(CellError::Value)?.text.push(ch);
                }
            }
            Event::CData(event) => {
                if stack.is_empty() {
                    return Err(CellError::Value);
                }
                if let Some(index) = stack.last().copied() {
                    let decoded = event.decode().map_err(|_| CellError::Value)?;
                    text_len = text_len.checked_add(decoded.chars().count()).ok_or(CellError::Value)?;
                    if text_len > MAX_TEXT {
                        return Err(CellError::Value);
                    }
                    nodes.get_mut(index).ok_or(CellError::Value)?.text.push_str(&decoded);
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
            Event::DocType(_) => return Err(CellError::Value),
        }
    }
    if !stack.is_empty() {
        return Err(CellError::Value);
    }
    Ok((nodes, root.ok_or(CellError::Value)?))
}

fn local_name(name: QName<'_>) -> String {
    let bytes = name.as_ref();
    let bytes = bytes.get(bytes.iter().rposition(|byte| *byte == b':').map_or(0, |at| at + 1)..).unwrap_or_default();
    std::str::from_utf8(bytes).map(str::to_string).unwrap_or_default()
}

fn is_xml10_char(ch: char) -> bool {
    matches!(ch, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn add_node(nodes: &mut Vec<Node>, stack: &[usize], root: &mut Option<usize>, element: &quick_xml::events::BytesStart<'_>) -> R<usize> {
    let name = local_name(element.name());
    if name.is_empty() {
        return Err(CellError::Value);
    }
    let mut node = Node { name, ..Node::default() };
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| CellError::Value)?;
        if attribute.key.as_ref() == b"xmlns" || attribute.key.as_ref().starts_with(b"xmlns:") {
            continue;
        }
        let key = local_name(attribute.key);
        let value = attribute.normalized_value(XmlVersion::Implicit1_0).map_err(|_| CellError::Value)?.into_owned();
        node.attrs.push((key, value));
    }
    let index = nodes.len();
    nodes.push(node);
    if let Some(parent) = stack.last().copied() {
        nodes.get_mut(parent).ok_or(CellError::Value)?.children.push(index);
    } else if root.replace(index).is_some() {
        return Err(CellError::Value);
    }
    Ok(index)
}

fn descendant_child_groups(nodes: &[Node], root: usize, include_root: bool) -> R<Vec<Vec<usize>>> {
    let mut groups = Vec::new();
    if include_root {
        groups.push(vec![root]);
    }
    let mut pending = vec![root];
    while let Some(index) = pending.pop() {
        let node = nodes.get(index).ok_or(CellError::Value)?;
        groups.push(node.children.clone());
        pending.extend(node.children.iter().rev().copied());
        if pending.len() > MAX_NODES {
            return Err(CellError::Value);
        }
    }
    Ok(groups)
}

fn matches_test(node: &Node, test: &Test) -> bool {
    match test {
        Test::Name(name) => node.name == *name,
        Test::Any => true,
        Test::Attribute(name) => node.attrs.iter().any(|(key, _)| key == name),
    }
}

fn predicate_matches(node: &Node, predicate: Option<&Predicate>, position: usize) -> bool {
    match predicate {
        None => true,
        Some(Predicate::Position(n)) => position == *n,
        Some(Predicate::AttributeEquals(name, value)) => node.attrs.iter().any(|(key, actual)| key == name && actual == value),
    }
}

fn parse_xpath(xpath: &str) -> R<Vec<Step>> {
    let bytes = xpath.as_bytes();
    let mut at = 0usize;
    let mut steps = Vec::new();
    if bytes.first() != Some(&b'/') {
        return Err(CellError::Value);
    }
    while at < bytes.len() {
        if bytes.get(at) != Some(&b'/') {
            return Err(CellError::Value);
        }
        at += 1;
        let axis = if bytes.get(at) == Some(&b'/') {
            at += 1;
            Axis::Descendant
        } else {
            Axis::Child
        };
        let start = at;
        while bytes.get(at).is_some_and(|b| *b != b'/' && *b != b'[') {
            at += 1;
        }
        let raw = std::str::from_utf8(bytes.get(start..at).ok_or(CellError::Value)?).map_err(|_| CellError::Value)?;
        if raw.is_empty() {
            return Err(CellError::Value);
        }
        let test = if let Some(attribute) = raw.strip_prefix('@') {
            if attribute.is_empty() || at != bytes.len() || steps.is_empty() || matches!(axis, Axis::Descendant) {
                return Err(CellError::Value);
            }
            let name = attribute.rsplit(':').next().ok_or(CellError::Value)?;
            if name.is_empty() {
                return Err(CellError::Value);
            }
            Test::Attribute(name.to_string())
        } else if raw == "*" {
            Test::Any
        } else if raw.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')) {
            let name = raw.rsplit(':').next().ok_or(CellError::Value)?;
            if name.is_empty() {
                return Err(CellError::Value);
            }
            Test::Name(name.to_string())
        } else {
            return Err(CellError::Value);
        };
        let predicate = if bytes.get(at) == Some(&b'[') {
            at += 1;
            let start = at;
            while bytes.get(at).is_some_and(|b| *b != b']') {
                at += 1;
            }
            if bytes.get(at) != Some(&b']') {
                return Err(CellError::Value);
            }
            let raw = std::str::from_utf8(bytes.get(start..at).ok_or(CellError::Value)?).map_err(|_| CellError::Value)?;
            at += 1;
            Some(parse_predicate(raw)?)
        } else {
            None
        };
        steps.push(Step { axis, test, predicate });
        if matches!(steps.last().map(|step| &step.test), Some(Test::Attribute(_))) && at != bytes.len() {
            return Err(CellError::Value);
        }
    }
    if steps.is_empty() || steps.len() > MAX_DEPTH {
        return Err(CellError::Value);
    }
    Ok(steps)
}

fn parse_predicate(raw: &str) -> R<Predicate> {
    if !raw.is_empty() && raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return raw.parse::<usize>().ok().filter(|n| *n > 0).map(Predicate::Position).ok_or(CellError::Value);
    }
    let expression = raw.strip_prefix('@').ok_or(CellError::Value)?;
    let (name, expected) = expression.split_once('=').ok_or(CellError::Value)?;
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')) {
        return Err(CellError::Value);
    }
    let quote = expected.as_bytes().first().copied().ok_or(CellError::Value)?;
    if !matches!(quote, b'\'' | b'"') || expected.as_bytes().last().copied() != Some(quote) || expected.len() < 2 {
        return Err(CellError::Value);
    }
    Ok(Predicate::AttributeEquals(name.rsplit(':').next().ok_or(CellError::Value)?.to_string(), expected[1..expected.len() - 1].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::DateSystem;

    struct TestCtx;

    impl Ctx for TestCtx {
        fn date_system(&self) -> DateSystem {
            DateSystem::D1900
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.0
        }
    }

    fn evaluate(xml: &str, xpath: &str) -> Value {
        filterxml(&[Arg::val(xml), Arg::val(xpath)], &mut TestCtx).unwrap_or_else(Value::Error)
    }

    #[test]
    fn filterxml_paths_predicates_namespaces_attributes_and_values() {
        assert_eq!(evaluate("<r><v>7</v><v>9</v></r>", "//v"), Value::from(Array::column(vec![Value::Number(7.0), Value::Number(9.0)])));
        assert_eq!(evaluate("<r><v>7</v><v>9</v></r>", "//v[2]"), Value::Number(9.0));
        assert_eq!(evaluate("<r><v>abc</v></r>", "//v"), Value::from("abc"));
        assert_eq!(evaluate("<r><v a='5'/></r>", "//v/@a"), Value::Number(5.0));
        assert_eq!(evaluate("<r xmlns:p='urn:x' p:a='5'/>", "//r/@p:a"), Value::Number(5.0));
        assert_eq!(evaluate("<r xmlns='urn:x'><v>1</v></r>", "//v"), Value::Number(1.0));
        assert_eq!(evaluate("<r><v> 3 </v></r>", "//v"), Value::Number(3.0));
        assert_eq!(evaluate("<r>&amp;lt;</r>", "/r"), Value::from("&lt;"));
        assert_eq!(evaluate("<r><v>TRUE</v></r>", "//v"), Value::Bool(true));
        assert_eq!(evaluate("<r><v>1.5</v></r>", "//v"), Value::Number(1.5));
        assert_eq!(evaluate("<r><v>1,5</v></r>", "//v"), Value::from("1,5"));
        assert_eq!(evaluate("<r><v>2026-01-02</v></r>", "//v"), Value::Number(46_024.0));
        assert_eq!(evaluate("<r><v a='x'>1</v><v a='y'>2</v></r>", "//v[@a='y']"), Value::Number(2.0));
        assert_eq!(evaluate("<r><v>1</v></r>", "/r/v"), Value::Number(1.0));
        assert_eq!(evaluate("<r>1</r>", "//r"), Value::Number(1.0));
        assert_eq!(evaluate("<r><a><a><b>value</b></a></a></r>", "//a//b"), Value::from("value"));
        assert_eq!(
            evaluate("<r><a><v>1</v><b><v>2</v></b><v>3</v></a></r>", "//v"),
            Value::from(Array::column(vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]))
        );
    }

    #[test]
    fn filterxml_rejects_unsupported_or_empty_selections() {
        for (xml, xpath) in [
            ("<r><v>1</v></r>", "//missing"),
            ("<r><v/></r>", "//v"),
            ("<r><v>", "//v"),
            ("", "//v"),
            ("<r><v>1</v></r>", ""),
            ("<r><v>1</v></r>", "count(//v)"),
            ("<r><v>1</v></r>", "//v/text()"),
            ("<r a='1'/>", "/@a"),
            ("<r a='1'/>", "//@a"),
            ("<r><v a='1'/></r>", "/r//@a"),
            ("<r><v><w>1</w></v></r>", "//v"),
            ("<r>&#1;</r>", "/r"),
        ] {
            assert_eq!(evaluate(xml, xpath), Value::Error(CellError::Value));
        }
    }

    #[test]
    fn filterxml_limits_xml_by_character_count() {
        let xml = format!("<r>{}</r>", "é".repeat(MAX_TEXT - 7));
        assert_eq!(xml.chars().count(), MAX_TEXT);
        assert!(xml.len() > MAX_TEXT);
        assert_eq!(evaluate(&xml, "/r"), Value::from("é".repeat(MAX_TEXT - 7)));
    }
}
