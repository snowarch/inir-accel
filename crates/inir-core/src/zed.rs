//! `scripts/colors/zed_themegen/main.go`, line for line: `inir-zed-themegen [--scss f] [--colors f] [--terminal-json f]
//! [--output f] [--template f] [scss [colors [terminal-json]]]`. JSON goes through Go 1.27's own reading (`gojsonv`)
//! and writing (`marshal_indent`, maps sorted), files through Go's `os` (modes, error texts).
//! This code was written by: Hikari.
use crate::gojsonv::{self, go_quote};
use crate::gotheme::{self, go_trim_space, marshal_indent, Colors, J};
use crate::vscode::{dir_of, go_max, go_min, home, join, mkdir_all, read_file, read_string_map_json, write_file};
use std::collections::BTreeMap;
use std::fs;

type Style = BTreeMap<String, J>;

const DEFAULT_TERM: [&str; 16] = [
    "#1d1f21", "#cc6666", "#b5bd68", "#f0c674", "#81a2be", "#b294bb", "#8abeb7", "#c5c8c6", "#666666", "#d54e53", "#b9ca4a",
    "#e7c547", "#7aa6da", "#c397d8", "#70c0ba", "#ffffff",
];

#[derive(Clone, Copy, Default)]
struct Hsl {
    h: f64,
    s: f64,
    l: f64,
}

/// `^#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{8})$` on the trimmed value.
fn is_hex_color(v: &str) -> bool {
    match go_trim_space(v).strip_prefix('#') {
        Some(h) => (h.len() == 6 || h.len() == 8) && h.bytes().all(|b| b.is_ascii_hexdigit()),
        None => false,
    }
}

fn normalize_hex6(v: &str) -> String {
    let v = go_trim_space(v).to_lowercase();
    let v = v.strip_prefix('#').unwrap_or(&v);
    if v.len() < 6 {
        return "#000000".into();
    }
    format!("#{}", String::from_utf8_lossy(&v.as_bytes()[..6]))
}

fn hex_with_alpha(c: &str, alpha: &str) -> String {
    normalize_hex6(c) + &alpha.to_lowercase()
}

fn ha(c: &str, alpha: &str) -> J {
    J::Str(hex_with_alpha(c, alpha))
}

/// `strconv.ParseInt(s, 16, 64)`, which leaves 0 on a syntax error.
fn parse_hex(b: &[u8]) -> i64 {
    let (neg, d) = match b.first() {
        Some(b'+') => (false, &b[1..]),
        Some(b'-') => (true, &b[1..]),
        _ => (false, b),
    };
    if d.is_empty() || !d.iter().all(u8::is_ascii_hexdigit) {
        return 0;
    }
    let v = d.iter().fold(0i64, |v, c| v * 16 + (*c as char).to_digit(16).unwrap_or(0) as i64);
    if neg {
        -v
    } else {
        v
    }
}

/// The three channels of a `#`-less hex string of at least six bytes.
fn channels(h: &[u8]) -> (i64, i64, i64) {
    (parse_hex(&h[0..2]), parse_hex(&h[2..4]), parse_hex(&h[4..6]))
}

fn hex_to_hsl(hex: &str) -> Hsl {
    let lower = hex.to_lowercase();
    let h = lower.strip_prefix('#').unwrap_or(&lower).as_bytes();
    if h.len() < 6 {
        return Hsl::default();
    }
    let (r, g, b) = channels(h);
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max_c = go_max(r, go_max(g, b));
    let min_c = go_min(r, go_min(g, b));
    let l = (max_c + min_c) / 2.0;
    if max_c == min_c {
        return Hsl { h: 0.0, s: 0.0, l };
    }
    let d = max_c - min_c;
    let s = if l > 0.5 { d / (2.0 - max_c - min_c) } else { d / (max_c + min_c) };
    let h = if max_c == r {
        let mut h = (g - b) / d;
        if g < b {
            h += 6.0;
        }
        h
    } else if max_c == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    Hsl { h: h / 6.0, s, l }
}

fn hsl_to_hex(c: Hsl) -> String {
    let mut h = c.h;
    while h < 0.0 {
        h += 1.0;
    }
    while h > 1.0 {
        h -= 1.0;
    }
    let s = go_max(0.0, go_min(1.0, c.s));
    let l = go_max(0.0, go_min(1.0, c.l));
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (hue_to_rgb(p, q, h + 1.0 / 3.0), hue_to_rgb(p, q, h), hue_to_rgb(p, q, h - 1.0 / 3.0))
    };
    format!("#{:02x}{:02x}{:02x}", clamp255(r), clamp255(g), clamp255(b))
}

fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

fn clamp255(v: f64) -> i64 {
    ((v * 255.0).round() as i64).clamp(0, 255)
}

fn adjust_lightness(hex: &str, factor: f64) -> String {
    let mut c = hex_to_hsl(hex);
    c.l = go_max(0.0, go_min(1.0, c.l * factor));
    hsl_to_hex(c)
}

fn saturate(hex: &str, factor: f64, min_sat: f64, additive_floor: f64, hue_hint: f64) -> String {
    let mut c = hex_to_hsl(hex);
    if c.s < 0.001 {
        c.h = (hue_hint + 1.0) % 1.0;
    }
    let mut boosted = go_min(1.0, c.s * factor + additive_floor * (1.0 - c.s));
    if boosted < min_sat {
        boosted = go_min(1.0, boosted + min_sat);
    }
    c.s = boosted;
    hsl_to_hex(c)
}

fn blend_colors(base: &str, mix: &str, mix_ratio: f64) -> String {
    let (base, mix) = (normalize_hex6(base), normalize_hex6(mix));
    let (br, bg, bb) = channels(&base.as_bytes()[1..]);
    let (mr, mg, mb) = channels(&mix.as_bytes()[1..]);
    let ratio = go_max(0.0, go_min(1.0, mix_ratio));
    let m = |b: i64, x: i64| ((b as f64 * (1.0 - ratio) + x as f64 * ratio).round() as i64).clamp(0, 255);
    format!("#{:02x}{:02x}{:02x}", m(br, mr), m(bg, mg), m(bb, mb))
}

fn luminance(hex: &str) -> f64 {
    let h = normalize_hex6(hex);
    let (r, g, b) = channels(&h.as_bytes()[1..]);
    r as f64 * 0.299 + g as f64 * 0.587 + b as f64 * 0.114
}

fn term_color(term_colors: &Colors, idx: usize) -> String {
    match term_colors.get(&format!("term{idx}")) {
        Some(v) if is_hex_color(v) => normalize_hex6(v),
        _ => DEFAULT_TERM.get(idx).unwrap_or(&"#ffffff").to_string(),
    }
}

fn syntax_entry(color: &str, font_style: J, font_weight: J) -> J {
    let mut m = Style::new();
    m.insert("color".into(), ha(color, "ff"));
    m.insert("font_style".into(), font_style);
    m.insert("font_weight".into(), font_weight);
    J::Map(m)
}

type SyntaxToken = (&'static str, &'static str, Option<&'static str>, Option<f64>);

const ONE_DARK_SYNTAX: &[SyntaxToken] = &[
    ("attribute", "#74ade8", None, None),
    ("boolean", "#bf956a", None, None),
    ("comment", "#5d636f", None, None),
    ("comment.doc", "#878e98", None, None),
    ("constant", "#dfc184", None, None),
    ("constructor", "#73ade9", None, None),
    ("embedded", "#dce0e5", None, None),
    ("emphasis", "#74ade8", None, None),
    ("emphasis.strong", "#bf956a", None, Some(700.0)),
    ("enum", "#6eb4bf", None, None),
    ("function", "#73ade9", None, None),
    ("hint", "#788ca6", None, None),
    ("keyword", "#b477cf", None, None),
    ("label", "#74ade8", None, None),
    ("link_text", "#73ade9", Some("normal"), None),
    ("link_uri", "#6eb4bf", None, None),
    ("namespace", "#dce0e5", None, None),
    ("number", "#bf956a", None, None),
    ("operator", "#6eb4bf", None, None),
    ("predictive", "#5a6a87", Some("italic"), None),
    ("preproc", "#dce0e5", None, None),
    ("primary", "#acb2be", None, None),
    ("property", "#d07277", None, None),
    ("punctuation", "#acb2be", None, None),
    ("punctuation.bracket", "#b2b9c6", None, None),
    ("punctuation.delimiter", "#b2b9c6", None, None),
    ("punctuation.list_marker", "#d07277", None, None),
    ("punctuation.markup", "#d07277", None, None),
    ("punctuation.special", "#b1574b", None, None),
    ("selector", "#dfc184", None, None),
    ("selector.pseudo", "#74ade8", None, None),
    ("string", "#a1c181", None, None),
    ("string.escape", "#878e98", None, None),
    ("string.regex", "#bf956a", None, None),
    ("string.special", "#bf956a", None, None),
    ("string.special.symbol", "#bf956a", None, None),
    ("tag", "#74ade8", None, None),
    ("text.literal", "#a1c181", None, None),
    ("title", "#d07277", None, Some(400.0)),
    ("type", "#6eb4bf", None, None),
    ("variable", "#acb2be", None, None),
    ("variable.special", "#bf956a", None, None),
    ("variant", "#73ade9", None, None),
];

const ONE_LIGHT_SYNTAX: &[SyntaxToken] = &[
    ("attribute", "#5c78e2", None, None),
    ("boolean", "#ad6e25", None, None),
    ("comment", "#a2a3a7", None, None),
    ("comment.doc", "#7c7e86", None, None),
    ("constant", "#c18401", None, None),
    ("constructor", "#5c78e2", None, None),
    ("embedded", "#242529", None, None),
    ("emphasis", "#5c78e2", None, None),
    ("emphasis.strong", "#ad6e25", None, Some(700.0)),
    ("enum", "#3882b7", None, None),
    ("function", "#5b79e3", None, None),
    ("hint", "#7274a7", None, None),
    ("keyword", "#a449ab", None, None),
    ("label", "#5c78e2", None, None),
    ("link_text", "#5b79e3", Some("italic"), None),
    ("link_uri", "#3882b7", None, None),
    ("namespace", "#242529", None, None),
    ("number", "#ad6e25", None, None),
    ("operator", "#3882b7", None, None),
    ("predictive", "#9b9ec6", Some("italic"), None),
    ("preproc", "#242529", None, None),
    ("primary", "#242529", None, None),
    ("property", "#d3604f", None, None),
    ("punctuation", "#242529", None, None),
    ("punctuation.bracket", "#4d4f52", None, None),
    ("punctuation.delimiter", "#4d4f52", None, None),
    ("punctuation.list_marker", "#d3604f", None, None),
    ("punctuation.markup", "#d3604f", None, None),
    ("punctuation.special", "#b92b46", None, None),
    ("selector", "#669f59", None, None),
    ("selector.pseudo", "#5c78e2", None, None),
    ("string", "#649f57", None, None),
    ("string.escape", "#7c7e86", None, None),
    ("string.regex", "#ad6e26", None, None),
    ("string.special", "#ad6e26", None, None),
    ("string.special.symbol", "#ad6e26", None, None),
    ("tag", "#5c78e2", None, None),
    ("text.literal", "#649f57", None, None),
    ("title", "#d3604f", None, Some(400.0)),
    ("type", "#3882b7", None, None),
    ("variable", "#242529", None, None),
    ("variable.special", "#ad6e25", None, None),
    ("variant", "#5b79e3", None, None),
];

/// Zed's built-in One Dark/One Light syntax, blended halfway into the theme primary.
fn build_syntax_map(primary: &str, appearance: &str) -> J {
    let defaults = if appearance == "light" { ONE_LIGHT_SYNTAX } else { ONE_DARK_SYNTAX };
    let mut syntax = Style::new();
    for (token, color, style, weight) in defaults {
        let entry = syntax_entry(&blend_colors(color, primary, 0.50), style.map_or(J::Null, J::from), weight.map_or(J::Null, J::Num));
        syntax.insert(token.to_string(), entry);
    }
    J::Map(syntax)
}

const BORDER_KEYS: [&str; 8] =
    ["border", "border.variant", "border.focused", "border.selected", "border.transparent", "border.disabled", "scrollbar.thumb.border", "scrollbar.track.border"];

fn apply_border_palette(dst: &mut Style, src: &Style) {
    for key in BORDER_KEYS.iter().chain(&["panel.focused_border", "pane.focused_border"]) {
        if let Some(v) = src.get(*key) {
            dst.insert(key.to_string(), v.clone());
        }
    }
}

fn make_borderless_style(style: &Style) -> Style {
    let mut out = style.clone();
    for key in BORDER_KEYS {
        if let Some(v) = out.get_mut(key) {
            *v = J::from("#00000000");
        }
    }
    out.insert("panel.focused_border".into(), J::Null);
    out.insert("pane.focused_border".into(), J::Null);
    out
}

fn sanitize_hex_map(m: &Colors) -> Colors {
    m.iter().filter(|(_, v)| is_hex_color(v)).map(|(k, v)| (k.clone(), v.to_lowercase())).collect()
}

/// `^term\d{1,2}$` keys holding a hex colour.
fn parse_term_map(m: &Colors) -> Colors {
    let term_key = |k: &str| k.strip_prefix("term").is_some_and(|d| (1..=2).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()));
    m.iter().filter(|(k, v)| term_key(k) && is_hex_color(v)).map(|(k, v)| (k.clone(), v.to_lowercase())).collect()
}

fn read_string_json(path: &str) -> Colors {
    read_string_map_json(path).unwrap_or_default()
}

fn pick(m: &Colors, key: &str, fallback: &str) -> String {
    match m.get(key) {
        Some(v) if !v.is_empty() => normalize_hex6(v),
        _ => normalize_hex6(fallback),
    }
}

fn style(entries: Vec<(&str, J)>) -> Style {
    entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn players(colors: &[String], selection: impl Fn(&str) -> String) -> J {
    J::Arr(
        colors
            .iter()
            .map(|c| J::Map(style(vec![("cursor", ha(c, "ff")), ("background", ha(c, "ff")), ("selection", ha(&selection(c), "3d"))])))
            .collect(),
    )
}

const ANSI: [(&str, usize); 21] = [
    ("red", 1), ("bright_red", 9), ("dim_red", 1),
    ("green", 2), ("bright_green", 10), ("dim_green", 2),
    ("yellow", 3), ("bright_yellow", 11), ("dim_yellow", 3),
    ("blue", 4), ("bright_blue", 12), ("dim_blue", 4),
    ("magenta", 5), ("bright_magenta", 13), ("dim_magenta", 5),
    ("cyan", 6), ("bright_cyan", 14), ("dim_cyan", 6),
    ("white", 7), ("bright_white", 15), ("dim_white", 7),
];

fn build_styles(my_colors: &Colors, term_colors: &Colors) -> (Style, Style, Colors, Colors) {
    let p = |key: &str, fallback: &str| pick(my_colors, key, fallback);
    let al = adjust_lightness;
    let is_dark = luminance(&p("surface", "#000000")) < 128.0;

    let (dk_surface, dk_surface_low, dk_surface_std, dk_surface_high, dk_surface_highest);
    let (dk_on_surface, dk_on_surface_variant, dk_outline, dk_outline_variant);
    let (lt_surface, lt_surface_low, lt_surface_std, lt_surface_high, lt_surface_highest);
    let (lt_on_surface, lt_on_surface_variant, lt_outline, lt_outline_variant);
    if is_dark {
        dk_surface = p("surface", "#1a1b26");
        dk_surface_low = p("surface_container_low", "#24283b");
        dk_surface_std = p("surface_container", "#414868");
        dk_surface_high = p("surface_container_high", "#565f89");
        dk_surface_highest = p("surface_container_highest", "#3d3231");
        dk_on_surface = p("on_surface", "#c0caf5");
        dk_on_surface_variant = p("on_surface_variant", "#9aa5ce");
        dk_outline = p("outline", "#565f89");
        dk_outline_variant = p("outline_variant", "#534341");
        lt_surface = p("inverse_surface", "#f1dedc");
        lt_on_surface = p("inverse_on_surface", "#392e2c");
        lt_on_surface_variant = p("outline_variant", "#534341");
        lt_outline = p("outline", "#565f89");
        lt_outline_variant = p("outline_variant", "#534341");
        lt_surface_low = al(&lt_surface, 0.97);
        lt_surface_std = al(&lt_surface, 0.94);
        lt_surface_high = al(&lt_surface, 0.91);
        lt_surface_highest = al(&lt_surface, 0.88);
    } else {
        lt_surface = p("surface", "#fff8f7");
        lt_surface_low = p("surface_container_low", "#fff0f2");
        lt_surface_std = p("surface_container", "#fbeaec");
        lt_surface_high = p("surface_container_high", "#f5e4e6");
        lt_surface_highest = p("surface_container_highest", "#efdee0");
        lt_on_surface = p("on_surface", "#22191b");
        lt_on_surface_variant = p("on_surface_variant", "#514346");
        lt_outline = p("outline", "#847376");
        lt_outline_variant = p("outline_variant", "#d6c2c4");
        dk_surface = p("inverse_surface", "#382e30");
        dk_on_surface = p("inverse_on_surface", "#feedef");
        dk_on_surface_variant = p("outline_variant", "#d6c2c4");
        dk_outline = p("outline", "#847376");
        dk_outline_variant = p("outline_variant", "#d6c2c4");
        dk_surface_low = al(&dk_surface, 1.15);
        dk_surface_std = al(&dk_surface, 1.35);
        dk_surface_high = al(&dk_surface, 1.55);
        dk_surface_highest = al(&dk_surface, 1.75);
    }

    let primary = p("primary", "#7aa2f7");
    let secondary = p("secondary", "#bb9af7");
    let tertiary = p("tertiary", "#9ece6a");
    let error_col = p("error", "#f7768e");

    let mut dark_style = style(vec![
        ("border", ha(&dk_on_surface, "20")),
        ("border.variant", ha(&dk_surface, "20")),
        ("border.focused", ha(&dk_surface, "40")),
        ("border.selected", ha(&dk_surface, "ff")),
        ("border.transparent", ha(&dk_surface, "20")),
        ("border.disabled", ha(&dk_outline_variant, "60")),
        ("elevated_surface.background", ha(&dk_surface_low, "ff")),
        ("surface.background", ha(&dk_surface_low, "ff")),
        ("background", ha(&dk_surface, "ff")),
        ("element.background", ha(&dk_surface_low, "ff")),
        ("element.hover", ha(&dk_surface_std, "ff")),
        ("element.active", ha(&dk_surface_high, "ff")),
        ("element.selected", ha(&dk_surface_high, "ff")),
        ("element.disabled", ha(&dk_surface_low, "ff")),
        ("drop_target.background", ha(&primary, "80")),
        ("ghost_element.background", J::from("#00000000")),
        ("ghost_element.hover", ha(&dk_surface_std, "ff")),
        ("ghost_element.active", ha(&dk_surface_high, "ff")),
        ("ghost_element.selected", ha(&dk_surface_high, "ff")),
        ("ghost_element.disabled", ha(&dk_surface_low, "ff")),
        ("text", ha(&dk_on_surface, "ff")),
        ("text.muted", ha(&dk_on_surface_variant, "ff")),
        ("text.placeholder", ha(&al(&dk_on_surface_variant, 0.7), "ff")),
        ("text.disabled", ha(&al(&dk_on_surface_variant, 0.6), "ff")),
        ("text.accent", ha(&primary, "ff")),
        ("icon", ha(&dk_on_surface, "ff")),
        ("icon.muted", ha(&dk_on_surface_variant, "ff")),
        ("icon.disabled", ha(&al(&dk_on_surface_variant, 0.6), "ff")),
        ("icon.placeholder", ha(&dk_on_surface_variant, "ff")),
        ("icon.accent", ha(&primary, "ff")),
        ("status_bar.background", ha(&dk_surface, "ff")),
        ("title_bar.background", ha(&dk_surface, "ff")),
        ("title_bar.inactive_background", ha(&dk_surface_low, "ff")),
        ("toolbar.background", ha(&dk_surface_low, "ff")),
        ("tab_bar.background", ha(&dk_surface_low, "ff")),
        ("tab.inactive_background", ha(&dk_surface_low, "ff")),
        ("tab.active_background", ha(&al(&dk_surface, 0.9), "ff")),
        ("search.match_background", ha(&primary, "66")),
        ("search.active_match_background", ha(&tertiary, "66")),
        ("panel.background", ha(&dk_surface_low, "ff")),
        ("panel.focused_border", J::Null),
        ("pane.focused_border", J::Null),
        ("scrollbar.thumb.background", ha(&dk_on_surface_variant, "4c")),
        ("scrollbar.thumb.hover_background", ha(&dk_surface_high, "ff")),
        ("scrollbar.thumb.border", ha(&dk_surface_std, "ff")),
        ("scrollbar.track.background", J::from("#00000000")),
        ("scrollbar.track.border", ha(&dk_surface_std, "ff")),
        ("editor.foreground", ha(&dk_on_surface, "ff")),
        ("editor.background", ha(&dk_surface, "ff")),
        ("editor.gutter.background", ha(&dk_surface, "ff")),
        ("editor.subheader.background", ha(&dk_surface_low, "ff")),
        ("editor.active_line.background", ha(&dk_surface_low, "bf")),
        ("editor.highlighted_line.background", ha(&dk_surface_std, "ff")),
        ("editor.line_number", ha(&dk_on_surface_variant, "ff")),
        ("editor.active_line_number", ha(&dk_on_surface, "ff")),
        ("editor.hover_line_number", ha(&al(&dk_on_surface, 1.1), "ff")),
        ("editor.invisible", ha(&dk_on_surface_variant, "ff")),
        ("editor.wrap_guide", ha(&dk_on_surface_variant, "0d")),
        ("editor.active_wrap_guide", ha(&dk_on_surface_variant, "1a")),
        ("editor.document_highlight.read_background", ha(&primary, "1a")),
        ("editor.document_highlight.write_background", ha(&dk_surface_std, "66")),
        ("terminal.background", ha(&dk_surface, "ff")),
        ("terminal.foreground", ha(&dk_on_surface, "ff")),
        ("terminal.bright_foreground", ha(&dk_on_surface, "ff")),
        ("terminal.dim_foreground", ha(&al(&dk_on_surface, 0.6), "ff")),
        ("link_text.hover", ha(&primary, "ff")),
        ("version_control.added", ha(&tertiary, "ff")),
        ("version_control.modified", ha(&al(&primary, 0.8), "ff")),
        ("version_control.word_added", ha(&tertiary, "59")),
        ("version_control.word_deleted", ha(&error_col, "cc")),
        ("version_control.deleted", ha(&error_col, "ff")),
        ("version_control.conflict_marker.ours", ha(&tertiary, "1a")),
        ("version_control.conflict_marker.theirs", ha(&primary, "1a")),
        ("conflict", ha(&al(&tertiary, 0.8), "ff")),
        ("conflict.background", ha(&al(&tertiary, 0.8), "1a")),
        ("conflict.border", ha(&al(&tertiary, 0.6), "ff")),
        ("created", ha(&tertiary, "ff")),
        ("created.background", ha(&tertiary, "1a")),
        ("created.border", ha(&al(&tertiary, 0.6), "ff")),
        ("deleted", ha(&error_col, "ff")),
        ("deleted.background", ha(&error_col, "1a")),
        ("deleted.border", ha(&al(&error_col, 0.6), "ff")),
        ("error", ha(&error_col, "ff")),
        ("error.background", ha(&error_col, "1a")),
        ("error.border", ha(&al(&error_col, 0.6), "ff")),
        ("hidden", ha(&dk_on_surface_variant, "ff")),
        ("hidden.background", ha(&al(&dk_on_surface_variant, 0.3), "1a")),
        ("hidden.border", ha(&dk_outline, "ff")),
        ("hint", ha(&al(&primary, 0.7), "ff")),
        ("hint.background", ha(&al(&primary, 0.7), "1a")),
        ("hint.border", ha(&al(&primary, 0.6), "ff")),
        ("ignored", ha(&dk_on_surface_variant, "ff")),
        ("ignored.background", ha(&al(&dk_on_surface_variant, 0.3), "1a")),
        ("ignored.border", ha(&dk_outline, "ff")),
        ("info", ha(&primary, "ff")),
        ("info.background", ha(&primary, "1a")),
        ("info.border", ha(&al(&primary, 0.6), "ff")),
        ("modified.background", ha(&al(&primary, 0.8), "1a")),
        ("modified.border", ha(&primary, "ff")),
        ("predictive", ha(&al(&secondary, 0.8), "ff")),
        ("predictive.background", ha(&al(&secondary, 0.8), "1a")),
        ("predictive.border", ha(&secondary, "ff")),
        ("renamed", ha(&primary, "ff")),
        ("renamed.background", ha(&primary, "1a")),
        ("renamed.border", ha(&al(&primary, 0.6), "ff")),
        ("success", ha(&tertiary, "ff")),
        ("success.background", ha(&tertiary, "1a")),
        ("success.border", ha(&al(&tertiary, 0.6), "ff")),
        ("unreachable", ha(&dk_on_surface_variant, "ff")),
        ("unreachable.background", ha(&al(&dk_on_surface_variant, 0.3), "1a")),
        ("unreachable.border", ha(&dk_outline, "ff")),
        ("warning", ha(&al(&tertiary, 0.9), "ff")),
        ("warning.background", ha(&al(&tertiary, 0.9), "1a")),
        ("warning.border", ha(&al(&tertiary, 0.9), "ff")),
    ]);

    dark_style.insert("terminal.ansi.black".into(), ha(&term_color(term_colors, 0), "ff"));
    dark_style.insert("terminal.ansi.bright_black".into(), ha(&term_color(term_colors, 8), "ff"));
    dark_style.insert("terminal.ansi.dim_black".into(), ha(&al(&term_color(term_colors, 0), 0.6), "ff"));
    for (name, idx) in ANSI {
        let base = term_color(term_colors, idx);
        let c = if name.contains("bright") {
            al(&base, 1.2)
        } else if name.contains("dim") {
            al(&base, 0.7)
        } else {
            base
        };
        dark_style.insert(format!("terminal.ansi.{name}"), ha(&c, "ff"));
    }
    let player_colors = [
        primary.clone(),
        error_col.clone(),
        al(&tertiary, 0.8),
        secondary.clone(),
        al(&secondary, 1.2),
        al(&error_col, 0.8),
        al(&tertiary, 0.9),
        al(&primary, 0.8),
    ];
    dark_style.insert("players".into(), players(&player_colors, |c| c.to_string()));
    dark_style.insert("syntax".into(), build_syntax_map(&primary, "dark"));

    let sat = |c: &str, factor: f64| saturate(c, factor, 0.38, 0.22, 0.0);

    let mut light_style = style(vec![
        ("border", ha(&lt_on_surface, "20")),
        ("border.variant", ha(&lt_surface, "20")),
        ("border.focused", ha(&lt_surface, "40")),
        ("border.selected", ha(&lt_surface, "ff")),
        ("border.transparent", ha(&lt_surface, "20")),
        ("border.disabled", ha(&lt_outline_variant, "60")),
        ("elevated_surface.background", ha(&lt_surface_low, "ff")),
        ("surface.background", ha(&lt_surface_low, "ff")),
        ("background", ha(&lt_surface, "ff")),
        ("element.background", ha(&lt_surface_std, "ff")),
        ("element.hover", ha(&lt_surface_high, "ff")),
        ("element.active", ha(&lt_surface_highest, "ff")),
        ("element.selected", ha(&lt_surface_highest, "ff")),
        ("element.disabled", ha(&lt_surface_low, "ff")),
        ("drop_target.background", ha(&primary, "30")),
        ("ghost_element.background", J::from("#00000000")),
        ("ghost_element.hover", ha(&lt_surface_high, "ff")),
        ("ghost_element.active", ha(&lt_surface_highest, "ff")),
        ("ghost_element.selected", ha(&lt_surface_highest, "ff")),
        ("ghost_element.disabled", ha(&lt_surface_low, "ff")),
        ("text", ha(&lt_on_surface, "ff")),
        ("text.muted", ha(&lt_on_surface_variant, "ff")),
        ("text.placeholder", ha(&al(&lt_on_surface_variant, 1.3), "ff")),
        ("text.disabled", ha(&al(&lt_on_surface_variant, 1.5), "ff")),
        ("text.accent", ha(&primary, "ff")),
        ("icon", ha(&lt_on_surface, "ff")),
        ("icon.muted", ha(&lt_on_surface_variant, "ff")),
        ("icon.disabled", ha(&al(&lt_on_surface_variant, 1.5), "ff")),
        ("icon.placeholder", ha(&lt_on_surface_variant, "ff")),
        ("icon.accent", ha(&primary, "ff")),
        ("status_bar.background", ha(&lt_surface, "ff")),
        ("title_bar.background", ha(&lt_surface, "ff")),
        ("title_bar.inactive_background", ha(&lt_surface_low, "ff")),
        ("toolbar.background", ha(&lt_surface_low, "ff")),
        ("tab_bar.background", ha(&lt_surface_low, "ff")),
        ("tab.inactive_background", ha(&lt_surface_low, "ff")),
        ("tab.active_background", ha(&lt_surface, "ff")),
        ("search.match_background", ha(&primary, "40")),
        ("search.active_match_background", ha(&tertiary, "40")),
        ("panel.background", ha(&lt_surface_low, "ff")),
        ("panel.focused_border", J::Null),
        ("pane.focused_border", J::Null),
        ("scrollbar.thumb.background", ha(&lt_on_surface_variant, "4c")),
        ("scrollbar.thumb.hover_background", ha(&lt_on_surface_variant, "80")),
        ("scrollbar.thumb.border", ha(&lt_on_surface_variant, "60")),
        ("scrollbar.track.background", J::from("#00000000")),
        ("scrollbar.track.border", ha(&lt_outline_variant, "ff")),
        ("editor.foreground", ha(&lt_on_surface, "ff")),
        ("editor.background", ha(&lt_surface, "ff")),
        ("editor.gutter.background", ha(&lt_surface, "ff")),
        ("editor.subheader.background", ha(&lt_surface_low, "ff")),
        ("editor.active_line.background", ha(&lt_surface_std, "bf")),
        ("editor.highlighted_line.background", ha(&lt_surface_high, "ff")),
        ("editor.line_number", ha(&lt_on_surface_variant, "ff")),
        ("editor.active_line_number", ha(&lt_on_surface, "ff")),
        ("editor.hover_line_number", ha(&al(&lt_on_surface, 0.8), "ff")),
        ("editor.invisible", ha(&al(&lt_on_surface_variant, 1.3), "ff")),
        ("editor.wrap_guide", ha(&lt_on_surface_variant, "0d")),
        ("editor.active_wrap_guide", ha(&lt_on_surface_variant, "1a")),
        ("editor.document_highlight.read_background", ha(&primary, "20")),
        ("editor.document_highlight.write_background", ha(&lt_on_surface_variant, "66")),
        ("terminal.background", ha(&lt_surface, "ff")),
        ("terminal.foreground", ha(&lt_on_surface, "ff")),
        ("terminal.bright_foreground", ha(&lt_on_surface, "ff")),
        ("terminal.dim_foreground", ha(&lt_on_surface_variant, "ff")),
        ("link_text.hover", ha(&primary, "ff")),
        ("version_control.added", ha(&sat(&tertiary, 1.3), "ff")),
        ("version_control.modified", ha(&sat(&primary, 1.3), "ff")),
        ("version_control.word_added", ha(&tertiary, "40")),
        ("version_control.word_deleted", ha(&error_col, "40")),
        ("version_control.deleted", ha(&sat(&error_col, 1.3), "ff")),
        ("version_control.conflict_marker.ours", ha(&tertiary, "25")),
        ("version_control.conflict_marker.theirs", ha(&primary, "25")),
        ("conflict", ha(&sat(&tertiary, 1.3), "ff")),
        ("conflict.background", ha(&tertiary, "18")),
        ("conflict.border", ha(&sat(&tertiary, 1.5), "ff")),
        ("created", ha(&sat(&tertiary, 1.3), "ff")),
        ("created.background", ha(&tertiary, "18")),
        ("created.border", ha(&sat(&tertiary, 1.5), "ff")),
        ("deleted", ha(&sat(&error_col, 1.3), "ff")),
        ("deleted.background", ha(&error_col, "18")),
        ("deleted.border", ha(&sat(&error_col, 1.5), "ff")),
        ("error", ha(&sat(&error_col, 1.3), "ff")),
        ("error.background", ha(&error_col, "18")),
        ("error.border", ha(&sat(&error_col, 1.5), "ff")),
        ("hidden", ha(&lt_on_surface_variant, "ff")),
        ("hidden.background", ha(&lt_on_surface_variant, "18")),
        ("hidden.border", ha(&lt_outline_variant, "ff")),
        ("hint", ha(&sat(&primary, 1.3), "ff")),
        ("hint.background", ha(&primary, "18")),
        ("hint.border", ha(&sat(&primary, 1.5), "ff")),
        ("ignored", ha(&lt_on_surface_variant, "ff")),
        ("ignored.background", ha(&lt_on_surface_variant, "18")),
        ("ignored.border", ha(&lt_outline_variant, "ff")),
        ("info", ha(&sat(&primary, 1.3), "ff")),
        ("info.background", ha(&primary, "18")),
        ("info.border", ha(&sat(&primary, 1.5), "ff")),
        ("modified", ha(&sat(&primary, 1.3), "ff")),
        ("modified.background", ha(&primary, "18")),
        ("modified.border", ha(&sat(&primary, 1.5), "ff")),
        ("predictive", ha(&sat(&secondary, 1.3), "ff")),
        ("predictive.background", ha(&secondary, "18")),
        ("predictive.border", ha(&sat(&secondary, 1.5), "ff")),
        ("renamed", ha(&sat(&primary, 1.3), "ff")),
        ("renamed.background", ha(&primary, "18")),
        ("renamed.border", ha(&sat(&primary, 1.5), "ff")),
        ("success", ha(&sat(&tertiary, 1.3), "ff")),
        ("success.background", ha(&tertiary, "18")),
        ("success.border", ha(&sat(&tertiary, 1.5), "ff")),
        ("unreachable", ha(&lt_on_surface_variant, "ff")),
        ("unreachable.background", ha(&lt_on_surface_variant, "18")),
        ("unreachable.border", ha(&lt_outline_variant, "ff")),
        ("warning", ha(&sat(&tertiary, 1.3), "ff")),
        ("warning.background", ha(&tertiary, "18")),
        ("warning.border", ha(&sat(&tertiary, 1.5), "ff")),
    ]);

    light_style.insert("terminal.ansi.black".into(), ha(&term_color(term_colors, 0), "ff"));
    light_style.insert("terminal.ansi.white".into(), ha(&al(&term_color(term_colors, 7), 0.5), "ff"));
    light_style.insert("terminal.ansi.bright_black".into(), ha(&al(&term_color(term_colors, 8), 0.6), "ff"));
    light_style.insert("terminal.ansi.bright_white".into(), ha(&al(&term_color(term_colors, 15), 0.3), "ff"));
    for (name, idx) in &ANSI[..18] {
        let base = term_color(term_colors, *idx);
        let c = if name.contains("bright") {
            al(&base, 0.75)
        } else if name.contains("dim") {
            al(&base, 0.5)
        } else {
            al(&base, 0.85)
        };
        light_style.insert(format!("terminal.ansi.{name}"), ha(&c, "ff"));
    }
    let player_colors_light = [sat(&primary, 1.3), sat(&error_col, 1.3), sat(&tertiary, 1.3), sat(&secondary, 1.3)];
    light_style.insert("players".into(), players(&player_colors_light, |c| al(c, 1.2)));
    light_style.insert("syntax".into(), build_syntax_map(&primary, "light"));

    let mode = |entries: [(&str, String); 11]| -> Colors {
        let mut m: Colors = entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        for (k, v) in my_colors {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
        m
    };
    let mode_dark = mode([
        ("surface", dk_surface.clone()),
        ("surface_container_low", dk_surface_low),
        ("surface_container", dk_surface_std.clone()),
        ("surface_container_high", dk_surface_high),
        ("surface_container_highest", dk_surface_highest),
        ("on_surface", dk_on_surface),
        ("on_surface_variant", dk_on_surface_variant),
        ("outline", dk_outline),
        ("outline_variant", dk_outline_variant),
        ("surface_dim", p("surface_dim", &al(&dk_surface, 0.92))),
        ("surface_variant", p("surface_variant", &dk_surface_std)),
    ]);
    let mode_light = mode([
        ("surface", lt_surface.clone()),
        ("surface_container_low", lt_surface_low),
        ("surface_container", lt_surface_std.clone()),
        ("surface_container_high", lt_surface_high),
        ("surface_container_highest", lt_surface_highest),
        ("on_surface", lt_on_surface),
        ("on_surface_variant", lt_on_surface_variant),
        ("outline", lt_outline),
        ("outline_variant", lt_outline_variant),
        ("surface_dim", p("surface_dim", &al(&lt_surface, 0.95))),
        ("surface_variant", p("surface_variant", &lt_surface_std)),
    ]);
    (dark_style, light_style, mode_dark, mode_light)
}

struct Palettes {
    dark: Colors,
    light: Colors,
}

fn resolve_material_color(token: &str, mode: &str, palettes: &Palettes) -> String {
    let palette = if mode == "light" { &palettes.light } else { &palettes.dark };
    if let Some(raw) = palette.get(token) {
        if is_hex_color(raw) {
            return normalize_hex6(raw);
        }
    }
    let t = |dark: &'static str, light: &'static str| if mode == "dark" { dark } else { light };
    let fallback = match token {
        "primary" => Some("#7aa2f7"),
        "secondary" => Some("#bb9af7"),
        "tertiary" => Some("#9ece6a"),
        "error" => Some("#f7768e"),
        "surface" => Some(t("#1a1b26", "#faf4f2")),
        "surface_container" => Some(t("#24283b", "#f0e8e6")),
        "surface_container_low" => Some(t("#1f2230", "#f7efed")),
        "surface_container_high" => Some(t("#2d3246", "#ece2df")),
        "surface_container_highest" => Some(t("#3a415b", "#e5d9d6")),
        "surface_dim" => Some(t("#14161d", "#ece2df")),
        "on_surface" => Some(t("#c0caf5", "#2a2022")),
        "on_surface_variant" => Some(t("#9aa5ce", "#5a4b4e")),
        "outline" => Some(t("#565f89", "#7e6e72")),
        "outline_variant" => Some(t("#434a68", "#ccb8bc")),
        "primary_container" => Some(t("#39426a", "#dbe2ff")),
        "on_primary_container" => Some(t("#d7e2ff", "#1f2a4d")),
        "secondary_container" => Some(t("#4a4064", "#e8defc")),
        "on_secondary_container" => Some(t("#e8defd", "#352d4b")),
        "tertiary_container" => Some(t("#334f2e", "#d9f4bf")),
        "on_tertiary_container" => Some(t("#d2f0b8", "#243a1e")),
        "error_container" => Some(t("#5d1f2d", "#f9d8df")),
        "on_error_container" => Some(t("#ffd9df", "#5b1b2a")),
        _ => None,
    };
    if let Some(v) = fallback {
        return v.to_string();
    }
    if token.starts_with("on_") {
        return resolve_material_color("on_surface", mode, palettes);
    }
    if token.ends_with("_container") {
        return resolve_material_color("surface_container", mode, palettes);
    }
    if token.starts_with("surface") {
        return resolve_material_color("surface", mode, palettes);
    }
    resolve_material_color("primary", mode, palettes)
}

/// Every `{{colors.<token>.<dark|light>.hex}}`, leftmost first, replaced by its colour.
fn render_str(s: &str, palettes: &Palettes) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(p) = rest.find("{{colors.") {
        let after = &rest[p + 9..];
        let n = after.bytes().take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_').count();
        let tail = &after[n..];
        let mode = ["dark", "light"].into_iter().find(|m| n > 0 && tail.starts_with(&format!(".{m}.hex}}}}")));
        match mode {
            Some(m) => {
                out.push_str(&rest[..p]);
                out.push_str(&resolve_material_color(&after[..n], m, palettes));
                rest = &tail[m.len() + 7..];
            }
            None => {
                out.push_str(&rest[..p + 1]);
                rest = &rest[p + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn render_template(node: &J, palettes: &Palettes) -> J {
    match node {
        J::Map(m) => J::Map(m.iter().map(|(k, v)| (k.clone(), render_template(v, palettes))).collect()),
        J::Arr(items) => J::Arr(items.iter().map(|v| render_template(v, palettes)).collect()),
        J::Str(s) => J::Str(render_str(s, palettes)),
        other => other.clone(),
    }
}

fn load_alt_styles(template_path: &str, palettes: &Palettes) -> BTreeMap<String, Style> {
    let mut styles = BTreeMap::new();
    if template_path.is_empty() {
        return styles;
    }
    let Ok(data) = read_file(template_path) else {
        return styles;
    };
    let (raw, err) = gojsonv::unmarshal_map(&data);
    if err.is_some() {
        return styles;
    }
    let Some(J::Arr(themes)) = raw.as_ref().and_then(|r| r.get("themes")) else {
        return styles;
    };
    for item in themes {
        let J::Map(obj) = item else { continue };
        let appearance = match obj.get("appearance") {
            Some(J::Str(a)) if a == "dark" || a == "light" => a.clone(),
            _ => continue,
        };
        let Some(style @ J::Map(_)) = obj.get("style") else { continue };
        if let J::Map(rendered) = render_template(style, palettes) {
            styles.insert(appearance, rendered);
        }
    }
    styles
}

// ── Main ────────────────────────────────────────────────────────────────

struct Flags {
    scss: String,
    colors: String,
    terminal_json: String,
    output: String,
    template: String,
}

/// The `flag` package's usage text for this program (flags sorted, a default shown unless empty).
fn usage(argv0: &str, d: &Flags) {
    eprintln!("Usage of {argv0}:");
    for (name, def) in [("colors", &d.colors), ("output", &d.output), ("scss", &d.scss), ("template", &d.template), ("terminal-json", &d.terminal_json)] {
        let default = if def.is_empty() { String::new() } else { format!(" (default {})", go_quote(def)) };
        eprintln!("  -{name} string\n    \t{default}");
    }
}

/// `flag.Parse()`: the flags and where the positional arguments start, or the exit code `ExitOnError` ends with.
fn parse_flags(argv0: &str, args: &[String], defaults: &Flags) -> Result<(Flags, usize), i32> {
    let mut f = Flags {
        scss: defaults.scss.clone(),
        colors: defaults.colors.clone(),
        terminal_json: defaults.terminal_json.clone(),
        output: defaults.output.clone(),
        template: defaults.template.clone(),
    };
    let fail = |msg: String| {
        eprintln!("{msg}");
        usage(argv0, defaults);
        2
    };
    let mut i = 0;
    while i < args.len() {
        let s = &args[i];
        if s.len() < 2 || !s.starts_with('-') {
            break;
        }
        let minuses = if s.as_bytes()[1] == b'-' {
            if s.len() == 2 {
                i += 1;
                break;
            }
            2
        } else {
            1
        };
        let mut name = &s[minuses..];
        if name.is_empty() || name.starts_with('-') || name.starts_with('=') {
            return Err(fail(format!("bad flag syntax: {s}")));
        }
        i += 1;
        let mut value = None;
        if let Some(eq) = name[1..].find('=') {
            value = Some(name[eq + 2..].to_string());
            name = &name[..eq + 1];
        }
        let slot = match name {
            "scss" => &mut f.scss,
            "colors" => &mut f.colors,
            "terminal-json" => &mut f.terminal_json,
            "output" => &mut f.output,
            "template" => &mut f.template,
            "help" | "h" => {
                usage(argv0, defaults);
                return Err(0);
            }
            _ => return Err(fail(format!("flag provided but not defined: -{name}"))),
        };
        *slot = match value {
            Some(v) => v,
            None if i < args.len() => {
                i += 1;
                args[i - 1].clone()
            }
            None => return Err(fail(format!("flag needs an argument: -{name}"))),
        };
    }
    Ok((f, i))
}

pub fn run_cli(argv0: &str, args: &[String]) -> i32 {
    let home = home();
    let generated = join(&[&home, ".local/state/quickshell/user/generated"]);
    let mut template = join(&[".", "dots/.config/matugen/templates/zed-colors.json"]);
    if fs::metadata(&template).is_err() {
        template = String::new();
    }
    let defaults = Flags {
        scss: join(&[&generated, "material_colors.scss"]),
        colors: join(&[&generated, "palette.json"]),
        terminal_json: join(&[&generated, "terminal.json"]),
        output: join(&[&home, ".config/zed/themes/ii-theme.json"]),
        template,
    };
    let (mut flags, rest) = match parse_flags(argv0, args, &defaults) {
        Ok(f) => f,
        Err(code) => return code,
    };
    // Backward-compatible positional args: <scss> <colors> <terminal-json>
    let positional = &args[rest..];
    for (slot, arg) in [&mut flags.scss, &mut flags.colors, &mut flags.terminal_json].into_iter().zip(positional) {
        *slot = arg.clone();
    }

    let my_colors_raw = read_string_json(&flags.colors);
    let mut my_colors = sanitize_hex_map(&my_colors_raw);
    if my_colors.is_empty() {
        my_colors = [
            ("primary", "#7aa2f7"),
            ("secondary", "#bb9af7"),
            ("tertiary", "#9ece6a"),
            ("error", "#f7768e"),
            ("surface", "#1a1b26"),
            ("surface_container_low", "#24283b"),
            ("surface_container", "#414868"),
            ("surface_container_high", "#565f89"),
            ("outline", "#565f89"),
            ("on_surface", "#c0caf5"),
            ("on_surface_variant", "#9aa5ce"),
            ("on_primary", "#1a1b26"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    }

    let scss_colors = gotheme::parse_scss(&flags.scss).unwrap_or_default();
    let term_from_scss = parse_term_map(&scss_colors);
    let term_from_palette = parse_term_map(&my_colors_raw);
    let term_from_json = parse_term_map(&read_string_json(&flags.terminal_json));
    let term_colors = gotheme::merge(&term_from_palette, &[&term_from_scss, &term_from_json]);

    let (mut dark_style, mut light_style, mode_dark, mode_light) = build_styles(&my_colors, &term_colors);
    let palettes = Palettes { dark: mode_dark, light: mode_light };

    let mut alt = load_alt_styles(&flags.template, &palettes);
    let mut alt_dark = alt.remove("dark").unwrap_or_else(|| dark_style.clone());
    let mut alt_light = alt.remove("light").unwrap_or_else(|| light_style.clone());

    // Keep default iNiR border tones aligned with the iNiR-alt border palette.
    apply_border_palette(&mut dark_style, &alt_dark);
    apply_border_palette(&mut light_style, &alt_light);

    alt_dark.insert("syntax".into(), build_syntax_map(&resolve_material_color("primary", "dark", &palettes), "dark"));
    alt_light.insert("syntax".into(), build_syntax_map(&resolve_material_color("primary", "light", &palettes), "light"));

    let theme = |name: &str, appearance: &str, s: Style| J::Map(style(vec![("name", J::from(name)), ("appearance", J::from(appearance)), ("style", J::Map(s))]));
    let theme_data = J::Map(style(vec![
        ("$schema", J::from("https://zed.dev/schema/themes/v0.2.0.json")),
        ("name", J::from("iNiR Material")),
        ("author", J::from("iNiR Theme System")),
        (
            "themes",
            J::Arr(vec![
                theme("iNiR Dark", "dark", dark_style.clone()),
                theme("iNiR Light", "light", light_style.clone()),
                theme("iNiR Borderless Dark", "dark", make_borderless_style(&dark_style)),
                theme("iNiR Borderless Light", "light", make_borderless_style(&light_style)),
                theme("iNiR-alt Dark", "dark", alt_dark),
                theme("iNiR-alt Light", "light", alt_light),
            ]),
        ),
    ]));

    if let Err(e) = mkdir_all(&dir_of(&flags.output)) {
        eprintln!("{e}");
        return 1;
    }
    let data = marshal_indent(&theme_data, "", "  ") + "\n";
    if let Err(e) = write_file(&flags.output, data.as_bytes()) {
        eprintln!("{e}");
        return 1;
    }
    println!("✓ Generated Zed theme");
    0
}
