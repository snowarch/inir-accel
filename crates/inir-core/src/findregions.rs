//! `find-regions`: the region selector's content targets, as `scripts/images/find_regions.py` found them with
//! opencv-contrib-python 5.0.0: selective search in fast mode (ximgproc) over a tenth of the screenshot, scaled
//! back, filtered and suppressed. Ported from selectivesearchsegmentation.cpp and graphsegmentation.cpp with the
//! arithmetic cv2 runs on x86-64 (its AVX2+FMA blur, warp and convert kernels), libstdc++'s introsort where ties
//! pick the order, and glibc's `rand()` for the ranks.

use crate::region::decode::{imread, Read};
use crate::region::resize::{resize, Interp};
use crate::region::Mat;
use std::collections::BTreeSet;

const DEFAULT_IMAGE: &str = "/tmp/quickshell/media/screenshot/image";
const USAGE: &str = "usage: inir-accel find-regions [-i IMAGE] [--min-width N] [--min-height N] [--max-width N] \
[--max-height N] [--single] [--k N] [--min-size N] [--sigma F] [--resize-factor F] [--hyprctl]";

struct Args {
    image: String,
    min_w: i64,
    min_h: i64,
    max_w: Option<i64>,
    max_h: Option<i64>,
    single: bool,
    hyprctl: bool,
    k: i32,
    inc_k: i32,
    sigma: f32,
    factor: f64,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        image: DEFAULT_IMAGE.into(),
        min_w: 200,
        min_h: 100,
        max_w: None,
        max_h: None,
        single: false,
        hyprctl: false,
        k: 3000,
        inc_k: 50,
        sigma: 0.6,
        factor: 0.1,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        match key {
            "--single" => a.single = true,
            "--hyprctl" => a.hyprctl = true,
            "-i" | "--image" | "--min-width" | "--min-height" | "--max-width" | "--max-height" | "--k" | "--min-size"
            | "--sigma" | "--resize-factor" => {
                let v = match inline {
                    Some(v) => v,
                    None => it.next().ok_or(format!("argument {key}: expected one argument"))?.clone(),
                };
                let int = || v.trim().parse::<i64>().map_err(|_| format!("argument {key}: invalid int value: '{v}'"));
                let float = || v.trim().parse::<f64>().map_err(|_| format!("argument {key}: invalid float value: '{v}'"));
                match key {
                    "-i" | "--image" => a.image = v.clone(),
                    "--min-width" => a.min_w = int()?,
                    "--min-height" => a.min_h = int()?,
                    "--max-width" => a.max_w = Some(int()?),
                    "--max-height" => a.max_h = Some(int()?),
                    "--k" => a.k = int()? as i32,
                    "--min-size" => a.inc_k = int()? as i32,
                    "--sigma" => a.sigma = float()? as f32,
                    _ => a.factor = float()?,
                }
            }
            _ => return Err(format!("unrecognized arguments: {arg}")),
        }
    }
    if a.inc_k == 0 {
        return Err("argument --min-size: must not be 0".into());
    }
    Ok(a)
}

pub fn run_cli(args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{USAGE}\nfind-regions: error: {e}");
            return 2;
        }
    };
    let img = match imread(&a.image, false, crate::region::site_packages().as_deref()) {
        Read::Ok(m) => m,
        Read::None => {
            eprintln!("Error: Could not load image {}", a.image);
            return 1;
        }
    };
    let (ow, oh) = (img.w as i64, img.h as i64);
    let small = if a.factor != 1.0 {
        let (dw, dh) = ((ow as f64 * a.factor) as i64, (oh as f64 * a.factor) as i64);
        if dw <= 0 || dh <= 0 {
            eprintln!("Error: resize factor {} leaves no pixels of {}x{}", a.factor, ow, oh);
            return 1;
        }
        resize(&img, dw as usize, dh as usize, Interp::Area)
    } else {
        img
    };
    let Some(rects) = selective_search(&small, a.k, a.inc_k, a.sigma) else {
        eprintln!("Error: the image is too small to search");
        return 1;
    };
    let scale = |v: i32| if a.factor != 1.0 { (v as f64 / a.factor) as i64 } else { v as i64 };
    let mut regions = Vec::new();
    for r in rects {
        let (x, y, w, h) = (scale(r.x), scale(r.y), scale(r.w), scale(r.h));
        if w == ow && h == oh && x == 0 && y == 0 {
            continue;
        }
        if w > a.min_w && h > a.min_h && a.max_w.is_none_or(|m| w < m) && a.max_h.is_none_or(|m| h < m) {
            regions.push([x, y, w, h]);
        }
    }
    let mut regions = suppress(regions, 0.7);
    if a.single {
        regions.truncate(1);
    }
    let items: Vec<String> = regions
        .iter()
        .map(|[x, y, w, h]| {
            if a.hyprctl {
                format!("{{\"at\": [{x}, {y}], \"size\": [{w}, {h}]}}")
            } else {
                format!("{{\"x\": {x}, \"y\": {y}, \"width\": {w}, \"height\": {h}}}")
            }
        })
        .collect();
    println!("[{}]", items.join(", "));
    0
}

fn iou(a: &[i64; 4], b: &[i64; 4]) -> f64 {
    let (xa, ya) = (a[0].max(b[0]), a[1].max(b[1]));
    let (xb, yb) = ((a[0] + a[2]).min(b[0] + b[2]), (a[1] + a[3]).min(b[1] + b[3]));
    let inter = (xb - xa).max(0) * (yb - ya).max(0);
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union > 0 { inter as f64 / union as f64 } else { 0.0 }
}

/// Largest first (stable, as Python's sorted), each kept box drops the rest it overlaps by `limit` or more.
fn suppress(mut regions: Vec<[i64; 4]>, limit: f64) -> Vec<[i64; 4]> {
    regions.sort_by(|a, b| (b[2] * b[3]).cmp(&(a[2] * a[3])));
    let mut keep = Vec::new();
    let mut rest = regions.as_slice().to_vec();
    while !rest.is_empty() {
        let cur = rest.remove(0);
        rest.retain(|r| iou(&cur, r) < limit);
        keep.push(cur);
    }
    keep
}

// glibc rand() and libstdc++ std::sort: the ranks and every tie in the grouping come out of these two.

/// glibc's `rand()` from its default seed: the TYPE_3 additive feedback generator.
struct CRand {
    r: [u32; 34],
    k: usize,
}

impl CRand {
    fn new() -> Self {
        let mut seq: Vec<u32> = vec![1];
        for i in 1..31 {
            seq.push(((16807 * seq[i - 1] as i64) % 2147483647) as u32);
        }
        for i in 31..34 {
            seq.push(seq[i - 31]);
        }
        for i in 34..344 {
            seq.push(seq[i - 31].wrapping_add(seq[i - 3]));
        }
        let mut r = [0u32; 34];
        for (i, v) in seq.iter().enumerate().skip(310) {
            r[i % 34] = *v;
        }
        CRand { r, k: 344 }
    }

    fn next(&mut self) -> i32 {
        let k = self.k;
        let v = self.r[(k - 31) % 34].wrapping_add(self.r[(k - 3) % 34]);
        self.r[k % 34] = v;
        self.k += 1;
        (v >> 1) as i32
    }
}

fn std_sort<T: Copy>(v: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    let n = v.len();
    if n > 1 {
        introsort(v, 0, n, 2 * (usize::BITS - 1 - n.leading_zeros()) as usize, less);
        if n > 16 {
            insertion_sort(v, 0, 16, less);
            for i in 16..n {
                linear_insert(v, i, less);
            }
        } else {
            insertion_sort(v, 0, n, less);
        }
    }
}

fn introsort<T: Copy>(v: &mut [T], first: usize, mut last: usize, mut depth: usize, less: &impl Fn(&T, &T) -> bool) {
    while last - first > 16 {
        if depth == 0 {
            heap_sort(&mut v[first..last], less);
            return;
        }
        depth -= 1;
        let mid = first + (last - first) / 2;
        let (a, b, c) = (first + 1, mid, last - 1);
        let median = if less(&v[a], &v[b]) {
            if less(&v[b], &v[c]) { b } else if less(&v[a], &v[c]) { c } else { a }
        } else if less(&v[a], &v[c]) {
            a
        } else if less(&v[b], &v[c]) {
            c
        } else {
            b
        };
        v.swap(first, median);
        let (mut lo, mut hi) = (first + 1, last);
        let cut = loop {
            while less(&v[lo], &v[first]) {
                lo += 1;
            }
            hi -= 1;
            while less(&v[first], &v[hi]) {
                hi -= 1;
            }
            if lo >= hi {
                break lo;
            }
            v.swap(lo, hi);
            lo += 1;
        };
        introsort(v, cut, last, depth, less);
        last = cut;
    }
}

fn insertion_sort<T: Copy>(v: &mut [T], first: usize, last: usize, less: &impl Fn(&T, &T) -> bool) {
    for i in first + 1..last {
        if less(&v[i], &v[first]) {
            let val = v[i];
            v.copy_within(first..i, first + 1);
            v[first] = val;
        } else {
            linear_insert(v, i, less);
        }
    }
}

fn linear_insert<T: Copy>(v: &mut [T], mut last: usize, less: &impl Fn(&T, &T) -> bool) {
    let val = v[last];
    while last > 0 && less(&val, &v[last - 1]) {
        v[last] = v[last - 1];
        last -= 1;
    }
    v[last] = val;
}

fn heap_sort<T: Copy>(v: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    let len = v.len();
    if len >= 2 {
        let mut parent = (len - 2) / 2;
        loop {
            adjust_heap(v, parent, len, v[parent], less);
            if parent == 0 {
                break;
            }
            parent -= 1;
        }
    }
    let mut last = len;
    while last > 1 {
        last -= 1;
        let value = v[last];
        v[last] = v[0];
        adjust_heap(v, 0, last, value, less);
    }
}

fn adjust_heap<T: Copy>(v: &mut [T], mut hole: usize, len: usize, value: T, less: &impl Fn(&T, &T) -> bool) {
    let top = hole;
    let mut child = hole;
    while child < (len - 1) / 2 {
        child = 2 * (child + 1);
        if less(&v[child], &v[child - 1]) {
            child -= 1;
        }
        v[hole] = v[child];
        hole = child;
    }
    if len & 1 == 0 && child == (len - 2) / 2 {
        child = 2 * (child + 1);
        v[hole] = v[child - 1];
        hole = child - 1;
    }
    while hole > top {
        let parent = (hole - 1) / 2;
        if !less(&v[parent], &value) {
            break;
        }
        v[hole] = v[parent];
        hole = parent;
    }
    v[hole] = value;
}

// OpenCV's rounding: cvRound is round-half-even, and an out of range float converts to INT_MIN.

fn round_f32(v: f32) -> i32 {
    if v.is_nan() || !(-2147483648.0..2147483648.0).contains(&v) { i32::MIN } else { v.round_ties_even() as i32 }
}

fn sat_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

// Colour conversions, 8-bit and integer as cvtColor runs them.

fn bgr2hsv(src: &Mat) -> Mat {
    let (mut sdiv, mut hdiv) = ([0i32; 256], [0i32; 256]);
    for i in 1..256 {
        sdiv[i] = ((255 << 12) as f64 / i as f64).round_ties_even() as i32;
        hdiv[i] = ((180 << 12) as f64 / (6.0 * i as f64)).round_ties_even() as i32;
    }
    let mut data = vec![0u8; src.data.len()];
    for (s, d) in src.data.chunks_exact(3).zip(data.chunks_exact_mut(3)) {
        let (b, g, r) = (s[0] as i32, s[1] as i32, s[2] as i32);
        let v = b.max(g).max(r);
        let diff = v - b.min(g).min(r);
        let vr = if v == r { -1 } else { 0 };
        let vg = if v == g { -1 } else { 0 };
        let sat = (diff * sdiv[v as usize] + (1 << 11)) >> 12;
        let mut h = (vr & (g - b)) + (!vr & ((vg & (b - r + 2 * diff)) + (!vg & (r - g + 4 * diff))));
        h = (h * hdiv[diff as usize] + (1 << 11)) >> 12;
        if h < 0 {
            h += 180;
        }
        d[0] = sat_u8(h);
        d[1] = sat_u8(sat);
        d[2] = v as u8;
    }
    Mat { w: src.w, h: src.h, cn: 3, data }
}

fn bgr2lab(src: &Mat) -> Mat {
    let mut gamma = [0i32; 256];
    for (i, g) in gamma.iter_mut().enumerate() {
        let x = (i as f32 / 255.0) as f64;
        let lin = if x <= 809.0 / 20000.0 { x / (323.0 / 25.0) } else { ((x + 0.055) / (1.0 + 0.055)).powf(2.4) };
        *g = (2040.0f32 * lin as f32).round_ties_even() as i32;
    }
    let (lthresh, lscale, lbias) = (216.0f32 / 24389.0, 841.0f32 / 108.0, 16.0f32 / 116.0);
    let step = 1.0f32 / 2040.0;
    let cbrt: Vec<i32> = (0..3072)
        .map(|i| {
            let x = step * i as f32;
            let f = if x < lthresh { x.mul_add(lscale, lbias) } else { soft_cbrt(x) };
            (32768.0f32 * f).round_ties_even() as i32
        })
        .collect();
    const XYZ: [u64; 9] = [
        0x3fda65a14488c60d, 0x3fd6e297396d0918, 0x3fc71819d2391d58, 0x3fcb38cda6e75ff6, 0x3fe6e297396d0918,
        0x3fb279aae6c8f755, 0x3f93cc4ac6cdaf4b, 0x3fbe836eb4e98138, 0x3fee68427418d691,
    ];
    let white = [f64::from_bits(0x3fee6a22b3892ee8), 1.0, f64::from_bits(0x3ff16b8950763a19)];
    // Per output row, the weights of the B, G and R bytes in that order.
    let mut c = [[0i32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][2 - j] = (4096.0 * f64::from_bits(XYZ[i * 3 + j]) / white[i]).round_ties_even() as i32;
        }
    }
    let descale = |v: i32, n: u32| (v + (1 << (n - 1))) >> n;
    let lshift = -((16 * 255 * (1 << 15) + 50) / 100);
    let mut data = vec![0u8; src.data.len()];
    for (s, d) in src.data.chunks_exact(3).zip(data.chunks_exact_mut(3)) {
        let (b, g, r) = (gamma[s[0] as usize], gamma[s[1] as usize], gamma[s[2] as usize]);
        let f = |k: usize| cbrt[descale(b * c[k][0] + g * c[k][1] + r * c[k][2], 12) as usize];
        let (fx, fy, fz) = (f(0), f(1), f(2));
        d[0] = sat_u8(descale(296 * fy + lshift, 15));
        d[1] = sat_u8(descale(500 * (fx - fy) + (128 << 15), 15));
        d[2] = sat_u8(descale(200 * (fy - fz) + (128 << 15), 15));
    }
    Mat { w: src.w, h: src.h, cn: 3, data }
}

/// softfloat's `f32_cbrt`, which builds the Lab table: a rational polynomial in doubles with the fraction cut, not
/// rounded, back to 23 bits.
fn soft_cbrt(x: f32) -> f32 {
    let bits = x.to_bits();
    if bits & 0x7fff_ffff == 0 {
        return 0.0;
    }
    let mut ex = ((bits >> 23) & 0xff) as i32 - 127;
    let mut shx = ex % 3;
    if shx >= 0 {
        shx -= 3;
    }
    ex = (ex - shx) / 3 - 1;
    let fr = f64::from_bits((((shx + 1023) as u64) << 52) | (((bits & 0x7f_ffff) as u64) << 29));
    let c = |r: u64| f64::from_bits(r);
    let num = (((c(0x4046a09e6653ba70) * fr + c(0x406808f46c6116e0)) * fr + c(0x405dca97439cae14)) * fr + c(0x402add70d2827500)) * fr
        + c(0x3fc4f15f83f55d2d);
    let den = (((c(0x402d9e20660edb21) * fr + c(0x4062ff15c0285815)) * fr + c(0x406510d06a8112ce)) * fr + c(0x4040fecbc9e2c375)) * fr + 1.0;
    let frac = (((num / den).to_bits() & 0xf_ffff_ffff_ffff) >> 29) as u32;
    f32::from_bits((bits & 0x8000_0000) + (((ex + 127) as u32) << 23) + frac)
}

// Filters: BORDER_REFLECT_101, the row pass then the column pass, with the FMA chains of cv2's AVX2 kernels.

fn reflect(mut p: i64, len: usize) -> usize {
    let len = len as i64;
    if len == 1 {
        return 0;
    }
    while p < 0 || p >= len {
        p = if p < 0 { -p } else { 2 * len - p - 2 };
    }
    p as usize
}

fn gaussian_kernel(n: usize, sigma: f64) -> Vec<f32> {
    let scale = -0.125 / (sigma * sigma);
    let half = (n - 1) / 2;
    let values: Vec<f64> = (0..half).map(|i| ((1 - n as i64 + 2 * i as i64).pow(2) as f64 * scale).exp()).collect();
    let mut sum = values.iter().fold(0.0, |s, v| s + v);
    sum = sum * 2.0 + 1.0;
    let mul = 1.0 / sum;
    let mut k = vec![0f32; n];
    for (i, v) in values.iter().enumerate() {
        k[i] = (v * mul) as f32;
        k[n - 1 - i] = k[i];
    }
    k[half] = mul as f32;
    k
}

fn gaussian_blur(src: &[f32], w: usize, h: usize, cn: usize, sigma: f64) -> Vec<f32> {
    let n = ((sigma * 4.0 * 2.0 + 1.0).round_ties_even() as usize) | 1;
    let kx = gaussian_kernel(if w == 1 { 1 } else { n }, sigma);
    let ky = gaussian_kernel(if h == 1 { 1 } else { n }, sigma);
    if kx.len() == 1 && ky.len() == 1 {
        return src.to_vec();
    }
    let (rx, ry, stride) = (kx.len() / 2, ky.len() / 2, w * cn);
    let mut rows = vec![0f32; w * h * cn];
    let mut ext = vec![0f32; (w + 2 * rx) * cn];
    for y in 0..h {
        let row = &src[y * stride..(y + 1) * stride];
        for xx in 0..w + 2 * rx {
            let sx = reflect(xx as i64 - rx as i64, w);
            ext[xx * cn..(xx + 1) * cn].copy_from_slice(&row[sx * cn..(sx + 1) * cn]);
        }
        for i in 0..stride {
            rows[y * stride + i] = kx.iter().enumerate().fold(0.0f32, |s, (k, &f)| ext[i + k * cn].mul_add(f, s));
        }
    }
    let mut out = vec![0f32; w * h * cn];
    for y in 0..h {
        for i in 0..stride {
            let mut s = rows[y * stride + i].mul_add(ky[ry], 0.0);
            for k in 1..=ry {
                let a = rows[reflect(y as i64 + k as i64, h) * stride + i];
                let b = rows[reflect(y as i64 - k as i64, h) * stride + i];
                s = (a + b).mul_add(ky[ry + k], s);
            }
            out[y * stride + i] = s;
        }
    }
    out
}

/// `Scharr(plane, CV_32F, dx, dy)`: integer kernels, so every order gives the same floats.
fn scharr(src: &[u8], w: usize, h: usize, dx: bool) -> Vec<f32> {
    let at = |x: i64, y: usize| src[y * w + reflect(x, w)] as i32;
    let mut rows = vec![0i32; w * h];
    for y in 0..h {
        for x in 0..w as i64 {
            rows[y * w + x as usize] =
                if dx { at(x + 1, y) - at(x - 1, y) } else { 3 * at(x - 1, y) + 10 * at(x, y) + 3 * at(x + 1, y) };
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h as i64 {
        for x in 0..w {
            let r = |dy: i64| rows[reflect(y + dy, h) * w + x];
            out[y as usize * w + x] = if dx { 3 * r(-1) + 10 * r(0) + 3 * r(1) } else { r(1) - r(-1) } as f32;
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Rect {
    fn union(self, b: Rect) -> Rect {
        if self.w <= 0 || self.h <= 0 {
            return b;
        }
        if b.w <= 0 || b.h <= 0 {
            return self;
        }
        let (x, y) = (self.x.min(b.x), self.y.min(b.y));
        Rect { x, y, w: (self.x + self.w).max(b.x + b.w) - x, h: (self.y + self.h).max(b.y + b.h) - y }
    }
}

fn rotation_matrix(cx: f32, cy: f32, angle: f64) -> [f64; 6] {
    let a = angle * std::f64::consts::PI / 180.0;
    let (alpha, beta) = (a.cos(), a.sin());
    let (cx, cy) = (cx as f64, cy as f64);
    [alpha, beta, (1.0 - alpha) * cx - beta * cy, -beta, alpha, beta * cx + (1.0 - alpha) * cy]
}

/// `RotatedRect(center, size, angle).boundingRect()`.
fn rotated_bounds(cx: f32, cy: f32, w: f32, h: f32, angle: f32) -> Rect {
    let rad = angle as f64 * std::f64::consts::PI / 180.0;
    let (b, a) = (rad.cos() as f32 * 0.5, rad.sin() as f32 * 0.5);
    let (ah, aw, bh, bw) = (a * h, a * w, b * h, b * w);
    let xs = [cx - ah - bw, cx + ah - bw, cx + ah + bw, cx - ah + bw];
    let ys = [cy + bh - aw, cy - bh - aw, cy - bh + aw, cy + bh + aw];
    let fold = |v: &[f32; 4], f: fn(f32, f32) -> f32| f(f(f(v[0], v[1]), v[2]), v[3]);
    let (x0, y0) = (fold(&xs, f32::min).floor() as i32, fold(&ys, f32::min).floor() as i32);
    let (x1, y1) = (fold(&xs, f32::max).ceil() as i32, fold(&ys, f32::max).ceil() as i32);
    Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }
}

/// `warpAffine(src, M, (dw, dh))`: bilinear, constant black border, through the inverted matrix in floats.
fn warp_affine(src: &[f32], sw: usize, sh: usize, m: [f64; 6], dw: usize, dh: usize) -> Vec<f32> {
    let mut m = m;
    let d = m[0] * m[4] - m[1] * m[3];
    let d = if d != 0.0 { 1.0 / d } else { 0.0 };
    let (a11, a22) = (m[4] * d, m[0] * d);
    m[0] = a11;
    m[1] *= -d;
    m[3] *= -d;
    m[4] = a22;
    let (b1, b2) = (-m[0] * m[2] - m[1] * m[5], -m[3] * m[2] - m[4] * m[5]);
    m[2] = b1;
    m[5] = b2;
    let f = m.map(|v| v as f32);
    let (cols, rows) = (sw as i32, sh as i32);
    let px = |x: i32, y: i32| if (x as u32) < cols as u32 && (y as u32) < rows as u32 { src[y as usize * sw + x as usize] } else { 0.0 };
    let mut out = vec![0f32; dw * dh];
    for y in 0..dh {
        let (mx, my) = (y as f32 * f[1] + f[2], y as f32 * f[4] + f[5]);
        for x in 0..dw {
            let (sx, sy) = (f[0].mul_add(x as f32, mx), f[3].mul_add(x as f32, my));
            let (ix, iy) = (sx.floor() as i32, sy.floor() as i32);
            let (ax, ay) = (sx - ix as f32, sy - iy as f32);
            if (ix.wrapping_add(1) as u32) >= (cols + 1) as u32 || (iy.wrapping_add(1) as u32) >= (rows + 1) as u32 {
                continue;
            }
            let (p00, p01, p10, p11) = (px(ix, iy), px(ix + 1, iy), px(ix, iy + 1), px(ix + 1, iy + 1));
            let v0 = ax.mul_add(p01 - p00, p00);
            let v1 = ax.mul_add(p11 - p10, p10);
            out[y * dw + x] = ay.mul_add(v1 - v0, v0);
        }
    }
    out
}

/// The texture strategy's 8 gradient images per channel (x, y and both at 45 degrees, positive and negative),
/// each stretched to 0..255. They depend on the image alone, so one set serves every segmentation of it.
fn texture_planes(img: &Mat) -> Option<Vec<Vec<u8>>> {
    let (w, h) = (img.w, img.h);
    let mut grads: Vec<Vec<f32>> = Vec::with_capacity(24);
    let mut split = |g: Vec<f32>| {
        grads.push(g.iter().map(|&v| if v > 0.0 { v } else { 0.0 }).collect());
        grads.push(g.iter().map(|&v| if v > 0.0 { 0.0 } else { v }).collect());
    };
    for p in 0..3 {
        let plane: Vec<u8> = img.data.iter().skip(p).step_by(3).copied().collect();
        split(scharr(&plane, w, h, true));
        split(scharr(&plane, w, h, false));
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let mut rot = rotation_matrix(cx, cy, 45.0);
        let bb = rotated_bounds(cx, cy, w as f32, h as f32, 45.0);
        rot[2] += bb.w as f64 / 2.0 - cx as f64;
        rot[5] += bb.h as f64 / 2.0 - cy as f64;
        let (rw, rh) = (bb.w as usize, bb.h as usize);
        let planef: Vec<f32> = plane.iter().map(|&v| v as f32).collect();
        let turned: Vec<u8> = warp_affine(&planef, w, h, rot, rw, rh).iter().map(|&v| sat_u8(round_f32(v))).collect();
        let (sx, sy) = ((bb.w - w as i32).max(0) / 2, (bb.h - h as i32).max(0) / 2);
        for dx in [true, false] {
            let g = scharr(&turned, rw, rh, dx);
            let (c2x, c2y) = ((rw as f64 / 2.0) as i32 as f32, (rh as f64 / 2.0) as i32 as f32);
            let bb2 = rotated_bounds(c2x, c2y, rw as f32, rh as f32, -45.0);
            let back = warp_affine(&g, rw, rh, rotation_matrix(c2x, c2y, -45.0), bb2.w as usize, bb2.h as usize);
            if sx as usize + w > bb2.w as usize || sy as usize + h > bb2.h as usize {
                return None;
            }
            let crop = (0..h).flat_map(|y| {
                let start = (sy as usize + y) * bb2.w as usize + sx as usize;
                back[start..start + w].iter().copied()
            });
            split(crop.collect());
        }
    }
    Some(
        grads
            .iter()
            .map(|g| {
                let (lo, hi) = g.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v as f64), hi.max(v as f64)));
                let (alpha, beta) = ((255.0 / (hi - lo)) as f32, (-255.0 * lo / (hi - lo)) as f32);
                g.iter().map(|&v| sat_u8(round_f32(v.mul_add(alpha, beta)))).collect()
            })
            .collect(),
    )
}

// Graph segmentation (Felzenszwalb), as GraphSegmentationImpl::processImage.

#[derive(Clone, Copy)]
struct Edge {
    from: u32,
    to: u32,
    w: f32,
}

struct PointSet {
    parent: Vec<u32>,
    size: Vec<i32>,
}

impl PointSet {
    fn base(&mut self, p: u32) -> u32 {
        let mut b = p;
        while b != self.parent[b as usize] {
            b = self.parent[b as usize];
        }
        self.parent[p as usize] = b;
        b
    }

    fn join(&mut self, mut a: u32, mut b: u32) {
        if self.size[a as usize] < self.size[b as usize] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parent[b as usize] = a;
        self.size[a as usize] += self.size[b as usize];
    }
}

fn segment(img: &Mat, sigma: f64, k: f32) -> (Vec<usize>, usize) {
    const MIN_SIZE: i32 = 100;
    let (w, h) = (img.w, img.h);
    let src: Vec<f32> = img.data.iter().map(|&v| v as f32).collect();
    let f = gaussian_blur(&src, w, h, 3, sigma);
    let mut edges = Vec::with_capacity(w * h * 4);
    for i in 0..h as i64 {
        for j in 0..w as i64 {
            for (i2, j2) in [(i - 1, j), (i, j - 1), (i + 1, j), (i, j + 1)] {
                if i2 < 0 || i2 >= h as i64 || j2 < 0 || j2 >= w as i64 {
                    continue;
                }
                let (a, b) = ((i * w as i64 + j) as usize, (i2 * w as i64 + j2) as usize);
                let mut t = 0f32;
                for c in 0..3 {
                    let d = f[a * 3 + c] - f[b * 3 + c];
                    t += d * d;
                }
                edges.push(Edge { from: a as u32, to: b as u32, w: t.sqrt() });
            }
        }
    }
    std_sort(&mut edges, &|a: &Edge, b: &Edge| a.w < b.w);
    let n = w * h;
    let mut set = PointSet { parent: (0..n as u32).collect(), size: vec![1; n] };
    let mut thresholds = vec![k; n];
    for e in edges.iter_mut() {
        let (a, b) = (set.base(e.from), set.base(e.to));
        if a != b && e.w <= thresholds[a as usize] && e.w <= thresholds[b as usize] {
            set.join(a, b);
            let a = set.base(a);
            thresholds[a as usize] = e.w + k / set.size[a as usize] as f32;
            e.w = 0.0;
        }
    }
    for e in &edges {
        if e.w > 0.0 {
            let (a, b) = (set.base(e.from), set.base(e.to));
            if a != b && (set.size[a as usize] < MIN_SIZE || set.size[b as usize] < MIN_SIZE) {
                set.join(a, b);
            }
        }
    }
    let mut ids = vec![usize::MAX; n];
    let mut next = 0;
    let labels = (0..n as u32)
        .map(|p| {
            let b = set.base(p) as usize;
            if ids[b] == usize::MAX {
                ids[b] = next;
                next += 1;
            }
            ids[b]
        })
        .collect();
    (labels, next)
}

// Selective search: hierarchical grouping under two strategy mixes per segmentation.

#[derive(Clone, Copy)]
enum Kind {
    Color,
    Fill,
    Texture,
    Size,
}

const FAST_MIX_1: [(Kind, f32); 4] = [(Kind::Color, 0.25), (Kind::Fill, 0.25), (Kind::Texture, 0.25), (Kind::Size, 0.25)];
const FAST_MIX_2: [(Kind, f32); 3] = [(Kind::Fill, 0.3333), (Kind::Texture, 0.3333), (Kind::Size, 0.3333)];
const COLOR_BINS: usize = 75;
const TEXTURE_BINS: usize = 240;

struct Strategy<'a> {
    mix: &'a [(Kind, f32)],
    color: Vec<f32>,
    texture: Vec<f32>,
    fill: Vec<Rect>,
    image_size: i32,
}

impl Strategy<'_> {
    fn get(&self, r1: usize, r2: usize, sizes: &[i32]) -> f32 {
        let intersect = |h: &[f32], bins: usize| {
            let (a, b) = (&h[r1 * bins..(r1 + 1) * bins], &h[r2 * bins..(r2 + 1) * bins]);
            a.iter().zip(b).fold(0f32, |s, (x, y)| s + if y < x { *y } else { *x })
        };
        let total = self.mix.iter().fold(0f32, |s, m| s + m.1);
        let tt = self.mix.iter().fold(0f32, |tt, &(kind, weight)| {
            let v = match kind {
                Kind::Color => intersect(&self.color, COLOR_BINS),
                Kind::Texture => intersect(&self.texture, TEXTURE_BINS),
                Kind::Size => (1.0 - (sizes[r1] + sizes[r2]) as f32 / self.image_size as f32).min(1.0).max(0.0),
                Kind::Fill => {
                    let bb = self.fill[r1].union(self.fill[r2]);
                    (1.0 - (bb.w * bb.h - sizes[r1] - sizes[r2]) as f32 / self.image_size as f32).min(1.0).max(0.0)
                }
            };
            tt + weight * v
        });
        tt / total
    }

    fn merge(&mut self, r1: usize, r2: usize, sizes: &[i32]) {
        let (s1, s2) = (sizes[r1], sizes[r2]);
        let blend = |h: &mut Vec<f32>, bins: usize| {
            for i in 0..bins {
                let v = (h[r1 * bins + i] * s1 as f32 + h[r2 * bins + i] * s2 as f32) / (s1 + s2) as f32;
                h[r1 * bins + i] = v;
                h[r2 * bins + i] = v;
            }
        };
        for &(kind, _) in self.mix {
            match kind {
                Kind::Color => blend(&mut self.color, COLOR_BINS),
                Kind::Texture => blend(&mut self.texture, TEXTURE_BINS),
                Kind::Fill => {
                    self.fill[r1] = self.fill[r1].union(self.fill[r2]);
                    self.fill[r2] = self.fill[r1];
                }
                Kind::Size => {}
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Region {
    id: usize,
    level: i32,
    bb: Rect,
}

#[derive(Clone, Copy)]
struct Neighbour {
    from: usize,
    to: usize,
    sim: f32,
}

fn group(s: &mut Strategy, sizes: &[i32], pairs: &[(usize, usize)], rects: &[Rect], rng: &mut CRand, out: &mut Vec<(f64, Rect)>) {
    let mut sizes = sizes.to_vec();
    let mut regions: Vec<Region> = rects.iter().enumerate().map(|(id, &bb)| Region { id, level: 1, bb }).collect();
    let mut sims: Vec<Neighbour> = pairs.iter().map(|&(from, to)| Neighbour { from, to, sim: s.get(from, to, &sizes) }).collect();
    while !sims.is_empty() {
        std_sort(&mut sims, &|a: &Neighbour, b: &Neighbour| a.sim < b.sim);
        let p = sims.pop().unwrap();
        let (rf, rt) = (regions[p.from], regions[p.to]);
        regions.push(Region { id: rf.id.min(rt.id), level: rf.level.max(rt.level) + 1, bb: rf.bb.union(rt.bb) });
        s.merge(rf.id, rt.id, &sizes);
        sizes[rf.id] += sizes[rt.id];
        sizes[rt.id] = sizes[rf.id];
        let mut local: Vec<usize> = Vec::new();
        sims.retain(|n| {
            let touches_from = n.from == p.from || n.from == p.to;
            if !touches_from && n.to != p.from && n.to != p.to {
                return true;
            }
            let other = if touches_from { n.to } else { n.from };
            if !local.contains(&other) {
                local.push(other);
            }
            false
        });
        let new = regions.len() - 1;
        for to in local {
            sims.push(Neighbour { from: new, to, sim: s.get(regions[new].id, regions[to].id, &sizes) });
        }
    }
    for r in &regions {
        out.push((rng.next() as f64 / 2147483647.0 * r.level as f64, r.bb));
    }
}

/// `createSelectiveSearchSegmentation`, `switchToSelectiveSearchFast(base_k, inc_k, sigma)`, `process()`.
fn selective_search(img: &Mat, base_k: i32, inc_k: i32, sigma: f32) -> Option<Vec<Rect>> {
    let (w, h) = (img.w, img.h);
    let mut ks = Vec::new();
    let mut k = base_k;
    while k <= base_k + inc_k * 2 {
        ks.push(k);
        k += inc_k;
    }
    let mut rng = CRand::new();
    let mut all: Vec<(f64, Rect)> = Vec::new();
    for image in [bgr2hsv(img), bgr2lab(img)] {
        let planes = texture_planes(&image)?;
        for &k in &ks {
            let (labels, nb) = segment(&image, sigma as f64, k as f32);
            let mut sizes = vec![0i32; nb];
            let mut bounds = vec![(i32::MAX, i32::MAX, i32::MIN, i32::MIN); nb];
            let mut pairs = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    let l = labels[y * w + x];
                    sizes[l] += 1;
                    let b = &mut bounds[l];
                    *b = (b.0.min(x as i32), b.1.min(y as i32), b.2.max(x as i32), b.3.max(y as i32));
                    if y > 0 && x > 0 {
                        for o in [labels[y * w + x - 1], labels[(y - 1) * w + x], labels[(y - 1) * w + x - 1]] {
                            if o != l {
                                pairs.push((l.min(o), l.max(o)));
                            }
                        }
                    }
                }
            }
            pairs.sort_unstable();
            pairs.dedup();
            let rects: Vec<Rect> = bounds.iter().map(|b| Rect { x: b.0, y: b.1, w: b.2 - b.0 + 1, h: b.3 - b.1 + 1 }).collect();
            let mut color = vec![0f32; nb * COLOR_BINS];
            let mut counts = vec![0i32; nb * TEXTURE_BINS];
            for (i, &l) in labels.iter().enumerate() {
                for c in 0..3 {
                    color[l * COLOR_BINS + c * 25 + (image.data[i * 3 + c] as usize * 25 >> 8)] += 1.0;
                }
                for (g, plane) in planes.iter().enumerate() {
                    counts[l * TEXTURE_BINS + g * 10 + (plane[i] as f32 / 25.6f32) as usize] += 1;
                }
            }
            for l in 0..nb {
                let row = &mut color[l * COLOR_BINS..(l + 1) * COLOR_BINS];
                let tt = row.iter().fold(0f32, |s, v| s + v);
                row.iter_mut().for_each(|v| *v /= tt);
            }
            let texture: Vec<f32> = counts.iter().enumerate().map(|(i, &c)| c as f32 / (sizes[i / TEXTURE_BINS] * 24) as f32).collect();
            for (mix, color) in [(&FAST_MIX_1[..], color), (&FAST_MIX_2[..], Vec::new())] {
                let mut s = Strategy { mix, color, texture: texture.clone(), fill: rects.clone(), image_size: (w * h) as i32 };
                group(&mut s, &sizes, &pairs, &rects, &mut rng, &mut all);
            }
        }
    }
    std_sort(&mut all, &|a: &(f64, Rect), b: &(f64, Rect)| a.0 < b.0);
    let mut seen = BTreeSet::new();
    Some(all.into_iter().filter(|(_, r)| seen.insert(*r)).map(|(_, r)| r).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rand_is_glibcs() {
        let mut r = CRand::new();
        let first: Vec<i32> = (0..5).map(|_| r.next()).collect();
        assert_eq!(first, [1804289383, 846930886, 1681692777, 1714636915, 1957747793]);
    }

    #[test]
    fn sort_orders_like_std_sort() {
        let mut v: Vec<(i32, usize)> = (0..200).map(|i| ((i * 7919 % 13) as i32, i)).collect();
        std_sort(&mut v, &|a: &(i32, usize), b: &(i32, usize)| a.0 < b.0);
        assert!(v.windows(2).all(|p| p[0].0 <= p[1].0));
    }

    #[test]
    fn suppression_keeps_largest() {
        let kept = suppress(vec![[0, 0, 100, 100], [0, 0, 300, 300], [5, 5, 290, 290], [400, 400, 50, 50]], 0.7);
        assert_eq!(kept, vec![[0, 0, 300, 300], [0, 0, 100, 100], [400, 400, 50, 50]]);
    }
}
