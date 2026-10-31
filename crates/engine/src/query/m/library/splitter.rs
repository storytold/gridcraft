//! Splitters, combiners, replacers and comparers: the functions table steps take as arguments.

use super::super::eval::{Ev, MAX_LIST, limit};
use super::super::value::{MError, MResult, MValue, equal, sort_order};
use super::text::replace_text;
use super::{Args, cant, folded, long_text};

pub fn call(ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "Splitter.SplitTextByDelimiter"
        | "Splitter.SplitTextByEachDelimiter"
        | "Splitter.SplitTextByPositions"
        | "Splitter.SplitTextByLengths"
        | "Splitter.SplitByNothing"
        | "Combiner.CombineTextByDelimiter" => MValue::bound(&format!("{name}#apply"), a.v.clone()),
        "Splitter.SplitTextByDelimiter#apply" => split_apply(a, Split::Delimiter)?,
        "Splitter.SplitTextByEachDelimiter#apply" => split_apply(a, Split::Each)?,
        "Splitter.SplitTextByPositions#apply" => split_apply(a, Split::Positions)?,
        "Splitter.SplitTextByLengths#apply" => split_apply(a, Split::Lengths)?,
        "Splitter.SplitByNothing#apply" => MValue::list(vec![a.v.last().cloned().unwrap_or(MValue::Null)]),
        "Combiner.CombineTextByDelimiter#apply" => {
            let delim = a.text(0)?;
            let MValue::List(l) = a.v.last().cloned().unwrap_or(MValue::Null) else {
                return Err(cant(a.v.last().unwrap_or(&MValue::Null), "List"));
            };
            let mut parts = Vec::with_capacity(l.len());
            for v in l.iter() {
                match ev.force(v.clone())? {
                    MValue::Null => parts.push(String::new()),
                    MValue::Text(t) => parts.push(t.to_string()),
                    other => return Err(cant(&other, "Text")),
                }
            }
            long_text(parts.join(&delim))?
        }
        "Replacer.ReplaceText" => replace_text(a.at(0), a.at(1), a.at(2))?,
        "Replacer.ReplaceValue" => {
            if equal(a.at(0), a.at(1)) {
                a.at(2).clone()
            } else {
                a.at(0).clone()
            }
        }
        "Comparer.Ordinal" | "Comparer.OrdinalIgnoreCase" => {
            let fold = name == "Comparer.OrdinalIgnoreCase";
            let o = sort_order(&folded(a.at(0), fold), &folded(a.at(1), fold));
            MValue::Number(o as i8 as f64)
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}

#[derive(Clone, Copy)]
pub enum Split {
    Delimiter,
    Each,
    Positions,
    Lengths,
}

/// A splitter applied to its input (the last argument; the splitter's settings come first).
pub fn split_apply(a: &Args<'_>, kind: Split) -> MResult<MValue> {
    let input = a.v.last().cloned().unwrap_or(MValue::Null);
    let cfg = Args { name: a.name, v: a.v.get(..a.v.len().saturating_sub(1)).unwrap_or(&[]).to_vec() };
    let text = match &input {
        MValue::Null => return Ok(MValue::list(vec![MValue::Null])),
        MValue::Text(t) => t.clone(),
        other => return Err(cant(other, "Text")),
    };
    let parts: Vec<String> = match kind {
        Split::Delimiter => {
            let d = cfg.text(0)?;
            if d.is_empty() {
                return Err(MError::expr("The delimiter can't be empty."));
            }
            let csv = matches!(cfg.at(1), MValue::Number(n) if *n == 1.0);
            if csv { split_quoted(&text, &d) } else { text.split(&*d).map(str::to_string).collect() }
        }
        Split::Each => {
            let delims = super::names_of(cfg.at(0))?;
            let from_end = matches!(cfg.at(2), MValue::Logical(true));
            let mut parts: Vec<String> = Vec::new();
            let mut rest = text.to_string();
            for d in delims.iter().filter(|d| !d.is_empty()) {
                let at = if from_end { rest.rfind(&**d) } else { rest.find(&**d) };
                let Some(p) = at else { break };
                let (head, tail) = (rest.get(..p).unwrap_or("").to_string(), rest.get(p + d.len()..).unwrap_or("").to_string());
                if from_end {
                    parts.insert(0, tail);
                    rest = head;
                } else {
                    parts.push(head);
                    rest = tail;
                }
            }
            if from_end {
                parts.insert(0, rest);
            } else {
                parts.push(rest);
            }
            parts
        }
        Split::Positions | Split::Lengths => {
            let l = cfg.list(0)?;
            let mut nums = Vec::with_capacity(l.len());
            for x in l.iter() {
                match x {
                    MValue::Number(n) if *n >= 0.0 && n.fract() == 0.0 => nums.push(*n as usize),
                    other => return Err(cant(other, "Number")),
                }
            }
            let u: Vec<u16> = text.encode_utf16().collect();
            let mut cuts: Vec<(usize, usize)> = Vec::new();
            match kind {
                Split::Positions => {
                    for (i, p) in nums.iter().enumerate() {
                        let end = nums.get(i + 1).copied().unwrap_or(u.len());
                        cuts.push(((*p).min(u.len()), end.min(u.len()).max((*p).min(u.len()))));
                    }
                }
                _ => {
                    let mut at = 0usize;
                    for n in &nums {
                        let end = at.saturating_add(*n).min(u.len());
                        cuts.push((at, end));
                        at = end;
                    }
                }
            }
            cuts.into_iter().map(|(s, e)| String::from_utf16_lossy(u.get(s..e).unwrap_or(&[]))).collect()
        }
    };
    if parts.len() > MAX_LIST {
        return Err(limit("The text splits into too many parts."));
    }
    Ok(MValue::list(parts.into_iter().map(MValue::text).collect()))
}

/// Splits CSV-style: a field starting with a quote runs to its closing quote (`""` is a quote).
pub fn split_quoted(text: &str, d: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut rest = text;
    let mut field_start = true;
    let mut quoted = false;
    while !rest.is_empty() {
        if quoted {
            if let Some(r) = rest.strip_prefix("\"\"") {
                cur.push('"');
                rest = r;
            } else if let Some(r) = rest.strip_prefix('"') {
                quoted = false;
                rest = r;
            } else {
                let mut cs = rest.chars();
                if let Some(c) = cs.next() {
                    cur.push(c);
                }
                rest = cs.as_str();
            }
            continue;
        }
        if field_start && let Some(r) = rest.strip_prefix('"') {
            quoted = true;
            field_start = false;
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix(d) {
            out.push(std::mem::take(&mut cur));
            field_start = true;
            rest = r;
            continue;
        }
        let mut cs = rest.chars();
        if let Some(c) = cs.next() {
            cur.push(c);
        }
        rest = cs.as_str();
        field_start = false;
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_splitting() {
        assert_eq!(split_quoted("a,\"b,c\",\"d\"\"e\"", ","), vec!["a", "b,c", "d\"e"]);
    }
}
