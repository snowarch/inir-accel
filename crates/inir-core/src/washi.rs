//! iRiS's palette solver, ported line for line from the shell's Go (`scripts/colors/washi`, Version 30): the same
//! request gives the same bytes (test/parity.py holds the two together). Every role of every scheme (dark, ink, light)
//! comes from what the person chose and the seeds the wallpaper and the colour theme give.
//!
//! The one place this port may differ is opt-in: `Options::warm_neutrals` gives the quiet neutrals (secondary and
//! tertiary text, the apps' variant and outline) a sand undertone. Off, the output is the Go solver's.

use crate::gojson::{self, Value};
use crate::gomath;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub const VERSION: i32 = 32;

// ---------------------------------------------------------------- colour (colour.go)

/// Gamma-encoded sRGB, 0..1.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RGB {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

/// OKLCH: perceptual lightness 0..1, chroma, hue in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LCH {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

/// Go's math.Max and math.Min: NaN wins, and +0 beats -0 (Max) or -0 beats +0 (Min).
fn gmax(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        f64::NAN
    } else if x == 0.0 && y == 0.0 {
        if x.is_sign_negative() { y } else { x }
    } else if x > y {
        x
    } else {
        y
    }
}

fn gmin(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        f64::NAN
    } else if x == 0.0 && y == 0.0 {
        if x.is_sign_negative() { x } else { y }
    } else if x < y {
        x
    } else {
        y
    }
}

/// Go's int(math.Round(v)) on amd64, NaN and overflow included.
fn go_int(v: f64) -> i64 {
    let r = v.round();
    if r.is_nan() || r >= 9.223372036854775807e18 || r < -9.223372036854775808e18 { i64::MIN } else { r as i64 }
}

pub fn parse_hex(s: &str) -> Option<RGB> {
    let t = s.trim();
    let t = t.strip_prefix('#').unwrap_or(t).as_bytes();
    let t = if t.len() == 8 { &t[2..] } else { t }; // #AARRGGBB as Qt prints it
    if t.len() != 6 || !t.iter().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(std::str::from_utf8(t).ok()?, 16).ok()?;
    Some(RGB { r: ((v >> 16) & 0xff) as f64 / 255.0, g: ((v >> 8) & 0xff) as f64 / 255.0, b: (v & 0xff) as f64 / 255.0 })
}

impl RGB {
    pub fn hex(self) -> String {
        let b = |v: f64| go_int(gmax(0.0, gmin(1.0, v)) * 255.0);
        let f = |n: i64| if n < 0 { format!("-{:02x}", n.unsigned_abs()) } else { format!("{n:02x}") };
        format!("#{}{}{}", f(b(self.r)), f(b(self.g)), f(b(self.b)))
    }

    pub fn lab(self) -> (f64, f64, f64) {
        let (r, g, b) = (to_linear(self.r), to_linear(self.g), to_linear(self.b));
        let l = gomath::cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
        let m = gomath::cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
        let s = gomath::cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
        (
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        )
    }

    pub fn lch(self) -> LCH {
        let (l, a, b) = self.lab();
        let mut h = gomath::atan2(b, a) * 180.0 / std::f64::consts::PI;
        if h < 0.0 {
            h += 360.0;
        }
        LCH { l, c: gomath::hypot(a, b), h }
    }
}

pub fn to_linear(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { gomath::pow((v + 0.055) / 1.055, 2.4) }
}

pub fn to_gamma(v: f64) -> f64 {
    if v <= 0.0031308 { 12.92 * v } else { 1.055 * gomath::pow(v, 5.0 / 12.0) - 0.055 }
}

pub fn from_lab(ll: f64, a: f64, b: f64) -> RGB {
    let l = ll + 0.3963377774 * a + 0.2158037573 * b;
    let m = ll - 0.1055613458 * a - 0.0638541728 * b;
    let s = ll - 0.0894841775 * a - 1.2914855480 * b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    RGB {
        r: to_gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        g: to_gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        b: to_gamma(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    }
}

impl LCH {
    pub fn raw(self) -> RGB {
        let r = self.h * std::f64::consts::PI / 180.0;
        from_lab(self.l, self.c * gomath::cos(r), self.c * gomath::sin(r))
    }

    /// Maps into sRGB by giving up chroma, never lightness or hue.
    pub fn rgb(mut self) -> RGB {
        self.l = gmax(0.0, gmin(1.0, self.l));
        let c = self.raw();
        if in_gamut(c) {
            return clamp_rgb(c);
        }
        self.c = max_chroma(self.l, self.h);
        clamp_rgb(self.raw())
    }
}

pub fn in_gamut(c: RGB) -> bool {
    const E: f64 = 1e-4;
    c.r >= -E && c.r <= 1.0 + E && c.g >= -E && c.g <= 1.0 + E && c.b >= -E && c.b <= 1.0 + E
}

/// The most chroma sRGB holds at this lightness and hue.
pub fn max_chroma(l: f64, h: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 0.4);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if in_gamut(LCH { l, c: mid, h }.raw()) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Also quantizes to 8 bits: a contrast is solved on the colour that will be written, not a finer one.
pub fn clamp_rgb(c: RGB) -> RGB {
    let f = |v: f64| (gmax(0.0, gmin(1.0, v)) * 255.0).round() / 255.0;
    RGB { r: f(c.r), g: f(c.g), b: f(c.b) }
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
    let mut q = l + s - l * s;
    if l < 0.5 {
        q = l * (1.0 + s);
    }
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
    RGB { r: f(h + 1.0 / 3.0), g: f(h), b: f(h - 1.0 / 3.0) }
}

/// Turns h toward target by half the distance, at most limit degrees (Material's harmonize).
pub fn hue_toward(h: f64, target: f64, limit: f64) -> f64 {
    let d = (target - h + 540.0) % 360.0 - 180.0;
    let mut step = gmin(d.abs() * 0.5, limit);
    if d < 0.0 {
        step = -step;
    }
    (h + step + 360.0) % 360.0
}

pub fn hue_distance(a: f64, b: f64) -> f64 {
    let d = ((a - b + 360.0) % 360.0).abs();
    gmin(d, 360.0 - d)
}

fn clamp_f(v: f64, lo: f64, hi: f64) -> f64 {
    gmax(lo, gmin(hi, v))
}

fn seed_lch(hex: &str) -> Option<LCH> {
    parse_hex(hex).map(RGB::lch)
}

fn hex_lch(hex: &str) -> LCH {
    seed_lch(hex).unwrap_or_default()
}

// ---------------------------------------------------------------- the request

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seeds {
    pub wallpaper: String,
    pub primary: String,
    pub secondary: String,
    pub tertiary: String,
    pub background: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tune {
    pub tone: f64,
    pub colour: Option<f64>,
    pub widgets: Option<f64>,
    /// Paper schemes: how much washi fibre the paper carries, 0..100.
    pub warmth: Option<f64>,
}

/// Everything iRiS chose that colours it, plus the seeds read from the wallpaper and the colour theme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub language: String,
    pub variant: String,
    pub material: String,
    pub glass: bool,
    pub ink_style: String,
    pub dark_style: String,
    pub accent: String,
    pub accent_hue: f64,
    pub highlight: String,
    pub highlight_hue: f64,
    pub vibrance: f64,
    /// `None` is Go's nil map (a request that sent `"tune": null`).
    pub tune: Option<BTreeMap<String, Tune>>,
    pub seeds: Seeds,
}

impl Request {
    fn tune_of(&self, name: &str) -> Tune {
        self.tune.as_ref().and_then(|t| t.get(name).copied()).unwrap_or_default()
    }
}

/// The palette of a fresh install. Light's fresh paper is Soft (defaults/config.json tune.light).
pub fn default_request() -> Request {
    let mut tune = BTreeMap::new();
    tune.insert("light".to_string(), Tune { tone: -8.0, warmth: Some(38.0), ..Tune::default() });
    Request {
        language: "iris".into(),
        material: "black".into(),
        accent: "blue".into(),
        accent_hue: 212.0,
        highlight: "orange".into(),
        highlight_hue: 32.0,
        vibrance: 0.85,
        tune: Some(tune),
        ..Request::default()
    }
}

/// Decodes a request over the defaults exactly as Go's json.Unmarshal does into `defaultRequest()`.
pub fn decode_request(raw: &str) -> Result<Request, String> {
    let mut req = default_request();
    let obj = match gojson::parse(raw)? {
        Value::Null => return Ok(req),
        Value::Object(o) => o,
        other => return Err(format!("json: cannot unmarshal {} into Go value of type main.Request", gojson::kind(&other))),
    };
    let mut first_err: Option<String> = None;
    let mut keep = |r: Result<(), String>| {
        if let Err(e) = r {
            first_err.get_or_insert(e);
        }
    };
    let set_str = |obj: &[(String, Value)], name: &str, dst: &mut String| -> Result<(), String> {
        for v in gojson::fields(obj, name) {
            if let Some(s) = gojson::string(v)? {
                *dst = s;
            }
        }
        Ok(())
    };
    let set_f64 = |obj: &[(String, Value)], name: &str, dst: &mut f64| -> Result<(), String> {
        for v in gojson::fields(obj, name) {
            if let Some(f) = gojson::number(v)? {
                *dst = f;
            }
        }
        Ok(())
    };
    keep(set_str(&obj, "language", &mut req.language));
    keep(set_str(&obj, "variant", &mut req.variant));
    keep(set_str(&obj, "material", &mut req.material));
    for v in gojson::fields(&obj, "glass") {
        match gojson::boolean(v) {
            Ok(Some(b)) => req.glass = b,
            Ok(None) => {}
            Err(e) => keep(Err(e)),
        }
    }
    keep(set_str(&obj, "inkStyle", &mut req.ink_style));
    keep(set_str(&obj, "darkStyle", &mut req.dark_style));
    keep(set_str(&obj, "accent", &mut req.accent));
    keep(set_f64(&obj, "accentHue", &mut req.accent_hue));
    keep(set_str(&obj, "highlight", &mut req.highlight));
    keep(set_f64(&obj, "highlightHue", &mut req.highlight_hue));
    keep(set_f64(&obj, "vibrance", &mut req.vibrance));
    for v in gojson::fields(&obj, "tune") {
        match v {
            Value::Null => req.tune = None,
            Value::Object(entries) => {
                let map = req.tune.get_or_insert_with(BTreeMap::new);
                for (key, tv) in entries {
                    let mut t = Tune::default();
                    match tv {
                        Value::Null => {}
                        Value::Object(fields) => {
                            keep(set_f64(fields, "tone", &mut t.tone));
                            for (name, dst) in [("colour", &mut t.colour), ("widgets", &mut t.widgets), ("warmth", &mut t.warmth)] {
                                for fv in gojson::fields(fields, name) {
                                    match gojson::number(fv) {
                                        Ok(n) => *dst = n,
                                        Err(e) => keep(Err(e)),
                                    }
                                }
                            }
                        }
                        other => keep(Err(format!("json: cannot unmarshal {} into Go value of type main.Tune", gojson::kind(other)))),
                    }
                    map.insert(key.clone(), t);
                }
            }
            other => keep(Err(format!("json: cannot unmarshal {} into Go struct field Request.tune", gojson::kind(other)))),
        }
    }
    for v in gojson::fields(&obj, "seeds") {
        match v {
            Value::Null => {}
            Value::Object(fields) => {
                keep(set_str(fields, "wallpaper", &mut req.seeds.wallpaper));
                keep(set_str(fields, "primary", &mut req.seeds.primary));
                keep(set_str(fields, "secondary", &mut req.seeds.secondary));
                keep(set_str(fields, "tertiary", &mut req.seeds.tertiary));
                keep(set_str(fields, "background", &mut req.seeds.background));
            }
            other => keep(Err(format!("json: cannot unmarshal {} into Go struct field Request.seeds", gojson::kind(other)))),
        }
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(req),
    }
}

/// A request as Go's json.Marshal writes it (the key of `--default`).
pub fn encode_request(req: &Request) -> String {
    let mut o = String::from("{");
    let s = |o: &mut String, k: &str, v: &str| {
        gojson::write_str(o, k);
        o.push(':');
        gojson::write_str(o, v);
        o.push(',');
    };
    let f = |o: &mut String, k: &str, v: f64| {
        gojson::write_str(o, k);
        o.push(':');
        gojson::write_f64(o, v);
        o.push(',');
    };
    if !req.language.is_empty() {
        s(&mut o, "language", &req.language);
    }
    if !req.variant.is_empty() {
        s(&mut o, "variant", &req.variant);
    }
    s(&mut o, "material", &req.material);
    if req.glass {
        o.push_str("\"glass\":true,");
    }
    if !req.ink_style.is_empty() {
        s(&mut o, "inkStyle", &req.ink_style);
    }
    if !req.dark_style.is_empty() {
        s(&mut o, "darkStyle", &req.dark_style);
    }
    s(&mut o, "accent", &req.accent);
    f(&mut o, "accentHue", req.accent_hue);
    s(&mut o, "highlight", &req.highlight);
    f(&mut o, "highlightHue", req.highlight_hue);
    f(&mut o, "vibrance", req.vibrance);
    o.push_str("\"tune\":");
    match &req.tune {
        None => o.push_str("null"),
        Some(map) => {
            o.push('{');
            for (i, (k, t)) in map.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                gojson::write_str(&mut o, k);
                o.push_str(":{\"tone\":");
                gojson::write_f64(&mut o, t.tone);
                for (name, v) in [("colour", t.colour), ("widgets", t.widgets), ("warmth", t.warmth)] {
                    o.push_str(&format!(",\"{name}\":"));
                    match v {
                        Some(x) => gojson::write_f64(&mut o, x),
                        None => o.push_str("null"),
                    }
                }
                o.push('}');
            }
            o.push('}');
        }
    }
    o.push_str(",\"seeds\":{");
    let seeds = [
        ("wallpaper", &req.seeds.wallpaper),
        ("primary", &req.seeds.primary),
        ("secondary", &req.seeds.secondary),
        ("tertiary", &req.seeds.tertiary),
        ("background", &req.seeds.background),
    ];
    for (i, (k, v)) in seeds.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        gojson::write_str(&mut o, k);
        o.push(':');
        gojson::write_str(&mut o, v);
    }
    o.push_str("}}");
    o
}

/// What a solve may do beyond the Go solver. Default is none: the Go output, byte for byte.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options {
    /// The quiet neutrals (secondary and tertiary text, the apps' onSurfaceVariant and outline) lean toward washi's
    /// sand hue by the scheme's warmth (docs/COLOR_WARMTH.md).
    pub warm_neutrals: bool,
}

impl Options {
    /// The person's choice: `INIR_ACCEL_WARM_NEUTRALS` (1/0) over `$XDG_CONFIG_HOME/inir-accel/options.json`
    /// (`{"warmNeutrals": true}`). Absent, off.
    pub fn from_env() -> Options {
        if let Ok(v) = std::env::var("INIR_ACCEL_WARM_NEUTRALS") {
            return Options { warm_neutrals: matches!(v.trim(), "1" | "true" | "on" | "yes") };
        }
        let dir = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
        let warm = std::fs::read_to_string(format!("{dir}/inir-accel/options.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("warmNeutrals").and_then(|b| b.as_bool()))
            .unwrap_or(false);
        Options { warm_neutrals: warm }
    }
}

// ---------------------------------------------------------------- language.go

// The contrasts every role is held to, in every language.
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

struct IdentitySeed {
    name: &'static str,
    hue: f64,
    l: f64,
    c: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LangId {
    Washi,
    Iris,
    Airisu,
    Material(usize),
}

struct Language {
    id: LangId,
    schemes: Vec<Spec>,
    material_names: &'static [&'static str],
    accent_seeds: &'static [(&'static str, &'static str)],
    highlight_seeds: &'static [(&'static str, &'static str)],
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

fn table_get(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

impl Language {
    fn spec_of(&self, name: &str) -> &Spec {
        self.schemes.iter().find(|s| s.name == name).unwrap_or(&self.schemes[0])
    }

    fn paper(&self, s: &Spec, name: &str, seeds: &Seeds, tone: f64) -> LCH {
        match self.id {
            LangId::Washi => paper(s, name, seeds, tone),
            LangId::Iris => {
                let mut p = paper(s, name, seeds, tone);
                if name == "wallpaper" || name == "theme" {
                    // A seed's material keeps more of its colour than washi's fibre.
                    if let Some(c) = seed_lch(&seeds.wallpaper) {
                        if name == "wallpaper" && c.c >= 0.02 {
                            p.c = gmin(s.paper_c, c.c * 0.45);
                        }
                    }
                }
                p
            }
            LangId::Airisu => airisu_paper(s, name, seeds, tone),
            LangId::Material(v) => material_paper(&VARIANTS[v], s, name, seeds, tone),
        }
    }

    fn ramp(&self, s: &Spec, p: LCH) -> Ramp {
        match self.id {
            LangId::Washi => {
                let step = |i: usize| {
                    if s.dark && p.l < 0.05 {
                        // Black has no hue to keep: its groups are sumi greys.
                        return LCH { l: [0.235, 0.305][i], c: 0.006, h: SUMI_HUE };
                    }
                    LCH { l: clamp_f(p.l + s.steps[i], 0.0, 1.0), c: p.c * 1.1, h: p.h }
                };
                Ramp { surface: p, high: step(0), highest: step(1) }
            }
            LangId::Iris => {
                let step = |i: usize| {
                    if s.dark && p.l < 0.05 {
                        return LCH { l: [0.227, 0.294][i], c: 0.004, h: IRIS_HUE };
                    }
                    if !s.dark && p.l > 0.995 {
                        return LCH { l: [0.963, 0.923][i], c: 0.007, h: IRIS_HUE };
                    }
                    LCH { l: clamp_f(p.l + s.steps[i], 0.0, 1.0), c: p.c, h: p.h }
                };
                Ramp { surface: p, high: step(0), highest: step(1) }
            }
            LangId::Airisu => {
                let step = |i: usize| LCH {
                    l: clamp_f(p.l + s.steps[i], 0.0, 1.0),
                    c: gmin(0.032, gmax(p.c, 0.003) * [1.3, 1.8][i]),
                    h: p.h,
                };
                Ramp { surface: p, high: step(0), highest: step(1) }
            }
            LangId::Material(_) => {
                let c = gmin(0.09, gmax(p.c, s.paper_c * 0.35) * 1.2);
                let step = |i: usize| {
                    if s.dark && p.l < 0.05 {
                        return LCH { l: [0.214, 0.269][i], c, h: p.h };
                    }
                    LCH { l: clamp_f(p.l + s.steps[i], 0.0, 1.0), c, h: p.h }
                };
                Ramp { surface: p, high: step(0), highest: step(1) }
            }
        }
    }

    fn container(&self, s: &Spec, pc: LCH, al: LCH) -> LCH {
        match self.id {
            LangId::Washi => {
                if s.dark {
                    return LCH { l: gmax(0.32, pc.l + 0.16), c: gmin(0.05, al.c * 0.45), h: al.h };
                }
                LCH { l: pc.l - 0.075, c: gmin(0.042, al.c * 0.4), h: al.h }
            }
            LangId::Iris => {
                if s.dark {
                    return LCH { l: gmax(0.3, pc.l + 0.13), c: gmin(0.07, al.c * 0.6), h: al.h };
                }
                LCH { l: pc.l - 0.08, c: gmin(0.06, al.c * 0.35), h: al.h }
            }
            LangId::Airisu => {
                if s.dark {
                    return LCH { l: gmax(0.3, pc.l + 0.15), c: gmin(0.08, al.c * 0.45), h: al.h };
                }
                LCH { l: pc.l - 0.08, c: gmin(0.06, al.c * 0.35), h: al.h }
            }
            LangId::Material(v) => {
                let v = &VARIANTS[v];
                let mut l = gmax(v.container_l[0], pc.l + 0.16);
                if !s.dark {
                    l = gmin(v.container_l[1], pc.l - 0.075);
                }
                LCH { l, c: gmin(v.container_c, gmax(al.c, v.accent_c[0])), h: al.h }
            }
        }
    }

    fn ink_chroma(&self, s: &Spec, pc: LCH) -> f64 {
        match self.id {
            LangId::Washi => {
                if s.dark {
                    gmin(0.012, pc.c * 0.3 + 0.008)
                } else {
                    gmin(0.014, pc.c * 0.8 + 0.004)
                }
            }
            LangId::Iris => {
                if s.name == "ink" {
                    0.012
                } else {
                    0.003
                }
            }
            LangId::Airisu => {
                if s.dark {
                    0.009
                } else {
                    0.004
                }
            }
            LangId::Material(_) => gmin(0.04, 0.008 + pc.c * 0.45),
        }
    }
}

struct Languages {
    washi: Language,
    iris: Language,
    airisu: Language,
    material: Vec<Language>,
    white: RGB,
}

fn langs() -> &'static Languages {
    static L: OnceLock<Languages> = OnceLock::new();
    L.get_or_init(|| Languages {
        washi: washi_language(),
        iris: iris_language(),
        airisu: airisu_language(),
        material: (0..VARIANTS.len()).map(material_language).collect(),
        white: LCH { l: 0.975, c: 0.011, h: 88.0 }.rgb(),
    })
}

fn lang_by_key(key: &str) -> Option<&'static Language> {
    let l = langs();
    match key {
        "washi" => Some(&l.washi),
        "iris" => Some(&l.iris),
        "airisu" => Some(&l.airisu),
        _ => key.strip_prefix("material:").and_then(|v| VARIANTS.iter().position(|x| x.name == v)).map(|i| &l.material[i]),
    }
}

/// A request's language; an unknown or empty one is washi. Material without a known variant is Tonal spot.
fn language_of(req: &Request) -> &'static Language {
    if req.language == "material" {
        return lang_by_key(&format!("material:{}", req.variant)).unwrap_or(&langs().material[0]);
    }
    lang_by_key(&req.language).unwrap_or(&langs().washi)
}

// ---------------------------------------------------------------- washi.go

struct PaperLevel {
    name: &'static str,
    tone: f64,
    warmth: f64,
}

fn paper_levels(name: &str) -> Option<&'static [PaperLevel]> {
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
    match name {
        "light" => Some(LIGHT),
        "ink" => Some(INK),
        "dark" => Some(DARK),
        _ => None,
    }
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

/// Moves a paper along the warmth scale, hue and chroma mixed as a vector (OKLab a, b).
fn warm(s: &Spec, p: LCH, warmth: f64) -> LCH {
    let Some(own) = own_warmth(s.name) else {
        return p;
    };
    let w = clamp_f(warmth, 0.0, 100.0);
    let rad = p.h * std::f64::consts::PI / 180.0;
    let (mut a, mut b) = (p.c * gomath::cos(rad), p.c * gomath::sin(rad));
    if w < own {
        let k = 1.0 - (own - w) / own;
        a *= k;
        b *= k;
    } else {
        let k = (w - own) / (100.0 - own);
        // Go folds sumiHue*math.Pi/180 at compile time, exactly: float64(4π/9).
        let sumi = f64::from_bits(0x3ff657184ae74487);
        let (fa, fb) = (fibre_c(s.name) * gomath::cos(sumi), fibre_c(s.name) * gomath::sin(sumi));
        a += (fa - a) * k;
        b += (fb - b) * k;
    }
    let c = gomath::hypot(a, b);
    let mut h = p.h;
    if c > 1e-6 {
        h = (gomath::atan2(b, a) * 180.0 / std::f64::consts::PI + 360.0) % 360.0;
    }
    LCH { l: p.l, c, h }
}

fn warmth_of(name: &str, t: &Tune) -> f64 {
    t.warmth.unwrap_or_else(|| own_warmth(name).unwrap_or(0.0))
}

/// One scheme: its papers and the band every colour on them is solved in.
#[derive(Clone, Default)]
struct Spec {
    name: &'static str,
    dark: bool,
    materials: Vec<(&'static str, LCH)>,
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

impl Spec {
    fn material(&self, name: &str) -> Option<LCH> {
        self.materials.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
    }

    fn side(dark: bool) -> Spec {
        Spec { dark, ..Spec::default() }
    }
}

const SUMI_HUE: f64 = 80.0;
const NAMES: &[&str] = &["black", "graphite", "midnight", "wallpaper", "theme"];
const ACCENT_SEEDS: &[(&str, &str)] = &[("blue", "#0a60d1"), ("mint", "#0f8f66"), ("rose", "#c2274f"), ("lilac", "#6b47c9")];
const HIGHLIGHT_SEEDS: &[(&str, &str)] =
    &[("orange", "#ff9f0a"), ("yellow", "#ffd60a"), ("red", "#ff453a"), ("pink", "#ff375f"), ("green", "#30d158")];
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

fn lch(l: f64, c: f64, h: f64) -> LCH {
    LCH { l, c, h }
}

/// washi is the shell's own language: paper and sumi, pigments instead of light.
fn washi_language() -> Language {
    Language {
        id: LangId::Washi,
        schemes: vec![
            Spec {
                name: "dark",
                dark: true,
                materials: vec![("black", lch(0.0, 0.0, 0.0)), ("graphite", lch(0.215, 0.011, 72.0)), ("midnight", lch(0.19, 0.028, 258.0))],
                paper_l: 0.18,
                paper_c: 0.03,
                steps: [0.055, 0.11],
                ink_l: 0.94,
                accent_l: 0.72,
                accent_c: [0.13, 0.18],
                mark_l: 0.74,
                mark_c: 0.15,
                widgets: 1.0,
            },
            Spec {
                name: "ink",
                dark: false,
                materials: vec![("black", lch(0.905, 0.018, 88.0)), ("graphite", lch(0.865, 0.03, 72.0)), ("midnight", lch(0.89, 0.014, 232.0))],
                paper_l: 0.9,
                paper_c: 0.035,
                steps: [-0.035, -0.07],
                ink_l: 0.25,
                accent_l: 0.56,
                accent_c: [0.11, 0.16],
                mark_l: 0.62,
                mark_c: 0.16,
                widgets: 1.0,
            },
            Spec {
                name: "light",
                dark: false,
                materials: vec![("black", lch(0.975, 0.011, 88.0)), ("graphite", lch(0.935, 0.007, 85.0)), ("midnight", lch(0.95, 0.014, 240.0))],
                paper_l: 0.965,
                paper_c: 0.025,
                steps: [-0.03, -0.06],
                ink_l: 0.21,
                accent_l: 0.6,
                accent_c: [0.12, 0.17],
                mark_l: 0.65,
                mark_c: 0.17,
                widgets: 1.1,
            },
        ],
        material_names: NAMES,
        accent_seeds: ACCENT_SEEDS,
        highlight_seeds: HIGHLIGHT_SEEDS,
        identity: IDENTITY_SEEDS,
        identity_pull: 12.0,
        identity_hex: &[],
        neutral_hue: SUMI_HUE,
        on_colour: LCH { l: 0.975, c: 0.011, h: 88.0 }.rgb(),
        dark_on: [0.2, 0.03],
        tonal_on: false,
        fill_from: [0.66, 0.52],
        fill_c: [0.12, 0.18],
        whole: false,
        pigments: false,
        fixed_ink: false,
        wheel: None,
        danger_seed: "",
    }
}

/// Resolves a material name to its paper in a scheme.
fn paper(s: &Spec, name: &str, seeds: &Seeds, tone: f64) -> LCH {
    let (mut p, mut ok) = match s.material(name) {
        Some(p) => (p, true),
        None => (LCH::default(), false),
    };
    let from_seed = |hex: &str| -> Option<LCH> {
        let c = seed_lch(hex)?;
        if c.c < 0.02 {
            return None;
        }
        Some(LCH { l: s.paper_l, c: gmin(s.paper_c, c.c * 0.3), h: c.h })
    };
    let assign = |r: Option<LCH>, p: &mut LCH, ok: &mut bool| {
        *p = r.unwrap_or_default();
        *ok = r.is_some();
    };
    match name {
        "wallpaper" => {
            assign(from_seed(&seeds.wallpaper), &mut p, &mut ok);
            if !ok {
                assign(from_seed(&seeds.primary), &mut p, &mut ok);
            }
        }
        "theme" => {
            // A theme's own background when it is of this scheme's polarity, kept a paper (or a night).
            match seed_lch(&seeds.background) {
                Some(bg) if (s.dark && bg.l < 0.36) || (!s.dark && bg.l > 0.8) => {
                    p = if s.dark {
                        LCH { l: bg.l, c: gmin(bg.c, 0.05), h: bg.h }
                    } else if s.name == "ink" {
                        LCH { l: clamp_f(bg.l, 0.86, 0.92), c: gmin(bg.c, 0.04), h: bg.h }
                    } else {
                        LCH { l: gmax(bg.l, 0.93), c: gmin(bg.c, 0.03), h: bg.h }
                    };
                    ok = true;
                }
                _ => assign(from_seed(&seeds.primary), &mut p, &mut ok),
            }
        }
        _ => {}
    }
    if !ok {
        p = s.material("graphite").unwrap_or_default();
    }
    tone_paper(s, &mut p, tone, 0.0);
    p
}

/// Tone lifts or dims the paper, never past where it stops being one (or a night stops being one).
fn tone_paper(s: &Spec, p: &mut LCH, tone: f64, night_floor: f64) {
    if s.dark {
        p.l = clamp_f(p.l + tone / 100.0 * 0.4, night_floor, 0.3);
    } else {
        let floor = match s.name {
            "ink" => 0.83,
            "light" => 0.89,
            _ => 0.0,
        };
        p.l = clamp_f(p.l + tone / 100.0 * 0.3, floor, 1.0);
    }
}

/// Moves a colour's lightness from where it is preferred, away from the paper, until every check holds.
fn solve(s: &Spec, h: f64, c: f64, from: f64, ok: impl Fn(RGB) -> bool) -> RGB {
    let dir = if s.dark { 1.0 } else { -1.0 };
    let at = |l: f64| LCH { l, c: gmin(c, max_chroma(l, h)), h }.rgb();
    // Coarse steps to the first tone that reads, then back in fine steps to the first one past it.
    let mut l = clamp_f(from, 0.0, 1.0);
    while l > 0.0 && l < 1.0 && !ok(at(l)) {
        l += dir * 0.02;
    }
    l = clamp_f(l, 0.0, 1.0);
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
    move |c| grounds.iter().all(|g| contrast(c, *g) >= min)
}

fn over(top: RGB, under: RGB, alpha: f64) -> RGB {
    RGB {
        r: top.r * alpha + under.r * (1.0 - alpha),
        g: top.g * alpha + under.g * (1.0 - alpha),
        b: top.b * alpha + under.b * (1.0 - alpha),
    }
}

#[derive(Clone, Copy)]
struct Ramp {
    surface: LCH,
    high: LCH,
    highest: LCH,
}

impl Ramp {
    fn all(&self) -> Vec<RGB> {
        vec![self.surface.rgb(), self.high.rgb(), self.highest.rgb()]
    }
}

/// One scheme's complete set of roles.
#[derive(Clone, Debug, Default, PartialEq)]
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
    pub papers: Option<BTreeMap<String, String>>,
}

#[derive(Clone)]
struct Solver<'a> {
    lang: &'static Language,
    s: &'a Spec,
    req: &'a Request,
    ramp: Ramp,
    grounds: Vec<RGB>,
    colour: f64,
    widgets: f64,
    opts: Options,
}

/// What a paper body can become as glass: the frost 0.1 (gamma) under the paper, and a group on that.
fn glass_grounds(paper: RGB, ink: RGB) -> [RGB; 2] {
    let f = |c: f64| gmax(0.0, c - 0.1);
    let frost = clamp_rgb(RGB { r: f(paper.r), g: f(paper.g), b: f(paper.b) });
    [frost, clamp_rgb(over(ink, frost, 0.1))]
}

impl Solver<'_> {
    /// The hardest ground: the darkest on paper, the brightest at night.
    fn far(&self) -> RGB {
        let mut best = self.grounds[0];
        for g in &self.grounds[1..] {
            if self.s.dark == (luminance(*g) > luminance(best)) {
                best = *g;
            }
        }
        best
    }

    fn accent(&self, seed: LCH) -> RGB {
        let c = clamp_f(seed.c, self.s.accent_c[0], self.s.accent_c[1]) * self.colour;
        let papers = self.ramp.all();
        let on_colour = self.lang.on_colour;
        let mut far = papers[0];
        for g in &papers[1..] {
            if self.s.dark == (luminance(*g) > luminance(far)) {
                far = *g;
            }
        }
        let dark = self.s.dark;
        let check = |x: RGB| {
            reads_on(READ_TEXT, &papers)(x)
                && reads_on(READ_OVER_GLASS, &self.grounds)(x)
                && (dark || contrast(x, on_colour) >= READ_ON)
                && contrast(x, over(x, far, 0.15)) >= READ_WASH
        };
        if self.lang.wheel.is_some() {
            // A pigment set as type starts from itself, with its own chroma.
            return solve(self.s, seed.h, gmin(seed.c, self.s.accent_c[1]) * self.colour, seed.l, check);
        }
        solve(self.s, seed.h, c, self.s.accent_l, check)
    }

    fn fill(&self, seed: LCH, need: f64) -> RGB {
        let mut c = clamp_f(seed.c, self.lang.fill_c[0], self.lang.fill_c[1]) * gmax(0.6, self.colour);
        let ramp = self.ramp.all();
        let on_colour = self.lang.on_colour;
        if self.lang.pigments {
            let dark_on = LCH { l: self.lang.dark_on[0], c: self.lang.dark_on[1], h: seed.h }.rgb();
            let ink = |f: RGB| contrast(f, on_colour) >= READ_ON || contrast(f, dark_on) >= READ_ON;
            let shape = if self.s.dark { READ_SHAPE } else { READ_PIGMENT };
            return solve(self.s, seed.h, c, seed.l, |x| reads_on(shape, &ramp)(x) && ink(x));
        }
        if !self.s.dark {
            c = gmin(c, PAPER_FILL_C);
        }
        let from = if self.s.dark { self.lang.fill_from[0] } else { self.lang.fill_from[1] };
        let dark = self.s.dark;
        solve(self.s, seed.h, c, from, |x| reads_on(need, &ramp)(x) && (dark || contrast(x, on_colour) >= READ_ON))
    }

    fn mark(&self, h: f64, c: f64, dl: f64) -> RGB {
        let mut from = self.s.mark_l + dl;
        if self.s.dark && (85.0..=125.0).contains(&h) {
            // Yellow is only yellow when light: at the band's lightness kihada turned olive on a night widget.
            from = gmax(from, 0.86);
        }
        solve(self.s, h, c, from, reads_on(READ_MARK, &self.grounds))
    }

    fn highlight(&self, seed: LCH) -> RGB {
        let c = clamp_f(seed.c, self.s.mark_c * 0.7, self.s.mark_c * 1.15) * self.colour;
        if self.lang.pigments {
            return solve(
                self.s,
                seed.h,
                gmax(c, gmin(seed.c, self.s.mark_c * 1.4) * self.colour),
                seed.l,
                reads_on(READ_MARK, &self.grounds),
            );
        }
        self.mark(seed.h, c, 0.0)
    }

    fn on_container(&self, cont: RGB, h: f64, c: f64) -> RGB {
        let side = Spec::side(cont.lch().l < 0.6);
        let far = self.far();
        let both_sides = side.dark == self.s.dark;
        let from = if side.dark { 0.9 } else { 0.32 };
        solve(&side, h, c, from, |x| contrast(x, cont) >= READ_ON && (!both_sides || contrast(x, far) >= READ_ON))
    }

    fn on_fill(&self, fill: RGB, need: f64) -> RGB {
        let fl = fill.lch();
        let light = self.lang.on_colour;
        let mut dark = LCH { l: self.lang.dark_on[0], c: self.lang.dark_on[1], h: fl.h }.rgb();
        if self.lang.tonal_on {
            dark = solve(&Spec::default(), fl.h, gmin(self.lang.dark_on[1], fl.c), self.lang.dark_on[0], |x| contrast(x, fill) >= need);
        }
        if !self.s.dark && contrast(light, fill) >= need {
            return light;
        }
        if contrast(dark, fill) >= contrast(light, fill) {
            return dark;
        }
        light
    }

    fn seed_of(&self, choice: &str, hue: f64, table: &[(&str, &'static str)], fallback: &str) -> LCH {
        let pick = |hexes: &[&str]| -> Option<LCH> {
            hexes.iter().find_map(|h| seed_lch(h).filter(|c| c.c >= 0.03)).map(|c| self.on_wheel(c))
        };
        let mut fallback = fallback;
        if table_get(table, "design").is_some() && self.lang.wheel.is_some() {
            // A source with no hue to give (a grey wallpaper) leaves the Design's own colour.
            fallback = "design";
        }
        let seeds = &self.req.seeds;
        match choice {
            "custom" => return self.on_wheel(hsl(hue, 0.7, 0.5).lch()),
            "wallpaper" => {
                if let Some(c) = pick(&[&seeds.wallpaper, &seeds.primary]) {
                    return c;
                }
            }
            "theme" => {
                if let Some(c) = pick(&[&seeds.primary]) {
                    return c;
                }
            }
            "wallpaperHighlight" => {
                if let Some(c) = pick(&[&seeds.secondary, &seeds.wallpaper, &seeds.primary]) {
                    return self.on_wheel(LCH { l: c.l, c: gmax(c.c, 0.12), h: c.h });
                }
            }
            "themeHighlight" => {
                if let Some(c) = pick(&[&seeds.tertiary]) {
                    return self.on_wheel(LCH { l: c.l, c: gmax(c.c, 0.12), h: c.h });
                }
            }
            _ => {}
        }
        if let Some(hex) = table_get(table, choice) {
            return hex_lch(hex);
        }
        hex_lch(table_get(table, fallback).unwrap_or(""))
    }

    fn on_wheel(&self, c: LCH) -> LCH {
        match &self.lang.wheel {
            None => c,
            Some(w) => pigment_of(w, c),
        }
    }

    /// The same palette in Material's role names, for the app themes.
    fn apps(&self, p: &Palette, accent: RGB, highlight: RGB, danger: RGB, cont: RGB) -> BTreeMap<String, String> {
        let s = self.s;
        let mut pc = self.ramp.surface;
        if pc.c < 0.004 {
            pc.h = self.lang.neutral_hue;
        }
        let sign = if s.dark { 1.0 } else { -1.0 };
        let mut base = pc;
        if s.dark && base.l < APPS_NIGHT_L {
            base = LCH { l: APPS_NIGHT_L, c: gmax(base.c, 0.004), h: base.h };
        }
        if let Some(c) = seed_lch(&self.req.seeds.wallpaper) {
            if self.req.glass && c.c >= 0.02 && s.name != "ink" {
                // Under glass a window beside the shell carries the frost's hue. Ink keeps its paper.
                let frost = paper_of(self.lang, s, "wallpaper", self.req);
                base.h = frost.h;
                base.c = gmax(base.c, gmin(frost.c, if s.dark { 0.008 } else { 0.006 }));
                pc.h = frost.h;
            }
        }
        let at = |d: f64, c_scale: f64| -> String {
            let mut q = base;
            let d = if s.dark { d * APPS_NIGHT_STEP } else { d };
            q.l = clamp_f(q.l + sign * d, 0.0, 1.0);
            q.c = gmin(0.012, q.c * c_scale);
            q.rgb().hex()
        };
        let (al, hl, dl) = (accent.lch(), highlight.lch(), danger.lch());
        let deepest = parse_hex(&at(0.105, 1.15)).unwrap_or_default();
        let mut grounds = vec![deepest];
        grounds.extend_from_slice(&self.grounds);
        let mut accent_from = s.accent_l;
        let mut text_grounds = grounds.clone();
        if !s.dark {
            // A window on paper has its own planes: its colours read on those, from the lightest tone that does.
            let lowest = parse_hex(&at(-0.012, 0.8)).unwrap_or_default();
            let ground = parse_hex(&at(0.0, 1.0)).unwrap_or_default();
            let card = parse_hex(&at(0.04, 1.05)).unwrap_or_default();
            text_grounds = vec![lowest, ground, card];
            accent_from = gmax(accent_from, APPS_PAPER_FROM_L);
        }
        let mut app_ramp = vec![deepest];
        app_ramp.extend_from_slice(&text_grounds);
        let reads = |need: f64, x: RGB| -> bool {
            if s.dark {
                return reads_on(need, &grounds)(x);
            }
            reads_on(need, &text_grounds)(x) && contrast(x, deepest) >= gmin(need, READ_SHAPE)
        };
        let sec = solve(s, al.h, gmin(0.06, al.c * 0.4), accent_from, reads_on(READ_TEXT, &app_ramp));
        let cont_l = cont.lch().l;
        let sec_cont = LCH { l: cont_l, c: gmin(0.035, al.c * 0.25), h: al.h }.rgb();
        let ter = solve(s, hl.h, gmin(hl.c, if s.dark { 0.11 } else { 0.09 }), accent_from, reads_on(READ_TEXT, &app_ramp));
        let ter_cont = LCH { l: cont_l, c: gmin(0.07, hl.c * 0.45), h: hl.h }.rgb();
        let err_cont = LCH { l: cont_l, c: gmin(0.08, dl.c * 0.45), h: dl.h }.rgb();
        let on_c = |fill: RGB, h: f64, c: f64| -> String {
            let side = Spec::side(fill.lch().l < 0.6);
            solve(&side, h, c, if side.dark { 0.9 } else { 0.3 }, |x| contrast(x, fill) >= READ_ON).hex()
        };
        let on_colour = self.lang.on_colour;
        let app_fill = |hex: &str, need: f64, paper_c: f64, night_c: f64| -> RGB {
            let l = parse_hex(hex).unwrap_or_default().lch();
            let mut from = l.l;
            if !s.dark {
                from = gmax(from, APPS_PAPER_FROM_L);
            }
            solve(s, l.h, gmin(l.c, if s.dark { night_c } else { paper_c }), from, |x| {
                reads(need, x) && (s.dark || contrast(x, on_colour) >= READ_ON)
            })
        };
        let primary = app_fill(&p.accent_fill, READ_TEXT, APPS_FILL_C[0], APPS_FILL_C[1]);
        let error_fill = app_fill(&p.danger_fill, READ_SHAPE, APPS_ALERT_C[0], APPS_ALERT_C[1]);
        let error_text = app_fill(&p.danger, READ_TEXT, APPS_ALERT_C[0], APPS_ALERT_C[1]);
        let tl = parse_hex(&p.text).unwrap_or_default().lch();
        let (var_h, var_c, out_c, var_from, out_from) = match self.warm_factor() {
            Some(wf) if self.lang.id != LangId::Airisu => (
                hue_toward(pc.h, SUMI_HUE, 45.0 * wf),
                gmin((pc.c * 1.4 + 0.018 * wf) * gmax(self.colour, 0.6), 0.028),
                gmin((pc.c * 1.2 + 0.014 * wf) * gmax(self.colour, 0.6), 0.022),
                0.64,
                0.42,
            ),
            _ => (pc.h, gmin(0.02, pc.c * 1.5 + 0.006), gmin(0.02, pc.c * 1.5 + 0.006), 0.62, 0.4),
        };
        let variant = solve(s, var_h, var_c, pc.l + (tl.l - pc.l) * var_from, reads_on(READ_SECONDARY, &grounds));
        let outline = solve(s, var_h, out_c, pc.l + (tl.l - pc.l) * out_from, reads_on(READ_TERTIARY, &grounds));
        let on_surface = solve(s, tl.h, tl.c, tl.l, reads_on(READ_INK, &grounds)).hex();
        let inverse = if s.dark { LCH { l: 0.93, c: 0.008, h: pc.h } } else { LCH { l: 0.27, c: 0.012, h: pc.h } }.rgb();
        let inverse_primary = solve(s, al.h, al.c, if s.dark { 0.5 } else { 0.8 }, |x| contrast(x, inverse) >= READ_TEXT);
        let mut m = BTreeMap::new();
        let mut put = |k: &str, v: String| {
            m.insert(k.to_string(), v);
        };
        // Dim is darker and Bright lighter in both polarities (at moves toward the ink: darker on paper).
        put("background", at(0.0, 1.0));
        put("surface", at(0.0, 1.0));
        put("surfaceDim", at(-0.03 * sign, 1.0));
        put("surfaceBright", at(0.035 * sign, 1.0));
        put("surfaceContainerLowest", at(-0.012, 0.8));
        put("surfaceContainerLow", at(0.02, 1.0));
        put("surfaceContainer", at(0.04, 1.05));
        put("surfaceContainerHigh", at(0.065, 1.1));
        put("surfaceContainerHighest", at(0.09, 1.15));
        put("surfaceVariant", at(0.065, 1.2));
        put("onBackground", on_surface.clone());
        put("onSurface", on_surface);
        put("onSurfaceVariant", variant.hex());
        put("outline", outline.hex());
        put("outlineVariant", at(0.16, 1.2));
        put("inverseSurface", inverse.hex());
        put("inverseOnSurface", at(0.0, 1.0));
        put("primary", primary.hex());
        put("onPrimary", self.on_fill(primary, READ_ON).hex());
        put("primaryContainer", p.accent_container.clone());
        put("onPrimaryContainer", p.on_accent_container.clone());
        put("inversePrimary", inverse_primary.hex());
        put("surfaceTint", primary.hex());
        put("secondary", sec.hex());
        put("onSecondary", self.on_fill(sec, READ_ON).hex());
        put("secondaryContainer", sec_cont.hex());
        put("onSecondaryContainer", on_c(sec_cont, al.h, 0.05));
        put("tertiary", ter.hex());
        put("onTertiary", self.on_fill(ter, READ_ON).hex());
        put("tertiaryContainer", ter_cont.hex());
        put("onTertiaryContainer", on_c(ter_cont, hl.h, 0.1));
        put("error", error_text.hex());
        put("onError", self.on_fill(error_text, READ_ON).hex());
        put("errorFill", error_fill.hex());
        put("onErrorFill", self.on_fill(error_fill, READ_ON).hex());
        put("errorContainer", err_cont.hex());
        put("onErrorContainer", on_c(err_cont, dl.h, 0.12));
        put("success", p.success.clone());
        put("shadow", "#000000".into());
        put("scrim", "#000000".into());
        m
    }

    /// With warm neutrals on, how far they lean: the scheme's warmth scaled by its colour. None leaves Go's neutrals.
    fn warm_factor(&self) -> Option<f64> {
        if !self.opts.warm_neutrals {
            return None;
        }
        let wf = clamp_f(warmth_of(self.s.name, &self.req.tune_of(self.s.name)), 0.0, 100.0) / 100.0 * clamp_f(self.colour, 0.0, 1.0);
        (wf > 0.01).then_some(wf)
    }
}

const APPS_FILL_C: [f64; 2] = [0.12, 0.17];
const PAPER_FILL_C: f64 = 0.12;
const APPS_ALERT_C: [f64; 2] = [0.11, 0.12];
const APPS_PAPER_FROM_L: f64 = 0.62;
const APPS_NIGHT_L: f64 = 0.18;
const APPS_NIGHT_STEP: f64 = 1.45;

fn hexes(cs: &[RGB]) -> Vec<String> {
    cs.iter().map(|c| c.hex()).collect()
}

/// The language one scheme is solved in: Ink is washi's paper unless the person lets it follow the style.
fn scheme_language(req: &Request, name: &str) -> &'static Language {
    let lang = language_of(req);
    if lang.whole {
        return lang;
    }
    if (name == "ink" && req.ink_style != "style") || (name == "dark" && req.dark_style == "ink") {
        return &langs().washi;
    }
    lang
}

/// A material's paper in a scheme with the person's tone and warmth.
fn paper_of(lang: &Language, s: &Spec, material: &str, req: &Request) -> LCH {
    let t = req.tune_of(s.name);
    warm(s, lang.paper(s, material, &req.seeds, t.tone), warmth_of(s.name, &t))
}

fn build(req: &Request, name: &str, opts: Options) -> Palette {
    let lang = scheme_language(req, name);
    let s = lang.spec_of(name);
    let tune = req.tune_of(name);
    let colour = tune.colour.map_or(1.0, |c| clamp_f(c / 100.0, 0.0, 1.0));
    let widgets = tune.widgets.map_or(s.widgets, |w| clamp_f(w / 100.0, 0.4, 1.6));
    let mut material = req.material.as_str();
    if s.material(material).is_none() && material != "wallpaper" && material != "theme" {
        material = "black";
    }
    let ramp = lang.ramp(s, paper_of(lang, s, material, req));
    let mut v = Solver { lang, s, req, ramp, grounds: ramp.all(), colour, widgets, opts };
    let mut p = Palette { dark: s.dark, material: material.to_string(), ..Palette::default() };
    p.surface = v.grounds[0].hex();
    p.surface_high = v.grounds[1].hex();
    p.surface_highest = v.grounds[2].hex();

    // Ink: sumi in the paper's own hue (or paper white at night), readInk on the deepest group.
    let mut pc = ramp.surface;
    if pc.c < 0.004 || lang.fixed_ink {
        pc.h = lang.neutral_hue;
    }
    let ink_c = lang.ink_chroma(s, pc);
    let mut text = solve(s, pc.h, ink_c, s.ink_l, reads_on(READ_INK, &v.grounds));
    if !s.dark {
        // The group on the frost is made of the ink: solve both together until they agree.
        let paper_grounds = v.grounds.clone();
        let with_glass = |ink: RGB| {
            let mut g = paper_grounds.clone();
            g.extend_from_slice(&glass_grounds(paper_grounds[0], ink));
            g
        };
        for _ in 0..4 {
            v.grounds = with_glass(text);
            let next = solve(s, pc.h, ink_c, s.ink_l, reads_on(READ_INK, &v.grounds));
            if next == text {
                break;
            }
            text = next;
        }
        v.grounds = with_glass(text);
    }
    p.text = text.hex();
    p.fill_ink = p.text.clone();
    let warm_quiet = v.warm_factor().filter(|_| !lang.fixed_ink);
    // Warm neutrals lean to sand and start a step further from the paper: the sand ink reads at less lightness.
    let quiet = |need: f64, from: f64, warm_from: f64, warm_c: f64| -> RGB {
        let (h, c, from) = match warm_quiet {
            Some(wf) => (hue_toward(pc.h, SUMI_HUE, 42.0 * wf), gmin(ink_c * 1.5 + warm_c * wf, 0.024), warm_from),
            None => (pc.h, ink_c * 1.5, from),
        };
        solve(s, h, c, ramp.surface.l + (text.lch().l - ramp.surface.l) * from, reads_on(need, &v.grounds))
    };
    p.text_secondary = quiet(READ_SECONDARY, 0.62, 0.64, 0.014).hex();
    p.text_tertiary = quiet(READ_TERTIARY, 0.45, 0.46, 0.010).hex();

    let accent_choice = if req.accent.is_empty() { "blue" } else { req.accent.as_str() };
    let accent_seed = v.seed_of(accent_choice, req.accent_hue, lang.accent_seeds, "blue");
    let accent = v.accent(accent_seed);
    let al = accent.lch();
    p.accent = accent.hex();
    p.on_accent = v.on_fill(accent, READ_ON).hex();
    let cont = lang.container(s, pc, al).rgb();
    p.accent_container = cont.hex();
    p.on_accent_container = v.on_container(cont, al.h, gmin(0.12, al.c)).hex();

    let highlight_choice = req.highlight.as_str();
    let (mut highlight, mut highlight_seed) = match highlight_choice {
        "accent" => (accent, accent_seed),
        "wallpaper" => (RGB::default(), v.seed_of("wallpaperHighlight", 0.0, lang.highlight_seeds, "orange")),
        "theme" => (RGB::default(), v.seed_of("themeHighlight", 0.0, lang.highlight_seeds, "orange")),
        "custom" => (RGB::default(), v.on_wheel(hsl(req.highlight_hue, 0.92, 0.58).lch())),
        other => (RGB::default(), v.seed_of(other, 0.0, lang.highlight_seeds, "orange")),
    };
    if highlight_choice == "wallpaper" && matches!(lang.id, LangId::Washi) && hue_distance(highlight_seed.h, al.h) < 30.0 {
        // A wallpaper of one hue gave accent and highlight the same red: the highlight takes the mineral pigment
        // farthest from the accent, as the widgets' second series does.
        let mut far = 0.0;
        for h in [262.0, 160.0, 85.0] {
            let d = hue_distance(h, al.h);
            if d > far {
                far = d;
                highlight_seed = lch(0.72, 0.15, h);
            }
        }
    }
    if highlight_choice != "accent" {
        highlight = v.highlight(highlight_seed);
    }
    p.highlight = highlight.hex();
    p.on_highlight = v.on_fill(highlight, READ_ON).hex();

    let status = |h: f64, c: f64, need: f64, carries_ink: bool| -> RGB {
        let on_colour = lang.on_colour;
        solve(s, h, c * gmax(0.6, colour), s.mark_l, |x| {
            reads_on(need, &v.grounds)(x) && !(carries_ink && !s.dark && contrast(x, on_colour) < READ_ON)
        })
    };
    p.success = status(150.0, 0.13, READ_TEXT, false).hex();
    p.warning = status(72.0, 0.14, READ_MARK, false).hex();
    let (mut danger_h, mut danger_fill_seed) = (27.0, LCH { l: 0.55, c: 0.15, h: 33.0 });
    if !lang.danger_seed.is_empty() {
        let ds = hex_lch(lang.danger_seed);
        danger_h = ds.h;
        danger_fill_seed = ds;
    }
    let danger = status(danger_h, 0.17, READ_TEXT, true);
    p.danger = danger.hex();
    p.on_danger = v.on_fill(danger, READ_ON).hex();

    // Filled shapes: the same hues as pigments, each with its ink. A Design's seal is solved from a pigment's
    // lightness, never from a pale seed's.
    let mut seal_seed = accent_seed;
    if lang.pigments && lang.wheel.is_none() {
        seal_seed.l = gmin(seal_seed.l, s.accent_l);
    }
    let accent_fill = v.fill(seal_seed, READ_TEXT);
    let danger_fill = v.fill(danger_fill_seed, READ_SHAPE);
    let hl_lch = highlight.lch();
    let mut highlight_fill = v.fill(LCH { l: 0.6, c: gmax(0.1, hl_lch.c), h: hl_lch.h }, READ_SHAPE);
    if lang.pigments {
        highlight_fill = v.fill(highlight_seed, READ_SHAPE);
    }
    if highlight_choice == "accent" {
        highlight_fill = accent_fill;
    }
    p.accent_fill = accent_fill.hex();
    p.on_accent_fill = v.on_fill(accent_fill, READ_ON).hex();
    p.danger_fill = danger_fill.hex();
    p.on_danger_fill = v.on_fill(danger_fill, READ_ON).hex();
    p.highlight_fill = highlight_fill.hex();
    p.on_highlight_fill = v.on_fill(highlight_fill, READ_ON).hex();

    // Identity: one band of lightness and chroma for every hue, each turned a little toward the accent.
    let id_k = match name {
        "dark" => 0.86,
        "ink" => 0.8,
        "light" => 0.86,
        _ => 0.0,
    };
    let id_c = s.mark_c * id_k;
    for (id_name, id_hex) in lang.identity_hex {
        let c = hex_lch(id_hex);
        let k = match name {
            "dark" => 1.0,
            "ink" => 0.75,
            "light" => 0.9,
            _ => 0.0,
        };
        p.identity.insert(id_name.to_string(), v.mark(c.h, c.c * k, c.l - s.mark_l).hex());
    }
    for id in lang.identity {
        if p.identity.contains_key(id.name) {
            continue;
        }
        if id.hue < 0.0 {
            let grey = solve(s, pc.h, 0.012, s.mark_l, reads_on(READ_MARK, &v.grounds)).hex();
            p.identity.insert(id.name.to_string(), grey);
            continue;
        }
        let mut h = id.hue;
        if al.c >= 0.04 {
            h = hue_toward(h, al.h, lang.identity_pull);
        }
        p.identity.insert(id.name.to_string(), v.mark(h, id_c + id.c, id.l).hex());
    }

    // Data in widgets: three colours of one band, the widget colour and vibrance setting their strength.
    let vib = clamp_f(req.vibrance, 0.0, 1.0);
    let k = widgets / s.widgets * (0.8 + 0.4 * vib);
    let data = |h: f64, c: f64, dl: f64| v.mark(h, gmin(s.mark_c * 1.3, c * k), dl);
    let hue_or = |hex: &str, fallback: f64| seed_lch(hex).filter(|c| c.c >= 0.02).map_or(fallback, |c| c.h);
    let ph = hue_or(&req.seeds.primary, al.h);
    let mut th = hue_or(&req.seeds.tertiary, (ph + 60.0) % 360.0);
    if hue_distance(th, ph) < 24.0 {
        // A wallpaper of one hue: the second series takes the mineral pigment farthest from it.
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
    let mut third = data(sh, s.mark_c * 0.75, 0.0);
    if hue_distance(sh, ph) < 24.0 {
        third = data(ph, s.mark_c * 0.6, if s.dark { 0.12 } else { -0.14 });
    }
    let hl = highlight.lch();
    let id_h = |n: &str| parse_hex(p.identity.get(n).map(String::as_str).unwrap_or("")).unwrap_or_default().lch().h;
    let (blue_h, orange_h, teal_h) = (id_h("blue"), id_h("orange"), id_h("teal"));
    p.widgets.insert("wallpaper".into(), hexes(&[data(ph, s.mark_c, 0.0), data(th, s.mark_c, 0.0), third]));
    p.widgets.insert("system".into(), hexes(&[data(al.h, s.mark_c, 0.0), data(hl.h, s.mark_c, 0.0), data(150.0, s.mark_c * 0.9, 0.0)]));
    p.widgets.insert("spectrum".into(), hexes(&[data(blue_h, s.mark_c, 0.0), data(orange_h, s.mark_c, 0.0), data(teal_h, s.mark_c, 0.0)]));
    p.widgets.insert(
        "mono".into(),
        hexes(&[
            data(al.h, s.mark_c, 0.0),
            data(al.h, s.mark_c * 0.7, if s.dark { 0.12 } else { -0.16 }),
            data(al.h, s.mark_c * 0.5, if s.dark { -0.1 } else { 0.1 }),
        ]),
    );

    // Swatches for Settings: each choice as it would be solved here.
    for m in lang.material_names {
        p.materials.insert(m.to_string(), paper_of(lang, s, m, req).rgb().hex());
    }
    if let Some(levels) = paper_levels(name) {
        let mut papers = BTreeMap::new();
        for l in levels {
            let mut other = req.clone();
            let mut tune = other.tune.clone().unwrap_or_default();
            let mut t = req.tune_of(name);
            t.tone = l.tone;
            t.warmth = Some(l.warmth);
            tune.insert(name.to_string(), t);
            other.tune = Some(tune);
            other.dark_style = "ink".into(); // a night paper is washi's, whatever Dark wears now
            papers.insert(l.name.to_string(), paper_of(scheme_language(&other, name), s, material, &other).rgb().hex());
        }
        p.papers = Some(papers);
    }
    for (n, hex) in lang.accent_seeds {
        p.accents.insert(n.to_string(), v.accent(hex_lch(hex)).hex());
    }
    for n in ["wallpaper", "theme", "custom"] {
        p.accents.insert(n.to_string(), v.accent(v.seed_of(n, req.accent_hue, lang.accent_seeds, "blue")).hex());
    }
    p.highlights.insert("accent".into(), p.accent.clone());
    for (n, hex) in lang.highlight_seeds {
        p.highlights.insert(n.to_string(), v.highlight(hex_lch(hex)).hex());
    }
    p.highlights.insert("wallpaper".into(), v.highlight(v.seed_of("wallpaperHighlight", 0.0, lang.highlight_seeds, "orange")).hex());
    p.highlights.insert("theme".into(), v.highlight(v.seed_of("themeHighlight", 0.0, lang.highlight_seeds, "orange")).hex());
    p.highlights.insert("custom".into(), v.highlight(hsl(req.highlight_hue, 0.92, 0.58).lch()).hex());
    p.apps = v.apps(&p, accent, highlight, danger, cont);
    p
}

// ---------------------------------------------------------------- iris.go

const IRIS_HUE: f64 = 286.0;
const IRIS_IDENTITY_HEX: &[(&str, &str)] = &[
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

fn iris_language() -> Language {
    Language {
        id: LangId::Iris,
        schemes: vec![
            Spec {
                name: "dark",
                dark: true,
                materials: vec![("black", lch(0.0, 0.0, 0.0)), ("graphite", lch(0.192, 0.004, IRIS_HUE)), ("midnight", lch(0.161, 0.022, 271.0))],
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
                materials: vec![("black", lch(0.88, 0.018, 89.0)), ("graphite", lch(0.827, 0.02, 91.0)), ("midnight", lch(0.843, 0.009, 248.0))],
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
                materials: vec![("black", lch(1.0, 0.0, IRIS_HUE)), ("graphite", lch(0.935, 0.007, IRIS_HUE)), ("midnight", lch(0.938, 0.018, 258.0))],
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
        material_names: NAMES,
        accent_seeds: ACCENT_SEEDS,
        highlight_seeds: HIGHLIGHT_SEEDS,
        identity: IDENTITY_SEEDS,
        identity_pull: 0.0,
        identity_hex: IRIS_IDENTITY_HEX,
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

// ---------------------------------------------------------------- material.go

struct Variant {
    name: &'static str,
    neutral_c: [f64; 2],
    accent_c: [f64; 2],
    accent_l: [f64; 2],
    container_l: [f64; 2],
    container_c: f64,
    mark_c: f64,
}

const VARIANTS: &[Variant] = &[
    Variant { name: "tonalSpot", neutral_c: [0.012, 0.01], accent_c: [0.05, 0.085], accent_l: [0.83, 0.48], container_l: [0.44, 0.87], container_c: 0.075, mark_c: 0.1 },
    Variant { name: "vibrant", neutral_c: [0.06, 0.03], accent_c: [0.1, 0.18], accent_l: [0.77, 0.48], container_l: [0.75, 0.76], container_c: 0.17, mark_c: 0.14 },
    Variant { name: "expressive", neutral_c: [0.03, 0.016], accent_c: [0.07, 0.13], accent_l: [0.86, 0.49], container_l: [0.82, 0.83], container_c: 0.11, mark_c: 0.12 },
    Variant { name: "fidelity", neutral_c: [0.015, 0.012], accent_c: [0.02, 0.2], accent_l: [0.83, 0.45], container_l: [0.55, 0.55], container_c: 0.2, mark_c: 0.12 },
    Variant { name: "monochrome", neutral_c: [0.0, 0.0], accent_c: [0.0, 0.0], accent_l: [0.99, 0.02], container_l: [0.87, 0.35], container_c: 0.0, mark_c: 0.07 },
];

const MATERIAL_HUE: f64 = 250.0;

fn material_language(vi: usize) -> Language {
    let v = &VARIANTS[vi];
    let at = |dark: bool, pair: [f64; 2]| if dark { pair[0] } else { pair[1] };
    let scheme = |name: &'static str, dark: bool, paper_l: f64, ink_l: f64, mark_l: f64, steps: [f64; 2]| Spec {
        name,
        dark,
        paper_l,
        paper_c: at(dark, v.neutral_c),
        steps,
        ink_l,
        accent_l: at(dark, v.accent_l),
        accent_c: v.accent_c,
        mark_l,
        mark_c: v.mark_c,
        widgets: 1.0,
        materials: vec![("black", LCH::default()), ("graphite", LCH::default()), ("midnight", LCH::default())],
    };
    Language {
        id: LangId::Material(vi),
        schemes: vec![
            scheme("dark", true, 0.165, 0.924, 0.8, [0.05, 0.104]),
            scheme("ink", false, 0.915, 0.3, 0.55, [-0.035, -0.07]),
            scheme("light", false, 0.982, 0.317, 0.55, [-0.033, -0.067]),
        ],
        material_names: NAMES,
        accent_seeds: ACCENT_SEEDS,
        highlight_seeds: HIGHLIGHT_SEEDS,
        identity: IDENTITY_SEEDS,
        identity_pull: 15.0,
        identity_hex: &[],
        neutral_hue: MATERIAL_HUE,
        on_colour: RGB { r: 1.0, g: 1.0, b: 1.0 },
        dark_on: [0.37, 0.06],
        tonal_on: true,
        fill_from: [v.accent_l[0], v.accent_l[1]],
        fill_c: if v.name == "monochrome" { [0.0, 0.0] } else { [gmax(0.02, v.accent_c[0]), gmax(0.02, v.accent_c[1])] },
        whole: false,
        pigments: false,
        fixed_ink: false,
        wheel: None,
        danger_seed: "",
    }
}

fn material_paper(v: &Variant, s: &Spec, name: &str, seeds: &Seeds, tone: f64) -> LCH {
    let _ = v;
    let mut h = MATERIAL_HUE;
    for hex in [&seeds.wallpaper, &seeds.primary] {
        if let Some(c) = seed_lch(hex) {
            if c.c >= 0.02 {
                h = c.h;
                break;
            }
        }
    }
    let wallpaper = LCH { l: s.paper_l, c: s.paper_c, h };
    let mut p = match name {
        "black" => {
            // Black stays black at night; by day it is Material's brightest surface.
            if s.dark {
                LCH { l: 0.0, c: s.paper_c, h }
            } else {
                LCH { l: s.paper_l, c: s.paper_c * 0.6, h }
            }
        }
        "graphite" => {
            let mut p = LCH { l: s.paper_l, c: s.paper_c * 0.35, h };
            if !s.dark {
                p.l = s.paper_l - 0.033;
            }
            p
        }
        "midnight" => {
            if s.dark {
                LCH { l: 0.187, c: 0.03, h: 262.0 }
            } else {
                LCH { l: s.paper_l - 0.012, c: 0.016, h: 245.0 }
            }
        }
        "theme" => match seed_lch(&seeds.background) {
            Some(bg) if (s.dark && bg.l < 0.36) || (!s.dark && bg.l > 0.8) => {
                let mut p = LCH { l: bg.l, c: gmin(bg.c, 0.06), h: bg.h };
                if !s.dark {
                    p.l = gmax(bg.l, s.paper_l - 0.05);
                }
                p
            }
            _ => wallpaper,
        },
        _ => wallpaper,
    };
    tone_paper(s, &mut p, tone, 0.0);
    p
}

// ---------------------------------------------------------------- airisu.go

const AIRISU_HUE: f64 = 85.0;
const AIRISU_IDENTITY_HEX: &[(&str, &str)] = &[
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
const AIRISU_ACCENT_SEEDS: &[(&str, &str)] =
    &[("design", "#e5341b"), ("blue", "#4c6cb3"), ("mint", "#00a3af"), ("rose", "#d0576b"), ("lilac", "#a59aca"), ("green", "#b9d08b")];
const AIRISU_HIGHLIGHT_SEEDS: &[(&str, &str)] =
    &[("design", "#f8b500"), ("orange", "#ee7800"), ("yellow", "#f8b500"), ("red", "#e5341b"), ("pink", "#eebbcb"), ("green", "#b9d08b")];

fn airisu_language() -> Language {
    Language {
        id: LangId::Airisu,
        schemes: vec![
            Spec {
                name: "dark",
                dark: true,
                materials: vec![("black", hex_lch("#0b0b0c")), ("graphite", hex_lch("#18181b")), ("midnight", lch(0.165, 0.024, 262.0))],
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
                materials: vec![("black", hex_lch("#e9e6df")), ("graphite", lch(0.87, 0.012, AIRISU_HUE)), ("midnight", lch(0.9, 0.016, 250.0))],
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
                materials: vec![("black", lch(0.97, 0.006, AIRISU_HUE)), ("graphite", lch(0.935, 0.005, AIRISU_HUE)), ("midnight", lch(0.955, 0.012, 250.0))],
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
        material_names: NAMES,
        accent_seeds: AIRISU_ACCENT_SEEDS,
        highlight_seeds: AIRISU_HIGHLIGHT_SEEDS,
        identity: IDENTITY_SEEDS,
        identity_pull: 0.0,
        identity_hex: AIRISU_IDENTITY_HEX,
        neutral_hue: AIRISU_HUE,
        on_colour: hex_lch("#e9e6df").rgb(),
        dark_on: [0.14, 0.002],
        tonal_on: false,
        fill_from: [0.6, 0.56],
        fill_c: [0.09, 0.24],
        whole: true,
        pigments: true,
        fixed_ink: true,
        wheel: Some(airisu_wheel()),
        danger_seed: "#f8b500",
    }
}

/// Every hue as a traditional pigment, ordered by hue.
fn airisu_wheel() -> Vec<LCH> {
    let stops = ["#d0576b", "#e5341b", "#ee7800", "#f8b500", "#d7cf3a", "#b9d08b", "#68be8d", "#00a3af", "#2a83a2", "#4c6cb3", "#7058a3", "#e95295"];
    let mut out: Vec<LCH> = stops.iter().map(|h| hex_lch(h)).collect();
    // sort.Slice is not stable, but the stops' hues are distinct, so any sort gives Go's order.
    out.sort_by(|a, b| a.h.partial_cmp(&b.h).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// A material as lacquer: the wallpaper and the theme only lend their hue, at a lacquer's chroma.
fn airisu_paper(s: &Spec, name: &str, seeds: &Seeds, tone: f64) -> LCH {
    let (mut p, mut ok) = match s.material(name) {
        Some(p) => (p, true),
        None => (LCH::default(), false),
    };
    let hue_of = |hexes: &[&str], p: &mut LCH, ok: &mut bool| {
        for h in hexes {
            if let Some(c) = seed_lch(h) {
                if c.c >= 0.02 {
                    *p = LCH { l: s.paper_l, c: gmin(s.paper_c, c.c * 0.3), h: c.h };
                    *ok = true;
                    return;
                }
            }
        }
    };
    match name {
        "wallpaper" => hue_of(&[&seeds.wallpaper, &seeds.primary], &mut p, &mut ok),
        "theme" => hue_of(&[&seeds.background, &seeds.primary], &mut p, &mut ok),
        _ => {}
    }
    if !ok {
        p = s.material("black").unwrap_or_default();
    }
    tone_paper(s, &mut p, tone, 0.1);
    p
}

/// A seed on the wheel: its hue, and the lightness and chroma of the stops on either side.
fn pigment_of(wheel: &[LCH], seed: LCH) -> LCH {
    let n = wheel.len();
    for i in 0..n {
        let (a, b) = (wheel[i], wheel[(i + 1) % n]);
        let span = (b.h - a.h + 360.0) % 360.0;
        let at = (seed.h - a.h + 360.0) % 360.0;
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
        return LCH { l, c: gmin(a.c + (b.c - a.c) * t, max_chroma(l, seed.h)), h: seed.h };
    }
    seed
}

// ---------------------------------------------------------------- output

/// A style in a few colours: its paper, a raised group, its ink, its accent and three identities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleSwatch {
    pub surface: String,
    pub high: String,
    pub text: String,
    pub accent: String,
    pub dots: Vec<String>,
}

/// Every scheme, so switching scheme in Settings needs no new solve.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub version: i32,
    pub request: String,
    pub schemes: BTreeMap<String, Palette>,
    /// `None` is Go's nil map (`"styles":null`, the palette alone).
    pub styles: Option<BTreeMap<String, BTreeMap<String, StyleSwatch>>>,
}

fn style_ids() -> Vec<String> {
    let mut out = vec!["iris".to_string(), "washi".to_string()];
    out.extend(VARIANTS.iter().map(|v| format!("material:{}", v.name)));
    out
}

/// The palette alone, without the other styles' swatches.
pub fn solve_schemes(req: &Request, key: &str) -> Output {
    solve_schemes_with(req, key, Options::default())
}

pub fn solve_schemes_with(req: &Request, key: &str, opts: Options) -> Output {
    let mut out = Output { version: VERSION, request: key.to_string(), ..Output::default() };
    for s in &language_of(req).schemes {
        out.schemes.insert(s.name.to_string(), build(req, s.name, opts));
    }
    out
}

pub fn solve_all(req: &Request, key: &str) -> Output {
    solve_all_with(req, key, Options::default())
}

pub fn solve_all_with(req: &Request, key: &str, opts: Options) -> Output {
    let mut out = solve_schemes_with(req, key, opts);
    let mut styles = BTreeMap::new();
    for id in style_ids() {
        let mut other = req.clone();
        other.language = id.clone();
        other.variant = String::new();
        // A tile shows the style itself: never Ink's washi or the ink night laid over it.
        other.ink_style = "style".into();
        other.dark_style = "style".into();
        if let Some(v) = id.strip_prefix("material:") {
            other.language = "material".into();
            other.variant = v.into();
        }
        let mut per = BTreeMap::new();
        for s in &language_of(&other).schemes {
            let built;
            let p = if scheme_language(&other, s.name).id != scheme_language(req, s.name).id {
                built = build(&other, s.name, opts);
                &built
            } else {
                &out.schemes[s.name]
            };
            let dot = |n: &str| p.identity.get(n).cloned().unwrap_or_default();
            per.insert(
                s.name.to_string(),
                StyleSwatch {
                    surface: p.surface.clone(),
                    high: p.surface_high.clone(),
                    text: p.text.clone(),
                    accent: p.accent.clone(),
                    dots: vec![dot("blue"), dot("green"), dot("orange")],
                },
            );
        }
        styles.insert(id, per);
    }
    out.styles = Some(styles);
    out
}

impl Output {
    /// The output as Go's json.Marshal writes it.
    pub fn to_json(&self) -> String {
        let mut o = String::with_capacity(32 * 1024);
        o.push_str(&format!("{{\"version\":{},\"request\":", self.version));
        gojson::write_str(&mut o, &self.request);
        o.push_str(",\"schemes\":{");
        for (i, (k, p)) in self.schemes.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            gojson::write_str(&mut o, k);
            o.push(':');
            write_palette(&mut o, p);
        }
        o.push_str("},\"styles\":");
        match &self.styles {
            None => o.push_str("null"),
            Some(styles) => {
                o.push('{');
                for (i, (id, per)) in styles.iter().enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    gojson::write_str(&mut o, id);
                    o.push_str(":{");
                    for (j, (name, sw)) in per.iter().enumerate() {
                        if j > 0 {
                            o.push(',');
                        }
                        gojson::write_str(&mut o, name);
                        o.push_str(":{");
                        write_pairs(&mut o, &[("surface", &sw.surface), ("high", &sw.high), ("text", &sw.text), ("accent", &sw.accent)]);
                        o.push_str(",\"dots\":");
                        write_list(&mut o, &sw.dots);
                        o.push('}');
                    }
                    o.push('}');
                }
                o.push('}');
            }
        }
        o.push('}');
        o
    }
}

fn write_pairs(o: &mut String, pairs: &[(&str, &String)]) {
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        gojson::write_str(o, k);
        o.push(':');
        gojson::write_str(o, v);
    }
}

fn write_list(o: &mut String, items: &[String]) {
    o.push('[');
    for (i, v) in items.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        gojson::write_str(o, v);
    }
    o.push(']');
}

fn write_map(o: &mut String, m: &BTreeMap<String, String>) {
    o.push('{');
    for (i, (k, v)) in m.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        gojson::write_str(o, k);
        o.push(':');
        gojson::write_str(o, v);
    }
    o.push('}');
}

fn write_palette(o: &mut String, p: &Palette) {
    o.push_str(&format!("{{\"dark\":{},", p.dark));
    write_pairs(
        o,
        &[
            ("material", &p.material),
            ("surface", &p.surface),
            ("surfaceHigh", &p.surface_high),
            ("surfaceHighest", &p.surface_highest),
            ("text", &p.text),
            ("textSecondary", &p.text_secondary),
            ("textTertiary", &p.text_tertiary),
            ("fillInk", &p.fill_ink),
            ("accent", &p.accent),
            ("onAccent", &p.on_accent),
            ("accentContainer", &p.accent_container),
            ("onAccentContainer", &p.on_accent_container),
            ("highlight", &p.highlight),
            ("onHighlight", &p.on_highlight),
            ("success", &p.success),
            ("warning", &p.warning),
            ("danger", &p.danger),
            ("onDanger", &p.on_danger),
            ("accentFill", &p.accent_fill),
            ("onAccentFill", &p.on_accent_fill),
            ("dangerFill", &p.danger_fill),
            ("onDangerFill", &p.on_danger_fill),
            ("highlightFill", &p.highlight_fill),
            ("onHighlightFill", &p.on_highlight_fill),
        ],
    );
    o.push_str(",\"identity\":");
    write_map(o, &p.identity);
    o.push_str(",\"widgets\":{");
    for (i, (k, v)) in p.widgets.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        gojson::write_str(o, k);
        o.push(':');
        write_list(o, v);
    }
    o.push_str("},\"materials\":");
    write_map(o, &p.materials);
    o.push_str(",\"accents\":");
    write_map(o, &p.accents);
    o.push_str(",\"highlights\":");
    write_map(o, &p.highlights);
    o.push_str(",\"apps\":");
    write_map(o, &p.apps);
    if let Some(papers) = p.papers.as_ref().filter(|m| !m.is_empty()) {
        o.push_str(",\"papers\":");
        write_map(o, papers);
    }
    o.push('}');
}

/// `inir-washi --request '<json>' [--out file]` / `--default`, with Go's flag parsing and exit codes.
pub fn run_cli(args: &[String], opts: Options) -> i32 {
    let mut request: Option<String> = None;
    let mut out = String::new();
    let mut def = false;
    let usage = "Usage of inir-washi:\n  -default\n    \tsolve a fresh install's palette\n  -out string\n    \twrite the palette here (atomically) as well as to stdout\n  -request string\n    \trequest as JSON\n";
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            break;
        }
        let Some(flag) = a.strip_prefix("--").or_else(|| a.strip_prefix('-')).filter(|f| !f.is_empty() && !a.starts_with("---")) else {
            break; // the first non-flag argument ends Go's flag parsing
        };
        let (name, inline) = match flag.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (flag, None),
        };
        match name {
            "request" | "out" => {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match args.get(i) {
                            Some(v) => v.clone(),
                            None => {
                                eprint!("flag needs an argument: -{name}\n{usage}");
                                return 2;
                            }
                        }
                    }
                };
                if name == "request" {
                    request = Some(v);
                } else {
                    out = v;
                }
            }
            "default" => match inline.as_deref() {
                None | Some("1" | "t" | "T" | "true" | "TRUE" | "True") => def = true,
                Some("0" | "f" | "F" | "false" | "FALSE" | "False") => def = false,
                Some(v) => {
                    eprint!("invalid boolean value {v:?} for -default: parse error\n{usage}");
                    return 2;
                }
            },
            "h" | "help" => {
                eprint!("{usage}");
                return 0;
            }
            _ => {
                eprint!("flag provided but not defined: -{name}\n{usage}");
                return 2;
            }
        }
        i += 1;
    }
    let mut raw = request.unwrap_or_default();
    let req = if def {
        let r = default_request();
        raw = encode_request(&r);
        r
    } else {
        if raw.is_empty() {
            eprintln!("inir-washi: --request or --default");
            return 2;
        }
        match decode_request(&raw) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("inir-washi: {e}");
                return 2;
            }
        }
    };
    let data = solve_all_with(&req, &raw, opts).to_json();
    if !out.is_empty() {
        let path = std::path::Path::new(&out);
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        let base = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = dir.join(format!(".{base}.tmp"));
        let res = std::fs::write(&tmp, &data).and_then(|_| std::fs::rename(&tmp, path));
        if let Err(e) = res {
            eprintln!("inir-washi: {e}");
        }
    }
    use std::io::Write;
    let _ = std::io::stdout().write_all(data.as_bytes());
    0
}
