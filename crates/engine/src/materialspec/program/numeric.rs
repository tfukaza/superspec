//! Transcendental functions that give the same bits on every target.
//!
//! Platform `exp`, `ln` and `powf` come from each target's math library and may differ in the
//! last place between native and Wasm builds, which would break byte parity of material output.
//! These use only IEEE basic operations (`+ - * /`, exact rounding and bit manipulation), so every
//! target computes identical results. They are accurate to a few units in the last place.
pub(super) use superspec_geometry::math::{acos, cos, sin};

const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-1;
const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
const LN2: f64 = std::f64::consts::LN_2;

/// `2^k` for an integer exponent in the normal range, built from its bits.
fn two_to(k: i64) -> f64 {
    if k < -1022 {
        return 0.;
    }
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// Natural exponential. Arguments above 709 overflow to infinity; below -708, the result is zero.
pub(super) fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 709.0 {
        return f64::INFINITY;
    }
    if x < -708.0 {
        return 0.;
    }
    // x = k ln 2 + r with |r| <= ln 2 / 2; the split constant keeps r exact.
    let k = (x / LN2).round();
    let r = (x - k * LN2_HI) - k * LN2_LO;
    // Taylor series of e^r to degree 17 bounds the error below one ulp for |r| <= 0.35.
    let mut term = 1.0;
    let mut sum = 1.0;
    for n in 1..=17 {
        term *= r / n as f64;
        sum += term;
    }
    sum * two_to(k as i64)
}

/// Natural logarithm of a positive finite argument; zero gives negative infinity.
pub(super) fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0. {
        return f64::NAN;
    }
    if x == 0. {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return x;
    }
    // Normalize subnormals, then split x = m 2^e with m in [sqrt(1/2), sqrt(2)).
    let (x, bias) = if x < f64::MIN_POSITIVE {
        (x * two_to(54), -54i64)
    } else {
        (x, 0)
    };
    let bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i64 - 1023 + bias;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    // ln m = 2 atanh(f) with f = (m - 1) / (m + 1), |f| < 0.172.
    let f = (m - 1.) / (m + 1.);
    let s = f * f;
    let mut power = f;
    let mut sum = 0.;
    for n in 0..14 {
        sum += power / (2 * n + 1) as f64;
        power *= s;
    }
    (e as f64 * LN2_HI + 2. * sum) + e as f64 * LN2_LO
}

/// `x^y` for a nonnegative base: `0^y` is zero for positive `y` and one for zero `y`.
pub(super) fn pow(x: f64, y: f64) -> f64 {
    if y == 0. {
        return 1.;
    }
    if x == 0. {
        return if y > 0. { 0. } else { f64::INFINITY };
    }
    if x == 1. {
        return 1.;
    }
    exp(y * ln(x))
}

/// Base-2 logarithm of a positive argument.
pub(super) fn log2(x: f64) -> f64 {
    ln(x) / LN2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        a == b || (a - b).abs() <= 4. * f64::EPSILON * a.abs().max(b.abs())
    }

    #[test]
    fn matches_the_platform_functions_to_a_few_ulps() {
        for i in -2000..=2000 {
            let x = i as f64 * 0.0997;
            assert!(
                close(exp(x), x.exp()),
                "exp({x}) = {} vs {}",
                exp(x),
                x.exp()
            );
        }
        for i in 1..=4000 {
            let x = i as f64 * 0.0371;
            assert!(close(ln(x), x.ln()), "ln({x})");
            assert!(close(log2(x), x.log2()), "log2({x})");
            for y in [0.5, 0.75, 2., 3., 4., -1.5] {
                let (a, b) = (pow(x, y), x.powf(y));
                assert!(
                    (a - b).abs() <= 1e-13 * a.abs().max(b.abs()),
                    "pow({x}, {y})"
                );
            }
        }
        assert_eq!(pow(0., 2.), 0.);
        assert_eq!(pow(0., 0.), 1.);
        assert_eq!(exp(-1000.), 0.);
        assert!(ln(1e-310).is_finite(), "subnormal arguments are normalized");
    }
}
