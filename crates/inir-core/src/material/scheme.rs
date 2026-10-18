//! materialyoucolor 3.0.4's dynamic colour (`palettes`, `temperature`, `dislike`, `contrast`, `dynamiccolor`), in
//! the original's order of operations. Only what generate_colors_material.py reaches: platform "phone", the
//! default spec 2025 and its fallback to 2021 for content, fidelity, monochrome, rainbow and fruit salad.
//!
//! The library builds every colour from the 2025 spec object, whose methods extend the 2021 ones: each field picks
//! its 2025 or 2021 definition by the scheme's spec version, the four `*Dim` colours exist only in 2025 and the
//! palette key colours, shadow and scrim only in 2021. `Which` is that choice; references between colours go through it again.

use super::hct::{clamp_double, lab_from_argb, lerp, lstar_from_y, sanitize_degrees_double, sanitize_degrees_int, y_from_lstar, Hct};
use std::f64::consts::PI;

/// Python's `round()` on a float: ties go to the even neighbour.
fn py_round(x: f64) -> f64 {
    x.round_ties_even()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    Monochrome,
    Neutral,
    TonalSpot,
    Vibrant,
    Expressive,
    Fidelity,
    Content,
    Rainbow,
    FruitSalad,
}

impl Variant {
    /// The script's `--scheme` names; anything else is Tonal Spot, as its import chain falls through.
    pub fn from_scheme_name(name: &str) -> Variant {
        match name {
            "scheme-fruit-salad" => Variant::FruitSalad,
            "scheme-expressive" => Variant::Expressive,
            "scheme-monochrome" => Variant::Monochrome,
            "scheme-rainbow" => Variant::Rainbow,
            "scheme-neutral" => Variant::Neutral,
            "scheme-fidelity" => Variant::Fidelity,
            "scheme-content" => Variant::Content,
            "scheme-vibrant" => Variant::Vibrant,
            _ => Variant::TonalSpot,
        }
    }
}

// ---------------------------------------------------------------- palettes

fn key_color(hue: f64, requested_chroma: f64) -> Hct {
    let mut cache: [Option<f64>; 102] = [None; 102];
    let mut max_chroma = |tone: i64| -> f64 {
        let i = tone as usize;
        if let Some(c) = cache[i] {
            return c;
        }
        let c = Hct::from_hct(hue, 200.0, tone as f64).chroma;
        cache[i] = Some(c);
        c
    };
    let pivot_tone = 50i64;
    let epsilon = 0.01;
    let (mut lower_tone, mut upper_tone) = (0i64, 100i64);
    while lower_tone < upper_tone {
        let mid_tone = (lower_tone + upper_tone).div_euclid(2);
        let is_ascending = max_chroma(mid_tone) < max_chroma(mid_tone + 1);
        let sufficient_chroma = max_chroma(mid_tone) >= requested_chroma - epsilon;
        if sufficient_chroma {
            if (lower_tone - pivot_tone).abs() < (upper_tone - pivot_tone).abs() {
                upper_tone = mid_tone;
            } else {
                if lower_tone == mid_tone {
                    return Hct::from_hct(hue, requested_chroma, lower_tone as f64);
                }
                lower_tone = mid_tone;
            }
        } else if is_ascending {
            lower_tone = mid_tone + 1;
        } else {
            upper_tone = mid_tone;
        }
    }
    Hct::from_hct(hue, requested_chroma, lower_tone as f64)
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub hue: f64,
    pub chroma: f64,
    pub key: Hct,
}

impl Palette {
    pub fn from_hct(hct: Hct) -> Palette {
        Palette { hue: hct.hue, chroma: hct.chroma, key: hct }
    }

    pub fn from_hue_and_chroma(hue: f64, chroma: f64) -> Palette {
        Palette { hue, chroma, key: key_color(hue, chroma) }
    }

    pub fn tone(&self, tone: f64) -> u32 {
        if tone == 99.0 && Hct::is_yellow(self.hue) {
            average_argb(self.tone(98.0), self.tone(100.0))
        } else {
            Hct::from_hct(self.hue, self.chroma, tone).argb
        }
    }

    pub fn get_hct(&self, tone: f64) -> Hct {
        Hct::from_int(self.tone(tone))
    }
}

fn average_argb(a: u32, b: u32) -> u32 {
    let mix = |s: u32| -> u32 { py_round((((a >> s) & 255) + ((b >> s) & 255)) as f64 / 2.0) as u32 & 255 };
    255 << 24 | mix(16) << 16 | mix(8) << 8 | mix(0)
}

// ---------------------------------------------------------------- temperature and dislike

struct Temperature {
    input: Hct,
    /// `hcts_by_hue` (hues 0..=360) followed by the input, as the original builds its dictionary.
    hcts: Vec<Hct>,
    temps: Vec<f64>,
    /// Indices into `hcts`, coldest first (Python's stable sort).
    by_temp: Vec<usize>,
}

impl Temperature {
    fn new(input: Hct) -> Temperature {
        let mut hcts: Vec<Hct> = (0..=360).map(|h| Hct::from_hct(h as f64, input.chroma, input.tone)).collect();
        hcts.push(input);
        let temps: Vec<f64> = hcts.iter().map(|h| raw_temperature(h.argb)).collect();
        let mut by_temp: Vec<usize> = (0..hcts.len()).collect();
        by_temp.sort_by(|&a, &b| temps[a].partial_cmp(&temps[b]).unwrap_or(std::cmp::Ordering::Equal));
        Temperature { input, hcts, temps, by_temp }
    }

    const INPUT: usize = 361;

    fn coldest(&self) -> usize {
        self.by_temp[0]
    }

    fn warmest(&self) -> usize {
        self.by_temp[self.by_temp.len() - 1]
    }

    fn relative_temperature(&self, i: usize) -> f64 {
        let range = self.temps[self.warmest()] - self.temps[self.coldest()];
        let difference_from_coldest = self.temps[i] - self.temps[self.coldest()];
        if range == 0.0 {
            return 0.5;
        }
        difference_from_coldest / range
    }

    fn analogous(&self, count: usize, divisions: usize) -> Vec<Hct> {
        let start_hue = py_round(self.input.hue) as i64;
        let start = start_hue as usize;
        let mut last_temp = self.relative_temperature(start);
        let mut all: Vec<usize> = vec![start];
        let mut absolute_total_temp_delta = 0.0;
        for i in 0..360 {
            let hue = sanitize_degrees_int(start_hue + i) as usize;
            let temp = self.relative_temperature(hue);
            let temp_delta = (temp - last_temp).abs();
            last_temp = temp;
            absolute_total_temp_delta += temp_delta;
        }
        let mut hue_addend = 1i64;
        let temp_step = absolute_total_temp_delta / divisions as f64;
        let mut total_temp_delta = 0.0;
        last_temp = self.relative_temperature(start);
        while all.len() < divisions {
            let hue = sanitize_degrees_int(start_hue + hue_addend) as usize;
            let temp = self.relative_temperature(hue);
            let temp_delta = (temp - last_temp).abs();
            total_temp_delta += temp_delta;
            let mut desired = all.len() as f64 * temp_step;
            let mut index_satisfied = total_temp_delta >= desired;
            let mut index_addend = 1usize;
            while index_satisfied && all.len() < divisions {
                all.push(hue);
                desired = (all.len() + index_addend) as f64 * temp_step;
                index_satisfied = total_temp_delta >= desired;
                index_addend += 1;
            }
            last_temp = temp;
            hue_addend += 1;
            if hue_addend > 360 {
                while all.len() < divisions {
                    all.push(hue);
                }
                break;
            }
        }
        let mut answers = vec![Temperature::INPUT];
        let increase_hue_count = ((count as f64 - 1.0) / 2.0).floor() as i64;
        let n = all.len() as i64;
        for i in 1..=increase_hue_count {
            let mut index = -i;
            while index < 0 {
                index += n;
            }
            if index >= n {
                index %= n;
            }
            answers.insert(0, all[index as usize]);
        }
        let decrease_hue_count = count as i64 - increase_hue_count - 1;
        for i in 1..=decrease_hue_count {
            let mut index = i;
            if index >= n {
                index %= n;
            }
            answers.push(all[index as usize]);
        }
        answers.into_iter().map(|i| self.hcts[i]).collect()
    }

    fn complement(&self) -> Hct {
        let (coldest, warmest) = (self.coldest(), self.warmest());
        let coldest_hue = self.hcts[coldest].hue;
        let coldest_temp = self.temps[coldest];
        let warmest_hue = self.hcts[warmest].hue;
        let warmest_temp = self.temps[warmest];
        let range = warmest_temp - coldest_temp;
        let input_index = py_round(self.input.hue) as usize;
        if range == 0.0 {
            return self.hcts[input_index];
        }
        let coldest_to_warmest = is_between(self.input.hue, coldest_hue, warmest_hue);
        let start_hue = if coldest_to_warmest { warmest_hue } else { coldest_hue };
        let end_hue = if coldest_to_warmest { coldest_hue } else { warmest_hue };
        let direction_of_rotation = 1.0;
        let mut smallest_error = 1000.0;
        let mut answer = input_index;
        let complement_relative_temp = 1.0 - self.relative_temperature(Temperature::INPUT);
        for hue_addend in 0..=360 {
            let hue = sanitize_degrees_double(start_hue + direction_of_rotation * hue_addend as f64);
            if !is_between(hue, start_hue, end_hue) {
                continue;
            }
            let possible = py_round(hue) as usize;
            let relative_temp = (self.temps[possible] - coldest_temp) / range;
            let error = (complement_relative_temp - relative_temp).abs();
            if error < smallest_error {
                smallest_error = error;
                answer = possible;
            }
        }
        self.hcts[answer]
    }
}

fn is_between(angle: f64, a: f64, b: f64) -> bool {
    if a < b {
        a <= angle && angle <= b
    } else {
        a <= angle || angle <= b
    }
}

fn raw_temperature(argb: u32) -> f64 {
    let lab = lab_from_argb(argb);
    let hue = sanitize_degrees_double(lab[2].atan2(lab[1]) * 180.0 / PI);
    let chroma = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
    -0.5 + 0.02 * chroma.powf(1.07) * (sanitize_degrees_double(hue - 50.0) * PI / 180.0).cos()
}

fn is_disliked(hct: &Hct) -> bool {
    let hue_passes = py_round(hct.hue) >= 90.0 && py_round(hct.hue) <= 111.0;
    let chroma_passes = py_round(hct.chroma) > 16.0;
    let tone_passes = py_round(hct.tone) < 65.0;
    hue_passes && chroma_passes && tone_passes
}

fn fix_if_disliked(hct: Hct) -> Hct {
    if is_disliked(&hct) {
        return Hct::from_hct(hct.hue, hct.chroma, 70.0);
    }
    hct
}

// ---------------------------------------------------------------- contrast

fn ratio_of_tones(a: f64, b: f64) -> f64 {
    let a = clamp_double(0.0, 100.0, a);
    let b = clamp_double(0.0, 100.0, b);
    ratio_of_ys(y_from_lstar(a), y_from_lstar(b))
}

fn ratio_of_ys(y1: f64, y2: f64) -> f64 {
    let lighter = y1.max(y2);
    let darker = if lighter == y2 { y1 } else { y2 };
    (lighter + 5.0) / (darker + 5.0)
}

fn lighter(tone: f64, ratio: f64) -> f64 {
    if !(0.0..=100.0).contains(&tone) {
        return -1.0;
    }
    let dark_y = y_from_lstar(tone);
    let light_y = ratio * (dark_y + 5.0) - 5.0;
    let real_contrast = ratio_of_ys(light_y, dark_y);
    let delta = (real_contrast - ratio).abs();
    if real_contrast < ratio && delta > 0.04 {
        return -1.0;
    }
    let v = lstar_from_y(light_y) + 0.4;
    if !(0.0..=100.0).contains(&v) {
        return -1.0;
    }
    v
}

fn darker(tone: f64, ratio: f64) -> f64 {
    if !(0.0..=100.0).contains(&tone) {
        return -1.0;
    }
    let light_y = y_from_lstar(tone);
    let dark_y = (light_y + 5.0) / ratio - 5.0;
    let real_contrast = ratio_of_ys(light_y, dark_y);
    let delta = (real_contrast - ratio).abs();
    if real_contrast < ratio && delta > 0.04 {
        return -1.0;
    }
    let v = lstar_from_y(dark_y) - 0.4;
    if !(0.0..=100.0).contains(&v) {
        return -1.0;
    }
    v
}

fn lighter_unsafe(tone: f64, ratio: f64) -> f64 {
    let v = lighter(tone, ratio);
    if v < 0.0 { 100.0 } else { v }
}

fn darker_unsafe(tone: f64, ratio: f64) -> f64 {
    let v = darker(tone, ratio);
    if v < 0.0 { 0.0 } else { v }
}

fn foreground_tone(bg_tone: f64, ratio: f64) -> f64 {
    let lighter_tone = lighter_unsafe(bg_tone, ratio);
    let darker_tone = darker_unsafe(bg_tone, ratio);
    let lighter_ratio = ratio_of_tones(lighter_tone, bg_tone);
    let darker_ratio = ratio_of_tones(darker_tone, bg_tone);
    if tone_prefers_light_foreground(bg_tone) {
        let negligible = (lighter_ratio - darker_ratio).abs() < 0.1 && lighter_ratio < ratio && darker_ratio < ratio;
        if lighter_ratio >= ratio || lighter_ratio >= darker_ratio || negligible { lighter_tone } else { darker_tone }
    } else if darker_ratio >= ratio || darker_ratio >= lighter_ratio {
        darker_tone
    } else {
        lighter_tone
    }
}

fn tone_prefers_light_foreground(tone: f64) -> bool {
    py_round(tone) < 60.0
}

#[derive(Clone, Copy)]
struct Curve(f64, f64, f64, f64);

impl Curve {
    fn get(&self, level: f64) -> f64 {
        let Curve(low, normal, medium, high) = *self;
        if level <= -1.0 {
            low
        } else if level < 0.0 {
            lerp(low, normal, (level - -1.0) / 1.0)
        } else if level < 0.5 {
            lerp(normal, medium, (level - 0.0) / 0.5)
        } else if level < 1.0 {
            lerp(medium, high, (level - 0.5) / 0.5)
        } else {
            high
        }
    }
}

fn get_curve(default_contrast: f64) -> Curve {
    match default_contrast {
        x if x == 1.5 => Curve(1.5, 1.5, 3.0, 5.5),
        x if x == 3.0 => Curve(3.0, 3.0, 4.5, 7.0),
        x if x == 4.5 => Curve(4.5, 4.5, 7.0, 11.0),
        x if x == 6.0 => Curve(6.0, 6.0, 7.0, 11.0),
        x if x == 7.0 => Curve(7.0, 7.0, 11.0, 21.0),
        x if x == 9.0 => Curve(9.0, 9.0, 11.0, 21.0),
        x if x == 11.0 => Curve(11.0, 11.0, 21.0, 21.0),
        x if x == 21.0 => Curve(21.0, 21.0, 21.0, 21.0),
        x => Curve(x, x, 7.0, 21.0),
    }
}

// ---------------------------------------------------------------- scheme

#[derive(Clone, Debug)]
pub struct Scheme {
    pub source: Hct,
    pub variant: Variant,
    pub contrast: f64,
    pub dark: bool,
    pub spec2025: bool,
    pub primary: Palette,
    pub secondary: Palette,
    pub tertiary: Palette,
    pub neutral: Palette,
    pub neutral_variant: Palette,
    pub error: Palette,
}

fn piecewise_hue(source: &Hct, breakpoints: &[f64], hues: &[f64]) -> f64 {
    let size = (breakpoints.len() - 1).min(hues.len());
    let h = source.hue;
    for i in 0..size {
        if h >= breakpoints[i] && h < breakpoints[i + 1] {
            return sanitize_degrees_double(hues[i]);
        }
    }
    h
}

fn rotated_hue(source: &Hct, breakpoints: &[f64], rotations: &[f64]) -> f64 {
    let mut rotation = piecewise_hue(source, breakpoints, rotations);
    if (breakpoints.len() - 1).min(rotations.len()) == 0 {
        rotation = 0.0;
    }
    sanitize_degrees_double(source.hue + rotation)
}

impl Scheme {
    pub fn new(source: Hct, variant: Variant, dark: bool, contrast: f64) -> Scheme {
        use Variant::*;
        let spec2025 = matches!(variant, Expressive | Vibrant | TonalSpot | Neutral);
        let p = Palette::from_hue_and_chroma;
        let (h, c) = (source.hue, source.chroma);
        let fallback = || -> [Palette; 5] {
            [
                match variant {
                    Content | Fidelity => p(h, c),
                    FruitSalad => p(sanitize_degrees_double(h - 50.0), 48.0),
                    Monochrome => p(h, 0.0),
                    Neutral => p(h, 12.0),
                    Rainbow => p(h, 48.0),
                    TonalSpot => p(h, 36.0),
                    Expressive => p(sanitize_degrees_double(h + 240.0), 40.0),
                    Vibrant => p(h, 200.0),
                },
                match variant {
                    Content | Fidelity => p(h, (c - 32.0).max(c * 0.5)),
                    FruitSalad => p(sanitize_degrees_double(h - 50.0), 36.0),
                    Monochrome => p(h, 0.0),
                    Neutral => p(h, 8.0),
                    Rainbow | TonalSpot => p(h, 16.0),
                    Expressive => p(
                        rotated_hue(&source, &[0., 21., 51., 121., 151., 191., 271., 321., 360.], &[45., 95., 45., 20., 45., 90., 45., 45., 45.]),
                        24.0,
                    ),
                    Vibrant => p(
                        rotated_hue(&source, &[0., 41., 61., 101., 131., 181., 251., 301., 360.], &[18., 15., 10., 12., 15., 18., 15., 12., 12.]),
                        24.0,
                    ),
                },
                match variant {
                    Content => Palette::from_hct(fix_if_disliked(Temperature::new(source).analogous(3, 6)[2])),
                    Fidelity => Palette::from_hct(fix_if_disliked(Temperature::new(source).complement())),
                    FruitSalad => p(h, 36.0),
                    Monochrome => p(h, 0.0),
                    Neutral => p(h, 16.0),
                    Rainbow | TonalSpot => p(sanitize_degrees_double(h + 60.0), 24.0),
                    Expressive => p(
                        rotated_hue(&source, &[0., 21., 51., 121., 151., 191., 271., 321., 360.], &[120., 120., 20., 45., 20., 15., 20., 120., 120.]),
                        32.0,
                    ),
                    Vibrant => p(
                        rotated_hue(&source, &[0., 41., 61., 101., 131., 181., 251., 301., 360.], &[35., 30., 20., 25., 30., 35., 30., 25., 25.]),
                        32.0,
                    ),
                },
                match variant {
                    Content | Fidelity => p(h, c / 8.0),
                    FruitSalad => p(h, 10.0),
                    Monochrome | Rainbow => p(h, 0.0),
                    Neutral => p(h, 2.0),
                    TonalSpot => p(h, 6.0),
                    Expressive => p(sanitize_degrees_double(h + 15.0), 8.0),
                    Vibrant => p(h, 10.0),
                },
                match variant {
                    Content | Fidelity => p(h, c / 8.0 + 4.0),
                    FruitSalad => p(h, 16.0),
                    Monochrome | Rainbow => p(h, 0.0),
                    Neutral => p(h, 2.0),
                    TonalSpot => p(h, 8.0),
                    Expressive => p(sanitize_degrees_double(h + 15.0), 12.0),
                    Vibrant => p(h, 12.0),
                },
            ]
        };
        let [primary, secondary, tertiary, neutral, neutral_variant, error] = if spec2025 {
            let expressive_neutral_hue = rotated_hue(&source, &[0., 71., 124., 253., 278., 300., 360.], &[10., 0., 10., 0., 10., 0.]);
            let expressive_neutral_chroma = if dark && Hct::is_yellow(expressive_neutral_hue) {
                6.0
            } else if dark {
                14.0
            } else {
                18.0
            };
            let vibrant_neutral_hue = rotated_hue(&source, &[0., 38., 105., 140., 333., 360.], &[-14., 10., -14., 10., -14.]);
            let vibrant_neutral_chroma = 28.0;
            let error_hue = piecewise_hue(&source, &[0., 3., 13., 23., 33., 43., 153., 273., 360.], &[12., 22., 32., 12., 22., 32., 22., 12.]);
            match variant {
                Neutral => [
                    p(h, if Hct::is_blue(h) { 12.0 } else { 8.0 }),
                    p(h, if Hct::is_blue(h) { 6.0 } else { 4.0 }),
                    p(rotated_hue(&source, &[0., 38., 105., 161., 204., 278., 333., 360.], &[-32., 26., 10., -39., 24., -15., -32.]), 20.0),
                    p(h, 1.4),
                    p(h, 1.4 * 2.2),
                    p(error_hue, 50.0),
                ],
                TonalSpot => [
                    p(h, if dark { 26.0 } else { 32.0 }),
                    p(h, 16.0),
                    p(rotated_hue(&source, &[0., 20., 71., 161., 333., 360.], &[-40., 48., -32., 40., -32.]), 28.0),
                    p(h, 5.0),
                    p(h, 5.0 * 1.7),
                    p(error_hue, 60.0),
                ],
                Expressive => [
                    p(h, if dark { 36.0 } else { 48.0 }),
                    p(
                        rotated_hue(&source, &[0., 105., 140., 204., 253., 278., 300., 333., 360.], &[-160., 155., -100., 96., -96., -156., -165., -160.]),
                        if dark { 16.0 } else { 24.0 },
                    ),
                    p(rotated_hue(&source, &[0., 105., 140., 204., 253., 278., 300., 333., 360.], &[-165., 160., -105., 101., -101., -160., -170., -165.]), 48.0),
                    p(expressive_neutral_hue, expressive_neutral_chroma),
                    p(
                        expressive_neutral_hue,
                        expressive_neutral_chroma * if (105.0..125.0).contains(&expressive_neutral_hue) { 1.6 } else { 2.3 },
                    ),
                    p(error_hue, 64.0),
                ],
                _ => [
                    p(h, 74.0),
                    p(rotated_hue(&source, &[0., 38., 105., 140., 333., 360.], &[-14., 10., -14., 10., -14.]), 56.0),
                    p(rotated_hue(&source, &[0., 38., 71., 105., 140., 161., 253., 333., 360.], &[-72., 35., 24., -24., 62., 50., 62., -72.]), 56.0),
                    p(vibrant_neutral_hue, vibrant_neutral_chroma),
                    p(vibrant_neutral_hue, vibrant_neutral_chroma * 1.29),
                    p(error_hue, 80.0),
                ],
            }
        } else {
            let [a, b, c, d, e] = fallback();
            [a, b, c, d, e, p(25.0, 84.0)]
        };
        Scheme { source, variant, contrast, dark, spec2025, primary, secondary, tertiary, neutral, neutral_variant, error }
    }

    fn palette(&self, p: Pal) -> &Palette {
        match p {
            Pal::Primary => &self.primary,
            Pal::Secondary => &self.secondary,
            Pal::Tertiary => &self.tertiary,
            Pal::Neutral => &self.neutral,
            Pal::NeutralVariant => &self.neutral_variant,
            Pal::Error => &self.error,
        }
    }

    /// The light, contrast-0 copy the `*Fixed` tones are read from.
    fn light_twin(&self) -> Scheme {
        Scheme { dark: false, contrast: 0.0, ..self.clone() }
    }

    pub fn argb(&self, role: Role) -> u32 {
        get_hct(role, self).argb
    }
}

// ---------------------------------------------------------------- colours

macro_rules! roles {
    ($($r:ident = $n:literal),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Role { $($r),* }
        /// `vars(MaterialDynamicColors)` order, which is the order the script writes.
        pub const ROLES: &[Role] = &[$(Role::$r),*];
        impl Role {
            pub fn name(self) -> &'static str {
                match self { $(Role::$r => $n),* }
            }
        }
    };
}

roles! {
    Background = "background", OnBackground = "onBackground", Surface = "surface", SurfaceDim = "surfaceDim",
    SurfaceBright = "surfaceBright", SurfaceContainerLowest = "surfaceContainerLowest",
    SurfaceContainerLow = "surfaceContainerLow", SurfaceContainer = "surfaceContainer",
    SurfaceContainerHigh = "surfaceContainerHigh", SurfaceContainerHighest = "surfaceContainerHighest",
    OnSurface = "onSurface", SurfaceVariant = "surfaceVariant", OnSurfaceVariant = "onSurfaceVariant",
    Outline = "outline", OutlineVariant = "outlineVariant", InverseSurface = "inverseSurface",
    InverseOnSurface = "inverseOnSurface", Shadow = "shadow", Scrim = "scrim", SurfaceTint = "surfaceTint",
    Primary = "primary", PrimaryDim = "primaryDim", OnPrimary = "onPrimary", PrimaryContainer = "primaryContainer",
    OnPrimaryContainer = "onPrimaryContainer", InversePrimary = "inversePrimary", PrimaryFixed = "primaryFixed",
    PrimaryFixedDim = "primaryFixedDim", OnPrimaryFixed = "onPrimaryFixed",
    OnPrimaryFixedVariant = "onPrimaryFixedVariant", Secondary = "secondary", SecondaryDim = "secondaryDim",
    OnSecondary = "onSecondary", SecondaryContainer = "secondaryContainer",
    OnSecondaryContainer = "onSecondaryContainer", SecondaryFixed = "secondaryFixed",
    SecondaryFixedDim = "secondaryFixedDim", OnSecondaryFixed = "onSecondaryFixed",
    OnSecondaryFixedVariant = "onSecondaryFixedVariant", Tertiary = "tertiary", TertiaryDim = "tertiaryDim",
    OnTertiary = "onTertiary", TertiaryContainer = "tertiaryContainer", OnTertiaryContainer = "onTertiaryContainer",
    TertiaryFixed = "tertiaryFixed", TertiaryFixedDim = "tertiaryFixedDim", OnTertiaryFixed = "onTertiaryFixed",
    OnTertiaryFixedVariant = "onTertiaryFixedVariant", Error = "error", ErrorDim = "errorDim", OnError = "onError",
    ErrorContainer = "errorContainer", OnErrorContainer = "onErrorContainer",
    PrimaryPaletteKeyColor = "primaryPaletteKeyColor", SecondaryPaletteKeyColor = "secondaryPaletteKeyColor",
    TertiaryPaletteKeyColor = "tertiaryPaletteKeyColor", NeutralPaletteKeyColor = "neutralPaletteKeyColor",
    NeutralVariantPaletteKeyColor = "neutralVariantPaletteKeyColor", ErrorPaletteKeyColor = "errorPaletteKeyColor",
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pal {
    Primary,
    Secondary,
    Tertiary,
    Neutral,
    NeutralVariant,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Polarity {
    Darker,
    Lighter,
    Nearer,
    RelativeLighter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Constraint {
    Exact,
    Farther,
}

#[derive(Clone, Copy)]
struct Pair {
    a: Role,
    b: Role,
    delta: f64,
    polarity: Polarity,
    stay_together: bool,
    constraint: Constraint,
}

fn pair(a: Role, b: Role, delta: f64, polarity: Polarity, stay_together: bool, constraint: Constraint) -> Option<Pair> {
    Some(Pair { a, b, delta, polarity, stay_together, constraint })
}

/// Everything about a colour except its tone, which can recurse and is evaluated only when the calculation asks.
struct Meta {
    palette: Pal,
    is_background: bool,
    chroma_multiplier: f64,
    background: Option<Role>,
    second_background: Option<Role>,
    curve: Option<Curve>,
    pair: Option<Pair>,
}

fn meta(
    palette: Pal,
    is_background: bool,
    chroma_multiplier: f64,
    background: Option<Role>,
    second_background: Option<Role>,
    curve: Option<Curve>,
    pair: Option<Pair>,
) -> Meta {
    Meta { palette, is_background, chroma_multiplier, background, second_background, curve, pair }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    V2021,
    V2025,
}

fn which(r: Role, s: &Scheme) -> Which {
    use Role::*;
    match r {
        PrimaryDim | SecondaryDim | TertiaryDim | ErrorDim => Which::V2025,
        Shadow | Scrim | PrimaryPaletteKeyColor | SecondaryPaletteKeyColor | TertiaryPaletteKeyColor
        | NeutralPaletteKeyColor | NeutralVariantPaletteKeyColor | ErrorPaletteKeyColor => Which::V2021,
        _ if s.spec2025 => Which::V2025,
        _ => Which::V2021,
    }
}

fn is_monochrome(s: &Scheme) -> bool {
    s.variant == Variant::Monochrome
}

fn is_fidelity(s: &Scheme) -> bool {
    matches!(s.variant, Variant::Fidelity | Variant::Content)
}

fn highest_surface(s: &Scheme) -> Role {
    if s.dark { Role::SurfaceBright } else { Role::SurfaceDim }
}

fn find_desired_chroma_by_tone(hue: f64, chroma: f64, tone: f64, by_decreasing_tone: bool) -> f64 {
    let mut answer = tone;
    let mut closest = Hct::from_hct(hue, chroma, tone);
    if closest.chroma < chroma {
        let mut chroma_peak = closest.chroma;
        while closest.chroma < chroma {
            answer += if by_decreasing_tone { -1.0 } else { 1.0 };
            let potential = Hct::from_hct(hue, chroma, answer);
            if chroma_peak > potential.chroma {
                break;
            }
            if (potential.chroma - chroma).abs() < 0.4 {
                break;
            }
            if (potential.chroma - chroma).abs() < (closest.chroma - chroma).abs() {
                closest = potential;
            }
            chroma_peak = chroma_peak.max(potential.chroma);
        }
    }
    answer
}

fn find_best_tone_for_chroma(hue: f64, chroma: f64, mut tone: f64, by_decreasing_tone: bool) -> f64 {
    let mut answer = tone;
    let mut best = Hct::from_hct(hue, chroma, answer);
    while best.chroma < chroma {
        if !(0.0..=100.0).contains(&tone) {
            break;
        }
        tone += if by_decreasing_tone { -1.0 } else { 1.0 };
        let candidate = Hct::from_hct(hue, chroma, tone);
        if best.chroma < candidate.chroma {
            best = candidate;
            answer = tone;
        }
    }
    answer
}

fn t_max_c(p: &Palette, lower: f64, upper: f64, chroma_multiplier: f64) -> f64 {
    clamp_double(lower, upper, find_best_tone_for_chroma(p.hue, p.chroma * chroma_multiplier, 100.0, true))
}

fn t_min_c(p: &Palette, lower: f64, upper: f64) -> f64 {
    clamp_double(lower, upper, find_best_tone_for_chroma(p.hue, p.chroma, 0.0, false))
}

fn tmax(p: &Palette) -> f64 {
    t_max_c(p, 0.0, 100.0, 1.0)
}

fn tmax_in(p: &Palette, lower: f64, upper: f64) -> f64 {
    t_max_c(p, lower, upper, 1.0)
}

fn yellow_neutral(s: &Scheme) -> bool {
    Hct::is_yellow(s.neutral.hue)
}

/// The surface ladder's chroma multipliers in 2025: neutral, tonal spot, expressive (yellow / not), vibrant.
fn ladder(s: &Scheme, t: [f64; 5]) -> f64 {
    match s.variant {
        Variant::Neutral => t[0],
        Variant::TonalSpot => t[1],
        Variant::Expressive => {
            if yellow_neutral(s) {
                t[2]
            } else {
                t[3]
            }
        }
        Variant::Vibrant => t[4],
        _ => 1.0,
    }
}

/// onSurface, onSurfaceVariant, outline and outlineVariant share one multiplier in 2025.
fn on_surface_multiplier(s: &Scheme) -> f64 {
    match s.variant {
        Variant::Neutral => 2.2,
        Variant::TonalSpot => 1.7,
        Variant::Expressive => {
            if yellow_neutral(s) && s.dark {
                3.0
            } else if yellow_neutral(s) {
                2.3
            } else {
                1.6
            }
        }
        _ => 1.0,
    }
}

/// The surface ladder's tones in 2025: dark, yellow neutral, vibrant, otherwise.
fn surface_tone(s: &Scheme, t: [f64; 4]) -> f64 {
    if s.dark {
        t[0]
    } else if yellow_neutral(s) {
        t[1]
    } else if s.variant == Variant::Vibrant {
        t[2]
    } else {
        t[3]
    }
}

fn container_curve(s: &Scheme) -> Option<Curve> {
    if s.contrast > 0.0 { Some(get_curve(1.5)) } else { None }
}

fn meta_of(r: Role, s: &Scheme) -> Meta {
    match which(r, s) {
        Which::V2021 => meta_2021(r, s),
        Which::V2025 => meta_2025(r, s),
    }
}

fn meta_2021(r: Role, s: &Scheme) -> Meta {
    use Pal::*;
    use Role::*;
    let hs = Some(highest_surface(s));
    let c = |a, b, c, d| Some(Curve(a, b, c, d));
    let nearer = |container, main| pair(container, main, 10.0, Polarity::Nearer, false, Constraint::Exact);
    let lighter = |fixed, dim| pair(fixed, dim, 10.0, Polarity::Lighter, true, Constraint::Exact);
    match r {
        PrimaryPaletteKeyColor => meta(Pal::Primary, false, 1.0, None, None, None, None),
        SecondaryPaletteKeyColor => meta(Pal::Secondary, false, 1.0, None, None, None, None),
        TertiaryPaletteKeyColor => meta(Pal::Tertiary, false, 1.0, None, None, None, None),
        NeutralPaletteKeyColor => meta(Neutral, false, 1.0, None, None, None, None),
        NeutralVariantPaletteKeyColor => meta(NeutralVariant, false, 1.0, None, None, None, None),
        ErrorPaletteKeyColor => meta(Pal::Error, false, 1.0, None, None, None, None),
        Background | Surface | SurfaceDim | SurfaceBright | SurfaceContainerLowest | SurfaceContainerLow
        | SurfaceContainer | SurfaceContainerHigh | SurfaceContainerHighest | InverseSurface => {
            meta(Neutral, true, 1.0, None, None, None, None)
        }
        OnBackground => meta(Neutral, false, 1.0, Some(Background), None, c(3.0, 3.0, 4.5, 7.0), None),
        OnSurface => meta(Neutral, false, 1.0, hs, None, c(4.5, 7.0, 11.0, 21.0), None),
        SurfaceVariant => meta(NeutralVariant, true, 1.0, None, None, None, None),
        OnSurfaceVariant => meta(NeutralVariant, false, 1.0, hs, None, c(3.0, 4.5, 7.0, 11.0), None),
        InverseOnSurface => meta(Neutral, false, 1.0, Some(InverseSurface), None, c(4.5, 7.0, 11.0, 21.0), None),
        Outline => meta(NeutralVariant, false, 1.0, hs, None, c(1.5, 3.0, 4.5, 7.0), None),
        OutlineVariant => meta(NeutralVariant, false, 1.0, hs, None, c(1.0, 1.0, 3.0, 4.5), None),
        Shadow | Scrim => meta(Neutral, false, 1.0, None, None, None, None),
        SurfaceTint => meta(Pal::Primary, true, 1.0, None, None, None, None),
        Role::Primary => meta(Pal::Primary, true, 1.0, hs, None, c(3.0, 4.5, 7.0, 7.0), nearer(PrimaryContainer, Role::Primary)),
        OnPrimary => meta(Pal::Primary, false, 1.0, Some(Role::Primary), None, c(4.5, 7.0, 11.0, 21.0), None),
        PrimaryContainer => meta(Pal::Primary, true, 1.0, hs, None, c(1.0, 1.0, 3.0, 4.5), nearer(PrimaryContainer, Role::Primary)),
        OnPrimaryContainer => meta(Pal::Primary, false, 1.0, Some(PrimaryContainer), None, c(3.0, 4.5, 7.0, 11.0), None),
        InversePrimary => meta(Pal::Primary, false, 1.0, Some(InverseSurface), None, c(3.0, 4.5, 7.0, 7.0), None),
        Role::Secondary => {
            meta(Pal::Secondary, true, 1.0, hs, None, c(3.0, 4.5, 7.0, 7.0), nearer(SecondaryContainer, Role::Secondary))
        }
        OnSecondary => meta(Pal::Secondary, false, 1.0, Some(Role::Secondary), None, c(4.5, 7.0, 11.0, 21.0), None),
        SecondaryContainer => {
            meta(Pal::Secondary, true, 1.0, hs, None, c(1.0, 1.0, 3.0, 4.5), nearer(SecondaryContainer, Role::Secondary))
        }
        OnSecondaryContainer => meta(Pal::Secondary, false, 1.0, Some(SecondaryContainer), None, c(3.0, 4.5, 7.0, 11.0), None),
        Role::Tertiary => meta(Pal::Tertiary, true, 1.0, hs, None, c(3.0, 4.5, 7.0, 7.0), nearer(TertiaryContainer, Role::Tertiary)),
        OnTertiary => meta(Pal::Tertiary, false, 1.0, Some(Role::Tertiary), None, c(4.5, 7.0, 11.0, 21.0), None),
        TertiaryContainer => {
            meta(Pal::Tertiary, true, 1.0, hs, None, c(1.0, 1.0, 3.0, 4.5), nearer(TertiaryContainer, Role::Tertiary))
        }
        OnTertiaryContainer => meta(Pal::Tertiary, false, 1.0, Some(TertiaryContainer), None, c(3.0, 4.5, 7.0, 11.0), None),
        Role::Error => meta(Pal::Error, true, 1.0, hs, None, c(3.0, 4.5, 7.0, 7.0), nearer(ErrorContainer, Role::Error)),
        OnError => meta(Pal::Error, false, 1.0, Some(Role::Error), None, c(4.5, 7.0, 11.0, 21.0), None),
        ErrorContainer => meta(Pal::Error, true, 1.0, hs, None, c(1.0, 1.0, 3.0, 4.5), nearer(ErrorContainer, Role::Error)),
        OnErrorContainer => meta(Pal::Error, false, 1.0, Some(ErrorContainer), None, c(3.0, 4.5, 7.0, 11.0), None),
        PrimaryFixed | PrimaryFixedDim => {
            meta(Pal::Primary, true, 1.0, hs, None, c(1.0, 1.0, 3.0, 4.5), lighter(PrimaryFixed, PrimaryFixedDim))
        }
        OnPrimaryFixed => meta(Pal::Primary, false, 1.0, Some(PrimaryFixedDim), Some(PrimaryFixed), c(4.5, 7.0, 11.0, 21.0), None),
        OnPrimaryFixedVariant => {
            meta(Pal::Primary, false, 1.0, Some(PrimaryFixedDim), Some(PrimaryFixed), c(3.0, 4.5, 7.0, 11.0), None)
        }
        SecondaryFixed | SecondaryFixedDim => {
            meta(Pal::Secondary, true, 1.0, hs, None, c(1.0, 1.0, 3.0, 4.5), lighter(SecondaryFixed, SecondaryFixedDim))
        }
        OnSecondaryFixed => {
            meta(Pal::Secondary, false, 1.0, Some(SecondaryFixedDim), Some(SecondaryFixed), c(4.5, 7.0, 11.0, 21.0), None)
        }
        OnSecondaryFixedVariant => {
            meta(Pal::Secondary, false, 1.0, Some(SecondaryFixedDim), Some(SecondaryFixed), c(3.0, 4.5, 7.0, 11.0), None)
        }
        TertiaryFixed | TertiaryFixedDim => {
            meta(Pal::Tertiary, true, 1.0, hs, None, c(1.0, 1.0, 3.0, 4.5), lighter(TertiaryFixed, TertiaryFixedDim))
        }
        OnTertiaryFixed => {
            meta(Pal::Tertiary, false, 1.0, Some(TertiaryFixedDim), Some(TertiaryFixed), c(4.5, 7.0, 11.0, 21.0), None)
        }
        OnTertiaryFixedVariant => {
            meta(Pal::Tertiary, false, 1.0, Some(TertiaryFixedDim), Some(TertiaryFixed), c(3.0, 4.5, 7.0, 11.0), None)
        }
        PrimaryDim | SecondaryDim | TertiaryDim | ErrorDim => unreachable!("the *Dim colours exist only in 2025"),
    }
}

fn meta_2025(r: Role, s: &Scheme) -> Meta {
    use Role::*;
    let hs = Some(highest_surface(s));
    let g = |x| Some(get_curve(x));
    let farther = |container, main| pair(container, main, 5.0, Polarity::RelativeLighter, true, Constraint::Farther);
    let dim = |d, main| pair(d, main, 5.0, Polarity::Darker, true, Constraint::Farther);
    let exact = |d, fixed| pair(d, fixed, 5.0, Polarity::Darker, true, Constraint::Exact);
    let n = Pal::Neutral;
    match r {
        Surface | Background | SurfaceContainerLowest | InverseSurface => meta(n, true, 1.0, None, None, None, None),
        SurfaceDim => {
            let cm = if !s.dark { ladder(s, [2.5, 1.7, 2.7, 1.75, 1.36]) } else { 1.0 };
            meta(n, true, cm, None, None, None, None)
        }
        SurfaceBright => {
            let cm = if s.dark { ladder(s, [2.5, 1.7, 2.7, 1.75, 1.36]) } else { 1.0 };
            meta(n, true, cm, None, None, None, None)
        }
        SurfaceContainerLow => meta(n, true, ladder(s, [1.3, 1.25, 1.3, 1.15, 1.08]), None, None, None, None),
        SurfaceContainer => meta(n, true, ladder(s, [1.6, 1.4, 1.6, 1.3, 1.15]), None, None, None, None),
        SurfaceContainerHigh => meta(n, true, ladder(s, [1.9, 1.5, 1.95, 1.45, 1.22]), None, None, None, None),
        SurfaceContainerHighest | SurfaceVariant => meta(n, true, ladder(s, [2.2, 1.7, 2.3, 1.6, 1.29]), None, None, None, None),
        OnSurface | OnBackground => {
            let curve = if s.dark { get_curve(11.0) } else { get_curve(9.0) };
            meta(n, false, on_surface_multiplier(s), hs, None, Some(curve), None)
        }
        OnSurfaceVariant => {
            let curve = if s.dark { get_curve(6.0) } else { get_curve(4.5) };
            meta(n, false, on_surface_multiplier(s), hs, None, Some(curve), None)
        }
        Outline => meta(n, false, on_surface_multiplier(s), hs, None, g(3.0), None),
        OutlineVariant => meta(n, false, on_surface_multiplier(s), hs, None, g(1.5), None),
        InverseOnSurface => meta(n, false, 1.0, Some(InverseSurface), None, g(7.0), None),
        Role::Primary | SurfaceTint => meta(Pal::Primary, true, 1.0, hs, None, g(4.5), farther(PrimaryContainer, Role::Primary)),
        PrimaryDim => meta(Pal::Primary, true, 1.0, Some(SurfaceContainerHigh), None, g(4.5), dim(PrimaryDim, Role::Primary)),
        OnPrimary => meta(Pal::Primary, false, 1.0, Some(Role::Primary), None, g(6.0), None),
        PrimaryContainer | PrimaryFixed => meta(Pal::Primary, true, 1.0, hs, None, container_curve(s), None),
        OnPrimaryContainer => meta(Pal::Primary, false, 1.0, Some(PrimaryContainer), None, g(6.0), None),
        PrimaryFixedDim => meta(Pal::Primary, true, 1.0, None, None, None, exact(PrimaryFixedDim, PrimaryFixed)),
        OnPrimaryFixed => meta(Pal::Primary, false, 1.0, Some(PrimaryFixedDim), None, g(7.0), None),
        OnPrimaryFixedVariant => meta(Pal::Primary, false, 1.0, Some(PrimaryFixedDim), None, g(4.5), None),
        InversePrimary => meta(Pal::Primary, false, 1.0, Some(InverseSurface), None, g(6.0), None),
        Role::Secondary => meta(Pal::Secondary, true, 1.0, hs, None, g(4.5), farther(SecondaryContainer, Role::Secondary)),
        SecondaryDim => meta(Pal::Secondary, true, 1.0, Some(SurfaceContainerHigh), None, g(4.5), dim(SecondaryDim, Role::Secondary)),
        OnSecondary => meta(Pal::Secondary, false, 1.0, Some(Role::Secondary), None, g(6.0), None),
        SecondaryContainer | SecondaryFixed => meta(Pal::Secondary, true, 1.0, hs, None, container_curve(s), None),
        OnSecondaryContainer => meta(Pal::Secondary, false, 1.0, Some(SecondaryContainer), None, g(6.0), None),
        SecondaryFixedDim => meta(Pal::Secondary, true, 1.0, None, None, None, exact(SecondaryFixedDim, SecondaryFixed)),
        OnSecondaryFixed => meta(Pal::Secondary, false, 1.0, Some(SecondaryFixedDim), None, g(7.0), None),
        OnSecondaryFixedVariant => meta(Pal::Secondary, false, 1.0, Some(SecondaryFixedDim), None, g(4.5), None),
        Role::Tertiary => meta(Pal::Tertiary, true, 1.0, hs, None, g(4.5), farther(TertiaryContainer, Role::Tertiary)),
        TertiaryDim => meta(Pal::Tertiary, true, 1.0, Some(SurfaceContainerHigh), None, g(4.5), dim(TertiaryDim, Role::Tertiary)),
        OnTertiary => meta(Pal::Tertiary, false, 1.0, Some(Role::Tertiary), None, g(6.0), None),
        TertiaryContainer | TertiaryFixed => meta(Pal::Tertiary, true, 1.0, hs, None, container_curve(s), None),
        OnTertiaryContainer => meta(Pal::Tertiary, false, 1.0, Some(TertiaryContainer), None, g(6.0), None),
        TertiaryFixedDim => meta(Pal::Tertiary, true, 1.0, None, None, None, exact(TertiaryFixedDim, TertiaryFixed)),
        OnTertiaryFixed => meta(Pal::Tertiary, false, 1.0, Some(TertiaryFixedDim), None, g(7.0), None),
        OnTertiaryFixedVariant => meta(Pal::Tertiary, false, 1.0, Some(TertiaryFixedDim), None, g(4.5), None),
        Role::Error => meta(Pal::Error, true, 1.0, hs, None, g(4.5), farther(ErrorContainer, Role::Error)),
        ErrorDim => meta(Pal::Error, true, 1.0, Some(SurfaceContainerHigh), None, g(4.5), dim(ErrorDim, Role::Error)),
        OnError => meta(Pal::Error, false, 1.0, Some(Role::Error), None, g(6.0), None),
        ErrorContainer => meta(Pal::Error, true, 1.0, hs, None, container_curve(s), None),
        OnErrorContainer => meta(Pal::Error, false, 1.0, Some(ErrorContainer), None, g(4.5), None),
        _ => unreachable!("{} has no 2025 definition", r.name()),
    }
}

/// The colour's own `tone` lambda, before any contrast or pairing.
fn raw_tone(r: Role, s: &Scheme) -> f64 {
    match which(r, s) {
        Which::V2021 => raw_tone_2021(r, s),
        Which::V2025 => raw_tone_2025(r, s),
    }
}

fn raw_tone_2021(r: Role, s: &Scheme) -> f64 {
    use Role::*;
    let d = s.dark;
    let pick = |a: f64, b: f64| if d { a } else { b };
    let mono = is_monochrome(s);
    let fid = is_fidelity(s);
    let lvl = s.contrast;
    match r {
        PrimaryPaletteKeyColor => s.primary.key.tone,
        SecondaryPaletteKeyColor => s.secondary.key.tone,
        TertiaryPaletteKeyColor => s.tertiary.key.tone,
        NeutralPaletteKeyColor => s.neutral.key.tone,
        NeutralVariantPaletteKeyColor => s.neutral_variant.key.tone,
        ErrorPaletteKeyColor => s.error.key.tone,
        Background | Surface => pick(6.0, 98.0),
        SurfaceDim => {
            if d {
                6.0
            } else {
                Curve(87.0, 87.0, 80.0, 75.0).get(lvl)
            }
        }
        SurfaceBright => {
            if d {
                Curve(24.0, 24.0, 29.0, 34.0).get(lvl)
            } else {
                98.0
            }
        }
        SurfaceContainerLowest => {
            if d {
                Curve(4.0, 4.0, 2.0, 0.0).get(lvl)
            } else {
                100.0
            }
        }
        SurfaceContainerLow => pick(Curve(10.0, 10.0, 11.0, 12.0).get(lvl), Curve(96.0, 96.0, 96.0, 95.0).get(lvl)),
        SurfaceContainer => pick(Curve(12.0, 12.0, 16.0, 20.0).get(lvl), Curve(94.0, 94.0, 92.0, 90.0).get(lvl)),
        SurfaceContainerHigh => pick(Curve(17.0, 17.0, 21.0, 25.0).get(lvl), Curve(92.0, 92.0, 88.0, 85.0).get(lvl)),
        SurfaceContainerHighest => pick(Curve(22.0, 22.0, 26.0, 30.0).get(lvl), Curve(90.0, 90.0, 84.0, 80.0).get(lvl)),
        OnBackground | OnSurface => pick(90.0, 10.0),
        SurfaceVariant => pick(30.0, 90.0),
        OnSurfaceVariant => pick(80.0, 30.0),
        InverseSurface => pick(90.0, 20.0),
        InverseOnSurface => pick(20.0, 95.0),
        Outline => pick(60.0, 50.0),
        OutlineVariant => pick(30.0, 80.0),
        Shadow | Scrim => 0.0,
        SurfaceTint => pick(80.0, 40.0),
        Primary => {
            if mono {
                pick(100.0, 0.0)
            } else {
                pick(80.0, 40.0)
            }
        }
        OnPrimary => {
            if mono {
                pick(10.0, 90.0)
            } else {
                pick(20.0, 100.0)
            }
        }
        PrimaryContainer => {
            if fid {
                s.source.tone
            } else if mono {
                pick(85.0, 25.0)
            } else {
                pick(30.0, 90.0)
            }
        }
        OnPrimaryContainer => {
            if fid {
                foreground_tone(raw_tone(PrimaryContainer, s), 4.5)
            } else if mono {
                pick(0.0, 100.0)
            } else {
                pick(90.0, 30.0)
            }
        }
        InversePrimary => pick(40.0, 80.0),
        Secondary => pick(80.0, 40.0),
        OnSecondary => {
            if mono {
                pick(10.0, 100.0)
            } else {
                pick(20.0, 100.0)
            }
        }
        SecondaryContainer => {
            if mono {
                pick(30.0, 85.0)
            } else if fid {
                find_desired_chroma_by_tone(s.secondary.hue, s.secondary.chroma, pick(30.0, 90.0), !d)
            } else {
                pick(30.0, 90.0)
            }
        }
        OnSecondaryContainer => {
            if mono {
                pick(90.0, 10.0)
            } else if fid {
                foreground_tone(raw_tone(SecondaryContainer, s), 4.5)
            } else {
                pick(90.0, 30.0)
            }
        }
        Tertiary => {
            if mono {
                pick(90.0, 25.0)
            } else {
                pick(80.0, 40.0)
            }
        }
        OnTertiary => {
            if mono {
                pick(10.0, 90.0)
            } else {
                pick(20.0, 100.0)
            }
        }
        TertiaryContainer => {
            if mono {
                pick(60.0, 49.0)
            } else if fid {
                fix_if_disliked(s.tertiary.get_hct(s.source.tone)).tone
            } else {
                pick(30.0, 90.0)
            }
        }
        OnTertiaryContainer => {
            if mono {
                pick(0.0, 100.0)
            } else if fid {
                foreground_tone(raw_tone(TertiaryContainer, s), 4.5)
            } else {
                pick(90.0, 30.0)
            }
        }
        Error => pick(80.0, 40.0),
        OnError => pick(20.0, 100.0),
        ErrorContainer => pick(30.0, 90.0),
        OnErrorContainer => {
            if mono {
                pick(90.0, 10.0)
            } else {
                pick(90.0, 30.0)
            }
        }
        PrimaryFixed | TertiaryFixed => {
            if mono {
                40.0
            } else {
                90.0
            }
        }
        PrimaryFixedDim | TertiaryFixedDim => {
            if mono {
                30.0
            } else {
                80.0
            }
        }
        OnPrimaryFixed | OnTertiaryFixed => {
            if mono {
                100.0
            } else {
                10.0
            }
        }
        OnPrimaryFixedVariant | OnTertiaryFixedVariant => {
            if mono {
                90.0
            } else {
                30.0
            }
        }
        SecondaryFixed => {
            if mono {
                80.0
            } else {
                90.0
            }
        }
        SecondaryFixedDim => {
            if mono {
                70.0
            } else {
                80.0
            }
        }
        OnSecondaryFixed => 10.0,
        OnSecondaryFixedVariant => {
            if mono {
                25.0
            } else {
                30.0
            }
        }
        PrimaryDim | SecondaryDim | TertiaryDim | ErrorDim => unreachable!("the *Dim colours exist only in 2025"),
    }
}

fn raw_tone_2025(r: Role, s: &Scheme) -> f64 {
    use Role::*;
    use Variant as V;
    let d = s.dark;
    let (p, sec, t, e) = (&s.primary, &s.secondary, &s.tertiary, &s.error);
    // A colour without its own tone takes its background's (`get_initial_tone_from_background`).
    let from_background = |r: Role| match meta_of(r, s).background {
        Some(bg) => get_tone(bg, s),
        None => 50.0,
    };
    match r {
        Surface | Background => surface_tone(s, [4.0, 99.0, 97.0, 98.0]),
        SurfaceDim => surface_tone(s, [4.0, 90.0, 85.0, 87.0]),
        SurfaceBright => surface_tone(s, [18.0, 99.0, 97.0, 98.0]),
        SurfaceContainerLowest => {
            if d {
                0.0
            } else {
                100.0
            }
        }
        SurfaceContainerLow => surface_tone(s, [6.0, 98.0, 95.0, 96.0]),
        SurfaceContainer => surface_tone(s, [9.0, 96.0, 92.0, 94.0]),
        SurfaceContainerHigh => surface_tone(s, [12.0, 94.0, 90.0, 92.0]),
        SurfaceContainerHighest | SurfaceVariant => surface_tone(s, [15.0, 92.0, 88.0, 90.0]),
        OnSurface => {
            if s.variant == V::Vibrant {
                t_max_c(&s.neutral, 0.0, 100.0, 1.1)
            } else {
                get_tone(highest_surface(s), s)
            }
        }
        OnBackground => get_tone(OnSurface, s),
        OnSurfaceVariant | Outline | OutlineVariant | InverseOnSurface | OnPrimary | OnPrimaryContainer
        | OnPrimaryFixed | OnPrimaryFixedVariant | OnSecondary | OnSecondaryContainer | OnSecondaryFixed
        | OnSecondaryFixedVariant | OnTertiary | OnTertiaryContainer | OnTertiaryFixed | OnTertiaryFixedVariant
        | OnError | OnErrorContainer => from_background(r),
        InverseSurface => {
            if d {
                98.0
            } else {
                4.0
            }
        }
        Primary | SurfaceTint => match s.variant {
            V::Neutral => {
                if d {
                    80.0
                } else {
                    40.0
                }
            }
            V::TonalSpot => {
                if d {
                    80.0
                } else {
                    tmax(p)
                }
            }
            V::Expressive => {
                if Hct::is_yellow(p.hue) {
                    tmax_in(p, 0.0, 25.0)
                } else if Hct::is_cyan(p.hue) {
                    tmax_in(p, 0.0, 88.0)
                } else {
                    tmax_in(p, 0.0, 98.0)
                }
            }
            V::Vibrant => {
                if Hct::is_cyan(p.hue) {
                    tmax_in(p, 0.0, 88.0)
                } else {
                    tmax_in(p, 0.0, 98.0)
                }
            }
            _ => 0.0,
        },
        PrimaryDim => match s.variant {
            V::Neutral => 85.0,
            V::TonalSpot => tmax_in(p, 0.0, 90.0),
            _ => tmax(p),
        },
        PrimaryContainer => match s.variant {
            V::Neutral => {
                if d {
                    30.0
                } else {
                    90.0
                }
            }
            V::TonalSpot => {
                if d {
                    t_min_c(p, 35.0, 93.0)
                } else {
                    tmax_in(p, 0.0, 90.0)
                }
            }
            V::Expressive => {
                if d {
                    tmax_in(p, 30.0, 93.0)
                } else if Hct::is_cyan(p.hue) {
                    tmax_in(p, 78.0, 88.0)
                } else {
                    tmax_in(p, 78.0, 90.0)
                }
            }
            V::Vibrant => {
                if d {
                    t_min_c(p, 66.0, 93.0)
                } else if Hct::is_cyan(p.hue) {
                    tmax_in(p, 66.0, 88.0)
                } else {
                    tmax_in(p, 66.0, 93.0)
                }
            }
            _ => 0.0,
        },
        PrimaryFixed => get_tone(PrimaryContainer, &s.light_twin()),
        PrimaryFixedDim => get_tone(PrimaryFixed, s),
        InversePrimary => tmax(p),
        Secondary => match s.variant {
            V::Neutral => {
                if d {
                    t_min_c(sec, 0.0, 98.0)
                } else {
                    tmax(sec)
                }
            }
            V::Vibrant => {
                if d {
                    tmax_in(sec, 0.0, 90.0)
                } else {
                    tmax_in(sec, 0.0, 98.0)
                }
            }
            _ => {
                if d {
                    80.0
                } else {
                    tmax(sec)
                }
            }
        },
        SecondaryDim => {
            if s.variant == V::Neutral {
                85.0
            } else {
                tmax_in(sec, 0.0, 90.0)
            }
        }
        SecondaryContainer => match s.variant {
            V::Vibrant => {
                if d {
                    t_min_c(sec, 30.0, 40.0)
                } else {
                    tmax_in(sec, 84.0, 90.0)
                }
            }
            V::Expressive => {
                if d {
                    15.0
                } else {
                    tmax_in(sec, 90.0, 95.0)
                }
            }
            _ => {
                if d {
                    25.0
                } else {
                    90.0
                }
            }
        },
        SecondaryFixed => get_tone(SecondaryContainer, &s.light_twin()),
        SecondaryFixedDim => get_tone(SecondaryFixed, s),
        Tertiary => {
            if matches!(s.variant, V::Expressive | V::Vibrant) {
                if Hct::is_cyan(t.hue) {
                    tmax_in(t, 0.0, 88.0)
                } else if d {
                    tmax_in(t, 0.0, 98.0)
                } else {
                    tmax_in(t, 0.0, 100.0)
                }
            } else if d {
                tmax_in(t, 0.0, 98.0)
            } else {
                tmax(t)
            }
        }
        TertiaryDim => {
            if s.variant == V::TonalSpot {
                tmax_in(t, 0.0, 90.0)
            } else {
                tmax(t)
            }
        }
        TertiaryContainer => match s.variant {
            V::Neutral => {
                if d {
                    tmax_in(t, 0.0, 93.0)
                } else {
                    tmax_in(t, 0.0, 96.0)
                }
            }
            V::TonalSpot => {
                if d {
                    tmax_in(t, 0.0, 93.0)
                } else {
                    tmax_in(t, 0.0, 100.0)
                }
            }
            V::Expressive => {
                if Hct::is_cyan(t.hue) {
                    tmax_in(t, 75.0, 88.0)
                } else if d {
                    tmax_in(t, 75.0, 93.0)
                } else {
                    tmax_in(t, 75.0, 100.0)
                }
            }
            V::Vibrant => {
                if d {
                    tmax_in(t, 0.0, 93.0)
                } else {
                    tmax_in(t, 72.0, 100.0)
                }
            }
            _ => 0.0,
        },
        TertiaryFixed => get_tone(TertiaryContainer, &s.light_twin()),
        TertiaryFixedDim => get_tone(TertiaryFixed, s),
        Error => {
            if d {
                t_min_c(e, 0.0, 98.0)
            } else {
                tmax(e)
            }
        }
        ErrorDim => t_min_c(e, 0.0, 100.0),
        ErrorContainer => {
            if d {
                t_min_c(e, 30.0, 93.0)
            } else {
                tmax_in(e, 0.0, 90.0)
            }
        }
        Shadow | Scrim | PrimaryPaletteKeyColor | SecondaryPaletteKeyColor | TertiaryPaletteKeyColor
        | NeutralPaletteKeyColor | NeutralVariantPaletteKeyColor | ErrorPaletteKeyColor => {
            unreachable!("{} has no 2025 definition", r.name())
        }
    }
}

fn is_fixed_dim(r: Role) -> bool {
    matches!(r, Role::PrimaryFixedDim | Role::SecondaryFixedDim | Role::TertiaryFixedDim)
}

/// `DynamicColor.get_tone`: the calculation delegate follows the scheme's spec version, whatever spec the colour's
/// own definition came from.
pub fn get_tone(r: Role, s: &Scheme) -> f64 {
    if s.spec2025 { tone_2025(r, s) } else { tone_2021(r, s) }
}

fn second_background_tone(m: &Meta, s: &Scheme, answer: f64, desired_ratio: f64) -> f64 {
    let (Some(bg1), Some(bg2)) = (m.background, m.second_background) else { return answer };
    let bg_tone1 = get_tone(bg1, s);
    let bg_tone2 = get_tone(bg2, s);
    let upper = bg_tone1.max(bg_tone2);
    let lower = bg_tone1.min(bg_tone2);
    if ratio_of_tones(upper, answer) >= desired_ratio && ratio_of_tones(lower, answer) >= desired_ratio {
        return answer;
    }
    let light_option = lighter(upper, desired_ratio);
    let dark_option = darker(lower, desired_ratio);
    let mut availables = Vec::new();
    if light_option != -1.0 {
        availables.push(light_option);
    }
    if dark_option != -1.0 {
        availables.push(dark_option);
    }
    let prefers_light = tone_prefers_light_foreground(bg_tone1) || tone_prefers_light_foreground(bg_tone2);
    if prefers_light {
        return if light_option < 0.0 { 100.0 } else { light_option };
    }
    if availables.len() == 1 {
        return availables[0];
    }
    if dark_option < 0.0 { 0.0 } else { dark_option }
}

fn tone_2021(r: Role, s: &Scheme) -> f64 {
    let m = meta_of(r, s);
    let decreasing_contrast = s.contrast < 0.0;
    if let Some(p) = m.pair {
        let a_is_nearer = p.polarity == Polarity::Nearer
            || (p.polarity == Polarity::Lighter && !s.dark)
            || (p.polarity == Polarity::Darker && s.dark);
        let nearer = if a_is_nearer { p.a } else { p.b };
        let farther = if a_is_nearer { p.b } else { p.a };
        let am_nearer = r == nearer;
        let expansion_dir = if s.dark { 1.0 } else { -1.0 };
        let delta = p.delta;
        let mut n_tone = raw_tone(nearer, s);
        let mut f_tone = raw_tone(farther, s);
        if let (Some(bg), Some(n_curve), Some(f_curve)) = (m.background, meta_of(nearer, s).curve, meta_of(farther, s).curve) {
            let bg_tone = get_tone(bg, s);
            let n_contrast = n_curve.get(s.contrast);
            let f_contrast = f_curve.get(s.contrast);
            if ratio_of_tones(bg_tone, n_tone) < n_contrast {
                n_tone = foreground_tone(bg_tone, n_contrast);
            }
            if ratio_of_tones(bg_tone, f_tone) < f_contrast {
                f_tone = foreground_tone(bg_tone, f_contrast);
            }
            if decreasing_contrast {
                n_tone = foreground_tone(bg_tone, n_contrast);
                f_tone = foreground_tone(bg_tone, f_contrast);
            }
        }
        if (f_tone - n_tone) * expansion_dir < delta {
            f_tone = clamp_double(0.0, 100.0, n_tone + delta * expansion_dir);
            if (f_tone - n_tone) * expansion_dir < delta {
                n_tone = clamp_double(0.0, 100.0, f_tone - delta * expansion_dir);
            }
        }
        if (50.0..60.0).contains(&n_tone) {
            if expansion_dir > 0.0 {
                n_tone = 60.0;
                f_tone = f_tone.max(n_tone + delta * expansion_dir);
            } else {
                n_tone = 49.0;
                f_tone = f_tone.min(n_tone + delta * expansion_dir);
            }
        } else if (50.0..60.0).contains(&f_tone) {
            if p.stay_together {
                if expansion_dir > 0.0 {
                    n_tone = 60.0;
                    f_tone = f_tone.max(n_tone + delta * expansion_dir);
                } else {
                    n_tone = 49.0;
                    f_tone = f_tone.min(n_tone + delta * expansion_dir);
                }
            } else if expansion_dir > 0.0 {
                f_tone = 60.0;
            } else {
                f_tone = 49.0;
            }
        }
        return if am_nearer { n_tone } else { f_tone };
    }
    let mut answer = raw_tone(r, s);
    let (Some(bg), Some(curve)) = (m.background, m.curve) else { return answer };
    let bg_tone = get_tone(bg, s);
    let desired_ratio = curve.get(s.contrast);
    if ratio_of_tones(bg_tone, answer) < desired_ratio {
        answer = foreground_tone(bg_tone, desired_ratio);
    }
    if decreasing_contrast {
        answer = foreground_tone(bg_tone, desired_ratio);
    }
    if m.is_background && (50.0..60.0).contains(&answer) {
        answer = if ratio_of_tones(49.0, bg_tone) >= desired_ratio { 49.0 } else { 60.0 };
    }
    second_background_tone(&m, s, answer, desired_ratio)
}

fn tone_2025(r: Role, s: &Scheme) -> f64 {
    let m = meta_of(r, s);
    if let Some(p) = m.pair {
        let absolute_delta = if p.polarity == Polarity::Darker || (p.polarity == Polarity::RelativeLighter && s.dark) {
            -p.delta
        } else {
            p.delta
        };
        let am_role_a = r == p.a;
        let self_role = if am_role_a { p.a } else { p.b };
        let ref_role = if am_role_a { p.b } else { p.a };
        let mut self_tone = raw_tone(self_role, s);
        let ref_tone = get_tone(ref_role, s);
        let relative_delta = if am_role_a { absolute_delta } else { -absolute_delta };
        match p.constraint {
            Constraint::Exact => self_tone = clamp_double(0.0, 100.0, ref_tone + relative_delta),
            Constraint::Farther => {
                self_tone = if relative_delta > 0.0 {
                    clamp_double(ref_tone + relative_delta, 100.0, self_tone)
                } else {
                    clamp_double(0.0, ref_tone + relative_delta, self_tone)
                };
            }
        }
        if let (Some(bg), Some(curve)) = (m.background, m.curve) {
            let bg_tone = get_tone(bg, s);
            let self_contrast = curve.get(s.contrast);
            if !(ratio_of_tones(bg_tone, self_tone) >= self_contrast && s.contrast >= 0.0) {
                self_tone = foreground_tone(bg_tone, self_contrast);
            }
        }
        if m.is_background && !is_fixed_dim(r) {
            self_tone = if self_tone >= 57.0 { clamp_double(65.0, 100.0, self_tone) } else { clamp_double(0.0, 49.0, self_tone) };
        }
        return self_tone;
    }
    let mut answer = raw_tone(r, s);
    let (Some(bg), Some(curve)) = (m.background, m.curve) else { return answer };
    let bg_tone = get_tone(bg, s);
    let desired_ratio = curve.get(s.contrast);
    if !(ratio_of_tones(bg_tone, answer) >= desired_ratio && s.contrast >= 0.0) {
        answer = foreground_tone(bg_tone, desired_ratio);
    }
    if m.is_background && !is_fixed_dim(r) {
        answer = if answer >= 57.0 { clamp_double(65.0, 100.0, answer) } else { clamp_double(0.0, 49.0, answer) };
    }
    second_background_tone(&m, s, answer, desired_ratio)
}

pub fn get_hct(r: Role, s: &Scheme) -> Hct {
    let m = meta_of(r, s);
    let palette = s.palette(m.palette);
    let tone = get_tone(r, s);
    if s.spec2025 {
        Hct::from_hct(palette.hue, palette.chroma * m.chroma_multiplier, tone)
    } else {
        palette.get_hct(tone)
    }
}
