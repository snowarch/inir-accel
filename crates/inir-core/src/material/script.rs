//! generate_colors_material.py itself: argparse, the colour flow and every effect in the original's order (cache,
//! iris-seeds, SCSS file, stdout, the JSON files, templates). Python's failures stay failures: a step that raises
//! there exits 1 here after the same effects; what the port does not cover returns `Run::Delegate` before any.

use super::hct::{difference_degrees, rotation_direction, sanitize_degrees_double, Hct};
use super::pixels;
use super::py::{self, Json};
use super::quantize;
use super::scheme::{self, Scheme, Variant, ROLES};
use std::path::Path;

pub enum Run {
    Exit(i32),
    Delegate,
}

/// Where the original's own warnings point (`<venv site-packages>/PIL/Image.py`).
pub struct Context {
    pub site_packages: String,
}

const PROG: &str = "generate_colors_material.py";

const USAGE: &str = "usage: generate_colors_material.py [-h] [--path PATH] [--size SIZE]
                                   [--color COLOR] [--mode {dark,light}]
                                   [--scheme SCHEME] [--smart]
                                   [--transparency {opaque,transparent}]
                                   [--termscheme TERMSCHEME]
                                   [--harmony HARMONY]
                                   [--harmonize_threshold HARMONIZE_THRESHOLD]
                                   [--term_fg_boost TERM_FG_BOOST]
                                   [--term_saturation TERM_SATURATION]
                                   [--term_brightness TERM_BRIGHTNESS]
                                   [--term_bg_brightness TERM_BG_BRIGHTNESS]
                                   [--blend_bg_fg] [--cache CACHE] [--soften]
                                   [--debug] [--json-output JSON_OUTPUT]
                                   [--palette-output PALETTE_OUTPUT]
                                   [--app-palette-output APP_PALETTE_OUTPUT]
                                   [--terminal-output TERMINAL_OUTPUT]
                                   [--meta-output META_OUTPUT]
                                   [--scss-output SCSS_OUTPUT]
                                   [--surface-seed SURFACE_SEED]
                                   [--accent-seed ACCENT_SEED]
                                   [--washi-roles WASHI_ROLES]
                                   [--render-templates RENDER_TEMPLATES]
                                   [--color-strength COLOR_STRENGTH]
                                   [--invert-hue]
";

// ---------------------------------------------------------------- argparse

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Help,
    Flag,
    Str,
    Int,
    Float,
    Choice(&'static [&'static str]),
}

/// `parser._option_string_actions` in insertion order (the order an ambiguity message lists them).
const OPTIONS: &[(&str, Kind)] = &[
    ("-h", Kind::Help),
    ("--help", Kind::Help),
    ("--path", Kind::Str),
    ("--size", Kind::Int),
    ("--color", Kind::Str),
    ("--mode", Kind::Choice(&["dark", "light"])),
    ("--scheme", Kind::Str),
    ("--smart", Kind::Flag),
    ("--transparency", Kind::Choice(&["opaque", "transparent"])),
    ("--termscheme", Kind::Str),
    ("--harmony", Kind::Float),
    ("--harmonize_threshold", Kind::Float),
    ("--term_fg_boost", Kind::Float),
    ("--term_saturation", Kind::Float),
    ("--term_brightness", Kind::Float),
    ("--term_bg_brightness", Kind::Float),
    ("--blend_bg_fg", Kind::Flag),
    ("--cache", Kind::Str),
    ("--soften", Kind::Flag),
    ("--debug", Kind::Flag),
    ("--json-output", Kind::Str),
    ("--palette-output", Kind::Str),
    ("--app-palette-output", Kind::Str),
    ("--terminal-output", Kind::Str),
    ("--meta-output", Kind::Str),
    ("--scss-output", Kind::Str),
    ("--surface-seed", Kind::Str),
    ("--accent-seed", Kind::Str),
    ("--washi-roles", Kind::Str),
    ("--render-templates", Kind::Str),
    ("--color-strength", Kind::Float),
    ("--invert-hue", Kind::Flag),
];

/// A Python number as argparse leaves it: `--harmonize_threshold` defaults to the int 100, parsed values are floats.
#[derive(Clone, Copy)]
enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    fn f(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }
    fn json(self) -> Json {
        match self {
            Num::Int(i) => Json::Int(i as i128),
            Num::Float(f) => Json::Float(f),
        }
    }
}

struct Args {
    path: Option<String>,
    size: i64,
    color: Option<String>,
    mode: String,
    scheme: String,
    smart: bool,
    transparency: String,
    termscheme: Option<String>,
    harmony: f64,
    harmonize_threshold: Num,
    term_fg_boost: f64,
    term_saturation: f64,
    term_brightness: f64,
    term_bg_brightness: f64,
    blend_bg_fg: bool,
    cache: Option<String>,
    soften: bool,
    debug: bool,
    json_output: Option<String>,
    palette_output: Option<String>,
    app_palette_output: Option<String>,
    terminal_output: Option<String>,
    meta_output: Option<String>,
    scss_output: Option<String>,
    surface_seed: Option<String>,
    accent_seed: Option<String>,
    washi_roles: Option<String>,
    render_templates: Option<String>,
    color_strength: f64,
    invert_hue: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            path: None,
            size: 128,
            color: None,
            mode: "dark".into(),
            scheme: "vibrant".into(),
            smart: false,
            transparency: "opaque".into(),
            termscheme: None,
            harmony: 0.4,
            harmonize_threshold: Num::Int(100),
            term_fg_boost: 0.35,
            term_saturation: 0.65,
            term_brightness: 0.60,
            term_bg_brightness: 0.50,
            blend_bg_fg: false,
            cache: None,
            soften: false,
            debug: false,
            json_output: None,
            palette_output: None,
            app_palette_output: None,
            terminal_output: None,
            meta_output: None,
            scss_output: None,
            surface_seed: None,
            accent_seed: None,
            washi_roles: None,
            render_templates: None,
            color_strength: 1.0,
            invert_hue: false,
        }
    }
}

enum Parsed {
    Args(Box<Args>),
    Error(String),
    Help,
    /// An int past i64 (Python has no limit).
    Unsupported,
}

/// What `_parse_optional` makes of one string: a positional, a known option (with an `=` value), or an unknown one.
enum Class {
    Positional,
    Known(usize, Option<String>),
    Unknown,
}

fn is_negative_number(s: &str) -> bool {
    // '^-\d+$|^-\d*\.\d+$'
    let Some(body) = s.strip_prefix('-') else { return false };
    let digits = |t: &str| t.chars().all(|c| c.is_ascii_digit());
    if !body.is_empty() && digits(body) {
        return true;
    }
    match body.split_once('.') {
        Some((a, b)) => digits(a) && !b.is_empty() && digits(b),
        None => false,
    }
}

fn classify(arg: &str) -> Result<Class, String> {
    if arg.is_empty() || !arg.starts_with('-') {
        return Ok(Class::Positional);
    }
    if let Some(i) = OPTIONS.iter().position(|(o, _)| *o == arg) {
        return Ok(Class::Known(i, None));
    }
    if arg.chars().count() == 1 {
        return Ok(Class::Positional);
    }
    if let Some((opt, explicit)) = arg.split_once('=') {
        if let Some(i) = OPTIONS.iter().position(|(o, _)| *o == opt) {
            return Ok(Class::Known(i, Some(explicit.to_string())));
        }
    }
    // Abbreviations (allow_abbrev).
    let mut matches: Vec<(usize, Option<String>)> = Vec::new();
    if arg.starts_with("--") {
        let (prefix, explicit) = match arg.split_once('=') {
            Some((p, e)) => (p, Some(e.to_string())),
            None => (arg, None),
        };
        for (i, (o, _)) in OPTIONS.iter().enumerate() {
            if o.starts_with(prefix) {
                matches.push((i, explicit.clone()));
            }
        }
    } else {
        let short: String = arg.chars().take(2).collect();
        let short_explicit: String = arg.chars().skip(2).collect();
        for (i, (o, _)) in OPTIONS.iter().enumerate() {
            if *o == short {
                matches.push((i, Some(short_explicit.clone())));
            } else if o.starts_with(arg) {
                matches.push((i, None));
            }
        }
    }
    if matches.len() > 1 {
        let names: Vec<&str> = matches.iter().map(|(i, _)| OPTIONS[*i].0).collect();
        return Err(format!("ambiguous option: {arg} could match {}", names.join(", ")));
    }
    if let Some((i, explicit)) = matches.pop() {
        return Ok(Class::Known(i, explicit));
    }
    if is_negative_number(arg) || arg.contains(' ') {
        return Ok(Class::Positional);
    }
    Ok(Class::Unknown)
}

fn py_repr_str(s: &str) -> String {
    // repr() of a str, enough for argparse messages.
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

fn parse_args(argv: &[String]) -> Parsed {
    let mut classes = Vec::with_capacity(argv.len());
    let mut after_dashes = false;
    for a in argv {
        if after_dashes {
            classes.push(Class::Positional);
            continue;
        }
        if a == "--" {
            after_dashes = true;
            classes.push(Class::Positional);
            continue;
        }
        match classify(a) {
            Ok(c) => classes.push(c),
            Err(e) => return Parsed::Error(e),
        }
    }
    let mut args = Args::default();
    let mut extras: Vec<String> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        match &classes[i] {
            Class::Positional => extras.push(argv[i].clone()),
            Class::Unknown => extras.push(argv[i].clone()),
            Class::Known(k, explicit) => {
                let (name, kind) = OPTIONS[*k];
                if kind == Kind::Help {
                    return Parsed::Help;
                }
                let value = if kind == Kind::Flag {
                    if let Some(e) = explicit {
                        return Parsed::Error(format!("argument {name}: ignored explicit argument {}", py_repr_str(e)));
                    }
                    None
                } else if let Some(e) = explicit {
                    Some(e.clone())
                } else if i + 1 < argv.len() && matches!(classes[i + 1], Class::Positional) && argv[i + 1] != "--" {
                    i += 1;
                    Some(argv[i].clone())
                } else {
                    return Parsed::Error(format!("argument {name}: expected one argument"));
                };
                match kind {
                    Kind::Int => {
                        let v = value.unwrap_or_default();
                        match py::parse_int(&v) {
                            Some(n) => args.size = n,
                            None if v.trim().trim_start_matches(['+', '-']).chars().all(|c| c.is_ascii_digit() || c == '_')
                                && v.trim().chars().any(|c| c.is_ascii_digit()) =>
                            {
                                return Parsed::Unsupported;
                            }
                            None => return Parsed::Error(format!("argument {name}: invalid int value: {}", py_repr_str(&v))),
                        }
                    }
                    Kind::Float => {
                        let v = value.unwrap_or_default();
                        let Some(f) = py::parse_float(&v) else {
                            return Parsed::Error(format!("argument {name}: invalid float value: {}", py_repr_str(&v)));
                        };
                        match name {
                            "--harmony" => args.harmony = f,
                            "--harmonize_threshold" => args.harmonize_threshold = Num::Float(f),
                            "--term_fg_boost" => args.term_fg_boost = f,
                            "--term_saturation" => args.term_saturation = f,
                            "--term_brightness" => args.term_brightness = f,
                            "--term_bg_brightness" => args.term_bg_brightness = f,
                            _ => args.color_strength = f,
                        }
                    }
                    Kind::Choice(choices) => {
                        let v = value.unwrap_or_default();
                        if !choices.contains(&v.as_str()) {
                            return Parsed::Error(format!(
                                "argument {name}: invalid choice: {} (choose from {})",
                                py_repr_str(&v),
                                choices.join(", ")
                            ));
                        }
                        if name == "--mode" {
                            args.mode = v;
                        } else {
                            args.transparency = v;
                        }
                    }
                    Kind::Flag => match name {
                        "--smart" => args.smart = true,
                        "--blend_bg_fg" => args.blend_bg_fg = true,
                        "--soften" => args.soften = true,
                        "--debug" => args.debug = true,
                        _ => args.invert_hue = true,
                    },
                    Kind::Str => {
                        let v = value;
                        match name {
                            "--path" => args.path = v,
                            "--color" => args.color = v,
                            "--scheme" => args.scheme = v.unwrap_or_default(),
                            "--termscheme" => args.termscheme = v,
                            "--cache" => args.cache = v,
                            "--json-output" => args.json_output = v,
                            "--palette-output" => args.palette_output = v,
                            "--app-palette-output" => args.app_palette_output = v,
                            "--terminal-output" => args.terminal_output = v,
                            "--meta-output" => args.meta_output = v,
                            "--scss-output" => args.scss_output = v,
                            "--surface-seed" => args.surface_seed = v,
                            "--accent-seed" => args.accent_seed = v,
                            "--washi-roles" => args.washi_roles = v,
                            _ => args.render_templates = v,
                        }
                    }
                    Kind::Help => unreachable!(),
                }
            }
        }
        i += 1;
    }
    if !extras.is_empty() {
        return Parsed::Error(format!("unrecognized arguments: {}", extras.join(" ")));
    }
    Parsed::Args(Box::new(args))
}

// ---------------------------------------------------------------- Python's little pieces

/// A dict of str to str in insertion order.
#[derive(Clone, Default)]
struct Dict(Vec<(String, String)>);

impl Dict {
    fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }
    fn get_or<'a>(&'a self, k: &str, default: &'a str) -> &'a str {
        self.get(k).unwrap_or(default)
    }
    /// `d.get(k) or default`: an empty string falls through.
    fn truthy<'a>(&'a self, k: &str) -> Option<&'a str> {
        self.get(k).filter(|v| !v.is_empty())
    }
    fn has(&self, k: &str) -> bool {
        self.get(k).is_some()
    }
    fn set(&mut self, k: &str, v: String) {
        match self.0.iter_mut().find(|(key, _)| key == k) {
            Some(e) => e.1 = v,
            None => self.0.push((k.to_string(), v)),
        }
    }
    fn update(&mut self, other: &Dict) {
        for (k, v) in &other.0 {
            self.set(k, v.clone());
        }
    }
    fn json(&self) -> Json {
        Json::Obj(self.0.iter().map(|(k, v)| (k.clone(), Json::Str(v.clone()))).collect())
    }
}

/// Python's two-argument `min` and `max`: the first argument wins a tie (and a NaN on the right loses).
fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

fn py_round(x: f64) -> f64 {
    x.round_ties_even()
}

/// The failure a Python statement raises: the traceback's last line on stderr, exit 1.
struct Raise;

fn raise(kind: &str, message: &str) -> Raise {
    eprintln!("Traceback (most recent call last):\n  File \"{PROG}\", in <module>\n{kind}: {message}");
    Raise
}

fn os_error(e: &std::io::Error, path: &str) -> Raise {
    let kind = match e.kind() {
        std::io::ErrorKind::NotFound => "FileNotFoundError",
        std::io::ErrorKind::PermissionDenied => "PermissionError",
        std::io::ErrorKind::IsADirectory => "IsADirectoryError",
        std::io::ErrorKind::AlreadyExists => "FileExistsError",
        std::io::ErrorKind::NotADirectory => "NotADirectoryError",
        _ => "OSError",
    };
    let errno = e.raw_os_error().unwrap_or(0);
    let text = std::io::Error::from_raw_os_error(errno).to_string();
    let text = text.split(" (os error").next().unwrap_or(&text).to_string();
    raise(kind, &format!("[Errno {errno}] {text}: {}", py_repr_str(path)))
}

fn rgb_hex(argb: u32) -> String {
    format!("#{:02X}{:02X}{:02X}", (argb >> 16) & 0xFF, (argb >> 8) & 0xFF, argb & 0xFF)
}

/// `hex_to_argb`: three `int(slice, 16)` on code-point slices, `& 255`.
fn hex_to_argb(code: &str) -> Result<u32, Raise> {
    let chars: Vec<char> = code.chars().collect();
    let slice = |a: usize, b: usize| -> String { chars.iter().skip(a).take(b.saturating_sub(a)).collect() };
    let byte = |s: String| py::parse_hex_byte(&s).ok_or_else(|| raise("ValueError", &format!("invalid literal for int() with base 16: {}", py_repr_str(&s))));
    let r = byte(slice(1, 3))?;
    let g = byte(slice(3, 5))?;
    let b = byte(slice(5, chars.len().max(5)))?;
    Ok(0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32)
}

fn hct_of(code: &str) -> Result<Hct, Raise> {
    Ok(Hct::from_int(hex_to_argb(code)?))
}

/// `re.fullmatch(r"#[0-9A-Fa-f]{6}", s)`, or with `#?` when `hash_optional`.
fn is_hex6(s: &str, hash_optional: bool) -> bool {
    let body = match s.strip_prefix('#') {
        Some(b) => b,
        None if hash_optional => s,
        None => return false,
    };
    body.len() == 6 && body.bytes().all(|c| c.is_ascii_hexdigit())
}

/// A seed flag as the script reads it: stripped, `#?` and six hex digits.
fn seed_flag(v: &Option<String>) -> Option<String> {
    let s = v.as_deref()?;
    if s.is_empty() {
        return None;
    }
    let t = py::strip(s);
    is_hex6(t, true).then(|| t.trim_start_matches('#').to_string())
}

/// `open(path).read()`: UTF-8 with universal newlines.
fn read_text(path: &str) -> Result<Result<String, ()>, std::io::Error> {
    let bytes = std::fs::read(path)?;
    Ok(String::from_utf8(bytes).map(|t| t.replace("\r\n", "\n").replace('\r', "\n")).map_err(|_| ()))
}

fn write_file(path: &str, content: &str) -> Result<(), Raise> {
    std::fs::write(path, content).map_err(|e| os_error(&e, path))
}

// ---------------------------------------------------------------- the script's colour helpers

fn harmonize(design: u32, source: u32, threshold: f64, harmony: f64) -> u32 {
    let from = Hct::from_int(design);
    let to = Hct::from_int(source);
    let difference = difference_degrees(from.hue, to.hue);
    let rotation = py_min(difference * harmony, threshold);
    let hue = sanitize_degrees_double(from.hue + rotation * rotation_direction(from.hue, to.hue));
    Hct::from_hct(hue, from.chroma, from.tone).argb
}

fn boost_chroma_tone(argb: u32, chroma: f64, tone: f64, tone_cap: f64) -> u32 {
    let h = Hct::from_int(argb);
    let new_tone = py_min(tone_cap, h.tone * tone);
    Hct::from_hct(h.hue, h.chroma * chroma, new_tone).argb
}

fn ensure_min_chroma(argb: u32, min_chroma: f64) -> u32 {
    let h = Hct::from_int(argb);
    if h.chroma < min_chroma {
        return Hct::from_hct(h.hue, min_chroma, h.tone).argb;
    }
    argb
}

unsafe extern "C" {
    /// libm's `pow`, which Python's `**` on floats calls.
    safe fn pow(x: f64, y: f64) -> f64;
}

fn relative_luminance(argb: u32) -> f64 {
    let lin = |c: f64| if c <= 0.03928 { c / 12.92 } else { pow((c + 0.055) / 1.055, 2.4) };
    let r = ((argb >> 16) & 0xFF) as f64 / 255.0;
    let g = ((argb >> 8) & 0xFF) as f64 / 255.0;
    let b = (argb & 0xFF) as f64 / 255.0;
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn contrast_ratio(fg: u32, bg: u32) -> f64 {
    let l1 = relative_luminance(fg);
    let l2 = relative_luminance(bg);
    let lighter = py_max(l1, l2);
    let darker = py_min(l1, l2);
    (lighter + 0.05) / (darker + 0.05)
}

fn find_tone_for_contrast(hue: f64, chroma: f64, start_tone: f64, limit_tone: f64, bg: u32, min_ratio: f64, is_dark: bool) -> (u32, f64, bool, f64) {
    let step = 0.25;
    let direction = if is_dark { 1.0 } else { -1.0 };
    let mut tone = start_tone;
    let max_steps = 1i64.max(((limit_tone - start_tone).abs() / step).ceil() as i64 + 2);
    let initial = Hct::from_hct(hue, chroma, py_max(0.0, py_min(100.0, start_tone))).argb;
    let (mut best_color, mut best_tone, mut best_ratio) = (initial, start_tone, contrast_ratio(initial, bg));
    for _ in 0..max_steps {
        let clamped = py_max(0.0, py_min(100.0, tone));
        let candidate = Hct::from_hct(hue, chroma, clamped).argb;
        let ratio = contrast_ratio(candidate, bg);
        if ratio > best_ratio {
            best_color = candidate;
            best_tone = clamped;
            best_ratio = ratio;
        }
        if ratio >= min_ratio {
            return (candidate, clamped, true, ratio);
        }
        if is_dark && clamped >= limit_tone {
            break;
        }
        if !is_dark && clamped <= limit_tone {
            break;
        }
        tone += direction * step;
        if is_dark && tone > limit_tone {
            tone = limit_tone;
        }
        if !is_dark && tone < limit_tone {
            tone = limit_tone;
        }
    }
    (best_color, best_tone, false, best_ratio)
}

fn ensure_contrast(fg: u32, bg: u32, min_ratio: f64, is_dark: bool, tone_limit: Option<f64>) -> u32 {
    if contrast_ratio(fg, bg) >= min_ratio {
        return fg;
    }
    let h = Hct::from_int(fg);
    let (original_tone, original_chroma) = (h.tone, h.chroma);
    let tone_limit = tone_limit.unwrap_or(if is_dark { 88.0 } else { 20.0 });
    let (best, best_tone, met_min, best_ratio) = find_tone_for_contrast(h.hue, h.chroma, original_tone, tone_limit, bg, min_ratio, is_dark);
    let tone_shift = (best_tone - original_tone).abs();
    if tone_shift > 10.0 {
        let boost_factor = 1.0 + py_min(0.4, (tone_shift - 10.0) / 50.0);
        let boosted_chroma = py_min(original_chroma * boost_factor, 80.0);
        let boosted_same_tone = Hct::from_hct(h.hue, boosted_chroma, best_tone).argb;
        let boosted_ratio = contrast_ratio(boosted_same_tone, bg);
        if met_min && boosted_ratio >= min_ratio {
            return boosted_same_tone;
        }
        let (boosted_best, _, boosted_met, boosted_best_ratio) =
            find_tone_for_contrast(h.hue, boosted_chroma, best_tone, tone_limit, bg, min_ratio, is_dark);
        if met_min {
            return if boosted_met { boosted_best } else { best };
        }
        if boosted_best_ratio > best_ratio {
            return boosted_best;
        }
    }
    best
}

fn apca_lc(text: u32, bg: u32) -> f64 {
    let ys = |argb: u32| -> f64 {
        let r = ((argb >> 16) & 0xFF) as f64 / 255.0;
        let g = ((argb >> 8) & 0xFF) as f64 / 255.0;
        let b = (argb & 0xFF) as f64 / 255.0;
        let y = 0.2126729 * pow(r, 2.4) + 0.7151522 * pow(g, 2.4) + 0.0721750 * pow(b, 2.4);
        if y < 0.022 { y + pow(0.022 - y, 1.414) } else { y }
    };
    let (yt, yb) = (ys(text), ys(bg));
    if yb > yt {
        let s = (pow(yb, 0.56) - pow(yt, 0.57)) * 1.14;
        return if s < 0.1 { 0.0 } else { (s - 0.027) * 100.0 };
    }
    let s = (pow(yb, 0.65) - pow(yt, 0.62)) * 1.14;
    if s > -0.1 { 0.0 } else { -(s + 0.027) * 100.0 }
}

fn tone_for_lc(hue: f64, chroma: f64, bg: u32, lc: f64) -> f64 {
    let (mut lo, mut hi) = (Hct::from_int(bg).tone, 100.0);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if apca_lc(Hct::from_hct(hue, chroma, mid).argb, bg) >= lc {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

fn solve_dark_terminal_reading(term: &mut Dict, fg_boost: f64) -> Result<(), Raise> {
    let bg = hct_of(term.get_or("term0", ""))?;
    let bg_tone = py_max(6.0, py_min(16.0, bg.tone));
    let bg_argb = Hct::from_hct(bg.hue, py_min(bg.chroma, 8.0), bg_tone).argb;
    term.set("term0", rgb_hex(bg_argb));
    let ink_chroma = py_min(Hct::from_int(bg_argb).chroma, 6.0);
    let boost = py_max(-7.0, py_min(13.0, (fg_boost - 0.35) * 20.0));
    for (key, lc, floor) in [("term15", 70.0, 7.0), ("term7", 56.0, 4.5), ("term8", 42.0, 3.5)] {
        let tone = tone_for_lc(bg.hue, ink_chroma, bg_argb, lc + boost * if key == "term15" { 1.0 } else { 0.5 });
        let ink = Hct::from_hct(bg.hue, ink_chroma, tone).argb;
        term.set(key, rgb_hex(ensure_contrast(ink, bg_argb, floor, true, None)));
    }
    let fg_tone = hct_of(term.get_or("term15", ""))?.tone;
    let mut chromas: Vec<(usize, f64)> = Vec::new();
    for i in (1..7).chain(9..15) {
        if let Some(v) = term.get(&format!("term{i}")) {
            chromas.push((i, hct_of(v)?.chroma));
        }
    }
    let level = |range: std::ops::Range<usize>, default: f64| -> f64 {
        let mut v: Vec<f64> = range.map(|i| chromas.iter().find(|(k, _)| *k == i).map(|(_, c)| *c).unwrap_or(default)).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v[3]
    };
    let normal = py_max(34.0, py_min(44.0, level(1..7, 40.0)));
    let bright = py_max(30.0, py_min(38.0, level(9..15, 34.0)));
    for i in (1..7).chain(9..15) {
        let key = format!("term{i}");
        let Some(v) = term.get(&key) else { continue };
        let c = hct_of(v)?;
        let cap = if i >= 9 { fg_tone } else { fg_tone - 5.0 };
        let chroma = if i >= 9 { bright } else { normal };
        let evened = Hct::from_hct(c.hue, chroma, py_min(c.tone, cap)).argb;
        term.set(&key, rgb_hex(ensure_contrast(evened, bg_argb, if i >= 9 { 3.5 } else { 4.5 }, true, None)));
    }
    Ok(())
}

fn mix_hex(a: &str, b: &str, keep_a: f64) -> Result<String, Raise> {
    let a = a.trim_start_matches('#');
    let b = b.trim_start_matches('#');
    let part = |s: &str, i: usize| -> Result<f64, Raise> {
        let t: String = s.chars().skip(i).take(2).collect();
        py::parse_hex_byte(&t).map(|v| v as f64).ok_or_else(|| raise("ValueError", "invalid literal for int() with base 16"))
    };
    let mut out = String::from("#");
    for i in [0, 2, 4] {
        let v = py_round(part(a, i)? * keep_a + part(b, i)? * (1.0 - keep_a));
        out.push_str(&format!("{:02X}", v as i64));
    }
    Ok(out)
}

fn readable_hex(fg: &str, bg: &str, min_ratio: f64) -> Result<String, Raise> {
    let bg_argb = hex_to_argb(bg)?;
    let is_bg_dark = Hct::from_int(bg_argb).tone < 50.0;
    Ok(rgb_hex(ensure_contrast(hex_to_argb(fg)?, bg_argb, min_ratio, is_bg_dark, None)))
}

fn build_app_palette(base: &Dict) -> Result<Dict, Raise> {
    let layer0 = base.truthy("background").or(base.truthy("surface")).unwrap_or("#000000").to_string();
    let layer1 = base.truthy("surface_container_low").or(base.truthy("surface")).unwrap_or(&layer0).to_string();
    let layer2 = base.truthy("surface_container").unwrap_or(&layer1).to_string();
    let layer3 = base.truthy("surface_container_high").unwrap_or(&layer2).to_string();
    let layer4 = base.truthy("surface_container_highest").unwrap_or(&layer3).to_string();
    let on_surface = base.truthy("on_surface").or(base.truthy("on_background")).unwrap_or("#FFFFFF").to_string();
    let on_surface_variant = base.truthy("on_surface_variant").unwrap_or(&on_surface).to_string();
    let primary = base.truthy("primary").unwrap_or("#6750A4").to_string();
    let primary_container = base.truthy("primary_container").unwrap_or(&primary).to_string();
    let on_primary = match base.truthy("on_primary") {
        Some(v) => v.to_string(),
        None => readable_hex(&on_surface, &primary, 4.5)?,
    };
    let outline = base.truthy("outline").unwrap_or(&on_surface_variant).to_string();
    let outline_variant = match base.truthy("outline_variant") {
        Some(v) => v.to_string(),
        None => mix_hex(&layer1, &outline, 0.72)?,
    };
    let on_layer0 = readable_hex(&on_surface, &layer0, 4.5)?;
    let on_layer1 = readable_hex(&on_surface, &layer1, 4.5)?;
    let on_layer2 = readable_hex(&on_surface, &layer2, 4.5)?;
    let on_layer3 = readable_hex(&on_surface, &layer3, 4.5)?;
    let on_layer4 = readable_hex(&on_surface, &layer4, 4.5)?;
    let layer1_hover = mix_hex(&layer1, &on_layer1, 0.92)?;
    let layer1_active = mix_hex(&layer1, &on_layer1, 0.85)?;
    let subtext = if hct_of(&layer0)?.tone >= 50.0 {
        readable_hex(&mix_hex(&on_layer1, &layer1, 0.75)?, &layer1_active, 4.5)?
    } else {
        readable_hex(&mix_hex(&on_layer1, &layer1, 0.75)?, &layer1, 3.0)?
    };
    let layer2_hover = mix_hex(&layer2, &on_layer2, 0.90)?;
    let layer2_active = mix_hex(&layer2, &on_layer2, 0.80)?;
    let layer3_hover = mix_hex(&layer3, &on_layer3, 0.90)?;
    let layer3_active = mix_hex(&layer3, &on_layer3, 0.80)?;
    let container = hct_of(&primary_container)?;
    let layer0_tone = hct_of(&layer0)?.tone;
    let selection_tone =
        if layer0_tone < 50.0 { py_max(container.tone, layer0_tone + 12.0) } else { py_min(container.tone, layer0_tone - 10.0) };
    let selection = rgb_hex(Hct::from_hct(container.hue, py_min(container.chroma, 20.0), selection_tone).argb);
    let selection_hover = rgb_hex(Hct::from_hct(container.hue, py_min(container.chroma, 26.0), selection_tone).argb);
    let on_selection = readable_hex(&on_layer3, &selection, 4.5)?;
    let is_dark = hct_of(&layer0)?.tone < 50.0;
    let accent_hue = hct_of(&primary)?.hue;
    let layer0_argb = hex_to_argb(&layer0)?;
    let status = |hue: f64, chroma: f64| -> String {
        let delta = py::fmod(accent_hue - hue + 180.0, 360.0) - 180.0;
        let hue = py::fmod(hue + py_max(-15.0, py_min(15.0, delta * 0.25)), 360.0);
        let seed = Hct::from_hct(hue, chroma, if is_dark { 80.0 } else { 40.0 }).argb;
        rgb_hex(ensure_contrast(seed, layer0_argb, 4.5, is_dark, None))
    };
    let success = status(145.0, 48.0);
    let warning = status(75.0, 56.0);
    let error = match base.truthy("error") {
        Some(e) => readable_hex(e, &layer0, 4.5)?,
        None => readable_hex(&status(25.0, 60.0), &layer0, 4.5)?,
    };
    let mut app = base.clone();
    for (k, v) in [
        ("background", &layer0),
        ("on_background", &on_layer0),
        ("surface", &layer0),
        ("on_surface", &on_layer1),
        ("surface_dim", &layer0),
        ("surface_bright", &layer3),
        ("surface_container_lowest", &layer0),
        ("surface_container_low", &layer1),
        ("surface_container", &layer2),
        ("surface_container_high", &layer3),
        ("surface_container_highest", &layer4),
        ("outline", &outline),
        ("outline_variant", &outline_variant),
        ("app_background", &layer0),
        ("app_foreground", &on_layer0),
        ("app_subtext", &subtext),
        ("app_surface", &layer1),
        ("app_surface_hover", &layer1_hover),
        ("app_surface_active", &layer1_active),
        ("app_surface_elevated", &layer2),
        ("app_surface_elevated_hover", &layer2_hover),
        ("app_surface_elevated_active", &layer2_active),
        ("app_surface_popup", &layer3),
        ("app_surface_popup_hover", &layer3_hover),
        ("app_surface_popup_active", &layer3_active),
        ("app_on_surface", &on_layer1),
        ("app_on_surface_elevated", &on_layer2),
        ("app_on_surface_popup", &on_layer3),
        ("app_on_surface_highest", &on_layer4),
        ("app_border", &outline),
        ("app_border_subtle", &outline_variant),
        ("app_accent", &primary),
        ("app_on_accent", &on_primary),
        ("app_accent_container", &primary_container),
        ("app_selection", &selection),
        ("app_selection_hover", &selection_hover),
        ("app_on_selection", &on_selection),
        ("app_success", &success),
        ("app_warning", &warning),
        ("app_error", &error),
        ("app_on_status", &layer0),
        ("app_window_bg", &layer0),
        ("app_view_bg", &layer0),
        ("app_headerbar_bg", &layer0),
        ("app_sidebar_bg", &layer0),
        ("app_card_bg", &layer1),
        ("app_popover_bg", &layer2),
        ("app_dialog_bg", &layer3),
        ("app_thumbnail_bg", &layer4),
    ] {
        app.set(k, v.clone());
    }
    Ok(app)
}

const PALETTE_KEYS: &[(&str, &str)] = &[
    ("primary", "primary"),
    ("on_primary", "onPrimary"),
    ("primary_container", "primaryContainer"),
    ("on_primary_container", "onPrimaryContainer"),
    ("primary_fixed", "primaryFixed"),
    ("primary_fixed_dim", "primaryFixedDim"),
    ("on_primary_fixed", "onPrimaryFixed"),
    ("on_primary_fixed_variant", "onPrimaryFixedVariant"),
    ("secondary", "secondary"),
    ("on_secondary", "onSecondary"),
    ("secondary_container", "secondaryContainer"),
    ("on_secondary_container", "onSecondaryContainer"),
    ("secondary_fixed", "secondaryFixed"),
    ("secondary_fixed_dim", "secondaryFixedDim"),
    ("on_secondary_fixed", "onSecondaryFixed"),
    ("on_secondary_fixed_variant", "onSecondaryFixedVariant"),
    ("tertiary", "tertiary"),
    ("on_tertiary", "onTertiary"),
    ("tertiary_container", "tertiaryContainer"),
    ("on_tertiary_container", "onTertiaryContainer"),
    ("tertiary_fixed", "tertiaryFixed"),
    ("tertiary_fixed_dim", "tertiaryFixedDim"),
    ("on_tertiary_fixed", "onTertiaryFixed"),
    ("on_tertiary_fixed_variant", "onTertiaryFixedVariant"),
    ("error", "error"),
    ("on_error", "onError"),
    ("error_container", "errorContainer"),
    ("on_error_container", "onErrorContainer"),
    ("background", "background"),
    ("on_background", "onBackground"),
    ("surface", "surface"),
    ("on_surface", "onSurface"),
    ("surface_dim", "surfaceDim"),
    ("surface_bright", "surfaceBright"),
    ("surface_variant", "surfaceVariant"),
    ("on_surface_variant", "onSurfaceVariant"),
    ("surface_container_lowest", "surfaceContainerLowest"),
    ("surface_container_low", "surfaceContainerLow"),
    ("surface_container", "surfaceContainer"),
    ("surface_container_high", "surfaceContainerHigh"),
    ("surface_container_highest", "surfaceContainerHighest"),
    ("outline", "outline"),
    ("outline_variant", "outlineVariant"),
    ("inverse_surface", "inverseSurface"),
    ("inverse_on_surface", "inverseOnSurface"),
    ("inverse_primary", "inversePrimary"),
    ("shadow", "shadow"),
    ("scrim", "scrim"),
    ("surface_tint", "surfaceTint"),
    ("success", "success"),
    ("on_success", "onSuccess"),
    ("success_container", "successContainer"),
    ("on_success_container", "onSuccessContainer"),
];

const RAW_CONTRACT: &[(&str, &str)] = &[
    ("primary", "primary"),
    ("on_primary", "onPrimary"),
    ("primary_container", "primaryContainer"),
    ("on_primary_container", "onPrimaryContainer"),
    ("background", "background"),
    ("on_background", "onBackground"),
    ("surface", "surface"),
    ("on_surface", "onSurface"),
    ("surface_dim", "surfaceDim"),
    ("surface_bright", "surfaceBright"),
    ("surface_container_lowest", "surfaceContainerLowest"),
    ("surface_container_low", "surfaceContainerLow"),
    ("surface_container", "surfaceContainer"),
    ("surface_container_high", "surfaceContainerHigh"),
    ("surface_container_highest", "surfaceContainerHighest"),
    ("on_surface_variant", "onSurfaceVariant"),
    ("outline", "outline"),
    ("outline_variant", "outlineVariant"),
];

fn success_colours(d: &mut Dict, dark: bool) {
    let v = if dark {
        ["#B5CCBA", "#213528", "#374B3E", "#D1E9D6"]
    } else {
        ["#4F6354", "#FFFFFF", "#D1E8D5", "#0C1F13"]
    };
    for (k, v) in ["success", "onSuccess", "successContainer", "onSuccessContainer"].iter().zip(v) {
        d.set(k, v.into());
    }
}

fn softens(scheme_name: &str) -> bool {
    !matches!(scheme_name, "scheme-tonal-spot" | "scheme-neutral" | "scheme-monochrome")
}

// ---------------------------------------------------------------- templates

/// `re.sub(r"\{\{\s*(.*?)\s*\}\}", ...)`: Unicode `\s`, `.` stops at a newline, the group as short as it can be.
fn substitute(content: &str, mut resolve: impl FnMut(&str, &str) -> String) -> String {
    let c: Vec<char> = content.chars().collect();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    let ws = |ch: char| py::is_space(ch);
    'outer: while i < c.len() {
        if c[i] == '{' && i + 1 < c.len() && c[i + 1] == '{' {
            // `\s*` is greedy but gives back: try every split of the leading whitespace, longest first.
            let mut k_max = i + 2;
            while k_max < c.len() && ws(c[k_max]) {
                k_max += 1;
            }
            let mut k = k_max;
            loop {
                let mut e = k;
                loop {
                    let mut m = e;
                    while m < c.len() && ws(c[m]) {
                        m += 1;
                    }
                    if m + 1 < c.len() && c[m] == '}' && c[m + 1] == '}' {
                        let whole: String = c[i..m + 2].iter().collect();
                        let group: String = c[k..e].iter().collect();
                        out.push_str(&resolve(&whole, &group));
                        i = m + 2;
                        continue 'outer;
                    }
                    if e >= c.len() || c[e] == '\n' {
                        break;
                    }
                    e += 1;
                }
                if k == i + 2 {
                    break;
                }
                k -= 1;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

fn camel_to_snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() && i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit()) {
            out.push('_');
        }
        out.push(ch);
    }
    out.to_lowercase()
}

struct HexVal {
    hex: String,
    stripped: String,
    rgb: String,
}

fn hex_val(v: &str) -> Result<HexVal, Raise> {
    let stripped = v.trim_start_matches('#').to_string();
    let chars: Vec<char> = stripped.chars().collect();
    let mut parts = Vec::new();
    for i in [0, 2, 4] {
        let s: String = chars.iter().skip(i).take(2).collect();
        let n = i64::from_str_radix(py::strip(&s), 16).map_err(|_| raise("ValueError", "invalid literal for int() with base 16"))?;
        parts.push(n.to_string());
    }
    Ok(HexVal { hex: v.to_string(), stripped, rgb: parts.join(", ") })
}

// ---------------------------------------------------------------- the run

pub fn run(argv: &[String], ctx: &Context) -> Run {
    let args = match parse_args(argv) {
        Parsed::Args(a) => *a,
        Parsed::Error(e) => {
            eprint!("{USAGE}{PROG}: error: {e}\n");
            return Run::Exit(2);
        }
        Parsed::Help | Parsed::Unsupported => return Run::Delegate,
    };
    if args.debug {
        return Run::Delegate;
    }
    // Templates from a legacy config.toml go through tomllib: the original's.
    if let Some(dir) = &args.render_templates {
        if !Path::new(&py::join(dir, "templates.json")).is_file() && Path::new(&py::join(dir, "config.toml")).is_file() {
            return Run::Delegate;
        }
    }
    let image = match &args.path {
        Some(p) => match pixels::open(p) {
            pixels::Load::Ok(im) => Some(im),
            pixels::Load::Io(e) => {
                os_error(&e, p);
                return Run::Exit(1);
            }
            pixels::Load::Delegate => return Run::Delegate,
        },
        None => None,
    };
    // Without a source the original only runs when an accent seed defines the colour (a NameError otherwise).
    if image.is_none() && args.color.is_none() && seed_flag(&args.accent_seed).is_none() {
        return Run::Delegate;
    }
    match flow(args, image, ctx) {
        Ok(code) => Run::Exit(code),
        Err(Raise) => Run::Exit(1),
    }
}

fn flow(mut args: Args, image: Option<pixels::Image>, ctx: &Context) -> Result<i32, Raise> {
    let darkmode = args.mode == "dark";
    let transparent = args.transparency == "transparent";
    let mut argb: Option<u32> = None;
    let mut hct: Option<Hct> = None;

    if let Some(mut im) = image {
        if im.warns_palette_transparency {
            eprint!(
                "{}/PIL/Image.py:1136: UserWarning: Palette images with Transparency expressed in bytes should be converted to RGBA images\n  warnings.warn(\n",
                ctx.site_packages
            );
        }
        let (nw, nh) = pixels::optimal_size(im.width, im.height, args.size);
        if nw < im.width || nh < im.height {
            im = pixels::resize(&im, nw, nh);
        }
        if args.scheme == "auto" {
            args.scheme = pixels::auto_scheme(&im).to_string();
        }
        let px: Vec<[u8; 3]> = im.data.chunks_exact(im.channels).map(|p| [p[0], p[1], p[2]]).collect();
        let colours = quantize::quantize_celebi(&px, 128);
        let seed = quantize::score_first(&colours);
        if let Some(cache) = &args.cache {
            write_file(cache, &rgb_hex(seed))?;
        }
        let h = Hct::from_int(seed);
        if args.smart && h.chroma < 20.0 {
            args.scheme = "scheme-neutral".into();
        }
        argb = Some(seed);
        hct = Some(h);
    } else if let Some(color) = &args.color {
        let a = hex_to_argb(color)?;
        argb = Some(a);
        hct = Some(Hct::from_int(a));
    }
    if let Some(seed) = seed_flag(&args.accent_seed) {
        let a = hex_to_argb(&format!("#{seed}"))?;
        argb = Some(a);
        hct = Some(Hct::from_int(a));
    }
    let (Some(argb), Some(mut hct)) = (argb, hct) else { return Err(raise("NameError", "name 'hct' is not defined")) };
    if args.invert_hue {
        hct = Hct::from_hct(py::fmod(hct.hue + 180.0, 360.0), hct.chroma, hct.tone);
    }

    let variant = Variant::from_scheme_name(&args.scheme);
    let s = Scheme::new(hct, variant, darkmode, 0.0);
    let soften = args.soften && softens(&args.scheme);
    let strength_k = args.color_strength;
    let tamed = |mut g: Hct| {
        if soften {
            g = Hct::from_hct(g.hue, g.chroma * 0.60, g.tone);
        }
        if (strength_k - 1.0).abs() > 1e-6 && g.chroma > 2.0 {
            g = Hct::from_hct(g.hue, g.chroma * strength_k, g.tone);
        }
        g
    };
    let mut material = Dict::default();
    for &role in ROLES {
        material.set(role.name(), rgb_hex(tamed(scheme::get_hct(role, &s)).argb));
    }

    if args.scheme != "scheme-monochrome" {
        for (key, palette) in [("primary", &s.primary), ("secondary", &s.secondary), ("tertiary", &s.tertiary)] {
            if !material.has(key) {
                continue;
            }
            let tone = hct_of(material.get_or(key, ""))?.tone;
            if darkmode && !(70.0..=85.0).contains(&tone) {
                material.set(key, rgb_hex(tamed(palette.get_hct(80.0)).argb));
            } else if !darkmode && !(25.0..=50.0).contains(&tone) {
                material.set(key, rgb_hex(tamed(palette.get_hct(40.0)).argb));
            }
            let container = format!("{key}Container");
            let mut up = key.chars();
            let on_container = format!("on{}{}Container", up.next().map(|c| c.to_ascii_uppercase()).unwrap_or_default(), up.as_str());
            if material.has(&container) {
                let ctone = hct_of(material.get_or(&container, ""))?.tone;
                if darkmode && !(20.0..=40.0).contains(&ctone) {
                    material.set(&container, rgb_hex(tamed(palette.get_hct(30.0)).argb));
                    material.set(&on_container, rgb_hex(tamed(palette.get_hct(90.0)).argb));
                } else if !darkmode && !(80.0..=95.0).contains(&ctone) {
                    material.set(&container, rgb_hex(tamed(palette.get_hct(90.0)).argb));
                    material.set(&on_container, rgb_hex(tamed(palette.get_hct(10.0)).argb));
                }
            }
            let strength = py_max(0.3, args.color_strength);
            let ceiling = py_min(90.0, 60.0 * py_max(1.0, strength));
            let h = hct_of(material.get_or(key, ""))?;
            if h.chroma > ceiling {
                material.set(key, rgb_hex(Hct::from_hct(h.hue, ceiling, h.tone).argb));
            }
            if key == "primary" {
                let base_floors = if args.scheme == "scheme-neutral" { (16.0, 10.0) } else { (26.0, 18.0) };
                for (role, floor) in [(key.to_string(), base_floors.0 * strength), (container.clone(), base_floors.1 * strength)] {
                    if let Some(v) = material.get(&role) {
                        let h = hct_of(v)?;
                        if 4.0 <= h.chroma && h.chroma < floor {
                            material.set(&role, rgb_hex(Hct::from_hct(h.hue, floor, h.tone).argb));
                        }
                    }
                }
            }
        }
    }
    success_colours(&mut material, darkmode);

    const SURFACE_RAMP: [&str; 10] = [
        "background",
        "surface",
        "surfaceDim",
        "surfaceBright",
        "surfaceVariant",
        "surfaceContainerLowest",
        "surfaceContainerLow",
        "surfaceContainer",
        "surfaceContainerHigh",
        "surfaceContainerHighest",
    ];
    let seed_background = material.get_or("background", "").to_string();
    if let Some(seed) = seed_flag(&args.surface_seed) {
        let seed_hct = hct_of(&format!("#{seed}"))?;
        if (seed_hct.tone < 50.0) == darkmode && material.has("background") {
            let base_tone = hct_of(material.get_or("background", ""))?.tone;
            let seed_tone = if !darkmode { py_max(64.0, seed_hct.tone) } else { py_min(30.0, seed_hct.tone) };
            let seed_chroma = py_min(seed_hct.chroma, if darkmode { 10.0 } else { 8.0 });
            for key in SURFACE_RAMP {
                let Some(v) = material.get(key) else { continue };
                let tone = hct_of(v)?.tone;
                let shifted = py_max(0.0, py_min(100.0, seed_tone + (tone - base_tone)));
                material.set(key, rgb_hex(Hct::from_hct(seed_hct.hue, seed_chroma, shifted).argb));
            }
            let mut ramp = Vec::new();
            for key in SURFACE_RAMP {
                if let Some(v) = material.get(key) {
                    ramp.push(hex_to_argb(v)?);
                }
            }
            // min() and max() with a key keep the first of equals.
            let mut worst = ramp[0];
            for &a in &ramp[1..] {
                let (t, w) = (Hct::from_int(a).tone, Hct::from_int(worst).tone);
                if (!darkmode && t < w) || (darkmode && t > w) {
                    worst = a;
                }
            }
            let limit = Some(if darkmode { 94.0 } else { 8.0 });
            for (key, ratio) in [("onSurface", 7.0), ("onBackground", 7.0), ("onSurfaceVariant", 4.5), ("error", 4.5), ("outline", 3.0)] {
                if let Some(v) = material.get(key) {
                    let a = ensure_contrast(hex_to_argb(v)?, worst, ratio, darkmode, limit);
                    material.set(key, rgb_hex(a));
                }
            }
            let background = hex_to_argb(material.get("background").or(material.get("surface")).unwrap_or(""))?;
            for key in ["primary", "secondary", "tertiary"] {
                if let Some(v) = material.get(key) {
                    let a = ensure_contrast(hex_to_argb(v)?, worst, 3.0, darkmode, limit);
                    let a = ensure_contrast(a, background, 4.5, darkmode, limit);
                    material.set(key, rgb_hex(a));
                }
            }
        }
    }

    if let Some(roles_path) = args.washi_roles.clone() {
        write_iris_seeds(&roles_path, &material, &seed_background)?;
        apply_washi_roles(&roles_path, &mut material, darkmode)?;
    }

    let mut term = Dict::default();
    if let Some(termscheme) = &args.termscheme {
        terminal(&args, termscheme, &material, &mut term, darkmode)?;
    }
    if term.0.is_empty() && !material.0.is_empty() {
        for (k, role, default) in [
            ("term0", "surfaceVariant", "#282828"),
            ("term1", "error", "#CC241D"),
            ("term2", "secondary", "#98971A"),
            ("term3", "tertiary", "#D79921"),
            ("term4", "primary", "#458588"),
            ("term5", "tertiary", "#B16286"),
            ("term6", "secondary", "#689D6A"),
            ("term7", "onSurfaceVariant", "#A89984"),
            ("term8", "outline", "#928374"),
            ("term9", "error", "#FB4934"),
            ("term10", "secondary", "#B8BB26"),
            ("term11", "tertiary", "#FABD2F"),
            ("term12", "primary", "#83A598"),
            ("term13", "tertiary", "#D3869B"),
            ("term14", "secondary", "#8EC07C"),
            ("term15", "onSurface", "#EBDBB2"),
        ] {
            term.set(k, material.get_or(role, default).to_string());
        }
    }

    let py_bool = |b: bool| if b { "True" } else { "False" };
    let mut scss = format!("$darkmode: {};\n$transparent: {};", py_bool(darkmode), py_bool(transparent));
    for (k, v) in material.0.iter().chain(term.0.iter()) {
        scss.push_str(&format!("\n${k}: {v};"));
    }
    scss.push('\n');
    if let Some(p) = &args.scss_output {
        write_file(p, &scss)?;
    }
    {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(scss.as_bytes());
        let _ = out.flush();
    }

    let mut palette = Dict::default();
    for (k, src) in PALETTE_KEYS {
        palette.set(k, material.get_or(src, "").to_string());
    }
    if let Some(fill) = material.truthy("errorFill") {
        palette.set("error_fill", fill.to_string());
        palette.set("on_error_fill", material.get_or("onErrorFill", "").to_string());
    }
    let app_palette = build_app_palette(&palette)?;
    let mut colours = palette.clone();
    colours.update(&term);

    let seed_meta = |v: &Option<String>| seed_flag(v).map(|s| format!("#{}", s.to_lowercase())).unwrap_or_default();
    let meta = Json::Obj(vec![
        (
            "source".into(),
            Json::Str(if args.path.is_some() { "image" } else if args.color.is_some() { "color" } else { "unknown" }.into()),
        ),
        ("source_path".into(), args.path.clone().map(Json::Str).unwrap_or(Json::Null)),
        ("seed_color".into(), Json::Str(rgb_hex(argb))),
        ("mode".into(), Json::Str(if darkmode { "dark" } else { "light" }.into())),
        ("scheme".into(), Json::Str(args.scheme.clone())),
        ("transparent".into(), Json::Bool(transparent)),
        ("soften".into(), Json::Bool(args.soften)),
        ("term_harmony".into(), Json::Float(args.harmony)),
        ("term_saturation".into(), Json::Float(args.term_saturation)),
        ("term_brightness".into(), Json::Float(args.term_brightness)),
        ("term_bg_brightness".into(), Json::Float(args.term_bg_brightness)),
        ("term_fg_boost".into(), Json::Float(args.term_fg_boost)),
        ("harmonize_threshold".into(), args.harmonize_threshold.json()),
        ("color_strength".into(), Json::Float(args.color_strength)),
        ("blend_bg_fg".into(), Json::Bool(args.blend_bg_fg)),
        ("surface_seed".into(), Json::Str(seed_meta(&args.surface_seed))),
        ("accent_seed".into(), Json::Str(seed_meta(&args.accent_seed))),
        ("generated_by".into(), Json::Str("generate_colors_material.py".into())),
    ]);
    for (path, value) in [
        (&args.json_output, colours.json()),
        (&args.palette_output, palette.json()),
        (&args.app_palette_output, app_palette.json()),
        (&args.terminal_output, term.json()),
        (&args.meta_output, meta),
    ] {
        if let Some(p) = path.as_deref().filter(|p| !p.is_empty()) {
            write_file(p, &py::json_dump(&value))?;
        }
    }

    if let Some(dir) = args.render_templates.clone().filter(|d| !d.is_empty()) {
        return render_templates(&args, &dir, &s, hct, argb, darkmode, &material, &app_palette);
    }
    Ok(0)
}

fn write_iris_seeds(roles_path: &str, material: &Dict, seed_background: &str) -> Result<(), Raise> {
    let seeds_path = py::join(&py::dirname(roles_path), "iris-seeds.json");
    let seeds = [
        ("primary", material.get_or("primary", "").to_string()),
        ("secondary", material.get_or("secondary", "").to_string()),
        ("tertiary", material.get_or("tertiary", "").to_string()),
        ("background", seed_background.to_string()),
    ];
    let text = py::json_dumps_flat(&seeds);
    let unchanged = match read_text(&seeds_path) {
        Ok(Ok(t)) => t == text,
        // A UnicodeDecodeError is no OSError: the original stops there.
        Ok(Err(())) => return Err(raise("UnicodeDecodeError", "'utf-8' codec can't decode")),
        Err(_) => false,
    };
    if !unchanged {
        let tmp = format!("{seeds_path}.tmp");
        if std::fs::write(&tmp, &text).is_ok() {
            let _ = std::fs::rename(&tmp, &seeds_path);
        }
    }
    Ok(())
}

fn apply_washi_roles(roles_path: &str, material: &mut Dict, darkmode: bool) -> Result<(), Raise> {
    // OSError and ValueError (unreadable, not UTF-8, not JSON) are skipped; anything else raises.
    let Ok(Ok(text)) = read_text(roles_path) else { return Ok(()) };
    let Some(doc) = py::json_loads(&text) else { return Ok(()) };
    let Json::Obj(_) = doc else { return Err(raise("AttributeError", "object has no attribute 'get'")) };
    let roles = match doc.get("roles") {
        Some(r) if r.truthy() => r.clone(),
        _ => Json::Obj(Vec::new()),
    };
    let Json::Obj(entries) = &roles else { return Err(raise("AttributeError", "object has no attribute 'get'")) };
    let background = match roles.get("background") {
        None => String::new(),
        Some(Json::Str(s)) => s.clone(),
        Some(_) => return Err(raise("TypeError", "expected string or bytes-like object")),
    };
    if is_hex6(&background, false) && (hct_of(&background)?.tone < 50.0) == darkmode {
        for (k, v) in entries {
            if let Json::Str(sv) = v {
                if is_hex6(sv, false) {
                    material.set(k, sv.clone());
                }
            }
        }
    }
    Ok(())
}

fn terminal(args: &Args, termscheme: &str, material: &Dict, term: &mut Dict, darkmode: bool) -> Result<(), Raise> {
    let text = match read_text(termscheme) {
        Ok(Ok(t)) => t,
        Ok(Err(())) => return Err(raise("UnicodeDecodeError", "'utf-8' codec can't decode")),
        Err(e) => return Err(os_error(&e, termscheme)),
    };
    let doc = py::json_loads(&text).ok_or_else(|| raise("json.decoder.JSONDecodeError", "Expecting value"))?;
    let mode_key = if darkmode { "dark" } else { "light" };
    let Some(Json::Obj(source)) = doc.get(mode_key).cloned() else { return Err(raise("KeyError", &py_repr_str(mode_key))) };
    let primary_key = material
        .get("primary_paletteKeyColor")
        .or(material.get("primaryPaletteKeyColor"))
        .or(material.get("primary"))
        .unwrap_or("#6750A4")
        .to_string();
    let primary_argb = hex_to_argb(&primary_key)?;
    let levels = [
        ("background", 0.0),
        ("surfaceContainerLowest", 0.2),
        ("surfaceContainerLow", 0.4),
        ("surfaceContainer", 0.6),
        ("surfaceContainerHigh", 0.8),
        ("surfaceContainerHighest", 1.0),
    ];
    let surface = |brightness: f64| -> Result<String, Raise> {
        for (i, (name, level)) in levels.iter().enumerate() {
            if brightness <= *level || i == levels.len() - 1 {
                if i == 0 {
                    return Ok(material.get_or(name, "#1a1a1a").to_string());
                }
                let (prev_name, prev_level) = levels[i - 1];
                let t = if *level != prev_level { (brightness - prev_level) / (level - prev_level) } else { 0.0 };
                let c1 = hex_to_argb(material.get_or(prev_name, "#1a1a1a"))?;
                let c2 = hex_to_argb(material.get_or(name, "#2a2a2a"))?;
                let ch = |c: u32, s: u32| ((c >> s) & 0xFF) as f64;
                let mix = |s: u32| -> i64 { (ch(c1, s) + (ch(c2, s) - ch(c1, s)) * t) as i64 };
                return Ok(format!("#{}{}{}", py_hex2(mix(16)), py_hex2(mix(8)), py_hex2(mix(0))));
            }
        }
        Ok(material.get_or("surfaceContainerLow", "#1a1a1a").to_string())
    };
    let soften = args.soften && softens(&args.scheme);
    let sign = if darkmode { 1.0 } else { -1.0 };
    for (colour, val) in &source {
        if colour == "term0" {
            term.set(colour, surface(args.term_bg_brightness)?);
            continue;
        }
        if colour == "term15" {
            term.set(colour, material.get_or("onSurface", "#e0e0e0").to_string());
            continue;
        }
        if colour == "term8" {
            let v = match material.get("outline") {
                Some(o) => o.to_string(),
                None if darkmode => surface(py_min(1.0, args.term_bg_brightness + 0.45))?,
                None => surface(py_max(0.0, args.term_bg_brightness - 0.45))?,
            };
            term.set(colour, v);
            continue;
        }
        let Json::Str(val) = val else { return Err(raise("TypeError", "'int' object is not subscriptable")) };
        let mut harmonized;
        if colour == "term7" {
            harmonized = harmonize(hex_to_argb(val)?, primary_argb, args.harmonize_threshold.f() * 0.3, args.harmony * 0.4);
            harmonized = boost_chroma_tone(harmonized, args.term_saturation * 1.2, 1.0, 95.0);
        } else {
            harmonized = harmonize(hex_to_argb(val)?, primary_argb, args.harmonize_threshold.f() * 0.12, args.harmony);
            let mut tone_mult = 1.0 + (args.term_brightness - 0.5) * 0.8 * sign;
            let fg_boost_delta = args.term_fg_boost * 0.25 * sign;
            tone_mult = py_max(0.60, py_min(1.45, tone_mult + fg_boost_delta));
            let mut tone_cap = 95.0;
            if darkmode {
                tone_cap = if matches!(colour.as_str(), "term9" | "term10" | "term11" | "term12" | "term13" | "term14") { 84.0 } else { 76.0 };
            }
            harmonized = boost_chroma_tone(harmonized, args.term_saturation * 2.0, tone_mult, tone_cap);
            harmonized = ensure_min_chroma(harmonized, 40.0);
            let ceiling = 24.0 + 36.0 * args.term_saturation;
            let h = Hct::from_int(harmonized);
            if h.chroma > ceiling {
                harmonized = Hct::from_hct(h.hue, ceiling, h.tone).argb;
            }
        }
        if soften {
            harmonized = boost_chroma_tone(harmonized, 0.55, 1.0, 95.0);
        }
        term.set(colour, rgb_hex(harmonized));
    }
    if let Some(bg) = term.get("term0") {
        let bg_argb = hex_to_argb(bg)?;
        for c in ["term1", "term2", "term3", "term4", "term5", "term6"] {
            if let Some(v) = term.get(c) {
                let a = ensure_contrast(hex_to_argb(v)?, bg_argb, 4.5, darkmode, None);
                term.set(c, rgb_hex(a));
            }
        }
        let bg_hct = Hct::from_int(bg_argb);
        let grey_chroma = py_min(bg_hct.chroma, 10.0);
        for (c, start, ratio) in [("term7", if darkmode { 75.0 } else { 35.0 }, 4.5), ("term8", if darkmode { 60.0 } else { 50.0 }, 3.5)] {
            if term.has(c) {
                let grey = Hct::from_hct(bg_hct.hue, grey_chroma, start).argb;
                term.set(c, rgb_hex(ensure_contrast(grey, bg_argb, ratio, darkmode, None)));
            }
        }
        for c in ["term9", "term10", "term11", "term12", "term13", "term14"] {
            if let Some(v) = term.get(c) {
                let a = ensure_contrast(hex_to_argb(v)?, bg_argb, if darkmode { 3.5 } else { 4.5 }, darkmode, None);
                term.set(c, rgb_hex(a));
            }
        }
    }
    if darkmode && term.has("term0") && term.has("term15") {
        solve_dark_terminal_reading(term, args.term_fg_boost)?;
    }
    Ok(())
}

/// `f"{v:02X}"` for any Python int (a sign counts towards the width).
fn py_hex2(v: i64) -> String {
    let digits = format!("{:X}", v.unsigned_abs());
    let s = if v < 0 { format!("-{digits}") } else { digits };
    if s.len() < 2 { format!("{:0>2}", s) } else { s }
}

#[allow(clippy::too_many_arguments)]
fn render_templates(
    args: &Args,
    dir: &str,
    s: &Scheme,
    hct: Hct,
    argb: u32,
    darkmode: bool,
    material: &Dict,
    app_palette: &Dict,
) -> Result<i32, Raise> {
    let manifest_path = py::join(dir, "templates.json");
    let legacy = py::join(dir, "config.toml");
    let mut managed: Vec<String> = Vec::new();
    for p in [&args.json_output, &args.palette_output, &args.app_palette_output, &args.terminal_output, &args.meta_output, &args.scss_output]
        .into_iter()
        .flatten()
    {
        if p.is_empty() {
            continue;
        }
        let mut resolved = py::abspath(&py::expanduser(p));
        if resolved.ends_with(".tmp") {
            resolved.truncate(resolved.len() - 4);
        }
        managed.push(resolved);
    }
    let mut entries: Vec<(String, String)> = Vec::new();
    if Path::new(&manifest_path).is_file() {
        let text = match read_text(&manifest_path) {
            Ok(Ok(t)) => t,
            Ok(Err(())) => return Err(raise("UnicodeDecodeError", "'utf-8' codec can't decode")),
            Err(e) => return Err(os_error(&e, &manifest_path)),
        };
        let manifest = py::json_loads(&text).ok_or_else(|| raise("json.decoder.JSONDecodeError", "Expecting value"))?;
        let Json::Obj(_) = manifest else { return Err(raise("AttributeError", "object has no attribute 'get'")) };
        let list = match manifest.get("templates") {
            None => Vec::new(),
            Some(Json::Arr(a)) => a.clone(),
            Some(_) => return Err(raise("TypeError", "templates is not a list")),
        };
        let base = py::join(dir, "templates");
        for entry in list {
            let Some(Json::Str(output)) = entry.get("output") else { return Err(raise("KeyError", "'output'")) };
            let output_path = py::abspath(&py::expanduser(output));
            if managed.contains(&output_path) {
                continue;
            }
            let Some(Json::Str(input)) = entry.get("input") else { return Err(raise("KeyError", "'input'")) };
            entries.push((py::join(&base, input), output_path));
        }
    } else {
        // (config.toml was handed to the original before any effect.)
        eprintln!("[render-templates] Missing {manifest_path} and {legacy}, skipping template rendering");
    }
    if entries.is_empty() {
        return Ok(0);
    }

    let soften = args.soften && softens(&args.scheme);
    let generate = |is_dark: bool| -> Result<Dict, Raise> {
        let sc = Scheme::new(hct, s.variant, is_dark, 0.0);
        let mut p = Dict::default();
        for &role in ROLES {
            let mut g = scheme::get_hct(role, &sc);
            if soften {
                g = Hct::from_hct(g.hue, g.chroma * 0.60, g.tone);
            }
            p.set(role.name(), rgb_hex(g.argb));
        }
        p.set("source_color", rgb_hex(argb));
        success_colours(&mut p, is_dark);
        let mut raw = Dict::default();
        for (k, src) in RAW_CONTRACT {
            raw.set(k, p.get_or(src, "").to_string());
        }
        p.update(&build_app_palette(&raw)?);
        Ok(p)
    };
    let mut dark_palette = generate(true)?;
    let mut light_palette = generate(false)?;
    let mut current = if darkmode { dark_palette.clone() } else { light_palette.clone() };
    current.update(material);
    current.set("source_color", rgb_hex(argb));
    current.update(app_palette);
    if darkmode {
        dark_palette = current.clone();
    } else {
        light_palette = current.clone();
    }
    let default_palette = &current;

    let mut tokens: Vec<String> = dark_palette.0.iter().chain(light_palette.0.iter()).map(|(k, _)| k.clone()).collect();
    tokens.sort();
    tokens.dedup();
    let mut ns: Vec<(String, usize)> = Vec::new();
    let mut values: Vec<[HexVal; 3]> = Vec::new();
    for tok in &tokens {
        let dk = hex_val(dark_palette.get_or(tok, "#000000"))?;
        let lt = hex_val(light_palette.get_or(tok, "#000000"))?;
        let df = hex_val(default_palette.get_or(tok, "#000000"))?;
        values.push([dk, lt, df]);
        ns.push((tok.clone(), values.len() - 1));
    }
    let mut lookup: std::collections::HashMap<String, usize> = ns.iter().cloned().collect();
    for (i, tok) in tokens.iter().enumerate() {
        let snake = camel_to_snake(tok);
        if snake != *tok {
            lookup.entry(snake).or_insert(i);
        }
    }

    let image_path = args.path.clone().unwrap_or_default();
    let mut rendered_count = 0;
    for (tpl, out_path) in entries {
        if !Path::new(&tpl).is_file() {
            eprintln!("[render-templates] Skipping missing template: {tpl}");
            continue;
        }
        let content = match read_text(&tpl) {
            Ok(Ok(t)) => t,
            Ok(Err(())) => return Err(raise("UnicodeDecodeError", "'utf-8' codec can't decode")),
            Err(e) => return Err(os_error(&e, &tpl)),
        };
        let rendered = substitute(&content, |whole, expr| {
            if expr == "image" {
                return image_path.clone();
            }
            let parts: Vec<&str> = expr.split('.').collect();
            if parts.len() == 4 && parts[0] == "colors" {
                let (token, mode, prop) = (parts[1], parts[2], parts[3]);
                let Some(&i) = lookup.get(token) else {
                    eprintln!("[render-templates] WARNING: unresolved token '{token}' in {{{{colors.{token}.{mode}.{prop}}}}}");
                    return whole.to_string();
                };
                let m = match mode {
                    "dark" => 0,
                    "light" => 1,
                    "default" => 2,
                    _ => {
                        eprintln!(
                            "[render-templates] WARNING: unresolved mode '{mode}' for token '{token}' in {{{{colors.{token}.{mode}.{prop}}}}}"
                        );
                        return whole.to_string();
                    }
                };
                let v = &values[i][m];
                return match prop {
                    "hex" => v.hex.clone(),
                    "hex_stripped" => v.stripped.clone(),
                    "rgb" => v.rgb.clone(),
                    _ => {
                        eprintln!(
                            "[render-templates] WARNING: unresolved prop '{prop}' for token '{token}.{mode}' in {{{{colors.{token}.{mode}.{prop}}}}}"
                        );
                        whole.to_string()
                    }
                };
            }
            whole.to_string()
        });
        let parent = py::dirname(&out_path);
        if !parent.is_empty() {
            std::fs::create_dir_all(&parent).map_err(|e| os_error(&e, &parent))?;
        }
        if std::fs::symlink_metadata(&out_path).is_ok_and(|m| m.file_type().is_symlink()) {
            eprintln!("[render-templates] Replacing symlink with regular file: {out_path}");
            std::fs::remove_file(&out_path).map_err(|e| os_error(&e, &out_path))?;
        }
        write_file(&out_path, &rendered)?;
        rendered_count += 1;
    }
    if rendered_count > 0 {
        eprintln!("[render-templates] Rendered {rendered_count} template(s)");
    }
    Ok(0)
}
