//! `cv2.resize` on 8-bit images as OpenCV 5.0.0's C path computes it (imgproc/src/resize.cpp; IPP and the SIMD
//! paths give the same bytes, measured): INTER_LANCZOS4 and INTER_LINEAR in fixed point after float coefficients,
//! INTER_AREA as the integer fast path (with its 2x2 shortcut, which rounds half up) or the float decimation tables.

use super::Mat;

#[derive(Clone, Copy, PartialEq)]
pub enum Interp {
    Area,
    Lanczos4,
}

const COEF_BITS: i32 = 11;
const COEF_SCALE: i32 = 1 << COEF_BITS;

/// cvRound on a float: nearest, ties to even (lrintf).
fn round_f32(v: f32) -> i32 {
    v.round_ties_even() as i32
}

fn round_f64(v: f64) -> i64 {
    v.round_ties_even() as i64
}

fn sat_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn sat_i16(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

pub fn resize(src: &Mat, dw: usize, dh: usize, interp: Interp) -> Mat {
    if dw == src.w && dh == src.h {
        return Mat { w: src.w, h: src.h, cn: src.cn, data: src.data.clone() };
    }
    let inv_scale_x = dw as f64 / src.w as f64;
    let inv_scale_y = dh as f64 / src.h as f64;
    let dw = round_f64(src.w as f64 * inv_scale_x) as usize;
    let dh = round_f64(src.h as f64 * inv_scale_y) as usize;
    let scale_x = 1.0 / inv_scale_x;
    let scale_y = 1.0 / inv_scale_y;
    let iscale_x = round_f64(scale_x);
    let iscale_y = round_f64(scale_y);
    let is_area_fast = (scale_x - iscale_x as f64).abs() < f64::EPSILON && (scale_y - iscale_y as f64).abs() < f64::EPSILON;
    if interp == Interp::Area && scale_x >= 1.0 && scale_y >= 1.0 {
        return if is_area_fast { area_fast(src, dw, dh, iscale_x as usize, iscale_y as usize) } else { area(src, dw, dh, scale_x, scale_y) };
    }
    generic(src, dw, dh, interp, inv_scale_x, inv_scale_y, scale_x, scale_y)
}

fn interpolate_lanczos4(x: f32) -> [f32; 8] {
    const S45: f64 = 0.70710678118654752440084436210485;
    const CS: [[f64; 2]; 8] = [[1.0, 0.0], [-S45, -S45], [0.0, 1.0], [S45, -S45], [-1.0, 0.0], [S45, S45], [0.0, -1.0], [-S45, S45]];
    let mut coeffs = [0f32; 8];
    let mut sum = 0f32;
    let y0 = -((x + 3.0) as f64) * std::f64::consts::PI * 0.25;
    let (s0, c0) = (y0.sin(), y0.cos());
    for i in 0..8 {
        let y0_ = x + 3.0 - i as f32;
        if y0_.abs() >= 1e-6 {
            let y = -(y0_ as f64) * std::f64::consts::PI * 0.25;
            coeffs[i] = ((CS[i][0] * s0 + CS[i][1] * c0) / (y * y)) as f32;
        } else {
            coeffs[i] = 1e30;
        }
        sum += coeffs[i];
    }
    let sum = 1.0 / sum;
    for c in &mut coeffs {
        *c *= sum;
    }
    coeffs
}

#[allow(clippy::too_many_arguments)]
fn generic(src: &Mat, dw: usize, dh: usize, interp: Interp, inv_scale_x: f64, inv_scale_y: f64, scale_x: f64, scale_y: f64) -> Mat {
    let cn = src.cn;
    let (sw, sh) = (src.w as i64, src.h as i64);
    let area_mode = interp == Interp::Area;
    let ksize: usize = if interp == Interp::Lanczos4 { 8 } else { 2 };
    let ksize2 = (ksize / 2) as i64;
    let width = dw * cn;
    let mut xofs = vec![0i64; width];
    let mut ialpha = vec![0i16; width * ksize];
    let (mut xmin, mut xmax) = (0usize, dw);
    let coeffs = |f: f32| -> [f32; 8] {
        if interp == Interp::Lanczos4 {
            interpolate_lanczos4(f)
        } else {
            [1.0 - f, f, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        }
    };
    for dx in 0..dw {
        let (mut sx, mut fx);
        if !area_mode {
            fx = ((dx as f64 + 0.5) * scale_x - 0.5) as f32;
            sx = fx.floor() as i64;
            fx -= sx as f32;
        } else {
            sx = (dx as f64 * scale_x).floor() as i64;
            fx = ((dx + 1) as f64 - (sx + 1) as f64 * inv_scale_x) as f32;
            fx = if fx <= 0.0 { 0.0 } else { fx - fx.floor() };
        }
        if sx < ksize2 - 1 {
            xmin = dx + 1;
            if sx < 0 && interp != Interp::Lanczos4 {
                fx = 0.0;
                sx = 0;
            }
        }
        if sx + ksize2 >= sw {
            xmax = xmax.min(dx);
            if sx >= sw - 1 && interp != Interp::Lanczos4 {
                fx = 0.0;
                sx = sw - 1;
            }
        }
        for k in 0..cn {
            xofs[dx * cn + k] = sx * cn as i64 + k as i64;
        }
        let cbuf = coeffs(fx);
        for k in 0..ksize {
            ialpha[dx * cn * ksize + k] = sat_i16(round_f32(cbuf[k] * COEF_SCALE as f32));
        }
        for k in ksize..cn * ksize {
            ialpha[dx * cn * ksize + k] = ialpha[dx * cn * ksize + k - ksize];
        }
    }
    let mut yofs = vec![0i64; dh];
    let mut ibeta = vec![0i16; dh * ksize];
    for dy in 0..dh {
        let (sy, mut fy);
        if !area_mode {
            fy = ((dy as f64 + 0.5) * scale_y - 0.5) as f32;
            sy = fy.floor() as i64;
            fy -= sy as f32;
        } else {
            sy = (dy as f64 * scale_y).floor() as i64;
            fy = ((dy + 1) as f64 - (sy + 1) as f64 * inv_scale_y) as f32;
            fy = if fy <= 0.0 { 0.0 } else { fy - fy.floor() };
        }
        yofs[dy] = sy;
        let cbuf = coeffs(fy);
        for k in 0..ksize {
            ibeta[dy * ksize + k] = sat_i16(round_f32(cbuf[k] * COEF_SCALE as f32));
        }
    }

    // resizeGeneric_: widths in samples from here on.
    let swidth = sw * cn as i64;
    let (xmin, xmax) = (xmin * cn, xmax * cn);
    let hresize = |row: &[u8]| -> Vec<i32> {
        let mut d = vec![0i32; width];
        for dx in 0..width {
            let a = &ialpha[dx * ksize..dx * ksize + ksize];
            if interp == Interp::Lanczos4 {
                let sx = xofs[dx] - cn as i64 * 3;
                let mut v = 0i32;
                for j in 0..8 {
                    let mut sxj = sx + j as i64 * cn as i64;
                    if dx < xmin || dx >= xmax {
                        while sxj < 0 {
                            sxj += cn as i64;
                        }
                        while sxj >= swidth {
                            sxj -= cn as i64;
                        }
                    }
                    v += row[sxj as usize] as i32 * a[j] as i32;
                }
                d[dx] = v;
            } else {
                let sx = xofs[dx] as usize;
                d[dx] = if dx < xmax { row[sx] as i32 * a[0] as i32 + row[sx + cn] as i32 * a[1] as i32 } else { row[sx] as i32 * COEF_SCALE };
            }
        }
        d
    };
    let row_bytes = src.w * cn;
    let mut cache: Vec<(i64, Vec<i32>)> = Vec::new();
    let mut out = vec![0u8; dw * dh * cn];
    for dy in 0..dh {
        let sy0 = yofs[dy];
        let mut rows: Vec<usize> = Vec::with_capacity(ksize);
        for k in 0..ksize as i64 {
            let y = sy0 - ksize2 + 1 + k;
            let sy = if y >= 0 { if y < sh { y } else { sh - 1 } } else { 0 };
            let at = match cache.iter().position(|(s, _)| *s == sy) {
                Some(i) => i,
                None => {
                    let start = sy as usize * row_bytes;
                    cache.push((sy, hresize(&src.data[start..start + row_bytes])));
                    cache.len() - 1
                }
            };
            rows.push(at);
        }
        let beta = &ibeta[dy * ksize..dy * ksize + ksize];
        let dst = &mut out[dy * width..(dy + 1) * width];
        for x in 0..width {
            dst[x] = if interp == Interp::Lanczos4 {
                let mut v = 0i32;
                for k in 0..8 {
                    v += cache[rows[k]].1[x] * beta[k] as i32;
                }
                sat_u8((v + (1 << 21)) >> 22)
            } else {
                let (s0, s1) = (cache[rows[0]].1[x], cache[rows[1]].1[x]);
                ((((beta[0] as i32 * (s0 >> 4)) >> 16) + ((beta[1] as i32 * (s1 >> 4)) >> 16) + 2) >> 2) as u8
            };
        }
        let lowest = rows.iter().map(|&i| cache[i].0).min().unwrap_or(0);
        cache.retain(|(s, _)| *s >= lowest);
    }
    Mat { w: dw, h: dh, cn, data: out }
}

fn area_fast(src: &Mat, dw: usize, dh: usize, sx: usize, sy: usize) -> Mat {
    let cn = src.cn;
    let swidth = src.w * cn;
    let dwidth = dw * cn;
    let dwidth1 = (src.w / sx) * cn;
    let area = sx * sy;
    let scale = 1.0f32 / area as f32;
    let fast = sx == 2 && sy == 2 && (cn == 1 || cn == 3 || cn == 4);
    let mut out = vec![0u8; dwidth * dh];
    for dy in 0..dh {
        let sy0 = dy * sy;
        let d = &mut out[dy * dwidth..(dy + 1) * dwidth];
        if sy0 >= src.h {
            continue;
        }
        let w = if sy0 + sy <= src.h { dwidth1 } else { 0 };
        let row = &src.data[sy0 * swidth..];
        let mut dx = 0;
        if fast {
            let next = &src.data[(sy0 + 1).min(src.h - 1) * swidth..];
            while dx < w {
                let c = dx % cn;
                let i = (dx - c) * 2 + c;
                d[dx] = ((row[i] as u32 + row[i + cn] as u32 + next[i] as u32 + next[i + cn] as u32 + 2) >> 2) as u8;
                dx += 1;
            }
        }
        while dx < w {
            let c = dx % cn;
            let base = sx * (dx - c) + c;
            let mut sum = 0i32;
            for ky in 0..sy {
                for kx in 0..sx {
                    sum += row[ky * swidth + base + kx * cn] as i32;
                }
            }
            d[dx] = sat_u8(round_f32(sum as f32 * scale));
            dx += 1;
        }
        while dx < dwidth {
            let c = dx % cn;
            let sx0 = sx * (dx - c) + c;
            let (mut sum, mut count) = (0i32, 0i32);
            for ky in 0..sy {
                if sy0 + ky >= src.h {
                    break;
                }
                let r = &src.data[(sy0 + ky) * swidth..];
                let mut k = 0;
                while k < sx * cn {
                    if sx0 + k >= swidth {
                        break;
                    }
                    sum += r[sx0 + k] as i32;
                    count += 1;
                    k += cn;
                }
            }
            d[dx] = if sx0 >= swidth && count == 0 { 0 } else { sat_u8(round_f32(sum as f32 / count as f32)) };
            dx += 1;
        }
    }
    Mat { w: dw, h: dh, cn, data: out }
}

struct Tab {
    si: usize,
    di: usize,
    alpha: f32,
}

fn area_tab(ssize: usize, dsize: usize, cn: usize, scale: f64) -> Vec<Tab> {
    let mut tab = Vec::new();
    for dx in 0..dsize {
        let fsx1 = dx as f64 * scale;
        let fsx2 = fsx1 + scale;
        let cell_width = scale.min(ssize as f64 - fsx1);
        let mut sx1 = fsx1.ceil() as i64;
        let mut sx2 = fsx2.floor() as i64;
        sx2 = sx2.min(ssize as i64 - 1);
        sx1 = sx1.min(sx2);
        if sx1 as f64 - fsx1 > 1e-3 {
            tab.push(Tab { di: dx * cn, si: (sx1 - 1) as usize * cn, alpha: ((sx1 as f64 - fsx1) / cell_width) as f32 });
        }
        for s in sx1..sx2 {
            tab.push(Tab { di: dx * cn, si: s as usize * cn, alpha: (1.0 / cell_width) as f32 });
        }
        if fsx2 - sx2 as f64 > 1e-3 {
            tab.push(Tab { di: dx * cn, si: sx2 as usize * cn, alpha: ((fsx2 - sx2 as f64).min(1.0).min(cell_width) / cell_width) as f32 });
        }
    }
    tab
}

fn area(src: &Mat, dw: usize, dh: usize, scale_x: f64, scale_y: f64) -> Mat {
    let cn = src.cn;
    let xtab = area_tab(src.w, dw, cn, scale_x);
    let ytab = area_tab(src.h, dh, 1, scale_y);
    let width = dw * cn;
    let swidth = src.w * cn;
    let mut out = vec![0u8; width * dh];
    let mut buf = vec![0f32; width];
    let mut sum = vec![0f32; width];
    let mut prev_dy = ytab.first().map_or(0, |t| t.di);
    let store = |sum: &[f32], out: &mut [u8], dy: usize| {
        for (o, &s) in out[dy * width..(dy + 1) * width].iter_mut().zip(sum) {
            *o = sat_u8(round_f32(s));
        }
    };
    for t in &ytab {
        let (beta, dy, sy) = (t.alpha, t.di, t.si);
        let s = &src.data[sy * swidth..(sy + 1) * swidth];
        buf.iter_mut().for_each(|b| *b = 0.0);
        for x in &xtab {
            for c in 0..cn {
                buf[x.di + c] += s[x.si + c] as f32 * x.alpha;
            }
        }
        if dy != prev_dy {
            store(&sum, &mut out, prev_dy);
            for (sm, &b) in sum.iter_mut().zip(&buf) {
                *sm = beta * b;
            }
            prev_dy = dy;
        } else {
            for (sm, &b) in sum.iter_mut().zip(&buf) {
                *sm += beta * b;
            }
        }
    }
    store(&sum, &mut out, prev_dy);
    Mat { w: dw, h: dh, cn, data: out }
}
