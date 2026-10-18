//! The pixels generate_colors_material.py hands the quantizer, as Pillow 12.3.0 and numpy 2.5.4 produce them:
//! `Image.open` (format by content), `convert("RGB")` for L and P, `resize(BICUBIC)` (Resample.c, premultiplied for
//! RGBA as Image.resize does) and `_auto_detect_scheme`'s statistics with numpy's pairwise sums.
//!
//! What Pillow reads differently from the decoders here is not guessed: `Load::Delegate` hands the call to the
//! original (GIF, CMYK JPEG, animated WebP, 1-bit or 16-bit grey, grey with alpha, anything else).

use std::ffi::{c_int, c_ulong, c_void};
use std::sync::OnceLock;

/// A Pillow image in mode RGB (3 bytes a pixel) or RGBA (4).
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub data: Vec<u8>,
    /// A palette whose tRNS is not a single clear entry: Pillow keeps it as bytes and `convert("RGB")` warns.
    pub warns_palette_transparency: bool,
}

/// Past `Image.MAX_IMAGE_PIXELS` Pillow warns (DecompressionBombWarning) or refuses: the original says it.
const MAX_IMAGE_PIXELS: usize = 1024 * 1024 * 1024 / 4 / 3;

pub enum Load {
    Ok(Image),
    /// `open()` itself failed (missing file, a directory, no permission): Python's traceback, exit 1.
    Io(std::io::Error),
    Delegate,
}

pub fn open(path: &str) -> Load {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Load::Io(e),
    };
    let image = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(&bytes)
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        jpeg(&bytes)
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        webp(&bytes)
    } else {
        None
    };
    match image {
        Some(im) => Load::Ok(im),
        None => Load::Delegate,
    }
}

/// Bit depth, colour type and tRNS presence, read from the chunks (the `image` crate expands them before we see it).
fn png_header(bytes: &[u8]) -> Option<(u8, u8, Option<Vec<u8>>)> {
    let mut pos = 8;
    let mut header = None;
    let mut trns = None;
    while pos + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
        let kind = &bytes[pos + 4..pos + 8];
        let body = bytes.get(pos + 8..pos + 8 + len)?;
        match kind {
            b"IHDR" if body.len() >= 10 => header = Some((body[8], body[9])),
            b"tRNS" => trns = Some(body.to_vec()),
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        pos += 12 + len;
    }
    header.map(|(depth, colour)| (depth, colour, trns))
}

fn png(bytes: &[u8]) -> Option<Image> {
    let (depth, colour, trns) = png_header(bytes)?;
    // Pillow's modes (PngImagePlugin._MODES): grey 1 bit is "1", grey 16 is "I;16", grey+alpha is "LA". None of
    // them reach the quantizer as RGB tuples; the original is the reference for what happens to them.
    let mode_l = match (colour, depth) {
        (0, 2 | 4 | 8) => true,
        (2, 8 | 16) | (3, 1 | 2 | 4 | 8) | (6, 8 | 16) => false,
        _ => return None,
    };
    let dynimg = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
    let (w, h) = (dynimg.width() as usize, dynimg.height() as usize);
    if w * h > MAX_IMAGE_PIXELS {
        return None;
    }
    let raw: Vec<u8> = match &dynimg {
        image::DynamicImage::ImageLuma8(b) => b.as_raw().clone(),
        image::DynamicImage::ImageLumaA8(b) => b.as_raw().chunks_exact(2).map(|p| p[0]).collect(),
        image::DynamicImage::ImageRgb8(b) => b.as_raw().clone(),
        image::DynamicImage::ImageRgba8(b) => b.as_raw().clone(),
        // 16-bit samples: Pillow's RGB;16B and RGBA;16B unpackers keep the high byte.
        image::DynamicImage::ImageRgb16(b) => b.as_raw().iter().map(|v| (v >> 8) as u8).collect(),
        image::DynamicImage::ImageRgba16(b) => b.as_raw().iter().map(|v| (v >> 8) as u8).collect(),
        _ => return None,
    };
    let in_channels = raw.len() / (w * h).max(1);
    if mode_l {
        // L (and grey with tRNS, which Pillow keeps as L) through convert("RGB"): l2rgb.
        if in_channels != 1 {
            return None;
        }
        let data = raw.iter().flat_map(|&v| [v, v, v]).collect();
        return Some(Image { width: w, height: h, channels: 3, data, warns_palette_transparency: false });
    }
    // RGBA stays RGBA. RGB with tRNS stays RGB in Pillow, and a palette goes through p2rgb, which ignores the
    // transparency: the alpha the `image` crate made up is dropped.
    let rgba = colour == 6;
    match (rgba, in_channels) {
        (true, 4) => Some(Image { width: w, height: h, channels: 4, data: raw, warns_palette_transparency: false }),
        (false, 3) => Some(Image { width: w, height: h, channels: 3, data: raw, warns_palette_transparency: false }),
        (false, 4) if trns.is_some() || colour == 3 => {
            let data = raw.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
            // PngImagePlugin keeps a palette's tRNS as an index when it matches ^\xff*\x00\xff*$, as bytes otherwise.
            let simple = trns.as_ref().is_some_and(|t| t.iter().filter(|&&b| b == 0).count() == 1 && t.iter().all(|&b| b == 0 || b == 0xFF));
            let warns = colour == 3 && trns.is_some() && !simple;
            Some(Image { width: w, height: h, channels: 3, data, warns_palette_transparency: warns })
        }
        _ => None,
    }
}

fn webp(bytes: &[u8]) -> Option<Image> {
    // VP8X with the animation flag: Pillow composites frame 0 its own way.
    if bytes.len() >= 21 && &bytes[12..16] == b"VP8X" && bytes[20] & 0x02 != 0 {
        return None;
    }
    let dynimg = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP).ok()?;
    let (w, h) = (dynimg.width() as usize, dynimg.height() as usize);
    if w * h > MAX_IMAGE_PIXELS {
        return None;
    }
    match dynimg {
        image::DynamicImage::ImageRgb8(b) => Some(Image { width: w, height: h, channels: 3, data: b.into_raw(), warns_palette_transparency: false }),
        image::DynamicImage::ImageRgba8(b) => Some(Image { width: w, height: h, channels: 4, data: b.into_raw(), warns_palette_transparency: false }),
        _ => None,
    }
}

// ---------------------------------------------------------------- JPEG through the system's libjpeg-turbo

type TjInit = unsafe extern "C" fn() -> *mut c_void;
type TjHeader = unsafe extern "C" fn(*mut c_void, *const u8, c_ulong, *mut c_int, *mut c_int, *mut c_int, *mut c_int) -> c_int;
type TjDecompress = unsafe extern "C" fn(*mut c_void, *const u8, c_ulong, *mut u8, c_int, c_int, c_int, c_int, c_int) -> c_int;
type TjDestroy = unsafe extern "C" fn(*mut c_void) -> c_int;

struct TurboJpeg {
    init: TjInit,
    header: TjHeader,
    decompress: TjDecompress,
    destroy: TjDestroy,
}

/// Pillow decodes JPEG with libjpeg-turbo (islow DCT, fancy upsampling): the same library, with no flags, gives the
/// same pixels (166 of 166 files measured); a pure-Rust decoder does not.
fn turbojpeg() -> Option<&'static TurboJpeg> {
    static LIB: OnceLock<Option<TurboJpeg>> = OnceLock::new();
    LIB.get_or_init(|| unsafe {
        let handle = libc::dlopen(c"libturbojpeg.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            return None;
        }
        let sym = |name: &std::ffi::CStr| {
            let p = libc::dlsym(handle, name.as_ptr());
            (!p.is_null()).then_some(p)
        };
        Some(TurboJpeg {
            init: std::mem::transmute::<*mut c_void, TjInit>(sym(c"tjInitDecompress")?),
            header: std::mem::transmute::<*mut c_void, TjHeader>(sym(c"tjDecompressHeader3")?),
            decompress: std::mem::transmute::<*mut c_void, TjDecompress>(sym(c"tjDecompress2")?),
            destroy: std::mem::transmute::<*mut c_void, TjDestroy>(sym(c"tjDestroy")?),
        })
    })
    .as_ref()
}

/// libjpeg-turbo's pixels in one `TJPF_*` format (tjDecompress2, no flags): width, height and the rows, or nothing
/// for CMYK and YCCK, a failed decode or a missing library. A warning (corrupt data) counts as failed.
pub(crate) fn turbo_decode(bytes: &[u8], pixel_format: c_int, channels: usize) -> Option<(usize, usize, Vec<u8>)> {
    const TJCS_CMYK: c_int = 3;
    const TJCS_YCCK: c_int = 4;
    let tj = turbojpeg()?;
    unsafe {
        let handle = (tj.init)();
        if handle.is_null() {
            return None;
        }
        let (mut w, mut h, mut subsamp, mut colourspace) = (0, 0, 0, 0);
        let size = bytes.len() as c_ulong;
        let mut out = None;
        if (tj.header)(handle, bytes.as_ptr(), size, &mut w, &mut h, &mut subsamp, &mut colourspace) == 0
            && w > 0
            && h > 0
            && colourspace != TJCS_CMYK
            && colourspace != TJCS_YCCK
        {
            let mut buf = vec![0u8; w as usize * h as usize * channels];
            if (tj.decompress)(handle, bytes.as_ptr(), size, buf.as_mut_ptr(), w, 0, h, pixel_format, 0) == 0 {
                out = Some((w as usize, h as usize, buf));
            }
        }
        (tj.destroy)(handle);
        out
    }
}

fn jpeg(bytes: &[u8]) -> Option<Image> {
    const TJPF_RGB: c_int = 0;
    const TJPF_GRAY: c_int = 6;
    const TJCS_GRAY: c_int = 2;
    const TJCS_CMYK: c_int = 3;
    const TJCS_YCCK: c_int = 4;
    let tj = turbojpeg()?;
    unsafe {
        let handle = (tj.init)();
        if handle.is_null() {
            return None;
        }
        let (mut w, mut h, mut subsamp, mut colourspace) = (0, 0, 0, 0);
        let size = bytes.len() as c_ulong;
        let mut out = None;
        if (tj.header)(handle, bytes.as_ptr(), size, &mut w, &mut h, &mut subsamp, &mut colourspace) == 0
            && w > 0
            && h > 0
            && colourspace != TJCS_CMYK
            && colourspace != TJCS_YCCK
            && (w as usize) * (h as usize) <= MAX_IMAGE_PIXELS
        {
            let grey = colourspace == TJCS_GRAY;
            let channels = if grey { 1 } else { 3 };
            let mut buf = vec![0u8; w as usize * h as usize * channels];
            let pf = if grey { TJPF_GRAY } else { TJPF_RGB };
            if (tj.decompress)(handle, bytes.as_ptr(), size, buf.as_mut_ptr(), w, 0, h, pf, 0) == 0 {
                // Pillow opens a grey JPEG as L, and the script converts L to RGB.
                let data = if grey { buf.iter().flat_map(|&v| [v, v, v]).collect() } else { buf };
                out = Some(Image { width: w as usize, height: h as usize, channels: 3, data, warns_palette_transparency: false });
            }
        }
        (tj.destroy)(handle);
        out
    }
}

// ---------------------------------------------------------------- Resample.c, BICUBIC, 8 bits a channel

const PRECISION_BITS: u32 = 32 - 8 - 2;

fn bicubic_filter(x: f64) -> f64 {
    let a = -0.5;
    let x = if x < 0.0 { -x } else { x };
    if x < 1.0 {
        return ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0;
    }
    if x < 2.0 {
        return (((x - 5.0) * x + 8.0) * x - 4.0) * a;
    }
    0.0
}

fn clip8(v: i32) -> u8 {
    // `_clip8_lookups[640 + (in >> PRECISION_BITS)]`: 0 under zero, 255 over 255.
    (v >> PRECISION_BITS).clamp(0, 255) as u8
}

/// `precompute_coeffs` followed by `normalize_coeffs_8bpc`: per output pixel, the first input index, the count and
/// the fixed-point weights.
fn coeffs(in_size: usize, out_size: usize) -> (usize, Vec<(usize, usize)>, Vec<i32>) {
    let support_base = 2.0;
    let (in0, in1) = (0.0f32, in_size as f32);
    let scale = (in1 - in0) as f64 / out_size as f64;
    let filterscale = if scale < 1.0 { 1.0 } else { scale };
    let support = support_base * filterscale;
    let ksize = support.ceil() as usize * 2 + 1;
    let inv_filterscale = 1.0 / filterscale;
    let mut bounds = Vec::with_capacity(out_size);
    let mut kk = vec![0.0f64; out_size * ksize];
    for xx in 0..out_size {
        let center = in0 as f64 + (xx as f64 + 0.5) * scale;
        let mut ww = 0.0;
        let mut xmin = (center - support + 0.5) as i64;
        if xmin < 0 {
            xmin = 0;
        }
        let mut xmax = (center + support + 0.5) as i64;
        if xmax > in_size as i64 {
            xmax = in_size as i64;
        }
        xmax -= xmin;
        let k = &mut kk[xx * ksize..(xx + 1) * ksize];
        for x in 0..xmax as usize {
            let w = bicubic_filter((x as f64 + xmin as f64 - center + 0.5) * inv_filterscale);
            k[x] = w;
            ww += w;
        }
        if ww != 0.0 {
            for v in k.iter_mut().take(xmax as usize) {
                *v /= ww;
            }
        }
        bounds.push((xmin as usize, xmax as usize));
    }
    let scale_fixed = (1i64 << PRECISION_BITS) as f64;
    let fixed = kk
        .iter()
        .map(|&v| if v < 0.0 { (-0.5 + v * scale_fixed) as i32 } else { (0.5 + v * scale_fixed) as i32 })
        .collect();
    (ksize, bounds, fixed)
}

/// One pass along rows (`horizontal`) or columns, over `channels` bytes a pixel.
fn pass(src: &[u8], w: usize, h: usize, channels: usize, out_len: usize, horizontal: bool) -> Vec<u8> {
    let (ksize, bounds, kk) = coeffs(if horizontal { w } else { h }, out_len);
    let (ow, oh) = if horizontal { (out_len, h) } else { (w, out_len) };
    let mut out = vec![0u8; ow * oh * channels];
    for yy in 0..oh {
        for xx in 0..ow {
            let o = if horizontal { xx } else { yy };
            let (min, count) = bounds[o];
            let k = &kk[o * ksize..o * ksize + count];
            for c in 0..channels {
                let mut ss: i32 = 1 << (PRECISION_BITS - 1);
                for (i, &weight) in k.iter().enumerate() {
                    let idx = if horizontal { (yy * w + min + i) * channels + c } else { ((min + i) * w + xx) * channels + c };
                    ss = ss.wrapping_add((src[idx] as i32).wrapping_mul(weight));
                }
                out[(yy * ow + xx) * channels + c] = clip8(ss);
            }
        }
    }
    out
}

fn muldiv255(a: u32, b: u32) -> u8 {
    let tmp = a * b + 128;
    (((tmp >> 8) + tmp) >> 8) as u8
}

/// `Image.resize(size, BICUBIC)` for RGB, and for RGBA through RGBa (premultiplied) and back.
pub fn resize(im: &Image, nw: usize, nh: usize) -> Image {
    let rgba = im.channels == 4;
    let mut data = if rgba {
        // rgbA2rgba
        im.data
            .chunks_exact(4)
            .flat_map(|p| {
                let a = p[3] as u32;
                [muldiv255(p[0] as u32, a), muldiv255(p[1] as u32, a), muldiv255(p[2] as u32, a), p[3]]
            })
            .collect()
    } else {
        im.data.clone()
    };
    let (mut w, mut h) = (im.width, im.height);
    if nw != w {
        data = pass(&data, w, h, im.channels, nw, true);
        w = nw;
    }
    if nh != h {
        data = pass(&data, w, h, im.channels, nh, false);
        h = nh;
    }
    if rgba {
        // rgba2rgbA
        for p in data.chunks_exact_mut(4) {
            let a = p[3] as u32;
            if a != 255 && a != 0 {
                for c in &mut p[..3] {
                    *c = ((255 * *c as u32) / a).min(255) as u8;
                }
            }
        }
    }
    Image { width: w, height: h, channels: im.channels, data, warns_palette_transparency: im.warns_palette_transparency }
}

// ---------------------------------------------------------------- numpy

/// numpy's `pairwise_sum` for float64 (loops_utils.h.src): blocks of 8 accumulators up to 128 elements, halves
/// rounded down to a multiple of 8 above that.
pub(crate) fn pairwise_sum(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut res = 0.0;
        for &v in a {
            res += v;
        }
        res
    } else if n <= 128 {
        let mut r = [a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]];
        let mut i = 8;
        while i < n - (n % 8) {
            for j in 0..8 {
                r[j] += a[i + j];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        while i < n {
            res += a[i];
            i += 1;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise_sum(&a[..n2]) + pairwise_sum(&a[n2..])
    }
}

/// `np.add.reduce` over a whole contiguous array: the identity 0.0 plus one pairwise sum (no chunking: measured
/// against the venv's numpy on 1-D and 2-D arrays of 100 to 20000 elements).
fn np_sum(a: &[f64]) -> f64 {
    0.0 + pairwise_sum(a)
}

fn np_mean(a: &[f64]) -> f64 {
    np_sum(a) / a.len() as f64
}

pub(crate) fn np_std(a: &[f64]) -> f64 {
    let mean = np_sum(a) / a.len() as f64;
    let sq: Vec<f64> = a
        .iter()
        .map(|&v| {
            let x = v - mean;
            x * x
        })
        .collect();
    (np_sum(&sq) / a.len() as f64).sqrt()
}

unsafe extern "C" {
    /// libm's `pow`, which numpy's scalar `** 2` calls (Rust folds `powf(2.0)` into a multiplication).
    safe fn pow(x: f64, y: f64) -> f64;
}

/// numpy's float remainder (`npy_divmod`): the sign of the divisor, as Python's `%`.
fn np_mod(a: f64, b: f64) -> f64 {
    super::py::fmod(a, b)
}

/// The statistics `_auto_detect_scheme` reads: colourfulness, mean saturation and hue spread.
pub fn scheme_stats(im: &Image) -> (f64, f64, f64) {
    let n = im.width * im.height;
    let (mut rg, mut yb, mut s, mut h) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    for p in im.data.chunks_exact(im.channels) {
        let (rr, gg, bb) = (p[0] as f64, p[1] as f64, p[2] as f64);
        rg.push((rr - gg).abs());
        yb.push((0.5 * (rr + gg) - bb).abs());
        let (r, g, b) = (rr / 255.0, gg / 255.0, bb / 255.0);
        let maxc = r.max(g).max(b);
        let minc = r.min(g).min(b);
        let delta = maxc - minc;
        s.push((if maxc > 0.0 { (maxc - minc) / maxc } else { 0.0 }) * 255.0);
        // The three masked assignments run in order, so blue overwrites green, which overwrites red.
        let mut hv = 0.0;
        if delta > 0.0 {
            if maxc == r {
                hv = 30.0 * np_mod((g - b) / delta, 6.0);
            }
            if maxc == g {
                hv = 30.0 * ((b - r) / delta + 2.0);
            }
            if maxc == b {
                hv = 30.0 * ((r - g) / delta + 4.0);
            }
        }
        h.push(hv);
    }
    let (std_rg, std_yb) = (np_std(&rg), np_std(&yb));
    let (mean_rg, mean_yb) = (np_mean(&rg), np_mean(&yb));
    let colorfulness = (pow(std_rg, 2.0) + pow(std_yb, 2.0)).sqrt() + 0.3 * (pow(mean_rg, 2.0) + pow(mean_yb, 2.0)).sqrt();
    (colorfulness, np_mean(&s), np_std(&h))
}

/// `_auto_detect_scheme`'s decision tree.
pub fn auto_scheme(im: &Image) -> &'static str {
    let (colorfulness, saturation, hue_spread) = scheme_stats(im);
    if saturation < 20.0 {
        return "scheme-monochrome";
    }
    if colorfulness < 30.0 {
        if saturation < 55.0 {
            return "scheme-neutral";
        }
        if hue_spread < 22.0 {
            return "scheme-content";
        }
        return "scheme-tonal-spot";
    }
    if colorfulness < 55.0 {
        if hue_spread < 22.0 && saturation < 100.0 {
            return "scheme-content";
        }
        return "scheme-tonal-spot";
    }
    if colorfulness < 90.0 {
        if saturation > 140.0 && hue_spread < 35.0 {
            return "scheme-fidelity";
        }
        if hue_spread < 30.0 {
            return "scheme-content";
        }
        return "scheme-tonal-spot";
    }
    if hue_spread > 55.0 && saturation > 150.0 {
        return "scheme-rainbow";
    }
    if saturation > 160.0 {
        return "scheme-fidelity";
    }
    if hue_spread > 45.0 {
        return "scheme-expressive";
    }
    "scheme-tonal-spot"
}

/// `calculate_optimal_size`.
pub fn optimal_size(width: usize, height: usize, bitmap_size: i64) -> (usize, usize) {
    let image_area = (width * height) as f64;
    let bitmap_area = (bitmap_size * bitmap_size) as f64;
    let scale = if image_area > bitmap_area { (bitmap_area / image_area).sqrt() } else { 1.0 };
    let nw = (width as f64 * scale).round_ties_even() as usize;
    let nh = (height as f64 * scale).round_ties_even() as usize;
    (nw.max(1), nh.max(1))
}
