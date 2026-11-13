use super::{Arg, CellError, Ctx, R, Value, flatten, has, num, num_val, number_to_text, scalar, text, text_val};

// ---------------------------------------------------------------------------------------------
// Complex numbers

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Cx {
    pub(super) re: f64,
    pub(super) im: f64,
}

impl Cx {
    pub(super) fn new(re: f64, im: f64) -> Cx {
        Cx { re, im }
    }
    fn add(self, o: Cx) -> Cx {
        Cx::new(self.re + o.re, self.im + o.im)
    }
    fn sub(self, o: Cx) -> Cx {
        Cx::new(self.re - o.re, self.im - o.im)
    }
    fn mul(self, o: Cx) -> Cx {
        Cx::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    pub(super) fn div(self, o: Cx) -> R<Cx> {
        let d = o.re * o.re + o.im * o.im;
        if d == 0.0 {
            return Err(CellError::Num);
        }
        Ok(Cx::new((self.re * o.re + self.im * o.im) / d, (self.im * o.re - self.re * o.im) / d))
    }
    pub(super) fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
    fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }
    pub(super) fn exp(self) -> Cx {
        let e = self.re.exp();
        Cx::new(e * self.im.cos(), e * self.im.sin())
    }
    pub(super) fn ln(self) -> R<Cx> {
        if self.re == 0.0 && self.im == 0.0 {
            return Err(CellError::Num);
        }
        Ok(Cx::new(self.abs().ln(), self.arg()))
    }
    pub(super) fn sin(self) -> Cx {
        Cx::new(self.re.sin() * self.im.cosh(), self.re.cos() * self.im.sinh())
    }
    pub(super) fn cos(self) -> Cx {
        Cx::new(self.re.cos() * self.im.cosh(), -self.re.sin() * self.im.sinh())
    }
    pub(super) fn sinh(self) -> Cx {
        Cx::new(self.re.sinh() * self.im.cos(), self.re.cosh() * self.im.sin())
    }
    pub(super) fn cosh(self) -> Cx {
        Cx::new(self.re.cosh() * self.im.cos(), self.re.sinh() * self.im.sin())
    }
    fn powf(self, n: f64) -> R<Cx> {
        if self.re == 0.0 && self.im == 0.0 {
            return if n > 0.0 { Ok(Cx::new(0.0, 0.0)) } else { Err(CellError::Num) };
        }
        let r = self.abs().powf(n);
        let t = self.arg() * n;
        Ok(Cx::new(r * t.cos(), r * t.sin()))
    }
}

pub(super) const ONE: Cx = Cx { re: 1.0, im: 0.0 };

/// Parses a real number written in a complex string (no spaces, optional exponent). A `.` is
/// always a decimal point; where the region writes decimals with a `,` (`1,5+2i` in pt-BR) that
/// is one too.
fn parse_real(s: &str, decimal: char) -> Option<f64> {
    if s.is_empty() || s.contains(char::is_whitespace) {
        return None;
    }
    let ok = s.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-') || (c == ',' && decimal == ','));
    if !ok {
        return None;
    }
    s.replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Parses "a+bi" forms. Returns the number and its suffix ('i', 'j' or none).
pub(super) fn parse_complex(s: &str, decimal: char) -> R<(Cx, Option<char>)> {
    if s.is_empty() {
        return Ok((Cx::new(0.0, 0.0), None));
    }
    let suffix = match s.chars().last() {
        Some(c @ ('i' | 'j')) => Some(c),
        _ => None,
    };
    let Some(sfx) = suffix else {
        return parse_real(s, decimal).map(|r| (Cx::new(r, 0.0), None)).ok_or(CellError::Num);
    };
    let body = &s[..s.len() - 1];
    // Split at the last sign that is not at the start and not part of an exponent.
    let bytes = body.as_bytes();
    let mut split = None;
    for i in (1..bytes.len()).rev() {
        let c = bytes.get(i).copied().unwrap_or(0);
        let prev = bytes.get(i - 1).copied().unwrap_or(0);
        if (c == b'+' || c == b'-') && prev != b'e' && prev != b'E' {
            split = Some(i);
            break;
        }
    }
    let (re_s, im_s) = match split {
        Some(i) => (&body[..i], &body[i..]),
        None => ("", body),
    };
    let re = if re_s.is_empty() { 0.0 } else { parse_real(re_s, decimal).ok_or(CellError::Num)? };
    let im = match im_s {
        "" | "+" => 1.0,
        "-" => -1.0,
        t => parse_real(t, decimal).ok_or(CellError::Num)?,
    };
    Ok((Cx::new(re, im), Some(sfx)))
}

fn cx_of(v: &Value, decimal: char) -> R<(Cx, Option<char>)> {
    match v {
        Value::Number(n) => Ok((Cx::new(*n, 0.0), None)),
        Value::Empty => Ok((Cx::new(0.0, 0.0), None)),
        Value::Bool(_) => Err(CellError::Value),
        Value::Error(e) => Err(*e),
        Value::Text(t) => parse_complex(t, decimal),
        Value::Array(a) => cx_of(a.data.first().unwrap_or(&Value::Empty), decimal),
    }
}

fn cx_arg(a: &[Arg], i: usize, decimal: char) -> R<(Cx, Option<char>)> {
    cx_of(&a.get(i).map(scalar).unwrap_or(Value::Empty), decimal)
}

fn merge_suffix(acc: Option<char>, s: Option<char>) -> R<Option<char>> {
    match (acc, s) {
        (Some(x), Some(y)) if x != y => Err(CellError::Value),
        (Some(x), _) => Ok(Some(x)),
        (None, y) => Ok(y),
    }
}

/// Writes `z` as `a+bi`, with the region's decimal separator (`1,5+2i` in pt-BR).
fn fmt_complex(z: Cx, suffix: char, decimal: char) -> R<Value> {
    if !z.re.is_finite() || !z.im.is_finite() {
        return Err(CellError::Num);
    }
    let num_text = |x: f64| number_to_text(x).replace('.', decimal.encode_utf8(&mut [0; 4]));
    let re = clean(z.re);
    let im = clean(z.im);
    let mut s = String::new();
    if im == 0.0 {
        s.push_str(&num_text(re));
        return text_val(s);
    }
    if re != 0.0 {
        s.push_str(&num_text(re));
        if im > 0.0 {
            s.push('+');
        }
    }
    if im == -1.0 {
        s.push('-');
    } else if im != 1.0 {
        s.push_str(&num_text(im));
    }
    s.push(suffix);
    text_val(s)
}

/// Rounds to 15 significant digits so values like 0.999999999999999999 print as 1.
fn clean(x: f64) -> f64 {
    let s = format!("{:.14e}", x);
    s.parse().unwrap_or(x)
}

pub(super) fn complex(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let re = num(a, 0, c)?;
    let im = num(a, 1, c)?;
    let sfx = if has(a, 2) { text(a, 2)? } else { "i".into() };
    let sfx = match sfx.as_str() {
        "i" | "" => 'i',
        "j" => 'j',
        _ => return Err(CellError::Value),
    };
    fmt_complex(Cx::new(re, im), sfx, c.region().decimal)
}

pub(super) fn im_unary(a: &[Arg], f: fn(Cx) -> R<Cx>, decimal: char) -> R<Value> {
    let (z, s) = cx_arg(a, 0, decimal)?;
    fmt_complex(f(z)?, s.unwrap_or('i'), decimal)
}

fn im_real_fn(a: &[Arg], f: fn(Cx) -> R<f64>, decimal: char) -> R<Value> {
    let (z, _) = cx_arg(a, 0, decimal)?;
    num_val(f(z)?)
}

pub(super) fn imabs(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_real_fn(a, |z| Ok(z.abs()), c.region().decimal)
}

pub(super) fn imreal(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_real_fn(a, |z| Ok(z.re), c.region().decimal)
}

pub(super) fn imaginary(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_real_fn(a, |z| Ok(z.im), c.region().decimal)
}

pub(super) fn imargument(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_real_fn(a, |z| if z.re == 0.0 && z.im == 0.0 { Err(CellError::Div0) } else { Ok(z.arg()) }, c.region().decimal)
}

fn im_binary(a: &[Arg], f: fn(Cx, Cx) -> R<Cx>, decimal: char) -> R<Value> {
    let (x, s1) = cx_arg(a, 0, decimal)?;
    let (y, s2) = cx_arg(a, 1, decimal)?;
    let s = merge_suffix(s1, s2)?;
    fmt_complex(f(x, y)?, s.unwrap_or('i'), decimal)
}

pub(super) fn imdiv(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_binary(a, Cx::div, c.region().decimal)
}

pub(super) fn imsub(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_binary(a, |x, y| Ok(x.sub(y)), c.region().decimal)
}

pub(super) fn impower(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let decimal = c.region().decimal;
    let (z, s) = cx_arg(a, 0, decimal)?;
    let n = num(a, 1, c)?;
    // Integer powers by repeated multiplication are exact for small Gaussian integers.
    let r = if n == n.trunc() && n.abs() <= 64.0 && !(z.re == 0.0 && z.im == 0.0) {
        let mut acc = ONE;
        for _ in 0..(n.abs() as u32) {
            acc = acc.mul(z);
        }
        if n < 0.0 { ONE.div(acc)? } else { acc }
    } else {
        z.powf(n)?
    };
    fmt_complex(r, s.unwrap_or('i'), decimal)
}

fn im_fold(a: &[Arg], init: Cx, f: fn(Cx, Cx) -> Cx, decimal: char) -> R<Value> {
    let mut acc = init;
    let mut sfx = None;
    for arg in a {
        let direct = !matches!(arg.value, Value::Array(_));
        for v in flatten(std::slice::from_ref(arg)) {
            if matches!(v, Value::Empty) && !direct {
                continue;
            }
            let (z, s) = cx_of(&v, decimal)?;
            sfx = merge_suffix(sfx, s)?;
            acc = f(acc, z);
        }
    }
    fmt_complex(acc, sfx.unwrap_or('i'), decimal)
}

pub(super) fn imsum(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_fold(a, Cx::new(0.0, 0.0), Cx::add, c.region().decimal)
}

pub(super) fn improduct(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_fold(a, ONE, Cx::mul, c.region().decimal)
}
