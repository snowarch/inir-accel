//! Port of `scripts/colors/system24_palette.py`, line for line: the System24, Midnight and TUI Discord themes
//! (Vesktop, Equicord, Equibop) from the generated palette. `test/parity.py system24`.

use crate::material::py::{self, Json};
use crate::pyrt::{self, exc, R};
use std::path::{Path, PathBuf};

const THEME_TEMPLATE: &str = r#"/**
 * @name iNiR Material
 * @description Material Design Discord theme with Material You colors.
 * @author refact0r (system24 base), iNiR (Material adaptation)
 * @version 2.2.0
 * @source https://github.com/snowarch/iNiR
 */

/*
 * Base theme import:
 * - A local copy (system24.local.css next to this file) is imported first when it exists.
 * - The remote import keeps the theme working without it.
 */
{local_import}@import url('https://refact0r.github.io/system24/build/system24.css');
@import url('https://fonts.googleapis.com/css2?family=Oxanium:wght@300;400;500;600;700&display=swap');

body {{
    --font: 'Oxanium';
    --code-font: 'Oxanium';
    font-weight: 400;
    letter-spacing: -0.02ch;
    --gap: 12px;
    --divider-thickness: 4px;
    --border-thickness: 1px;
    --border-hover-transition: 0.2s ease;
    --animations: on;
    --list-item-transition: 0.2s ease;
    --dms-icon-svg-transition: 0.4s ease;
    --top-bar-height: 36px;
    --top-bar-button-position: off;
    --top-bar-title-position: off;
    --subtle-top-bar-title: off;
    --custom-window-controls: off;
    --window-control-size: 14px;
    --custom-dms-icon: off;
    --dms-icon-svg-size: 90%;
    --dms-icon-color-before: var(--text-0);
    --dms-icon-color-after: var(--text-0);
    --custom-dms-background: color;
    --dms-background-color: var(--accent-2);
    --background-image: off;
    --transparency-tweaks: off;
    --remove-bg-layer: off;
    --panel-blur: off;
    --blur-amount: 0px;
    --bg-floating: var(--bg-1);
    --small-user-panel: on;
    --unrounding: off;
    --custom-spotify-bar: on;
    --ascii-titles: off;
    --ascii-loader: off;
    --panel-labels: off;
    --label-color: var(--text-4);
    --label-font-weight: 500;
}}

/* Material You Palette - Auto-generated */
{palette_css}

/* Material Design System */
"#;

const TUI_THEME_TEMPLATE: &str = r#"/**
 * @name iNiR TUI
 * @description System24 TUI layout with iNiR Material You colors.
 * @author refact0r (system24 base), iNiR (palette integration)
 * @version 2.2.0
 * @source https://github.com/snowarch/inir
 */

{local_import}@import url('https://refact0r.github.io/system24/build/system24.css');

body {{
    --font: 'JetBrainsMono Nerd Font';
    --code-font: 'JetBrainsMono Nerd Font';
    font-weight: 300;
    letter-spacing: -0.05ch;
    --gap: 12px;
    --divider-thickness: 4px;
    --border-thickness: 2px;
    --border-hover-transition: 0.2s ease;
    --animations: on;
    --list-item-transition: 0.2s ease;
    --dms-icon-svg-transition: 0.4s ease;
    --top-bar-height: var(--gap);
    --top-bar-button-position: titlebar;
    --top-bar-title-position: off;
    --subtle-top-bar-title: off;
    --custom-window-controls: off;
    --window-control-size: 14px;
    --custom-dms-icon: off;
    --dms-icon-svg-size: 90%;
    --dms-icon-color-before: var(--text-4);
    --dms-icon-color-after: var(--text-1);
    --custom-dms-background: off;
    --background-image: off;
    --transparency-tweaks: off;
    --remove-bg-layer: off;
    --panel-blur: off;
    --blur-amount: 12px;
    --bg-floating: var(--bg-3);
    --small-user-panel: on;
    --unrounding: on;
    --round-pfp: off;
    --remove-pfp-decor: off;
    --custom-spotify-bar: on;
    --ascii-titles: on;
    --ascii-loader: system24;
    --panel-labels: on;
    --label-color: var(--text-5);
    --label-font-weight: 500;
}}

/* Material You Palette - Auto-generated */
{palette_css}
"#;

const MIDNIGHT_THEME_TEMPLATE: &str = r#"/**
 * @name iNiR Midnight
 * @description iNiR Midnight Discord theme with Material You colors.
 * @author iNiR (Material palette injection)
 * @version 2.2.0
 * @source https://github.com/snowarch/iNiR
 */

/*
 * Base theme import:
 * - Prefer your local copy (if you have it in the same folder).
 * - Fallback to remote so the theme works on fresh installs.
 */
{local_import}@import url('https://refact0r.github.io/midnight-discord/build/midnight.css');

/* Material You Palette - Auto-generated */
{palette_css}

/*
 * Reduce hover/active highlight.
 * The base theme can still style hover states, but the strong accent overlay is removed.
 */
:root, :root:root, body, #app-mount {{
    --hover: color-mix(in hsl, var(--text-3), transparent 92%);
    --active: color-mix(in hsl, var(--text-3), transparent 86%);
    --active-2: color-mix(in hsl, var(--text-3), transparent 80%);
    --message-hover: color-mix(in hsl, var(--text-3), transparent 94%);
    --border-hover: var(--border);
    --border-light: var(--border);
    --button-border: var(--border);
}}
"#;

/// `template.format(palette_css=..., local_import=...)`
fn format(template: &str, palette_css: &str, local_import: &str) -> String {
    let mut out = String::with_capacity(template.len() + palette_css.len());
    let mut rest = template;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("{{") {
            out.push('{');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("}}") {
            out.push('}');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{palette_css}") {
            out.push_str(palette_css);
            rest = r;
        } else if let Some(r) = rest.strip_prefix("{local_import}") {
            out.push_str(local_import);
            rest = r;
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

fn env_path(var: &str, default: &str) -> PathBuf {
    PathBuf::from(py::expanduser(&std::env::var(var).unwrap_or_else(|_| default.to_string())))
}

fn resolve_output_files(var: &str, default: &Path) -> Vec<PathBuf> {
    if let Ok(explicit) = std::env::var(var) {
        if !explicit.is_empty() {
            return vec![PathBuf::from(py::expanduser(&explicit))];
        }
    }
    let base = default.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let existing: Vec<PathBuf> = ["vesktop", "Vesktop", "equicord", "Equicord", "equibop", "Equibop"]
        .iter()
        .map(|d| PathBuf::from(py::expanduser(&format!("~/.config/{d}/themes/{base}"))))
        .filter(|p| p.parent().is_some_and(pyrt::exists))
        .collect();
    if existing.is_empty() { vec![default.to_path_buf()] } else { existing }
}

type Colors = Vec<(String, String)>;

fn load_colors() -> R<Colors> {
    let color = env_path("QUICKSHELL_PALETTE_JSON", "~/.local/state/quickshell/user/generated/app-palette.json");
    let raw = env_path("QUICKSHELL_RAW_PALETTE_JSON", "~/.local/state/quickshell/user/generated/palette.json");
    let fallback = env_path("QUICKSHELL_COLORS_JSON", "~/.local/state/quickshell/user/generated/colors.json");
    let source = if pyrt::exists(&color) { &color } else if pyrt::exists(&raw) { &raw } else { &fallback };
    if !pyrt::exists(source) {
        return Err(exc("FileNotFoundError", format!(
            "Material colors file not found: {}, {}, or {}. Ensure switchwall.sh has been executed successfully.",
            color.display(), raw.display(), fallback.display())));
    }
    let data = pyrt::json_load(source).map_err(|e| if e.ty == "FileNotFoundError" { exc("OSError", e.msg) } else { e })?;
    let Json::Obj(items) = data else {
        return Err(exc("AttributeError", format!("'{}' object has no attribute 'items'", pyrt::type_name(&data))));
    };
    let mut out: Colors = vec![];
    for (k, v) in items {
        let v = pyrt::as_str(&v, "lower")?.to_lowercase();
        match out.iter_mut().find(|(key, _)| *key == k) {
            Some(slot) => slot.1 = v,
            None => out.push((k, v)),
        }
    }
    Ok(out)
}

fn hex_to_rgb(color: &str) -> R<[i64; 3]> {
    let c = color.trim_start_matches('#');
    Ok([pyrt::int16(&pyrt::slice(c, 0, Some(2)))?, pyrt::int16(&pyrt::slice(c, 2, Some(4)))?, pyrt::int16(&pyrt::slice(c, 4, Some(6)))?])
}

fn rgb_to_hex(rgb: [f64; 3]) -> String {
    let ch = |v: f64| ((v + 0.5).floor() as i64).clamp(0, 255);
    format!("#{:02x}{:02x}{:02x}", ch(rgb[0]), ch(rgb[1]), ch(rgb[2]))
}

fn mix(a: &str, b: &str, w: f64) -> R<String> {
    let (x, y) = (hex_to_rgb(a)?, hex_to_rgb(b)?);
    let m = |i: usize| x[i] as f64 * (1.0 - w) + y[i] as f64 * w;
    Ok(rgb_to_hex([m(0), m(1), m(2)]))
}

fn pmax(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

fn pmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// colorsys.rgb_to_hls (3.12)
fn rgb_to_hls(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let maxc = pmax(pmax(r, g), b);
    let minc = pmin(pmin(r, g), b);
    let sumc = maxc + minc;
    let rangec = maxc - minc;
    let l = sumc / 2.0;
    if minc == maxc {
        return (0.0, l, 0.0);
    }
    let s = if l <= 0.5 { rangec / sumc } else { rangec / (2.0 - maxc - minc) };
    let rc = (maxc - r) / rangec;
    let gc = (maxc - g) / rangec;
    let bc = (maxc - b) / rangec;
    let h = if r == maxc { bc - gc } else if g == maxc { 2.0 + rc - bc } else { 4.0 + gc - rc };
    (py::fmod(h / 6.0, 1.0), l, s)
}

fn hls_v(m1: f64, m2: f64, hue: f64) -> f64 {
    let hue = py::fmod(hue, 1.0);
    if hue < 1.0 / 6.0 {
        return m1 + (m2 - m1) * hue * 6.0;
    }
    if hue < 0.5 {
        return m2;
    }
    if hue < 2.0 / 3.0 {
        return m1 + (m2 - m1) * (2.0 / 3.0 - hue) * 6.0;
    }
    m1
}

/// colorsys.hls_to_rgb
fn hls_to_rgb(h: f64, l: f64, s: f64) -> (f64, f64, f64) {
    if s == 0.0 {
        return (l, l, l);
    }
    let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - (l * s) };
    let m1 = 2.0 * l - m2;
    (hls_v(m1, m2, h + 1.0 / 3.0), hls_v(m1, m2, h), hls_v(m1, m2, h - 1.0 / 3.0))
}

fn adjust_lightness(color: &str, delta: f64) -> R<String> {
    let c = hex_to_rgb(color)?;
    let (h, l, s) = rgb_to_hls(c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0);
    let l = pmax(0.0, pmin(1.0, l + delta));
    let (r, g, b) = hls_to_rgb(h, l, s);
    Ok(rgb_to_hex([r * 255.0, g * 255.0, b * 255.0]))
}

fn rgba(color: &str, alpha: f64) -> R<String> {
    let [r, g, b] = hex_to_rgb(color)?;
    Ok(format!("rgba({r}, {g}, {b}, {alpha:.2})"))
}

fn build_palette(colors: &Colors) -> R<Vec<(String, String)>> {
    let get = |k: &str| colors.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    let req = |k: &str| get(k).ok_or_else(|| exc("KeyError", pyrt::repr_str(k)));
    let primary = req("primary")?;
    let secondary = req("secondary")?;
    let tertiary = req("tertiary")?;
    let error = req("error")?;
    let surface = req("surface")?;
    let surface_low = req("surface_container_low")?;
    let surface_std = req("surface_container")?;
    let surface_high = req("surface_container_high")?;
    let surface_highest = req("surface_container_highest")?;
    let outline = req("outline")?;
    let on_surface = req("on_surface")?;
    let on_surface_variant = req("on_surface_variant")?;
    let on_primary = req("on_primary")?;
    let truthy = |v: Option<String>| v.filter(|s| !s.is_empty());
    let success = truthy(get("success")).unwrap_or_else(|| tertiary.clone());
    let warning = match truthy(get("app_warning")) {
        Some(w) => w,
        None => mix(&primary, &tertiary, 0.3)?,
    };
    let mut p: Vec<(String, String)> = vec![];
    let mut put = |k: &str, v: String| match p.iter_mut().find(|(key, _)| key == k) {
        Some(slot) => slot.1 = v,
        None => p.push((k.to_string(), v)),
    };
    put("--colors", "on".into());
    put("--text-0", on_primary);
    put("--text-1", on_surface.clone());
    put("--text-2", mix(&on_surface, &on_surface_variant, 0.2)?);
    put("--text-3", mix(&on_surface, &on_surface_variant, 0.4)?);
    put("--text-4", on_surface_variant.clone());
    put("--text-5", outline.clone());
    put("--bg-4", surface);
    put("--bg-3", surface_low);
    put("--bg-2", surface_std);
    put("--bg-1", surface_high);
    put("--bg-floating", surface_highest);
    put("--hover", rgba(&primary, 0.10)?);
    put("--active", rgba(&primary, 0.20)?);
    put("--active-2", rgba(&primary, 0.30)?);
    put("--message-hover", rgba(&primary, 0.08)?);
    put("--accent-1", adjust_lightness(&primary, 0.10)?);
    put("--accent-2", primary.clone());
    put("--accent-3", adjust_lightness(&primary, -0.05)?);
    put("--accent-4", adjust_lightness(&primary, -0.10)?);
    put("--accent-5", adjust_lightness(&primary, -0.15)?);
    put("--accent-new", truthy(get("error_fill")).unwrap_or_else(|| primary.clone()));
    put("--mention", format!("linear-gradient(to right, color-mix(in hsl, {primary}, transparent 90%) 40%, transparent)"));
    put("--mention-hover", format!("linear-gradient(to right, color-mix(in hsl, {primary}, transparent 95%) 40%, transparent)"));
    put("--reply", format!("linear-gradient(to right, color-mix(in hsl, {on_surface_variant}, transparent 90%) 40%, transparent)"));
    put("--reply-hover", format!("linear-gradient(to right, color-mix(in hsl, {on_surface_variant}, transparent 95%) 40%, transparent)"));
    put("--online", success.clone());
    put("--dnd", error.clone());
    put("--idle", secondary.clone());
    put("--streaming", adjust_lightness(&secondary, 0.10)?);
    put("--offline", on_surface_variant);
    put("--border-light", rgba(&outline, 0.12)?);
    put("--border", rgba(&outline, 0.22)?);
    put("--border-hover", rgba(&mix(&outline, &primary, 0.35)?, 0.50)?);
    put("--button-border", rgba(&outline, 0.16)?);
    let ladder = |base: &str| -> R<Vec<String>> {
        Ok(vec![adjust_lightness(base, 0.15)?, adjust_lightness(base, 0.08)?, base.to_string(),
            adjust_lightness(base, -0.08)?, adjust_lightness(base, -0.15)?])
    };
    let red_base = truthy(get("error_fill")).unwrap_or_else(|| error.clone());
    let purple_base = mix(&primary, &secondary, 0.5)?;
    let orange_base = mix(&primary, &error, 0.3)?;
    for (name, base) in [("red", red_base), ("green", success), ("blue", secondary), ("yellow", warning)] {
        for (i, c) in ladder(&base)?.into_iter().enumerate() {
            put(&format!("--{name}-{}", i + 1), c);
        }
    }
    for (i, c) in ladder(&purple_base)?.into_iter().enumerate() {
        put(&format!("--purple-{}", i + 1), c);
    }
    for (i, c) in ladder(&orange_base)?.into_iter().enumerate() {
        put(&format!("--orange-{}", i + 1), c);
    }
    put("--brand-360", adjust_lightness(&primary, 0.08)?);
    put("--brand-400", adjust_lightness(&primary, 0.04)?);
    put("--brand-500", primary.clone());
    put("--brand-560", adjust_lightness(&primary, -0.06)?);
    put("--brand-600", adjust_lightness(&primary, -0.12)?);
    Ok(p)
}

fn discord_roles_css(palette: &[(String, String)]) -> R<String> {
    let roles = [("--header-primary", "--text-1"), ("--header-secondary", "--text-3"), ("--text-strong", "--text-1"),
        ("--text-normal", "--text-2"), ("--text-default", "--text-2"), ("--text-muted", "--text-5"), ("--text-subtle", "--text-4"),
        ("--interactive-normal", "--text-4"), ("--interactive-hover", "--text-2"), ("--interactive-active", "--text-1"),
        ("--interactive-muted", "--text-5"), ("--channels-default", "--text-4"), ("--channel-icon", "--text-4"),
        ("--text-primary", "--text-1"), ("--text-secondary", "--text-3"), ("--text-tertiary", "--text-5"),
        ("--white", "--text-0"), ("--white-500", "--text-0"), ("--text-overlay-light", "--text-0")];
    let mut lines = vec![":is(.theme-dark, .theme-light):not(.custom-user-profile-theme), :is(.theme-dark, .theme-light) .theme-dark, :is(.theme-dark, .theme-light) .theme-light {".to_string()];
    lines.extend(roles.iter().map(|(k, v)| format!("    {k}: var({v});")));
    lines.push("}".into());
    let bg = &palette.iter().find(|(k, _)| k == "--bg-4").unwrap().1;
    if hex_to_rgb(bg)?.iter().sum::<i64>() > 3 * 128 {
        lines.push(r#":is([class*="username"], [class*="roleColor"], [class*="mention"])[style*="color: rgb(255, 255, 255)"] { color: var(--text-1) !important; }"#.into());
    }
    Ok(lines.join("\n"))
}

fn build_palette_css(palette: &[(String, String)]) -> R<String> {
    let mut keys: Vec<&(String, String)> = palette.iter().collect();
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    let mut lines = vec![":root, :root:root, body, #app-mount {".to_string()];
    lines.extend(keys.iter().map(|(k, v)| format!("    {k}: {v};")));
    lines.push("}".into());
    Ok(format!("{}\n\n{}", lines.join("\n"), discord_roles_css(palette)?))
}

fn write_theme(out: &Path, template: &str, palette_css: &str, local_copy: &str) -> R<()> {
    let local = out.parent().unwrap_or(Path::new("")).join(local_copy);
    let local_import = if local.is_file() { format!("@import url('{local_copy}');\n") } else { String::new() };
    let content = format(template, palette_css, &local_import);
    match pyrt::read_text(out) {
        Ok(old) if old == content => return Ok(()),
        Ok(_) => {}
        Err(e) if e.ty == "UnicodeDecodeError" => return Err(e),
        Err(_) => {}
    }
    pyrt::write_text(out, &content)?;
    println!("Generated: {}", out.display());
    Ok(())
}

fn write_palette(palette: &[(String, String)]) -> R<()> {
    let css = build_palette_css(palette)?;
    let s24 = resolve_output_files("SYSTEM24_PALETTE_CSS", &env_path("SYSTEM24_PALETTE_CSS", "~/.config/vesktop/themes/system24.theme.css"));
    let midnight = resolve_output_files("MIDNIGHT_DMS_CSS", &env_path("MIDNIGHT_DMS_CSS", "~/.config/vesktop/themes/inir-midnight.theme.css"));
    let tui = resolve_output_files("INIR_TUI_CSS", &env_path("INIR_TUI_CSS", "~/.config/vesktop/themes/inir-tui.theme.css"));
    for out in s24.iter().chain(&midnight).chain(&tui) {
        pyrt::makedirs(out.parent().unwrap_or(Path::new("")))?;
    }
    for out in &s24 {
        write_theme(out, THEME_TEMPLATE, &css, "system24.local.css")?;
    }
    for out in &midnight {
        write_theme(out, MIDNIGHT_THEME_TEMPLATE, &css, "midnight-discord.local.css")?;
        let legacy = out.parent().unwrap_or(Path::new("")).join("ii-midnight.theme.css");
        if &legacy != out {
            match std::fs::remove_file(&legacy) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(pyrt::os_exc(&e, &legacy)),
                _ => {}
            }
        }
    }
    for out in &tui {
        write_theme(out, TUI_THEME_TEMPLATE, &css, "system24.local.css")?;
    }
    Ok(())
}

fn run() -> R<()> {
    let colors = match load_colors() {
        Ok(c) => c,
        Err(e) if e.ty == "FileNotFoundError" => {
            println!("Error: {}", e.msg);
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    write_palette(&build_palette(&colors)?)
}

/// `inir-accel system24-themegen`: the exit code.
pub fn run_cli(_args: &[String]) -> i32 {
    match run() {
        Ok(()) => 0,
        Err(e) => pyrt::die(e),
    }
}
