use super::{Arg, CellError, Ctx, PI, R, Value, num, num_val};

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

/// Horner evaluation of `c[0] + c[1]·t + c[2]·t² + …`.
fn horner(c: &[f64], t: f64) -> f64 {
    c.iter().rev().fold(0.0, |r, a| r * t + a)
}

// The I and K polynomials for orders 0 and 1 follow Abramowitz & Stegun 9.8.1–9.8.8.

/// I_0(x) (A&S 9.8.1, 9.8.2).
fn bessel_i0(x: f64) -> f64 {
    let ax = x.abs();
    if ax < 3.75 {
        let t = (x / 3.75) * (x / 3.75);
        horner(&[1.0, 3.515_622_9, 3.089_942_4, 1.206_749_2, 0.265_973_2, 0.036_076_8, 0.004_581_3], t)
    } else {
        let t = 3.75 / ax;
        let p = [0.398_942_28, 0.013_285_92, 0.002_253_19, -0.001_575_65, 0.009_162_81, -0.020_577_06, 0.026_355_37, -0.016_476_33, 0.003_923_77];
        (ax - 0.5 * ax.ln() + horner(&p, t).ln()).exp()
    }
}

/// I_1(x) (A&S 9.8.3, 9.8.4).
fn bessel_i1(x: f64) -> f64 {
    let ax = x.abs();
    let r = if ax < 3.75 {
        let t = (x / 3.75) * (x / 3.75);
        ax * horner(&[0.5, 0.878_905_94, 0.514_988_69, 0.150_849_34, 0.026_587_33, 0.003_015_32, 0.000_324_11], t)
    } else {
        let t = 3.75 / ax;
        let p = [0.398_942_28, -0.039_880_24, -0.003_620_18, 0.001_638_01, -0.010_315_55, 0.022_829_67, -0.028_953_12, 0.017_876_54, -0.004_200_59];
        (ax - 0.5 * ax.ln() + horner(&p, t).ln()).exp()
    };
    if x < 0.0 { -r } else { r }
}

/// I_n(x), using A&S approximations for orders 0 and 1 and the existing series for higher orders.
fn bessel_i(n: u32, x: f64) -> Option<f64> {
    let r = match n {
        0 => bessel_i0(x),
        1 => bessel_i1(x),
        _ => {
            if x == 0.0 {
                return Some(0.0);
            }
            let nf = n as f64;
            let ln_t0 = nf * (x.abs() / 2.0).ln() - crate::special::ln_gamma(nf + 1.0);
            let ax = x.abs();
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
            let value = (ln_t0 + sum.ln()).exp();
            if x < 0.0 && n % 2 == 1 { -value } else { value }
        }
    };
    if r.is_finite() { Some(r) } else { None }
}

/// K_0(x) for x > 0 (A&S 9.8.5, 9.8.6).
// The first coefficient is A&S's 8-digit −γ, which Excel reproduces; the exact constant would differ.
#[allow(clippy::approx_constant)]
fn bessel_k0(x: f64) -> f64 {
    if x <= 2.0 {
        let t = (x / 2.0) * (x / 2.0);
        let p = [-0.577_215_66, 0.422_784_20, 0.230_697_56, 0.034_885_90, 0.002_626_98, 0.000_107_50, 0.000_007_40];
        -(x / 2.0).ln() * bessel_i0(x) + horner(&p, t)
    } else {
        let p = [1.253_314_14, -0.078_323_58, 0.021_895_68, -0.010_624_46, 0.005_878_72, -0.002_515_40, 0.000_532_08];
        (-x).exp() / x.sqrt() * horner(&p, 2.0 / x)
    }
}

/// K_1(x) for x > 0 (A&S 9.8.7, 9.8.8).
fn bessel_k1(x: f64) -> f64 {
    if x <= 2.0 {
        let t = (x / 2.0) * (x / 2.0);
        let p = [1.0, 0.154_431_44, -0.672_785_79, -0.181_568_97, -0.019_194_02, -0.001_104_04, -0.000_046_86];
        (x * (x / 2.0).ln() * bessel_i1(x) + horner(&p, t)) / x
    } else {
        let p = [1.253_314_14, 0.234_986_19, -0.036_556_20, 0.015_042_68, -0.007_803_53, 0.003_256_14, -0.000_682_45];
        (-x).exp() / x.sqrt() * horner(&p, 2.0 / x)
    }
}

/// K_n(x) for x > 0: K_0 and K_1 from the polynomials, then upward recurrence.
fn bessel_k(n: u32, x: f64) -> Option<f64> {
    let (mut a, mut b) = (bessel_k0(x), bessel_k1(x));
    if n == 0 {
        return if a.is_finite() { Some(a) } else { None };
    }
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

fn bessel_args(a: &[Arg]) -> R<(f64, u32)> {
    let x = num(a, 0)?;
    let n = num(a, 1)?.trunc();
    if n < 0.0 {
        return Err(CellError::Num);
    }
    if n > 100_000.0 {
        return Err(CellError::Num);
    }
    Ok((x, n as u32))
}

pub(super) fn besselj(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, n) = bessel_args(a)?;
    num_val(bessel_j(n, x).ok_or(CellError::Num)?)
}

pub(super) fn bessely(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, n) = bessel_args(a)?;
    if x <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(bessel_y(n, x).ok_or(CellError::Num)?)
}

pub(super) fn besseli(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, n) = bessel_args(a)?;
    num_val(bessel_i(n, x).ok_or(CellError::Num)?)
}

pub(super) fn besselk(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, n) = bessel_args(a)?;
    if x <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(bessel_k(n, x).ok_or(CellError::Num)?)
}
