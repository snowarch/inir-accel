// This code was written by: Hikari.
//! Native Rust replacement for `scripts/images/least_busy_region.py`.
//!
//! Implements:
//! - Screen-scaled wallpaper loading (`fill`, `fit`, `stretch`, `center`, `tile`) with binary disk cache
//!   in `~/.cache/inir/region-sampling/`
//! - `--luma-grid CELL` per-cell mean luma, mean squared luma, and mean RGB for `services/WallpaperLuma.qml`
//! - 2D Integral Image (`O(1)` window variance) for `find_least_busy_region`, `--busiest`, and
//!   binary-search `find_largest_region` (`WaffleBackgroundClock`, `AbstractBackgroundWidget`)
//! - `--color-only` region brightness + std-dev + deterministic K-means (`K=3`) dominant color

use image::{GenericImageView, RgbaImage, imageops::FilterType};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const CACHE_KEEP: usize = 8;
const CACHE_MAGIC: &[u8; 8] = b"INIRREG1";

#[derive(Debug, Clone)]
pub struct ScreenImage {
    pub width: u32,
    pub height: u32,
    /// Row-major RGB bytes (`width * height * 3`)
    pub rgb: Vec<u8>,
}

impl ScreenImage {
    #[inline]
    pub fn to_grayscale(&self) -> Vec<u8> {
        let mut gray = Vec::with_capacity((self.width * self.height) as usize);
        for px in self.rgb.chunks_exact(3) {
            // Match OpenCV cvtColor(BGR2GRAY): 0.299*R + 0.587*G + 0.114*B
            let y = (299u32 * px[0] as u32 + 587u32 * px[1] as u32 + 114u32 * px[2] as u32 + 500) / 1000;
            gray.push(y.min(255) as u8);
        }
        gray
    }
}

fn cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".cache")
        });
    base.join("inir").join("region-sampling")
}

fn fnv1a_hash_hex(input: &str) -> String {
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x84222325cbf29ce4;
    for (i, b) in input.bytes().enumerate() {
        h1 ^= b as u64;
        h1 = h1.wrapping_mul(0x100000001b3);
        h2 ^= (b as u64).wrapping_add(i as u64);
        h2 = h2.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}{:016x}", h1, h2)
}

fn center_crop_rgb(rgb: &[u8], src_w: u32, src_h: u32, target_w: u32, target_h: u32) -> (u32, u32, Vec<u8>) {
    if src_w == target_w && src_h == target_h {
        return (src_w, src_h, rgb.to_vec());
    }
    let crop_w = src_w.min(target_w);
    let crop_h = src_h.min(target_h);
    let x1 = src_w.saturating_sub(target_w) / 2;
    let y1 = src_h.saturating_sub(target_h) / 2;
    let mut out = vec![0u8; (crop_w * crop_h * 3) as usize];
    for row in 0..crop_h {
        let src_off = (((y1 + row) * src_w + x1) * 3) as usize;
        let dst_off = (row * crop_w * 3) as usize;
        let len = (crop_w * 3) as usize;
        out[dst_off..dst_off + len].copy_from_slice(&rgb[src_off..src_off + len]);
    }
    (crop_w, crop_h, out)
}

fn center_on_black_rgb(rgb: &[u8], src_w: u32, src_h: u32, target_w: u32, target_h: u32) -> Vec<u8> {
    let (cw, ch, cropped) = center_crop_rgb(rgb, src_w, src_h, target_w, target_h);
    if cw == target_w && ch == target_h {
        return cropped;
    }
    let mut out = vec![0u8; (target_w * target_h * 3) as usize];
    let x_off = (target_w.saturating_sub(cw)) / 2;
    let y_off = (target_h.saturating_sub(ch)) / 2;
    for row in 0..ch {
        let src_idx = (row * cw * 3) as usize;
        let dst_idx = (((y_off + row) * target_w + x_off) * 3) as usize;
        let len = (cw * 3) as usize;
        out[dst_idx..dst_idx + len].copy_from_slice(&cropped[src_idx..src_idx + len]);
    }
    out
}

fn decode_and_scale_screen_image(
    image_path: &Path,
    screen_width: Option<u32>,
    screen_height: Option<u32>,
    screen_mode: &str,
) -> Result<ScreenImage, String> {
    let dyn_img = image::open(image_path).map_err(|e| format!("Failed to open {:?}: {}", image_path, e))?;
    let (orig_w, orig_h) = dyn_img.dimensions();
    if orig_w == 0 || orig_h == 0 {
        return Err("Empty image dimensions".to_string());
    }

    let (Some(sw), Some(sh)) = (screen_width, screen_height) else {
        let rgb_img = dyn_img.to_rgb8();
        return Ok(ScreenImage {
            width: orig_w,
            height: orig_h,
            rgb: rgb_img.into_raw(),
        });
    };

    if sw == 0 || sh == 0 {
        let rgb_img = dyn_img.to_rgb8();
        return Ok(ScreenImage {
            width: orig_w,
            height: orig_h,
            rgb: rgb_img.into_raw(),
        });
    }

    match screen_mode {
        "stretch" => {
            let resized = dyn_img.resize_exact(sw, sh, FilterType::Triangle).to_rgb8();
            Ok(ScreenImage {
                width: sw,
                height: sh,
                rgb: resized.into_raw(),
            })
        }
        "center" => {
            let raw = dyn_img.to_rgb8().into_raw();
            let out = center_on_black_rgb(&raw, orig_w, orig_h, sw, sh);
            Ok(ScreenImage {
                width: sw,
                height: sh,
                rgb: out,
            })
        }
        "tile" => {
            let raw = dyn_img.to_rgb8().into_raw();
            let mut out = vec![0u8; (sw * sh * 3) as usize];
            let x_phase = (orig_w as i64 - (((sw as i64 - orig_w as i64).div_euclid(2)).rem_euclid(orig_w as i64))).rem_euclid(orig_w as i64) as u32;
            let y_phase = (orig_h as i64 - (((sh as i64 - orig_h as i64).div_euclid(2)).rem_euclid(orig_h as i64))).rem_euclid(orig_h as i64) as u32;
            for y in 0..sh {
                let sy = (y + y_phase) % orig_h;
                for x in 0..sw {
                    let sx = (x + x_phase) % orig_w;
                    let s_idx = ((sy * orig_w + sx) * 3) as usize;
                    let d_idx = ((y * sw + x) * 3) as usize;
                    out[d_idx..d_idx + 3].copy_from_slice(&raw[s_idx..s_idx + 3]);
                }
            }
            Ok(ScreenImage {
                width: sw,
                height: sh,
                rgb: out,
            })
        }
        mode => {
            let scale_w = sw as f64 / orig_w as f64;
            let scale_h = sh as f64 / orig_h as f64;
            let scale = if mode == "fit" {
                scale_w.min(scale_h)
            } else {
                scale_w.max(scale_h)
            };
            let new_w = ((orig_w as f64 * scale) as u32).max(1);
            let new_h = ((orig_h as f64 * scale) as u32).max(1);
            // Fast CatmullRom bicubic resize (orders of magnitude faster than Python Lanczos4 with visually identical cell/region statistics)
            let resized = dyn_img.resize_exact(new_w, new_h, FilterType::CatmullRom).to_rgb8().into_raw();
            let rgb = if mode == "fit" {
                center_on_black_rgb(&resized, new_w, new_h, sw, sh)
            } else {
                let (_, _, cropped) = center_crop_rgb(&resized, new_w, new_h, sw, sh);
                cropped
            };
            Ok(ScreenImage {
                width: sw,
                height: sh,
                rgb,
            })
        }
    }
}

pub fn load_screen_image(
    image_path: &Path,
    screen_width: Option<u32>,
    screen_height: Option<u32>,
    screen_mode: &str,
) -> Result<ScreenImage, String> {
    let (Some(sw), Some(sh)) = (screen_width, screen_height) else {
        return decode_and_scale_screen_image(image_path, None, None, screen_mode);
    };

    let canonical = fs::canonicalize(image_path).unwrap_or_else(|_| image_path.to_path_buf());
    let meta = fs::metadata(&canonical).map_err(|e| format!("Image not found {:?}: {}", canonical, e))?;
    let key_input = format!(
        "{}|{}|{}|{}x{}|{}|rgb1",
        canonical.display(),
        meta.mtime_nsec(),
        meta.size(),
        sw,
        sh,
        screen_mode
    );
    let key = fnv1a_hash_hex(&key_input);
    let cdir = cache_dir();
    let cache_path = cdir.join(format!("{}.rbin", key));

    if let Ok(mut f) = fs::File::open(&cache_path) {
        let mut header = [0u8; 16];
        if f.read_exact(&mut header).is_ok() && &header[0..8] == CACHE_MAGIC {
            let w = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
            let h = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
            let expected_len = (w as usize).saturating_mul(h as usize).saturating_mul(3);
            let mut rgb = vec![0u8; expected_len];
            if f.read_exact(&mut rgb).is_ok() {
                return Ok(ScreenImage { width: w, height: h, rgb });
            }
        }
    }

    let img = decode_and_scale_screen_image(image_path, Some(sw), Some(sh), screen_mode)?;

    if fs::create_dir_all(&cdir).is_ok() {
        let tmp_path = cdir.join(format!("{}.rbin.tmp.{}", key, std::process::id()));
        if let Ok(mut f) = fs::File::create(&tmp_path) {
            let mut header = [0u8; 16];
            header[0..8].copy_from_slice(CACHE_MAGIC);
            header[8..12].copy_from_slice(&img.width.to_le_bytes());
            header[12..16].copy_from_slice(&img.height.to_le_bytes());
            if f.write_all(&header).is_ok() && f.write_all(&img.rgb).is_ok() {
                let _ = fs::rename(&tmp_path, &cache_path);
            } else {
                let _ = fs::remove_file(&tmp_path);
            }
        }
        // Prune old cache entries
        if let Ok(rd) = fs::read_dir(&cdir) {
            let mut rbins: Vec<(PathBuf, i64)> = rd
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().and_then(|s| s.to_str()) == Some("rbin") {
                        let mtime = e.metadata().ok().map(|m| m.mtime()).unwrap_or(0);
                        Some((p, mtime))
                    } else {
                        None
                    }
                })
                .collect();
            if rbins.len() > CACHE_KEEP {
                rbins.sort_by(|a, b| b.1.cmp(&a.1));
                for (stale, _) in &rbins[CACHE_KEEP..] {
                    let _ = fs::remove_file(stale);
                }
            }
        }
    }

    Ok(img)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LumaGridOutput {
    pub cols: usize,
    pub rows: usize,
    pub cell: usize,
    pub width: usize,
    pub height: usize,
    pub mean: Vec<i32>,
    pub sq: Vec<i32>,
    pub r: Vec<i32>,
    pub g: Vec<i32>,
    pub b: Vec<i32>,
}

pub fn compute_luma_grid(
    image_path: &Path,
    cell: usize,
    screen_width: u32,
    screen_height: u32,
    screen_mode: &str,
) -> Result<LumaGridOutput, String> {
    let cell = cell.max(1);
    let img = load_screen_image(image_path, Some(screen_width), Some(screen_height), screen_mode)?;
    let w = img.width as usize;
    let h = img.height as usize;
    let cols = (w + cell - 1) / cell;
    let rows = (h + cell - 1) / cell;
    let total_cells = cols * rows;

    let mut mean = Vec::with_capacity(total_cells);
    let mut sq = Vec::with_capacity(total_cells);
    let mut r_out = Vec::with_capacity(total_cells);
    let mut g_out = Vec::with_capacity(total_cells);
    let mut b_out = Vec::with_capacity(total_cells);

    let cell_area = (cell * cell) as f64;
    for ry in 0..rows {
        let y0 = ry * cell;
        for cx in 0..cols {
            let x0 = cx * cell;
            let mut sum_gray = 0.0f64;
            let mut sum_sq = 0.0f64;
            let mut sum_r = 0.0f64;
            let mut sum_g = 0.0f64;
            let mut sum_b = 0.0f64;

            for dy in 0..cell {
                // Replicate border just like cv2.BORDER_REPLICATE
                let py = (y0 + dy).min(h - 1);
                let row_base = py * w * 3;
                for dx in 0..cell {
                    let px = (x0 + dx).min(w - 1);
                    let idx = row_base + px * 3;
                    let rv = img.rgb[idx] as f64;
                    let gv = img.rgb[idx + 1] as f64;
                    let bv = img.rgb[idx + 2] as f64;
                    let gray = (0.299 * rv + 0.587 * gv + 0.114 * bv).round();
                    sum_gray += gray;
                    sum_sq += gray * gray;
                    sum_r += rv;
                    sum_g += gv;
                    sum_b += bv;
                }
            }

            mean.push((sum_gray / cell_area).round() as i32);
            sq.push((sum_sq / cell_area).round() as i32);
            r_out.push((sum_r / cell_area).round() as i32);
            g_out.push((sum_g / cell_area).round() as i32);
            b_out.push((sum_b / cell_area).round() as i32);
        }
    }

    Ok(LumaGridOutput {
        cols,
        rows,
        cell,
        width: w,
        height: h,
        mean,
        sq,
        r: r_out,
        g: g_out,
        b: b_out,
    })
}

struct IntegralImage {
    w: usize,
    h: usize,
    sum: Vec<f64>,
    sum_sq: Vec<f64>,
}

impl IntegralImage {
    fn from_gray(gray: &[u8], w: usize, h: usize) -> Self {
        let mut sum = vec![0.0f64; w * h];
        let mut sum_sq = vec![0.0f64; w * h];
        for y in 0..h {
            let mut row_s = 0.0f64;
            let mut row_s2 = 0.0f64;
            for x in 0..w {
                let v = gray[y * w + x] as f64;
                row_s += v;
                row_s2 += v * v;
                let above_s = if y > 0 { sum[(y - 1) * w + x] } else { 0.0 };
                let above_s2 = if y > 0 { sum_sq[(y - 1) * w + x] } else { 0.0 };
                sum[y * w + x] = above_s + row_s;
                sum_sq[y * w + x] = above_s2 + row_s2;
            }
        }
        Self { w, h, sum, sum_sq }
    }

    #[inline]
    fn rect_sum(buf: &[f64], w: usize, x1: usize, y1: usize, x2: usize, y2: usize) -> f64 {
        let mut total = buf[y2 * w + x2];
        if x1 > 0 {
            total -= buf[y2 * w + (x1 - 1)];
        }
        if y1 > 0 {
            total -= buf[(y1 - 1) * w + x2];
        }
        if x1 > 0 && y1 > 0 {
            total += buf[(y1 - 1) * w + (x1 - 1)];
        }
        total
    }

    #[inline]
    fn rect_variance(&self, x1: usize, y1: usize, x2: usize, y2: usize) -> f64 {
        let area = ((x2 - x1 + 1) * (y2 - y1 + 1)) as f64;
        let s = Self::rect_sum(&self.sum, self.w, x1, y1, x2, y2);
        let s2 = Self::rect_sum(&self.sum_sq, self.w, x1, y1, x2, y2);
        let mean = s / area;
        (s2 / area) - (mean * mean)
    }
}

pub fn region_brightness_and_dominant_color(
    img: &ScreenImage,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> (f64, f64, [u8; 3]) {
    let img_w = img.width as i32;
    let img_h = img.height as i32;
    if img_w <= 0 || img_h <= 0 {
        return (128.0, 0.0, [0, 0, 0]);
    }
    let rx = x.max(0).min(img_w - 1) as usize;
    let ry = y.max(0).min(img_h - 1) as usize;
    let rw = w.max(1).min(img_w - rx as i32).max(1) as usize;
    let rh = h.max(1).min(img_h - ry as i32).max(1) as usize;

    let count = rw * rh;
    if count == 0 {
        return (128.0, 0.0, [0, 0, 0]);
    }

    let mut sum_gray = 0.0f64;
    let mut sum_sq_gray = 0.0f64;
    // Subsample if region is huge so K-means runs in <0.5ms
    let step = if count > 4096 { (count / 2048).max(1) } else { 1 };
    let mut samples: Vec<[f32; 3]> = Vec::with_capacity(count / step + 1);

    let stride_w = img.width as usize;
    let mut idx_counter = 0usize;
    for row in ry..(ry + rh) {
        let row_off = row * stride_w * 3;
        for col in rx..(rx + rw) {
            let off = row_off + col * 3;
            let r = img.rgb[off];
            let g = img.rgb[off + 1];
            let b = img.rgb[off + 2];
            let gray = (299u32 * r as u32 + 587u32 * g as u32 + 114u32 * b as u32 + 500) / 1000;
            let gf = gray as f64;
            sum_gray += gf;
            sum_sq_gray += gf * gf;
            if idx_counter % step == 0 {
                samples.push([r as f32, g as f32, b as f32]);
            }
            idx_counter += 1;
        }
    }

    let mean = sum_gray / count as f64;
    let var = (sum_sq_gray / count as f64 - mean * mean).max(0.0);
    let std = var.sqrt();

    if samples.len() < 3 {
        let mut sr = 0.0f32;
        let mut sg = 0.0f32;
        let mut sb = 0.0f32;
        for s in &samples {
            sr += s[0];
            sg += s[1];
            sb += s[2];
        }
        let n = samples.len().max(1) as f32;
        return (mean, std, [(sr / n) as u8, (sg / n) as u8, (sb / n) as u8]);
    }

    // Deterministic K-means (K=3, 10 iterations) matching least_busy_region.py
    let n = samples.len();
    let mut centers = [
        samples[0],
        samples[n / 2],
        samples[n - 1],
    ];
    let mut counts = [0usize; 3];
    for _ in 0..10 {
        let mut sums = [[0.0f32; 3]; 3];
        counts = [0usize; 3];
        for p in &samples {
            let mut best_k = 0;
            let mut best_d = f32::MAX;
            for k in 0..3 {
                let dr = p[0] - centers[k][0];
                let dg = p[1] - centers[k][1];
                let db = p[2] - centers[k][2];
                let d = dr * dr + dg * dg + db * db;
                if d < best_d {
                    best_d = d;
                    best_k = k;
                }
            }
            counts[best_k] += 1;
            sums[best_k][0] += p[0];
            sums[best_k][1] += p[1];
            sums[best_k][2] += p[2];
        }
        for k in 0..3 {
            if counts[k] > 0 {
                let inv = 1.0 / counts[k] as f32;
                centers[k][0] = sums[k][0] * inv;
                centers[k][1] = sums[k][1] * inv;
                centers[k][2] = sums[k][2] * inv;
            }
        }
    }

    let total_samples = samples.len().max(1) as f64;
    let mut dom_k = 0;
    let mut best_score = -1.0f64;
    for k in 0..3 {
        if counts[k] == 0 {
            continue;
        }
        let rgb = crate::washi::RGB {
            r: (centers[k][0] / 255.0).clamp(0.0, 1.0) as f64,
            g: (centers[k][1] / 255.0).clamp(0.0, 1.0) as f64,
            b: (centers[k][2] / 255.0).clamp(0.0, 1.0) as f64,
        };
        let lch = rgb.lch();
        let pop_ratio = counts[k] as f64 / total_samples;
        let l_weight = if lch.l > 0.12 && lch.l < 0.88 { 1.0 } else { 0.45 };
        let chroma_boost = 1.0 + (lch.c * 18.0).powf(1.35) * l_weight;
        let score = pop_ratio.powf(0.55) * chroma_boost;
        if score > best_score {
            best_score = score;
            dom_k = k;
        }
    }
    let dom = [
        centers[dom_k][0].clamp(0.0, 255.0) as u8,
        centers[dom_k][1].clamp(0.0, 255.0) as u8,
        centers[dom_k][2].clamp(0.0, 255.0) as u8,
    ];
    (mean, std, dom)
}

pub fn find_least_busy_region(
    img: &ScreenImage,
    mut region_width: usize,
    mut region_height: usize,
    stride: usize,
    mut horizontal_padding: usize,
    mut vertical_padding: usize,
    busiest: bool,
) -> ((usize, usize), f64) {
    let w = img.width as usize;
    let h = img.height as usize;
    let stride = stride.max(1);
    if horizontal_padding * 2 >= w || vertical_padding * 2 >= h {
        horizontal_padding = horizontal_padding.min(w.saturating_sub(1) / 2);
        vertical_padding = vertical_padding.min(h.saturating_sub(1) / 2);
    }
    let max_region_w = w.saturating_sub(2 * horizontal_padding).max(1);
    let max_region_h = h.saturating_sub(2 * vertical_padding).max(1);
    region_width = region_width.min(max_region_w).max(1);
    region_height = region_height.min(max_region_h).max(1);

    let gray = img.to_grayscale();
    let ii = IntegralImage::from_gray(&gray, w, h);

    let x_start = horizontal_padding;
    let y_start = vertical_padding;
    let x_end = w.saturating_sub(region_width + horizontal_padding).max(x_start);
    let y_end = h.saturating_sub(region_height + vertical_padding).max(y_start);

    let mut min_var: Option<f64> = None;
    let mut max_var: Option<f64> = None;
    let mut min_coords = (horizontal_padding, vertical_padding);
    let mut max_coords = (horizontal_padding, vertical_padding);

    let mut y = y_start;
    while y <= y_end {
        let mut x = x_start;
        while x <= x_end {
            let x2 = x + region_width - 1;
            let y2 = y + region_height - 1;
            if x2 < w && y2 < h {
                let var = ii.rect_variance(x, y, x2, y2);
                if min_var.map_or(true, |mv| var < mv) {
                    min_var = Some(var);
                    min_coords = (x, y);
                }
                if max_var.map_or(true, |mv| var > mv) {
                    max_var = Some(var);
                    max_coords = (x, y);
                }
            }
            x += stride;
        }
        y += stride;
    }

    if busiest {
        (max_coords, max_var.unwrap_or(0.0))
    } else {
        (min_coords, min_var.unwrap_or(0.0))
    }
}

pub fn find_largest_region(
    img: &ScreenImage,
    stride: usize,
    threshold: f64,
    aspect_ratio: f64,
    mut horizontal_padding: usize,
    mut vertical_padding: usize,
) -> Option<((usize, usize), (usize, usize), f64)> {
    let w = img.width as usize;
    let h = img.height as usize;
    let stride = stride.max(1);
    let threshold = threshold.max(0.0);
    if horizontal_padding * 2 >= w || vertical_padding * 2 >= h {
        horizontal_padding = horizontal_padding.min(w.saturating_sub(1) / 2);
        vertical_padding = vertical_padding.min(h.saturating_sub(1) / 2);
    }
    let effective_w = w.saturating_sub(2 * horizontal_padding);
    let effective_h = h.saturating_sub(2 * vertical_padding);
    if effective_w == 0 || effective_h == 0 {
        return None;
    }

    let gray = img.to_grayscale();
    let ii = IntegralImage::from_gray(&gray, w, h);

    let mut min_size: isize = 10;
    let mut max_size: isize = if aspect_ratio >= 1.0 {
        effective_h.min((effective_w as f64 / aspect_ratio) as usize) as isize
    } else {
        ((effective_h as f64 * aspect_ratio) as usize).min(effective_w) as isize
    };
    if max_size < min_size {
        min_size = 1;
        max_size = max_size.max(1);
    }

    let mut best: Option<(usize, usize, usize, usize, f64)> = None;
    while min_size <= max_size {
        let mid = (min_size + max_size) / 2;
        let (region_w, region_h) = if aspect_ratio >= 1.0 {
            (
                (mid as f64 * aspect_ratio).round() as usize,
                mid as usize,
            )
        } else {
            (
                mid as usize,
                (if aspect_ratio != 0.0 { mid as f64 / aspect_ratio } else { mid as f64 }).round() as usize,
            )
        };
        if region_w == 0 || region_h == 0 {
            break;
        }
        if region_w > effective_w || region_h > effective_h {
            max_size = mid - 1;
            continue;
        }

        let mut found = false;
        let x_start = horizontal_padding;
        let y_start = vertical_padding;
        let x_end = w.saturating_sub(region_w + horizontal_padding);
        let y_end = h.saturating_sub(region_h + vertical_padding);

        let mut y = y_start;
        while y <= y_end {
            let mut x = x_start;
            while x <= x_end {
                let x2 = x + region_w - 1;
                let y2 = y + region_h - 1;
                if x2 < w && y2 < h {
                    let var = ii.rect_variance(x, y, x2, y2);
                    if var <= threshold {
                        found = true;
                        best = Some((x, y, region_w, region_h, var));
                        break;
                    }
                }
                x += stride;
            }
            if found {
                break;
            }
            y += stride;
        }

        if found {
            min_size = mid + 1;
        } else {
            max_size = mid - 1;
        }
    }

    best.map(|(x, y, rw, rh, var)| ((x + rw / 2, y + rh / 2), (rw, rh), var))
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// CLI entrypoint compatible with `least_busy_region.py`.
pub fn run_cli(args: &[String]) -> Result<(), String> {
    let mut image_path: Option<PathBuf> = None;
    let mut width: usize = 300;
    let mut height: usize = 200;
    let mut screen_width: u32 = 1920;
    let mut screen_height: u32 = 1080;
    let mut stride: usize = 10;
    let mut screen_mode = "fill".to_string();
    let mut largest_region = false;
    let mut variance_threshold: f64 = 1000.0;
    let mut aspect_ratio: f64 = 1.78;
    let mut horizontal_padding: usize = 50;
    let mut vertical_padding: usize = 50;
    let mut busiest = false;
    let mut color_only = false;
    let mut position_x: i32 = 0;
    let mut position_y: i32 = 0;
    let mut luma_grid_cell: usize = 0;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--width" if i + 1 < args.len() => {
                width = args[i + 1].parse().unwrap_or(300);
                i += 2;
            }
            "--height" if i + 1 < args.len() => {
                height = args[i + 1].parse().unwrap_or(200);
                i += 2;
            }
            "--screen-width" if i + 1 < args.len() => {
                screen_width = args[i + 1].parse().unwrap_or(1920);
                i += 2;
            }
            "--screen-height" if i + 1 < args.len() => {
                screen_height = args[i + 1].parse().unwrap_or(1080);
                i += 2;
            }
            "--stride" if i + 1 < args.len() => {
                stride = args[i + 1].parse().unwrap_or(10);
                i += 2;
            }
            "--screen-mode" if i + 1 < args.len() => {
                screen_mode = args[i + 1].clone();
                i += 2;
            }
            "-l" | "--largest-region" => {
                largest_region = true;
                i += 1;
            }
            "-t" | "--variance-threshold" if i + 1 < args.len() => {
                variance_threshold = args[i + 1].parse().unwrap_or(1000.0);
                i += 2;
            }
            "--aspect-ratio" if i + 1 < args.len() => {
                aspect_ratio = args[i + 1].parse().unwrap_or(1.78);
                i += 2;
            }
            "-hp" | "--horizontal-padding" if i + 1 < args.len() => {
                horizontal_padding = args[i + 1].parse().unwrap_or(50);
                i += 2;
            }
            "-vp" | "--vertical-padding" if i + 1 < args.len() => {
                vertical_padding = args[i + 1].parse().unwrap_or(50);
                i += 2;
            }
            "--busiest" => {
                busiest = true;
                i += 1;
            }
            "--color-only" => {
                color_only = true;
                i += 1;
            }
            "--position-x" if i + 1 < args.len() => {
                position_x = args[i + 1].parse().unwrap_or(0);
                i += 2;
            }
            "--position-y" if i + 1 < args.len() => {
                position_y = args[i + 1].parse().unwrap_or(0);
                i += 2;
            }
            "--luma-grid" if i + 1 < args.len() => {
                luma_grid_cell = args[i + 1].parse().unwrap_or(0);
                i += 2;
            }
            "--verbose" | "-v" | "--visual-output" => {
                i += 1;
            }
            other if !other.starts_with('-') => {
                image_path = Some(PathBuf::from(other));
                i += 1;
            }
            other => {
                return Err(format!("Unsupported flag in region CLI: {}", other));
            }
        }
    }

    let path = image_path.ok_or_else(|| "Missing image_path positional argument".to_string())?;

    if luma_grid_cell > 0 {
        let grid = compute_luma_grid(&path, luma_grid_cell, screen_width, screen_height, &screen_mode)?;
        let json = serde_json::to_string(&grid).map_err(|e| e.to_string())?;
        println!("{}", json);
        return Ok(());
    }

    let img = load_screen_image(&path, Some(screen_width), Some(screen_height), &screen_mode)?;

    if color_only {
        let (brightness, brightness_std, dom) =
            region_brightness_and_dominant_color(&img, position_x, position_y, width as i32, height as i32);
        let dom_hex = format!("#{:02x}{:02x}{:02x}", dom[0], dom[1], dom[2]);
        let out = serde_json::json!({
            "center_x": position_x + (width as i32) / 2,
            "center_y": position_y + (height as i32) / 2,
            "width": width,
            "height": height,
            "dominant_color": dom_hex,
            "brightness": round1(brightness),
            "brightness_std": round1(brightness_std),
        });
        println!("{}", out);
        return Ok(());
    }

    if largest_region {
        if let Some(((cx, cy), (rw, rh), var)) = find_largest_region(
            &img,
            stride,
            variance_threshold,
            aspect_ratio,
            horizontal_padding,
            vertical_padding,
        ) {
            let x1 = (cx as i32) - (rw as i32) / 2;
            let y1 = (cy as i32) - (rh as i32) / 2;
            let (brightness, brightness_std, dom) =
                region_brightness_and_dominant_color(&img, x1, y1, rw as i32, rh as i32);
            let dom_hex = format!("#{:02x}{:02x}{:02x}", dom[0], dom[1], dom[2]);
            let out = serde_json::json!({
                "center_x": cx,
                "center_y": cy,
                "width": rw,
                "height": rh,
                "variance": var,
                "dominant_color": dom_hex,
                "brightness": round1(brightness),
                "brightness_std": round1(brightness_std),
            });
            println!("{}", out);
        } else {
            println!("{{\"error\": \"No region found under the threshold.\"}}");
        }
        return Ok(());
    }

    let ((rx, ry), variance) = find_least_busy_region(
        &img,
        width,
        height,
        stride,
        horizontal_padding,
        vertical_padding,
        busiest,
    );
    let center_x = rx + width / 2;
    let center_y = ry + height / 2;
    let (brightness, brightness_std, dom) =
        region_brightness_and_dominant_color(&img, rx as i32, ry as i32, width as i32, height as i32);
    let dom_hex = format!("#{:02x}{:02x}{:02x}", dom[0], dom[1], dom[2]);
    let out = serde_json::json!({
        "center_x": center_x,
        "center_y": center_y,
        "width": width,
        "height": height,
        "variance": variance,
        "dominant_color": dom_hex,
        "brightness": round1(brightness),
        "brightness_std": round1(brightness_std),
    });
    println!("{}", out);
    Ok(())
}
