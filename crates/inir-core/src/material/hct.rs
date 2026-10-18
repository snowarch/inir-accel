//! materialyoucolor 3.0.4's colour science (`utils`, `hct`), every expression in the original's order of operations.
//! Python's floats are f64 and its `math` calls glibc's libm, as Rust's do, so a faithful transcription is exact.

use super::planes::CRITICAL_PLANES;
use super::py::fmod;
use std::f64::consts::PI;
use std::sync::OnceLock;

const SRGB_TO_XYZ: [[f64; 3]; 3] =
    [[0.41233895, 0.35762064, 0.18051042], [0.2126, 0.7152, 0.0722], [0.01932141, 0.11916382, 0.95034478]];
const XYZ_TO_SRGB: [[f64; 3]; 3] = [
    [3.2413774792388685, -1.5376652402851851, -0.49885366846268053],
    [-0.9691452513005321, 1.8758853451067872, 0.04156585616912061],
    [0.05562093689691305, -0.20395524564742123, 1.0571799111220335],
];
const WHITE_POINT_D65: [f64; 3] = [95.047, 100.0, 108.883];

pub fn argb_from_rgb(r: i64, g: i64, b: i64) -> u32 {
    (255u32 << 24) | (((r & 255) as u32) << 16) | (((g & 255) as u32) << 8) | ((b & 255) as u32)
}

pub fn red(argb: u32) -> i64 {
    ((argb >> 16) & 255) as i64
}
pub fn green(argb: u32) -> i64 {
    ((argb >> 8) & 255) as i64
}
pub fn blue(argb: u32) -> i64 {
    (argb & 255) as i64
}

pub fn signum(x: f64) -> f64 {
    if x < 0.0 {
        -1.0
    } else if x == 0.0 {
        0.0
    } else {
        1.0
    }
}

pub fn lerp(start: f64, stop: f64, amount: f64) -> f64 {
    (1.0 - amount) * start + amount * stop
}

pub fn clamp_double(min: f64, max: f64, x: f64) -> f64 {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}

pub fn sanitize_degrees_double(d: f64) -> f64 {
    let d = fmod(d, 360.0);
    if d < 0.0 { d + 360.0 } else { d }
}

pub fn sanitize_degrees_int(d: i64) -> i64 {
    d.rem_euclid(360)
}

pub fn rotation_direction(from: f64, to: f64) -> f64 {
    if sanitize_degrees_double(to - from) <= 180.0 { 1.0 } else { -1.0 }
}

pub fn difference_degrees(a: f64, b: f64) -> f64 {
    180.0 - ((a - b).abs() - 180.0).abs()
}

fn matrix_multiply(row: [f64; 3], m: &[[f64; 3]; 3]) -> [f64; 3] {
    [
        row[0] * m[0][0] + row[1] * m[0][1] + row[2] * m[0][2],
        row[0] * m[1][0] + row[1] * m[1][1] + row[2] * m[1][2],
        row[0] * m[2][0] + row[1] * m[2][1] + row[2] * m[2][2],
    ]
}

pub fn linearized(c: i64) -> f64 {
    let n = c as f64 / 255.0;
    if n <= 0.040449936 { n / 12.92 * 100.0 } else { ((n + 0.055) / 1.055).powf(2.4) * 100.0 }
}

/// `clamp_int(0, 255, round(...))` with Python's round (half to even).
pub fn delinearized(c: f64) -> i64 {
    let n = c / 100.0;
    let d = if n <= 0.0031308 { n * 12.92 } else { 1.055 * n.powf(1.0 / 2.4) - 0.055 };
    let r = (d * 255.0).round_ties_even();
    if r < 0.0 {
        0
    } else if r > 255.0 {
        255
    } else {
        r as i64
    }
}

fn lab_f(t: f64) -> f64 {
    let e = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    if t > e { t.powf(1.0 / 3.0) } else { (kappa * t + 16.0) / 116.0 }
}

fn lab_invf(ft: f64) -> f64 {
    let e = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    let ft3 = ft * ft * ft;
    if ft3 > e { ft3 } else { (116.0 * ft - 16.0) / kappa }
}

pub fn argb_from_xyz(x: f64, y: f64, z: f64) -> u32 {
    let m = &XYZ_TO_SRGB;
    let lr = m[0][0] * x + m[0][1] * y + m[0][2] * z;
    let lg = m[1][0] * x + m[1][1] * y + m[1][2] * z;
    let lb = m[2][0] * x + m[2][1] * y + m[2][2] * z;
    argb_from_rgb(delinearized(lr), delinearized(lg), delinearized(lb))
}

pub fn argb_from_linrgb(l: [f64; 3]) -> u32 {
    argb_from_rgb(delinearized(l[0]), delinearized(l[1]), delinearized(l[2]))
}

pub fn lab_from_argb(argb: u32) -> [f64; 3] {
    let lr = linearized(red(argb));
    let lg = linearized(green(argb));
    let lb = linearized(blue(argb));
    let m = &SRGB_TO_XYZ;
    let x = m[0][0] * lr + m[0][1] * lg + m[0][2] * lb;
    let y = m[1][0] * lr + m[1][1] * lg + m[1][2] * lb;
    let z = m[2][0] * lr + m[2][1] * lg + m[2][2] * lb;
    let fx = lab_f(x / WHITE_POINT_D65[0]);
    let fy = lab_f(y / WHITE_POINT_D65[1]);
    let fz = lab_f(z / WHITE_POINT_D65[2]);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

pub fn y_from_lstar(lstar: f64) -> f64 {
    100.0 * lab_invf((lstar + 16.0) / 116.0)
}

pub fn lstar_from_y(y: f64) -> f64 {
    lab_f(y / 100.0) * 116.0 - 16.0
}

pub fn lstar_from_argb(argb: u32) -> f64 {
    let y = matrix_multiply([linearized(red(argb)), linearized(green(argb)), linearized(blue(argb))], &SRGB_TO_XYZ)[1];
    116.0 * lab_f(y / 100.0) - 16.0
}

pub fn argb_from_lstar(lstar: f64) -> u32 {
    let c = delinearized(y_from_lstar(lstar));
    argb_from_rgb(c, c, c)
}

// ---------------------------------------------------------------- viewing conditions and CAM16

pub struct ViewingConditions {
    pub n: f64,
    pub aw: f64,
    pub nbb: f64,
    pub ncb: f64,
    pub c: f64,
    pub nc: f64,
    pub rgb_d: [f64; 3],
    pub fl: f64,
    pub f_l_root: f64,
    pub z: f64,
}

/// `ViewingConditions.DEFAULT`.
pub fn vc() -> &'static ViewingConditions {
    static VC: OnceLock<ViewingConditions> = OnceLock::new();
    VC.get_or_init(|| {
        let xyz = WHITE_POINT_D65;
        let adapting_luminance = 200.0 / PI * y_from_lstar(50.0) / 100.0;
        let background_lstar = 50.0;
        let surround = 2.0;
        let r_w = xyz[0] * 0.401288 + xyz[1] * 0.650173 + xyz[2] * -0.051461;
        let g_w = xyz[0] * -0.250268 + xyz[1] * 1.204414 + xyz[2] * 0.045854;
        let b_w = xyz[0] * -0.002079 + xyz[1] * 0.048952 + xyz[2] * 0.953127;
        let f = 0.8 + surround / 10.0;
        let c = if f >= 0.9 { lerp(0.59, 0.69, (f - 0.9) * 10.0) } else { lerp(0.525, 0.59, (f - 0.8) * 10.0) };
        let mut d = f * (1.0 - 1.0 / 3.6 * ((-adapting_luminance - 42.0) / 92.0).exp());
        d = if d > 1.0 { 1.0 } else if d < 0.0 { 0.0 } else { d };
        let nc = f;
        let rgb_d = [d * (100.0 / r_w) + 1.0 - d, d * (100.0 / g_w) + 1.0 - d, d * (100.0 / b_w) + 1.0 - d];
        let k = 1.0 / (5.0 * adapting_luminance + 1.0);
        let k4 = k * k * k * k;
        let k4_f = 1.0 - k4;
        let fl = k4 * adapting_luminance + 0.1 * k4_f * k4_f * (5.0 * adapting_luminance).powf(1.0 / 3.0);
        let n = y_from_lstar(background_lstar) / xyz[1];
        let z = 1.48 + n.sqrt();
        let nbb = 0.725 / n.powf(0.2);
        let ncb = nbb;
        let f0 = (fl * rgb_d[0] * r_w / 100.0).powf(0.42);
        let f1 = (fl * rgb_d[1] * g_w / 100.0).powf(0.42);
        let f2 = (fl * rgb_d[2] * b_w / 100.0).powf(0.42);
        let rgb_a = [400.0 * f0 / (f0 + 27.13), 400.0 * f1 / (f1 + 27.13), 400.0 * f2 / (f2 + 27.13)];
        let aw = (2.0 * rgb_a[0] + rgb_a[1] + 0.05 * rgb_a[2]) * nbb;
        ViewingConditions { n, aw, nbb, ncb, c, nc, rgb_d, fl, f_l_root: fl.powf(0.25), z }
    })
}

/// Hue and chroma of `Cam16.from_int` (the rest of the CAM is never read by the script's path).
fn cam16_hue_chroma(argb: u32) -> (f64, f64) {
    let v = vc();
    let red_l = linearized(red(argb));
    let green_l = linearized(green(argb));
    let blue_l = linearized(blue(argb));
    let x = 0.41233895 * red_l + 0.35762064 * green_l + 0.18051042 * blue_l;
    let y = 0.2126 * red_l + 0.7152 * green_l + 0.0722 * blue_l;
    let z = 0.01932141 * red_l + 0.11916382 * green_l + 0.95034478 * blue_l;
    let r_c = 0.401288 * x + 0.650173 * y - 0.051461 * z;
    let g_c = -0.250268 * x + 1.204414 * y + 0.045854 * z;
    let b_c = -0.002079 * x + 0.048952 * y + 0.953127 * z;
    let r_d = v.rgb_d[0] * r_c;
    let g_d = v.rgb_d[1] * g_c;
    let b_d = v.rgb_d[2] * b_c;
    let r_af = (v.fl * r_d.abs() / 100.0).powf(0.42);
    let g_af = (v.fl * g_d.abs() / 100.0).powf(0.42);
    let b_af = (v.fl * b_d.abs() / 100.0).powf(0.42);
    let r_a = signum(r_d) * 400.0 * r_af / (r_af + 27.13);
    let g_a = signum(g_d) * 400.0 * g_af / (g_af + 27.13);
    let b_a = signum(b_d) * 400.0 * b_af / (b_af + 27.13);
    let a = (11.0 * r_a + -12.0 * g_a + b_a) / 11.0;
    let b = (r_a + g_a - 2.0 * b_a) / 9.0;
    let u = (20.0 * r_a + 20.0 * g_a + 21.0 * b_a) / 20.0;
    let p2 = (40.0 * r_a + 20.0 * g_a + b_a) / 20.0;
    let atan_degrees = b.atan2(a) * 180.0 / PI;
    let hue = sanitize_degrees_double(atan_degrees);
    let ac = p2 * v.nbb;
    let j = 100.0 * (ac / v.aw).powf(v.c * v.z);
    let hue_prime = if hue < 20.14 { hue + 360.0 } else { hue };
    let e_hue = 0.25 * ((hue_prime * PI / 180.0 + 2.0).cos() + 3.8);
    let p1 = 50000.0 / 13.0 * e_hue * v.nc * v.ncb;
    let t = p1 * (a * a + b * b).sqrt() / (u + 0.305);
    let alpha = t.powf(0.9) * (1.64 - 0.29f64.powf(v.n)).powf(0.73);
    let c = alpha * (j / 100.0).sqrt();
    (hue, c)
}

// ---------------------------------------------------------------- HCT

#[derive(Clone, Copy, Debug)]
pub struct Hct {
    pub hue: f64,
    pub chroma: f64,
    pub tone: f64,
    pub argb: u32,
}

impl Hct {
    pub fn from_int(argb: u32) -> Hct {
        let (hue, chroma) = cam16_hue_chroma(argb);
        Hct { hue, chroma, tone: lstar_from_argb(argb), argb }
    }
    pub fn from_hct(hue: f64, chroma: f64, tone: f64) -> Hct {
        Hct::from_int(solve_to_int(hue, chroma, tone))
    }
    pub fn is_blue(hue: f64) -> bool {
        (250.0..270.0).contains(&hue)
    }
    pub fn is_yellow(hue: f64) -> bool {
        (105.0..125.0).contains(&hue)
    }
    pub fn is_cyan(hue: f64) -> bool {
        (170.0..207.0).contains(&hue)
    }
}

// ---------------------------------------------------------------- HctSolver

const SCALED_DISCOUNT_FROM_LINRGB: [[f64; 3]; 3] = [
    [0.001200833568784504, 0.002389694492170889, 0.0002795742885861124],
    [0.0005891086651375999, 0.0029785502573438758, 0.0003270666104008398],
    [0.00010146692491640572, 0.0005364214359186694, 0.0032979401770712076],
];
const LINRGB_FROM_SCALED_DISCOUNT: [[f64; 3]; 3] = [
    [1373.2198709594231, -1100.4251190754821, -7.278681089101213],
    [-271.815969077903, 559.6580465940733, -32.46047482791194],
    [1.9622899599665666, -57.173814538844006, 308.7233197812385],
];
const Y_FROM_LINRGB: [f64; 3] = [0.2126, 0.7152, 0.0722];

fn sanitize_radians(angle: f64) -> f64 {
    fmod(angle + PI * 8.0, PI * 2.0)
}

fn true_delinearized(c: f64) -> f64 {
    let n = c / 100.0;
    let d = if n <= 0.0031308 { n * 12.92 } else { 1.055 * n.powf(1.0 / 2.4) - 0.055 };
    d * 255.0
}

fn chromatic_adaptation(c: f64) -> f64 {
    let af = c.abs().powf(0.42);
    signum(c) * 400.0 * af / (af + 27.13)
}

fn hue_of(linrgb: [f64; 3]) -> f64 {
    let sd = matrix_multiply(linrgb, &SCALED_DISCOUNT_FROM_LINRGB);
    let r_a = chromatic_adaptation(sd[0]);
    let g_a = chromatic_adaptation(sd[1]);
    let b_a = chromatic_adaptation(sd[2]);
    let a = (11.0 * r_a + -12.0 * g_a + b_a) / 11.0;
    let b = (r_a + g_a - 2.0 * b_a) / 9.0;
    b.atan2(a)
}

fn are_in_cyclic_order(a: f64, b: f64, c: f64) -> bool {
    sanitize_radians(b - a) < sanitize_radians(c - a)
}

fn lerp_point(s: [f64; 3], t: f64, e: [f64; 3]) -> [f64; 3] {
    [s[0] + (e[0] - s[0]) * t, s[1] + (e[1] - s[1]) * t, s[2] + (e[2] - s[2]) * t]
}

fn set_coordinate(s: [f64; 3], coordinate: f64, e: [f64; 3], axis: usize) -> [f64; 3] {
    let t = (coordinate - s[axis]) / (e[axis] - s[axis]);
    lerp_point(s, t, e)
}

fn is_bounded(x: f64) -> bool {
    (0.0..=100.0).contains(&x)
}

fn nth_vertex(y: f64, n: i64) -> [f64; 3] {
    let [kr, kg, kb] = Y_FROM_LINRGB;
    let coord_a = if n % 4 <= 1 { 0.0 } else { 100.0 };
    let coord_b = if n % 2 == 0 { 0.0 } else { 100.0 };
    if n < 4 {
        let (g, b) = (coord_a, coord_b);
        let r = (y - g * kg - b * kb) / kr;
        if is_bounded(r) { [r, g, b] } else { [-1.0; 3] }
    } else if n < 8 {
        let (b, r) = (coord_a, coord_b);
        let g = (y - r * kr - b * kb) / kg;
        if is_bounded(g) { [r, g, b] } else { [-1.0; 3] }
    } else {
        let (r, g) = (coord_a, coord_b);
        let b = (y - r * kr - g * kg) / kb;
        if is_bounded(b) { [r, g, b] } else { [-1.0; 3] }
    }
}

fn bisect_to_segment(y: f64, target_hue: f64) -> ([f64; 3], [f64; 3]) {
    let mut left = [-1.0; 3];
    let mut right = left;
    let mut left_hue = 0.0;
    let mut right_hue = 0.0;
    let mut initialized = false;
    let mut uncut = true;
    for n in 0..12 {
        let mid = nth_vertex(y, n);
        if mid[0] < 0.0 {
            continue;
        }
        let mid_hue = hue_of(mid);
        if !initialized {
            left = mid;
            right = mid;
            left_hue = mid_hue;
            right_hue = mid_hue;
            initialized = true;
            continue;
        }
        if uncut || are_in_cyclic_order(left_hue, mid_hue, right_hue) {
            uncut = false;
            if are_in_cyclic_order(left_hue, target_hue, mid_hue) {
                right = mid;
                right_hue = mid_hue;
            } else {
                left = mid;
                left_hue = mid_hue;
            }
        }
    }
    (left, right)
}

fn bisect_to_limit(y: f64, target_hue: f64) -> [f64; 3] {
    let (mut left, mut right) = bisect_to_segment(y, target_hue);
    let mut left_hue = hue_of(left);
    for axis in 0..3 {
        if left[axis] != right[axis] {
            let (mut l_plane, mut r_plane): (i64, i64);
            if left[axis] < right[axis] {
                l_plane = (true_delinearized(left[axis]) - 0.5).floor() as i64;
                r_plane = (true_delinearized(right[axis]) - 0.5).ceil() as i64;
            } else {
                l_plane = (true_delinearized(left[axis]) - 0.5).ceil() as i64;
                r_plane = (true_delinearized(right[axis]) - 0.5).floor() as i64;
            }
            for _ in 0..8 {
                if (r_plane - l_plane).abs() <= 1 {
                    break;
                }
                let m_plane = ((l_plane + r_plane) as f64 / 2.0).floor() as i64;
                let mid = set_coordinate(left, CRITICAL_PLANES[m_plane as usize], right, axis);
                let mid_hue = hue_of(mid);
                if are_in_cyclic_order(left_hue, target_hue, mid_hue) {
                    right = mid;
                    r_plane = m_plane;
                } else {
                    left = mid;
                    left_hue = mid_hue;
                    l_plane = m_plane;
                }
            }
        }
    }
    [(left[0] + right[0]) / 2.0, (left[1] + right[1]) / 2.0, (left[2] + right[2]) / 2.0]
}

fn inverse_chromatic_adaptation(adapted: f64) -> f64 {
    let abs = adapted.abs();
    let x = 27.13 * abs / (400.0 - abs);
    let base = if x > 0.0 { x } else { 0.0 };
    signum(adapted) * base.powf(1.0 / 0.42)
}

fn find_result_by_j(hue_radians: f64, chroma: f64, y: f64) -> u32 {
    let mut j = y.sqrt() * 11.0;
    let v = vc();
    let t_inner_coeff = 1.0 / (1.64 - 0.29f64.powf(v.n)).powf(0.73);
    let e_hue = 0.25 * ((hue_radians + 2.0).cos() + 3.8);
    let p1 = e_hue * (50000.0 / 13.0) * v.nc * v.ncb;
    let h_sin = hue_radians.sin();
    let h_cos = hue_radians.cos();
    for round in 0..5 {
        let j_normalized = j / 100.0;
        let alpha = if chroma != 0.0 && j != 0.0 { chroma / j_normalized.sqrt() } else { 0.0 };
        let t = (alpha * t_inner_coeff).powf(1.0 / 0.9);
        let ac = v.aw * j_normalized.powf(1.0 / v.c / v.z);
        let p2 = ac / v.nbb;
        let gamma = 23.0 * (p2 + 0.305) * t / (23.0 * p1 + 11.0 * t * h_cos + 108.0 * t * h_sin);
        let a = gamma * h_cos;
        let b = gamma * h_sin;
        let r_a = (460.0 * p2 + 451.0 * a + 288.0 * b) / 1403.0;
        let g_a = (460.0 * p2 - 891.0 * a - 261.0 * b) / 1403.0;
        let b_a = (460.0 * p2 - 220.0 * a - 6300.0 * b) / 1403.0;
        let scaled =
            [inverse_chromatic_adaptation(r_a), inverse_chromatic_adaptation(g_a), inverse_chromatic_adaptation(b_a)];
        let linrgb = matrix_multiply(scaled, &LINRGB_FROM_SCALED_DISCOUNT);
        if linrgb[0] < 0.0 || linrgb[1] < 0.0 || linrgb[2] < 0.0 {
            return 0;
        }
        let fnj = Y_FROM_LINRGB[0] * linrgb[0] + Y_FROM_LINRGB[1] * linrgb[1] + Y_FROM_LINRGB[2] * linrgb[2];
        if fnj <= 0.0 {
            return 0;
        }
        if round == 4 || (fnj - y).abs() < 0.002 {
            if linrgb[0] > 100.01 || linrgb[1] > 100.01 || linrgb[2] > 100.01 {
                return 0;
            }
            return argb_from_linrgb(linrgb);
        }
        j = j - (fnj - y) * j / (2.0 * fnj);
    }
    0
}

pub fn solve_to_int(hue_degrees: f64, chroma: f64, lstar: f64) -> u32 {
    if chroma < 0.0001 || lstar < 0.0001 || lstar > 99.9999 {
        return argb_from_lstar(lstar);
    }
    let hue_degrees = sanitize_degrees_double(hue_degrees);
    let hue_radians = hue_degrees / 180.0 * PI;
    let y = y_from_lstar(lstar);
    let exact = find_result_by_j(hue_radians, chroma, y);
    if exact != 0 {
        return exact;
    }
    argb_from_linrgb(bisect_to_limit(y, hue_radians))
}
