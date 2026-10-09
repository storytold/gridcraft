//! Text functions.

use gridcraft_core::parse::parse_number_text;
use gridcraft_core::{CellError, Value, number_to_text};

use crate::criteria::wildcard_find;
use crate::util::{
    A, MAX_TEXT, R, S, arg, array_val, as_array, boolean, has, num, num_val, opt_bool, opt_int, opt_num, round_half_away, scalar, text, text_val,
};
use crate::{Arg, Ctx, FnSpec, VAR};

const TJ: &[bool] = &[false, true, false];
const TB: &[bool] = &[true, false, true, true, true, false];
const TS: &[bool] = &[true, false, false, true, true, false];
const ATT: &[bool] = &[false, true];

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn check_len(s: String) -> R<Value> {
    if s.len() > MAX_TEXT && s.chars().count() > MAX_TEXT {
        return Err(CellError::Value);
    }
    text_val(s)
}

/// Pushes `piece` onto `out`, failing once the result exceeds the cell limit.
fn push_capped(out: &mut String, piece: &str) -> R<()> {
    out.push_str(piece);
    if out.len() > MAX_TEXT * 4 {
        return Err(CellError::Value);
    }
    Ok(())
}

fn concatenate(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let mut out = String::new();
    for x in a {
        push_capped(&mut out, &scalar(x).to_text()?)?;
    }
    check_len(out)
}

fn concat(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let mut out = String::new();
    for x in a {
        for v in as_array(&x.value).iter() {
            push_capped(&mut out, &v.to_text()?)?;
        }
    }
    check_len(out)
}

fn textjoin(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let delims: Vec<String> = as_array(&arg(a, 0)?.value).iter().map(|v| v.to_text()).collect::<R<_>>()?;
    let ignore = boolean(a, 1)?;
    let mut items = Vec::new();
    for x in a.iter().skip(2) {
        for v in as_array(&x.value).iter() {
            let s = v.to_text()?;
            if ignore && s.is_empty() {
                continue;
            }
            items.push(s);
        }
    }
    let mut out = String::new();
    for (i, s) in items.iter().enumerate() {
        if i > 0 && !delims.is_empty() {
            push_capped(&mut out, &delims[(i - 1) % delims.len()])?;
        }
        push_capped(&mut out, s)?;
    }
    check_len(out)
}

fn count_arg(a: &[Arg], i: usize, default: f64) -> R<usize> {
    let n = opt_num(a, i, default)?;
    if n < 0.0 || n.is_nan() {
        return Err(CellError::Value);
    }
    Ok(if n > 1e9 { 1_000_000_000 } else { n.trunc() as usize })
}

fn left(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let n = count_arg(a, 1, 1.0)?;
    text_val(s.chars().take(n).collect::<String>())
}

fn right(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = chars(&text(a, 0)?);
    let n = count_arg(a, 1, 1.0)?.min(s.len());
    text_val(s[s.len() - n..].iter().collect::<String>())
}

fn mid(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let start = num(a, 1)?;
    if start.is_nan() || start < 1.0 {
        return Err(CellError::Value);
    }
    let n = count_arg(a, 2, 0.0)?;
    let start = if start > 1e9 { 1_000_000_000 } else { start as usize };
    text_val(s.chars().skip(start - 1).take(n).collect::<String>())
}

fn len(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    num_val(text(a, 0)?.chars().count() as f64)
}

fn lower(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    text_val(text(a, 0)?.to_lowercase())
}

fn upper(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    text_val(text(a, 0)?.to_uppercase())
}

fn proper(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let mut out = String::with_capacity(s.len());
    let mut prev_letter = false;
    for ch in s.chars() {
        if ch.is_alphabetic() {
            if prev_letter {
                out.extend(ch.to_lowercase());
            } else {
                out.extend(ch.to_uppercase());
            }
            prev_letter = true;
        } else {
            out.push(ch);
            prev_letter = false;
        }
    }
    text_val(out)
}

fn trim(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    text_val(s.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" "))
}

fn clean(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    text_val(text(a, 0)?.chars().filter(|c| (*c as u32) >= 32).collect::<String>())
}

fn substitute(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let old = text(a, 1)?;
    let new = text(a, 2)?;
    if old.is_empty() {
        return text_val(s);
    }
    if has(a, 3) {
        let inst = num(a, 3)?;
        if inst.is_nan() || inst < 1.0 {
            return Err(CellError::Value);
        }
        let inst = inst.trunc() as usize;
        match s.match_indices(old.as_str()).nth(inst - 1) {
            Some((pos, _)) => {
                let mut out = String::with_capacity(s.len() + new.len());
                out.push_str(&s[..pos]);
                out.push_str(&new);
                out.push_str(&s[pos + old.len()..]);
                check_len(out)
            }
            None => text_val(s),
        }
    } else {
        let count = s.matches(old.as_str()).count();
        if new.len() > old.len() && s.len() + count.saturating_mul(new.len() - old.len()) > MAX_TEXT * 4 {
            return Err(CellError::Value);
        }
        check_len(s.replace(old.as_str(), &new))
    }
}

fn replace(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = chars(&text(a, 0)?);
    let start = num(a, 1)?;
    if start.is_nan() || start < 1.0 {
        return Err(CellError::Value);
    }
    let n = count_arg(a, 2, 0.0)?;
    let new = text(a, 3)?;
    let start = (if start > 1e9 { 1_000_000_000 } else { start as usize } - 1).min(s.len());
    let end = start.saturating_add(n).min(s.len());
    let mut out: String = s[..start].iter().collect();
    out.push_str(&new);
    out.extend(s[end..].iter());
    check_len(out)
}

fn rept(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let n = num(a, 1)?;
    if n.is_nan() || n < 0.0 {
        return Err(CellError::Value);
    }
    let n = n.trunc();
    let total = s.chars().count() as f64 * n;
    if total > MAX_TEXT as f64 {
        return Err(CellError::Value);
    }
    text_val(s.repeat(n as usize))
}

fn find_impl(a: &[Arg], wild: bool) -> R<Value> {
    let needle = text(a, 0)?;
    let hay = text(a, 1)?;
    let start = opt_num(a, 2, 1.0)?;
    let hc = chars(&hay);
    if (start.is_nan() || start < 1.0) || start > hc.len() as f64 + 1.0 {
        return Err(CellError::Value);
    }
    let start = start as usize - 1;
    if needle.is_empty() {
        return num_val((start + 1) as f64);
    }
    let pos = if wild {
        wildcard_find(&needle, &hay, start)
    } else {
        let nc = chars(&needle);
        (start..hc.len()).find(|&i| hc.get(i..i + nc.len()).is_some_and(|w| w == nc.as_slice()))
    };
    match pos {
        Some(p) => num_val((p + 1) as f64),
        None => Err(CellError::Value),
    }
}

fn find(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    find_impl(a, false)
}

fn search(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    find_impl(a, true)
}

fn exact(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    Ok(Value::Bool(text(a, 0)? == text(a, 1)?))
}

/// Windows-1252 code points 128–159.
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

fn char_fn(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let n = num(a, 0)?.trunc();
    if !(1.0..=255.0).contains(&n) {
        return Err(CellError::Value);
    }
    let n = n as u32;
    let cp = if (128..160).contains(&n) { CP1252[(n - 128) as usize] } else { n };
    text_val(char::from_u32(cp).map(String::from).unwrap_or_default())
}

fn code(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let ch = s.chars().next().ok_or(CellError::Value)?;
    let cp = ch as u32;
    let code = if cp < 128 || (160..256).contains(&cp) {
        cp
    } else if let Some(i) = CP1252.iter().position(|&x| x == cp) {
        128 + i as u32
    } else {
        63
    };
    num_val(code as f64)
}

fn unichar(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let n = num(a, 0)?.trunc();
    if !(1.0..=1_114_111.0).contains(&n) {
        return Err(CellError::Value);
    }
    match char::from_u32(n as u32) {
        Some(c) => text_val(c.to_string()),
        None => Err(CellError::NA),
    }
}

fn unicode(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let ch = s.chars().next().ok_or(CellError::Value)?;
    num_val(ch as u32 as f64)
}

fn value(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    match scalar(arg(a, 0)?) {
        Value::Number(n) => num_val(n),
        Value::Empty => num_val(0.0),
        Value::Error(e) => Err(e),
        Value::Text(t) => {
            if t.trim().is_empty() {
                return num_val(0.0);
            }
            parse_number_text(&t).ok_or(CellError::Value).and_then(num_val)
        }
        _ => Err(CellError::Value),
    }
}

fn numbervalue(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let dec = if a.len() > 1 { text(a, 1)?.chars().next().ok_or(CellError::Value)? } else { '.' };
    let grp = if a.len() > 2 { text(a, 2)?.chars().next().ok_or(CellError::Value)? } else { ',' };
    if dec == grp {
        return Err(CellError::Value);
    }
    let mut body: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if body.is_empty() {
        return num_val(0.0);
    }
    let mut pct = 0;
    while let Some(b) = body.strip_suffix('%') {
        body = b.to_string();
        pct += 1;
    }
    let mut seen_dec = false;
    let mut out = String::new();
    for c in body.chars() {
        if c == dec {
            if seen_dec {
                return Err(CellError::Value);
            }
            seen_dec = true;
            out.push('.');
        } else if c == grp {
            if seen_dec {
                return Err(CellError::Value);
            }
        } else {
            out.push(c);
        }
    }
    let mut n = parse_number_text(&out).ok_or(CellError::Value)?;
    for _ in 0..pct {
        n /= 100.0;
    }
    num_val(n)
}

/// Formats `x` (already rounded) with `decimals` places and optional thousands separators.
fn format_fixed(x: f64, decimals: i64, commas: bool) -> String {
    let d = decimals.max(0) as usize;
    let s = format!("{:.*}", d, x.abs());
    let (int_part, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (s.clone(), None),
    };
    let int_part = if commas {
        let b = int_part.as_bytes();
        let mut o = String::new();
        for (i, ch) in b.iter().enumerate() {
            if i > 0 && (b.len() - i) % 3 == 0 {
                o.push(',');
            }
            o.push(char::from(*ch));
        }
        o
    } else {
        int_part
    };
    let mut out = int_part;
    if let Some(f) = frac {
        out.push('.');
        out.push_str(&f);
    }
    out
}

fn rounded(a: &[Arg], default_dec: f64) -> R<(f64, i64)> {
    let x = num(a, 0)?;
    let d = opt_num(a, 1, default_dec)?.trunc();
    if d > 127.0 {
        return Err(CellError::Value);
    }
    let d = d.max(-308.0) as i64;
    let r = round_half_away(x, d as i32);
    if !r.is_finite() || r.abs() >= 1e300 {
        return Err(CellError::Num);
    }
    Ok((r, d))
}

fn fixed(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (r, d) = rounded(a, 2.0)?;
    let no_commas = opt_bool(a, 2, false)?;
    let body = format_fixed(r, d, !no_commas);
    text_val(if r < 0.0 { format!("-{body}") } else { body })
}

fn dollar(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (r, d) = rounded(a, 2.0)?;
    let body = format_fixed(r, d, true);
    text_val(if r < 0.0 { format!("(${body})") } else { format!("${body}") })
}

fn convert_int_to_thai(num: u64, is_overall_gt_1: bool) -> String {
    if num == 0 {
        return String::new();
    }
    const THAI_DIGITS: &[&str] = &["", "หนึ่ง", "สอง", "สาม", "สี่", "ห้า", "หก", "เจ็ด", "แปด", "เก้า"];
    const THAI_POSITIONS: &[&str] = &["", "สิบ", "ร้อย", "พัน", "หมื่น", "แสน"];

    let mut groups = Vec::new();
    let mut temp = num;
    while temp > 0 {
        groups.push(temp % 1_000_000);
        temp /= 1_000_000;
    }

    let mut out = String::new();
    for g_idx in (0..groups.len()).rev() {
        let g = groups.get(g_idx).copied().unwrap_or(0);
        if g == 0 {
            continue;
        }
        let s = g.to_string();
        let bytes = s.as_bytes();
        let g_len = bytes.len();
        let mut group_text = String::new();

        for (pos_idx, &b) in bytes.iter().enumerate() {
            let d = (b.saturating_sub(b'0')) as usize;
            if d == 0 {
                continue;
            }
            let unit_pos = g_len.saturating_sub(pos_idx).saturating_sub(1);
            if unit_pos == 0 {
                if d == 1 {
                    if g_len > 1 {
                        group_text.push_str("เอ็ด");
                    } else if is_overall_gt_1 && g_idx == 0 {
                        group_text.push_str("เอ็ด");
                    } else {
                        group_text.push_str("หนึ่ง");
                    }
                } else if let Some(dig) = THAI_DIGITS.get(d) {
                    group_text.push_str(dig);
                }
            } else if unit_pos == 1 {
                if d == 1 {
                    group_text.push_str("สิบ");
                } else if d == 2 {
                    group_text.push_str("ยี่สิบ");
                } else if let Some(dig) = THAI_DIGITS.get(d) {
                    group_text.push_str(dig);
                    group_text.push_str("สิบ");
                }
            } else if let Some(dig) = THAI_DIGITS.get(d) {
                group_text.push_str(dig);
                if let Some(pos) = THAI_POSITIONS.get(unit_pos) {
                    group_text.push_str(pos);
                }
            }
        }

        out.push_str(&group_text);
        if g_idx > 0 {
            for _ in 0..g_idx {
                out.push_str("ล้าน");
            }
        }
    }
    out
}

fn bahttext_core(val: f64) -> R<String> {
    if !val.is_finite() || val.abs() > 1e15 {
        return Err(CellError::Num);
    }
    let is_neg = val < 0.0;
    let abs_val = val.abs();
    let cents = (abs_val * 100.0).round() as u64;
    let baht = cents / 100;
    let satang = cents % 100;

    if baht == 0 && satang == 0 {
        return Ok("ศูนย์บาทถ้วน".to_string());
    }

    let mut res = String::new();
    if is_neg {
        res.push_str("ลบ");
    }

    if baht > 0 {
        res.push_str(&convert_int_to_thai(baht, baht > 1));
        res.push_str("บาท");
    }

    if satang > 0 {
        res.push_str(&convert_int_to_thai(satang, satang > 1));
        res.push_str("สตางค์");
    } else {
        res.push_str("ถ้วน");
    }

    Ok(res)
}

fn bahttext(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let n = num(a, 0)?;
    text_val(bahttext_core(n)?)
}

fn t_fn(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    match scalar(arg(a, 0)?) {
        Value::Text(t) => Ok(Value::Text(t)),
        Value::Error(e) => Err(e),
        _ => text_val(""),
    }
}

fn identity(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    text_val(text(a, 0)?)
}

/// Lower-cased char sequence with one char per input char (keeps positions aligned).
fn fold(c: &[char]) -> Vec<char> {
    c.iter().map(|ch| ch.to_lowercase().next().unwrap_or(*ch)).collect()
}

/// Delimiter list from a scalar or array argument.
fn delimiters(v: &Value) -> R<Vec<Vec<char>>> {
    as_array(v).iter().map(|d| d.to_text().map(|s| chars(&s))).collect()
}

/// Non-overlapping delimiter matches (start, end) in char positions, scanning left to right and
/// preferring the longest delimiter at each position. Empty delimiters are ignored unless all
/// delimiters are empty, in which case every position matches with zero length.
fn find_matches(text: &[char], delims: &[Vec<char>], insensitive: bool) -> Vec<(usize, usize)> {
    let t = if insensitive { fold(text) } else { text.to_vec() };
    let ds: Vec<Vec<char>> = delims.iter().filter(|d| !d.is_empty()).map(|d| if insensitive { fold(d) } else { d.clone() }).collect();
    if ds.is_empty() {
        return (0..=t.len()).map(|i| (i, i)).collect();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let best = ds.iter().filter(|d| t.get(i..i + d.len()).is_some_and(|w| w == d.as_slice())).map(|d| d.len()).max();
        match best {
            Some(l) => {
                out.push((i, i + l));
                i += l;
            }
            None => i += 1,
        }
    }
    out
}

fn text_before_after(a: &[Arg], before: bool) -> R<Value> {
    let s = chars(&text(a, 0)?);
    let delims = delimiters(&arg(a, 1)?.value)?;
    let inst = opt_int(a, 2, 1)?;
    let mode = opt_int(a, 3, 0)?;
    let match_end = opt_bool(a, 4, false)?;
    if !(0..=1).contains(&mode) || inst == 0 || inst.unsigned_abs() as usize > s.len().max(1) {
        return Err(CellError::Value);
    }
    let mut m = find_matches(&s, &delims, mode == 1);
    if match_end {
        if inst > 0 {
            if m.last().is_none_or(|x| x.0 != s.len()) {
                m.push((s.len(), s.len()));
            }
        } else if m.first().is_none_or(|x| x.0 != 0) {
            m.insert(0, (0, 0));
        }
    }
    let k = inst.unsigned_abs() as usize;
    let hit = if inst > 0 { m.get(k - 1) } else { m.len().checked_sub(k).and_then(|i| m.get(i)) };
    match hit {
        Some(&(st, en)) => text_val(if before { s[..st].iter().collect::<String>() } else { s[en..].iter().collect::<String>() }),
        None => match a.get(5) {
            Some(x) => Ok(scalar(x)),
            None => Err(CellError::NA),
        },
    }
}

fn textbefore(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    text_before_after(a, true)
}

fn textafter(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    text_before_after(a, false)
}

fn split_by(s: &[char], delims: &[Vec<char>], insensitive: bool) -> Vec<Vec<char>> {
    if delims.iter().all(|d| d.is_empty()) {
        return vec![s.to_vec()];
    }
    let mut out = Vec::new();
    let mut last = 0;
    for (st, en) in find_matches(s, delims, insensitive) {
        out.push(s[last..st].to_vec());
        last = en;
    }
    out.push(s[last..].to_vec());
    out
}

fn textsplit(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = chars(&text(a, 0)?);
    let col_d = if has(a, 1) { delimiters(&arg(a, 1)?.value)? } else { vec![] };
    let row_d = if has(a, 2) { delimiters(&arg(a, 2)?.value)? } else { vec![] };
    if col_d.iter().all(|d| d.is_empty()) && row_d.iter().all(|d| d.is_empty()) {
        return Err(CellError::Value);
    }
    let ignore = opt_bool(a, 3, false)?;
    let mode = opt_int(a, 4, 0)?;
    if !(0..=1).contains(&mode) {
        return Err(CellError::Value);
    }
    let pad = if has(a, 5) { scalar(arg(a, 5)?) } else { Value::Error(CellError::NA) };
    let mut rows: Vec<Vec<String>> = Vec::new();
    for r in split_by(&s, &row_d, mode == 1) {
        if ignore && r.is_empty() {
            continue;
        }
        let cells: Vec<String> =
            split_by(&r, &col_d, mode == 1).into_iter().filter(|c| !(ignore && c.is_empty())).map(|c| c.into_iter().collect()).collect();
        if ignore && cells.is_empty() {
            continue;
        }
        rows.push(cells);
    }
    if rows.is_empty() {
        return Err(CellError::Calc);
    }
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let nrows = rows.len();
    let mut data = Vec::with_capacity(nrows * ncols);
    for r in rows {
        let n = r.len();
        data.extend(r.into_iter().map(Value::from));
        data.extend(std::iter::repeat_n(pad.clone(), ncols - n));
    }
    array_val(nrows, ncols, data)
}

fn value_text(v: &Value, strict: bool) -> String {
    match v {
        Value::Text(t) if strict => format!("\"{}\"", t.replace('"', "\"\"")),
        Value::Text(t) => t.to_string(),
        Value::Error(e) => e.as_str().to_string(),
        Value::Number(n) => number_to_text(*n),
        other => other.to_text().unwrap_or_default(),
    }
}

fn format_arg(a: &[Arg], i: usize) -> R<bool> {
    match opt_int(a, i, 0)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(CellError::Value),
    }
}

fn valuetotext(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let strict = format_arg(a, 1)?;
    check_len(value_text(&scalar(arg(a, 0)?), strict))
}

fn arraytotext(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let strict = format_arg(a, 1)?;
    let arr = as_array(&arg(a, 0)?.value);
    let mut out = String::new();
    if strict {
        out.push('{');
    }
    for r in 0..arr.rows {
        for c in 0..arr.cols {
            if r > 0 && c == 0 {
                out.push_str(if strict { ";" } else { ", " });
            } else if c > 0 {
                out.push_str(if strict { "," } else { ", " });
            }
            push_capped(&mut out, &value_text(arr.get(r, c).unwrap_or(&Value::Empty), strict))?;
        }
    }
    if strict {
        out.push('}');
    }
    check_len(out)
}

fn build_regex(pattern: &str, insensitive: bool) -> R<regex::Regex> {
    if pattern.len() > MAX_TEXT {
        return Err(CellError::Value);
    }
    regex::RegexBuilder::new(pattern)
        .case_insensitive(insensitive)
        .size_limit(1 << 22)
        .dfa_size_limit(1 << 22)
        .nest_limit(100)
        .build()
        .map_err(|_| CellError::Value)
}

fn case_arg(a: &[Arg], i: usize) -> R<bool> {
    match opt_int(a, i, 0)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(CellError::Value),
    }
}

fn regextest(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let re = build_regex(&text(a, 1)?, case_arg(a, 2)?)?;
    Ok(Value::Bool(re.is_match(&s)))
}

fn regexextract(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let mode = opt_int(a, 2, 0)?;
    let re = build_regex(&text(a, 1)?, case_arg(a, 3)?)?;
    match mode {
        0 => re.find(&s).map(|m| Value::from(m.as_str())).ok_or(CellError::NA),
        1 => {
            let all: Vec<Value> = re.find_iter(&s).take(1_000_000).map(|m| Value::from(m.as_str())).collect();
            if all.is_empty() {
                return Err(CellError::NA);
            }
            array_val(all.len(), 1, all)
        }
        2 => {
            let caps = re.captures(&s).ok_or(CellError::NA)?;
            if caps.len() <= 1 {
                return Ok(Value::from(caps.get(0).map_or("", |m| m.as_str())));
            }
            let groups: Vec<Value> = (1..caps.len()).map(|i| Value::from(caps.get(i).map_or("", |m| m.as_str()))).collect();
            array_val(1, groups.len(), groups)
        }
        _ => Err(CellError::Value),
    }
}

/// Converts Excel-style `$1` references to the regex crate's unambiguous `${1}` form.
fn convert_replacement(r: &str) -> String {
    let mut out = String::with_capacity(r.len() + 4);
    let c: Vec<char> = r.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '$' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit()) {
            let mut j = i + 1;
            while c.get(j).is_some_and(|d| d.is_ascii_digit()) {
                j += 1;
            }
            out.push_str("${");
            out.extend(c[i + 1..j].iter());
            out.push('}');
            i = j;
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

fn regexreplace(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let s = text(a, 0)?;
    let rep = convert_replacement(&text(a, 2)?);
    let occ = opt_int(a, 3, 0)?;
    let re = build_regex(&text(a, 1)?, case_arg(a, 4)?)?;
    if occ == 0 {
        return check_len(re.replace_all(&s, rep.as_str()).into_owned());
    }
    let caps: Vec<regex::Captures> = re.captures_iter(&s).collect();
    let k = occ.unsigned_abs() as usize;
    let idx = if occ > 0 { k.checked_sub(1) } else { caps.len().checked_sub(k) };
    let Some(cap) = idx.and_then(|i| caps.get(i)) else { return text_val(s) };
    let Some(m) = cap.get(0) else { return text_val(s) };
    let mut out = String::new();
    out.push_str(&s[..m.start()]);
    cap.expand(&rep, &mut out);
    out.push_str(&s[m.end()..]);
    check_len(out)
}

pub(crate) fn specs() -> Vec<FnSpec> {
    vec![
        f!("CONCATENATE", 1, VAR, Text, S, "CONCATENATE(text1, [text2], ...)", "Joins several text values into one.", concatenate),
        f!("CONCAT", 1, VAR, Text, A, "CONCAT(text1, [text2], ...)", "Joins text from values and every cell of ranges, without a separator.", concat),
        f!(
            "TEXTJOIN",
            3,
            VAR,
            Text,
            TJ,
            "TEXTJOIN(delimiter, ignore_empty, text1, [text2], ...)",
            "Joins text from values and ranges with a delimiter, optionally skipping empty items.",
            textjoin
        ),
        f!("LEFT", 1, 2, Text, S, "LEFT(text, [num_chars])", "Returns the first characters of a text.", left),
        f!("LEFTB", 1, 2, Text, S, "LEFTB(text, [num_bytes])", "Byte version of LEFT (same as LEFT for single-byte text).", left),
        f!("RIGHT", 1, 2, Text, S, "RIGHT(text, [num_chars])", "Returns the last characters of a text.", right),
        f!("RIGHTB", 1, 2, Text, S, "RIGHTB(text, [num_bytes])", "Byte version of RIGHT (same as RIGHT for single-byte text).", right),
        f!("MID", 3, 3, Text, S, "MID(text, start_num, num_chars)", "Returns characters from the middle of a text.", mid),
        f!("MIDB", 3, 3, Text, S, "MIDB(text, start_num, num_bytes)", "Byte version of MID (same as MID for single-byte text).", mid),
        f!("LEN", 1, 1, Text, S, "LEN(text)", "Counts the characters in a text.", len),
        f!("LENB", 1, 1, Text, S, "LENB(text)", "Byte version of LEN (same as LEN for single-byte text).", len),
        f!("LOWER", 1, 1, Text, S, "LOWER(text)", "Converts text to lower case.", lower),
        f!("UPPER", 1, 1, Text, S, "UPPER(text)", "Converts text to upper case.", upper),
        f!("PROPER", 1, 1, Text, S, "PROPER(text)", "Capitalises the first letter of each word and lower-cases the rest.", proper),
        f!("TRIM", 1, 1, Text, S, "TRIM(text)", "Removes leading and trailing spaces and collapses runs of spaces to one.", trim),
        f!("CLEAN", 1, 1, Text, S, "CLEAN(text)", "Removes non-printable control characters from text.", clean),
        f!(
            "SUBSTITUTE",
            3,
            4,
            Text,
            S,
            "SUBSTITUTE(text, old_text, new_text, [instance_num])",
            "Replaces occurrences of one text with another, optionally only a given occurrence.",
            substitute
        ),
        f!("REPLACE", 4, 4, Text, S, "REPLACE(old_text, start_num, num_chars, new_text)", "Replaces part of a text by position.", replace),
        f!("REPLACEB", 4, 4, Text, S, "REPLACEB(old_text, start_num, num_bytes, new_text)", "Byte version of REPLACE.", replace),
        f!("REPT", 2, 2, Text, S, "REPT(text, number_times)", "Repeats a text a given number of times.", rept),
        f!("FIND", 2, 3, Text, S, "FIND(find_text, within_text, [start_num])", "Finds the position of one text in another (case-sensitive).", find),
        f!("FINDB", 2, 3, Text, S, "FINDB(find_text, within_text, [start_num])", "Byte version of FIND.", find),
        f!(
            "SEARCH",
            2,
            3,
            Text,
            S,
            "SEARCH(find_text, within_text, [start_num])",
            "Finds the position of one text in another, ignoring case and allowing wildcards.",
            search
        ),
        f!("SEARCHB", 2, 3, Text, S, "SEARCHB(find_text, within_text, [start_num])", "Byte version of SEARCH.", search),
        f!("EXACT", 2, 2, Text, S, "EXACT(text1, text2)", "TRUE when two texts are identical, including case.", exact),
        f!("CHAR", 1, 1, Text, S, "CHAR(number)", "Returns the character for a code from the Windows-1252 character set.", char_fn),
        f!("CODE", 1, 1, Text, S, "CODE(text)", "Returns the Windows-1252 code of the first character of a text.", code),
        f!("UNICHAR", 1, 1, Text, S, "UNICHAR(number)", "Returns the character for a Unicode code point.", unichar),
        f!("UNICODE", 1, 1, Text, S, "UNICODE(text)", "Returns the Unicode code point of the first character of a text.", unicode),
        f!("VALUE", 1, 1, Text, S, "VALUE(text)", "Converts text that looks like a number, date or time into a number.", value),
        f!(
            "NUMBERVALUE",
            1,
            3,
            Text,
            S,
            "NUMBERVALUE(text, [decimal_separator], [group_separator])",
            "Converts text to a number using the given decimal and group separators.",
            numbervalue
        ),
        f!(
            "FIXED",
            1,
            3,
            Text,
            S,
            "FIXED(number, [decimals], [no_commas])",
            "Rounds a number and formats it as text with fixed decimals and optional thousands separators.",
            fixed
        ),
        f!("DOLLAR", 1, 2, Text, S, "DOLLAR(number, [decimals])", "Formats a number as currency text.", dollar),
        f!("BAHTTEXT", 1, 1, Text, S, "BAHTTEXT(number)", "Converts a number to Thai currency text.", bahttext),
        f!("T", 1, 1, Text, S, "T(value)", "Returns the value if it is text, otherwise empty text.", t_fn),
        f!(
            "TEXTBEFORE",
            2,
            6,
            Text,
            TB,
            "TEXTBEFORE(text, delimiter, [instance_num], [match_mode], [match_end], [if_not_found])",
            "Returns the text that comes before a delimiter.",
            textbefore
        ),
        f!(
            "TEXTAFTER",
            2,
            6,
            Text,
            TB,
            "TEXTAFTER(text, delimiter, [instance_num], [match_mode], [match_end], [if_not_found])",
            "Returns the text that comes after a delimiter.",
            textafter
        ),
        f!(
            "TEXTSPLIT",
            2,
            6,
            Text,
            TS,
            "TEXTSPLIT(text, col_delimiter, [row_delimiter], [ignore_empty], [match_mode], [pad_with])",
            "Splits text into a grid of columns and rows at the given delimiters.",
            textsplit
        ),
        f!(
            "VALUETOTEXT",
            1,
            2,
            Text,
            S,
            "VALUETOTEXT(value, [format])",
            "Converts any value to text, optionally in strict (quoted) form.",
            valuetotext
        ),
        f!(
            "ARRAYTOTEXT",
            1,
            2,
            Text,
            ATT,
            "ARRAYTOTEXT(array, [format])",
            "Converts an array to text, either as a comma list or as a strict array constant.",
            arraytotext
        ),
        f!(
            "REGEXTEST",
            2,
            3,
            Text,
            S,
            "REGEXTEST(text, pattern, [case_sensitivity])",
            "TRUE when a regular expression matches somewhere in the text.",
            regextest
        ),
        f!(
            "REGEXEXTRACT",
            2,
            4,
            Text,
            S,
            "REGEXEXTRACT(text, pattern, [return_mode], [case_sensitivity])",
            "Extracts the first match, all matches or the capture groups of a regular expression.",
            regexextract
        ),
        f!(
            "REGEXREPLACE",
            3,
            5,
            Text,
            S,
            "REGEXREPLACE(text, pattern, replacement, [occurrence], [case_sensitivity])",
            "Replaces matches of a regular expression, all of them or a chosen occurrence.",
            regexreplace
        ),
        f!("ASC", 1, 1, Text, S, "ASC(text)", "Converts full-width characters to half-width (no change for single-byte text).", identity),
        f!("DBCS", 1, 1, Text, S, "DBCS(text)", "Converts half-width characters to full-width (no change in this locale).", identity),
        f!("JIS", 1, 1, Text, S, "JIS(text)", "Converts half-width characters to full-width (no change in this locale).", identity),
        f!("PHONETIC", 1, 1, Text, S, "PHONETIC(reference)", "Returns the phonetic (furigana) text; here the text itself.", identity),
    ]
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellError, Value};

    use crate::util::testutil::*;

    #[test]
    fn concat_family() {
        is_text(ev("CONCATENATE", vec![t("a"), n(1.5), b(true)]), "a1.5TRUE");
        is_text(ev("CONCAT", vec![av(arr(vec![vec![tv("a"), tv("b")], vec![nv(1.0), Value::Empty]])), t("!")]), "ab1!");
        is_text(ev("TEXTJOIN", vec![t("-"), b(true), av(arr(vec![vec![tv("a"), tv(""), tv("b")]])), t("c")]), "a-b-c");
        is_text(ev("TEXTJOIN", vec![t("-"), b(false), av(arr(vec![vec![tv("a"), tv(""), tv("b")]]))]), "a--b");
        is_text(ev("TEXTJOIN", vec![av(row_t(&[",", ";"])), b(true), t("a"), t("b"), t("c")]), "a,b;c");
        is_err(ev("CONCAT", vec![t("a"), e(CellError::NA)]), CellError::NA);
        // Lifting: CONCATENATE over an array.
        assert_eq!(rows_of(&ev("CONCATENATE", vec![av(row_t(&["a", "b"])), t("x")])), vec![vec![tv("ax"), tv("bx")]]);
    }

    fn row_t(xs: &[&str]) -> Value {
        arr(vec![xs.iter().map(|x| tv(x)).collect()])
    }

    #[test]
    fn slicing() {
        is_text(ev("LEFT", vec![t("Hello")]), "H");
        is_text(ev("LEFT", vec![t("Hello"), n(3.0)]), "Hel");
        is_text(ev("LEFT", vec![t("Hello"), n(99.0)]), "Hello");
        is_err(ev("LEFT", vec![t("Hello"), n(-1.0)]), CellError::Value);
        is_text(ev("RIGHT", vec![t("Hello"), n(2.0)]), "lo");
        is_text(ev("RIGHT", vec![t("héllo"), n(4.0)]), "éllo");
        is_text(ev("MID", vec![t("Hello"), n(2.0), n(3.0)]), "ell");
        is_text(ev("MID", vec![t("Hello"), n(10.0), n(3.0)]), "");
        is_err(ev("MID", vec![t("Hello"), n(0.0), n(3.0)]), CellError::Value);
        close(ev("LEN", vec![t("héllo")]), 5.0);
        close(ev("LEN", vec![n(123.45)]), 6.0);
        close(ev("LEN", vec![empty()]), 0.0);
        is_text(ev("LEFT", vec![n(12345.0), n(2.0)]), "12");
    }

    #[test]
    fn case_and_cleanup() {
        is_text(ev("UPPER", vec![t("abc")]), "ABC");
        is_text(ev("LOWER", vec![t("ABC")]), "abc");
        is_text(ev("PROPER", vec![t("this is a TITLE")]), "This Is A Title");
        is_text(ev("PROPER", vec![t("2-way street's")]), "2-Way Street'S");
        is_text(ev("TRIM", vec![t("  a   b  c ")]), "a b c");
        is_text(ev("CLEAN", vec![t("a\u{7}b\nc")]), "abc");
    }

    #[test]
    fn substitute_replace_rept() {
        is_text(ev("SUBSTITUTE", vec![t("a-b-c"), t("-"), t("+")]), "a+b+c");
        is_text(ev("SUBSTITUTE", vec![t("a-b-c"), t("-"), t("+"), n(2.0)]), "a-b+c");
        is_text(ev("SUBSTITUTE", vec![t("a-b-c"), t("-"), t("+"), n(5.0)]), "a-b-c");
        is_err(ev("SUBSTITUTE", vec![t("a"), t("a"), t("b"), n(0.0)]), CellError::Value);
        is_text(ev("SUBSTITUTE", vec![t("abc"), t(""), t("x")]), "abc");
        is_text(ev("REPLACE", vec![t("abcdef"), n(2.0), n(3.0), t("XY")]), "aXYef");
        is_text(ev("REPLACE", vec![t("abc"), n(10.0), n(1.0), t("Z")]), "abcZ");
        is_text(ev("REPT", vec![t("ab"), n(3.0)]), "ababab");
        is_text(ev("REPT", vec![t("ab"), n(0.0)]), "");
        is_err(ev("REPT", vec![t("ab"), n(-1.0)]), CellError::Value);
        is_err(ev("REPT", vec![t("ab"), n(20000.0)]), CellError::Value);
        is_err(ev("REPT", vec![t("ab"), n(1e300)]), CellError::Value);
    }

    #[test]
    fn find_search() {
        close(ev("FIND", vec![t("b"), t("abcb")]), 2.0);
        close(ev("FIND", vec![t("b"), t("abcb"), n(3.0)]), 4.0);
        is_err(ev("FIND", vec![t("B"), t("abc")]), CellError::Value);
        close(ev("FIND", vec![t(""), t("abc")]), 1.0);
        is_err(ev("FIND", vec![t("a"), t("abc"), n(0.0)]), CellError::Value);
        close(ev("SEARCH", vec![t("B"), t("abc")]), 2.0);
        close(ev("SEARCH", vec![t("c*"), t("abcd")]), 3.0);
        close(ev("SEARCH", vec![t("?d"), t("abcd")]), 3.0);
        close(ev("SEARCH", vec![t("~*"), t("a*b")]), 2.0);
        is_err(ev("SEARCH", vec![t("z"), t("abc")]), CellError::Value);
        is_err(ev("SEARCH", vec![t("a"), t("abc"), n(5.0)]), CellError::Value);
        assert_eq!(ev("EXACT", vec![t("a"), t("A")]), Value::Bool(false));
        assert_eq!(ev("EXACT", vec![t("a"), t("a")]), Value::Bool(true));
    }

    #[test]
    fn codes() {
        is_text(ev("CHAR", vec![n(65.0)]), "A");
        is_text(ev("CHAR", vec![n(128.0)]), "€");
        is_err(ev("CHAR", vec![n(0.0)]), CellError::Value);
        is_err(ev("CHAR", vec![n(256.0)]), CellError::Value);
        close(ev("CODE", vec![t("A")]), 65.0);
        close(ev("CODE", vec![t("€")]), 128.0);
        is_err(ev("CODE", vec![t("")]), CellError::Value);
        is_text(ev("UNICHAR", vec![n(9731.0)]), "☃");
        close(ev("UNICODE", vec![t("☃")]), 9731.0);
        is_err(ev("UNICHAR", vec![n(0.0)]), CellError::Value);
        is_err(ev("UNICHAR", vec![n(55296.0)]), CellError::NA);
    }

    #[test]
    fn numbers_from_text() {
        close(ev("VALUE", vec![t("1,234.5")]), 1234.5);
        close(ev("VALUE", vec![t("50%")]), 0.5);
        close(ev("VALUE", vec![t("$10")]), 10.0);
        close(ev("VALUE", vec![t("12:00")]), 0.5);
        close(ev("VALUE", vec![t("2020-01-01")]), 43831.0);
        is_err(ev("VALUE", vec![t("abc")]), CellError::Value);
        close(ev("NUMBERVALUE", vec![t("2.500,27"), t(","), t(".")]), 2500.27);
        close(ev("NUMBERVALUE", vec![t("3.5%")]), 0.035);
        close(ev("NUMBERVALUE", vec![t(" 1 000 ")]), 1000.0);
        close(ev("NUMBERVALUE", vec![t("")]), 0.0);
        is_err(ev("NUMBERVALUE", vec![t("1.2.3")]), CellError::Value);
        is_text(ev("FIXED", vec![n(1234.567), n(1.0)]), "1,234.6");
        is_text(ev("FIXED", vec![n(1234.567), n(-1.0)]), "1,230");
        is_text(ev("FIXED", vec![n(-1234.567), n(1.0), b(true)]), "-1234.6");
        is_text(ev("FIXED", vec![n(44.332)]), "44.33");
        is_text(ev("FIXED", vec![n(2.675), n(2.0)]), "2.68");
        is_text(ev("DOLLAR", vec![n(1234.567)]), "$1,234.57");
        is_text(ev("DOLLAR", vec![n(-1234.567), n(1.0)]), "($1,234.6)");
        is_text(ev("DOLLAR", vec![n(0.5), n(0.0)]), "$1");
        is_text(ev("T", vec![t("x")]), "x");
        is_text(ev("T", vec![n(1.0)]), "");
    }

    #[test]
    fn before_after() {
        is_text(ev("TEXTBEFORE", vec![t("Red riding hood"), t(" ")]), "Red");
        is_text(ev("TEXTBEFORE", vec![t("Red riding hood"), t(" "), n(2.0)]), "Red riding");
        is_text(ev("TEXTBEFORE", vec![t("Red riding hood"), t(" "), n(-1.0)]), "Red riding");
        is_text(ev("TEXTAFTER", vec![t("Red riding hood"), t(" ")]), "riding hood");
        is_text(ev("TEXTAFTER", vec![t("Red riding hood"), t(" "), n(-1.0)]), "hood");
        is_err(ev("TEXTAFTER", vec![t("abc"), t("x")]), CellError::NA);
        is_text(ev("TEXTAFTER", vec![t("abc"), t("x"), n(1.0), n(0.0), n(0.0), t("none")]), "none");
        is_text(ev("TEXTAFTER", vec![t("aXb"), t("x"), n(1.0), n(1.0)]), "b");
        is_err(ev("TEXTAFTER", vec![t("aXb"), t("x")]), CellError::NA);
        is_text(ev("TEXTBEFORE", vec![t("abc"), t("x"), n(1.0), n(0.0), n(1.0)]), "abc");
        is_text(ev("TEXTAFTER", vec![t("a-b+c"), av(row_t(&["+", "-"]))]), "b+c");
        is_err(ev("TEXTBEFORE", vec![t("abc"), t("b"), n(0.0)]), CellError::Value);
        is_text(ev("TEXTBEFORE", vec![t("abc"), t("")]), "");
        is_text(ev("TEXTAFTER", vec![t("abc"), t("")]), "abc");
    }

    #[test]
    fn split() {
        let v = ev("TEXTSPLIT", vec![t("a,b;c,d"), t(","), t(";")]);
        assert_eq!(rows_of(&v), vec![vec![tv("a"), tv("b")], vec![tv("c"), tv("d")]]);
        let v = ev("TEXTSPLIT", vec![t("a,b;c"), t(","), t(";")]);
        assert_eq!(rows_of(&v), vec![vec![tv("a"), tv("b")], vec![tv("c"), Value::Error(CellError::NA)]]);
        let v = ev("TEXTSPLIT", vec![t("a,,b"), t(","), empty(), b(true)]);
        assert_eq!(rows_of(&v), vec![vec![tv("a"), tv("b")]]);
        let v = ev("TEXTSPLIT", vec![t("a,,b"), t(",")]);
        assert_eq!(rows_of(&v), vec![vec![tv("a"), tv(""), tv("b")]]);
        let v = ev("TEXTSPLIT", vec![t("a b-c"), av(row_t(&[" ", "-"]))]);
        assert_eq!(rows_of(&v), vec![vec![tv("a"), tv("b"), tv("c")]]);
        let v = ev("TEXTSPLIT", vec![t("1;2"), empty(), t(";")]);
        assert_eq!(rows_of(&v), vec![vec![tv("1")], vec![tv("2")]]);
        let v = ev("TEXTSPLIT", vec![t("aXbxc"), t("x"), empty(), b(false), n(1.0)]);
        assert_eq!(rows_of(&v), vec![vec![tv("a"), tv("b"), tv("c")]]);
        let v = ev("TEXTSPLIT", vec![t("a,b;c"), t(","), t(";"), b(false), n(0.0), t("-")]);
        assert_eq!(rows_of(&v)[1], vec![tv("c"), tv("-")]);
    }

    #[test]
    fn to_text() {
        is_text(ev("VALUETOTEXT", vec![t("a\"b")]), "a\"b");
        is_text(ev("VALUETOTEXT", vec![t("ab"), n(1.0)]), "\"ab\"");
        is_text(ev("VALUETOTEXT", vec![n(1.5), n(1.0)]), "1.5");
        is_text(ev("VALUETOTEXT", vec![e(CellError::Div0)]), "#DIV/0!");
        is_err(ev("VALUETOTEXT", vec![n(1.0), n(2.0)]), CellError::Value);
        let a = av(arr(vec![vec![nv(1.0), tv("a")], vec![Value::Bool(true), Value::Error(CellError::NA)]]));
        is_text(ev("ARRAYTOTEXT", vec![a.clone()]), "1, a, TRUE, #N/A");
        is_text(ev("ARRAYTOTEXT", vec![a, n(1.0)]), "{1,\"a\";TRUE,#N/A}");
    }

    #[test]
    fn regex() {
        assert_eq!(ev("REGEXTEST", vec![t("abc123"), t("[0-9]+")]), Value::Bool(true));
        assert_eq!(ev("REGEXTEST", vec![t("ABC"), t("abc")]), Value::Bool(false));
        assert_eq!(ev("REGEXTEST", vec![t("ABC"), t("abc"), n(1.0)]), Value::Bool(true));
        is_err(ev("REGEXTEST", vec![t("a"), t("(")]), CellError::Value);
        is_text(ev("REGEXEXTRACT", vec![t("tel 555-1234 or 555-9876"), t("[0-9]{3}-[0-9]{4}")]), "555-1234");
        let v = ev("REGEXEXTRACT", vec![t("tel 555-1234 or 555-9876"), t("[0-9]{3}-[0-9]{4}"), n(1.0)]);
        assert_eq!(rows_of(&v), vec![vec![tv("555-1234")], vec![tv("555-9876")]]);
        let v = ev("REGEXEXTRACT", vec![t("John Smith"), t("(\\w+) (\\w+)"), n(2.0)]);
        assert_eq!(rows_of(&v), vec![vec![tv("John"), tv("Smith")]]);
        is_err(ev("REGEXEXTRACT", vec![t("abc"), t("z")]), CellError::NA);
        is_text(ev("REGEXREPLACE", vec![t("a1b22c"), t("[0-9]+"), t("#")]), "a#b#c");
        is_text(ev("REGEXREPLACE", vec![t("a1b22c"), t("[0-9]+"), t("#"), n(2.0)]), "a1b#c");
        is_text(ev("REGEXREPLACE", vec![t("a1b22c"), t("[0-9]+"), t("#"), n(-2.0)]), "a#b22c");
        is_text(ev("REGEXREPLACE", vec![t("John Smith"), t("(\\w+) (\\w+)"), t("$2, $1")]), "Smith, John");
        is_text(ev("REGEXREPLACE", vec![t("ab"), t("(a)"), t("$1x")]), "axb");
    }

    #[test]
    fn bahttext() {
        is_text(ev("BAHTTEXT", vec![n(0.0)]), "ศูนย์บาทถ้วน");
        is_text(ev("BAHTTEXT", vec![n(1.0)]), "หนึ่งบาทถ้วน");
        is_text(ev("BAHTTEXT", vec![n(11.0)]), "สิบเอ็ดบาทถ้วน");
        is_text(ev("BAHTTEXT", vec![n(21.0)]), "ยี่สิบเอ็ดบาทถ้วน");
        is_text(ev("BAHTTEXT", vec![n(101.0)]), "หนึ่งร้อยเอ็ดบาทถ้วน");
        is_text(ev("BAHTTEXT", vec![n(1001.0)]), "หนึ่งพันเอ็ดบาทถ้วน");
        is_text(ev("BAHTTEXT", vec![n(1500.50)]), "หนึ่งพันห้าร้อยบาทห้าสิบสตางค์");
        is_text(ev("BAHTTEXT", vec![n(1234.50)]), "หนึ่งพันสองร้อยสามสิบสี่บาทห้าสิบสตางค์");
        is_text(ev("BAHTTEXT", vec![n(0.50)]), "ห้าสิบสตางค์");
        is_text(ev("BAHTTEXT", vec![n(-1500.50)]), "ลบหนึ่งพันห้าร้อยบาทห้าสิบสตางค์");
        is_text(ev("BAHTTEXT", vec![n(1_000_000.0)]), "หนึ่งล้านบาทถ้วน");
        is_text(ev("BAHTTEXT", vec![n(1_000_001.0)]), "หนึ่งล้านเอ็ดบาทถ้วน");
        is_err(ev("BAHTTEXT", vec![t("invalid")]), CellError::Value);
    }
}

