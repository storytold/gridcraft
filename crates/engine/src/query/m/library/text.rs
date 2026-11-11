//! `Text.*`. Positions count UTF-16 code units, as M does.

use std::rc::Rc;

use super::super::eval::{Ev, limit};
use super::super::value::{MError, MResult, MValue};
use super::{Args, cant, is_ignore_case, long_text};

pub fn call(ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "Text.Upper" => map_text(a.at(0), |s| s.to_uppercase())?,
        "Text.Lower" => map_text(a.at(0), |s| s.to_lowercase())?,
        "Text.Proper" => map_text(a.at(0), proper)?,
        "Text.Trim" => {
            let chars: Option<Vec<char>> = match a.at(1) {
                MValue::Null => None,
                MValue::Text(t) => Some(t.chars().collect()),
                MValue::List(l) => Some(l.iter().filter_map(|x| if let MValue::Text(t) = x { t.chars().next() } else { None }).collect()),
                other => return Err(cant(other, "Text")),
            };
            map_text(a.at(0), |s| match &chars {
                None => s.trim().to_string(),
                Some(cs) => s.trim_matches(|c| cs.contains(&c)).to_string(),
            })?
        }
        "Text.Clean" => map_text(a.at(0), |s| s.chars().filter(|c| !c.is_control()).collect())?,
        "Text.Contains" | "Text.StartsWith" | "Text.EndsWith" => {
            let (MValue::Text(t), sub) = (a.at(0), a.text(1)?) else {
                return if a.at(0).is_null() { Ok(Some(MValue::Null)) } else { Err(cant(a.at(0), "Text")) };
            };
            let fold = a.has(2) && is_ignore_case(a.at(2));
            let (t, sub) = if fold { (t.to_lowercase(), sub.to_lowercase()) } else { (t.to_string(), sub.to_string()) };
            MValue::Logical(match name {
                "Text.Contains" => t.contains(&sub),
                "Text.StartsWith" => t.starts_with(&sub),
                _ => t.ends_with(&sub),
            })
        }
        "Text.Length" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => MValue::Number(t.encode_utf16().count() as f64),
            other => return Err(cant(other, "Text")),
        },
        "Text.Replace" => match a.at(0) {
            MValue::Null => MValue::Null,
            v => replace_text(v, &MValue::Text(a.text(1)?), a.at(2))?,
        },
        "Text.Combine" => {
            let l = a.list(0)?;
            let sep = a.opt_text(1)?.unwrap_or_else(|| Rc::from(""));
            let mut parts = Vec::with_capacity(l.len());
            for x in l.iter() {
                match ev.force(x.clone())? {
                    MValue::Null => {}
                    MValue::Text(t) => parts.push(t),
                    other => return Err(cant(&other, "Text")),
                }
            }
            long_text(parts.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(&sep))?
        }
        "Text.Split" => {
            let t = a.text(0)?;
            let sep = a.text(1)?;
            if sep.is_empty() {
                MValue::list(t.chars().map(|c| MValue::text(c.to_string())).collect())
            } else {
                MValue::list(t.split(&*sep).map(MValue::text).collect())
            }
        }
        "Text.Start" | "Text.End" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let u: Vec<u16> = t.encode_utf16().collect();
                let n = a.count(1)?.min(u.len());
                let slice = if name == "Text.Start" { u.get(..n) } else { u.get(u.len() - n..) };
                MValue::text(String::from_utf16_lossy(slice.unwrap_or(&[])))
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.Middle" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let u: Vec<u16> = t.encode_utf16().collect();
                let start = a.count(1)?.min(u.len());
                let count = if a.has(2) { a.count(2)? } else { usize::MAX };
                let end = start.saturating_add(count).min(u.len());
                MValue::text(String::from_utf16_lossy(u.get(start..end).unwrap_or(&[])))
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.BeforeDelimiter" | "Text.AfterDelimiter" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let d = a.text(1)?;
                let (n, from_end) = delimiter_index(a.at(2))?;
                match find_nth(t, &d, n, from_end) {
                    Some(p) if name == "Text.BeforeDelimiter" => MValue::text(t.get(..p).unwrap_or("")),
                    Some(p) => MValue::text(t.get(p + d.len()..).unwrap_or("")),
                    None if name == "Text.BeforeDelimiter" => MValue::Text(t.clone()),
                    None => MValue::text(""),
                }
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.BetweenDelimiters" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let (sd, ed) = (a.text(1)?, a.text(2)?);
                let (sn, s_end) = delimiter_index(a.at(3))?;
                let (en, _) = delimiter_index(a.at(4))?;
                match find_nth(t, &sd, sn, s_end) {
                    None => MValue::text(""),
                    Some(p) => {
                        let rest = t.get(p + sd.len()..).unwrap_or("");
                        match find_nth(rest, &ed, en, false) {
                            Some(q) => MValue::text(rest.get(..q).unwrap_or("")),
                            None => MValue::text(rest),
                        }
                    }
                }
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.TrimStart" | "Text.TrimEnd" => {
            let chars = trim_chars(a.at(1))?;
            let start = name == "Text.TrimStart";
            map_text(a.at(0), |s| {
                let keep = |c: char| match &chars {
                    None => !c.is_whitespace(),
                    Some(cs) => !cs.contains(&c),
                };
                if start { s.trim_start_matches(|c| !keep(c)).to_string() } else { s.trim_end_matches(|c| !keep(c)).to_string() }
            })?
        }
        "Text.PadStart" | "Text.PadEnd" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let width = a.count(1)?;
                let pad = match a.at(2) {
                    MValue::Null => ' ',
                    MValue::Text(c) => {
                        let mut cs = c.chars();
                        match (cs.next(), cs.next()) {
                            (Some(c), None) => c,
                            _ => return Err(MError::expr("The padding must be a single character.")),
                        }
                    }
                    other => return Err(cant(other, "Text")),
                };
                let len = t.encode_utf16().count();
                if width <= len {
                    MValue::Text(t.clone())
                } else {
                    if width > super::super::MAX_TEXT {
                        return Err(limit("The text is too long."));
                    }
                    let fill: String = std::iter::repeat_n(pad, width - len).collect();
                    MValue::text(if name == "Text.PadStart" { format!("{fill}{t}") } else { format!("{t}{fill}") })
                }
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.Range" | "Text.RemoveRange" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let u = units(t);
                let offset = a.count(1)?;
                let count = if a.has(2) {
                    a.count(2)?
                } else if name == "Text.Range" {
                    u.len().saturating_sub(offset)
                } else {
                    1
                };
                let end = in_range(u.len(), offset, count)?;
                if name == "Text.Range" {
                    MValue::text(from_units(u.get(offset..end).unwrap_or(&[])))
                } else {
                    let mut out = u.get(..offset).unwrap_or(&[]).to_vec();
                    out.extend_from_slice(u.get(end..).unwrap_or(&[]));
                    MValue::text(from_units(&out))
                }
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.Insert" | "Text.ReplaceRange" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let u = units(t);
                let offset = a.count(1)?;
                let (count, new) = if name == "Text.Insert" { (0, a.text(2)?) } else { (a.count(2)?, a.text(3)?) };
                let end = in_range(u.len(), offset, count)?;
                let mut out = u.get(..offset).unwrap_or(&[]).to_vec();
                out.extend(new.encode_utf16());
                out.extend_from_slice(u.get(end..).unwrap_or(&[]));
                long_text(from_units(&out))?
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.Remove" | "Text.Select" => {
            let set = char_set(a.at(1))?;
            let keep = name == "Text.Select";
            map_text(a.at(0), |s| s.chars().filter(|c| set.contains(c) == keep).collect())?
        }
        "Text.PositionOf" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let sub = a.text(1)?;
                let fold = a.has(3) && is_ignore_case(a.at(3));
                let (hay, needle) = if fold { (t.to_lowercase(), sub.to_lowercase()) } else { (t.to_string(), sub.to_string()) };
                let hits: Vec<f64> = if needle.is_empty() {
                    vec![0.0]
                } else {
                    hay.match_indices(&needle).map(|(i, _)| hay.get(..i).map(|p| p.encode_utf16().count()).unwrap_or(0) as f64).collect()
                };
                occurrence(&hits, a.at(2))?
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.PositionOfAny" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let set = char_set(a.at(1))?;
                let mut hits = Vec::new();
                let mut at = 0usize;
                for c in t.chars() {
                    if set.contains(&c) {
                        hits.push(at as f64);
                    }
                    at += c.len_utf16();
                }
                occurrence(&hits, a.at(2))?
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.Repeat" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => {
                let n = a.count(1)?;
                if t.len().saturating_mul(n) > super::super::MAX_TEXT {
                    return Err(limit("The text is too long."));
                }
                MValue::text(t.repeat(n))
            }
            other => return Err(cant(other, "Text")),
        },
        "Text.Reverse" => map_text(a.at(0), |s| s.chars().rev().collect())?,
        "Text.At" => {
            let u = units(&a.text(0)?);
            let i = a.count(1)?;
            match u.get(i) {
                Some(c) => MValue::text(String::from_utf16_lossy(&[*c])),
                None => return Err(MError::expr("The 'index' argument is out of range.")),
            }
        }
        "Text.ToList" => MValue::list(a.text(0)?.chars().map(|c| MValue::text(c.to_string())).collect()),
        "Text.SplitAny" => {
            let t = a.text(0)?;
            let set: Vec<char> = a.text(1)?.chars().collect();
            MValue::list(t.split(|c| set.contains(&c)).map(MValue::text).collect())
        }
        "Text.Format" => {
            let f = a.text(0)?;
            let culture = a.opt_text(2)?;
            long_text(format_text(ev, &f, a.at(1), culture.as_deref())?)?
        }
        "Character.FromNumber" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Number(n) => match (n.fract() == 0.0 && *n >= 0.0).then(|| char::from_u32(*n as u32)).flatten() {
                Some(c) => MValue::text(c.to_string()),
                None => return Err(MError::expr(format!("{} isn't a character code.", super::show(a.at(0))))),
            },
            other => return Err(cant(other, "Number")),
        },
        "Character.ToNumber" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(t) => match t.chars().next() {
                Some(c) if t.chars().count() == 1 => MValue::Number(f64::from(u32::from(c))),
                _ => return Err(MError::expr("Character.ToNumber needs a single character.")),
            },
            other => return Err(cant(other, "Text")),
        },
        _ => return Ok(None),
    };
    Ok(Some(r))
}

pub fn map_text(v: &MValue, f: impl Fn(&str) -> String) -> MResult<MValue> {
    match v {
        MValue::Null => Ok(MValue::Null),
        MValue::Text(t) => Ok(MValue::text(f(t))),
        other => Err(cant(other, "Text")),
    }
}

pub fn proper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphabetic() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = !c.is_alphanumeric() && c != '\'';
        }
    }
    out
}

/// `Replacer.ReplaceText`: text inside text values; other values unchanged.
pub fn replace_text(v: &MValue, old: &MValue, new: &MValue) -> MResult<MValue> {
    let MValue::Text(t) = v else { return Ok(v.clone()) };
    let MValue::Text(o) = old else { return Err(cant(old, "Text")) };
    let n = match new {
        MValue::Null => Rc::from(""),
        MValue::Text(n) => n.clone(),
        other => return Err(cant(other, "Text")),
    };
    if o.is_empty() {
        return Ok(v.clone());
    }
    let count = t.matches(&**o).count();
    if t.len().saturating_add(count.saturating_mul(n.len())) > super::super::MAX_TEXT {
        return Err(limit("The text is too long."));
    }
    Ok(MValue::text(t.replace(&**o, &n)))
}

/// A delimiter index argument: a number, or `{n, RelativePosition.FromEnd}`.
pub fn delimiter_index(v: &MValue) -> MResult<(usize, bool)> {
    let n = |x: &MValue| match x {
        MValue::Number(n) if *n >= 0.0 && n.fract() == 0.0 => Ok(*n as usize),
        other => Err(cant(other, "Number")),
    };
    match v {
        MValue::Null => Ok((0, false)),
        MValue::Number(_) => Ok((n(v)?, false)),
        MValue::List(l) => {
            let i = l.first().map(n).transpose()?.unwrap_or(0);
            let from_end = matches!(l.get(1), Some(MValue::Number(x)) if *x == 1.0);
            Ok((i, from_end))
        }
        other => Err(cant(other, "Number")),
    }
}

/// The byte offset of the `n`th (from 0) occurrence of `d` in `t`, from the start or the end.
pub fn find_nth(t: &str, d: &str, n: usize, from_end: bool) -> Option<usize> {
    if d.is_empty() {
        return None;
    }
    if from_end { t.rmatch_indices(d).nth(n).map(|(i, _)| i) } else { t.match_indices(d).nth(n).map(|(i, _)| i) }
}

/// A text's UTF-16 code units (M's text positions).
pub fn units(t: &str) -> Vec<u16> {
    t.encode_utf16().collect()
}

pub fn from_units(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

/// The end of `count` units from `offset` in a text of `len` units; out of range is an error, as
/// in M.
fn in_range(len: usize, offset: usize, count: usize) -> MResult<usize> {
    if offset > len {
        return Err(MError::expr("The 'offset' argument is out of range."));
    }
    match offset.checked_add(count).filter(|e| *e <= len) {
        Some(e) => Ok(e),
        None => Err(MError::expr("The 'count' argument is out of range.")),
    }
}

/// `Text.Trim`'s characters: whitespace (`None`), a text's characters, or a list of characters.
fn trim_chars(v: &MValue) -> MResult<Option<Vec<char>>> {
    match v {
        MValue::Null => Ok(None),
        _ => char_set(v).map(Some),
    }
}

/// Characters given as a text or a list of one-character texts.
fn char_set(v: &MValue) -> MResult<Vec<char>> {
    match v {
        MValue::Text(t) => Ok(t.chars().collect()),
        MValue::List(l) => {
            let mut out = Vec::with_capacity(l.len());
            for x in l.iter() {
                match x {
                    MValue::Text(t) => out.extend(t.chars()),
                    other => return Err(cant(other, "Text")),
                }
            }
            Ok(out)
        }
        other => Err(cant(other, "List")),
    }
}

/// The first (default), last or all of the positions, by `Occurrence.First/Last/All`; -1 when none.
fn occurrence(hits: &[f64], occ: &MValue) -> MResult<MValue> {
    Ok(match occ {
        MValue::Null => MValue::Number(hits.first().copied().unwrap_or(-1.0)),
        MValue::Number(n) if *n == 0.0 => MValue::Number(hits.first().copied().unwrap_or(-1.0)),
        MValue::Number(n) if *n == 1.0 => MValue::Number(hits.last().copied().unwrap_or(-1.0)),
        MValue::Number(n) if *n == 2.0 => MValue::list(hits.iter().map(|h| MValue::Number(*h)).collect()),
        other => return Err(cant(other, "Occurrence")),
    })
}

/// `Text.Format`: `#{0}` takes an item of a list, `#[name]` a field of a record.
fn format_text(ev: &mut Ev<'_, '_>, f: &str, args: &MValue, culture: Option<&str>) -> MResult<String> {
    let mut out = String::with_capacity(f.len());
    let mut rest = f;
    while let Some(i) = rest.find('#') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let after = rest.get(i + 1..).unwrap_or("");
        let (open, close) = match after.chars().next() {
            Some('{') => ('{', '}'),
            Some('[') => ('[', ']'),
            _ => {
                out.push('#');
                rest = after;
                continue;
            }
        };
        let Some(end) = after.find(close) else {
            out.push('#');
            rest = after;
            continue;
        };
        let key = after.get(1..end).unwrap_or("");
        let v = match (open, args) {
            ('{', MValue::List(l)) => {
                let i: usize = key.trim().parse().map_err(|_| MError::expr(format!("'{key}' isn't a list index.")))?;
                l.get(i).cloned().ok_or_else(|| MError::expr(format!("The format refers to item {i}, but only {} were given.", l.len())))?
            }
            ('[', MValue::Record(r)) => r.get(key).cloned().ok_or_else(|| MError::expr(format!("The field '{key}' of the record wasn't found.")))?,
            _ => return Err(cant(args, if open == '{' { "List" } else { "Record" })),
        };
        let v = ev.force(v)?;
        match v {
            MValue::Null => {}
            v => out.push_str(&super::convert::text_of(&v, culture).ok_or_else(|| cant(&v, "Text"))?),
        }
        rest = after.get(end + 1..).unwrap_or("");
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_helpers() {
        assert_eq!(proper("hello WORLD o'neil"), "Hello World O'neil");
        assert_eq!(find_nth("a-b-c", "-", 1, false), Some(3));
        assert_eq!(find_nth("a-b-c", "-", 0, true), Some(3));
    }
}
