//! least_busy_region.py's flow: argparse, the screen image and its `.npy` cache (the same files the original reads
//! and writes, so either can take over), the window search over integral images, numpy's brightness statistics,
//! the k-means dominant colour and the luma grid, printed as Python's json does.

use super::decode::{self, gray_of, Read};
use super::kmeans::{kmeans, Rng};
use super::resize::{resize, Interp};
use super::Mat;
use crate::material::pixels::np_std;
use crate::material::py::{float_repr, parse_float, parse_int};
use std::io::Write as _;

pub struct Context {
    pub site_packages: Option<String>,
}

const PROG: &str = "least_busy_region.py";

const MODES: [&str; 5] = ["fill", "fit", "stretch", "center", "tile"];

struct Args {
    image_path: String,
    width: i64,
    height: i64,
    visual_output: bool,
    screen_width: i64,
    screen_height: i64,
    stride: i64,
    screen_mode: String,
    verbose: bool,
    largest_region: bool,
    variance_threshold: f64,
    aspect_ratio: f64,
    horizontal_padding: i64,
    vertical_padding: i64,
    busiest: bool,
    color_only: bool,
    position_x: i64,
    position_y: i64,
    luma_grid: i64,
}

#[derive(Clone, Copy)]
enum Opt {
    Width,
    Height,
    Visual,
    ScreenWidth,
    ScreenHeight,
    Stride,
    ScreenMode,
    Verbose,
    Largest,
    Threshold,
    Aspect,
    HPad,
    VPad,
    Busiest,
    ColorOnly,
    PosX,
    PosY,
    LumaGrid,
    Help,
}

const LONG: [(&str, Opt); 19] = [
    ("--width", Opt::Width),
    ("--height", Opt::Height),
    ("--visual-output", Opt::Visual),
    ("--screen-width", Opt::ScreenWidth),
    ("--screen-height", Opt::ScreenHeight),
    ("--stride", Opt::Stride),
    ("--screen-mode", Opt::ScreenMode),
    ("--verbose", Opt::Verbose),
    ("--largest-region", Opt::Largest),
    ("--variance-threshold", Opt::Threshold),
    ("--aspect-ratio", Opt::Aspect),
    ("--horizontal-padding", Opt::HPad),
    ("--vertical-padding", Opt::VPad),
    ("--busiest", Opt::Busiest),
    ("--color-only", Opt::ColorOnly),
    ("--position-x", Opt::PosX),
    ("--position-y", Opt::PosY),
    ("--luma-grid", Opt::LumaGrid),
    ("--help", Opt::Help),
];

const SHORT: [(&str, Opt); 6] = [("-v", Opt::Visual), ("-l", Opt::Largest), ("-t", Opt::Threshold), ("-hp", Opt::HPad), ("-vp", Opt::VPad), ("-h", Opt::Help)];

fn takes_value(o: Opt) -> bool {
    !matches!(o, Opt::Visual | Opt::Verbose | Opt::Largest | Opt::Busiest | Opt::ColorOnly | Opt::Help)
}

fn is_negative_number(s: &str) -> bool {
    let t = &s[1..];
    !t.is_empty() && t.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.') && t.parse::<f64>().is_ok()
}

/// argparse for the forms a caller writes (exact or unambiguous long prefixes, `--opt=value`, the short options as
/// given, `-tVALUE`); `None` is anything argparse would answer with usage, help or an error: the original says it.
fn parse_args(argv: &[String]) -> Option<Args> {
    let mut a = Args {
        image_path: String::new(),
        width: 300,
        height: 200,
        visual_output: false,
        screen_width: 1920,
        screen_height: 1080,
        stride: 10,
        screen_mode: "fill".into(),
        verbose: false,
        largest_region: false,
        variance_threshold: 1000.0,
        aspect_ratio: 1.78,
        horizontal_padding: 50,
        vertical_padding: 50,
        busiest: false,
        color_only: false,
        position_x: 0,
        position_y: 0,
        luma_grid: 0,
    };
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    let mut only_positional = false;
    while i < argv.len() {
        let arg = &argv[i];
        i += 1;
        if only_positional || !arg.starts_with('-') || arg == "-" || is_negative_number(arg) {
            positional.push(arg.clone());
            continue;
        }
        if arg == "--" {
            only_positional = true;
            continue;
        }
        let (opt, explicit) = if arg.starts_with("--") {
            let (name, value) = match arg.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (arg.as_str(), None),
            };
            let opt = match LONG.iter().find(|(s, _)| *s == name) {
                Some((_, o)) => *o,
                None => {
                    let hits: Vec<Opt> = LONG.iter().filter(|(s, _)| s.starts_with(name)).map(|(_, o)| *o).collect();
                    if hits.len() != 1 {
                        return None;
                    }
                    hits[0]
                }
            };
            (opt, value)
        } else if let Some((_, o)) = SHORT.iter().find(|(s, _)| *s == arg) {
            (*o, None)
        } else if arg.len() > 2 && arg.starts_with("-t") && !arg.contains('=') {
            (Opt::Threshold, Some(arg[2..].to_string()))
        } else {
            return None;
        };
        if matches!(opt, Opt::Help) {
            return None;
        }
        if !takes_value(opt) {
            if explicit.is_some() {
                return None;
            }
            match opt {
                Opt::Visual => a.visual_output = true,
                Opt::Verbose => a.verbose = true,
                Opt::Largest => a.largest_region = true,
                Opt::Busiest => a.busiest = true,
                Opt::ColorOnly => a.color_only = true,
                _ => {}
            }
            continue;
        }
        let value = match explicit {
            Some(v) => v,
            None => {
                let v = argv.get(i)?;
                if v.starts_with('-') && v != "-" && !is_negative_number(v) {
                    return None;
                }
                i += 1;
                v.clone()
            }
        };
        let int = || parse_int(&value);
        match opt {
            Opt::Width => a.width = int()?,
            Opt::Height => a.height = int()?,
            Opt::ScreenWidth => a.screen_width = int()?,
            Opt::ScreenHeight => a.screen_height = int()?,
            Opt::Stride => a.stride = int()?,
            Opt::HPad => a.horizontal_padding = int()?,
            Opt::VPad => a.vertical_padding = int()?,
            Opt::PosX => a.position_x = int()?,
            Opt::PosY => a.position_y = int()?,
            Opt::LumaGrid => a.luma_grid = int()?,
            Opt::Threshold => a.variance_threshold = parse_float(&value)?,
            Opt::Aspect => a.aspect_ratio = parse_float(&value)?,
            Opt::ScreenMode => {
                if !MODES.contains(&value.as_str()) {
                    return None;
                }
                a.screen_mode = value;
            }
            _ => return None,
        }
    }
    if positional.len() != 1 {
        return None;
    }
    a.image_path = positional.remove(0);
    Some(a)
}

/// A Python exception escaping main(): the traceback's last line on stderr, exit 1.
struct Raise;

fn raise(kind: &str, message: &str) -> Raise {
    eprintln!("Traceback (most recent call last):\n  File \"least_busy_region.py\", in <module>\n{kind}: {message}");
    Raise
}

enum Fail {
    Raise,
}

impl From<Raise> for Fail {
    fn from(_: Raise) -> Self {
        Fail::Raise
    }
}

pub fn run(argv: &[String], ctx: &Context) -> i32 {
    let help = include_str!("help.txt");
    let Some(args) = parse_args(argv) else {
        if argv.iter().take_while(|a| *a != "--").any(|a| a == "-h" || a == "--help") {
            print!("{help}");
            return 0;
        }
        eprint!("{}{PROG}: error: invalid arguments\n", help.split("\n\n").next().unwrap_or_default().to_string() + "\n");
        return 2;
    };
    // The original drew the region into an image file, or turned these into numpy warnings and odd slices.
    let draws = args.visual_output && args.luma_grid <= 0 && !args.color_only;
    if draws {
        eprintln!("{PROG}: error: --visual-output without --luma-grid or --color-only drew an image; it is gone");
        return 2;
    }
    if args.screen_width <= 0
        || args.screen_height <= 0
        || args.width <= 0
        || args.height <= 0
        || args.horizontal_padding < 0
        || args.vertical_padding < 0
        || !args.aspect_ratio.is_finite()
        || args.variance_threshold.is_nan()
    {
        eprintln!("{PROG}: error: sizes must be positive, paddings at least 0, the aspect ratio and threshold numbers");
        return 2;
    }
    let mut out = String::new();
    let mut loader = Loader { ctx, args: &args, cache: Vec::new() };
    match flow(&args, &mut loader, &mut out) {
        Ok(()) => {
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(out.as_bytes());
            let _ = stdout.flush();
            0
        }
        Err(Fail::Raise) => {
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(out.as_bytes());
            let _ = stdout.flush();
            1
        }
    }
}

fn flow(args: &Args, loader: &mut Loader, out: &mut String) -> Result<(), Fail> {
    let (sw, sh, mode) = (args.screen_width, args.screen_height, args.screen_mode.as_str());
    if args.luma_grid > 0 {
        let Some(img) = loader.load(false)? else { return Err(raise("FileNotFoundError", &format!("Image not found: {}", args.image_path)).into()) };
        out.push_str(&luma_json(&luma_grid(&img, args.luma_grid as usize)));
        out.push('\n');
        return Ok(());
    }
    if args.color_only {
        let (px, py) = (args.position_x, args.position_y);
        let colour = dominant_colour(loader, px, py, args.width, args.height)?;
        let (b, s) = brightness(loader, px, py, args.width, args.height)?;
        out.push_str(&format!(
            "{{\"center_x\": {}, \"center_y\": {}, \"width\": {}, \"height\": {}, \"dominant_color\": \"{}\", \"brightness\": {}, \"brightness_std\": {}}}\n",
            px + args.width.div_euclid(2),
            py + args.height.div_euclid(2),
            args.width,
            args.height,
            colour,
            float_repr(round1(b)),
            float_repr(round1(s))
        ));
        return Ok(());
    }
    if args.largest_region {
        let Some(gray) = loader.load(true)? else { return Err(raise("FileNotFoundError", &format!("Image not found: {}", args.image_path)).into()) };
        if args.verbose {
            out.push_str(&format!("Using screen image of {}x{} (mode: {mode})\n", gray.w, gray.h));
        }
        match largest_region(&gray, args) {
            Some((cx, cy, rw, rh, var)) => {
                let (x1, y1) = (cx - rw.div_euclid(2), cy - rh.div_euclid(2));
                let colour = dominant_colour(loader, x1, y1, rw, rh)?;
                let (b, s) = brightness(loader, x1, y1, rw, rh)?;
                out.push_str(&format!(
                    "{{\"center_x\": {cx}, \"center_y\": {cy}, \"width\": {rw}, \"height\": {rh}, \"variance\": {}, \"dominant_color\": \"{colour}\", \"brightness\": {}, \"brightness_std\": {}}}\n",
                    float_repr(var),
                    float_repr(round1(b)),
                    float_repr(round1(s))
                ));
            }
            None => out.push_str("{\"error\": \"No region found under the threshold.\"}\n"),
        }
        return Ok(());
    }
    let Some(gray) = loader.load(true)? else { return Err(raise("FileNotFoundError", &format!("Image not found: {}", args.image_path)).into()) };
    if args.verbose {
        out.push_str(&format!("Using screen image of {}x{} (mode: {mode})\n", gray.w, gray.h));
    }
    let ((x, y), var) = least_busy(&gray, args, out)?;
    let _ = (sw, sh);
    let colour = dominant_colour(loader, x, y, args.width, args.height)?;
    let (b, s) = brightness(loader, x, y, args.width, args.height)?;
    out.push_str(&format!(
        "{{\"center_x\": {}, \"center_y\": {}, \"width\": {}, \"height\": {}, \"variance\": {}, \"dominant_color\": \"{colour}\", \"brightness\": {}, \"brightness_std\": {}}}\n",
        x + args.width.div_euclid(2),
        y + args.height.div_euclid(2),
        args.width,
        args.height,
        var.map_or("null".to_string(), float_repr),
        float_repr(round1(b)),
        float_repr(round1(s))
    ));
    Ok(())
}

/// Python's `round(x, 1)`: the nearest one-decimal value of the exact double, ties to even.
fn round1(x: f64) -> f64 {
    format!("{x:.1}").parse().unwrap_or(x)
}

unsafe extern "C" {
    /// libm's `pow`, which numpy's scalar `mean ** 2` calls.
    safe fn pow(x: f64, y: f64) -> f64;
}

fn square(x: f64) -> f64 {
    pow(x, std::hint::black_box(2.0))
}

// ---------------------------------------------------------------- screen image and its cache

struct Loader<'a> {
    ctx: &'a Context,
    args: &'a Args,
    /// What the run already holds, so a delegation can still happen before anything prints.
    cache: Vec<(bool, Option<Mat>)>,
}

impl Loader<'_> {
    fn load(&mut self, gray: bool) -> Result<Option<Mat>, Fail> {
        let a = self.args;
        let m = load_screen_image(&a.image_path, a.screen_width, a.screen_height, &a.screen_mode, gray, self.ctx.site_packages.as_deref())?;
        self.cache.push((gray, None));
        Ok(m)
    }
}

fn read_screen_image(path: &str, gray: bool, sw: usize, sh: usize, mode: &str, site: Option<&str>) -> Result<Option<Mat>, Fail> {
    let img = match decode::imread(path, gray, site) {
        Read::Ok(m) => m,
        Read::None => return Ok(None),
    };
    Ok(Some(screen_image(&img, sw, sh, mode)))
}

pub fn screen_image(img: &Mat, sw: usize, sh: usize, mode: &str) -> Mat {
    let (ow, oh) = (img.w, img.h);
    match mode {
        "stretch" => resize(img, sw, sh, Interp::Area),
        "center" => center_on_black(img, sw, sh),
        "tile" => {
            let (sw_i, sh_i, ow_i, oh_i) = (sw as i64, sh as i64, ow as i64, oh as i64);
            let x = (ow_i - (sw_i - ow_i).div_euclid(2).rem_euclid(ow_i)).rem_euclid(ow_i) as usize;
            let y = (oh_i - (sh_i - oh_i).div_euclid(2).rem_euclid(oh_i)).rem_euclid(oh_i) as usize;
            let cn = img.cn;
            let mut data = vec![0u8; sw * sh * cn];
            for r in 0..sh {
                let sy = (y + r) % oh;
                for c in 0..sw {
                    let sx = (x + c) % ow;
                    data[(r * sw + c) * cn..(r * sw + c + 1) * cn].copy_from_slice(&img.data[(sy * ow + sx) * cn..(sy * ow + sx + 1) * cn]);
                }
            }
            Mat { w: sw, h: sh, cn, data }
        }
        _ => {
            let scale_w = sw as f64 / ow as f64;
            let scale_h = sh as f64 / oh as f64;
            let scale = if mode == "fit" { scale_w.min(scale_h) } else { scale_w.max(scale_h) };
            let nw = ((ow as f64 * scale) as i64).max(1) as usize;
            let nh = ((oh as f64 * scale) as i64).max(1) as usize;
            let r = resize(img, nw, nh, Interp::Lanczos4);
            if mode == "fit" { center_on_black(&r, sw, sh) } else { center_crop(&r, sw, sh) }
        }
    }
}

fn center_crop(img: &Mat, tw: usize, th: usize) -> Mat {
    if img.w == tw && img.h == th {
        return Mat { w: img.w, h: img.h, cn: img.cn, data: img.data.clone() };
    }
    let x1 = (img.w as i64 - tw as i64).div_euclid(2).max(0) as usize;
    let y1 = (img.h as i64 - th as i64).div_euclid(2).max(0) as usize;
    let x2 = (x1 + tw).min(img.w);
    let y2 = (y1 + th).min(img.h);
    crop(img, x1, y1, x2.saturating_sub(x1), y2.saturating_sub(y1))
}

fn crop(img: &Mat, x: usize, y: usize, w: usize, h: usize) -> Mat {
    let cn = img.cn;
    let mut data = Vec::with_capacity(w * h * cn);
    for r in y..y + h {
        data.extend_from_slice(&img.data[(r * img.w + x) * cn..(r * img.w + x + w) * cn]);
    }
    Mat { w, h, cn, data }
}

fn center_on_black(img: &Mat, tw: usize, th: usize) -> Mat {
    let img = center_crop(img, tw, th);
    let cn = img.cn;
    let mut data = vec![0u8; tw * th * cn];
    let x = (tw - img.w) / 2;
    let y = (th - img.h) / 2;
    for r in 0..img.h {
        let d = ((y + r) * tw + x) * cn;
        data[d..d + img.w * cn].copy_from_slice(&img.data[r * img.w * cn..(r + 1) * img.w * cn]);
    }
    Mat { w: tw, h: th, cn, data }
}

fn cache_dir() -> String {
    let base = match std::env::var("XDG_CACHE_HOME") {
        Ok(v) if !v.is_empty() => v,
        _ => format!("{}/.cache", std::env::var("HOME").unwrap_or_default()),
    };
    format!("{base}/inir/region-sampling")
}

const CACHE_KEEP: usize = 8;

fn load_screen_image(path: &str, sw: i64, sh: i64, mode: &str, gray: bool, site: Option<&str>) -> Result<Option<Mat>, Fail> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let (swu, shu) = (sw as usize, sh as usize);
    let Ok(meta) = std::fs::metadata(path) else { return read_screen_image(path, gray, swu, shu, mode, site) };
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.into()));
    let mtime_ns = meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128;
    let key = sha1_hex(format!("{}|{mtime_ns}|{}|{sw}x{sh}|{mode}|{}", real.to_string_lossy(), meta.size(), if gray { "gray" } else { "bgr" }).as_bytes());
    let dir = cache_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return read_screen_image(path, gray, swu, shu, mode, site);
    }
    let cache_path = format!("{dir}/{key}.npy");
    let Ok(lock) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o666).open(format!("{dir}/{key}.lock")) else {
        return read_screen_image(path, gray, swu, shu, mode, site);
    };
    use std::os::fd::AsRawFd;
    unsafe {
        libc::flock(lock.as_raw_fd(), libc::LOCK_EX);
    }
    if let Ok(bytes) = std::fs::read(&cache_path) {
        match npy_load(&bytes) {
            Npy::Ok(m) => return Ok(Some(m)),
            // an array this port doesn't write: read the image again and write over it
            Npy::Foreign | Npy::Invalid => {}
        }
    }
    let Some(img) = read_screen_image(path, gray, swu, shu, mode, site)? else { return Ok(None) };
    let _ = save_cache(&dir, &cache_path, &img);
    drop(lock);
    Ok(Some(img))
}

/// np.save through NamedTemporaryFile (mode 0600, `tmp*.tmp`) and os.replace, then keep the newest eight.
fn save_cache(dir: &str, cache_path: &str, img: &Mat) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let bytes = npy_bytes(img);
    let mut seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) ^ std::process::id() as u64;
    let (tmp_path, mut file) = loop {
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
        let mut name = String::from("tmp");
        for _ in 0..8 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            name.push(CHARS[(seed % CHARS.len() as u64) as usize] as char);
        }
        let p = format!("{dir}/{name}.tmp");
        match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&p) {
            Ok(f) => break (p, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    file.write_all(&bytes)?;
    drop(file);
    std::fs::rename(&tmp_path, cache_path)?;
    let mut entries: Vec<(f64, String)> = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name.ends_with(".npy") {
            use std::os::unix::fs::MetadataExt;
            let m = std::fs::metadata(e.path())?;
            entries.push((m.mtime() as f64 + m.mtime_nsec() as f64 * 1e-9, e.path().to_string_lossy().into_owned()));
        }
    }
    entries.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, stale) in entries.iter().skip(CACHE_KEEP) {
        let stem = &stale[..stale.len() - 4];
        let _ = std::fs::remove_file(format!("{stem}.npy"));
        let _ = std::fs::remove_file(format!("{stem}.lock"));
    }
    Ok(())
}

/// numpy.lib.format version 1.0 for a C-ordered uint8 array, header grown and padded as np.save writes it.
fn npy_bytes(img: &Mat) -> Vec<u8> {
    let shape = if img.cn == 1 { format!("({}, {})", img.h, img.w) } else { format!("({}, {}, {})", img.h, img.w, img.cn) };
    let mut header = format!("{{'descr': '|u1', 'fortran_order': False, 'shape': {shape}, }}");
    header.push_str(&" ".repeat(21usize.saturating_sub(img.h.to_string().len())));
    let hlen = header.len() + 1;
    let padlen = 64 - ((8 + 2 + hlen) % 64);
    let mut out = Vec::with_capacity(10 + hlen + padlen + img.data.len());
    out.extend_from_slice(b"\x93NUMPY\x01\x00");
    out.extend_from_slice(&((hlen + padlen) as u16).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend(std::iter::repeat_n(b' ', padlen));
    out.push(b'\n');
    out.extend_from_slice(&img.data);
    out
}

enum Npy {
    Ok(Mat),
    /// A readable array np.load would hand back that is not what this script saves.
    Foreign,
    /// np.load raises (OSError, ValueError): the original recomputes.
    Invalid,
}

fn npy_load(b: &[u8]) -> Npy {
    if b.len() < 10 || &b[0..6] != b"\x93NUMPY" {
        return Npy::Invalid;
    }
    let (hlen, start) = match b[6] {
        1 => (u16::from_le_bytes([b[8], b[9]]) as usize, 10),
        2 | 3 if b.len() >= 12 => (u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize, 12),
        _ => return Npy::Invalid,
    };
    let Some(header) = b.get(start..start + hlen).and_then(|h| std::str::from_utf8(h).ok()) else { return Npy::Invalid };
    let compact: String = header.chars().filter(|c| !c.is_whitespace()).collect();
    let Some(shape) = compact.split("'shape':(").nth(1).and_then(|s| s.split(')').next()) else { return Npy::Invalid };
    let dims: Vec<usize> = shape.split(',').filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    if !compact.contains("'descr':'|u1'") || !compact.contains("'fortran_order':False") {
        return Npy::Foreign;
    }
    let data = &b[start + hlen..];
    let (h, w, cn) = match dims.as_slice() {
        [h, w] => (*h, *w, 1),
        [h, w, 3] => (*h, *w, 3),
        _ => return Npy::Foreign,
    };
    if data.len() != h * w * cn {
        return Npy::Invalid;
    }
    Npy::Ok(Mat { w, h, cn, data: data.to_vec() })
}

pub(crate) fn sha1_hex(msg: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut m = msg.to_vec();
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&((msg.len() as u64) * 8).to_be_bytes());
    for chunk in m.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

// ---------------------------------------------------------------- the search

fn integral(img: &Mat, square: bool) -> Vec<f64> {
    let (w, h) = (img.w, img.h);
    let mut ii = vec![0f64; w * h];
    let mut above = vec![0u64; w];
    for y in 0..h {
        let mut run = 0u64;
        for x in 0..w {
            let v = img.data[y * w + x] as u64;
            run += if square { v * v } else { v };
            above[x] += run;
            ii[y * w + x] = above[x] as f64;
        }
    }
    ii
}

fn region_sum(ii: &[f64], w: usize, x1: i64, y1: i64, x2: i64, y2: i64) -> f64 {
    let at = |x: i64, y: i64| ii[y as usize * w + x as usize];
    let mut total = at(x2, y2);
    if x1 > 0 {
        total -= at(x1 - 1, y2);
    }
    if y1 > 0 {
        total -= at(x2, y1 - 1);
    }
    if x1 > 0 && y1 > 0 {
        total += at(x1 - 1, y1 - 1);
    }
    total
}

fn least_busy(img: &Mat, args: &Args, out: &mut String) -> Result<((i64, i64), Option<f64>), Fail> {
    let (w, h) = (img.w as i64, img.h as i64);
    let stride = args.stride.max(1);
    let (mut hp, mut vp) = (args.horizontal_padding, args.vertical_padding);
    if hp * 2 >= w || vp * 2 >= h {
        hp = hp.min((w - 1).div_euclid(2)).max(0);
        vp = vp.min((h - 1).div_euclid(2)).max(0);
    }
    let (max_w, max_h) = (w - 2 * hp, h - 2 * vp);
    if max_w <= 0 || max_h <= 0 {
        return Err(raise("ValueError", "Image too small for the specified padding.").into());
    }
    let (mut rw, mut rh) = (args.width, args.height);
    if rw > max_w {
        if args.verbose {
            out.push_str(&format!("Requested region_width {rw} too large; clamping to {max_w}\n"));
        }
        rw = max_w;
    }
    if rh > max_h {
        if args.verbose {
            out.push_str(&format!("Requested region_height {rh} too large; clamping to {max_h}\n"));
        }
        rh = max_h;
    }
    let ii = integral(img, false);
    let ii2 = integral(img, true);
    let area = (rw * rh) as f64;
    let (mut min_var, mut max_var): (Option<f64>, Option<f64>) = (None, None);
    let (mut min_c, mut max_c) = ((hp, vp), (hp, vp));
    let x_end = (w - rw - hp + 1).max(hp);
    let y_end = (h - rh - vp + 1).max(vp);
    let mut y = vp;
    while y <= y_end {
        let mut x = hp;
        while x <= x_end {
            let (x2, y2) = (x + rw - 1, y + rh - 1);
            if x2 < w && y2 < h {
                let s = region_sum(&ii, img.w, x, y, x2, y2);
                let s2 = region_sum(&ii2, img.w, x, y, x2, y2);
                let mean = s / area;
                let var = (s2 / area) - square(mean);
                if min_var.is_none_or(|m| var < m) {
                    min_var = Some(var);
                    min_c = (x, y);
                }
                if max_var.is_none_or(|m| var > m) {
                    max_var = Some(var);
                    max_c = (x, y);
                }
            }
            x += stride;
        }
        y += stride;
    }
    Ok(if args.busiest { (max_c, max_var) } else { (min_c, min_var) })
}

fn largest_region(img: &Mat, args: &Args) -> Option<(i64, i64, i64, i64, f64)> {
    let (w, h) = (img.w as i64, img.h as i64);
    let stride = args.stride.max(1);
    let threshold = args.variance_threshold.max(0.0);
    let ar = args.aspect_ratio;
    let (mut hp, mut vp) = (args.horizontal_padding, args.vertical_padding);
    if hp * 2 >= w || vp * 2 >= h {
        hp = hp.min((w - 1).div_euclid(2)).max(0);
        vp = vp.min((h - 1).div_euclid(2)).max(0);
    }
    let ii = integral(img, false);
    let ii2 = integral(img, true);
    let mut min_size: i64 = 10;
    let (eff_w, eff_h) = (w - 2 * hp, h - 2 * vp);
    if eff_w <= 0 || eff_h <= 0 {
        return None;
    }
    let mut max_size = if ar >= 1.0 { eff_h.min((eff_w as f64 / ar) as i64) } else { ((eff_h as f64 * ar) as i64).min(eff_w) };
    if max_size < min_size {
        min_size = 1;
        max_size = max_size.max(1);
    }
    let mut best = None;
    while min_size <= max_size {
        let mid = (min_size + max_size).div_euclid(2);
        let (rw, rh) = if ar >= 1.0 {
            (mid, (mid as f64 * ar).round_ties_even() as i64)
        } else {
            (mid, (if ar != 0.0 { mid as f64 / ar } else { mid as f64 }).round_ties_even() as i64)
        };
        let (rw, rh) = if ar >= 1.0 { (rh, rw) } else { (rw, rh) };
        if rw <= 0 || rh <= 0 {
            break;
        }
        if rw > eff_w || rh > eff_h {
            max_size = mid - 1;
            continue;
        }
        let mut found = false;
        let (x_end, y_end) = (w - rw - hp, h - rh - vp);
        let mut y = vp;
        'rows: while y <= y_end {
            let mut x = hp;
            while x <= x_end {
                let (x2, y2) = (x + rw - 1, y + rh - 1);
                if x2 < w && y2 < h {
                    let s = region_sum(&ii, img.w, x, y, x2, y2);
                    let s2 = region_sum(&ii2, img.w, x, y, x2, y2);
                    let area = (rw * rh) as f64;
                    let mean = s / area;
                    let var = (s2 / area) - square(mean);
                    if var <= threshold {
                        found = true;
                        best = Some((x, y, rw, rh, var));
                        break 'rows;
                    }
                }
                x += stride;
            }
            y += stride;
        }
        if found {
            min_size = mid + 1;
        } else {
            max_size = mid - 1;
        }
    }
    best.map(|(x, y, rw, rh, var)| (x + rw.div_euclid(2), y + rh.div_euclid(2), rw, rh, var))
}

/// The region `img[y:y+h, x:x+w]` after the script's clamps, as Python slices it.
fn region_of(img: &Mat, x: i64, y: i64, w: i64, h: i64) -> Mat {
    let x = x.max(0);
    let y = y.max(0);
    let w = w.min(img.w as i64 - x).max(1);
    let h = h.min(img.h as i64 - y).max(1);
    let (x0, y0) = ((x as usize).min(img.w), (y as usize).min(img.h));
    let x1 = ((x + w) as usize).min(img.w);
    let y1 = ((y + h) as usize).min(img.h);
    crop(img, x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

fn brightness(loader: &mut Loader, x: i64, y: i64, w: i64, h: i64) -> Result<(f64, f64), Fail> {
    let Some(img) = loader.load(true)? else { return Ok((128.0, 0.0)) };
    let r = region_of(&img, x, y, w, h);
    if r.data.is_empty() {
        return Ok((128.0, 0.0));
    }
    let sum: u64 = r.data.iter().map(|&v| v as u64).sum();
    let vals: Vec<f64> = r.data.iter().map(|&v| v as f64).collect();
    Ok((sum as f64 / vals.len() as f64, np_std(&vals)))
}

fn dominant_colour(loader: &mut Loader, x: i64, y: i64, w: i64, h: i64) -> Result<String, Fail> {
    let Some(img) = loader.load(false)? else { return Err(raise("FileNotFoundError", &format!("Image not found: {}", loader.args.image_path)).into()) };
    let r = region_of(&img, x, y, w, h);
    let rgb = if r.data.is_empty() {
        [0i64; 3]
    } else {
        let px: Vec<[f32; 3]> = r.data.chunks_exact(3).map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        let bgr: [f32; 3] = if px.len() < 3 {
            let n = px.len() as f32;
            let mut s = [0f32; 3];
            for p in &px {
                for j in 0..3 {
                    s[j] += p[j];
                }
            }
            [s[0] / n, s[1] / n, s[2] / n]
        } else {
            let k = px.len().min(3);
            let (labels, centers) = kmeans(&px, k, 10, 1.0, 10, &mut Rng::new(0));
            let mut counts = vec![0usize; k];
            for &l in &labels {
                counts[l] += 1;
            }
            let mut best = 0;
            for (i, &c) in counts.iter().enumerate() {
                if c > counts[best] {
                    best = i;
                }
            }
            centers[best]
        };
        [bgr[2] as i64, bgr[1] as i64, bgr[0] as i64]
    };
    Ok(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]))
}

// ---------------------------------------------------------------- luma grid

pub struct LumaGrid {
    pub cols: usize,
    pub rows: usize,
    pub cell: usize,
    pub width: usize,
    pub height: usize,
    pub mean: Vec<i64>,
    pub sq: Vec<i64>,
    pub r: Vec<i64>,
    pub g: Vec<i64>,
    pub b: Vec<i64>,
}

/// numpy's float32 pairwise sum (8 accumulators, blocks of 128).
fn pairwise_f32(a: &[f32]) -> f32 {
    let n = a.len();
    if n < 8 {
        let mut res = 0f32;
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
        pairwise_f32(&a[..n2]) + pairwise_f32(&a[n2..])
    }
}

/// `round()` on a numpy float32, to a Python int: ties to even.
fn round_f32(v: f32) -> i64 {
    v.round_ties_even() as i64
}

pub fn luma_grid(img: &Mat, cell: usize) -> LumaGrid {
    let (w, h) = (img.w, img.h);
    let cols = w.div_ceil(cell).max(1);
    let rows = h.div_ceil(cell).max(1);
    let (pw, ph) = (cols * cell, rows * cell);
    let px = |x: usize, y: usize| -> &[u8] {
        let (x, y) = (x.min(w - 1), y.min(h - 1));
        &img.data[(y * w + x) * 3..(y * w + x) * 3 + 3]
    };
    let mut gray = vec![0f32; pw * ph];
    for y in 0..ph {
        for x in 0..pw {
            let p = px(x, y);
            gray[y * pw + x] = gray_of(p[0], p[1], p[2]) as f32;
        }
    }
    let count = (cell * cell) as f32;
    let mut g = LumaGrid { cols, rows, cell, width: w, height: h, mean: vec![], sq: vec![], r: vec![], g: vec![], b: vec![] };
    let mut row_sq = vec![0f32; cell];
    for r in 0..rows {
        for c in 0..cols {
            let (mut m, mut s) = (0f32, 0f32);
            let mut bgr = [0u64; 3];
            for cy in 0..cell {
                let y = r * cell + cy;
                let line = &gray[y * pw + c * cell..y * pw + c * cell + cell];
                m += pairwise_f32(line);
                for (o, &v) in row_sq.iter_mut().zip(line) {
                    *o = v * v;
                }
                s += pairwise_f32(&row_sq);
                for cx in 0..cell {
                    let p = px(c * cell + cx, y);
                    for k in 0..3 {
                        bgr[k] += p[k] as u64;
                    }
                }
            }
            g.mean.push(round_f32(m / count));
            g.sq.push(round_f32(s / count));
            g.b.push(round_f32(bgr[0] as f32 / count));
            g.g.push(round_f32(bgr[1] as f32 / count));
            g.r.push(round_f32(bgr[2] as f32 / count));
        }
    }
    g
}

pub fn luma_json(g: &LumaGrid) -> String {
    let list = |v: &[i64]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
    format!(
        "{{\"cols\":{},\"rows\":{},\"cell\":{},\"width\":{},\"height\":{},\"mean\":[{}],\"sq\":[{}],\"r\":[{}],\"g\":[{}],\"b\":[{}]}}",
        g.cols,
        g.rows,
        g.cell,
        g.width,
        g.height,
        list(&g.mean),
        list(&g.sq),
        list(&g.r),
        list(&g.g),
        list(&g.b)
    )
}

/// The screen image the shell's widgets read, for callers outside the script (the QML plugin, the bench).
pub fn screen_image_of(path: &str, sw: usize, sh: usize, mode: &str, site: Option<&str>) -> Option<Mat> {
    match decode::imread(path, false, site) {
        Read::Ok(m) => Some(screen_image(&m, sw, sh, mode)),
        _ => None,
    }
}
