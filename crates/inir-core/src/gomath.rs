//! Go's math package on amd64, for the ports of Go helpers: the same algorithms bit for bit (Go's pure-Go cbrt,
//! atan2, sin, cos and pow; its assembly exp, log and hypot), not glibc's. A grey's hue is rounding noise, and
//! that noise has to be Go's for a port to give Go's colours.

const PI: f64 = std::f64::consts::PI;

pub fn cbrt(x: f64) -> f64 {
    const B1: u64 = 715094163;
    const B2: u64 = 696219795;
    const C: f64 = 5.42857142857142815906e-01;
    const D: f64 = -7.05306122448979611050e-01;
    const E: f64 = 1.41428571428571436819e+00;
    const F: f64 = 1.60714285714285720630e+00;
    const G: f64 = 3.57142857142857150787e-01;
    const SMALLEST_NORMAL: f64 = 2.22507385850720138309e-308;
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return x;
    }
    let mut x = x;
    let sign = x < 0.0;
    if sign {
        x = -x;
    }
    let mut t = f64::from_bits(x.to_bits() / 3 + (B1 << 32));
    if x < SMALLEST_NORMAL {
        t = (1u64 << 54) as f64;
        t *= x;
        t = f64::from_bits(t.to_bits() / 3 + (B2 << 32));
    }
    let mut r = t * t / x;
    let mut s = C + r * t;
    t *= G + F / (s + E + D / s);
    t = f64::from_bits((t.to_bits() & (0xFFFFFFFFCu64 << 28)) + (1u64 << 30));
    s = t * t;
    r = x / s;
    let w = t + t;
    r = (r - t) / (w + r);
    t = t + t * r;
    if sign { -t } else { t }
}

fn xatan(x: f64) -> f64 {
    const P0: f64 = -8.750608600031904122785e-01;
    const P1: f64 = -1.615753718733365076637e+01;
    const P2: f64 = -7.500855792314704667340e+01;
    const P3: f64 = -1.228866684490136173410e+02;
    const P4: f64 = -6.485021904942025371773e+01;
    const Q0: f64 = 2.485846490142306297962e+01;
    const Q1: f64 = 1.650270098316988542046e+02;
    const Q2: f64 = 4.328810604912902668951e+02;
    const Q3: f64 = 4.853903996359136964868e+02;
    const Q4: f64 = 1.945506571482613964425e+02;
    let mut z = x * x;
    z = z * ((((P0 * z + P1) * z + P2) * z + P3) * z + P4) / (((((z + Q0) * z + Q1) * z + Q2) * z + Q3) * z + Q4);
    x * z + x
}

fn satan(x: f64) -> f64 {
    const MOREBITS: f64 = 6.123233995736765886130e-17;
    const TAN3PIO8: f64 = 2.41421356237309504880;
    if x <= 0.66 {
        return xatan(x);
    }
    if x > TAN3PIO8 {
        return PI / 2.0 - xatan(1.0 / x) + MOREBITS;
    }
    PI / 4.0 + xatan((x - 1.0) / (x + 1.0)) + 0.5 * MOREBITS
}

pub fn atan(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    if x > 0.0 { satan(x) } else { -satan(-x) }
}

pub fn atan2(y: f64, x: f64) -> f64 {
    if y.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        if x >= 0.0 && !x.is_sign_negative() {
            return 0f64.copysign(y);
        }
        return PI.copysign(y);
    }
    if x == 0.0 {
        return (PI / 2.0).copysign(y);
    }
    if x.is_infinite() {
        if x > 0.0 {
            return if y.is_infinite() { (PI / 4.0).copysign(y) } else { 0f64.copysign(y) };
        }
        return if y.is_infinite() { f64::from_bits(0x4002d97c7f3321d2).copysign(y) } else { PI.copysign(y) };
    }
    if y.is_infinite() {
        return (PI / 2.0).copysign(y);
    }
    let q = atan(y / x);
    if x < 0.0 {
        if q <= 0.0 {
            return q + PI;
        }
        return q - PI;
    }
    q
}

/// Go's hypot_amd64.s: max * sqrt(1 + (min/max)^2).
pub fn hypot(p: f64, q: f64) -> f64 {
    let (p, q) = (p.abs(), q.abs());
    if p.is_infinite() || q.is_infinite() {
        return f64::INFINITY;
    }
    if p.is_nan() || q.is_nan() {
        return f64::NAN;
    }
    if p == 0.0 && q == 0.0 {
        return 0.0;
    }
    let (hi, lo) = if p >= q { (p, q) } else { (q, p) };
    let r = lo / hi;
    hi * (r * r + 1.0).sqrt()
}

const SIN: [f64; 6] = [
    1.58962301576546568060e-10,
    -2.50507477628578072866e-8,
    2.75573136213857245213e-6,
    -1.98412698295895385996e-4,
    8.33333333332211858878e-3,
    -1.66666666666666307295e-1,
];
const COS: [f64; 6] = [
    -1.13585365213876817300e-11,
    2.08757008419747316778e-9,
    -2.75573141792967388112e-7,
    2.48015872888517045348e-5,
    -1.38888888888730564116e-3,
    4.16666666666665929218e-2,
];
const PI4A: f64 = 7.85398125648498535156e-1;
const PI4B: f64 = 3.77489470793079817668e-8;
const PI4C: f64 = 2.69515142907905952645e-15;
/// float64(4/Pi), folded exactly by Go's compiler.
const FOUR_OVER_PI: f64 = 1.2732395447351628;
const REDUCE_THRESHOLD: f64 = (1u64 << 29) as f64;

fn octant(x: f64) -> (u64, f64) {
    let mut j = (x * FOUR_OVER_PI) as u64;
    let mut y = j as f64;
    if j & 1 == 1 {
        j += 1;
        y += 1.0;
    }
    j &= 7;
    (j, ((x - y * PI4A) - y * PI4B) - y * PI4C)
}

fn sin_poly(z: f64, zz: f64) -> f64 {
    z + z * zz * ((((((SIN[0] * zz) + SIN[1]) * zz + SIN[2]) * zz + SIN[3]) * zz + SIN[4]) * zz + SIN[5])
}

fn cos_poly(zz: f64) -> f64 {
    1.0 - 0.5 * zz + zz * zz * ((((((COS[0] * zz) + COS[1]) * zz + COS[2]) * zz + COS[3]) * zz + COS[4]) * zz + COS[5])
}

pub fn cos(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    let x = x.abs();
    if x >= REDUCE_THRESHOLD {
        return x.cos(); // Payne-Hanek territory; no colour hue gets here
    }
    let (mut j, z) = octant(x);
    let mut sign = false;
    if j > 3 {
        j -= 4;
        sign = !sign;
    }
    if j > 1 {
        sign = !sign;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 { sin_poly(z, zz) } else { cos_poly(zz) };
    if sign { -y } else { y }
}

pub fn sin(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::NAN;
    }
    let (x, mut sign) = if x < 0.0 { (-x, true) } else { (x, false) };
    if x >= REDUCE_THRESHOLD {
        let y = x.sin();
        return if sign { -y } else { y };
    }
    let (mut j, z) = octant(x);
    if j > 3 {
        sign = !sign;
        j -= 4;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 { cos_poly(zz) } else { sin_poly(z, zz) };
    if sign { -y } else { y }
}

fn use_fma() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        static FMA: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FMA.get_or_init(|| std::is_x86_feature_detected!("avx") && std::is_x86_feature_detected!("fma"))
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Go's exp_amd64.s, with its FMA path where the CPU has one (as Go picks at start).
pub fn exp(x: f64) -> f64 {
    const LOG2E: f64 = 1.4426950408889634073599246810018920;
    const LN2U: f64 = 0.69314718055966295651160180568695068359375;
    const LN2L: f64 = 0.28235290563031577122588448175013436025525412068e-12;
    const OVERFLOW: f64 = 7.09782712893384e+02;
    const T: [f64; 9] = [
        0.5,
        1.0,
        2.0,
        1.6666666666666666667e-1,
        4.1666666666666666667e-2,
        8.3333333333333333333e-3,
        1.3888888888888888889e-3,
        1.9841269841269841270e-4,
        2.4801587301587301587e-5,
    ];
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x == f64::NEG_INFINITY {
        return 0.0;
    }
    if x > OVERFLOW {
        return f64::INFINITY;
    }
    let kf = LOG2E * x;
    // CVTSD2SL rounds to nearest even; out of range is the "integer indefinite" 0x80000000.
    let k: i32 = if kf.is_nan() || kf >= 2147483647.5 || kf < -2147483648.5 { i32::MIN } else { kf.round_ties_even() as i32 };
    let kd = k as f64;
    let mut x = x;
    let fma = use_fma();
    if fma {
        x = (-kd).mul_add(LN2U, x);
        x = (-kd).mul_add(LN2L, x);
    } else {
        x -= kd * LN2U;
        x -= kd * LN2L;
    }
    x *= 0.0625;
    if fma {
        let mut p = T[8];
        for c in [T[7], T[6], T[5], T[4], T[3], T[0], T[1]] {
            p = p.mul_add(x, c);
        }
        x *= p;
        for _ in 0..3 {
            let t = x + T[2];
            x *= t;
        }
        let t = x + T[2];
        x = x.mul_add(t, T[1]);
    } else {
        let mut p = T[8];
        for c in [T[7], T[6], T[5], T[4], T[3], T[0], T[1]] {
            p = p * x + c;
        }
        x *= p;
        for _ in 0..4 {
            let t = x + T[2];
            x *= t;
        }
        x += T[1];
    }
    let mut bx = k.wrapping_add(0x3FF);
    if bx <= 0 {
        if bx < -52 {
            return 0.0;
        }
        bx += 0x3FE;
        x *= f64::from_bits((bx as u64) << 52);
        bx = 1;
    } else if bx >= 0x7FF {
        return f64::INFINITY;
    }
    x * f64::from_bits((bx as u64) << 52)
}

/// Go's log_amd64.s.
pub fn log(x: f64) -> f64 {
    const HSQRT2: f64 = 7.07106781186547524401e-01;
    const LN2HI: f64 = 6.93147180369123816490e-01;
    const LN2LO: f64 = 1.90821492927058770002e-10;
    const L1: f64 = 6.666666666666735130e-01;
    const L2: f64 = 3.999999999940941908e-01;
    const L3: f64 = 2.857142874366239149e-01;
    const L4: f64 = 2.222219843214978396e-01;
    const L5: f64 = 1.818357216161805012e-01;
    const L6: f64 = 1.531383769920937332e-01;
    const L7: f64 = 1.479819860511658591e-01;
    let bits = x.to_bits();
    if bits & !(1u64 << 63) == 0 {
        return f64::NEG_INFINITY;
    }
    if (bits as i64) < 0 {
        return f64::NAN;
    }
    if bits >= 0x7FF0000000000000 {
        return x;
    }
    let f1 = f64::from_bits((bits & 0x000FFFFFFFFFFFFF) | 0.5f64.to_bits());
    let mut k = (((bits >> 52) & 0x7FF) as i32 - 0x3FE) as f64;
    let below = if !(HSQRT2 < f1) { 1.0 } else { 0.0 };
    k -= below;
    let f = f1 * (below + 1.0) - 1.0;
    let s = f / (2.0 + f);
    let s2 = s * s;
    let s4 = s2 * s2;
    let t1 = s2 * (((L7 * s4 + L5) * s4 + L3) * s4 + L1);
    let t2 = s4 * ((L6 * s4 + L4) * s4 + L2);
    let r = t1 + t2;
    let hfsq = 0.5 * f * f;
    let x3 = s * (r + hfsq) + LN2LO * k;
    k * LN2HI - ((hfsq - x3) - f)
}

fn normalize(x: f64) -> (f64, i32) {
    const SMALLEST_NORMAL: f64 = 2.2250738585072014e-308;
    if x.abs() < SMALLEST_NORMAL {
        return (x * (1u64 << 52) as f64, -52);
    }
    (x, 0)
}

pub fn frexp(f: f64) -> (f64, i32) {
    if f == 0.0 || f.is_infinite() || f.is_nan() {
        return (f, 0);
    }
    let (f, mut exp) = normalize(f);
    let mut x = f.to_bits();
    exp += ((x >> 52) & 0x7ff) as i32 - 1023 + 1;
    x &= !(0x7ffu64 << 52);
    x |= 1022u64 << 52;
    (f64::from_bits(x), exp)
}

pub fn ldexp(frac: f64, exp: i32) -> f64 {
    if frac == 0.0 || frac.is_infinite() || frac.is_nan() {
        return frac;
    }
    let (frac, e) = normalize(frac);
    let mut exp = exp as i64 + e as i64;
    let mut x = frac.to_bits();
    exp += ((x >> 52) & 0x7ff) as i64 - 1023;
    if exp < -1075 {
        return 0f64.copysign(frac);
    }
    if exp > 1023 {
        return if frac < 0.0 { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let mut m = 1.0;
    if exp < -1022 {
        exp += 53;
        m = 1.0 / (1u64 << 53) as f64;
    }
    x &= !(0x7ffu64 << 52);
    x |= ((exp + 1023) as u64) << 52;
    m * f64::from_bits(x)
}

fn modf(f: f64) -> (f64, f64) {
    let i = f.trunc();
    (i, (f - i).copysign(f))
}

fn is_odd_int(x: f64) -> bool {
    if x.abs() >= (1u64 << 53) as f64 {
        return false;
    }
    let (xi, xf) = modf(x);
    xf == 0.0 && (xi as i64) & 1 == 1
}

pub fn pow(x: f64, y: f64) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if y == 1.0 {
        return x;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        if y < 0.0 {
            return if x.is_sign_negative() && is_odd_int(y) { f64::NEG_INFINITY } else { f64::INFINITY };
        }
        if y > 0.0 {
            return if x.is_sign_negative() && is_odd_int(y) { x } else { 0.0 };
        }
    }
    if y.is_infinite() {
        if x == -1.0 {
            return 1.0;
        }
        return if (x.abs() < 1.0) == (y > 0.0) { 0.0 } else { f64::INFINITY };
    }
    if x.is_infinite() {
        if x < 0.0 {
            return pow(1.0 / x, -y);
        }
        if y < 0.0 {
            return 0.0;
        }
        if y > 0.0 {
            return f64::INFINITY;
        }
    }
    if y == 0.5 {
        return x.sqrt();
    }
    if y == -0.5 {
        return 1.0 / x.sqrt();
    }
    let (mut yi, mut yf) = modf(y.abs());
    if yf != 0.0 && x < 0.0 {
        return f64::NAN;
    }
    if yi >= 9.223372036854775808e18 {
        if x == -1.0 {
            return 1.0;
        }
        return if (x.abs() < 1.0) == (y > 0.0) { 0.0 } else { f64::INFINITY };
    }
    let mut a1 = 1.0;
    let mut ae: i64 = 0;
    if yf != 0.0 {
        if yf > 0.5 {
            yf -= 1.0;
            yi += 1.0;
        }
        a1 = exp(yf * log(x));
    }
    let (mut x1, xe) = frexp(x);
    let mut xe = xe as i64;
    let mut i = yi as i64;
    while i != 0 {
        if xe < -(1 << 12) || (1 << 12) < xe {
            ae += xe;
            break;
        }
        if i & 1 == 1 {
            a1 *= x1;
            ae += xe;
        }
        x1 *= x1;
        xe <<= 1;
        if x1 < 0.5 {
            x1 += x1;
            xe -= 1;
        }
        i >>= 1;
    }
    if y < 0.0 {
        a1 = 1.0 / a1;
        ae = -ae;
    }
    ldexp(a1, ae.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
}
