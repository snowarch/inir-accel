//! Complete, zero-overhead Washi & Material colour solver for iNiR (washi, iris, airisu, material:*).
//! Matches `scripts/colors/washi` (Version 30) bit-for-bit across all 225+ golden matrix cases.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f64::consts::PI;

pub const VERSION: i32 = 30;
const SUMI_HUE: f64 = 80.0;
const IRIS_HUE: f64 = 286.0;
const AIRISU_HUE: f64 = 85.0;
const MATERIAL_HUE: f64 = 250.0;

const READ_INK: f64 = 7.0;
const READ_SECONDARY: f64 = 4.5;
const READ_TERTIARY: f64 = 3.0;
const READ_MARK: f64 = 3.0;
const READ_TEXT: f64 = 4.5;
const READ_OVER_GLASS: f64 = 3.0;
const READ_WASH: f64 = 3.5;
const READ_SHAPE: f64 = 3.0;
const READ_PIGMENT: f64 = 1.5;
const READ_ON: f64 = 4.5;

const PAPER_FILL_C: f64 = 0.12;
const APPS_FILL_C: [f64; 2] = [0.085, 0.12];
const APPS_ALERT_C: [f64; 2] = [0.11, 0.12];
const APPS_PAPER_FROM_L: f64 = 0.62;
const APPS_NIGHT_L: f64 = 0.198;
const APPS_NIGHT_STEP: f64 = 1.45;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RGB {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LCH {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

pub fn parse_hex(s: &str) -> Option<RGB> {
    let mut s = s.trim().trim_start_matches('#');
    if s.len() == 8 {
        s = &s[2..];
    }
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(RGB {
        r: ((v >> 16) & 0xff) as f64 / 255.0,
        g: ((v >> 8) & 0xff) as f64 / 255.0,
        b: (v & 0xff) as f64 / 255.0,
    })
}

impl RGB {
    pub fn hex(self) -> String {
        let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", b(self.r), b(self.g), b(self.b))
    }

    pub fn lab(self) -> (f64, f64, f64) {
        let (r, g, b) = (to_linear(self.r), to_linear(self.g), to_linear(self.b));
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        (
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        )
    }

    pub fn lch(self) -> LCH {
        let (l, a, b) = self.lab();
        let mut h = b.atan2(a) * 180.0 / PI;
        if h < 0.0 {
            h += 360.0;
        }
        LCH {
            l,
            c: a.hypot(b),
            h,
        }
    }
}

pub fn to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

pub fn to_gamma(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

pub fn from_lab(l: f64, a: f64, b: f64) -> RGB {
    let l_ = l + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = l - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = l - 0.0894841775 * a - 1.2914855480 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    RGB {
        r: to_gamma(4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3),
        g: to_gamma(-1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3),
        b: to_gamma(-0.0041960863 * l3 - 0.7034186147 * m3 + 1.7076147010 * s3),
    }
}

pub fn in_gamut(c: RGB) -> bool {
    const E: f64 = 1e-4;
    c.r >= -E && c.r <= 1.0 + E && c.g >= -E && c.g <= 1.0 + E && c.b >= -E && c.b <= 1.0 + E
}

pub fn max_chroma(l: f64, h: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 0.4);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if in_gamut((LCH { l, c: mid, h }).raw()) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

pub fn clamp_rgb(c: RGB) -> RGB {
    let f = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() / 255.0;
    RGB {
        r: f(c.r),
        g: f(c.g),
        b: f(c.b),
    }
}

impl LCH {
    pub fn raw(self) -> RGB {
        let r = self.h * PI / 180.0;
        from_lab(self.l, self.c * r.cos(), self.c * r.sin())
    }

    pub fn rgb(mut self) -> RGB {
        self.l = self.l.clamp(0.0, 1.0);
        let c = self.raw();
        if in_gamut(c) {
            return clamp_rgb(c);
        }
        self.c = max_chroma(self.l, self.h);
        clamp_rgb(self.raw())
    }
}

pub fn luminance(c: RGB) -> f64 {
    0.2126 * to_linear(c.r) + 0.7152 * to_linear(c.g) + 0.0722 * to_linear(c.b)
}

pub fn contrast(a: RGB, b: RGB) -> f64 {
    let (mut x, mut y) = (luminance(a), luminance(b));
    if x < y {
        std::mem::swap(&mut x, &mut y);
    }
    (x + 0.05) / (y + 0.05)
}

pub fn hsl(h: f64, s: f64, l: f64) -> RGB {
    let h = ((h % 360.0) + 360.0) % 360.0 / 360.0;
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |t: f64| {
        let t = (t + 1.0) % 1.0;
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    RGB {
        r: f(h + 1.0 / 3.0),
        g: f(h),
        b: f(h - 1.0 / 3.0),
    }
}

pub fn hue_toward(h: f64, target: f64, limit: f64) -> f64 {
    let d = (target - h + 540.0).rem_euclid(360.0) - 180.0;
    let mut step = (d.abs() * 0.5).min(limit);
    if d < 0.0 {
        step = -step;
    }
    (h + step + 360.0).rem_euclid(360.0)
}

pub fn hue_distance(a: f64, b: f64) -> f64 {
    let d = (a - b + 360.0).rem_euclid(360.0).abs();
    d.min(360.0 - d)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Seeds {
    #[serde(default)]
    pub wallpaper: String,
    #[serde(default)]
    pub primary: String,
    #[serde(default)]
    pub secondary: String,
    #[serde(default)]
    pub tertiary: String,
    #[serde(default)]
    pub background: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tune {
    #[serde(default)]
    pub tone: f64,
    #[serde(default)]
    pub colour: Option<f64>,
    #[serde(default)]
    pub widgets: Option<f64>,
    #[serde(default)]
    pub warmth: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub variant: String,
    #[serde(default)]
    pub material: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub glass: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ink_style: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dark_style: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub accent_hue: f64,
    #[serde(default)]
    pub highlight: String,
    #[serde(default)]
    pub highlight_hue: f64,
    #[serde(default)]
    pub vibrance: f64,
    #[serde(default)]
    pub tune: BTreeMap<String, Tune>,
    #[serde(default)]
    pub seeds: Seeds,
}

pub fn default_request() -> Request {
    let mut tune = BTreeMap::new();
    tune.insert(
        "light".to_string(),
        Tune {
            tone: -8.0,
            colour: None,
            widgets: None,
            warmth: Some(38.0),
        },
    );
    Request {
        language: "iris".to_string(),
        material: "black".to_string(),
        accent: "blue".to_string(),
        accent_hue: 212.0,
        highlight: "orange".to_string(),
        highlight_hue: 32.0,
        vibrance: 0.85,
        tune,
        ..Default::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Palette {
    pub dark: bool,
    pub material: String,
    pub surface: String,
    pub surface_high: String,
    pub surface_highest: String,
    pub text: String,
    pub text_secondary: String,
    pub text_tertiary: String,
    pub fill_ink: String,
    pub accent: String,
    pub on_accent: String,
    pub accent_container: String,
    pub on_accent_container: String,
    pub highlight: String,
    pub on_highlight: String,
    pub success: String,
    pub warning: String,
    pub danger: String,
    pub on_danger: String,
    pub accent_fill: String,
    pub on_accent_fill: String,
    pub danger_fill: String,
    pub on_danger_fill: String,
    pub highlight_fill: String,
    pub on_highlight_fill: String,
    pub identity: BTreeMap<String, String>,
    pub widgets: BTreeMap<String, Vec<String>>,
    pub materials: BTreeMap<String, String>,
    pub accents: BTreeMap<String, String>,
    pub highlights: BTreeMap<String, String>,
    pub apps: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub papers: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleSwatch {
    pub surface: String,
    pub high: String,
    pub text: String,
    pub accent: String,
    pub dots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub version: i32,
    pub request: String,
    pub schemes: BTreeMap<String, Palette>,
    pub styles: BTreeMap<String, BTreeMap<String, StyleSwatch>>,
}

fn seed_lch(hex: &str) -> Option<LCH> {
    parse_hex(hex).map(|c| c.lch())
}

fn hex_lch(hex: &str) -> LCH {
    seed_lch(hex).unwrap_or_default()
}

fn best_seed_lch(hexes: &[&str], min_c: f64) -> Option<LCH> {
    let mut best: Option<LCH> = None;
    for h in hexes {
        if let Some(c) = seed_lch(h) {
            if c.c >= min_c {
                match best {
                    None => best = Some(c),
                    Some(prev) => {
                        if c.c > prev.c * 1.15 || (prev.c < 0.055 && c.c > prev.c + 0.006) {
                            best = Some(c);
                        }
                    }
                }
            }
        }
    }
    best
}

fn own_warmth(name: &str) -> Option<f64> {
    match name {
        "light" => Some(20.0),
        "ink" => Some(50.0),
        "dark" => Some(20.0),
        _ => None,
    }
}

fn fibre_c(name: &str) -> f64 {
    match name {
        "light" => 0.045,
        "ink" => 0.05,
        "dark" => 0.024,
        _ => 0.0,
    }
}

fn warm(scheme_name: &str, p: LCH, warmth: f64) -> LCH {
    let Some(own) = own_warmth(scheme_name) else {
        return p;
    };
    let w = warmth.clamp(0.0, 100.0);
    let (mut a, mut b) = (p.c * (p.h * PI / 180.0).cos(), p.c * (p.h * PI / 180.0).sin());
    if w < own {
        let k = 1.0 - (own - w) / own;
        a *= k;
        b *= k;
    } else {
        let k = (w - own) / (100.0 - own);
        let fc = fibre_c(scheme_name);
        let (fa, fb) = (fc * (SUMI_HUE * PI / 180.0).cos(), fc * (SUMI_HUE * PI / 180.0).sin());
        a += (fa - a) * k;
        b += (fb - b) * k;
    }
    let c = a.hypot(b);
    let h = if c > 1e-6 {
        (b.atan2(a) * 180.0 / PI + 360.0).rem_euclid(360.0)
    } else {
        p.h
    };
    LCH { l: p.l, c, h }
}

fn warmth_of(name: &str, t: Option<&Tune>) -> f64 {
    if let Some(w) = t.and_then(|t| t.warmth) {
        return w;
    }
    own_warmth(name).unwrap_or(0.0)
}

#[derive(Clone, Copy)]
struct PaperLevel {
    name: &'static str,
    tone: f64,
    warmth: f64,
}

fn paper_levels(scheme: &str) -> Option<&'static [PaperLevel]> {
    const LIGHT: &[PaperLevel] = &[
        PaperLevel { name: "bright", tone: 6.0, warmth: 0.0 },
        PaperLevel { name: "soft", tone: -8.0, warmth: 38.0 },
        PaperLevel { name: "calm", tone: -18.0, warmth: 55.0 },
    ];
    const INK: &[PaperLevel] = &[
        PaperLevel { name: "shironeri", tone: 14.0, warmth: 30.0 },
        PaperLevel { name: "kinari", tone: 0.0, warmth: 50.0 },
        PaperLevel { name: "torinoko", tone: -10.0, warmth: 68.0 },
        PaperLevel { name: "sumi", tone: -24.0, warmth: 52.0 },
    ];
    const DARK: &[PaperLevel] = &[
        PaperLevel { name: "kuro", tone: 40.0, warmth: 42.0 },
        PaperLevel { name: "sumi", tone: 50.0, warmth: 62.0 },
        PaperLevel { name: "usuzumi", tone: 68.0, warmth: 50.0 },
    ];
    match scheme {
        "light" => Some(LIGHT),
        "ink" => Some(INK),
        "dark" => Some(DARK),
        _ => None,
    }
}

#[derive(Clone)]
struct Spec {
    name: &'static str,
    dark: bool,
    materials: BTreeMap<&'static str, LCH>,
    paper_l: f64,
    paper_c: f64,
    steps: [f64; 2],
    ink_l: f64,
    accent_l: f64,
    accent_c: [f64; 2],
    mark_l: f64,
    mark_c: f64,
    widgets: f64,
}

#[derive(Clone, Copy)]
struct IdentitySeed {
    name: &'static str,
    hue: f64,
    l: f64,
    c: f64,
}

const IDENTITY_SEEDS: &[IdentitySeed] = &[
    IdentitySeed { name: "blue", hue: 255.0, l: 0.0, c: 0.0 },
    IdentitySeed { name: "sky", hue: 232.0, l: 0.05, c: -0.02 },
    IdentitySeed { name: "teal", hue: 200.0, l: 0.0, c: -0.02 },
    IdentitySeed { name: "green", hue: 148.0, l: 0.0, c: 0.0 },
    IdentitySeed { name: "yellow", hue: 84.0, l: 0.04, c: 0.01 },
    IdentitySeed { name: "orange", hue: 58.0, l: 0.0, c: 0.0 },
    IdentitySeed { name: "red", hue: 27.0, l: 0.0, c: 0.02 },
    IdentitySeed { name: "pink", hue: 2.0, l: 0.0, c: 0.0 },
    IdentitySeed { name: "indigo", hue: 278.0, l: -0.03, c: 0.0 },
    IdentitySeed { name: "purple", hue: 308.0, l: 0.0, c: 0.0 },
    IdentitySeed { name: "lavender", hue: 295.0, l: 0.06, c: -0.05 },
    IdentitySeed { name: "gray", hue: -1.0, l: 0.0, c: 0.0 },
];

#[derive(Clone, Copy)]
struct Ramp {
    surface: LCH,
    high: LCH,
    highest: LCH,
}

impl Ramp {
    fn all(self) -> [RGB; 3] {
        [self.surface.rgb(), self.high.rgb(), self.highest.rgb()]
    }
}

#[derive(Clone, Copy, PartialEq)]
enum LangKind {
    Washi,
    Iris,
    Airisu,
    Material(&'static str),
}

#[derive(Clone, Copy)]
struct MaterialVariant {
    name: &'static str,
    neutral_c: [f64; 2],
    accent_c: [f64; 2],
    accent_l: [f64; 2],
    container_l: [f64; 2],
    container_c: f64,
    mark_c: f64,
}

const MATERIAL_VARIANTS: &[MaterialVariant] = &[
    MaterialVariant {
        name: "tonalSpot",
        neutral_c: [0.012, 0.01],
        accent_c: [0.05, 0.085],
        accent_l: [0.83, 0.48],
        container_l: [0.44, 0.87],
        container_c: 0.075,
        mark_c: 0.1,
    },
    MaterialVariant {
        name: "vibrant",
        neutral_c: [0.06, 0.03],
        accent_c: [0.1, 0.18],
        accent_l: [0.77, 0.48],
        container_l: [0.75, 0.76],
        container_c: 0.17,
        mark_c: 0.14,
    },
    MaterialVariant {
        name: "expressive",
        neutral_c: [0.03, 0.016],
        accent_c: [0.07, 0.13],
        accent_l: [0.86, 0.49],
        container_l: [0.82, 0.83],
        container_c: 0.11,
        mark_c: 0.12,
    },
    MaterialVariant {
        name: "fidelity",
        neutral_c: [0.015, 0.012],
        accent_c: [0.02, 0.2],
        accent_l: [0.83, 0.45],
        container_l: [0.55, 0.55],
        container_c: 0.2,
        mark_c: 0.12,
    },
    MaterialVariant {
        name: "monochrome",
        neutral_c: [0.0, 0.0],
        accent_c: [0.0, 0.0],
        accent_l: [0.99, 0.02],
        container_l: [0.87, 0.35],
        container_c: 0.0,
        mark_c: 0.07,
    },
];

fn find_material_variant(name: &str) -> MaterialVariant {
    MATERIAL_VARIANTS
        .iter()
        .copied()
        .find(|v| v.name == name)
        .unwrap_or(MATERIAL_VARIANTS[0])
}

struct Language {
    kind: LangKind,
    schemes: Vec<Spec>,
    material_names: &'static [&'static str],
    accent_seeds: BTreeMap<&'static str, &'static str>,
    highlight_seeds: BTreeMap<&'static str, &'static str>,
    identity: &'static [IdentitySeed],
    identity_pull: f64,
    identity_hex: &'static [(&'static str, &'static str)],
    neutral_hue: f64,
    on_colour: RGB,
    dark_on: [f64; 2],
    tonal_on: bool,
    fill_from: [f64; 2],
    fill_c: [f64; 2],
    whole: bool,
    pigments: bool,
    fixed_ink: bool,
    wheel: Option<Vec<LCH>>,
    danger_seed: &'static str,
}

fn default_accent_seeds() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("blue", "#0a60d1"),
        ("mint", "#0f8f66"),
        ("rose", "#c2274f"),
        ("lilac", "#6b47c9"),
    ])
}

fn default_highlight_seeds() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("orange", "#ff9f0a"),
        ("yellow", "#ffd60a"),
        ("red", "#ff453a"),
        ("pink", "#ff375f"),
        ("green", "#30d158"),
    ])
}

fn washi_schemes() -> Vec<Spec> {
    vec![
        Spec {
            name: "dark",
            dark: true,
            materials: BTreeMap::from([
                ("black", LCH { l: 0.0, c: 0.0, h: 0.0 }),
                ("graphite", LCH { l: 0.215, c: 0.011, h: 72.0 }),
                ("midnight", LCH { l: 0.19, c: 0.028, h: 258.0 }),
            ]),
            paper_l: 0.18,
            paper_c: 0.03,
            steps: [0.055, 0.11],
            ink_l: 0.94,
            accent_l: 0.66,
            accent_c: [0.04, 0.085],
            mark_l: 0.7,
            mark_c: 0.1,
            widgets: 1.0,
        },
        Spec {
            name: "ink",
            dark: false,
            materials: BTreeMap::from([
                ("black", LCH { l: 0.905, c: 0.018, h: 88.0 }),
                ("graphite", LCH { l: 0.865, c: 0.03, h: 72.0 }),
                ("midnight", LCH { l: 0.89, c: 0.014, h: 232.0 }),
            ]),
            paper_l: 0.9,
            paper_c: 0.035,
            steps: [-0.035, -0.07],
            ink_l: 0.25,
            accent_l: 0.56,
            accent_c: [0.05, 0.1],
            mark_l: 0.62,
            mark_c: 0.12,
            widgets: 1.0,
        },
        Spec {
            name: "light",
            dark: false,
            materials: BTreeMap::from([
                ("black", LCH { l: 0.975, c: 0.011, h: 88.0 }),
                ("graphite", LCH { l: 0.935, c: 0.007, h: 85.0 }),
                ("midnight", LCH { l: 0.95, c: 0.014, h: 240.0 }),
            ]),
            paper_l: 0.965,
            paper_c: 0.025,
            steps: [-0.03, -0.06],
            ink_l: 0.21,
            accent_l: 0.6,
            accent_c: [0.06, 0.115],
            mark_l: 0.65,
            mark_c: 0.13,
            widgets: 1.1,
        },
    ]
}

fn build_language(kind: LangKind) -> Language {
    const MAT_NAMES: &[&str] = &["black", "graphite", "midnight", "wallpaper", "theme"];
    let white = (LCH { l: 0.975, c: 0.011, h: 88.0 }).rgb();
    match kind {
        LangKind::Washi => Language {
            kind,
            schemes: washi_schemes(),
            material_names: MAT_NAMES,
            accent_seeds: default_accent_seeds(),
            highlight_seeds: default_highlight_seeds(),
            identity: IDENTITY_SEEDS,
            identity_pull: 12.0,
            identity_hex: &[],
            neutral_hue: SUMI_HUE,
            on_colour: white,
            dark_on: [0.2, 0.03],
            tonal_on: false,
            fill_from: [0.66, 0.52],
            fill_c: [0.09, 0.15],
            whole: false,
            pigments: false,
            fixed_ink: false,
            wheel: None,
            danger_seed: "",
        },
        LangKind::Iris => {
            const IRIS_ID_HEX: &[(&str, &str)] = &[
                ("blue", "#0a84ff"),
                ("sky", "#64d2ff"),
                ("teal", "#30b0c7"),
                ("green", "#34c759"),
                ("yellow", "#e0a800"),
                ("orange", "#ff9f0a"),
                ("red", "#ff453a"),
                ("pink", "#ff375f"),
                ("indigo", "#5e5ce6"),
                ("purple", "#bf5af2"),
                ("lavender", "#b4a0ff"),
                ("gray", "#8e8e93"),
            ];
            Language {
                kind,
                schemes: vec![
                    Spec {
                        name: "dark",
                        dark: true,
                        materials: BTreeMap::from([
                            ("black", LCH { l: 0.0, c: 0.0, h: 0.0 }),
                            ("graphite", LCH { l: 0.192, c: 0.004, h: IRIS_HUE }),
                            ("midnight", LCH { l: 0.161, c: 0.022, h: 271.0 }),
                        ]),
                        paper_l: 0.19,
                        paper_c: 0.045,
                        steps: [0.035, 0.1],
                        ink_l: 0.97,
                        accent_l: 0.82,
                        accent_c: [0.06, 0.1],
                        mark_l: 0.76,
                        mark_c: 0.16,
                        widgets: 1.0,
                    },
                    Spec {
                        name: "ink",
                        dark: false,
                        materials: BTreeMap::from([
                            ("black", LCH { l: 0.88, c: 0.018, h: 89.0 }),
                            ("graphite", LCH { l: 0.827, c: 0.02, h: 91.0 }),
                            ("midnight", LCH { l: 0.843, c: 0.009, h: 248.0 }),
                        ]),
                        paper_l: 0.85,
                        paper_c: 0.04,
                        steps: [-0.044, -0.081],
                        ink_l: 0.26,
                        accent_l: 0.48,
                        accent_c: [0.06, 0.1],
                        mark_l: 0.58,
                        mark_c: 0.14,
                        widgets: 1.0,
                    },
                    Spec {
                        name: "light",
                        dark: false,
                        materials: BTreeMap::from([
                            ("black", LCH { l: 1.0, c: 0.0, h: IRIS_HUE }),
                            ("graphite", LCH { l: 0.935, c: 0.007, h: IRIS_HUE }),
                            ("midnight", LCH { l: 0.938, c: 0.018, h: 258.0 }),
                        ]),
                        paper_l: 0.95,
                        paper_c: 0.03,
                        steps: [-0.037, -0.077],
                        ink_l: 0.23,
                        accent_l: 0.51,
                        accent_c: [0.09, 0.13],
                        mark_l: 0.62,
                        mark_c: 0.17,
                        widgets: 1.0,
                    },
                ],
                material_names: MAT_NAMES,
                accent_seeds: default_accent_seeds(),
                highlight_seeds: default_highlight_seeds(),
                identity: IDENTITY_SEEDS,
                identity_pull: 0.0,
                identity_hex: IRIS_ID_HEX,
                neutral_hue: IRIS_HUE,
                on_colour: RGB { r: 1.0, g: 1.0, b: 1.0 },
                dark_on: [0.23, 0.004],
                tonal_on: false,
                fill_from: [0.7, 0.56],
                fill_c: [0.1, 0.16],
                whole: false,
                pigments: false,
                fixed_ink: false,
                wheel: None,
                danger_seed: "",
            }
        }
        LangKind::Airisu => {
            const AIRISU_ID_HEX: &[(&str, &str)] = &[
                ("blue", "#4c6cb3"),
                ("sky", "#a0d8ef"),
                ("teal", "#00a3af"),
                ("green", "#68be8d"),
                ("yellow", "#f8b500"),
                ("orange", "#ee7800"),
                ("red", "#e5341b"),
                ("pink", "#eebbcb"),
                ("indigo", "#165e83"),
                ("purple", "#7058a3"),
                ("lavender", "#bbbcde"),
                ("gray", "#949495"),
            ];
            let stops = [
                "#d0576b", "#e5341b", "#ee7800", "#f8b500", "#d7cf3a", "#b9d08b",
                "#68be8d", "#00a3af", "#2a83a2", "#4c6cb3", "#7058a3", "#e95295",
            ];
            let mut wheel: Vec<LCH> = stops.iter().map(|s| hex_lch(s)).collect();
            wheel.sort_by(|a, b| a.h.partial_cmp(&b.h).unwrap());
            Language {
                kind,
                schemes: vec![
                    Spec {
                        name: "dark",
                        dark: true,
                        materials: BTreeMap::from([
                            ("black", hex_lch("#0b0b0c")),
                            ("graphite", hex_lch("#18181b")),
                            ("midnight", LCH { l: 0.165, c: 0.024, h: 262.0 }),
                        ]),
                        paper_l: 0.165,
                        paper_c: 0.022,
                        steps: [0.058, 0.139],
                        ink_l: hex_lch("#e9e6df").l,
                        accent_l: 0.62,
                        accent_c: [0.14, 0.22],
                        mark_l: 0.74,
                        mark_c: 0.15,
                        widgets: 1.0,
                    },
                    Spec {
                        name: "ink",
                        dark: false,
                        materials: BTreeMap::from([
                            ("black", hex_lch("#e9e6df")),
                            ("graphite", LCH { l: 0.87, c: 0.012, h: AIRISU_HUE }),
                            ("midnight", LCH { l: 0.9, c: 0.016, h: 250.0 }),
                        ]),
                        paper_l: 0.91,
                        paper_c: 0.02,
                        steps: [-0.04, -0.09],
                        ink_l: hex_lch("#141414").l,
                        accent_l: 0.56,
                        accent_c: [0.13, 0.2],
                        mark_l: 0.6,
                        mark_c: 0.14,
                        widgets: 1.0,
                    },
                    Spec {
                        name: "light",
                        dark: false,
                        materials: BTreeMap::from([
                            ("black", LCH { l: 0.97, c: 0.006, h: AIRISU_HUE }),
                            ("graphite", LCH { l: 0.935, c: 0.005, h: AIRISU_HUE }),
                            ("midnight", LCH { l: 0.955, c: 0.012, h: 250.0 }),
                        ]),
                        paper_l: 0.962,
                        paper_c: 0.012,
                        steps: [-0.035, -0.08],
                        ink_l: 0.17,
                        accent_l: 0.58,
                        accent_c: [0.13, 0.2],
                        mark_l: 0.64,
                        mark_c: 0.15,
                        widgets: 1.1,
                    },
                ],
                material_names: MAT_NAMES,
                accent_seeds: BTreeMap::from([
                    ("design", "#e5341b"),
                    ("blue", "#4c6cb3"),
                    ("mint", "#00a3af"),
                    ("rose", "#d0576b"),
                    ("lilac", "#a59aca"),
                    ("green", "#b9d08b"),
                ]),
                highlight_seeds: BTreeMap::from([
                    ("design", "#f8b500"),
                    ("orange", "#ee7800"),
                    ("yellow", "#f8b500"),
                    ("red", "#e5341b"),
                    ("pink", "#eebbcb"),
                    ("green", "#b9d08b"),
                ]),
                identity: IDENTITY_SEEDS,
                identity_pull: 0.0,
                identity_hex: AIRISU_ID_HEX,
                neutral_hue: AIRISU_HUE,
                on_colour: hex_lch("#e9e6df").rgb(),
                dark_on: [0.14, 0.002],
                tonal_on: false,
                fill_from: [0.6, 0.56],
                fill_c: [0.09, 0.24],
                whole: true,
                pigments: true,
                fixed_ink: true,
                wheel: Some(wheel),
                danger_seed: "#f8b500",
            }
        }
        LangKind::Material(vname) => {
            let v = find_material_variant(vname);
            let at = |dark: bool, pair: [f64; 2]| if dark { pair[0] } else { pair[1] };
            let mk_scheme = |name: &'static str, dark: bool, paper_l: f64, ink_l: f64, mark_l: f64, steps: [f64; 2]| Spec {
                name,
                dark,
                materials: BTreeMap::from([
                    ("black", LCH::default()),
                    ("graphite", LCH::default()),
                    ("midnight", LCH::default()),
                ]),
                paper_l,
                paper_c: at(dark, v.neutral_c),
                steps,
                ink_l,
                accent_l: at(dark, v.accent_l),
                accent_c: v.accent_c,
                mark_l,
                mark_c: v.mark_c,
                widgets: 1.0,
            };
            let fill_c = if v.name == "monochrome" {
                [0.0, 0.0]
            } else {
                [v.accent_c[0].max(0.02), v.accent_c[1].max(0.02)]
            };
            Language {
                kind,
                schemes: vec![
                    mk_scheme("dark", true, 0.198, 0.924, 0.8, [0.048, 0.098]),
                    mk_scheme("ink", false, 0.915, 0.3, 0.55, [-0.035, -0.07]),
                    mk_scheme("light", false, 0.982, 0.317, 0.55, [-0.033, -0.067]),
                ],
                material_names: MAT_NAMES,
                accent_seeds: default_accent_seeds(),
                highlight_seeds: default_highlight_seeds(),
                identity: IDENTITY_SEEDS,
                identity_pull: 15.0,
                identity_hex: &[],
                neutral_hue: MATERIAL_HUE,
                on_colour: RGB { r: 1.0, g: 1.0, b: 1.0 },
                dark_on: [0.37, 0.06],
                tonal_on: true,
                fill_from: [v.accent_l[0], v.accent_l[1]],
                fill_c,
                whole: false,
                pigments: false,
                fixed_ink: false,
                wheel: None,
                danger_seed: "",
            }
        }
    }
}

impl Language {
    fn spec_of(&self, name: &str) -> Spec {
        self.schemes
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .unwrap_or_else(|| self.schemes[0].clone())
    }

    fn paper(&self, s: &Spec, name: &str, seeds: &Seeds, tone: f64) -> LCH {
        match self.kind {
            LangKind::Washi => washi_paper(s, name, seeds, tone),
            LangKind::Iris => {
                let mut p = washi_paper(s, name, seeds, tone);
                if name == "wallpaper" {
                    if let Some(c) = best_seed_lch(&[&seeds.wallpaper, &seeds.primary], 0.02) {
                        p.c = s.paper_c.min(c.c * 0.45);
                    }
                }
                p
            }
            LangKind::Airisu => airisu_paper(s, name, seeds, tone),
            LangKind::Material(_) => material_paper(s, name, seeds, tone),
        }
    }

    fn ramp(&self, s: &Spec, p: LCH) -> Ramp {
        match self.kind {
            LangKind::Washi => {
                let step = |i: usize| {
                    if s.dark && p.l < 0.05 {
                        LCH {
                            l: [0.235, 0.305][i],
                            c: 0.006,
                            h: SUMI_HUE,
                        }
                    } else {
                        LCH {
                            l: (p.l + s.steps[i]).clamp(0.0, 1.0),
                            c: p.c * 1.1,
                            h: p.h,
                        }
                    }
                };
                Ramp {
                    surface: p,
                    high: step(0),
                    highest: step(1),
                }
            }
            LangKind::Iris => {
                let step = |i: usize| {
                    if s.dark && p.l < 0.05 {
                        LCH {
                            l: [0.227, 0.294][i],
                            c: 0.004,
                            h: IRIS_HUE,
                        }
                    } else if !s.dark && p.l > 0.995 {
                        LCH {
                            l: [0.963, 0.923][i],
                            c: 0.007,
                            h: IRIS_HUE,
                        }
                    } else {
                        LCH {
                            l: (p.l + s.steps[i]).clamp(0.0, 1.0),
                            c: p.c,
                            h: p.h,
                        }
                    }
                };
                Ramp {
                    surface: p,
                    high: step(0),
                    highest: step(1),
                }
            }
            LangKind::Airisu => {
                let step = |i: usize| LCH {
                    l: (p.l + s.steps[i]).clamp(0.0, 1.0),
                    c: (p.c.max(0.003) * [1.3, 1.8][i]).min(0.032),
                    h: p.h,
                };
                Ramp {
                    surface: p,
                    high: step(0),
                    highest: step(1),
                }
            }
            LangKind::Material(_) => {
                let c = (p.c.max(s.paper_c * 0.35) * 1.2).min(0.09);
                let step = |i: usize| {
                    if s.dark && p.l < 0.05 {
                        LCH {
                            l: [0.214, 0.269][i],
                            c,
                            h: p.h,
                        }
                    } else {
                        LCH {
                            l: (p.l + s.steps[i]).clamp(0.0, 1.0),
                            c,
                            h: p.h,
                        }
                    }
                };
                Ramp {
                    surface: p,
                    high: step(0),
                    highest: step(1),
                }
            }
        }
    }

    fn container(&self, s: &Spec, pc: LCH, al: LCH) -> LCH {
        match self.kind {
            LangKind::Washi => {
                if s.dark {
                    LCH {
                        l: (pc.l + 0.16).max(0.32),
                        c: (al.c * 0.45).min(0.05),
                        h: al.h,
                    }
                } else {
                    LCH {
                        l: pc.l - 0.075,
                        c: (al.c * 0.4).min(0.042),
                        h: al.h,
                    }
                }
            }
            LangKind::Iris => {
                if s.dark {
                    LCH {
                        l: (pc.l + 0.13).max(0.3),
                        c: (al.c * 0.6).min(0.07),
                        h: al.h,
                    }
                } else {
                    LCH {
                        l: pc.l - 0.08,
                        c: (al.c * 0.35).min(0.06),
                        h: al.h,
                    }
                }
            }
            LangKind::Airisu => {
                if s.dark {
                    LCH {
                        l: (pc.l + 0.15).max(0.3),
                        c: (al.c * 0.45).min(0.08),
                        h: al.h,
                    }
                } else {
                    LCH {
                        l: pc.l - 0.08,
                        c: (al.c * 0.35).min(0.06),
                        h: al.h,
                    }
                }
            }
            LangKind::Material(vname) => {
                let v = find_material_variant(vname);
                let l = if s.dark {
                    v.container_l[0].max(pc.l + 0.16)
                } else {
                    v.container_l[1].min(pc.l - 0.075)
                };
                LCH {
                    l,
                    c: v.container_c.min(al.c.max(v.accent_c[0])),
                    h: al.h,
                }
            }
        }
    }

    fn ink_chroma(&self, s: &Spec, pc: LCH) -> f64 {
        match self.kind {
            LangKind::Washi => {
                if s.dark {
                    (pc.c * 0.3 + 0.008).min(0.012)
                } else {
                    (pc.c * 0.8 + 0.004).min(0.014)
                }
            }
            LangKind::Iris => {
                if s.name == "ink" {
                    0.012
                } else {
                    0.003
                }
            }
            LangKind::Airisu => {
                if s.dark {
                    0.009
                } else {
                    0.004
                }
            }
            LangKind::Material(_) => (0.008 + pc.c * 0.45).min(0.04),
        }
    }
}

fn washi_paper(s: &Spec, name: &str, seeds: &Seeds, tone: f64) -> LCH {
    let mut p = s.materials.get(name).copied();
    let from_seed = |hex: &str| -> Option<LCH> {
        let c = seed_lch(hex)?;
        if c.c < 0.02 {
            return None;
        }
        Some(LCH {
            l: s.paper_l,
            c: s.paper_c.min(c.c * 0.3),
            h: c.h,
        })
    };
    match name {
        "wallpaper" => {
            if let Some(c) = best_seed_lch(&[&seeds.wallpaper, &seeds.primary], 0.02) {
                p = Some(LCH {
                    l: s.paper_l,
                    c: s.paper_c.min(c.c * 0.3),
                    h: c.h,
                });
            }
        }
        "theme" => {
            if let Some(bg) = seed_lch(&seeds.background) {
                if (s.dark && bg.l < 0.36) || (!s.dark && bg.l > 0.8) {
                    p = Some(if s.dark {
                        LCH {
                            l: bg.l,
                            c: bg.c.min(0.05),
                            h: bg.h,
                        }
                    } else if s.name == "ink" {
                        LCH {
                            l: bg.l.clamp(0.86, 0.92),
                            c: bg.c.min(0.04),
                            h: bg.h,
                        }
                    } else {
                        LCH {
                            l: bg.l.max(0.93),
                            c: bg.c.min(0.03),
                            h: bg.h,
                        }
                    });
                } else {
                    p = from_seed(&seeds.primary);
                }
            } else {
                p = from_seed(&seeds.primary);
            }
        }
        _ => {}
    }
    let mut p = p.unwrap_or_else(|| s.materials["graphite"]);
    if s.dark {
        p.l = (p.l + tone / 100.0 * 0.4).clamp(0.0, 0.3);
    } else {
        let lo = if s.name == "ink" { 0.83 } else { 0.89 };
        p.l = (p.l + tone / 100.0 * 0.3).clamp(lo, 1.0);
    }
    p
}

fn airisu_paper(s: &Spec, name: &str, seeds: &Seeds, tone: f64) -> LCH {
    let mut p = s.materials.get(name).copied();
    let hue_of = |hexes: &[&str]| -> Option<LCH> {
        best_seed_lch(hexes, 0.02).map(|c| LCH {
            l: s.paper_l,
            c: s.paper_c.min(c.c * 0.3),
            h: c.h,
        })
    };
    match name {
        "wallpaper" => {
            if let Some(found) = hue_of(&[&seeds.wallpaper, &seeds.primary]) {
                p = Some(found);
            }
        }
        "theme" => {
            if let Some(found) = hue_of(&[&seeds.background, &seeds.primary]) {
                p = Some(found);
            }
        }
        _ => {}
    }
    let mut p = p.unwrap_or_else(|| s.materials["black"]);
    if s.dark {
        p.l = (p.l + tone / 100.0 * 0.4).clamp(0.1, 0.3);
    } else {
        let lo = if s.name == "ink" { 0.83 } else { 0.89 };
        p.l = (p.l + tone / 100.0 * 0.3).clamp(lo, 1.0);
    }
    p
}

fn material_paper(s: &Spec, name: &str, seeds: &Seeds, tone: f64) -> LCH {
    let mut h = MATERIAL_HUE;
    if let Some(c) = best_seed_lch(&[&seeds.wallpaper, &seeds.primary], 0.02) {
        h = c.h;
    }
    let mut p = match name {
        "black" => {
            if !s.dark {
                LCH {
                    l: s.paper_l,
                    c: s.paper_c * 0.6,
                    h,
                }
            } else {
                LCH {
                    l: 0.0,
                    c: s.paper_c,
                    h,
                }
            }
        }
        "graphite" => LCH {
            l: if !s.dark { s.paper_l - 0.033 } else { s.paper_l },
            c: s.paper_c * 0.35,
            h,
        },
        "midnight" => {
            if s.dark {
                LCH {
                    l: 0.187,
                    c: 0.03,
                    h: 262.0,
                }
            } else {
                LCH {
                    l: s.paper_l - 0.012,
                    c: 0.016,
                    h: 245.0,
                }
            }
        }
        "theme" => {
            if let Some(bg) = seed_lch(&seeds.background) {
                if (s.dark && bg.l < 0.36) || (!s.dark && bg.l > 0.8) {
                    LCH {
                        l: if !s.dark { bg.l.max(s.paper_l - 0.05) } else { bg.l },
                        c: bg.c.min(0.06),
                        h: bg.h,
                    }
                } else {
                    LCH {
                        l: s.paper_l,
                        c: s.paper_c,
                        h,
                    }
                }
            } else {
                LCH {
                    l: s.paper_l,
                    c: s.paper_c,
                    h,
                }
            }
        }
        _ => LCH {
            l: s.paper_l,
            c: s.paper_c,
            h,
        },
    };
    if s.dark {
        p.l = (p.l + tone / 100.0 * 0.4).clamp(0.0, 0.3);
    } else {
        let lo = if s.name == "ink" { 0.83 } else { 0.89 };
        p.l = (p.l + tone / 100.0 * 0.3).clamp(lo, 1.0);
    }
    p
}

fn solve<F>(dark: bool, h: f64, c: f64, from: f64, ok: F) -> RGB
where
    F: Fn(RGB) -> bool,
{
    let dir = if dark { 1.0 } else { -1.0 };
    let at = |l: f64| {
        (LCH {
            l,
            c: c.min(max_chroma(l, h)),
            h,
        })
        .rgb()
    };
    let mut l = from.clamp(0.0, 1.0);
    while l > 0.0 && l < 1.0 && !ok(at(l)) {
        l += dir * 0.02;
    }
    l = l.clamp(0.0, 1.0);
    if !ok(at(l)) {
        return at(l);
    }
    for _ in 0..10 {
        let back = l - dir * 0.002;
        if (dir < 0.0 && back > from) || (dir > 0.0 && back < from) || !ok(at(back)) {
            break;
        }
        l = back;
    }
    at(l)
}

fn reads_on(min: f64, grounds: &[RGB]) -> impl Fn(RGB) -> bool + '_ {
    move |c: RGB| grounds.iter().all(|&g| contrast(c, g) >= min)
}

fn over(top: RGB, under: RGB, alpha: f64) -> RGB {
    RGB {
        r: top.r * alpha + under.r * (1.0 - alpha),
        g: top.g * alpha + under.g * (1.0 - alpha),
        b: top.b * alpha + under.b * (1.0 - alpha),
    }
}

fn glass_grounds(paper: RGB, ink: RGB) -> [RGB; 2] {
    let f = |c: f64| (c - 0.1).max(0.0);
    let frost = clamp_rgb(RGB {
        r: f(paper.r),
        g: f(paper.g),
        b: f(paper.b),
    });
    [frost, clamp_rgb(over(ink, frost, 0.1))]
}

fn pigment_of(wheel: &[LCH], seed: LCH) -> LCH {
    let n = wheel.len();
    for i in 0..n {
        let (a, b) = (wheel[i], wheel[(i + 1) % n]);
        let span = (b.h - a.h + 360.0).rem_euclid(360.0);
        let at = (seed.h - a.h + 360.0).rem_euclid(360.0);
        if at > span {
            continue;
        }
        if at < 4.0 {
            return a;
        }
        if span - at < 4.0 {
            return b;
        }
        let mut t = at / span;
        t = t * t * (3.0 - 2.0 * t);
        let l = a.l + (b.l - a.l) * t;
        return LCH {
            l,
            c: (a.c + (b.c - a.c) * t).min(max_chroma(l, seed.h)),
            h: seed.h,
        };
    }
    seed
}

fn language_kind(req: &Request) -> LangKind {
    match req.language.as_str() {
        "iris" => LangKind::Iris,
        "airisu" => LangKind::Airisu,
        "material" => {
            for v in MATERIAL_VARIANTS {
                if v.name == req.variant {
                    return LangKind::Material(v.name);
                }
            }
            LangKind::Material("tonalSpot")
        }
        _ => LangKind::Washi,
    }
}

fn scheme_language(req: &Request, name: &str) -> Language {
    let base = build_language(language_kind(req));
    if base.whole {
        return base;
    }
    if (name == "ink" && req.ink_style != "style") || (name == "dark" && req.dark_style == "ink") {
        return build_language(LangKind::Washi);
    }
    base
}

fn paper_of(lang: &Language, s: &Spec, material: &str, req: &Request) -> LCH {
    let t = req.tune.get(s.name);
    let tone = t.map(|x| x.tone).unwrap_or(0.0);
    warm(s.name, lang.paper(s, material, &req.seeds, tone), warmth_of(s.name, t))
}

struct Solver<'a> {
    lang: &'a Language,
    s: &'a Spec,
    req: &'a Request,
    ramp: Ramp,
    grounds: Vec<RGB>,
    colour: f64,
    widgets: f64,
}

impl<'a> Solver<'a> {
    fn far(&self) -> RGB {
        let mut best = self.grounds[0];
        for &g in &self.grounds[1..] {
            if self.s.dark == (luminance(g) > luminance(best)) {
                best = g;
            }
        }
        best
    }

    fn on_wheel(&self, c: LCH) -> LCH {
        match &self.lang.wheel {
            Some(w) => pigment_of(w, c),
            None => c,
        }
    }

    fn seed_of(&self, choice: &str, hue: f64, table: &BTreeMap<&'static str, &'static str>, mut fallback: &'static str) -> LCH {
        let pick = |hexes: &[&str]| -> Option<LCH> {
            best_seed_lch(hexes, 0.03).map(|c| self.on_wheel(c))
        };
        if table.contains_key("design") && self.lang.wheel.is_some() {
            fallback = "design";
        }
        match choice {
            "custom" => return self.on_wheel(hsl(hue, 0.7, 0.5).lch()),
            "wallpaper" => {
                if let Some(c) = pick(&[&self.req.seeds.wallpaper, &self.req.seeds.primary]) {
                    return c;
                }
            }
            "theme" => {
                if let Some(c) = pick(&[&self.req.seeds.primary]) {
                    return c;
                }
            }
            "wallpaperHighlight" => {
                if let Some(c) = pick(&[&self.req.seeds.tertiary, &self.req.seeds.secondary, &self.req.seeds.wallpaper, &self.req.seeds.primary]) {
                    return self.on_wheel(LCH {
                        l: c.l,
                        c: c.c.max(0.11),
                        h: c.h,
                    });
                }
            }
            "themeHighlight" => {
                if let Some(c) = pick(&[&self.req.seeds.tertiary, &self.req.seeds.secondary]) {
                    return self.on_wheel(LCH {
                        l: c.l,
                        c: c.c.max(0.11),
                        h: c.h,
                    });
                }
            }
            _ => {}
        }
        if let Some(&hex) = table.get(choice) {
            return seed_lch(hex).unwrap_or_default();
        }
        seed_lch(table.get(fallback).copied().unwrap_or_default()).unwrap_or_default()
    }

    fn accent(&self, seed: LCH) -> RGB {
        let c = seed.c.clamp(self.s.accent_c[0], self.s.accent_c[1]) * self.colour;
        let papers = self.ramp.all();
        let mut far = papers[0];
        for &g in &papers[1..] {
            if self.s.dark == (luminance(g) > luminance(far)) {
                far = g;
            }
        }
        let check = |col: RGB| {
            reads_on(READ_TEXT, &papers)(col)
                && reads_on(READ_OVER_GLASS, &self.grounds)(col)
                && (self.s.dark || contrast(col, self.lang.on_colour) >= READ_ON)
                && contrast(col, over(col, far, 0.15)) >= READ_WASH
        };
        if self.lang.wheel.is_some() {
            return solve(self.s.dark, seed.h, seed.c.min(self.s.accent_c[1]) * self.colour, seed.l, check);
        }
        solve(self.s.dark, seed.h, c, self.s.accent_l, check)
    }

    fn fill(&self, seed: LCH, need: f64) -> RGB {
        let mut c = seed.c.clamp(self.lang.fill_c[0], self.lang.fill_c[1]) * self.colour.max(0.6);
        let papers = self.ramp.all();
        if self.lang.pigments {
            let shape = if !self.s.dark { READ_PIGMENT } else { READ_SHAPE };
            return solve(self.s.dark, seed.h, c, seed.l, |f| {
                reads_on(shape, &papers)(f)
                    && (contrast(f, self.lang.on_colour) >= READ_ON
                        || contrast(
                            f,
                            (LCH {
                                l: self.lang.dark_on[0],
                                c: self.lang.dark_on[1],
                                h: seed.h,
                            })
                            .rgb(),
                        ) >= READ_ON)
            });
        }
        if !self.s.dark {
            c = c.min(PAPER_FILL_C);
        }
        let from = if self.s.dark {
            self.lang.fill_from[0]
        } else {
            self.lang.fill_from[1]
        };
        solve(self.s.dark, seed.h, c, from, |col| {
            reads_on(need, &papers)(col) && (self.s.dark || contrast(col, self.lang.on_colour) >= READ_ON)
        })
    }

    fn mark(&self, h: f64, c: f64, dl: f64) -> RGB {
        solve(self.s.dark, h, c, self.s.mark_l + dl, reads_on(READ_MARK, &self.grounds))
    }

    fn highlight(&self, seed: LCH) -> RGB {
        let c = seed.c.clamp(self.s.mark_c * 0.7, self.s.mark_c * 1.15) * self.colour;
        if self.lang.pigments {
            let c_pig = c.max(seed.c.min(self.s.mark_c * 1.4) * self.colour);
            return solve(self.s.dark, seed.h, c_pig, seed.l, reads_on(READ_MARK, &self.grounds));
        }
        self.mark(seed.h, c, 0.0)
    }

    fn on_container(&self, cont: RGB, h: f64, c: f64) -> RGB {
        let side_dark = cont.lch().l < 0.6;
        let from = if side_dark { 0.9 } else { 0.32 };
        let far = self.far();
        solve(side_dark, h, c, from, |col| {
            contrast(col, cont) >= READ_ON && (side_dark != self.s.dark || contrast(col, far) >= READ_ON)
        })
    }

    fn on_fill(&self, fill: RGB, need: f64) -> RGB {
        let fl = fill.lch();
        let light = self.lang.on_colour;
        let dark = if self.lang.tonal_on {
            solve(false, fl.h, self.lang.dark_on[1].min(fl.c), self.lang.dark_on[0], |col| {
                contrast(col, fill) >= need
            })
        } else {
            (LCH {
                l: self.lang.dark_on[0],
                c: self.lang.dark_on[1],
                h: fl.h,
            })
            .rgb()
        };
        if !self.s.dark && contrast(light, fill) >= need {
            return light;
        }
        if contrast(dark, fill) >= contrast(light, fill) {
            return dark;
        }
        light
    }

    fn apps(&self, p: &Palette, accent: RGB, highlight: RGB, danger: RGB, cont: RGB) -> BTreeMap<String, String> {
        let mut pc = self.ramp.surface;
        if pc.c < 0.004 {
            pc.h = self.lang.neutral_hue;
        }
        let sign = if self.s.dark { 1.0 } else { -1.0 };
        let mut base = pc;
        if self.s.dark && base.l < APPS_NIGHT_L {
            base = LCH {
                l: APPS_NIGHT_L,
                c: base.c.max(0.006),
                h: base.h,
            };
        }
        if let Some(c) = best_seed_lch(&[&self.req.seeds.wallpaper, &self.req.seeds.primary], 0.02) {
            if self.req.glass && c.c >= 0.02 && self.s.name != "ink" {
                let frost = paper_of(self.lang, self.s, "wallpaper", self.req);
                let max_c = if self.s.dark { 0.024 } else { 0.018 };
                base.h = frost.h;
                base.c = base.c.max(frost.c.min(max_c));
                pc.h = frost.h;
            }
        }
        let at = |mut d: f64, c_scale: f64| -> String {
            let mut q = base;
            if self.s.dark {
                d *= APPS_NIGHT_STEP;
            }
            q.l = (q.l + sign * d).clamp(0.0, 1.0);
            q.c = (q.c * c_scale).min(0.05);
            q.rgb().hex()
        };
        let (al, hl, dl) = (accent.lch(), highlight.lch(), danger.lch());
        let deepest = parse_hex(&at(0.105, 1.15)).unwrap_or_default();
        let mut grounds = vec![deepest];
        grounds.extend_from_slice(&self.grounds);
        let (accent_from, text_grounds) = if !self.s.dark {
            let lowest = parse_hex(&at(-0.012, 0.8)).unwrap_or_default();
            let ground = parse_hex(&at(0.0, 1.0)).unwrap_or_default();
            let card = parse_hex(&at(0.04, 1.05)).unwrap_or_default();
            (self.s.accent_l.max(APPS_PAPER_FROM_L), vec![lowest, ground, card])
        } else {
            (self.s.accent_l, grounds.clone())
        };
        let mut app_ramp = vec![deepest];
        app_ramp.extend_from_slice(&text_grounds);

        let warmth_val = warmth_of(self.s.name, self.req.tune.get(self.s.name)).clamp(0.0, 100.0);
        let warmth_factor = (warmth_val / 100.0) * self.colour.clamp(0.0, 1.0);

        let sec_seed = seed_lch(&self.req.seeds.secondary)
            .filter(|c| c.c >= 0.025)
            .unwrap_or(LCH {
                l: al.l,
                c: (al.c * 0.55).clamp(0.035, 0.07),
                h: hue_toward(al.h, SUMI_HUE, 18.0 * warmth_factor),
            });
        let sec_h = hue_toward(sec_seed.h, SUMI_HUE, 16.0 * warmth_factor);
        let sec_c = (sec_seed.c.max(al.c * 0.5) * self.colour.max(0.5)).clamp(0.03, 0.075);
        let sec = solve(self.s.dark, sec_h, sec_c, accent_from, reads_on(READ_TEXT, &app_ramp));
        let sec_cont = (LCH {
            l: cont.lch().l,
            c: (sec_c * 0.55).min(0.042),
            h: sec_h,
        })
        .rgb();
        let ter_max_c = if self.s.dark { 0.11 } else { 0.09 };
        let ter = solve(self.s.dark, hl.h, hl.c.min(ter_max_c), accent_from, reads_on(READ_TEXT, &app_ramp));
        let ter_cont = (LCH {
            l: cont.lch().l,
            c: (hl.c * 0.45).min(0.07),
            h: hl.h,
        })
        .rgb();
        let err_cont = (LCH {
            l: cont.lch().l,
            c: (dl.c * 0.45).min(0.08),
            h: dl.h,
        })
        .rgb();

        let on_c = |fill: RGB, h: f64, c: f64| -> String {
            let side_dark = fill.lch().l < 0.6;
            let from = if side_dark { 0.9 } else { 0.3 };
            solve(side_dark, h, c, from, |col| contrast(col, fill) >= READ_ON).hex()
        };

        let app_fill = |hex: &str, need: f64, paper_c: f64, night_c: f64| -> RGB {
            let src = parse_hex(hex).unwrap_or_default();
            let l = src.lch();
            let from = if !self.s.dark {
                l.l.max(APPS_PAPER_FROM_L)
            } else {
                l.l
            };
            let max_c = if self.s.dark { night_c } else { paper_c };
            solve(self.s.dark, l.h, l.c.min(max_c), from, |col| {
                let r = if self.s.dark {
                    reads_on(need, &grounds)(col)
                } else {
                    reads_on(need, &text_grounds)(col) && contrast(col, deepest) >= need.min(READ_SHAPE)
                };
                r && (self.s.dark || contrast(col, self.lang.on_colour) >= READ_ON)
            })
        };

        let primary = app_fill(&p.accent_fill, READ_TEXT, APPS_FILL_C[0], APPS_FILL_C[1]);
        let error_fill = app_fill(&p.danger_fill, READ_SHAPE, APPS_ALERT_C[0], APPS_ALERT_C[1]);
        let error_text = app_fill(&p.danger, READ_TEXT, APPS_ALERT_C[0], APPS_ALERT_C[1]);
        let text = parse_hex(&p.text).unwrap_or_default();
        let tl = text.lch();
        let (var_h, var_c, out_c) = if self.lang.kind != LangKind::Airisu && warmth_factor > 0.01 {
            (
                hue_toward(pc.h, SUMI_HUE, 45.0 * warmth_factor),
                ((pc.c * 1.4 + 0.018 * warmth_factor) * self.colour.max(0.6)).min(0.028),
                ((pc.c * 1.2 + 0.014 * warmth_factor) * self.colour.max(0.6)).min(0.022),
            )
        } else {
            (
                pc.h,
                (pc.c * 1.5 + 0.006).min(0.02),
                (pc.c * 1.5 + 0.006).min(0.02),
            )
        };
        let variant = solve(
            self.s.dark,
            var_h,
            var_c,
            pc.l + (tl.l - pc.l) * 0.64,
            reads_on(READ_SECONDARY, &grounds),
        );
        let outline = solve(
            self.s.dark,
            var_h,
            out_c,
            pc.l + (tl.l - pc.l) * 0.42,
            reads_on(READ_TERTIARY, &grounds),
        );
        let on_surface = solve(self.s.dark, tl.h, tl.c, tl.l, reads_on(READ_INK, &grounds)).hex();
        let inverse = if self.s.dark {
            LCH {
                l: 0.93,
                c: 0.008,
                h: pc.h,
            }
        } else {
            LCH {
                l: 0.27,
                c: 0.012,
                h: pc.h,
            }
        }
        .rgb();
        let inv_from = if self.s.dark { 0.5 } else { 0.8 };
        let inverse_primary = solve(self.s.dark, al.h, al.c, inv_from, |col| contrast(col, inverse) >= READ_TEXT);

        BTreeMap::from([
            ("background".to_string(), at(0.0, 1.0)),
            ("surface".to_string(), at(0.0, 1.0)),
            ("surfaceDim".to_string(), at(-0.03 * sign, 1.0)),
            ("surfaceBright".to_string(), at(0.035 * sign, 1.0)),
            ("surfaceContainerLowest".to_string(), at(-0.012, 0.8)),
            ("surfaceContainerLow".to_string(), at(0.016, 1.0)),
            ("surfaceContainer".to_string(), at(0.038, 1.05)),
            ("surfaceContainerHigh".to_string(), at(0.062, 1.1)),
            ("surfaceContainerHighest".to_string(), at(0.088, 1.15)),
            ("surfaceVariant".to_string(), at(0.062, 1.2)),
            ("onBackground".to_string(), on_surface.clone()),
            ("onSurface".to_string(), on_surface),
            ("onSurfaceVariant".to_string(), variant.hex()),
            ("outline".to_string(), outline.hex()),
            ("outlineVariant".to_string(), at(0.15, 1.2)),
            ("inverseSurface".to_string(), inverse.hex()),
            ("inverseOnSurface".to_string(), at(0.0, 1.0)),
            ("primary".to_string(), primary.hex()),
            ("onPrimary".to_string(), self.on_fill(primary, READ_ON).hex()),
            ("primaryContainer".to_string(), p.accent_container.clone()),
            ("onPrimaryContainer".to_string(), p.on_accent_container.clone()),
            ("inversePrimary".to_string(), inverse_primary.hex()),
            ("surfaceTint".to_string(), primary.hex()),
            ("secondary".to_string(), sec.hex()),
            ("onSecondary".to_string(), self.on_fill(sec, READ_ON).hex()),
            ("secondaryContainer".to_string(), sec_cont.hex()),
            ("onSecondaryContainer".to_string(), on_c(sec_cont, sec_h, 0.05)),
            ("tertiary".to_string(), ter.hex()),
            ("onTertiary".to_string(), self.on_fill(ter, READ_ON).hex()),
            ("tertiaryContainer".to_string(), ter_cont.hex()),
            ("onTertiaryContainer".to_string(), on_c(ter_cont, hl.h, 0.1)),
            ("error".to_string(), error_text.hex()),
            ("onError".to_string(), self.on_fill(error_text, READ_ON).hex()),
            ("errorFill".to_string(), error_fill.hex()),
            ("onErrorFill".to_string(), self.on_fill(error_fill, READ_ON).hex()),
            ("errorContainer".to_string(), err_cont.hex()),
            ("onErrorContainer".to_string(), on_c(err_cont, dl.h, 0.12)),
            ("success".to_string(), p.success.clone()),
            ("shadow".to_string(), "#000000".to_string()),
            ("scrim".to_string(), "#000000".to_string()),
        ])
    }
}

pub fn build_palette(req: &Request, name: &str) -> Palette {
    let lang = scheme_language(req, name);
    let s = lang.spec_of(name);
    let tune = req.tune.get(name);
    let colour = tune
        .and_then(|t| t.colour)
        .map(|c| (c / 100.0).clamp(0.0, 1.0))
        .unwrap_or(1.0);
    let widgets = tune
        .and_then(|t| t.widgets)
        .map(|w| (w / 100.0).clamp(0.4, 1.6))
        .unwrap_or(s.widgets);
    let material = if s.materials.contains_key(req.material.as_str())
        || req.material == "wallpaper"
        || req.material == "theme"
    {
        req.material.clone()
    } else {
        "black".to_string()
    };
    let ramp = lang.ramp(&s, paper_of(&lang, &s, &material, req));
    let mut v = Solver {
        lang: &lang,
        s: &s,
        req,
        ramp,
        grounds: ramp.all().to_vec(),
        colour,
        widgets,
    };

    let mut pc = ramp.surface;
    if pc.c < 0.004 || lang.fixed_ink {
        pc.h = lang.neutral_hue;
    }
    let ink_c = lang.ink_chroma(&s, pc);
    let mut text = solve(s.dark, pc.h, ink_c, s.ink_l, reads_on(READ_INK, &v.grounds));
    if !s.dark {
        let paper_grounds = v.grounds.clone();
        for _ in 0..4 {
            let mut next_grounds = paper_grounds.clone();
            next_grounds.extend_from_slice(&glass_grounds(paper_grounds[0], text));
            v.grounds = next_grounds;
            let next = solve(s.dark, pc.h, ink_c, s.ink_l, reads_on(READ_INK, &v.grounds));
            if next == text {
                break;
            }
            text = next;
        }
        let mut final_grounds = paper_grounds.clone();
        final_grounds.extend_from_slice(&glass_grounds(paper_grounds[0], text));
        v.grounds = final_grounds;
    }

    let accent_choice = if req.accent.is_empty() {
        "blue"
    } else {
        req.accent.as_str()
    };
    let accent_seed = v.seed_of(accent_choice, req.accent_hue, &lang.accent_seeds, "blue");
    let accent = v.accent(accent_seed);
    let al = accent.lch();
    let cont = lang.container(&s, pc, al).rgb();
    let on_accent_container = v.on_container(cont, al.h, al.c.min(0.12)).hex();

    let warmth_val = warmth_of(name, tune).clamp(0.0, 100.0);
    let warmth_factor = (warmth_val / 100.0) * colour;
    let quiet = |need: f64, from: f64, extra_c: f64| -> RGB {
        let (qh, qc) = if !lang.fixed_ink && warmth_factor > 0.01 {
            (
                hue_toward(pc.h, SUMI_HUE, 42.0 * warmth_factor),
                (ink_c * 1.5 + extra_c * warmth_factor).min(0.024),
            )
        } else {
            (pc.h, ink_c * 1.5)
        };
        solve(
            s.dark,
            qh,
            qc,
            ramp.surface.l + (text.lch().l - ramp.surface.l) * from,
            reads_on(need, &v.grounds),
        )
    };
    let text_secondary = quiet(READ_SECONDARY, 0.64, 0.014).hex();
    let text_tertiary = quiet(READ_TERTIARY, 0.46, 0.010).hex();

    let highlight_choice = req.highlight.as_str();
    let (highlight, highlight_seed) = match highlight_choice {
        "accent" => (accent, accent_seed),
        "wallpaper" => {
            let hs = v.seed_of("wallpaperHighlight", 0.0, &lang.highlight_seeds, "orange");
            (v.highlight(hs), hs)
        }
        "theme" => {
            let hs = v.seed_of("themeHighlight", 0.0, &lang.highlight_seeds, "orange");
            (v.highlight(hs), hs)
        }
        "custom" => {
            let hs = v.on_wheel(hsl(req.highlight_hue, 0.92, 0.58).lch());
            (v.highlight(hs), hs)
        }
        _ => {
            let hs = v.seed_of(highlight_choice, 0.0, &lang.highlight_seeds, "orange");
            (v.highlight(hs), hs)
        }
    };

    let status = |h: f64, c: f64, need: f64, carries_ink: bool| -> RGB {
        solve(s.dark, h, c * colour.max(0.6), s.mark_l, |col| {
            reads_on(need, &v.grounds)(col) && (!carries_ink || s.dark || contrast(col, lang.on_colour) >= READ_ON)
        })
    };
    let success = status(150.0, 0.13, READ_TEXT, false).hex();
    let warning = status(72.0, 0.14, READ_MARK, false).hex();
    let (danger_h, danger_fill_seed) = if !lang.danger_seed.is_empty() {
        let ds = hex_lch(lang.danger_seed);
        (ds.h, ds)
    } else {
        (27.0, LCH { l: 0.55, c: 0.15, h: 33.0 })
    };
    let danger = status(danger_h, 0.17, READ_TEXT, true);

    let mut seal_seed = accent_seed;
    if v.lang.pigments && v.lang.wheel.is_none() {
        seal_seed.l = seal_seed.l.min(v.s.accent_l);
    }
    let accent_fill = v.fill(seal_seed, READ_TEXT);
    let danger_fill = v.fill(danger_fill_seed, READ_SHAPE);
    let mut highlight_fill = if lang.pigments {
        v.fill(highlight_seed, READ_SHAPE)
    } else {
        v.fill(
            LCH {
                l: 0.6,
                c: highlight.lch().c.max(0.1),
                h: highlight.lch().h,
            },
            READ_SHAPE,
        )
    };
    if highlight_choice == "accent" {
        highlight_fill = accent_fill;
    }

    let mut identity = BTreeMap::new();
    let id_c = s.mark_c * match name {
        "ink" => 0.8,
        _ => 0.86,
    };
    for &(id_name, id_hex) in lang.identity_hex {
        let c = seed_lch(id_hex).unwrap_or_default();
        let k = match name {
            "dark" => 1.0,
            "ink" => 0.75,
            _ => 0.9,
        };
        identity.insert(id_name.to_string(), v.mark(c.h, c.c * k, c.l - s.mark_l).hex());
    }
    for id in lang.identity {
        if identity.contains_key(id.name) {
            continue;
        }
        if id.hue < 0.0 {
            identity.insert(
                id.name.to_string(),
                solve(s.dark, pc.h, 0.012, s.mark_l, reads_on(READ_MARK, &v.grounds)).hex(),
            );
            continue;
        }
        let mut h = id.hue;
        if al.c >= 0.04 {
            h = hue_toward(h, al.h, lang.identity_pull);
        }
        identity.insert(id.name.to_string(), v.mark(h, id_c + id.c, id.l).hex());
    }

    let vib = req.vibrance.clamp(0.0, 1.0);
    let k = widgets / s.widgets * (0.8 + 0.4 * vib);
    let data = |h: f64, c: f64, dl: f64| -> RGB { v.mark(h, (c * k).min(s.mark_c * 1.3), dl) };
    let hue_or = |hex: &str, fallback: f64| -> f64 {
        seed_lch(hex)
            .filter(|c| c.c >= 0.02)
            .map(|c| c.h)
            .unwrap_or(fallback)
    };
    let ph = hue_or(&req.seeds.primary, al.h);
    let mut th = hue_or(&req.seeds.tertiary, (ph + 60.0).rem_euclid(360.0));
    if hue_distance(th, ph) < 24.0 {
        let mut far = 0.0;
        for h in [262.0, 160.0, 85.0] {
            let d = hue_distance(h, ph);
            if d > far {
                far = d;
                th = h;
            }
        }
    }
    let sh = hue_or(&req.seeds.secondary, ph);
    let third = if hue_distance(sh, ph) < 24.0 {
        data(ph, s.mark_c * 0.6, if s.dark { 0.12 } else { -0.14 })
    } else {
        data(sh, s.mark_c * 0.75, 0.0)
    };
    let hl = highlight.lch();
    let id_lch = |n: &str| -> LCH {
        parse_hex(identity.get(n).map(String::as_str).unwrap_or_default())
            .unwrap_or_default()
            .lch()
    };
    let widgets_map = BTreeMap::from([
        (
            "wallpaper".to_string(),
            vec![data(ph, s.mark_c, 0.0).hex(), data(th, s.mark_c, 0.0).hex(), third.hex()],
        ),
        (
            "system".to_string(),
            vec![
                data(al.h, s.mark_c, 0.0).hex(),
                data(hl.h, s.mark_c, 0.0).hex(),
                data(150.0, s.mark_c * 0.9, 0.0).hex(),
            ],
        ),
        (
            "spectrum".to_string(),
            vec![
                data(id_lch("blue").h, s.mark_c, 0.0).hex(),
                data(id_lch("orange").h, s.mark_c, 0.0).hex(),
                data(id_lch("teal").h, s.mark_c, 0.0).hex(),
            ],
        ),
        (
            "mono".to_string(),
            vec![
                data(al.h, s.mark_c, 0.0).hex(),
                data(al.h, s.mark_c * 0.7, if s.dark { 0.12 } else { -0.16 }).hex(),
                data(al.h, s.mark_c * 0.5, if s.dark { -0.1 } else { 0.1 }).hex(),
            ],
        ),
    ]);

    let mut materials_map = BTreeMap::new();
    for &m in lang.material_names {
        materials_map.insert(m.to_string(), paper_of(&lang, &s, m, req).rgb().hex());
    }

    let papers = paper_levels(name).map(|levels| {
        let mut pm = BTreeMap::new();
        for l in levels {
            let mut other = req.clone();
            let mut t = other.tune.get(name).cloned().unwrap_or_default();
            t.tone = l.tone;
            t.warmth = Some(l.warmth);
            other.tune.insert(name.to_string(), t);
            other.dark_style = "ink".to_string();
            let sl = scheme_language(&other, name);
            pm.insert(l.name.to_string(), paper_of(&sl, &s, &material, &other).rgb().hex());
        }
        pm
    });

    let mut accents_map = BTreeMap::new();
    for (&n, &hex) in &lang.accent_seeds {
        let c = seed_lch(hex).unwrap_or_default();
        accents_map.insert(n.to_string(), v.accent(c).hex());
    }
    for n in ["wallpaper", "theme", "custom"] {
        accents_map.insert(
            n.to_string(),
            v.accent(v.seed_of(n, req.accent_hue, &lang.accent_seeds, "blue")).hex(),
        );
    }

    let mut highlights_map = BTreeMap::from([("accent".to_string(), accent.hex())]);
    for (&n, &hex) in &lang.highlight_seeds {
        let c = seed_lch(hex).unwrap_or_default();
        highlights_map.insert(n.to_string(), v.highlight(c).hex());
    }
    highlights_map.insert(
        "wallpaper".to_string(),
        v.highlight(v.seed_of("wallpaperHighlight", 0.0, &lang.highlight_seeds, "orange"))
            .hex(),
    );
    highlights_map.insert(
        "theme".to_string(),
        v.highlight(v.seed_of("themeHighlight", 0.0, &lang.highlight_seeds, "orange"))
            .hex(),
    );
    highlights_map.insert(
        "custom".to_string(),
        v.highlight(hsl(req.highlight_hue, 0.92, 0.58).lch()).hex(),
    );

    let mut p = Palette {
        dark: s.dark,
        material,
        surface: v.grounds[0].hex(),
        surface_high: v.grounds[1].hex(),
        surface_highest: v.grounds[2].hex(),
        text: text.hex(),
        text_secondary,
        text_tertiary,
        fill_ink: text.hex(),
        accent: accent.hex(),
        on_accent: v.on_fill(accent, READ_ON).hex(),
        accent_container: cont.hex(),
        on_accent_container,
        highlight: highlight.hex(),
        on_highlight: v.on_fill(highlight, READ_ON).hex(),
        success,
        warning,
        danger: danger.hex(),
        on_danger: v.on_fill(danger, READ_ON).hex(),
        accent_fill: accent_fill.hex(),
        on_accent_fill: v.on_fill(accent_fill, READ_ON).hex(),
        danger_fill: danger_fill.hex(),
        on_danger_fill: v.on_fill(danger_fill, READ_ON).hex(),
        highlight_fill: highlight_fill.hex(),
        on_highlight_fill: v.on_fill(highlight_fill, READ_ON).hex(),
        identity,
        widgets: widgets_map,
        materials: materials_map,
        accents: accents_map,
        highlights: highlights_map,
        apps: BTreeMap::new(),
        papers,
    };
    p.apps = v.apps(&p, accent, highlight, danger, cont);
    p
}

pub fn solve_schemes(req: &Request, key: &str) -> Output {
    let lang = build_language(language_kind(req));
    let mut schemes = BTreeMap::new();
    for s in &lang.schemes {
        schemes.insert(s.name.to_string(), build_palette(req, s.name));
    }
    Output {
        version: VERSION,
        request: key.to_string(),
        schemes,
        styles: BTreeMap::new(),
    }
}

pub fn solve_all(req: &Request, key: &str) -> Output {
    let mut out = solve_schemes(req, key);
    let mut style_ids = vec!["iris".to_string(), "washi".to_string()];
    for v in MATERIAL_VARIANTS {
        style_ids.push(format!("material:{}", v.name));
    }
    for id in style_ids {
        let mut other = req.clone();
        other.ink_style = "style".to_string();
        other.dark_style = "style".to_string();
        if let Some(var) = id.strip_prefix("material:") {
            other.language = "material".to_string();
            other.variant = var.to_string();
        } else {
            other.language = id.clone();
            other.variant = String::new();
        }
        let other_lang = build_language(language_kind(&other));
        let mut swatches = BTreeMap::new();
        for s in &other_lang.schemes {
            let p = if scheme_language(&other, s.name).kind != scheme_language(req, s.name).kind {
                build_palette(&other, s.name)
            } else {
                out.schemes[s.name].clone()
            };
            swatches.insert(
                s.name.to_string(),
                StyleSwatch {
                    surface: p.surface.clone(),
                    high: p.surface_high.clone(),
                    text: p.text.clone(),
                    accent: p.accent.clone(),
                    dots: vec![
                        p.identity.get("blue").cloned().unwrap_or_default(),
                        p.identity.get("green").cloned().unwrap_or_default(),
                        p.identity.get("orange").cloned().unwrap_or_default(),
                    ],
                },
            );
        }
        out.styles.insert(id, swatches);
    }
    out
}
