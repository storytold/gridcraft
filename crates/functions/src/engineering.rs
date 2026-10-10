//! Engineering functions: base conversion, bit operations, unit conversion, error function,
//! complex numbers and Bessel functions.

use std::f64::consts::PI;

use gridcraft_core::{CellError, Value, number_to_text_in};

use crate::special::{erf, erfc};
use crate::util::{A, R, S, flatten, has, num, num_val, opt_num, scalar, text, text_val};
use crate::{Arg, Ctx, FnSpec, VAR};

// ---------------------------------------------------------------------------------------------
// Base conversion

/// Source text of a base-conversion argument (numbers are written out as integers).
fn base_source(c: &dyn Ctx, a: &[Arg]) -> R<String> {
    let v = a.first().map(scalar).unwrap_or(Value::Empty);
    match v {
        Value::Error(e) => Err(e),
        Value::Bool(_) => Err(CellError::Value),
        Value::Number(n) => {
            if n < 0.0 || n != n.trunc() {
                return Err(CellError::Num);
            }
            Ok(number_to_text_in(n, &c.locale().regional))
        }
        Value::Empty => Ok(String::new()),
        v => Ok(crate::util::to_str(c, &v)?.trim().to_string()),
    }
}

/// Parses a 10-digit two's-complement number in `radix`.
fn parse_base(s: &str, radix: u32) -> R<i64> {
    if s.len() > 10 {
        return Err(CellError::Num);
    }
    if s.is_empty() {
        return Ok(0);
    }
    let v = i64::from_str_radix(s, radix).map_err(|_| CellError::Num)?;
    if s.starts_with(['+', '-']) {
        return Err(CellError::Num);
    }
    let bits = match radix {
        2 => 10,
        8 => 30,
        _ => 40,
    };
    // A full 10-digit number with the top bit set is negative.
    if s.len() == 10 && v >= 1i64 << (bits - 1) { Ok(v - (1i64 << bits)) } else { Ok(v) }
}

/// Writes `v` in `radix`, using 10-digit two's complement for negatives and padding to `places`.
fn format_base(v: i64, radix: u32, places: Option<f64>) -> R<Value> {
    let bits: u32 = match radix {
        2 => 10,
        8 => 30,
        _ => 40,
    };
    let min = -(1i64 << (bits - 1));
    let max = (1i64 << (bits - 1)) - 1;
    if v < min || v > max {
        return Err(CellError::Num);
    }
    if v < 0 {
        let u = v + (1i64 << bits);
        return text_val(to_radix(u, radix));
    }
    let s = to_radix(v, radix);
    match places {
        None => text_val(s),
        Some(p) => {
            let p = p.trunc();
            if !(1.0..=10.0).contains(&p) {
                return Err(CellError::Num);
            }
            let p = p as usize;
            if s.len() > p {
                return Err(CellError::Num);
            }
            text_val(format!("{}{}", "0".repeat(p - s.len()), s))
        }
    }
}

fn to_radix(mut v: i64, radix: u32) -> String {
    if v == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while v > 0 {
        let d = (v % radix as i64) as u32;
        out.push(char::from_digit(d, radix).unwrap_or('0').to_ascii_uppercase());
        v /= radix as i64;
    }
    out.iter().rev().collect()
}

fn places_arg(c: &dyn Ctx, a: &[Arg]) -> R<Option<f64>> {
    if has(a, 1) { Ok(Some(num(c, a, 1)?)) } else { Ok(None) }
}

fn convert_base(c: &dyn Ctx, a: &[Arg], from: u32, to: u32) -> R<Value> {
    let s = base_source(c, a)?;
    let v = parse_base(&s, from)?;
    let places = places_arg(c, a)?;
    if to == 10 {
        return Ok(Value::Number(v as f64));
    }
    format_base(v, to, places)
}

fn dec_to(c: &dyn Ctx, a: &[Arg], to: u32) -> R<Value> {
    let v = a.first().map(scalar).unwrap_or(Value::Empty);
    if matches!(v, Value::Bool(_)) {
        return Err(CellError::Value);
    }
    let n = crate::util::to_num(c, &v)?.trunc();
    if n.abs() > 1e15 {
        return Err(CellError::Num);
    }
    format_base(n as i64, to, places_arg(c, a)?)
}

macro_rules! base_fn {
    ($name:ident, $from:expr, $to:expr) => {
        fn $name(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
            convert_base(c, a, $from, $to)
        }
    };
}
base_fn!(bin2dec, 2, 10);
base_fn!(bin2hex, 2, 16);
base_fn!(bin2oct, 2, 8);
base_fn!(hex2bin, 16, 2);
base_fn!(hex2dec, 16, 10);
base_fn!(hex2oct, 16, 8);
base_fn!(oct2bin, 8, 2);
base_fn!(oct2dec, 8, 10);
base_fn!(oct2hex, 8, 16);

fn dec2bin(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    dec_to(c, a, 2)
}
fn dec2oct(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    dec_to(c, a, 8)
}
fn dec2hex(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    dec_to(c, a, 16)
}

// ---------------------------------------------------------------------------------------------
// Bits

const BIT_LIMIT: f64 = 281_474_976_710_656.0; // 2^48

fn bit_operand(c: &dyn Ctx, a: &[Arg], i: usize) -> R<u64> {
    let n = num(c, a, i)?;
    if !(0.0..BIT_LIMIT).contains(&n) || n != n.trunc() {
        return Err(CellError::Num);
    }
    Ok(n as u64)
}

fn bitand(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    Ok(Value::Number((bit_operand(c, a, 0)? & bit_operand(c, a, 1)?) as f64))
}
fn bitor(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    Ok(Value::Number((bit_operand(c, a, 0)? | bit_operand(c, a, 1)?) as f64))
}
fn bitxor(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    Ok(Value::Number((bit_operand(c, a, 0)? ^ bit_operand(c, a, 1)?) as f64))
}

fn shift(c: &dyn Ctx, a: &[Arg], left: bool) -> R<Value> {
    let n = bit_operand(c, a, 0)?;
    let s = num(c, a, 1)?.trunc();
    if s.abs() > 53.0 {
        return Err(CellError::Num);
    }
    let s = if left { s as i64 } else { -(s as i64) };
    let r = if s >= 0 {
        let r = (n as u128) << s;
        if r >= BIT_LIMIT as u128 {
            return Err(CellError::Num);
        }
        r as u64
    } else {
        n >> (-s)
    };
    Ok(Value::Number(r as f64))
}
fn bitlshift(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    shift(c, a, true)
}
fn bitrshift(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    shift(c, a, false)
}

// ---------------------------------------------------------------------------------------------
// CONVERT

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dim {
    Mass,
    Length,
    Time,
    Pressure,
    Force,
    Energy,
    Power,
    Magnetism,
    Temperature,
    Volume,
    Area,
    Information,
    Speed,
}

/// How a unit accepts prefixes: none, SI (raised to `power` for areas/volumes) or SI + binary.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pre {
    No,
    Si(i32),
    Bin,
}

struct Unit {
    names: &'static [&'static str],
    dim: Dim,
    factor: f64,
    pre: Pre,
}

const fn u(names: &'static [&'static str], dim: Dim, factor: f64, pre: Pre) -> Unit {
    Unit { names, dim, factor, pre }
}

const IN: f64 = 0.0254;
const FT: f64 = 0.3048;
const YD: f64 = 0.9144;
const MI: f64 = 1609.344;
const NMI: f64 = 1852.0;
const LY: f64 = 9_460_730_472_580_800.0;
const PICA_PT: f64 = 0.0254 / 72.0;
const PICA: f64 = 0.0254 / 6.0;
const GAL: f64 = 0.003_785_411_784;

const UNITS: &[Unit] = &[
    // Mass (gram)
    u(&["g"], Dim::Mass, 1.0, Pre::Si(1)),
    u(&["sg"], Dim::Mass, 14_593.902_937_206_4, Pre::No),
    u(&["lbm"], Dim::Mass, 453.592_37, Pre::No),
    u(&["u"], Dim::Mass, 1.660_539_066_6e-24, Pre::Si(1)),
    u(&["ozm"], Dim::Mass, 28.349_523_125, Pre::No),
    u(&["grain"], Dim::Mass, 0.064_798_91, Pre::No),
    u(&["cwt", "shweight"], Dim::Mass, 45_359.237, Pre::No),
    u(&["uk_cwt", "lcwt", "hweight"], Dim::Mass, 50_802.345_44, Pre::No),
    u(&["stone"], Dim::Mass, 6_350.293_18, Pre::No),
    u(&["ton"], Dim::Mass, 907_184.74, Pre::No),
    u(&["uk_ton", "LTON", "brton"], Dim::Mass, 1_016_046.908_8, Pre::No),
    // Length (metre)
    u(&["m"], Dim::Length, 1.0, Pre::Si(1)),
    u(&["mi"], Dim::Length, MI, Pre::No),
    u(&["Nmi"], Dim::Length, NMI, Pre::No),
    u(&["in"], Dim::Length, IN, Pre::No),
    u(&["ft"], Dim::Length, FT, Pre::No),
    u(&["yd"], Dim::Length, YD, Pre::No),
    u(&["ang"], Dim::Length, 1e-10, Pre::Si(1)),
    u(&["ell"], Dim::Length, 1.143, Pre::No),
    u(&["ly"], Dim::Length, LY, Pre::Si(1)),
    u(&["parsec", "pc"], Dim::Length, 30_856_775_814_913_673.0, Pre::Si(1)),
    u(&["Picapt", "Pica"], Dim::Length, PICA_PT, Pre::No),
    u(&["pica"], Dim::Length, PICA, Pre::No),
    u(&["survey_mi"], Dim::Length, 1_609.347_218_694_437, Pre::No),
    // Time (second)
    u(&["yr"], Dim::Time, 31_557_600.0, Pre::No),
    u(&["day", "d"], Dim::Time, 86_400.0, Pre::No),
    u(&["hr"], Dim::Time, 3_600.0, Pre::No),
    u(&["mn", "min"], Dim::Time, 60.0, Pre::No),
    u(&["sec", "s"], Dim::Time, 1.0, Pre::Si(1)),
    // Pressure (pascal)
    u(&["Pa", "p"], Dim::Pressure, 1.0, Pre::Si(1)),
    u(&["atm", "at"], Dim::Pressure, 101_325.0, Pre::Si(1)),
    u(&["mmHg"], Dim::Pressure, 133.322_387_415, Pre::Si(1)),
    u(&["psi"], Dim::Pressure, 6_894.757_293_168_36, Pre::No),
    u(&["Torr"], Dim::Pressure, 101_325.0 / 760.0, Pre::No),
    // Force (newton)
    u(&["N"], Dim::Force, 1.0, Pre::Si(1)),
    u(&["dyn", "dy"], Dim::Force, 1e-5, Pre::Si(1)),
    u(&["lbf"], Dim::Force, 4.448_221_615_260_5, Pre::No),
    u(&["pond"], Dim::Force, 0.009_806_65, Pre::Si(1)),
    // Energy (joule)
    u(&["J"], Dim::Energy, 1.0, Pre::Si(1)),
    u(&["e"], Dim::Energy, 1e-7, Pre::Si(1)),
    u(&["c"], Dim::Energy, 4.184, Pre::Si(1)),
    u(&["cal"], Dim::Energy, 4.1868, Pre::Si(1)),
    u(&["eV", "ev"], Dim::Energy, 1.602_176_634e-19, Pre::Si(1)),
    u(&["HPh", "hh"], Dim::Energy, 2_684_519.537_696_17, Pre::No),
    u(&["Wh", "wh"], Dim::Energy, 3_600.0, Pre::Si(1)),
    u(&["flb"], Dim::Energy, 1.355_817_948_331_4, Pre::No),
    u(&["BTU", "btu"], Dim::Energy, 1_055.055_852_62, Pre::No),
    // Power (watt)
    u(&["W", "w"], Dim::Power, 1.0, Pre::Si(1)),
    u(&["HP", "h"], Dim::Power, 745.699_871_582_270_2, Pre::No),
    u(&["PS"], Dim::Power, 735.498_75, Pre::No),
    // Magnetism (tesla)
    u(&["T"], Dim::Magnetism, 1.0, Pre::Si(1)),
    u(&["ga"], Dim::Magnetism, 1e-4, Pre::Si(1)),
    // Temperature (handled specially; factor unused)
    u(&["C", "cel"], Dim::Temperature, 1.0, Pre::No),
    u(&["F", "fah"], Dim::Temperature, 1.0, Pre::No),
    u(&["K", "kel"], Dim::Temperature, 1.0, Pre::Si(1)),
    u(&["Rank"], Dim::Temperature, 1.0, Pre::No),
    u(&["Reau"], Dim::Temperature, 1.0, Pre::No),
    // Volume (cubic metre)
    u(&["l", "L", "lt"], Dim::Volume, 0.001, Pre::Si(1)),
    u(&["tsp"], Dim::Volume, GAL / 768.0, Pre::No),
    u(&["tspm"], Dim::Volume, 5e-6, Pre::No),
    u(&["tbs"], Dim::Volume, GAL / 256.0, Pre::No),
    u(&["oz"], Dim::Volume, GAL / 128.0, Pre::No),
    u(&["cup"], Dim::Volume, GAL / 16.0, Pre::No),
    u(&["pt", "us_pt"], Dim::Volume, GAL / 8.0, Pre::No),
    u(&["uk_pt"], Dim::Volume, 0.000_568_261_25, Pre::No),
    u(&["qt"], Dim::Volume, GAL / 4.0, Pre::No),
    u(&["uk_qt"], Dim::Volume, 0.001_136_522_5, Pre::No),
    u(&["gal"], Dim::Volume, GAL, Pre::No),
    u(&["uk_gal"], Dim::Volume, 0.004_546_09, Pre::No),
    u(&["m3"], Dim::Volume, 1.0, Pre::Si(3)),
    u(&["mi3"], Dim::Volume, MI * MI * MI, Pre::No),
    u(&["Nmi3"], Dim::Volume, NMI * NMI * NMI, Pre::No),
    u(&["yd3"], Dim::Volume, YD * YD * YD, Pre::No),
    u(&["ft3"], Dim::Volume, FT * FT * FT, Pre::No),
    u(&["in3"], Dim::Volume, IN * IN * IN, Pre::No),
    u(&["ang3"], Dim::Volume, 1e-30, Pre::Si(3)),
    u(&["ly3"], Dim::Volume, LY * LY * LY, Pre::No),
    u(&["Picapt3", "Pica3"], Dim::Volume, PICA_PT * PICA_PT * PICA_PT, Pre::No),
    u(&["barrel"], Dim::Volume, 0.158_987_294_928, Pre::No),
    u(&["bushel"], Dim::Volume, 0.035_239_070_166_88, Pre::No),
    u(&["MTON"], Dim::Volume, 1.132_673_863_68, Pre::No),
    u(&["GRT", "regton"], Dim::Volume, 2.831_684_659_2, Pre::No),
    // Area (square metre)
    u(&["m2"], Dim::Area, 1.0, Pre::Si(2)),
    u(&["ha"], Dim::Area, 10_000.0, Pre::No),
    u(&["ar"], Dim::Area, 100.0, Pre::Si(1)),
    u(&["ft2"], Dim::Area, FT * FT, Pre::No),
    u(&["in2"], Dim::Area, IN * IN, Pre::No),
    u(&["yd2"], Dim::Area, YD * YD, Pre::No),
    u(&["mi2"], Dim::Area, MI * MI, Pre::No),
    u(&["Nmi2"], Dim::Area, NMI * NMI, Pre::No),
    u(&["ang2"], Dim::Area, 1e-20, Pre::Si(2)),
    u(&["ly2"], Dim::Area, LY * LY, Pre::No),
    u(&["Picapt2", "Pica2"], Dim::Area, PICA_PT * PICA_PT, Pre::No),
    u(&["uk_acre"], Dim::Area, 4_046.856_422_4, Pre::No),
    u(&["us_acre"], Dim::Area, 4_046.872_609_874_252, Pre::No),
    u(&["Morgen"], Dim::Area, 2_500.0, Pre::No),
    // Information (bit)
    u(&["bit"], Dim::Information, 1.0, Pre::Bin),
    u(&["byte"], Dim::Information, 8.0, Pre::Bin),
    // Speed (metres per second)
    u(&["m/s", "m/sec"], Dim::Speed, 1.0, Pre::Si(1)),
    u(&["m/h", "m/hr"], Dim::Speed, 1.0 / 3600.0, Pre::Si(1)),
    u(&["mph"], Dim::Speed, MI / 3600.0, Pre::No),
    u(&["kn"], Dim::Speed, NMI / 3600.0, Pre::No),
    u(&["admkn"], Dim::Speed, 6080.0 * FT / 3600.0, Pre::No),
];

const SI_PREFIXES: &[(&str, f64)] = &[
    ("da", 1e1),
    ("Y", 1e24),
    ("Z", 1e21),
    ("E", 1e18),
    ("P", 1e15),
    ("T", 1e12),
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("h", 1e2),
    ("e", 1e1),
    ("d", 1e-1),
    ("c", 1e-2),
    ("m", 1e-3),
    ("u", 1e-6),
    ("n", 1e-9),
    ("p", 1e-12),
    ("f", 1e-15),
    ("a", 1e-18),
    ("z", 1e-21),
    ("y", 1e-24),
];

const BIN_PREFIXES: &[(&str, f64)] = &[
    ("ki", 1024.0),
    ("Mi", 1_048_576.0),
    ("Gi", 1_073_741_824.0),
    ("Ti", 1_099_511_627_776.0),
    ("Pi", 1_125_899_906_842_624.0),
    ("Ei", 1_152_921_504_606_846_976.0),
    ("Zi", 1_180_591_620_717_411_303_424.0),
    ("Yi", 1_208_925_819_614_629_174_706_176.0),
];

fn find_unit(name: &str) -> Option<&'static Unit> {
    UNITS.iter().find(|u| u.names.contains(&name))
}

/// Resolves a unit name to (unit, prefix multiplier). Exact names win over prefixed ones.
fn resolve(name: &str) -> Option<(&'static Unit, f64)> {
    let name = name.replace("^2", "2").replace("^3", "3");
    if let Some(u) = find_unit(&name) {
        return Some((u, 1.0));
    }
    for (p, f) in BIN_PREFIXES {
        if let Some(rest) = name.strip_prefix(p)
            && let Some(u) = find_unit(rest)
            && u.pre == Pre::Bin
        {
            return Some((u, *f));
        }
    }
    for (p, f) in SI_PREFIXES {
        if let Some(rest) = name.strip_prefix(p)
            && let Some(u) = find_unit(rest)
        {
            match u.pre {
                Pre::Si(pow) => return Some((u, f.powi(pow))),
                Pre::Bin => return Some((u, *f)),
                Pre::No => {}
            }
        }
    }
    None
}

fn to_kelvin(name: &str, x: f64) -> f64 {
    match name {
        "C" | "cel" => x + 273.15,
        "F" | "fah" => (x - 32.0) * 5.0 / 9.0 + 273.15,
        "Rank" => x * 5.0 / 9.0,
        "Reau" => x * 1.25 + 273.15,
        _ => x,
    }
}

fn from_kelvin(name: &str, k: f64) -> f64 {
    match name {
        "C" | "cel" => k - 273.15,
        "F" | "fah" => (k - 273.15) * 9.0 / 5.0 + 32.0,
        "Rank" => k * 9.0 / 5.0,
        "Reau" => (k - 273.15) / 1.25,
        _ => k,
    }
}

fn convert(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let x = num(c, a, 0)?;
    let from = text(c, a, 1)?;
    let to = text(c, a, 2)?;
    let (fu, fp) = resolve(&from).ok_or(CellError::NA)?;
    let (tu, tp) = resolve(&to).ok_or(CellError::NA)?;
    if fu.dim != tu.dim {
        return Err(CellError::NA);
    }
    if fu.dim == Dim::Temperature {
        let fname = fu.names.first().copied().unwrap_or("K");
        let tname = tu.names.first().copied().unwrap_or("K");
        if fname == tname && fp == tp {
            return num_val(x);
        }
        let k = to_kelvin(fname, x * fp);
        return num_val(from_kelvin(tname, k) / tp);
    }
    let base = x * fu.factor * fp;
    num_val(base / (tu.factor * tp))
}

// ---------------------------------------------------------------------------------------------
// DELTA, GESTEP, ERF

fn delta(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let x = num(c, a, 0)?;
    let y = opt_num(c, a, 1, 0.0)?;
    Ok(Value::Number(if x == y { 1.0 } else { 0.0 }))
}
fn gestep(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let x = num(c, a, 0)?;
    let y = opt_num(c, a, 1, 0.0)?;
    Ok(Value::Number(if x >= y { 1.0 } else { 0.0 }))
}
fn erf_fn(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let lo = num(c, a, 0)?;
    if has(a, 1) {
        let hi = num(c, a, 1)?;
        return num_val(erf(hi) - erf(lo));
    }
    num_val(erf(lo))
}
fn erf_precise(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(erf(num(c, a, 0)?))
}
fn erfc_fn(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(erfc(num(c, a, 0)?))
}

// ---------------------------------------------------------------------------------------------
// Complex numbers

#[derive(Clone, Copy, Debug, PartialEq)]
struct Cx {
    re: f64,
    im: f64,
}

impl Cx {
    fn new(re: f64, im: f64) -> Cx {
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
    fn div(self, o: Cx) -> R<Cx> {
        let d = o.re * o.re + o.im * o.im;
        if d == 0.0 {
            return Err(CellError::Num);
        }
        Ok(Cx::new((self.re * o.re + self.im * o.im) / d, (self.im * o.re - self.re * o.im) / d))
    }
    fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
    fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }
    fn exp(self) -> Cx {
        let e = self.re.exp();
        Cx::new(e * self.im.cos(), e * self.im.sin())
    }
    fn ln(self) -> R<Cx> {
        if self.re == 0.0 && self.im == 0.0 {
            return Err(CellError::Num);
        }
        Ok(Cx::new(self.abs().ln(), self.arg()))
    }
    fn sin(self) -> Cx {
        Cx::new(self.re.sin() * self.im.cosh(), self.re.cos() * self.im.sinh())
    }
    fn cos(self) -> Cx {
        Cx::new(self.re.cos() * self.im.cosh(), -self.re.sin() * self.im.sinh())
    }
    fn sinh(self) -> Cx {
        Cx::new(self.re.sinh() * self.im.cos(), self.re.cosh() * self.im.sin())
    }
    fn cosh(self) -> Cx {
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

const ONE: Cx = Cx { re: 1.0, im: 0.0 };

/// Longest text read as a complex number.
const MAX_COMPLEX_TEXT: usize = 255;

/// Parses a real number written in a complex string (no spaces, optional exponent).
fn parse_real(s: &str, decimal: char) -> Option<f64> {
    if s.is_empty() || s.contains(char::is_whitespace) {
        return None;
    }
    let plain = |c: char| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-');
    if decimal == '.' {
        return if s.chars().all(plain) { s.parse::<f64>().ok().filter(|v| v.is_finite()) } else { None };
    }
    let mut canonical = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == decimal {
            canonical.push('.');
        } else if ch == '.' {
            return None;
        } else {
            canonical.push(ch);
        }
    }
    if !canonical.chars().all(plain) {
        return None;
    }
    canonical.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Parses "a+bi" forms. Returns the number and its suffix ('i', 'j' or none).
fn parse_complex(c: &dyn Ctx, s: &str) -> R<(Cx, Option<char>)> {
    let decimal = c.locale().regional.decimal;
    if s.is_empty() {
        return Ok((Cx::new(0.0, 0.0), None));
    }
    if s.len() > MAX_COMPLEX_TEXT {
        return Err(CellError::Num);
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

fn cx_of(c: &dyn Ctx, v: &Value) -> R<(Cx, Option<char>)> {
    match v {
        Value::Number(n) => Ok((Cx::new(*n, 0.0), None)),
        Value::Empty => Ok((Cx::new(0.0, 0.0), None)),
        Value::Bool(_) => Err(CellError::Value),
        Value::Error(e) => Err(*e),
        Value::Text(t) => parse_complex(c, t),
        Value::Array(a) => cx_of(c, a.data.first().unwrap_or(&Value::Empty)),
    }
}

fn cx_arg(c: &dyn Ctx, a: &[Arg], i: usize) -> R<(Cx, Option<char>)> {
    cx_of(c, &a.get(i).map(scalar).unwrap_or(Value::Empty))
}

fn merge_suffix(acc: Option<char>, s: Option<char>) -> R<Option<char>> {
    match (acc, s) {
        (Some(x), Some(y)) if x != y => Err(CellError::Value),
        (Some(x), _) => Ok(Some(x)),
        (None, y) => Ok(y),
    }
}

fn fmt_complex(c: &dyn Ctx, z: Cx, suffix: char) -> R<Value> {
    if !z.re.is_finite() || !z.im.is_finite() {
        return Err(CellError::Num);
    }
    let re = clean(z.re);
    let im = clean(z.im);
    let mut s = String::new();
    if im == 0.0 {
        s.push_str(&number_to_text_in(re, &c.locale().regional));
        return text_val(s);
    }
    if re != 0.0 {
        s.push_str(&number_to_text_in(re, &c.locale().regional));
        if im > 0.0 {
            s.push('+');
        }
    }
    if im == -1.0 {
        s.push('-');
    } else if im != 1.0 {
        s.push_str(&number_to_text_in(im, &c.locale().regional));
    }
    s.push(suffix);
    text_val(s)
}

/// Rounds to 15 significant digits so values like 0.999999999999999999 print as 1.
fn clean(x: f64) -> f64 {
    let s = format!("{:.14e}", x);
    s.parse().unwrap_or(x)
}

fn complex(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let re = num(c, a, 0)?;
    let im = num(c, a, 1)?;
    let sfx = if has(a, 2) { text(c, a, 2)? } else { "i".into() };
    let sfx = match sfx.as_str() {
        "i" | "" => 'i',
        "j" => 'j',
        _ => return Err(CellError::Value),
    };
    fmt_complex(c, Cx::new(re, im), sfx)
}

fn im_unary(c: &dyn Ctx, a: &[Arg], f: fn(Cx) -> R<Cx>) -> R<Value> {
    let (z, s) = cx_arg(c, a, 0)?;
    fmt_complex(c, f(z)?, s.unwrap_or('i'))
}

fn im_real_fn(c: &dyn Ctx, a: &[Arg], f: fn(Cx) -> R<f64>) -> R<Value> {
    let (z, _) = cx_arg(c, a, 0)?;
    num_val(f(z)?)
}

macro_rules! im1 {
    ($name:ident, $f:expr) => {
        fn $name(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
            im_unary(c, a, $f)
        }
    };
}

im1!(imconjugate, |z: Cx| Ok(Cx::new(z.re, -z.im)));
im1!(imexp, |z: Cx| Ok(z.exp()));
im1!(imln, |z: Cx| z.ln());
im1!(imlog10, |z: Cx| z.ln().map(|l| Cx::new(l.re / std::f64::consts::LN_10, l.im / std::f64::consts::LN_10)));
im1!(imlog2, |z: Cx| z.ln().map(|l| Cx::new(l.re / std::f64::consts::LN_2, l.im / std::f64::consts::LN_2)));
im1!(imsqrt, |z: Cx| z.powf(0.5));
im1!(imsin, |z: Cx| Ok(z.sin()));
im1!(imcos, |z: Cx| Ok(z.cos()));
im1!(imtan, |z: Cx| z.sin().div(z.cos()));
im1!(imcot, |z: Cx| z.cos().div(z.sin()));
im1!(imsec, |z: Cx| ONE.div(z.cos()));
im1!(imcsc, |z: Cx| ONE.div(z.sin()));
im1!(imsinh, |z: Cx| Ok(z.sinh()));
im1!(imcosh, |z: Cx| Ok(z.cosh()));
im1!(imsech, |z: Cx| ONE.div(z.cosh()));
im1!(imcsch, |z: Cx| ONE.div(z.sinh()));

fn imabs(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_real_fn(c, a, |z| Ok(z.abs()))
}
fn imreal(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_real_fn(c, a, |z| Ok(z.re))
}
fn imaginary(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_real_fn(c, a, |z| Ok(z.im))
}
fn imargument(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_real_fn(c, a, |z| if z.re == 0.0 && z.im == 0.0 { Err(CellError::Div0) } else { Ok(z.arg()) })
}

fn im_binary(c: &dyn Ctx, a: &[Arg], f: fn(Cx, Cx) -> R<Cx>) -> R<Value> {
    let (x, s1) = cx_arg(c, a, 0)?;
    let (y, s2) = cx_arg(c, a, 1)?;
    let s = merge_suffix(s1, s2)?;
    fmt_complex(c, f(x, y)?, s.unwrap_or('i'))
}
fn imdiv(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_binary(c, a, |x, y| x.div(y))
}
fn imsub(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_binary(c, a, |x, y| Ok(x.sub(y)))
}
fn impower(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (z, s) = cx_arg(c, a, 0)?;
    let n = num(c, a, 1)?;
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
    fmt_complex(c, r, s.unwrap_or('i'))
}

fn im_fold(c: &dyn Ctx, a: &[Arg], init: Cx, f: fn(Cx, Cx) -> Cx) -> R<Value> {
    let mut acc = init;
    let mut sfx = None;
    for arg in a {
        let direct = !matches!(arg.value, Value::Array(_));
        for v in flatten(std::slice::from_ref(arg)) {
            if matches!(v, Value::Empty) && !direct {
                continue;
            }
            let (z, s) = cx_of(c, &v)?;
            sfx = merge_suffix(sfx, s)?;
            acc = f(acc, z);
        }
    }
    fmt_complex(c, acc, sfx.unwrap_or('i'))
}
fn imsum(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_fold(c, a, Cx::new(0.0, 0.0), Cx::add)
}
fn improduct(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    im_fold(c, a, ONE, Cx::mul)
}

// ---------------------------------------------------------------------------------------------
// Bessel functions

const MAX_BESSEL_POINTS: f64 = 4_000_000.0;

/// J_n(x) for integer n ≥ 0 via Bessel's integral, J_n(x) = (1/2π)∫₀^{2π} cos(nτ − x sin τ) dτ,
/// evaluated with the trapezoid rule, which converges exponentially for periodic integrands.
fn bessel_j(n: u32, x: f64) -> Option<f64> {
    let pts = (2.0 * (x.abs() + n as f64) + 64.0).ceil();
    if pts > MAX_BESSEL_POINTS {
        return if x.abs() > 1e6 { hankel(n, x.abs()).map(|(j, _)| if x < 0.0 && n % 2 == 1 { -j } else { j }) } else { None };
    }
    let m = pts as usize;
    let h = 2.0 * PI / m as f64;
    let nf = n as f64;
    let mut sum = 0.0;
    for k in 0..m {
        let t = k as f64 * h;
        sum += (nf * t - x * t.sin()).cos();
    }
    Some(sum / m as f64)
}

/// Hankel asymptotic expansion: (J_n(x), Y_n(x)) for large x.
fn hankel(n: u32, x: f64) -> Option<(f64, f64)> {
    let mu = 4.0 * (n as f64) * (n as f64);
    let mut p = 0.0;
    let mut q = 0.0;
    let mut term: f64 = 1.0;
    let mut last = f64::INFINITY;
    for k in 0..200 {
        if term.abs() > last {
            break;
        }
        let t = if k % 4 < 2 { term } else { -term };
        if k % 2 == 0 {
            p += t;
        } else {
            q += t;
        }
        last = term.abs();
        if last < 1e-17 {
            break;
        }
        let kk = (k + 1) as f64;
        let odd = 2.0 * kk - 1.0;
        term *= (mu - odd * odd) / (kk * 8.0 * x);
    }
    let chi = x - (n as f64 / 2.0 + 0.25) * PI;
    let f = (2.0 / (PI * x)).sqrt();
    let j = f * (p * chi.cos() - q * chi.sin());
    let y = f * (p * chi.sin() + q * chi.cos());
    if j.is_finite() && y.is_finite() { Some((j, y)) } else { None }
}

const EULER: f64 = std::f64::consts::EULER_GAMMA;

/// J_0..J_m(x) for 0 < x ≤ ~100 by Miller's backward recurrence, normalised with
/// J_0 + 2ΣJ_{2k} = 1.
fn bessel_j_all(x: f64) -> Vec<f64> {
    let top = x.ceil() as usize + 30 + (40.0 * x.max(1.0)).sqrt() as usize;
    let m = top + top % 2;
    let mut j = vec![0.0f64; m + 2];
    if let Some(v) = j.get_mut(m) {
        *v = 1e-30;
    }
    for k in (1..=m).rev() {
        let jk = j.get(k).copied().unwrap_or(0.0);
        let jk1 = j.get(k + 1).copied().unwrap_or(0.0);
        let mut prev = 2.0 * k as f64 / x * jk - jk1;
        if prev.abs() > 1e250 {
            for v in j.iter_mut() {
                *v *= 1e-250;
            }
            prev *= 1e-250;
        }
        if let Some(v) = j.get_mut(k - 1) {
            *v = prev;
        }
    }
    let norm: f64 = j.first().copied().unwrap_or(0.0) + 2.0 * j.iter().skip(2).step_by(2).sum::<f64>();
    if norm != 0.0 {
        for v in j.iter_mut() {
            *v /= norm;
        }
    }
    j
}

/// Y_0 and Y_1 by Neumann series over J_k (x ≤ 25) or the asymptotic expansion.
fn bessel_y01(x: f64) -> Option<(f64, f64)> {
    if x > 25.0 {
        let (_, y0) = hankel(0, x)?;
        let (_, y1) = hankel(1, x)?;
        return Some((y0, y1));
    }
    let j = bessel_j_all(x);
    let jj = |k: usize| j.get(k).copied().unwrap_or(0.0);
    let l = (x / 2.0).ln() + EULER;
    // Y0 = (2/π)(L J0) − (4/π) Σ (−1)^k J_{2k}/k
    // Y1 = −Y0' = −(2/π)(J0/x − L J1) + (2/π) Σ (−1)^k (J_{2k−1} − J_{2k+1})/k
    let mut s0 = 0.0;
    let mut s1 = 0.0;
    let mut k = 1usize;
    while 2 * k + 1 < j.len() {
        let sign = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
        s0 += sign * jj(2 * k) / k as f64;
        s1 += sign * (jj(2 * k - 1) - jj(2 * k + 1)) / k as f64;
        k += 1;
    }
    let y0 = (2.0 / PI) * l * jj(0) - (4.0 / PI) * s0;
    let y1 = -(2.0 / PI) * (jj(0) / x - l * jj(1)) + (2.0 / PI) * s1;
    if y0.is_finite() && y1.is_finite() { Some((y0, y1)) } else { None }
}

fn bessel_y(n: u32, x: f64) -> Option<f64> {
    let (y0, y1) = bessel_y01(x)?;
    if n == 0 {
        return Some(y0);
    }
    let (mut a, mut b) = (y0, y1);
    for k in 1..n {
        let c = 2.0 * k as f64 / x * b - a;
        a = b;
        b = c;
        if !b.is_finite() {
            return None;
        }
    }
    Some(b)
}

/// I_n(x) by its power series (all terms positive), computed with a log-scaled first term.
fn bessel_i(n: u32, x: f64) -> Option<f64> {
    if x == 0.0 {
        return Some(if n == 0 { 1.0 } else { 0.0 });
    }
    let ax = x.abs();
    let nf = n as f64;
    let ln_t0 = nf * (ax / 2.0).ln() - crate::special::ln_gamma(nf + 1.0);
    let q = ax * ax / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    let mut k = 0.0;
    while k < 1e6 {
        k += 1.0;
        term *= q / (k * (k + nf));
        sum += term;
        if term < 1e-17 * sum {
            break;
        }
        if !sum.is_finite() {
            return None;
        }
    }
    let r = (ln_t0 + sum.ln()).exp();
    let r = if x < 0.0 && n % 2 == 1 { -r } else { r };
    if r.is_finite() { Some(r) } else { None }
}

/// K_n(x) for x > 0: K_0 and K_1 from ∫₀^∞ e^{−x cosh t} cosh(νt) dt (trapezoid rule), then
/// upward recurrence.
fn bessel_k(n: u32, x: f64) -> Option<f64> {
    let k_int = |nu: f64| -> f64 {
        let h: f64 = 0.02;
        let mut sum = 0.5 * (-x).exp();
        let mut t: f64 = h;
        while t < 800.0 {
            let v = (-x * t.cosh() + (nu * t).ln_cosh_safe()).exp();
            sum += v;
            if v < 1e-18 * sum && x * t.cosh() > 50.0 {
                break;
            }
            t += h;
        }
        sum * h
    };
    let k0 = k_int(0.0);
    if n == 0 {
        return Some(k0);
    }
    let k1 = k_int(1.0);
    let (mut a, mut b) = (k0, k1);
    for k in 1..n {
        let c = a + 2.0 * k as f64 / x * b;
        a = b;
        b = c;
        if !b.is_finite() {
            return None;
        }
    }
    if b.is_finite() { Some(b) } else { None }
}

trait LnCosh {
    fn ln_cosh_safe(self) -> f64;
}
impl LnCosh for f64 {
    /// ln(cosh(x)) without overflow.
    fn ln_cosh_safe(self) -> f64 {
        let a = self.abs();
        a + (-2.0 * a).exp().ln_1p() - std::f64::consts::LN_2
    }
}

fn bessel_args(c: &dyn Ctx, a: &[Arg]) -> R<(f64, u32)> {
    let x = num(c, a, 0)?;
    let n = num(c, a, 1)?.trunc();
    if n < 0.0 {
        return Err(CellError::Num);
    }
    if n > 100_000.0 {
        return Err(CellError::Num);
    }
    Ok((x, n as u32))
}

fn besselj(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (x, n) = bessel_args(c, a)?;
    num_val(bessel_j(n, x).ok_or(CellError::Num)?)
}
fn bessely(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (x, n) = bessel_args(c, a)?;
    if x <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(bessel_y(n, x).ok_or(CellError::Num)?)
}
fn besseli(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (x, n) = bessel_args(c, a)?;
    num_val(bessel_i(n, x).ok_or(CellError::Num)?)
}
fn besselk(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (x, n) = bessel_args(c, a)?;
    if x <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(bessel_k(n, x).ok_or(CellError::Num)?)
}

pub(crate) fn specs() -> Vec<FnSpec> {
    vec![
        f!("BIN2DEC", 1, 1, Engineering, S, "BIN2DEC(number)", "Converts a binary number to decimal.", bin2dec),
        f!("BIN2HEX", 1, 2, Engineering, S, "BIN2HEX(number, [places])", "Converts a binary number to hexadecimal.", bin2hex),
        f!("BIN2OCT", 1, 2, Engineering, S, "BIN2OCT(number, [places])", "Converts a binary number to octal.", bin2oct),
        f!("DEC2BIN", 1, 2, Engineering, S, "DEC2BIN(number, [places])", "Converts a decimal number to binary.", dec2bin),
        f!("DEC2HEX", 1, 2, Engineering, S, "DEC2HEX(number, [places])", "Converts a decimal number to hexadecimal.", dec2hex),
        f!("DEC2OCT", 1, 2, Engineering, S, "DEC2OCT(number, [places])", "Converts a decimal number to octal.", dec2oct),
        f!("HEX2BIN", 1, 2, Engineering, S, "HEX2BIN(number, [places])", "Converts a hexadecimal number to binary.", hex2bin),
        f!("HEX2DEC", 1, 1, Engineering, S, "HEX2DEC(number)", "Converts a hexadecimal number to decimal.", hex2dec),
        f!("HEX2OCT", 1, 2, Engineering, S, "HEX2OCT(number, [places])", "Converts a hexadecimal number to octal.", hex2oct),
        f!("OCT2BIN", 1, 2, Engineering, S, "OCT2BIN(number, [places])", "Converts an octal number to binary.", oct2bin),
        f!("OCT2DEC", 1, 1, Engineering, S, "OCT2DEC(number)", "Converts an octal number to decimal.", oct2dec),
        f!("OCT2HEX", 1, 2, Engineering, S, "OCT2HEX(number, [places])", "Converts an octal number to hexadecimal.", oct2hex),
        f!("BITAND", 2, 2, Engineering, S, "BITAND(number1, number2)", "Bitwise AND of two non-negative integers.", bitand),
        f!("BITOR", 2, 2, Engineering, S, "BITOR(number1, number2)", "Bitwise OR of two non-negative integers.", bitor),
        f!("BITXOR", 2, 2, Engineering, S, "BITXOR(number1, number2)", "Bitwise exclusive OR of two non-negative integers.", bitxor),
        f!(
            "BITLSHIFT",
            2,
            2,
            Engineering,
            S,
            "BITLSHIFT(number, shift_amount)",
            "Shifts the bits of a number left (right for negative shifts).",
            bitlshift
        ),
        f!(
            "BITRSHIFT",
            2,
            2,
            Engineering,
            S,
            "BITRSHIFT(number, shift_amount)",
            "Shifts the bits of a number right (left for negative shifts).",
            bitrshift
        ),
        f!("CONVERT", 3, 3, Engineering, S, "CONVERT(number, from_unit, to_unit)", "Converts a measurement from one unit to another.", convert),
        f!("DELTA", 1, 2, Engineering, S, "DELTA(number1, [number2])", "Returns 1 when two numbers are equal, otherwise 0.", delta),
        f!("GESTEP", 1, 2, Engineering, S, "GESTEP(number, [step])", "Returns 1 when a number is at least the step, otherwise 0.", gestep),
        f!(
            "ERF",
            1,
            2,
            Engineering,
            S,
            "ERF(lower_limit, [upper_limit])",
            "Error function integrated from 0 to a limit, or between two limits.",
            erf_fn
        ),
        f!("ERF.PRECISE", 1, 1, Engineering, S, "ERF.PRECISE(x)", "Error function integrated from 0 to x.", erf_precise),
        f!("ERFC", 1, 1, Engineering, S, "ERFC(x)", "Complementary error function, 1 - ERF(x).", erfc_fn),
        f!("ERFC.PRECISE", 1, 1, Engineering, S, "ERFC.PRECISE(x)", "Complementary error function, 1 - ERF(x).", erfc_fn),
        f!(
            "COMPLEX",
            2,
            3,
            Engineering,
            S,
            "COMPLEX(real_num, i_num, [suffix])",
            "Builds a complex number as text from real and imaginary parts.",
            complex
        ),
        f!("IMABS", 1, 1, Engineering, S, "IMABS(inumber)", "Absolute value (modulus) of a complex number.", imabs),
        f!("IMAGINARY", 1, 1, Engineering, S, "IMAGINARY(inumber)", "Imaginary coefficient of a complex number.", imaginary),
        f!("IMREAL", 1, 1, Engineering, S, "IMREAL(inumber)", "Real coefficient of a complex number.", imreal),
        f!("IMARGUMENT", 1, 1, Engineering, S, "IMARGUMENT(inumber)", "Angle of a complex number in radians.", imargument),
        f!("IMCONJUGATE", 1, 1, Engineering, S, "IMCONJUGATE(inumber)", "Complex conjugate of a complex number.", imconjugate),
        f!("IMCOS", 1, 1, Engineering, S, "IMCOS(inumber)", "Cosine of a complex number.", imcos),
        f!("IMCOSH", 1, 1, Engineering, S, "IMCOSH(inumber)", "Hyperbolic cosine of a complex number.", imcosh),
        f!("IMCOT", 1, 1, Engineering, S, "IMCOT(inumber)", "Cotangent of a complex number.", imcot),
        f!("IMCSC", 1, 1, Engineering, S, "IMCSC(inumber)", "Cosecant of a complex number.", imcsc),
        f!("IMCSCH", 1, 1, Engineering, S, "IMCSCH(inumber)", "Hyperbolic cosecant of a complex number.", imcsch),
        f!("IMDIV", 2, 2, Engineering, S, "IMDIV(inumber1, inumber2)", "Quotient of two complex numbers.", imdiv),
        f!("IMEXP", 1, 1, Engineering, S, "IMEXP(inumber)", "Exponential of a complex number.", imexp),
        f!("IMLN", 1, 1, Engineering, S, "IMLN(inumber)", "Natural logarithm of a complex number.", imln),
        f!("IMLOG10", 1, 1, Engineering, S, "IMLOG10(inumber)", "Base-10 logarithm of a complex number.", imlog10),
        f!("IMLOG2", 1, 1, Engineering, S, "IMLOG2(inumber)", "Base-2 logarithm of a complex number.", imlog2),
        f!("IMPOWER", 2, 2, Engineering, S, "IMPOWER(inumber, number)", "Raises a complex number to a power.", impower),
        f!("IMPRODUCT", 1, VAR, Engineering, A, "IMPRODUCT(inumber1, [inumber2], ...)", "Product of complex numbers.", improduct),
        f!("IMSEC", 1, 1, Engineering, S, "IMSEC(inumber)", "Secant of a complex number.", imsec),
        f!("IMSECH", 1, 1, Engineering, S, "IMSECH(inumber)", "Hyperbolic secant of a complex number.", imsech),
        f!("IMSIN", 1, 1, Engineering, S, "IMSIN(inumber)", "Sine of a complex number.", imsin),
        f!("IMSINH", 1, 1, Engineering, S, "IMSINH(inumber)", "Hyperbolic sine of a complex number.", imsinh),
        f!("IMSQRT", 1, 1, Engineering, S, "IMSQRT(inumber)", "Square root of a complex number.", imsqrt),
        f!("IMSUB", 2, 2, Engineering, S, "IMSUB(inumber1, inumber2)", "Difference of two complex numbers.", imsub),
        f!("IMSUM", 1, VAR, Engineering, A, "IMSUM(inumber1, [inumber2], ...)", "Sum of complex numbers.", imsum),
        f!("IMTAN", 1, 1, Engineering, S, "IMTAN(inumber)", "Tangent of a complex number.", imtan),
        f!("BESSELJ", 2, 2, Engineering, S, "BESSELJ(x, n)", "Bessel function of the first kind, J_n(x).", besselj),
        f!("BESSELY", 2, 2, Engineering, S, "BESSELY(x, n)", "Bessel function of the second kind, Y_n(x).", bessely),
        f!("BESSELI", 2, 2, Engineering, S, "BESSELI(x, n)", "Modified Bessel function of the first kind, I_n(x).", besseli),
        f!("BESSELK", 2, 2, Engineering, S, "BESSELK(x, n)", "Modified Bessel function of the second kind, K_n(x).", besselk),
    ]
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellError, Value};

    use super::parse_complex;
    use crate::util::testutil::*;

    #[track_caller]
    fn cx(v: Value, re: f64, im: f64) {
        let Value::Text(s) = &v else { panic!("expected complex text, got {v:?}") };
        let (z, _) = parse_complex(&TestCtx::default(), s).unwrap();
        let tol = 1e-12;
        assert!((z.re - re).abs() <= tol * re.abs().max(1.0), "{s}: re {} vs {re}", z.re);
        assert!((z.im - im).abs() <= tol * im.abs().max(1.0), "{s}: im {} vs {im}", z.im);
    }

    #[test]
    fn base_conversions() {
        close(ev("BIN2DEC", vec![n(1100100.0)]), 100.0);
        close(ev("BIN2DEC", vec![n(1111111111.0)]), -1.0);
        is_text(ev("BIN2HEX", vec![n(11111011.0), n(4.0)]), "00FB");
        is_text(ev("BIN2HEX", vec![n(1110.0)]), "E");
        is_text(ev("BIN2HEX", vec![n(1111111111.0)]), "FFFFFFFFFF");
        is_text(ev("BIN2OCT", vec![n(1001.0), n(3.0)]), "011");
        is_text(ev("BIN2OCT", vec![n(1111111111.0)]), "7777777777");
        is_text(ev("DEC2BIN", vec![n(9.0), n(4.0)]), "1001");
        is_text(ev("DEC2BIN", vec![n(-100.0)]), "1110011100");
        is_err(ev("DEC2BIN", vec![n(512.0)]), CellError::Num);
        is_text(ev("DEC2HEX", vec![n(100.0), n(4.0)]), "0064");
        is_text(ev("DEC2HEX", vec![n(-54.0)]), "FFFFFFFFCA");
        is_text(ev("DEC2HEX", vec![n(28.0)]), "1C");
        is_err(ev("DEC2HEX", vec![n(64.0), n(1.0)]), CellError::Num);
        is_text(ev("DEC2OCT", vec![n(58.0), n(3.0)]), "072");
        is_text(ev("DEC2OCT", vec![n(-100.0)]), "7777777634");
        is_text(ev("HEX2BIN", vec![t("F"), n(8.0)]), "00001111");
        is_text(ev("HEX2BIN", vec![t("B7")]), "10110111");
        is_text(ev("HEX2BIN", vec![t("FFFFFFFE00")]), "1000000000");
        is_err(ev("HEX2BIN", vec![t("200")]), CellError::Num);
        close(ev("HEX2DEC", vec![t("A5")]), 165.0);
        close(ev("HEX2DEC", vec![t("FFFFFFFF5B")]), -165.0);
        close(ev("HEX2DEC", vec![t("3DA408B9")]), 1034160313.0);
        is_text(ev("HEX2OCT", vec![t("F"), n(3.0)]), "017");
        is_text(ev("HEX2OCT", vec![t("3B4E")]), "35516");
        is_text(ev("HEX2OCT", vec![t("FFFFFFFF00")]), "7777777400");
        is_text(ev("OCT2BIN", vec![n(3.0), n(3.0)]), "011");
        is_text(ev("OCT2BIN", vec![n(7777777000.0)]), "1000000000");
        close(ev("OCT2DEC", vec![n(54.0)]), 44.0);
        close(ev("OCT2DEC", vec![n(7777777533.0)]), -165.0);
        is_text(ev("OCT2HEX", vec![n(100.0), n(4.0)]), "0040");
        is_text(ev("OCT2HEX", vec![n(7777777533.0)]), "FFFFFFFF5B");
        is_err(ev("BIN2DEC", vec![n(102.0)]), CellError::Num);
        is_err(ev("HEX2DEC", vec![t("GG")]), CellError::Num);
        is_err(ev("BIN2DEC", vec![t("11111111111")]), CellError::Num);
        is_err(ev("DEC2BIN", vec![t("abc")]), CellError::Value);
        is_text(ev("DEC2BIN", vec![n(9.7)]), "1001");
        close(ev("HEX2DEC", vec![t("ff")]), 255.0);
    }

    #[test]
    fn bits() {
        close(ev("BITAND", vec![n(1.0), n(5.0)]), 1.0);
        close(ev("BITAND", vec![n(13.0), n(25.0)]), 9.0);
        close(ev("BITOR", vec![n(23.0), n(10.0)]), 31.0);
        close(ev("BITXOR", vec![n(5.0), n(3.0)]), 6.0);
        close(ev("BITLSHIFT", vec![n(4.0), n(2.0)]), 16.0);
        close(ev("BITRSHIFT", vec![n(13.0), n(2.0)]), 3.0);
        close(ev("BITRSHIFT", vec![n(13.0), n(-2.0)]), 52.0);
        is_err(ev("BITAND", vec![n(-1.0), n(5.0)]), CellError::Num);
        is_err(ev("BITAND", vec![n(1.5), n(5.0)]), CellError::Num);
        is_err(ev("BITLSHIFT", vec![n(1.0), n(54.0)]), CellError::Num);
        is_err(ev("BITLSHIFT", vec![n(2f64.powi(47)), n(1.0)]), CellError::Num);
    }

    #[test]
    fn convert_units() {
        close(ev("CONVERT", vec![n(1.0), t("lbm"), t("kg")]), 0.45359237);
        close(ev("CONVERT", vec![n(68.0), t("F"), t("C")]), 20.0);
        close(ev("CONVERT", vec![n(100.0), t("C"), t("F")]), 212.0);
        close(ev("CONVERT", vec![n(0.0), t("C"), t("K")]), 273.15);
        close(ev("CONVERT", vec![n(1.0), t("in"), t("cm")]), 2.54);
        close(ev("CONVERT", vec![n(1.0), t("mi"), t("km")]), 1.609344);
        close(ev("CONVERT", vec![n(1.0), t("gal"), t("l")]), 3.785411784);
        close(ev("CONVERT", vec![n(1.0), t("byte"), t("bit")]), 8.0);
        close(ev("CONVERT", vec![n(1.0), t("kibyte"), t("byte")]), 1024.0);
        close(ev("CONVERT", vec![n(100.0), t("km/h"), t("m/s")]), 100.0 / 3.6);
        close(ev("CONVERT", vec![n(1.0), t("m2"), t("ft2")]), 10.763910416709722);
        close(ev("CONVERT", vec![n(1.0), t("km2"), t("m2")]), 1e6);
        close(ev("CONVERT", vec![n(1.0), t("hr"), t("mn")]), 60.0);
        close(ev("CONVERT", vec![n(1.0), t("atm"), t("Pa")]), 101325.0);
        close(ev("CONVERT", vec![n(1.0), t("ha"), t("m^2")]), 10000.0);
        close(ev("CONVERT", vec![n(1.0), t("cal"), t("J")]), 4.1868);
        close(ev("CONVERT", vec![n(1.0), t("mm"), t("m")]), 0.001);
        close(ev("CONVERT", vec![n(6.0), t("ft"), t("yd")]), 2.0);
        close(ev("CONVERT", vec![n(1.0), t("cup"), t("tsp")]), 48.0);
        is_err(ev("CONVERT", vec![n(2.5), t("ft"), t("sec")]), CellError::NA);
        is_err(ev("CONVERT", vec![n(1.0), t("xyz"), t("m")]), CellError::NA);
        is_err(ev("CONVERT", vec![n(1.0), t("kft"), t("m")]), CellError::NA);
        is_err(ev("CONVERT", vec![n(1.0), t("LBM"), t("kg")]), CellError::NA);
    }

    #[test]
    fn delta_gestep_erf() {
        close(ev("DELTA", vec![n(5.0), n(4.0)]), 0.0);
        close(ev("DELTA", vec![n(5.0), n(5.0)]), 1.0);
        close(ev("DELTA", vec![n(0.5)]), 0.0);
        close(ev("GESTEP", vec![n(5.0), n(4.0)]), 1.0);
        close(ev("GESTEP", vec![n(-4.0)]), 0.0);
        close_tol(ev("ERF", vec![n(0.745)]), 0.707_928_920, 1e-9);
        close_tol(ev("ERF", vec![n(1.0)]), 0.842_700_792_949_715, 1e-12);
        close_tol(ev("ERF", vec![n(0.0), n(1.5)]), 0.966_105_146_475_311, 1e-12);
        close_tol(ev("ERF.PRECISE", vec![n(1.0)]), 0.842_700_792_949_715, 1e-12);
        close_tol(ev("ERFC", vec![n(1.0)]), 0.157_299_207_050_285, 1e-12);
        close_tol(ev("ERFC.PRECISE", vec![n(-1.0)]), 1.842_700_792_949_715, 1e-12);
    }

    #[test]
    fn complex_numbers() {
        is_text(ev("COMPLEX", vec![n(3.0), n(4.0)]), "3+4i");
        is_text(ev("COMPLEX", vec![n(3.0), n(4.0), t("j")]), "3+4j");
        is_text(ev("COMPLEX", vec![n(0.0), n(1.0)]), "i");
        is_text(ev("COMPLEX", vec![n(0.0), n(-1.0)]), "-i");
        is_text(ev("COMPLEX", vec![n(1.5), n(-2.0)]), "1.5-2i");
        is_text(ev("COMPLEX", vec![n(3.0), n(0.0)]), "3");
        is_text(ev("COMPLEX", vec![n(0.0), n(0.0)]), "0");
        is_err(ev("COMPLEX", vec![n(1.0), n(1.0), t("k")]), CellError::Value);
        close(ev("IMABS", vec![t("5+12i")]), 13.0);
        close(ev("IMREAL", vec![t("6-9i")]), 6.0);
        close(ev("IMAGINARY", vec![t("3+4i")]), 4.0);
        close(ev("IMAGINARY", vec![t("-j")]), -1.0);
        close(ev("IMAGINARY", vec![t("4")]), 0.0);
        close(ev("IMREAL", vec![n(7.0)]), 7.0);
        close(ev("IMARGUMENT", vec![t("3+4i")]), 0.927_295_218_001_612_2);
        is_err(ev("IMARGUMENT", vec![t("0")]), CellError::Div0);
        is_text(ev("IMCONJUGATE", vec![t("3+4i")]), "3-4i");
        is_text(ev("IMDIV", vec![t("-238+240i"), t("10+24i")]), "5+12i");
        is_err(ev("IMDIV", vec![t("1+i"), t("0")]), CellError::Num);
        is_text(ev("IMPRODUCT", vec![t("3+4i"), t("5-3i")]), "27+11i");
        is_text(ev("IMPRODUCT", vec![t("1+2i"), n(30.0)]), "30+60i");
        is_text(ev("IMSUM", vec![t("3+4i"), t("5-3i")]), "8+i");
        is_text(ev("IMSUM", vec![rf(arr(vec![vec![tv("1+i"), Value::Empty, tv("2")]]))]), "3+i");
        is_text(ev("IMSUB", vec![t("13+4i"), t("5+3i")]), "8+i");
        is_err(ev("IMSUM", vec![t("1+i"), t("1+j")]), CellError::Value);
        is_err(ev("IMABS", vec![t("abc")]), CellError::Num);
        is_err(ev("IMABS", vec![b(true)]), CellError::Value);
        cx(ev("IMEXP", vec![t("1+i")]), 1.468_693_939_915_885_2, 2.287_355_287_178_842);
        cx(ev("IMLN", vec![t("3+4i")]), 1.609_437_912_434_100_3, 0.927_295_218_001_612_2);
        cx(ev("IMLOG10", vec![t("3+4i")]), 0.698_970_004_336_018_8, 0.402_719_196_273_373_2);
        cx(ev("IMLOG2", vec![t("3+4i")]), 2.321_928_094_887_362, 1.337_804_212_450_976_8);
        cx(ev("IMSQRT", vec![t("1+i")]), 1.098_684_113_467_809_9, 0.455_089_860_562_227_3);
        cx(ev("IMSQRT", vec![t("-4")]), 0.0, 2.0);
        is_text(ev("IMPOWER", vec![t("2+3i"), n(3.0)]), "-46+9i");
        cx(ev("IMPOWER", vec![t("2+3i"), n(0.5)]), 1.674_149_228_035_540_2, 0.895_977_476_129_838);
        cx(ev("IMSIN", vec![t("3+4i")]), 3.853_738_037_919_377, -27.016_813_258_003_93);
        cx(ev("IMCOS", vec![t("1+i")]), 0.833_730_025_131_149, -0.988_897_705_762_865);
        cx(ev("IMSINH", vec![t("4+3i")]), -27.016_813_258_003_93, 3.853_738_037_919_377);
        cx(ev("IMCOSH", vec![t("4+3i")]), -27.034_945_603_074_224, 3.851_153_334_811_777);
        cx(ev("IMTAN", vec![t("4+3i")]), 0.004_908_258_067_496_06, 1.000_709_536_067_233);
        cx(ev("IMCOT", vec![t("4+3i")]), 0.004_901_182_394_304_4, -0.999_266_927_805_902);
        cx(ev("IMSEC", vec![t("4+3i")]), -0.065_294_027_857_947_1, -0.075_224_960_302_773_2);
        cx(ev("IMCSC", vec![t("4+3i")]), -0.075_489_832_915_863_7, 0.064_877_471_370_635_5);
        is_text(ev("IMSECH", vec![t("0")]), "1");
        is_err(ev("IMCSCH", vec![t("0")]), CellError::Num);
        // sech z · cosh z = 1
        let Value::Text(c) = ev("IMCOSH", vec![t("4+3i")]) else { panic!() };
        let Value::Text(sc) = ev("IMSECH", vec![t("4+3i")]) else { panic!() };
        cx(ev("IMPRODUCT", vec![t(&c), t(&sc)]), 1.0, 0.0);
        let Value::Text(sh) = ev("IMSINH", vec![t("4+3i")]) else { panic!() };
        let Value::Text(csh) = ev("IMCSCH", vec![t("4+3i")]) else { panic!() };
        cx(ev("IMPRODUCT", vec![t(&sh), t(&csh)]), 1.0, 0.0);
        cx(ev("IMSUM", vec![t("1.5e1+2E-1j")]), 15.0, 0.2);
    }

    #[test]
    fn bessel() {
        close_tol(ev("BESSELJ", vec![n(1.9), n(2.0)]), 0.329_925_829, 1e-6);
        close_tol(ev("BESSELY", vec![n(2.5), n(1.0)]), 0.145_918_138, 1e-6);
        close_tol(ev("BESSELI", vec![n(1.5), n(1.0)]), 0.981_666_428, 1e-6);
        close_tol(ev("BESSELK", vec![n(1.5), n(1.0)]), 0.277_387_804, 1e-6);
        close_tol(ev("BESSELJ", vec![n(1.0), n(0.0)]), 0.765_197_686_557_966_6, 1e-12);
        close_tol(ev("BESSELJ", vec![n(1.0), n(1.0)]), 0.440_050_585_744_933_5, 1e-12);
        close_tol(ev("BESSELJ", vec![n(-1.0), n(1.0)]), -0.440_050_585_744_933_5, 1e-12);
        close_tol(ev("BESSELY", vec![n(1.0), n(0.0)]), 0.088_256_964_215_676_96, 1e-11);
        close_tol(ev("BESSELY", vec![n(1.0), n(1.0)]), -0.781_212_821_300_288_7, 1e-11);
        close_tol(ev("BESSELI", vec![n(1.0), n(0.0)]), 1.266_065_877_752_008_4, 1e-12);
        close_tol(ev("BESSELI", vec![n(1.0), n(1.0)]), 0.565_159_103_992_485_1, 1e-12);
        close_tol(ev("BESSELK", vec![n(1.0), n(0.0)]), 0.421_024_438_240_708_3, 1e-11);
        close_tol(ev("BESSELK", vec![n(1.0), n(1.0)]), 0.601_907_230_197_234_6, 1e-11);
        is_err(ev("BESSELJ", vec![n(1.0), n(-1.0)]), CellError::Num);
        is_err(ev("BESSELY", vec![n(0.0), n(1.0)]), CellError::Num);
        is_err(ev("BESSELK", vec![n(-1.0), n(1.0)]), CellError::Num);
        is_err(ev("BESSELJ", vec![t("x"), n(1.0)]), CellError::Value);
        // Wronskian J_{1}Y_0 − J_0Y_1 = 2/(πx), checking series and asymptotic regimes.
        for x in [0.5, 7.0, 15.0, 24.0, 30.0, 80.0] {
            let j0 = match ev("BESSELJ", vec![n(x), n(0.0)]) {
                Value::Number(v) => v,
                _ => panic!(),
            };
            let j1 = match ev("BESSELJ", vec![n(x), n(1.0)]) {
                Value::Number(v) => v,
                _ => panic!(),
            };
            let y0 = match ev("BESSELY", vec![n(x), n(0.0)]) {
                Value::Number(v) => v,
                _ => panic!(),
            };
            let y1 = match ev("BESSELY", vec![n(x), n(1.0)]) {
                Value::Number(v) => v,
                _ => panic!(),
            };
            let w = j1 * y0 - j0 * y1;
            let expect = 2.0 / (std::f64::consts::PI * x);
            assert!((w - expect).abs() < 1e-9 * expect.max(1.0), "x={x}: {w} vs {expect}");
        }
        // Large orders and arguments stay finite.
        assert!(matches!(ev("BESSELJ", vec![n(50.0), n(3.0)]), Value::Number(_)));
        assert!(matches!(ev("BESSELK", vec![n(30.0), n(5.0)]), Value::Number(_)));
    }
}
