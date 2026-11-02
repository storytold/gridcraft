//! The bounded JSONPath subset FILTERJSON evaluates.
//!
//! A path is an optional `$`, then any of `.name`, `['name']` or `["name"]`, `[n]`, `[*]` and
//! `..name`. Nothing else: filters, slices, unions and script expressions are rejected rather
//! than half-ignored, so a path that parses means exactly what it says.

use crate::json::Json;

#[derive(Debug, PartialEq)]
pub(crate) enum Seg {
    /// `.name` or `['name']`
    Key(String),
    /// `[n]`
    Index(usize),
    /// `[*]`
    Any,
    /// `..name`, every matching member at any depth
    Deep(String),
}

/// Reads a path. `None` when it is not this subset.
pub(crate) fn parse(path: &str) -> Option<Vec<Seg>> {
    let mut segs = Vec::new();
    // `$` is optional. Without it the path opens with a bare name, as in `valor` or `lista[0]`.
    let mut rest = match path.strip_prefix('$') {
        Some(rest) => rest,
        None => {
            let (first, rest) = name(path)?;
            segs.push(Seg::Key(first));
            rest
        }
    };
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("..") {
            let (name, tail) = name(after)?;
            segs.push(Seg::Deep(name));
            rest = tail;
        } else if let Some(after) = rest.strip_prefix('.') {
            let (name, tail) = name(after)?;
            segs.push(Seg::Key(name));
            rest = tail;
        } else {
            let after = rest.strip_prefix('[')?;
            let (seg, tail) = bracket(after)?;
            segs.push(seg);
            rest = tail;
        }
    }
    Some(segs)
}

/// An unquoted name: letters, digits, `_` and `-`, at least one character.
fn name(s: &str) -> Option<(String, &str)> {
    let end = s.char_indices().find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-')).map_or(s.len(), |(i, _)| i);
    if end == 0 {
        return None;
    }
    Some((s.get(..end)?.to_string(), s.get(end..)?))
}

/// The inside of `[...]`, after the opening bracket.
fn bracket(s: &str) -> Option<(Seg, &str)> {
    if let Some(tail) = s.strip_prefix('*') {
        return Some((Seg::Any, tail.strip_prefix(']')?));
    }
    if let Some(inner) = s.strip_prefix('\'').or_else(|| s.strip_prefix('"')) {
        let quote = s.chars().next()?;
        let end = inner.find(quote)?;
        let key = inner.get(..end)?.to_string();
        let tail = inner.get(end + 1..)?.strip_prefix(']')?;
        return Some((Seg::Key(key), tail));
    }
    let end = s.find(']')?;
    let digits = s.get(..end)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let index: usize = digits.parse().ok()?;
    Some((Seg::Index(index), s.get(end + 1..)?))
}

/// Hard cap on the number of nodes `select` may walk, independent of how many matches it
/// produces. A path of repeated `..name` segments can visit the same subtree many times.
const WORK_BUDGET: usize = 1_000_000;

/// Every value the path selects, in document order.
///
/// `limit` caps the number of intermediate matches and `WORK_BUDGET` caps the traversal work,
/// so a path such as a long chain of `..name` segments cannot grow without bound on a deeply
/// nested document. Returns `None` when either cap is exceeded.
pub(crate) fn select<'a>(doc: &'a Json, path: &[Seg], limit: usize) -> Option<Vec<&'a Json>> {
    let mut current: Vec<&Json> = vec![doc];
    let mut work: usize = 0;
    for seg in path {
        let mut next: Vec<&Json> = Vec::new();
        for node in &current {
            match seg {
                Seg::Key(key) => {
                    work = work.saturating_add(1);
                    if work > WORK_BUDGET {
                        return None;
                    }
                    if let Some(v) = node.member(key) {
                        next.push(v);
                    }
                }
                Seg::Index(i) => {
                    work = work.saturating_add(1);
                    if work > WORK_BUDGET {
                        return None;
                    }
                    if let Some(v) = node.items().get(*i) {
                        next.push(v);
                    }
                }
                Seg::Any => {
                    work = work.saturating_add(1 + node.items().len());
                    if work > WORK_BUDGET {
                        return None;
                    }
                    next.extend(node.items());
                }
                Seg::Deep(key) => {
                    if descend(node, key, &mut next, &mut work, WORK_BUDGET, limit) {
                        return None;
                    }
                }
            }
            if next.len() > limit {
                return None;
            }
        }
        if next.is_empty() {
            return Some(next);
        }
        current = next;
    }
    Some(current)
}

/// `..key`: the member here, then the same search inside every child. Nesting is bounded by the
/// reader's depth cap. Returns `true` when the traversal or match budget is exhausted.
fn descend<'a>(
    node: &'a Json,
    key: &str,
    out: &mut Vec<&'a Json>,
    work: &mut usize,
    budget: usize,
    limit: usize,
) -> bool {
    *work = work.saturating_add(1);
    if *work > budget {
        return true;
    }
    if let Some(v) = node.member(key) {
        out.push(v);
        if out.len() > limit {
            return true;
        }
    }
    match node {
        Json::Object(items) => {
            for (_, v) in items {
                if descend(v, key, out, work, budget, limit) {
                    return true;
                }
                if out.len() > limit || *work > budget {
                    return true;
                }
            }
        }
        Json::Array(items) => {
            for v in items {
                if descend(v, key, out, work, budget, limit) {
                    return true;
                }
                if out.len() > limit || *work > budget {
                    return true;
                }
            }
        }
        Json::Null | Json::Bool(_) | Json::Number(_) | Json::Text(_) => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    /// parse_ok.
    fn parse_ok(p: &str) -> Vec<Seg> {
        parse(p).unwrap_or_default()
    }

    #[test]
    /// parses_the_subset.
    fn parses_the_subset() {
        assert_eq!(parse_ok("$.valor"), vec![Seg::Key("valor".to_string())]);
        assert_eq!(parse_ok("valor"), vec![Seg::Key("valor".to_string())]);
        assert_eq!(parse_ok("$['valor']"), vec![Seg::Key("valor".to_string())]);
        assert_eq!(parse_ok("$[\"con espacio\"]"), vec![Seg::Key("con espacio".to_string())]);
        assert_eq!(parse_ok("$.a.b"), vec![Seg::Key("a".to_string()), Seg::Key("b".to_string())]);
        assert_eq!(parse_ok("$.lista[0]"), vec![Seg::Key("lista".to_string()), Seg::Index(0)]);
        assert_eq!(parse_ok("$.lista[*]"), vec![Seg::Key("lista".to_string()), Seg::Any]);
        assert_eq!(parse_ok("$..id"), vec![Seg::Deep("id".to_string())]);
        assert_eq!(parse_ok("$"), Vec::new());
    }

    #[test]
    /// rejects_everything_else.
    fn rejects_everything_else() {
        for bad in ["", ".", "..", "$.", "a..", "$[", "$[]", "$[a]", "$[-1]", "$[1.5]", "$[?(@.a)]", "$.a[*", "$.a()", "[:2]"] {
            assert_eq!(parse(bad), None, "debió rechazar {bad:?}");
        }
    }

    #[test]
    /// selects_values.
    fn selects_values() {
        let src = r#"{"valor":"3166.91","anio":2026,"ok":true,"nulo":null,
                      "lista":[{"id":1},{"id":2}],"a":{"b":{"id":3}}}"#;
        let Some(doc) = json::parse(src) else { return };
        let one = |p: &str| -> Option<String> { select(&doc, &parse_ok(p), 100_000).unwrap().first().map(|j| format!("{j:?}")) };
        assert_eq!(one("$.valor"), Some(r#"Text("3166.91")"#.to_string()));
        assert_eq!(one("anio"), Some("Number(2026.0)".to_string()));
        assert_eq!(one("$.nulo"), Some("Null".to_string()));
        assert_eq!(one("$.a.b.id"), Some("Number(3.0)".to_string()));
        assert_eq!(one("$.lista[1].id"), Some("Number(2.0)".to_string()));
        assert_eq!(one("$.nada"), None);
        assert_eq!(select(&doc, &parse_ok("$.lista[*]"), 100_000).unwrap().len(), 2);
        assert_eq!(select(&doc, &parse_ok("$..id"), 100_000).unwrap().len(), 3);
    }

    #[test]
    /// deep_chain_stops_at_the_match_cap.
    fn deep_chain_stops_at_the_match_cap() {
        // A chain of nested "a" objects is small on disk but a repeated `..a` path would
        // otherwise multiply the match count without bound.
        let mut src = String::from("{\"a\":");
        for _ in 0..200 {
            src.push_str("{\"a\":");
        }
        src.push('1');
        for _ in 0..=200 {
            src.push('}');
        }
        let Some(doc) = json::parse(&src) else { return };
        let path = "$..a..a..a..a..a";
        let segs = parse(path).unwrap();
        assert!(select(&doc, &segs, 100_000).is_none());
    }

    #[test]
    /// fuzz_parse_and_select_never_panics.
    fn fuzz_parse_and_select_never_panics() {
        /// rng.
        fn rng(s: &mut u64) -> u64 {
            *s ^= *s << 13;
            *s ^= *s >> 7;
            *s ^= *s << 17;
            *s
        }
        let src = r#"{"a":{"b":[1,2,3]},"lista":[{"id":1},{"id":2}],"v":null}"#;
        let Some(doc) = json::parse(src) else { return };
        let mut s: u64 = 0xC2B2_AE3D_27D4_EB4F;
        for _ in 0..50_000 {
            // build a random path
            let mut path = String::new();
            if rng(&mut s).is_multiple_of(2) {
                path.push('$');
            }
            for _ in 0..(rng(&mut s) % 4) {
                match rng(&mut s) % 6 {
                    0 => path.push_str(&format!(".k{}", rng(&mut s) % 3)),
                    1 => path.push_str(&format!("[{}]", rng(&mut s) % 3)),
                    2 => path.push_str("[*]"),
                    3 => path.push_str("..id"),
                    4 => path.push_str("['a']"),
                    _ => path.push_str("[\"b\"]"),
                }
            }
            if let Some(segs) = parse(&path) {
                let _ = select(&doc, &segs, 100_000);
            }
        }
    }
}
