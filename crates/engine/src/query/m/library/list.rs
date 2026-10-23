//! `List.*`.

use super::super::eval::{Ev, MAX_LIST, limit};
use super::super::value::{MError, MResult, MValue, equal, order, sort_order};
use super::{Args, cant, folded, folded_key, forced, is_ignore_case, show};

pub fn call(ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "List.Sum" | "List.Average" => {
            let items = forced(ev, &a.list(0)?)?;
            let vals: Vec<&MValue> = items.iter().filter(|v| !v.is_null()).collect();
            if vals.is_empty() {
                MValue::Null
            } else if vals.iter().all(|v| matches!(v, MValue::Number(_))) {
                let s: f64 = vals.iter().map(|v| if let MValue::Number(n) = v { *n } else { 0.0 }).sum();
                MValue::Number(if name == "List.Sum" { s } else { s / vals.len() as f64 })
            } else if vals.iter().all(|v| matches!(v, MValue::Duration(_))) {
                let s: f64 = vals.iter().map(|v| if let MValue::Duration(n) = v { *n } else { 0.0 }).sum();
                MValue::Duration(if name == "List.Sum" { s } else { s / vals.len() as f64 })
            } else {
                let bad = vals.iter().find(|v| !matches!(v, MValue::Number(_))).map(|v| (*v).clone()).unwrap_or(MValue::Null);
                return Err(cant(&bad, "Number"));
            }
        }
        "List.Median" => {
            let items = forced(ev, &a.list(0)?)?;
            let mut nums = Vec::with_capacity(items.len());
            for v in items.iter().filter(|v| !v.is_null()) {
                match v {
                    MValue::Number(n) => nums.push(*n),
                    other => return Err(cant(other, "Number")),
                }
            }
            nums.sort_by(f64::total_cmp);
            let n = nums.len();
            match n {
                0 => MValue::Null,
                _ if n % 2 == 1 => MValue::Number(nums.get(n / 2).copied().unwrap_or(0.0)),
                _ => MValue::Number((nums.get(n / 2 - 1).copied().unwrap_or(0.0) + nums.get(n / 2).copied().unwrap_or(0.0)) / 2.0),
            }
        }
        "List.Min" | "List.Max" => {
            let items = forced(ev, &a.list(0)?)?;
            let mut best: Option<MValue> = None;
            for v in items.into_iter().filter(|v| !v.is_null()) {
                best = Some(match best {
                    None => v,
                    Some(b) => {
                        let o = order(&v, &b).ok_or_else(|| MError::expr(format!("We cannot compare {} and {}.", show(&v), show(&b))))?;
                        if (name == "List.Min" && o.is_lt()) || (name == "List.Max" && o.is_gt()) { v } else { b }
                    }
                });
            }
            best.unwrap_or_else(|| a.at(1).clone())
        }
        "List.Count" => MValue::Number(a.list(0)?.len() as f64),
        "List.NonNullCount" => MValue::Number(forced(ev, &a.list(0)?)?.iter().filter(|v| !v.is_null()).count() as f64),
        "List.Distinct" => {
            let fold = is_ignore_case(a.at(1));
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for v in forced(ev, &a.list(0)?)? {
                if seen.insert(folded_key(&v, fold)) {
                    out.push(v);
                }
            }
            MValue::list(out)
        }
        "List.First" | "List.Last" => {
            let l = a.list(0)?;
            let x = if name == "List.First" { l.first() } else { l.last() };
            match x {
                Some(v) => ev.force(v.clone())?,
                None => a.at(1).clone(),
            }
        }
        "List.Select" => {
            let f = a.func(1)?;
            let mut out = Vec::new();
            for v in a.list(0)?.iter() {
                let v = ev.force(v.clone())?;
                if matches!(ev.call1(&f, v.clone())?, MValue::Logical(true)) {
                    out.push(v);
                }
            }
            MValue::list(out)
        }
        "List.Transform" => {
            let f = a.func(1)?;
            let l = a.list(0)?;
            let mut out = Vec::with_capacity(l.len());
            for v in l.iter() {
                let r = ev.force(v.clone()).and_then(|v| ev.call1(&f, v));
                out.push(super::table::held(r)?);
            }
            MValue::list(out)
        }
        "List.Contains" | "List.PositionOf" => {
            let fold = is_ignore_case(a.at(if name == "List.Contains" { 2 } else { 3 }));
            let want = folded(a.at(1), fold);
            let mut found = None;
            for (i, v) in a.list(0)?.iter().enumerate() {
                let v = ev.force(v.clone())?;
                if equal(&folded(&v, fold), &want) {
                    found = Some(i);
                    if name == "List.Contains" || !matches!(a.at(2), MValue::Number(n) if *n == 1.0) {
                        break;
                    }
                }
            }
            if name == "List.Contains" { MValue::Logical(found.is_some()) } else { MValue::Number(found.map(|i| i as f64).unwrap_or(-1.0)) }
        }
        "List.Combine" => {
            let mut out = Vec::new();
            for x in a.list(0)?.iter() {
                match ev.force(x.clone())? {
                    MValue::List(l) => {
                        if out.len().saturating_add(l.len()) > MAX_LIST {
                            return Err(limit(format!("A list can hold at most {MAX_LIST} items here.")));
                        }
                        out.extend(l.iter().cloned());
                    }
                    other => return Err(cant(&other, "List")),
                }
            }
            MValue::list(out)
        }
        "List.RemoveNulls" => MValue::list(forced(ev, &a.list(0)?)?.into_iter().filter(|v| !v.is_null()).collect()),
        "List.Sort" => {
            let mut items = forced(ev, &a.list(0)?)?;
            let desc = matches!(a.at(1), MValue::Number(n) if *n == 1.0);
            ev.tick(items.len() as u64 * 20)?;
            items.sort_by(|x, y| if desc { sort_order(y, x) } else { sort_order(x, y) });
            MValue::list(items)
        }
        "List.Reverse" => MValue::list(a.list(0)?.iter().rev().cloned().collect()),
        "List.Numbers" => {
            let start = a.num(0)?;
            let count = a.count(1)?;
            let step = if a.has(2) { a.num(2)? } else { 1.0 };
            if count > MAX_LIST {
                return Err(limit(format!("A list can hold at most {MAX_LIST} items here.")));
            }
            ev.tick(count as u64)?;
            MValue::list((0..count).map(|i| MValue::Number(start + i as f64 * step)).collect())
        }
        "List.Buffer" => MValue::List(a.list(0)?),
        "List.IsEmpty" => MValue::Logical(a.list(0)?.is_empty()),
        "List.FirstN" | "List.LastN" | "List.Skip" | "List.RemoveFirstN" | "List.RemoveLastN" => {
            let items = a.list(0)?;
            let from_end = matches!(name, "List.LastN" | "List.RemoveLastN");
            let n = if a.has(1) { list_count(ev, &items, a.at(1), from_end)? } else { 1 };
            let len = items.len();
            MValue::list(match name {
                "List.FirstN" => items.iter().take(n).cloned().collect(),
                "List.LastN" => items.iter().skip(len.saturating_sub(n)).cloned().collect(),
                "List.Skip" | "List.RemoveFirstN" => items.iter().skip(n).cloned().collect(),
                _ => items.iter().take(len.saturating_sub(n)).cloned().collect(),
            })
        }
        "List.Range" => {
            let items = a.list(0)?;
            let offset = a.count(1)?;
            if offset > items.len() {
                return Err(MError::expr("The 'offset' argument is out of range."));
            }
            let count = if a.has(2) { a.count(2)? } else { usize::MAX };
            MValue::list(items.iter().skip(offset).take(count).cloned().collect())
        }
        "List.RemoveItems" => {
            let drop = forced(ev, &a.list(1)?)?;
            let mut out = Vec::new();
            for v in forced(ev, &a.list(0)?)? {
                if !drop.iter().any(|d| equal(d, &v)) {
                    out.push(v);
                }
            }
            MValue::list(out)
        }
        "List.ContainsAny" | "List.ContainsAll" => {
            let fold = is_ignore_case(a.at(2));
            let items: Vec<MValue> = forced(ev, &a.list(0)?)?.iter().map(|v| folded(v, fold)).collect();
            let wanted: Vec<MValue> = forced(ev, &a.list(1)?)?.iter().map(|v| folded(v, fold)).collect();
            let has = |w: &MValue| items.iter().any(|v| equal(v, w));
            MValue::Logical(if name == "List.ContainsAny" { wanted.iter().any(has) } else { wanted.iter().all(has) })
        }
        "List.Product" | "List.StandardDeviation" => {
            let mut nums = Vec::new();
            for v in forced(ev, &a.list(0)?)? {
                match v {
                    MValue::Null => {}
                    MValue::Number(n) => nums.push(n),
                    other => return Err(cant(&other, "Number")),
                }
            }
            match (name, nums.len()) {
                (_, 0) => MValue::Null,
                ("List.Product", _) => MValue::Number(nums.iter().product()),
                (_, 1) => return Err(MError::expr("The standard deviation needs at least two values.")),
                (_, k) => {
                    let mean = nums.iter().sum::<f64>() / k as f64;
                    MValue::Number((nums.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (k as f64 - 1.0)).sqrt())
                }
            }
        }
        "List.Mode" => {
            let items = forced(ev, &a.list(0)?)?;
            let mut counts: Vec<(super::super::value::Key, usize, MValue)> = Vec::new();
            for v in items.into_iter().filter(|v| !v.is_null()) {
                let k = super::super::value::key(&v);
                match counts.iter_mut().find(|(x, _, _)| *x == k) {
                    Some(c) => c.1 += 1,
                    None => counts.push((k, 1, v)),
                }
            }
            let best = counts.iter().map(|(_, n, _)| *n).max().unwrap_or(0);
            match counts.into_iter().find(|(_, n, _)| *n == best) {
                Some((_, _, v)) => v,
                None => return Err(MError::expr("The list is empty.")),
            }
        }
        "List.Union" | "List.Intersect" => {
            let lists: Vec<Vec<MValue>> = {
                let mut out = Vec::new();
                for l in a.list(0)?.iter() {
                    match ev.force(l.clone())? {
                        MValue::List(x) => out.push(forced(ev, &x)?),
                        other => return Err(cant(&other, "List")),
                    }
                }
                out
            };
            MValue::list(bag(&lists, name == "List.Union"))
        }
        "List.Difference" => {
            let mut left = forced(ev, &a.list(0)?)?;
            for r in forced(ev, &a.list(1)?)? {
                if let Some(i) = left.iter().position(|v| equal(v, &r)) {
                    left.remove(i);
                }
            }
            MValue::list(left)
        }
        "List.Positions" => MValue::list((0..a.list(0)?.len()).map(|i| MValue::Number(i as f64)).collect()),
        "List.Zip" => {
            let mut lists = Vec::new();
            for l in a.list(0)?.iter() {
                match ev.force(l.clone())? {
                    MValue::List(x) => lists.push(x),
                    other => return Err(cant(&other, "List")),
                }
            }
            let n = lists.iter().map(|l| l.len()).max().unwrap_or(0);
            MValue::list((0..n).map(|i| MValue::list(lists.iter().map(|l| l.get(i).cloned().unwrap_or(MValue::Null)).collect())).collect())
        }
        "List.Repeat" => {
            let items = a.list(0)?;
            let n = a.count(1)?;
            if items.len().saturating_mul(n) > MAX_LIST {
                return Err(limit(format!("A list can hold at most {MAX_LIST} items here.")));
            }
            MValue::list(items.iter().cloned().cycle().take(items.len() * n).collect())
        }
        "List.Single" => {
            let items = a.list(0)?;
            match (items.len(), items.first()) {
                (1, Some(v)) => ev.force(v.clone())?,
                _ => return Err(MError::expr("There weren't exactly one element in the enumeration.")),
            }
        }
        "List.Split" => {
            let items = a.list(0)?;
            let size = a.count(1)?.max(1);
            MValue::list(items.chunks(size).map(|c| MValue::list(c.to_vec())).collect())
        }
        "List.Dates" => {
            let start = match a.at(0) {
                MValue::Date(d) => *d,
                other => return Err(cant(other, "Date")),
            };
            let count = a.count(1)?;
            let step = match a.at(2) {
                MValue::Duration(d) => d.floor() as i64,
                other => return Err(cant(other, "Duration")),
            };
            if count > MAX_LIST {
                return Err(limit(format!("A list can hold at most {MAX_LIST} items here.")));
            }
            let mut out = Vec::with_capacity(count);
            for i in 0..count as i64 {
                let d = start.saturating_add(step.saturating_mul(i));
                if !(1..=9999).contains(&super::super::value::ymd(d).0) {
                    return Err(MError::expr("The date is out of range."));
                }
                out.push(MValue::Date(d));
            }
            MValue::list(out)
        }
        "List.ReplaceValue" => {
            let (old, new, replacer) = (a.at(1).clone(), a.at(2).clone(), a.func(3)?);
            let mut out = Vec::new();
            for v in forced(ev, &a.list(0)?)? {
                out.push(ev.call(&replacer, vec![v, old.clone(), new.clone()])?);
            }
            MValue::list(out)
        }
        "List.Accumulate" => {
            let f = a.func(2)?;
            let mut state = a.at(1).clone();
            for v in a.list(0)?.iter() {
                let v = ev.force(v.clone())?;
                state = ev.call(&f, vec![state, v])?;
            }
            state
        }
        "List.Generate" => {
            let (init, cond, next) = (a.func(0)?, a.func(1)?, a.func(2)?);
            let selector = if a.has(3) { Some(a.func(3)?) } else { None };
            let mut out = Vec::new();
            let mut x = ev.call(&init, vec![])?;
            loop {
                match ev.call1(&cond, x.clone())? {
                    MValue::Logical(true) => {}
                    MValue::Logical(false) | MValue::Null => break,
                    other => return Err(cant(&other, "Logical")),
                }
                if out.len() >= MAX_LIST {
                    return Err(limit(format!("A list can hold at most {MAX_LIST} items here.")));
                }
                out.push(match &selector {
                    Some(s) => ev.call1(s, x.clone())?,
                    None => x.clone(),
                });
                x = ev.call1(&next, x)?;
            }
            MValue::list(out)
        }
        _ => return Ok(None),
    };
    Ok(Some(r))
}

/// A count, or how many items from the start (or end) meet a condition.
fn list_count(ev: &mut Ev<'_, '_>, items: &[MValue], v: &MValue, from_end: bool) -> MResult<usize> {
    match v {
        MValue::Number(n) if *n >= 0.0 && n.fract() == 0.0 => Ok(if *n > usize::MAX as f64 { usize::MAX } else { *n as usize }),
        MValue::Function(_) => {
            let mut n = 0;
            let it: Box<dyn Iterator<Item = &MValue>> = if from_end { Box::new(items.iter().rev()) } else { Box::new(items.iter()) };
            for x in it {
                let x = ev.force(x.clone())?;
                match ev.call1(v, x)? {
                    MValue::Logical(true) => n += 1,
                    _ => break,
                }
            }
            Ok(n)
        }
        other => Err(cant(other, "Number")),
    }
}

/// Lists combined as bags: the union keeps each value as often as the list with most of it, the
/// intersection as often as the list with fewest.
fn bag(lists: &[Vec<MValue>], union: bool) -> Vec<MValue> {
    let mut out: Vec<MValue> = Vec::new();
    let Some(first) = lists.first() else { return out };
    let count = |l: &[MValue], v: &MValue| l.iter().filter(|x| equal(x, v)).count();
    let candidates: Vec<&MValue> = if union { lists.iter().flatten().collect() } else { first.iter().collect() };
    for v in candidates {
        if out.iter().any(|x| equal(x, v)) {
            continue;
        }
        let n = if union { lists.iter().map(|l| count(l, v)).max().unwrap_or(0) } else { lists.iter().map(|l| count(l, v)).min().unwrap_or(0) };
        out.extend(std::iter::repeat_n(v.clone(), n));
    }
    out
}
