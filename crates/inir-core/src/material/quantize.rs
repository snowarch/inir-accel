//! `QuantizeCelebi` (materialyoucolor 3.0.4's C++: `wu.cc`, `wsmeans.cc`, `lab.cc`) and `Score.score`.
//! The C++ is ported as compiled: C's `round` (half away from zero), the `int abs` that `wsmeans.cc` really calls,
//! and glibc's `rand()` seeded with 42688.

use super::hct::{difference_degrees, sanitize_degrees_int, Hct};
use std::collections::{BTreeMap, HashMap};

const WHITE: [f64; 3] = [95.047, 100.0, 108.883];

fn c_argb(r: i32, g: i32, b: i32) -> u32 {
    0xFF00_0000 | (((r & 0xff) as u32) << 16) | (((g & 0xff) as u32) << 8) | ((b & 0xff) as u32)
}

fn c_linearized(c: i32) -> f64 {
    let n = c as f64 / 255.0;
    if n <= 0.040449936 { n / 12.92 * 100.0 } else { ((n + 0.055) / 1.055).powf(2.4) * 100.0 }
}

fn c_delinearized(c: f64) -> i32 {
    let n = c / 100.0;
    let d = if n <= 0.0031308 { n * 12.92 } else { 1.055 * n.powf(1.0 / 2.4) - 0.055 };
    ((d * 255.0).round() as i32).clamp(0, 255)
}

#[derive(Clone, Copy)]
struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

impl Lab {
    fn delta_e(&self, o: &Lab) -> f64 {
        let dl = self.l - o.l;
        let da = self.a - o.a;
        let db = self.b - o.b;
        (dl * dl) + (da * da) + (db * db)
    }
}

fn lab_from_int(argb: u32) -> Lab {
    let r = ((argb & 0x00ff0000) >> 16) as i32;
    let g = ((argb & 0x0000ff00) >> 8) as i32;
    let b = (argb & 0x000000ff) as i32;
    let (rl, gl, bl) = (c_linearized(r), c_linearized(g), c_linearized(b));
    let x = 0.41233895 * rl + 0.35762064 * gl + 0.18051042 * bl;
    let y = 0.2126 * rl + 0.7152 * gl + 0.0722 * bl;
    let z = 0.01932141 * rl + 0.11916382 * gl + 0.95034478 * bl;
    let e = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    let f = |t: f64| if t > e { t.powf(1.0 / 3.0) } else { (kappa * t + 16.0) / 116.0 };
    let fy = f(y / WHITE[1]);
    let fx = f(x / WHITE[0]);
    let fz = f(z / WHITE[2]);
    Lab { l: 116.0 * fy - 16.0, a: 500.0 * (fx - fy), b: 200.0 * (fy - fz) }
}

fn int_from_lab(lab: Lab) -> u32 {
    let e = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    let ke = 8.0;
    let fy = (lab.l + 16.0) / 116.0;
    let fx = (lab.a / 500.0) + fy;
    let fz = fy - (lab.b / 200.0);
    let fx3 = fx * fx * fx;
    let xn = if fx3 > e { fx3 } else { (116.0 * fx - 16.0) / kappa };
    let yn = if lab.l > ke { fy * fy * fy } else { lab.l / kappa };
    let fz3 = fz * fz * fz;
    let zn = if fz3 > e { fz3 } else { (116.0 * fz - 16.0) / kappa };
    let x = xn * WHITE[0];
    let y = yn * WHITE[1];
    let z = zn * WHITE[2];
    let rl = 3.2406 * x - 1.5372 * y - 0.4986 * z;
    let gl = -0.9689 * x + 1.8758 * y + 0.0415 * z;
    let bl = 0.0557 * x - 0.2040 * y + 1.0570 * z;
    c_argb(c_delinearized(rl), c_delinearized(gl), c_delinearized(bl))
}

/// glibc's `random_r` TYPE_3 generator behind `srand`/`rand`.
struct GlibcRand {
    k: usize,
    ring: Vec<i32>,
}

impl GlibcRand {
    fn new(seed: u32) -> GlibcRand {
        let mut r = [0i32; 34];
        r[0] = if seed == 0 { 1 } else { seed as i32 };
        for i in 1..31 {
            let word = r[i - 1] as i64;
            let hi = word / 127773;
            let lo = word % 127773;
            let mut w = 16807 * lo - 2836 * hi;
            if w < 0 {
                w += 2147483647;
            }
            r[i] = w as i32;
        }
        for i in 31..34 {
            r[i] = r[i - 31];
        }
        let mut g = GlibcRand { k: 34, ring: r.to_vec() };
        for _ in 0..310 {
            g.next_raw();
        }
        g
    }
    fn next_raw(&mut self) -> i32 {
        let k = self.k;
        let v = self.ring[k - 31].wrapping_add(self.ring[k - 3]);
        self.ring.push(v);
        self.k += 1;
        if self.ring.len() > 4096 {
            self.ring.drain(..self.ring.len() - 64);
            self.k = self.ring.len();
        }
        v
    }
    fn rand(&mut self) -> i32 {
        ((self.next_raw() as u32) >> 1) as i32
    }
}

// ---------------------------------------------------------------- Wu

const INDEX_BITS: i32 = 5;
const INDEX_COUNT: i32 = (1 << INDEX_BITS) + 1;
const TOTAL_SIZE: usize = (INDEX_COUNT * INDEX_COUNT * INDEX_COUNT) as usize;

#[derive(Clone, Copy, Default)]
struct WuBox {
    r0: i32,
    r1: i32,
    g0: i32,
    g1: i32,
    b0: i32,
    b1: i32,
    vol: i32,
}

#[derive(Clone, Copy, PartialEq)]
enum Dir {
    R,
    G,
    B,
}

fn idx(r: i32, g: i32, b: i32) -> usize {
    ((r << (INDEX_BITS * 2)) + (r << (INDEX_BITS + 1)) + (g << INDEX_BITS) + r + g + b) as usize
}

struct Moments {
    w: Vec<i64>,
    r: Vec<i64>,
    g: Vec<i64>,
    b: Vec<i64>,
    m2: Vec<f64>,
}

fn top(c: &WuBox, d: Dir, p: i32, m: &[i64]) -> i64 {
    match d {
        Dir::R => m[idx(p, c.g1, c.b1)] - m[idx(p, c.g1, c.b0)] - m[idx(p, c.g0, c.b1)] + m[idx(p, c.g0, c.b0)],
        Dir::G => m[idx(c.r1, p, c.b1)] - m[idx(c.r1, p, c.b0)] - m[idx(c.r0, p, c.b1)] + m[idx(c.r0, p, c.b0)],
        Dir::B => m[idx(c.r1, c.g1, p)] - m[idx(c.r1, c.g0, p)] - m[idx(c.r0, c.g1, p)] + m[idx(c.r0, c.g0, p)],
    }
}

fn bottom(c: &WuBox, d: Dir, m: &[i64]) -> i64 {
    match d {
        Dir::R => -m[idx(c.r0, c.g1, c.b1)] + m[idx(c.r0, c.g1, c.b0)] + m[idx(c.r0, c.g0, c.b1)]
            - m[idx(c.r0, c.g0, c.b0)],
        Dir::G => -m[idx(c.r1, c.g0, c.b1)] + m[idx(c.r1, c.g0, c.b0)] + m[idx(c.r0, c.g0, c.b1)]
            - m[idx(c.r0, c.g0, c.b0)],
        Dir::B => -m[idx(c.r1, c.g1, c.b0)] + m[idx(c.r1, c.g0, c.b0)] + m[idx(c.r0, c.g1, c.b0)]
            - m[idx(c.r0, c.g0, c.b0)],
    }
}

fn vol(c: &WuBox, m: &[i64]) -> i64 {
    m[idx(c.r1, c.g1, c.b1)] - m[idx(c.r1, c.g1, c.b0)] - m[idx(c.r1, c.g0, c.b1)] + m[idx(c.r1, c.g0, c.b0)]
        - m[idx(c.r0, c.g1, c.b1)]
        + m[idx(c.r0, c.g1, c.b0)]
        + m[idx(c.r0, c.g0, c.b1)]
        - m[idx(c.r0, c.g0, c.b0)]
}

fn variance(c: &WuBox, mo: &Moments) -> f64 {
    let dr = vol(c, &mo.r) as f64;
    let dg = vol(c, &mo.g) as f64;
    let db = vol(c, &mo.b) as f64;
    let m = &mo.m2;
    let xx = m[idx(c.r1, c.g1, c.b1)] - m[idx(c.r1, c.g1, c.b0)] - m[idx(c.r1, c.g0, c.b1)] + m[idx(c.r1, c.g0, c.b0)]
        - m[idx(c.r0, c.g1, c.b1)]
        + m[idx(c.r0, c.g1, c.b0)]
        + m[idx(c.r0, c.g0, c.b1)]
        - m[idx(c.r0, c.g0, c.b0)];
    let hypotenuse = dr * dr + dg * dg + db * db;
    let volume = vol(c, &mo.w) as f64;
    xx - hypotenuse / volume
}

#[allow(clippy::too_many_arguments)]
fn maximize(c: &WuBox, d: Dir, first: i32, last: i32, cut: &mut i32, whole: [i64; 4], mo: &Moments) -> f64 {
    let [ww, wr, wg, wb] = whole;
    let br = bottom(c, d, &mo.r);
    let bg = bottom(c, d, &mo.g);
    let bb = bottom(c, d, &mo.b);
    let bw = bottom(c, d, &mo.w);
    let mut max = 0.0;
    *cut = -1;
    for i in first..last {
        let mut hr = br + top(c, d, i, &mo.r);
        let mut hg = bg + top(c, d, i, &mo.g);
        let mut hb = bb + top(c, d, i, &mo.b);
        let mut hw = bw + top(c, d, i, &mo.w);
        if hw == 0 {
            continue;
        }
        let mut temp = (hr as f64 * hr as f64 + hg as f64 * hg as f64 + hb as f64 * hb as f64) / hw as f64;
        hr = wr - hr;
        hg = wg - hg;
        hb = wb - hb;
        hw = ww - hw;
        if hw == 0 {
            continue;
        }
        temp += (hr as f64 * hr as f64 + hg as f64 * hg as f64 + hb as f64 * hb as f64) / hw as f64;
        if temp > max {
            max = temp;
            *cut = i;
        }
    }
    max
}

fn cut(cubes: &mut [WuBox], one: usize, two: usize, mo: &Moments) -> bool {
    let b1 = cubes[one];
    let whole = [vol(&b1, &mo.w), vol(&b1, &mo.r), vol(&b1, &mo.g), vol(&b1, &mo.b)];
    let (mut cut_r, mut cut_g, mut cut_b) = (0, 0, 0);
    let max_r = maximize(&b1, Dir::R, b1.r0 + 1, b1.r1, &mut cut_r, whole, mo);
    let max_g = maximize(&b1, Dir::G, b1.g0 + 1, b1.g1, &mut cut_g, whole, mo);
    let max_b = maximize(&b1, Dir::B, b1.b0 + 1, b1.b1, &mut cut_b, whole, mo);
    let dir = if max_r >= max_g && max_r >= max_b {
        if cut_r < 0 {
            return false;
        }
        Dir::R
    } else if max_g >= max_r && max_g >= max_b {
        Dir::G
    } else {
        Dir::B
    };
    let mut a = b1;
    let mut b = cubes[two];
    b.r1 = a.r1;
    b.g1 = a.g1;
    b.b1 = a.b1;
    match dir {
        Dir::R => {
            a.r1 = cut_r;
            b.r0 = cut_r;
            b.g0 = a.g0;
            b.b0 = a.b0;
        }
        Dir::G => {
            b.r0 = a.r0;
            a.g1 = cut_g;
            b.g0 = cut_g;
            b.b0 = a.b0;
        }
        Dir::B => {
            b.r0 = a.r0;
            b.g0 = a.g0;
            a.b1 = cut_b;
            b.b0 = cut_b;
        }
    }
    a.vol = (a.r1 - a.r0) * (a.g1 - a.g0) * (a.b1 - a.b0);
    b.vol = (b.r1 - b.r0) * (b.g1 - b.g0) * (b.b1 - b.b0);
    cubes[one] = a;
    cubes[two] = b;
    true
}

fn quantize_wu(pixels: &[u32], max_colors: i32) -> Vec<u32> {
    if max_colors <= 0 || max_colors > 256 || pixels.is_empty() {
        return Vec::new();
    }
    let mut mo = Moments {
        w: vec![0; TOTAL_SIZE],
        r: vec![0; TOTAL_SIZE],
        g: vec![0; TOTAL_SIZE],
        b: vec![0; TOTAL_SIZE],
        m2: vec![0.0; TOTAL_SIZE],
    };
    for &p in pixels {
        let r = ((p & 0x00ff0000) >> 16) as i32;
        let g = ((p & 0x0000ff00) >> 8) as i32;
        let b = (p & 0xff) as i32;
        let i = idx((r >> 3) + 1, (g >> 3) + 1, (b >> 3) + 1);
        mo.w[i] += 1;
        mo.r[i] += r as i64;
        mo.g[i] += g as i64;
        mo.b[i] += b as i64;
        mo.m2[i] += ((r * r) + (g * g) + (b * b)) as f64;
    }
    for r in 1..INDEX_COUNT {
        let n = INDEX_COUNT as usize;
        let mut area = vec![0i64; n];
        let mut area_r = vec![0i64; n];
        let mut area_g = vec![0i64; n];
        let mut area_b = vec![0i64; n];
        let mut area_2 = vec![0f64; n];
        for g in 1..INDEX_COUNT {
            let (mut line, mut line_r, mut line_g, mut line_b, mut line_2) = (0i64, 0i64, 0i64, 0i64, 0.0f64);
            for b in 1..INDEX_COUNT {
                let i = idx(r, g, b);
                let bu = b as usize;
                line += mo.w[i];
                line_r += mo.r[i];
                line_g += mo.g[i];
                line_b += mo.b[i];
                line_2 += mo.m2[i];
                area[bu] += line;
                area_r[bu] += line_r;
                area_g[bu] += line_g;
                area_b[bu] += line_b;
                area_2[bu] += line_2;
                let p = idx(r - 1, g, b);
                mo.w[i] = mo.w[p] + area[bu];
                mo.r[i] = mo.r[p] + area_r[bu];
                mo.g[i] = mo.g[p] + area_g[bu];
                mo.b[i] = mo.b[p] + area_b[bu];
                mo.m2[i] = mo.m2[p] + area_2[bu];
            }
        }
    }
    let mut cubes = vec![WuBox::default(); 256];
    cubes[0].r1 = INDEX_COUNT - 1;
    cubes[0].g1 = INDEX_COUNT - 1;
    cubes[0].b1 = INDEX_COUNT - 1;
    let mut volume_variance = vec![0.0f64; 256];
    let mut next = 0usize;
    let mut max_colors = max_colors;
    let mut i: i32 = 1;
    while i < max_colors {
        let iu = i as usize;
        if cut(&mut cubes, next, iu, &mo) {
            volume_variance[next] = if cubes[next].vol > 1 { variance(&cubes[next], &mo) } else { 0.0 };
            volume_variance[iu] = if cubes[iu].vol > 1 { variance(&cubes[iu], &mo) } else { 0.0 };
        } else {
            volume_variance[next] = 0.0;
            i -= 1;
        }
        next = 0;
        let mut temp = volume_variance[0];
        for j in 1..=(i.max(0) as usize) {
            if volume_variance[j] > temp {
                temp = volume_variance[j];
                next = j;
            }
        }
        if temp <= 0.0 {
            max_colors = i + 1;
            break;
        }
        i += 1;
    }
    let mut out = Vec::new();
    for c in cubes.iter().take(max_colors as usize) {
        let weight = vol(c, &mo.w);
        if weight > 0 {
            let r = (vol(c, &mo.r) / weight) as i32;
            let g = (vol(c, &mo.g) / weight) as i32;
            let b = (vol(c, &mo.b) / weight) as i32;
            out.push(c_argb(r, g, b));
        }
    }
    out
}

// ---------------------------------------------------------------- WSMeans

fn quantize_wsmeans(input: &[u32], starting: &[u32], max_colors: i32) -> BTreeMap<u32, u32> {
    if max_colors == 0 || input.is_empty() {
        return BTreeMap::new();
    }
    let max_colors = max_colors.min(256);
    let mut pixel_to_count: HashMap<u32, i32> = HashMap::new();
    let mut pixels: Vec<u32> = Vec::new();
    let mut points: Vec<Lab> = Vec::new();
    for &p in input {
        match pixel_to_count.get_mut(&p) {
            Some(c) => *c += 1,
            None => {
                pixels.push(p);
                points.push(lab_from_int(p));
                pixel_to_count.insert(p, 1);
            }
        }
    }
    let mut cluster_count = max_colors.min(points.len() as i32);
    if !starting.is_empty() {
        cluster_count = cluster_count.min(starting.len() as i32);
    }
    let k = cluster_count as usize;
    let mut sums = [0i32; 256];
    let mut clusters: Vec<Lab> = starting.iter().map(|&a| lab_from_int(a)).collect();
    let mut rng = GlibcRand::new(42688);
    let needed = cluster_count - clusters.len() as i32;
    if starting.is_empty() && needed > 0 {
        let rmax = 2147483647.0f64;
        for _ in 0..needed {
            let l = rng.rand() as f64 / rmax * (100.0) + 0.0;
            let a = rng.rand() as f64 / rmax * (100.0 - -100.0) - 100.0;
            let b = rng.rand() as f64 / rmax * (100.0 - -100.0) - 100.0;
            clusters.push(Lab { l, a, b });
        }
    }
    let mut rng = GlibcRand::new(42688);
    let mut cluster_indices: Vec<usize> = (0..points.len()).map(|_| (rng.rand() % cluster_count) as usize).collect();
    let mut dist = vec![vec![0.0f64; k]; k];
    for iteration in 0..100 {
        for i in 0..k {
            dist[i][i] = 0.0;
            for j in i + 1..k {
                let d = clusters[i].delta_e(&clusters[j]);
                dist[j][i] = d;
                dist[i][j] = d;
            }
        }
        let mut moved = false;
        for i in 0..points.len() {
            let point = points[i];
            let prev = cluster_indices[i];
            let prev_distance = point.delta_e(&clusters[prev]);
            let mut min_distance = prev_distance;
            let mut new_index: Option<usize> = None;
            for j in 0..k {
                if dist[prev][j] >= 4.0 * prev_distance {
                    continue;
                }
                let d = point.delta_e(&clusters[j]);
                if d < min_distance {
                    min_distance = d;
                    new_index = Some(j);
                }
            }
            if let Some(j) = new_index {
                // `abs` here is C's int abs: the difference is truncated toward zero first.
                let change = ((min_distance.sqrt() - prev_distance.sqrt()) as i32).wrapping_abs() as f64;
                if change > 3.0 {
                    moved = true;
                    cluster_indices[i] = j;
                }
            }
        }
        if !moved && iteration != 0 {
            break;
        }
        let mut sa = [0.0f64; 256];
        let mut sb = [0.0f64; 256];
        let mut sc = [0.0f64; 256];
        sums[..k].fill(0);
        for i in 0..points.len() {
            let ci = cluster_indices[i];
            let p = points[i];
            let count = pixel_to_count[&pixels[i]];
            sums[ci] += count;
            sa[ci] += p.l * count as f64;
            sb[ci] += p.a * count as f64;
            sc[ci] += p.b * count as f64;
        }
        for i in 0..k {
            let count = sums[i];
            if count == 0 {
                clusters[i] = Lab { l: 0.0, a: 0.0, b: 0.0 };
                continue;
            }
            clusters[i] = Lab { l: sa[i] / count as f64, a: sb[i] / count as f64, b: sc[i] / count as f64 };
        }
    }
    let mut swatches: Vec<(u32, i32)> = Vec::new();
    for i in 0..k {
        let argb = int_from_lab(clusters[i]);
        let count = sums[i];
        if count == 0 {
            continue;
        }
        match swatches.iter_mut().find(|s| s.0 == argb) {
            Some(s) => s.1 += count,
            None => swatches.push((argb, count)),
        }
    }
    swatches.into_iter().map(|(a, c)| (a, c as u32)).collect()
}

/// `QuantizeCelebi(list(image.getdata()), max_colors)`: pixels as `r << 16 | g << 8 | b`, no alpha.
pub fn quantize_celebi(rgb: &[[u8; 3]], max_colors: i32) -> BTreeMap<u32, u32> {
    let max_colors = max_colors.min(256);
    let pixels: Vec<u32> = rgb.iter().map(|p| ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32).collect();
    let wu = quantize_wu(&pixels, max_colors);
    quantize_wsmeans(&pixels, &wu, max_colors)
}

/// `Score.score(colors)[0]` with the default options (desired 4, fallback #4285F4, filter on).
pub fn score_first(colors: &BTreeMap<u32, u32>) -> u32 {
    let mut hcts: Vec<Hct> = Vec::new();
    let mut hue_population = [0i64; 360];
    let mut population_sum = 1e-05;
    for (&argb, &population) in colors {
        let h = Hct::from_int(argb);
        hcts.push(h);
        hue_population[h.hue as i64 as usize] += population as i64;
        population_sum += population as f64;
    }
    let mut excited = [0.0f64; 360];
    for hue in 0..360i64 {
        let proportion = hue_population[hue as usize] as f64 / population_sum;
        for i in hue - 14..hue + 16 {
            excited[sanitize_degrees_int(i) as usize] += proportion;
        }
    }
    let mut scored: Vec<(Hct, f64)> = Vec::new();
    for h in hcts {
        let hue = sanitize_degrees_int(h.hue.round_ties_even() as i64);
        let proportion = excited[hue as usize];
        if h.chroma < 5.0 || proportion <= 0.01 {
            continue;
        }
        let proportion_score = proportion * 100.0 * 0.7;
        let chroma_weight = if h.chroma < 48.0 { 0.1 } else { 0.3 };
        let chroma_score = (h.chroma - 48.0) * chroma_weight;
        scored.push((h, proportion_score + chroma_score));
    }
    // Python's sort(reverse=True) is stable: equal scores keep their order.
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut chosen: Vec<Hct> = Vec::new();
    for diff in (15..=90).rev() {
        chosen.clear();
        for (h, _) in &scored {
            if !chosen.iter().any(|c| difference_degrees(h.hue, c.hue) < diff as f64) {
                chosen.push(*h);
            }
            if chosen.len() >= 4 {
                break;
            }
        }
        if chosen.len() >= 4 {
            break;
        }
    }
    chosen.first().map(|h| h.argb).unwrap_or(4282549748)
}
