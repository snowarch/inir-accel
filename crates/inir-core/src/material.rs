//! Ultra-fast Material You + HCT + APCA contrast + Wallpaper color extractor & template renderer.
//! Replaces `generate_colors_material.py` (which takes ~400-800ms via Python/NumPy/PIL) in <8ms.

use crate::washi::{self, LCH, RGB, contrast, parse_hex, to_linear};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::PI;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialGenConfig {
    pub path: Option<String>,
    pub size: u32,
    pub color: Option<String>,
    pub mode: String,
    pub scheme: String,
    pub smart: bool,
    pub transparency: String,
    pub termscheme: Option<String>,
    pub harmony: f64,
    pub harmonize_threshold: f64,
    pub term_fg_boost: f64,
    pub term_saturation: f64,
    pub term_brightness: f64,
    pub term_bg_brightness: f64,
    pub blend_bg_fg: bool,
    pub cache: Option<String>,
    pub soften: bool,
    pub debug: bool,
    pub json_output: Option<String>,
    pub palette_output: Option<String>,
    pub app_palette_output: Option<String>,
    pub terminal_output: Option<String>,
    pub meta_output: Option<String>,
    pub scss_output: Option<String>,
    pub surface_seed: Option<String>,
    pub accent_seed: Option<String>,
    pub washi_roles: Option<String>,
    pub render_templates: Option<String>,
    pub color_strength: f64,
    pub invert_hue: bool,
}

impl Default for MaterialGenConfig {
    fn default() -> Self {
        Self {
            path: None,
            size: 128,
            color: None,
            mode: "dark".to_string(),
            scheme: "vibrant".to_string(),
            smart: false,
            transparency: "opaque".to_string(),
            termscheme: None,
            harmony: 0.4,
            harmonize_threshold: 100.0,
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

/// Extract dominant vibrant seed color and optimal scheme from an image file in pure Rust.
pub fn extract_image_seed_and_scheme(path: &str, max_size: u32, auto_scheme: bool) -> Result<(RGB, String), String> {
    let img = image::open(path).map_err(|e| format!("failed to open image {path}: {e}"))?;
    let (w, h) = (img.width(), img.height());
    let area = (w as f64) * (h as f64);
    let target_area = (max_size as f64) * (max_size as f64);
    let resized = if area > target_area {
        let scale = (target_area / area).sqrt();
        let nw = ((w as f64) * scale).round().max(1.0) as u32;
        let nh = ((h as f64) * scale).round().max(1.0) as u32;
        img.resize_exact(nw, nh, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgb_img = resized.to_rgb8();
    let pixels = rgb_img.as_raw();
    let n = (pixels.len() / 3).max(1) as f64;

    // Compute Hasler-Süsstrunk colorfulness & HSV stats for auto scheme detection
    let mut sum_rg = 0.0;
    let mut sum_rg2 = 0.0;
    let mut sum_yb = 0.0;
    let mut sum_yb2 = 0.0;
    let mut sum_s = 0.0;
    let mut sum_h = 0.0;
    let mut sum_h2 = 0.0;

    // Quantize into 36 hue buckets + chroma/tone weighting (Celebi/Score equivalent)
    let mut hue_pop = [0.0f64; 360];
    let mut hue_rgb_sum = [(0.0f64, 0.0f64, 0.0f64, 0.0f64); 360];
    let mut fallback_sum = (0.0f64, 0.0f64, 0.0f64, 0.0f64);

    for chunk in pixels.chunks_exact(3) {
        let (r, g, b) = (chunk[0] as f64, chunk[1] as f64, chunk[2] as f64);
        let rg = (r - g).abs();
        let yb = (0.5 * (r + g) - b).abs();
        sum_rg += rg;
        sum_rg2 += rg * rg;
        sum_yb += yb;
        sum_yb2 += yb * yb;

        let (rn, gn, bn) = (r / 255.0, g / 255.0, b / 255.0);
        let maxc = rn.max(gn).max(bn);
        let minc = rn.min(gn).min(bn);
        let delta = maxc - minc;
        let s = if maxc > 0.0 { (delta / maxc) * 255.0 } else { 0.0 };
        let mut h_cv = 0.0;
        if delta > 0.0 {
            if (maxc - rn).abs() < 1e-6 {
                h_cv = 30.0 * (((gn - bn) / delta).rem_euclid(6.0));
            } else if (maxc - gn).abs() < 1e-6 {
                h_cv = 30.0 * ((bn - rn) / delta + 2.0);
            } else {
                h_cv = 30.0 * ((rn - gn) / delta + 4.0);
            }
        }
        sum_s += s;
        sum_h += h_cv;
        sum_h2 += h_cv * h_cv;

        let rgb = RGB { r: rn, g: gn, b: bn };
        let lch = rgb.lch();
        fallback_sum.0 += rn;
        fallback_sum.1 += gn;
        fallback_sum.2 += bn;
        fallback_sum.3 += 1.0;

        if lch.c >= 0.025 && lch.l > 0.05 && lch.l < 0.96 {
            let bucket = (lch.h.round() as usize) % 360;
            let weight = (lch.c * 4.0).clamp(0.1, 2.5);
            hue_pop[bucket] += weight;
            hue_rgb_sum[bucket].0 += rn * weight;
            hue_rgb_sum[bucket].1 += gn * weight;
            hue_rgb_sum[bucket].2 += bn * weight;
            hue_rgb_sum[bucket].3 += weight;
        }
    }

    let detected_scheme = if auto_scheme {
        let mean_rg = sum_rg / n;
        let std_rg = (sum_rg2 / n - mean_rg * mean_rg).max(0.0).sqrt();
        let mean_yb = sum_yb / n;
        let std_yb = (sum_yb2 / n - mean_yb * mean_yb).max(0.0).sqrt();
        let colorfulness = (std_rg * std_rg + std_yb * std_yb).sqrt()
            + 0.3 * (mean_rg * mean_rg + mean_yb * mean_yb).sqrt();
        let saturation = sum_s / n;
        let mean_h = sum_h / n;
        let hue_spread = (sum_h2 / n - mean_h * mean_h).max(0.0).sqrt();

        if saturation < 20.0 {
            "scheme-monochrome"
        } else if colorfulness < 30.0 {
            if saturation < 55.0 {
                "scheme-neutral"
            } else if hue_spread < 22.0 {
                "scheme-content"
            } else {
                "scheme-tonal-spot"
            }
        } else if colorfulness < 55.0 {
            if hue_spread < 22.0 && saturation < 100.0 {
                "scheme-content"
            } else {
                "scheme-tonal-spot"
            }
        } else if colorfulness < 90.0 {
            if saturation > 140.0 && hue_spread < 35.0 {
                "scheme-fidelity"
            } else if hue_spread < 30.0 {
                "scheme-content"
            } else {
                "scheme-tonal-spot"
            }
        } else if hue_spread > 55.0 && saturation > 150.0 {
            "scheme-rainbow"
        } else if saturation > 160.0 {
            "scheme-fidelity"
        } else if hue_spread > 45.0 {
            "scheme-expressive"
        } else {
            "scheme-tonal-spot"
        }
        .to_string()
    } else {
        String::new()
    };

    // Smooth hue histogram with a 15-degree window (like Material Score)
    let mut best_bucket = None;
    let mut best_score = 0.0;
    for h in 0..360 {
        let mut window_pop = 0.0;
        for dh in -7i32..=7i32 {
            let idx = (h as i32 + dh).rem_euclid(360) as usize;
            window_pop += hue_pop[idx];
        }
        if window_pop > best_score && hue_rgb_sum[h].3 > 0.0 {
            best_score = window_pop;
            best_bucket = Some(h);
        }
    }

    let seed = if let Some(b) = best_bucket {
        let mut r = 0.0;
        let mut g = 0.0;
        let mut bl = 0.0;
        let mut w = 0.0;
        for dh in -7i32..=7i32 {
            let idx = (b as i32 + dh).rem_euclid(360) as usize;
            r += hue_rgb_sum[idx].0;
            g += hue_rgb_sum[idx].1;
            bl += hue_rgb_sum[idx].2;
            w += hue_rgb_sum[idx].3;
        }
        RGB {
            r: r / w,
            g: g / w,
            b: bl / w,
        }
    } else if fallback_sum.3 > 0.0 {
        RGB {
            r: fallback_sum.0 / fallback_sum.3,
            g: fallback_sum.1 / fallback_sum.3,
            b: fallback_sum.2 / fallback_sum.3,
        }
    } else {
        parse_hex("#6750A4").unwrap()
    };

    Ok((seed, detected_scheme))
}

pub fn apca_lc(text: RGB, bg: RGB) -> f64 {
    let ys = |c: RGB| -> f64 {
        let y = 0.2126729 * c.r.powf(2.4) + 0.7151522 * c.g.powf(2.4) + 0.0721750 * c.b.powf(2.4);
        if y < 0.022 {
            y + (0.022 - y).powf(1.414)
        } else {
            y
        }
    };
    let (yt, yb) = (ys(text), ys(bg));
    if yb > yt {
        let s = (yb.powf(0.56) - yt.powf(0.57)) * 1.14;
        if s < 0.1 { 0.0 } else { (s - 0.027) * 100.0 }
    } else {
        let s = (yb.powf(0.65) - yt.powf(0.62)) * 1.14;
        if s > -0.1 { 0.0 } else { -(s + 0.027) * 100.0 }
    }
}

pub fn ensure_contrast_lch(fg: RGB, bg: RGB, min_ratio: f64, is_dark: bool, tone_limit: Option<f64>) -> RGB {
    if contrast(fg, bg) >= min_ratio {
        return fg;
    }
    let lch = fg.lch();
    let limit = tone_limit.unwrap_or(if is_dark { 0.91 } else { 0.24 });
    let dir = if is_dark { 1.0 } else { -1.0 };
    let mut best = fg;
    let mut best_ratio = contrast(fg, bg);
    let mut l = lch.l;
    for _ in 0..200 {
        let clamped_l = l.clamp(0.0, 1.0);
        let cand = (LCH {
            l: clamped_l,
            c: lch.c,
            h: lch.h,
        })
        .rgb();
        let r = contrast(cand, bg);
        if r > best_ratio {
            best_ratio = r;
            best = cand;
        }
        if r >= min_ratio {
            return cand;
        }
        if (is_dark && clamped_l >= limit) || (!is_dark && clamped_l <= limit) {
            break;
        }
        l += dir * 0.005;
    }
    best
}

pub fn mix_hex(color_a: &str, color_b: &str, keep_a: f64) -> String {
    let a = parse_hex(color_a).unwrap_or_default();
    let b = parse_hex(color_b).unwrap_or_default();
    (RGB {
        r: a.r * keep_a + b.r * (1.0 - keep_a),
        g: a.g * keep_a + b.g * (1.0 - keep_a),
        b: a.b * keep_a + b.b * (1.0 - keep_a),
    })
    .hex()
    .to_uppercase()
}

pub fn readable_hex(fg_hex: &str, bg_hex: &str, min_ratio: f64) -> String {
    let bg = parse_hex(bg_hex).unwrap_or_default();
    let fg = parse_hex(fg_hex).unwrap_or_default();
    let is_bg_dark = bg.lch().l < 0.55;
    ensure_contrast_lch(fg, bg, min_ratio, is_bg_dark, None)
        .hex()
        .to_uppercase()
}

pub fn build_app_palette(base_palette: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let get = |k: &str, def: &str| {
        base_palette
            .get(k)
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| def.to_string())
    };
    let layer0 = get("background", &get("surface", "#000000"));
    let layer1 = get("surface_container_low", &get("surface", &layer0));
    let layer2 = get("surface_container", &layer1);
    let layer3 = get("surface_container_high", &layer2);
    let layer4 = get("surface_container_highest", &layer3);
    let on_surface = get("on_surface", &get("on_background", "#FFFFFF"));
    let on_surface_variant = get("on_surface_variant", &on_surface);
    let primary = get("primary", "#6750A4");
    let primary_container = get("primary_container", &primary);
    let on_primary = get("on_primary", &readable_hex(&on_surface, &primary, 4.5));
    let outline = get("outline", &on_surface_variant);
    let outline_variant = get("outline_variant", &mix_hex(&layer1, &outline, 0.72));

    let on_layer0 = readable_hex(&on_surface, &layer0, 4.5);
    let on_layer1 = readable_hex(&on_surface, &layer1, 4.5);
    let on_layer2 = readable_hex(&on_surface, &layer2, 4.5);
    let on_layer3 = readable_hex(&on_surface, &layer3, 4.5);
    let on_layer4 = readable_hex(&on_surface, &layer4, 4.5);

    let layer1_hover = mix_hex(&layer1, &on_layer1, 0.92);
    let layer1_active = mix_hex(&layer1, &on_layer1, 0.85);

    let l0_lch = parse_hex(&layer0).unwrap_or_default().lch();
    let is_dark = l0_lch.l < 0.55;
    let secondary = get("secondary", &on_surface_variant);
    let warm_sub_seed = mix_hex(&on_surface_variant, &secondary, 0.62);
    let subtext = if !is_dark {
        readable_hex(&warm_sub_seed, &layer1_active, 4.5)
    } else {
        readable_hex(&warm_sub_seed, &layer1, 4.2)
    };

    let layer2_hover = mix_hex(&layer2, &on_layer2, 0.90);
    let layer2_active = mix_hex(&layer2, &on_layer2, 0.80);
    let layer3_hover = mix_hex(&layer3, &on_layer3, 0.90);
    let layer3_active = mix_hex(&layer3, &on_layer3, 0.80);

    let cont_lch = parse_hex(&primary_container).unwrap_or_default().lch();
    let sel_l = if is_dark {
        cont_lch.l.max(l0_lch.l + 0.10)
    } else {
        cont_lch.l.min(l0_lch.l - 0.08)
    };
    let selection = (LCH {
        l: sel_l,
        c: cont_lch.c.min(0.06),
        h: cont_lch.h,
    })
    .rgb()
    .hex()
    .to_uppercase();
    let selection_hover = (LCH {
        l: sel_l,
        c: cont_lch.c.min(0.08),
        h: cont_lch.h,
    })
    .rgb()
    .hex()
    .to_uppercase();
    let on_selection = readable_hex(&on_layer3, &selection, 4.5);

    let accent_hue = parse_hex(&primary).unwrap_or_default().lch().h;
    let status = |hue: f64, chroma: f64| -> String {
        let delta = ((accent_hue - hue + 180.0).rem_euclid(360.0)) - 180.0;
        let h = (hue + (delta * 0.25).clamp(-15.0, 15.0)).rem_euclid(360.0);
        let seed = (LCH {
            l: if is_dark { 0.80 } else { 0.45 },
            c: chroma,
            h,
        })
        .rgb();
        ensure_contrast_lch(seed, parse_hex(&layer0).unwrap_or_default(), 4.5, is_dark, None)
            .hex()
            .to_uppercase()
    };

    let success = status(145.0, 0.13);
    let warning = status(75.0, 0.15);
    let error = readable_hex(&get("error", &status(25.0, 0.16)), &layer0, 4.5);

    let mut app = base_palette.clone();
    let updates = [
        ("background", layer0.clone()),
        ("on_background", on_layer0.clone()),
        ("surface", layer0.clone()),
        ("on_surface", on_layer1.clone()),
        ("surface_dim", layer0.clone()),
        ("surface_bright", layer3.clone()),
        ("surface_container_lowest", layer0.clone()),
        ("surface_container_low", layer1.clone()),
        ("surface_container", layer2.clone()),
        ("surface_container_high", layer3.clone()),
        ("surface_container_highest", layer4.clone()),
        ("outline", outline.clone()),
        ("outline_variant", outline_variant.clone()),
        ("app_background", layer0.clone()),
        ("app_foreground", on_layer0),
        ("app_subtext", subtext),
        ("app_surface", layer1.clone()),
        ("app_surface_hover", layer1_hover),
        ("app_surface_active", layer1_active),
        ("app_surface_elevated", layer2.clone()),
        ("app_surface_elevated_hover", layer2_hover),
        ("app_surface_elevated_active", layer2_active),
        ("app_surface_popup", layer3.clone()),
        ("app_surface_popup_hover", layer3_hover),
        ("app_surface_popup_active", layer3_active),
        ("app_on_surface", on_layer1),
        ("app_on_surface_elevated", on_layer2),
        ("app_on_surface_popup", on_layer3),
        ("app_on_surface_highest", on_layer4),
        ("app_border", outline),
        ("app_border_subtle", outline_variant),
        ("app_accent", primary),
        ("app_on_accent", on_primary),
        ("app_accent_container", primary_container),
        ("app_selection", selection),
        ("app_selection_hover", selection_hover),
        ("app_on_selection", on_selection),
        ("app_success", success),
        ("app_warning", warning),
        ("app_error", error),
        ("app_on_status", layer0.clone()),
        ("app_window_bg", layer0.clone()),
        ("app_view_bg", layer0.clone()),
        ("app_headerbar_bg", layer0.clone()),
        ("app_sidebar_bg", layer0.clone()),
        ("app_card_bg", layer1),
        ("app_popover_bg", layer2),
        ("app_dialog_bg", layer3),
        ("app_thumbnail_bg", layer4),
    ];
    for (k, v) in updates {
        app.insert(k.to_string(), v);
    }
    app
}

fn camel_to_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() && i > 0 {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

/// Generate the full Material Dynamic Colors dictionary for a given seed LCH, mode, and scheme.
pub fn generate_material_colors(seed: LCH, darkmode: bool, scheme_name: &str, soften: bool, strength: f64) -> BTreeMap<String, String> {
    let variant = match scheme_name {
        "scheme-vibrant" | "vibrant" => "vibrant",
        "scheme-expressive" | "expressive" => "expressive",
        "scheme-monochrome" | "monochrome" => "monochrome",
        "scheme-fidelity" | "fidelity" | "scheme-content" | "content" => "fidelity",
        "scheme-neutral" | "neutral" => "tonalSpot",
        _ => "tonalSpot",
    };

    let mut req = washi::default_request();
    req.language = "material".to_string();
    req.variant = variant.to_string();
    req.material = "wallpaper".to_string();
    let seed_hex = seed.rgb().hex();
    let (sec_rot, ter_rot) = match variant {
        "expressive" => (45.0, -60.0),
        "vibrant" => (24.0, 48.0),
        _ => (16.0, 48.0),
    };
    req.seeds = washi::Seeds {
        wallpaper: seed_hex.clone(),
        primary: seed_hex.clone(),
        secondary: (LCH {
            l: seed.l.clamp(0.55, 0.78),
            c: (seed.c * 0.65).clamp(0.045, 0.085),
            h: (seed.h + sec_rot).rem_euclid(360.0),
        })
        .rgb()
        .hex(),
        tertiary: (LCH {
            l: seed.l.clamp(0.55, 0.80),
            c: (seed.c * 0.85).clamp(0.065, 0.12),
            h: (seed.h + ter_rot).rem_euclid(360.0),
        })
        .rgb()
        .hex(),
        background: String::new(),
    };
    req.accent = "wallpaper".to_string();
    req.highlight = "theme".to_string();
    let mut col_mult = strength * 100.0;
    if soften && !matches!(scheme_name, "scheme-tonal-spot" | "scheme-neutral" | "scheme-monochrome") {
        col_mult *= 0.6;
    }
    if scheme_name == "scheme-neutral" {
        col_mult *= 0.4;
    } else if scheme_name == "scheme-monochrome" {
        col_mult = 0.0;
    }
    req.tune.insert(
        if darkmode { "dark" } else { "light" }.to_string(),
        washi::Tune {
            tone: 0.0,
            colour: Some(col_mult),
            widgets: None,
            warmth: Some(32.0),
        },
    );

    let pal = washi::build_palette(&req, if darkmode { "dark" } else { "light" });
    let mut m = BTreeMap::new();
    for (k, v) in &pal.apps {
        m.insert(k.clone(), v.to_uppercase());
    }
    // Add fixed variants expected by MaterialDynamicColors consumers
    let p_lch = parse_hex(&pal.apps["primary"]).unwrap_or_default().lch();
    let s_lch = parse_hex(&pal.apps["secondary"]).unwrap_or_default().lch();
    let t_lch = parse_hex(&pal.apps["tertiary"]).unwrap_or_default().lch();

    let fixed_set = |prefix: &str, lch: LCH, map: &mut BTreeMap<String, String>| {
        map.insert(format!("{prefix}Fixed"), (LCH { l: 0.90, c: lch.c, h: lch.h }).rgb().hex().to_uppercase());
        map.insert(format!("{prefix}FixedDim"), (LCH { l: 0.80, c: lch.c, h: lch.h }).rgb().hex().to_uppercase());
        map.insert(format!("on{}Fixed", capitalize(prefix)), (LCH { l: 0.15, c: lch.c * 0.5, h: lch.h }).rgb().hex().to_uppercase());
        map.insert(format!("on{}FixedVariant", capitalize(prefix)), (LCH { l: 0.30, c: lch.c * 0.7, h: lch.h }).rgb().hex().to_uppercase());
        map.insert(format!("{prefix}_paletteKeyColor"), (LCH { l: 0.55, c: lch.c, h: lch.h }).rgb().hex().to_uppercase());
    };
    fixed_set("primary", p_lch, &mut m);
    fixed_set("secondary", s_lch, &mut m);
    fixed_set("tertiary", t_lch, &mut m);

    if darkmode {
        m.insert("success".to_string(), "#B5CCBA".to_string());
        m.insert("onSuccess".to_string(), "#213528".to_string());
        m.insert("successContainer".to_string(), "#374B3E".to_string());
        m.insert("onSuccessContainer".to_string(), "#D1E9D6".to_string());
    } else {
        m.insert("success".to_string(), "#4F6354".to_string());
        m.insert("onSuccess".to_string(), "#FFFFFF".to_string());
        m.insert("successContainer".to_string(), "#D1E8D5".to_string());
        m.insert("onSuccessContainer".to_string(), "#0C1F13".to_string());
    }
    m
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

pub fn run_material_pipeline(mut cfg: MaterialGenConfig) -> Result<String, String> {
    let darkmode = cfg.mode == "dark";
    let transparent = cfg.transparency == "transparent";

    let mut seed_rgb = parse_hex("#6750A4").unwrap();
    if let Some(ref path) = cfg.path {
        let (extracted, auto_sch) = extract_image_seed_and_scheme(path, cfg.size, cfg.scheme == "auto")?;
        seed_rgb = extracted;
        if cfg.scheme == "auto" && !auto_sch.is_empty() {
            cfg.scheme = auto_sch;
        }
        if let Some(ref cache_path) = cfg.cache {
            let _ = fs::write(cache_path, seed_rgb.hex().to_uppercase());
        }
        if cfg.smart && seed_rgb.lch().c < 0.04 {
            cfg.scheme = "scheme-neutral".to_string();
        }
    } else if let Some(ref col) = cfg.color {
        if let Some(c) = parse_hex(col) {
            seed_rgb = c;
        }
    }

    if let Some(ref acc) = cfg.accent_seed {
        if let Some(c) = parse_hex(acc) {
            seed_rgb = c;
        }
    }

    let mut seed_lch = seed_rgb.lch();
    if cfg.invert_hue {
        seed_lch.h = (seed_lch.h + 180.0).rem_euclid(360.0);
        seed_rgb = seed_lch.rgb();
    }

    let mut material_colors = generate_material_colors(
        seed_lch,
        darkmode,
        &cfg.scheme,
        cfg.soften,
        cfg.color_strength,
    );

    let seed_background = material_colors.get("background").cloned().unwrap_or_default();

    // Apply surface_seed if provided
    if let Some(ref s_seed) = cfg.surface_seed {
        if let Some(s_rgb) = parse_hex(s_seed) {
            let s_lch = s_rgb.lch();
            if (s_lch.l < 0.55) == darkmode {
                if let Some(bg_hex) = material_colors.get("background") {
                    let base_l = parse_hex(bg_hex).unwrap_or_default().lch().l;
                    let seed_l = if !darkmode { s_lch.l.max(0.64) } else { s_lch.l.min(0.32) };
                    let seed_c = s_lch.c.min(if darkmode { 0.03 } else { 0.025 });
                    let surface_ramp = [
                        "background", "surface", "surfaceDim", "surfaceBright", "surfaceVariant",
                        "surfaceContainerLowest", "surfaceContainerLow", "surfaceContainer",
                        "surfaceContainerHigh", "surfaceContainerHighest",
                    ];
                    for key in surface_ramp {
                        if let Some(cur) = material_colors.get(key) {
                            let cur_l = parse_hex(cur).unwrap_or_default().lch().l;
                            let shifted = (seed_l + (cur_l - base_l)).clamp(0.0, 1.0);
                            material_colors.insert(
                                key.to_string(),
                                (LCH { l: shifted, c: seed_c, h: s_lch.h }).rgb().hex().to_uppercase(),
                            );
                        }
                    }
                }
            }
        }
    }

    // Apply washi_roles if provided, only if its primary hue is coherent with the current wallpaper seed
    if let Some(ref washi_path) = cfg.washi_roles {
        if let Some(parent) = Path::new(washi_path).parent() {
            let seeds_path = parent.join("iris-seeds.json");
            let seeds_json = serde_json::json!({
                "primary": material_colors.get("primary").cloned().unwrap_or_default(),
                "secondary": material_colors.get("secondary").cloned().unwrap_or_default(),
                "tertiary": material_colors.get("tertiary").cloned().unwrap_or_default(),
                "background": seed_background,
            });
            if let Ok(txt) = serde_json::to_string(&seeds_json) {
                let unchanged = fs::read_to_string(&seeds_path).map(|s| s == txt).unwrap_or(false);
                if !unchanged {
                    let tmp = seeds_path.with_extension("json.tmp");
                    if fs::write(&tmp, &txt).is_ok() {
                        let _ = fs::rename(&tmp, &seeds_path);
                    }
                }
            }
        }
        let mut applied_washi = false;
        if let Ok(raw) = fs::read_to_string(washi_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(roles) = val.get("roles").and_then(|r| r.as_object()) {
                    if let Some(bg_str) = roles.get("background").and_then(|v| v.as_str()) {
                        if let Some(bg_rgb) = parse_hex(bg_str) {
                            let mode_matches = (bg_rgb.lch().l < 0.55) == darkmode;
                            let hue_coherent = roles
                                .get("primary")
                                .and_then(|v| v.as_str())
                                .and_then(parse_hex)
                                .map(|p| washi::hue_distance(p.lch().h, seed_lch.h) <= 32.0)
                                .unwrap_or(false);
                            if mode_matches && hue_coherent {
                                applied_washi = true;
                                for (k, v) in roles {
                                    if let Some(vs) = v.as_str() {
                                        if parse_hex(vs).is_some() {
                                            material_colors.insert(k.clone(), vs.to_uppercase());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !applied_washi && Path::new(washi_path).exists() {
            if let Ok(raw) = fs::read_to_string(washi_path) {
                if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if let Some(roles) = val.get_mut("roles").and_then(|r| r.as_object_mut()) {
                        for (k, v) in &material_colors {
                            if roles.contains_key(k) {
                                roles.insert(k.clone(), serde_json::Value::String(v.to_lowercase()));
                            }
                        }
                        if let Ok(updated) = serde_json::to_string(&val) {
                            let _ = fs::write(washi_path, updated);
                        }
                    }
                }
            }
        }
    }

    // Generate terminal colors
    let mut term_colors = BTreeMap::new();
    let default_term = [
        ("term0", material_colors.get("surfaceContainerLow").cloned().unwrap_or_else(|| "#161918".into())),
        ("term1", material_colors.get("error").cloned().unwrap_or_else(|| "#CC241D".into())),
        ("term2", material_colors.get("primary").cloned().unwrap_or_else(|| "#98971A".into())),
        ("term3", material_colors.get("tertiary").cloned().unwrap_or_else(|| "#D79921".into())),
        ("term4", material_colors.get("primary").cloned().unwrap_or_else(|| "#458588".into())),
        ("term5", material_colors.get("secondary").cloned().unwrap_or_else(|| "#B16286".into())),
        ("term6", material_colors.get("tertiary").cloned().unwrap_or_else(|| "#689D6A".into())),
        ("term7", material_colors.get("onSurfaceVariant").cloned().unwrap_or_else(|| "#B5ACA0".into())),
        ("term8", material_colors.get("outline").cloned().unwrap_or_else(|| "#928374".into())),
        ("term9", material_colors.get("error").cloned().unwrap_or_else(|| "#FB4934".into())),
        ("term10", material_colors.get("primary").cloned().unwrap_or_else(|| "#B8BB26".into())),
        ("term11", material_colors.get("tertiary").cloned().unwrap_or_else(|| "#FABD2F".into())),
        ("term12", material_colors.get("primary").cloned().unwrap_or_else(|| "#83A598".into())),
        ("term13", material_colors.get("secondary").cloned().unwrap_or_else(|| "#D3869B".into())),
        ("term14", material_colors.get("tertiary").cloned().unwrap_or_else(|| "#8EC07C".into())),
        ("term15", material_colors.get("onSurface").cloned().unwrap_or_else(|| "#EBDBB2".into())),
    ];
    for (k, v) in default_term {
        term_colors.insert(k.to_string(), v);
    }

    if let Some(ref ts_path) = cfg.termscheme {
        if let Ok(raw) = fs::read_to_string(ts_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                let mode_key = if darkmode { "dark" } else { "light" };
                if let Some(obj) = val.get(mode_key).and_then(|v| v.as_object()) {
                    let bg_rgb = parse_hex(&term_colors["term0"]).unwrap_or_default();
                    for (k, v) in obj {
                        if k == "term0" || k == "term15" || k == "term8" {
                            continue;
                        }
                        if let Some(hex_str) = v.as_str() {
                            if let Some(src_rgb) = parse_hex(hex_str) {
                                let mut lch = src_rgb.lch();
                                if k == "term7" {
                                    // Warm stone-sand neutral harmonized gently with the wallpaper hue,
                                    // never forced to bright yellow.
                                    lch.h = washi::hue_toward(lch.h, seed_lch.h, cfg.harmonize_threshold * 0.35 * cfg.harmony);
                                    lch.c = (lch.c * cfg.term_saturation * 2.2).clamp(0.022, 0.042) * cfg.color_strength.clamp(0.0, 1.2);
                                    let solved = ensure_contrast_lch(lch.rgb(), bg_rgb, 4.5, darkmode, None);
                                    term_colors.insert(k.clone(), solved.hex().to_uppercase());
                                } else {
                                    lch.h = washi::hue_toward(lch.h, seed_lch.h, cfg.harmonize_threshold * 0.12 * cfg.harmony);
                                    lch.c = (lch.c * cfg.term_saturation * 1.4).clamp(0.08, 0.16);
                                    let min_r = if k.len() > 5 { 3.5 } else { 4.5 };
                                    let solved = ensure_contrast_lch(lch.rgb(), bg_rgb, min_r, darkmode, None);
                                    term_colors.insert(k.clone(), solved.hex().to_uppercase());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Build palette_json & app_palette_json
    let palette_keys = [
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

    let mut palette_json = BTreeMap::new();
    for (snake, camel) in palette_keys {
        palette_json.insert(
            snake.to_string(),
            material_colors.get(camel).cloned().unwrap_or_default(),
        );
    }
    if let Some(ef) = material_colors.get("errorFill") {
        palette_json.insert("error_fill".to_string(), ef.clone());
        palette_json.insert(
            "on_error_fill".to_string(),
            material_colors.get("onErrorFill").cloned().unwrap_or_default(),
        );
    }

    let app_palette_json = build_app_palette(&palette_json);
    let mut colors_json = palette_json.clone();
    for (tk, tv) in &term_colors {
        colors_json.insert(tk.clone(), tv.clone());
    }

    let mut scss_lines = vec![
        format!("$darkmode: {};", if darkmode { "True" } else { "False" }),
        format!("$transparent: {};", if transparent { "True" } else { "False" }),
    ];
    for (k, v) in &material_colors {
        scss_lines.push(format!("${k}: {v};"));
    }
    for (k, v) in &term_colors {
        scss_lines.push(format!("${k}: {v};"));
    }
    let scss_output = scss_lines.join("\n") + "\n";

    if let Some(ref p) = cfg.scss_output {
        let _ = fs::write(p, &scss_output);
    }
    if let Some(ref p) = cfg.json_output {
        let _ = fs::write(p, serde_json::to_string_pretty(&colors_json).unwrap_or_default());
    }
    if let Some(ref p) = cfg.palette_output {
        let _ = fs::write(p, serde_json::to_string_pretty(&palette_json).unwrap_or_default());
    }
    if let Some(ref p) = cfg.app_palette_output {
        let _ = fs::write(p, serde_json::to_string_pretty(&app_palette_json).unwrap_or_default());
    }
    if let Some(ref p) = cfg.terminal_output {
        let _ = fs::write(p, serde_json::to_string_pretty(&term_colors).unwrap_or_default());
    }
    if let Some(ref p) = cfg.meta_output {
        let meta = serde_json::json!({
            "source": if cfg.path.is_some() { "image" } else if cfg.color.is_some() { "color" } else { "unknown" },
            "source_path": cfg.path,
            "seed_color": seed_rgb.hex().to_uppercase(),
            "mode": if darkmode { "dark" } else { "light" },
            "scheme": cfg.scheme,
            "transparent": transparent,
            "soften": cfg.soften,
            "term_harmony": cfg.harmony,
            "term_saturation": cfg.term_saturation,
            "term_brightness": cfg.term_brightness,
            "term_bg_brightness": cfg.term_bg_brightness,
            "term_fg_boost": cfg.term_fg_boost,
            "harmonize_threshold": cfg.harmonize_threshold,
            "color_strength": cfg.color_strength,
            "blend_bg_fg": cfg.blend_bg_fg,
            "surface_seed": cfg.surface_seed.as_deref().unwrap_or(""),
            "accent_seed": cfg.accent_seed.as_deref().unwrap_or(""),
            "generated_by": "inir-accel (rust)",
        });
        let _ = fs::write(p, serde_json::to_string_pretty(&meta).unwrap_or_default());
    }

    if let Some(ref tpl_dir) = cfg.render_templates {
        render_templates_dir(
            tpl_dir,
            seed_lch,
            darkmode,
            &cfg,
            &material_colors,
            &app_palette_json,
        );
    }

    Ok(scss_output)
}

fn render_templates_dir(
    tpl_dir: &str,
    seed_lch: LCH,
    darkmode: bool,
    cfg: &MaterialGenConfig,
    material_colors: &BTreeMap<String, String>,
    app_palette_json: &BTreeMap<String, String>,
) {
    let manifest_path = Path::new(tpl_dir).join("templates.json");
    let Ok(raw) = fs::read_to_string(&manifest_path) else {
        return;
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Some(templates) = manifest.get("templates").and_then(|t| t.as_array()) else {
        return;
    };

    let opp_raw = generate_material_colors(seed_lch, !darkmode, &cfg.scheme, cfg.soften, cfg.color_strength);
    let mut current = material_colors.clone();
    current.insert("source_color".to_string(), seed_lch.rgb().hex().to_uppercase());
    for (k, v) in app_palette_json {
        current.insert(k.clone(), v.clone());
    }
    let mut opp = opp_raw.clone();
    opp.insert("source_color".to_string(), seed_lch.rgb().hex().to_uppercase());

    let (dark_pal, light_pal) = if darkmode {
        (&current, &opp)
    } else {
        (&opp, &current)
    };

    let mut token_map: HashMap<String, (String, String, String)> = HashMap::new();
    for k in current.keys().chain(opp.keys()) {
        let dk = dark_pal.get(k).cloned().unwrap_or_else(|| "#000000".into());
        let lt = light_pal.get(k).cloned().unwrap_or_else(|| "#000000".into());
        let df = current.get(k).cloned().unwrap_or_else(|| "#000000".into());
        token_map.insert(k.clone(), (dk.clone(), lt.clone(), df.clone()));
        let snake = camel_to_snake(k);
        if snake != *k {
            token_map.insert(snake, (dk, lt, df));
        }
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let templates_base = Path::new(tpl_dir).join("templates");

    for entry in templates {
        let Some(inp) = entry.get("input").and_then(|v| v.as_str()) else { continue };
        let Some(out_raw) = entry.get("output").and_then(|v| v.as_str()) else { continue };
        let out_path = if let Some(rest) = out_raw.strip_prefix("~/") {
            format!("{home}/{rest}")
        } else {
            out_raw.to_string()
        };
        let tpl_path = templates_base.join(inp);
        let Ok(content) = fs::read_to_string(&tpl_path) else { continue };

        let rendered = replace_template_vars(&content, &token_map, cfg.path.as_deref().unwrap_or(""));
        if let Some(parent) = Path::new(&out_path).parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::symlink_metadata(&out_path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            let _ = fs::remove_file(&out_path);
        }
        let _ = fs::write(&out_path, rendered);
    }
}

fn replace_template_vars(
    content: &str,
    tokens: &HashMap<String, (String, String, String)>,
    image_path: &str,
) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let expr = after_open[..end].trim();
        if expr == "image" {
            out.push_str(image_path);
        } else {
            let parts: Vec<&str> = expr.split('.').collect();
            if parts.len() == 4 && parts[0] == "colors" {
                if let Some((dk, lt, df)) = tokens.get(parts[1]) {
                    let chosen = match parts[2] {
                        "dark" => dk,
                        "light" => lt,
                        "default" => df,
                        _ => "",
                    };
                    if !chosen.is_empty() {
                        let stripped = chosen.trim_start_matches('#');
                        match parts[3] {
                            "hex" => out.push_str(chosen),
                            "hex_stripped" => out.push_str(stripped),
                            "rgb" if stripped.len() == 6 => {
                                let r = u8::from_str_radix(&stripped[0..2], 16).unwrap_or(0);
                                let g = u8::from_str_radix(&stripped[2..4], 16).unwrap_or(0);
                                let b = u8::from_str_radix(&stripped[4..6], 16).unwrap_or(0);
                                out.push_str(&format!("{r}, {g}, {b}"));
                            }
                            _ => out.push_str(&rest[start..start + 2 + end + 2]),
                        }
                    } else {
                        out.push_str(&rest[start..start + 2 + end + 2]);
                    }
                } else {
                    out.push_str(&rest[start..start + 2 + end + 2]);
                }
            } else {
                out.push_str(&rest[start..start + 2 + end + 2]);
            }
        }
        rest = &after_open[end + 2..];
    }
    out.push_str(rest);
    out
}
