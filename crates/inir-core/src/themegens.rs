// This code was written by: Hikari.
//! Native Rust implementations of iNiR terminal & editor theme generators:
//! - `scripts/colors/generate_terminal_configs.py` (Kitty, Alacritty, Foot, WezTerm, Ghostty, Konsole, Starship, OMP, btop, lazygit, yazi, fuzzel)
//! - `inir-opencode-themegen` (`scripts/colors/opencode_themegen`)
//! - `inir-system24-themegen` (`scripts/colors/system24_palette.py`)
//! - `inir-zed-themegen` (`scripts/colors/zed_themegen`)
//! - `inir-vscode-themegen` (`scripts/colors/vscode_themegen`)

use crate::washi;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Shared Color & File Utilities
// ---------------------------------------------------------------------------

pub fn parse_scss_colors(path: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return out;
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('$') {
            if let Some((name, val_semi)) = rest.split_once(':') {
                let val = val_semi.trim().trim_end_matches(';').trim();
                if (val.starts_with('#') && val.len() == 7) || val == "True" || val == "False" {
                    out.insert(name.trim().to_string(), val.to_string());
                }
            }
        }
    }
    out
}

pub fn load_json_colors(path: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return out;
    };
    let Ok(val) = serde_json::from_str::<Value>(&text) else {
        return out;
    };
    if let Some(obj) = val.as_object() {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                out.insert(k.clone(), s.to_string());
            }
        }
    }
    out
}

pub fn blend_hex(c1: &str, c2: &str, factor: f64) -> String {
    let f = factor.clamp(0.0, 1.0);
    let rgb1 = washi::parse_hex(c1).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    let rgb2 = washi::parse_hex(c2).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    let r = ((rgb1.r + (rgb2.r - rgb1.r) * f) * 255.0) as u8;
    let g = ((rgb1.g + (rgb2.g - rgb1.g) * f) * 255.0) as u8;
    let b = ((rgb1.b + (rgb2.b - rgb1.b) * f) * 255.0) as u8;
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

pub fn blend_hex_lower(c1: &str, c2: &str, factor: f64) -> String {
    let f = factor.clamp(0.0, 1.0);
    let rgb1 = washi::parse_hex(c1).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    let rgb2 = washi::parse_hex(c2).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    let r = ((rgb1.r * (1.0 - f) + rgb2.r * f) * 255.0 + 0.5).floor() as u8;
    let g = ((rgb1.g * (1.0 - f) + rgb2.g * f) * 255.0 + 0.5).floor() as u8;
    let b = ((rgb1.b * (1.0 - f) + rgb2.b * f) * 255.0 + 0.5).floor() as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

pub fn contrast_hex(a: &str, b: &str) -> f64 {
    let ra = washi::parse_hex(a).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    let rb = washi::parse_hex(b).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    washi::contrast(ra, rb)
}

fn pick<'a>(colors: &'a BTreeMap<String, String>, key: &str, fallback: &'a str) -> &'a str {
    colors
        .get(key)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
}

fn load_generator_colors(
    scss_path: &str,
    palette_path: &str,
    terminal_path: &str,
) -> BTreeMap<String, String> {
    let mut colors = if !scss_path.is_empty() {
        parse_scss_colors(scss_path)
    } else {
        BTreeMap::new()
    };
    for (k, v) in load_json_colors(palette_path) {
        colors.insert(k, v);
    }
    for (k, v) in load_json_colors(terminal_path) {
        colors.insert(k, v);
    }
    let term0 = pick(&colors, "term0", "#282828").to_string();
    let term15 = pick(&colors, "term15", "#EBDBB2").to_string();
    let primary = pick(&colors, "primary", "#458588").to_string();
    let mut factor = 0.35;
    while factor > 0.05 && contrast_hex(&term15, &blend_hex(&term0, &primary, factor)) < 4.5 {
        factor -= 0.03;
    }
    colors
        .entry("selectionBg".to_string())
        .or_insert_with(|| blend_hex(&term0, &primary, factor));
    colors
        .entry("tabInactiveBg".to_string())
        .or_insert_with(|| blend_hex(&term0, &term15, 0.06));
    colors
}

fn ensure_line_in_file(filepath: &Path, line_to_add: &str, check_substr: &str, at_top: bool) -> bool {
    if let Some(p) = filepath.parent() {
        let _ = fs::create_dir_all(p);
    }
    let mut content = fs::read_to_string(filepath).unwrap_or_default();
    if content.contains(check_substr) || content.contains(line_to_add) {
        return false;
    }
    if at_top {
        content = format!("{line_to_add}\n{content}");
    } else {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(line_to_add);
        content.push('\n');
    }
    let _ = fs::write(filepath, content);
    true
}

// ---------------------------------------------------------------------------
// 1. Terminal & TUI Config Generator (`generate_terminal_configs.py`)
// ---------------------------------------------------------------------------

pub fn run_generate_terminal_configs_cli(args: &[String]) -> Result<(), String> {
    let home = env::var("HOME").unwrap_or_default();
    let mut scss_path = format!("{home}/.local/state/quickshell/user/generated/material_colors.scss");
    let mut colors_path = format!("{home}/.local/state/quickshell/user/generated/palette.json");
    let mut terminal_json_path = format!("{home}/.local/state/quickshell/user/generated/terminal.json");
    let mut terminals: Option<Vec<String>> = None;
    let mut gen_zed = false;
    let mut gen_vscode = false;
    let mut vscode_forks: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--scss" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    scss_path = v.clone();
                }
            }
            "--colors" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    colors_path = v.clone();
                }
            }
            "--terminal-json" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    terminal_json_path = v.clone();
                }
            }
            "--terminals" => {
                let mut list = Vec::new();
                while i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    i += 1;
                    list.push(args[i].clone());
                }
                terminals = Some(list);
            }
            "--zed" => gen_zed = true,
            "--vscode" => gen_vscode = true,
            "--vscode-forks" => {
                while i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    i += 1;
                    vscode_forks.push(args[i].clone());
                }
            }
            _ => {}
        }
        i += 1;
    }

    let colors = load_generator_colors(&scss_path, &colors_path, &terminal_json_path);
    if colors.is_empty() {
        return Err("Error: No colors found in SCSS/JSON files".to_string());
    }

    let all_terminals: Vec<String> = [
        "kitty", "alacritty", "foot", "wezterm", "ghostty", "konsole", "starship", "omp", "btop",
        "lazygit", "yazi", "fuzzel",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let active_terminals = match terminals {
        None => {
            if gen_zed || gen_vscode {
                Vec::new()
            } else {
                all_terminals
            }
        }
        Some(list) => {
            if list.iter().any(|s| s == "all") {
                all_terminals
            } else {
                list
            }
        }
    };

    for t in &active_terminals {
        match t.as_str() {
            "kitty" => generate_kitty(&colors, &format!("{home}/.config/kitty/current-theme.conf"), &home),
            "alacritty" => generate_alacritty(&colors, &format!("{home}/.config/alacritty/colors.toml")),
            "foot" => generate_foot(&colors, &format!("{home}/.config/foot/inir-colors.ini"), &home),
            "wezterm" => generate_wezterm(&colors, &format!("{home}/.config/wezterm/colors.lua")),
            "ghostty" => generate_ghostty(&colors, &format!("{home}/.config/ghostty/themes/ii-auto"), &home),
            "konsole" => generate_konsole(&colors, &format!("{home}/.local/share/konsole/ii-auto.colorscheme")),
            "starship" => generate_starship(&colors, &format!("{home}/.config/starship/ii-palette.toml"), &home),
            "omp" => generate_omp(&colors, &format!("{home}/.config/oh-my-posh/ii-auto.json")),
            "btop" => generate_btop(&colors, &format!("{home}/.config/btop/themes/ii-auto.theme"), &home),
            "lazygit" => generate_lazygit(&colors, &format!("{home}/.config/lazygit/ii-theme.yml")),
            "yazi" => generate_yazi(&colors, &format!("{home}/.config/yazi/flavors/ii-auto.yazi/flavor.toml")),
            "fuzzel" => generate_fuzzel(&colors, &format!("{home}/.config/fuzzel/fuzzel_theme.ini")),
            _ => {}
        }
    }

    if gen_zed {
        let _ = run_zed_themegen_cli(&[
            "--scss".to_string(),
            scss_path.clone(),
            "--colors".to_string(),
            colors_path.clone(),
            "--terminal-json".to_string(),
            terminal_json_path.clone(),
        ]);
    }
    if gen_vscode {
        let mut vs_args = vec![
            "--colors".to_string(),
            colors_path,
            "--terminal-json".to_string(),
            terminal_json_path,
            "--scss".to_string(),
            scss_path,
        ];
        for fk in vscode_forks {
            vs_args.push("-forks".to_string());
            vs_args.push(fk);
        }
        let _ = run_vscode_themegen_cli(&vs_args);
    }

    Ok(())
}

fn generate_kitty(colors: &BTreeMap<String, String>, output_path: &str, home: &str) {
    let t0 = pick(colors, "term0", "#282828");
    let t7 = pick(colors, "term7", "#A89984");
    let t15 = pick(colors, "term15", "#EBDBB2");
    let tab_inact_bg = pick(colors, "tabInactiveBg", "#282828");
    let inact_fg = if contrast_hex(t7, tab_inact_bg) >= 4.5 {
        t7
    } else {
        t15
    };

    let cfg = format!(
        "# Auto-generated by ii wallpaper theming system\n\
         # Do not edit manually - changes will be overwritten\n\n\
         # The basic colors\n\
         foreground              {}\n\
         background              {}\n\
         selection_foreground    {}\n\
         selection_background    {}\n\n\
         # Cursor colors\n\
         cursor                  {}\n\
         cursor_text_color       {}\n\n\
         # URL underline color when hovering with mouse\n\
         url_color               {}\n\n\
         # Kitty window border colors\n\
         active_border_color     {}\n\
         inactive_border_color   {}\n\
         bell_border_color       {}\n\n\
         # Tab bar colors\n\
         active_tab_foreground   {}\n\
         active_tab_background   {}\n\
         inactive_tab_foreground {}\n\
         inactive_tab_background {}\n\
         tab_bar_background      {}\n\n\
         # The 16 terminal colors\n\n\
         # black\ncolor0 {}\ncolor8 {}\n\n\
         # red\ncolor1 {}\ncolor9 {}\n\n\
         # green\ncolor2  {}\ncolor10 {}\n\n\
         # yellow\ncolor3  {}\ncolor11 {}\n\n\
         # blue\ncolor4  {}\ncolor12 {}\n\n\
         # magenta\ncolor5  {}\ncolor13 {}\n\n\
         # cyan\ncolor6  {}\ncolor14 {}\n\n\
         # white\ncolor7  {}\ncolor15 {}\n",
        t15,
        t0,
        t15,
        pick(colors, "selectionBg", "#458588"),
        t15,
        t0,
        pick(colors, "secondary", pick(colors, "term3", "#C4A66F")),
        pick(colors, "primary", "#458588"),
        pick(colors, "term8", "#928374"),
        pick(colors, "term1", "#CC241D"),
        pick(colors, "onPrimary", pick(colors, "on_primary", "#FFFFFF")),
        pick(colors, "primary", "#458588"),
        inact_fg,
        tab_inact_bg,
        t0,
        t0,
        pick(colors, "term8", "#928374"),
        pick(colors, "term1", "#CC241D"),
        pick(colors, "term9", "#FB4934"),
        pick(colors, "term2", "#98971A"),
        pick(colors, "term10", "#B8BB26"),
        pick(colors, "term3", "#D79921"),
        pick(colors, "term11", "#FABD2F"),
        pick(colors, "term4", "#458588"),
        pick(colors, "term12", "#83A598"),
        pick(colors, "term5", "#B16286"),
        pick(colors, "term13", "#D3869B"),
        pick(colors, "term6", "#689D6A"),
        pick(colors, "term14", "#8EC07C"),
        t7,
        t15,
    );

    let out = Path::new(output_path);
    if let Some(dir) = out.parent() {
        let _ = fs::create_dir_all(dir);
        let theme_conf = dir.join("theme.conf");
        let _ = fs::write(&theme_conf, cfg);
        let tmp_link = format!("{output_path}.{}.tmp", std::process::id());
        let _ = fs::remove_file(&tmp_link);
        if symlink("theme.conf", &tmp_link).is_ok() {
            let _ = fs::rename(&tmp_link, out);
        }
    }
    let kitty_conf = PathBuf::from(format!("{home}/.config/kitty/kitty.conf"));
    ensure_line_in_file(&kitty_conf, "include current-theme.conf", "current-theme.conf", false);
    println!("✓ Generated Kitty config");
    let _ = Command::new("pkill")
        .args(["--signal", "SIGUSR1", "-x", "kitty"])
        .output();
}

fn generate_alacritty(colors: &BTreeMap<String, String>, output_path: &str) {
    let cfg = format!(
        "# Auto-generated by ii wallpaper theming system\n\
         # Do not edit manually - changes will be overwritten\n\n\
         [colors.primary]\n\
         background = '{}'\n\
         foreground = '{}'\n\n\
         [colors.cursor]\n\
         text   = '{}'\n\
         cursor = '{}'\n\n\
         [colors.selection]\n\
         text       = '{}'\n\
         background = '{}'\n\n\
         [colors.normal]\n\
         black   = '{}'\n\
         red     = '{}'\n\
         green   = '{}'\n\
         yellow  = '{}'\n\
         blue    = '{}'\n\
         magenta = '{}'\n\
         cyan    = '{}'\n\
         white   = '{}'\n\n\
         [colors.bright]\n\
         black   = '{}'\n\
         red     = '{}'\n\
         green   = '{}'\n\
         yellow  = '{}'\n\
         blue    = '{}'\n\
         magenta = '{}'\n\
         cyan    = '{}'\n\
         white   = '{}'\n",
        pick(colors, "term0", "#282828"),
        pick(colors, "term15", "#EBDBB2"),
        pick(colors, "term0", "#282828"),
        pick(colors, "term15", "#EBDBB2"),
        pick(colors, "term15", "#EBDBB2"),
        pick(colors, "selectionBg", "#458588"),
        pick(colors, "term0", "#282828"),
        pick(colors, "term1", "#CC241D"),
        pick(colors, "term2", "#98971A"),
        pick(colors, "term3", "#D79921"),
        pick(colors, "term4", "#458588"),
        pick(colors, "term5", "#B16286"),
        pick(colors, "term6", "#689D6A"),
        pick(colors, "term7", "#A89984"),
        pick(colors, "term8", "#928374"),
        pick(colors, "term9", "#FB4934"),
        pick(colors, "term10", "#B8BB26"),
        pick(colors, "term11", "#FABD2F"),
        pick(colors, "term12", "#83A598"),
        pick(colors, "term13", "#D3869B"),
        pick(colors, "term14", "#8EC07C"),
        pick(colors, "term15", "#EBDBB2"),
    );
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(output_path, cfg);
    println!("✓ Generated Alacritty config");
}

fn generate_foot(colors: &BTreeMap<String, String>, output_path: &str, home: &str) {
    let strip = |k: &str, fb: &str| pick(colors, k, fb).trim_start_matches('#').to_string();
    let cfg = format!(
        "# Auto-generated by ii wallpaper theming system\n\
         # Do not edit manually - changes will be overwritten\n\n\
         [colors-dark]\n\
         foreground={}\n\
         background={}\n\n\
         ## Normal/regular colors (color palette 0-7)\n\
         regular0={}  # black\n\
         regular1={}  # red\n\
         regular2={}  # green\n\
         regular3={}  # yellow\n\
         regular4={}  # blue\n\
         regular5={}  # magenta\n\
         regular6={}  # cyan\n\
         regular7={}  # white\n\n\
         ## Bright colors (color palette 8-15)\n\
         bright0={}   # bright black\n\
         bright1={}   # bright red\n\
         bright2={}  # bright green\n\
         bright3={}  # bright yellow\n\
         bright4={}  # bright blue\n\
         bright5={}  # bright magenta\n\
         bright6={}  # bright cyan\n\
         bright7={}  # bright white\n\n\
         ## Cursor and selection colors\n\
         selection-foreground={}\n\
         selection-background={}\n\
         jump-labels={} {}\n\
         urls={}\n",
        strip("term15", "#EBDBB2"),
        strip("term0", "#282828"),
        strip("term0", "#282828"),
        strip("term1", "#CC241D"),
        strip("term2", "#98971A"),
        strip("term3", "#D79921"),
        strip("term4", "#458588"),
        strip("term5", "#B16286"),
        strip("term6", "#689D6A"),
        strip("term7", "#A89984"),
        strip("term8", "#928374"),
        strip("term9", "#FB4934"),
        strip("term10", "#B8BB26"),
        strip("term11", "#FABD2F"),
        strip("term12", "#83A598"),
        strip("term13", "#D3869B"),
        strip("term14", "#8EC07C"),
        strip("term15", "#EBDBB2"),
        strip("term15", "#EBDBB2"),
        strip("selectionBg", "#458588"),
        strip("term0", "#282828"),
        strip("term3", "#D79921"),
        strip("term4", "#458588"),
    );
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(output_path, cfg);
    let foot_ini = PathBuf::from(format!("{home}/.config/foot/foot.ini"));
    ensure_line_in_file(&foot_ini, "include=~/.config/foot/inir-colors.ini", "inir-colors.ini", true);
    println!("✓ Generated Foot config");
}

fn generate_wezterm(colors: &BTreeMap<String, String>, output_path: &str) {
    let cfg = format!(
        "-- Auto-generated by ii wallpaper theming system\n\
         -- Do not edit manually - changes will be overwritten\n\n\
         return {{\n\
           foreground = '{}',\n\
           background = '{}',\n\
           cursor_bg = '{}',\n\
           cursor_fg = '{}',\n\
           cursor_border = '{}',\n\
           selection_fg = '{}',\n\
           selection_bg = '{}',\n\
           scrollbar_thumb = '{}',\n\
           split = '{}',\n\
           ansi = {{\n\
             '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}',\n\
           }},\n\
           brights = {{\n\
             '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}',\n\
           }},\n\
         }}\n",
        pick(colors, "term15", "#EBDBB2"),
        pick(colors, "term0", "#282828"),
        pick(colors, "term15", "#EBDBB2"),
        pick(colors, "term0", "#282828"),
        pick(colors, "term15", "#EBDBB2"),
        pick(colors, "term15", "#EBDBB2"),
        pick(colors, "selectionBg", "#458588"),
        pick(colors, "term8", "#928374"),
        pick(colors, "term8", "#928374"),
        pick(colors, "term0", "#282828"),
        pick(colors, "term1", "#CC241D"),
        pick(colors, "term2", "#98971A"),
        pick(colors, "term3", "#D79921"),
        pick(colors, "term4", "#458588"),
        pick(colors, "term5", "#B16286"),
        pick(colors, "term6", "#689D6A"),
        pick(colors, "term7", "#A89984"),
        pick(colors, "term8", "#928374"),
        pick(colors, "term9", "#FB4934"),
        pick(colors, "term10", "#B8BB26"),
        pick(colors, "term11", "#FABD2F"),
        pick(colors, "term12", "#83A598"),
        pick(colors, "term13", "#D3869B"),
        pick(colors, "term14", "#8EC07C"),
        pick(colors, "term15", "#EBDBB2"),
    );
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(output_path, cfg);
    println!("✓ Generated WezTerm config");
}

fn generate_ghostty(colors: &BTreeMap<String, String>, output_path: &str, home: &str) {
    let mut cfg = format!(
        "# Auto-generated by ii wallpaper theming system\n\
         background = {}\n\
         foreground = {}\n\
         cursor-color = {}\n\
         cursor-text = {}\n\
         selection-background = {}\n\
         selection-foreground = {}\n",
        pick(colors, "term0", "#282828"),
        pick(colors, "term15", "#EBDBB2"),
        pick(colors, "term15", "#EBDBB2"),
        pick(colors, "term0", "#282828"),
        pick(colors, "selectionBg", "#458588"),
        pick(colors, "term15", "#EBDBB2"),
    );
    for idx in 0..16 {
        let c = pick(colors, &format!("term{idx}"), "#888888");
        cfg.push_str(&format!("palette = {idx}={c}\n"));
    }
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(output_path, cfg);
    let ghostty_conf = PathBuf::from(format!("{home}/.config/ghostty/config"));
    ensure_line_in_file(&ghostty_conf, "theme = ii-auto", "ii-auto", false);
    println!("✓ Generated Ghostty config");
}

fn generate_konsole(colors: &BTreeMap<String, String>, output_path: &str) {
    let strip = |k: &str, fb: &str| pick(colors, k, fb).trim_start_matches('#').to_string();
    let mut cfg = format!(
        "[Background]\nColor={}\n\n[BackgroundIntense]\nColor={}\n\n[Foreground]\nColor={}\n\n[ForegroundIntense]\nColor={}\n\n",
        strip("term0", "#282828"),
        strip("term0", "#282828"),
        strip("term15", "#EBDBB2"),
        strip("term15", "#EBDBB2"),
    );
    for idx in 0..8 {
        let normal = strip(&format!("term{idx}"), "#888888");
        let bright = strip(&format!("term{}", idx + 8), "#888888");
        cfg.push_str(&format!(
            "[Color{idx}]\nColor={normal}\n\n[Color{idx}Intense]\nColor={bright}\n\n"
        ));
    }
    cfg.push_str("[General]\nDescription=ii Auto-generated Theme\nOpacity=1.0\nWallpaper=\n");
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(output_path, cfg);
    println!("✓ Generated Konsole config");
}

fn generate_starship(colors: &BTreeMap<String, String>, output_path: &str, home: &str) {
    let bg = pick(colors, "term0", "#282828");
    let red = blend_hex(&blend_hex(pick(colors, "term1", "#CC241D"), pick(colors, "secondary", "#83A598"), 0.45), bg, 0.20);
    let green = blend_hex(&blend_hex(pick(colors, "term2", "#98971A"), pick(colors, "primary", "#458588"), 0.45), bg, 0.20);
    let yellow = blend_hex(&blend_hex(pick(colors, "term3", "#D79921"), pick(colors, "tertiary", "#D3869B"), 0.45), bg, 0.20);
    let blue = blend_hex(&blend_hex(pick(colors, "term4", "#458588"), pick(colors, "primary", "#458588"), 0.45), bg, 0.20);
    let magenta = blend_hex(&blend_hex(pick(colors, "term5", "#B16286"), pick(colors, "secondary", "#83A598"), 0.45), bg, 0.20);
    let cyan = blend_hex(&blend_hex(pick(colors, "term6", "#689D6A"), pick(colors, "tertiary", "#D3869B"), 0.45), bg, 0.20);

    let palette_block = format!(
        "[palettes.ii]\n\
         primary = '{}'\n\
         onPrimary = '{}'\n\
         secondary = '{}'\n\
         onSecondary = '{}'\n\
         tertiary = '{}'\n\
         onTertiary = '{}'\n\
         surface = '{}'\n\
         onSurface = '{}'\n\
         background = '{}'\n\
         foreground = '{}'\n\
         black = '{}'\n\
         red = '{}'\n\
         green = '{}'\n\
         yellow = '{}'\n\
         blue = '{}'\n\
         magenta = '{}'\n\
         cyan = '{}'\n\
         white = '{}'\n\
         bright_black = '{}'\n\
         bright_red = '{}'\n\
         bright_green = '{}'\n\
         bright_yellow = '{}'\n\
         bright_blue = '{}'\n\
         bright_magenta = '{}'\n\
         bright_cyan = '{}'\n\
         bright_white = '{}'\n",
        pick(colors, "primary", "#458588"),
        pick(colors, "onPrimary", pick(colors, "on_primary", "#FFFFFF")),
        pick(colors, "secondary", "#83A598"),
        pick(colors, "onSecondary", pick(colors, "on_secondary", "#1D2021")),
        pick(colors, "tertiary", "#D3869B"),
        pick(colors, "onTertiary", pick(colors, "on_tertiary", "#1D2021")),
        pick(colors, "surface", "#1D2021"),
        pick(colors, "term15", "#EBDBB2"),
        bg,
        pick(colors, "term15", "#EBDBB2"),
        bg,
        red,
        green,
        yellow,
        blue,
        magenta,
        cyan,
        pick(colors, "term7", "#A89984"),
        pick(colors, "term8", "#928374"),
        pick(colors, "term9", "#FB4934"),
        pick(colors, "term10", "#B8BB26"),
        pick(colors, "term11", "#FABD2F"),
        pick(colors, "term12", "#83A598"),
        pick(colors, "term13", "#D3869B"),
        pick(colors, "term14", "#8EC07C"),
        pick(colors, "term15", "#EBDBB2"),
    );

    let full_cfg = format!(
        "# Auto-generated by ii wallpaper theming system\n# Do not edit manually - changes will be overwritten\n\n{palette_block}"
    );
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(output_path, &full_cfg);

    let starship_toml = PathBuf::from(format!("{home}/.config/starship.toml"));
    if starship_toml.exists() {
        if let Ok(mut content) = fs::read_to_string(&starship_toml) {
            if !content.contains("palette =") {
                content = format!("palette = \"ii\"\n\n{content}");
            }
            if let Some(start) = content.find("[palettes.ii]") {
                let rest = &content[start + "[palettes.ii]".len()..];
                let end = rest
                    .find("\n[")
                    .map(|idx| start + "[palettes.ii]".len() + idx + 1)
                    .unwrap_or(content.len());
                content.replace_range(start..end, &palette_block);
            } else {
                content.push('\n');
                content.push_str(&full_cfg);
            }
            let _ = fs::write(&starship_toml, content);
        }
    }
    println!("✓ Generated Starship palette");
}

fn generate_omp(colors: &BTreeMap<String, String>, output_path: &str) {
    let t0 = pick(colors, "term0", "#282828");
    let t15 = pick(colors, "term15", "#EBDBB2");
    let sc = blend_hex(t0, t15, 0.08);
    let sch = blend_hex(t0, t15, 0.14);
    let osv = pick(colors, "term7", "#A89984");
    let primary = pick(colors, "primary", "#458588");
    let pc = pick(colors, "primary_container", pick(colors, "primaryContainer", "#33281d"));
    let opc = pick(colors, "on_primary_container", pick(colors, "onPrimaryContainer", "#eddccb"));
    let ec = pick(colors, "error_container", pick(colors, "errorContainer", "#93000a"));
    let oec = pick(colors, "on_error_container", pick(colors, "onErrorContainer", "#ffdad6"));

    let theme = json!({
        "$schema": "https://raw.githubusercontent.com/JanDeDobbeleer/oh-my-posh/main/themes/schema.json",
        "version": 4,
        "blocks": [
            {
                "type": "prompt",
                "alignment": "left",
                "segments": [
                    {
                        "type": "executiontime",
                        "style": "plain",
                        "foreground": osv,
                        "background": sch,
                        "template": " {{ .FormattedMs }} ",
                        "properties": {"style": "austin", "threshold": 500}
                    },
                    {
                        "type": "status",
                        "style": "plain",
                        "foreground": oec,
                        "background": ec,
                        "template": " {{ if gt .Code 0 }}✗ {{ .Code }}{{ end }} "
                    },
                    {
                        "type": "path",
                        "style": "plain",
                        "foreground": t15,
                        "background": sc,
                        "template": " {{ .Path }} ",
                        "properties": {"style": "full", "folder_separator_icon": "/"}
                    },
                    {
                        "type": "git",
                        "style": "plain",
                        "foreground": opc,
                        "background": pc,
                        "template": " {{ .HEAD }}{{ if .Working.Changed }} ●{{ end }}{{ if .Staging.Changed }} ✚{{ end }} ",
                        "properties": {"branch_icon": "", "fetch_status": true}
                    }
                ]
            },
            {
                "type": "rprompt",
                "segments": [
                    {
                        "type": "time",
                        "style": "plain",
                        "foreground": osv,
                        "template": " {{ .CurrentDate | date \"15:04\" }} "
                    }
                ]
            },
            {
                "type": "prompt",
                "alignment": "left",
                "newline": true,
                "segments": [
                    {
                        "type": "text",
                        "style": "plain",
                        "foreground": primary,
                        "template": "❯ "
                    }
                ]
            }
        ]
    });
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(
        output_path,
        serde_json::to_string_pretty(&theme).unwrap_or_default(),
    );
    println!("✓ Generated oh-my-posh theme");
}

fn generate_btop(colors: &BTreeMap<String, String>, output_path: &str, home: &str) {
    let bg = pick(colors, "term0", pick(colors, "surface", "#282828"));
    let fg = pick(colors, "term15", pick(colors, "on_surface", "#EBDBB2"));
    let surface_low = blend_hex(bg, fg, 0.06);
    let surface_high = blend_hex(bg, fg, 0.14);
    let osv = pick(colors, "term7", pick(colors, "on_surface_variant", "#C4A66F"));
    let raw_ov = pick(colors, "outline_variant", pick(colors, "outlineVariant", "#534341"));
    let ov = blend_hex(raw_ov, osv, 0.25);
    let primary = pick(colors, "primary", "#458588");
    let p_dim = pick(colors, "primary_fixed_dim", primary);
    let s_dim = pick(colors, "secondary_fixed_dim", pick(colors, "secondary", "#83A598"));
    let t_dim = pick(colors, "tertiary_fixed_dim", pick(colors, "tertiary", "#D3869B"));
    let error = pick(colors, "error", "#CC241D");

    let cfg = format!(
        "# Auto-generated by ii wallpaper theming system\n\
         # Do not edit manually\n\n\
         theme[main_bg]=\"{bg}\"\n\
         theme[main_fg]=\"{fg}\"\n\
         theme[title]=\"{fg}\"\n\
         theme[hi_fg]=\"{primary}\"\n\
         theme[selected_bg]=\"{surface_high}\"\n\
         theme[selected_fg]=\"{fg}\"\n\
         theme[inactive_fg]=\"{osv}\"\n\
         theme[graph_text]=\"{osv}\"\n\
         theme[meter_bg]=\"{surface_low}\"\n\
         theme[proc_misc]=\"{osv}\"\n\n\
         theme[cpu_box]=\"{ov}\"\n\
         theme[mem_box]=\"{ov}\"\n\
         theme[net_box]=\"{ov}\"\n\
         theme[proc_box]=\"{ov}\"\n\
         theme[div_line]=\"{ov}\"\n\n\
         theme[temp_start]=\"{t_dim}\"\n\
         theme[temp_mid]=\"{s_dim}\"\n\
         theme[temp_end]=\"{error}\"\n\n\
         theme[cpu_start]=\"{t_dim}\"\n\
         theme[cpu_mid]=\"{s_dim}\"\n\
         theme[cpu_end]=\"{error}\"\n\n\
         theme[free_start]=\"{t_dim}\"\n\
         theme[free_mid]=\"{t_dim}\"\n\
         theme[free_end]=\"{t_dim}\"\n\
         theme[cached_start]=\"{p_dim}\"\n\
         theme[cached_mid]=\"{p_dim}\"\n\
         theme[cached_end]=\"{p_dim}\"\n\
         theme[available_start]=\"{s_dim}\"\n\
         theme[available_mid]=\"{s_dim}\"\n\
         theme[available_end]=\"{s_dim}\"\n\
         theme[used_start]=\"{t_dim}\"\n\
         theme[used_mid]=\"{s_dim}\"\n\
         theme[used_end]=\"{error}\"\n\n\
         theme[download_start]=\"{t_dim}\"\n\
         theme[download_mid]=\"{t_dim}\"\n\
         theme[download_end]=\"{t_dim}\"\n\
         theme[upload_start]=\"{s_dim}\"\n\
         theme[upload_mid]=\"{s_dim}\"\n\
         theme[upload_end]=\"{s_dim}\"\n\n\
         theme[process_start]=\"{p_dim}\"\n\
         theme[process_mid]=\"{p_dim}\"\n\
         theme[process_end]=\"{p_dim}\"\n"
    );
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(output_path, cfg);
    let btop_conf = PathBuf::from(format!("{home}/.config/btop/btop.conf"));
    ensure_line_in_file(&btop_conf, "color_theme = \"ii-auto\"", "color_theme", false);
    println!("✓ Generated btop theme");
}

fn generate_lazygit(colors: &BTreeMap<String, String>, output_path: &str) {
    let primary = pick(colors, "primary", "#458588");
    let fg = pick(colors, "term15", "#EBDBB2");
    let gray = pick(colors, "term8", "#928374");
    let sel_bg = pick(colors, "surfaceContainer", pick(colors, "surface_container", gray));
    let red = pick(colors, "term1", "#CC241D");
    let yellow = pick(colors, "term3", "#D79921");

    let theme_yaml = format!(
        "    theme:\n\
         \x20     activeBorderColor:\n\
         \x20       - \"{primary}\"\n\
         \x20       - bold\n\
         \x20     inactiveBorderColor:\n\
         \x20       - \"{gray}\"\n\
         \x20     optionsTextColor:\n\
         \x20       - \"{primary}\"\n\
         \x20     selectedLineBgColor:\n\
         \x20       - \"{sel_bg}\"\n\
         \x20     selectedRangeBgColor:\n\
         \x20       - \"{sel_bg}\"\n\
         \x20     cherryPickedCommitFgColor:\n\
         \x20       - \"{primary}\"\n\
         \x20     cherryPickedCommitBgColor:\n\
         \x20       - \"{sel_bg}\"\n\
         \x20     markedBaseCommitFgColor:\n\
         \x20       - \"{yellow}\"\n\
         \x20     markedBaseCommitBgColor:\n\
         \x20       - \"{sel_bg}\"\n\
         \x20     unstagedChangesColor:\n\
         \x20       - \"{red}\"\n\
         \x20     defaultFgColor:\n\
         \x20       - \"{fg}\"\n"
    );
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(
        output_path,
        format!("# Auto-generated by ii wallpaper theming system\ngui:\n{theme_yaml}\n"),
    );
    println!("✓ Generated lazygit theme");
}

fn generate_yazi(colors: &BTreeMap<String, String>, output_path: &str) {
    let bg = pick(colors, "term0", "#282828");
    let fg = pick(colors, "term15", "#EBDBB2");
    let fg_dim = pick(colors, "term7", "#A89984");
    let gray = pick(colors, "term8", "#928374");
    let primary = pick(colors, "primary", "#458588");
    let sel_bg = pick(colors, "surfaceContainer", pick(colors, "surface_container", gray));
    let red = pick(colors, "term1", "#CC241D");
    let green = pick(colors, "term2", "#98971A");
    let yellow = pick(colors, "term3", "#D79921");
    let magenta = pick(colors, "term5", "#B16286");
    let cyan = pick(colors, "term6", "#689D6A");

    let cfg = format!(
        "# Auto-generated by ii wallpaper theming system\n\n\
         [mgr]\n\
         cwd = {{ fg = \"{primary}\" }}\n\
         hovered         = {{ fg = \"{bg}\", bg = \"{primary}\" }}\n\
         preview_hovered = {{ underline = true }}\n\
         find_keyword    = {{ fg = \"{yellow}\", italic = true }}\n\
         find_position   = {{ fg = \"{magenta}\", bg = \"reset\", italic = true }}\n\
         marker_selected = {{ fg = \"{green}\", bg = \"{green}\" }}\n\
         marker_copied   = {{ fg = \"{yellow}\", bg = \"{yellow}\" }}\n\
         marker_cut      = {{ fg = \"{red}\", bg = \"{red}\" }}\n\
         tab_active      = {{ fg = \"{bg}\", bg = \"{sel_bg}\" }}\n\
         tab_inactive    = {{ fg = \"{fg_dim}\", bg = \"{sel_bg}\" }}\n\
         tab_width       = 1\n\
         border_symbol   = \"│\"\n\
         border_style    = {{ fg = \"{gray}\" }}\n\
         count_copied    = {{ fg = \"{bg}\", bg = \"{yellow}\" }}\n\
         count_cut       = {{ fg = \"{bg}\", bg = \"{red}\" }}\n\
         count_selected  = {{ fg = \"{bg}\", bg = \"{primary}\" }}\n\n\
         [mode]\n\
         normal_main = {{ fg = \"{bg}\", bg = \"{primary}\", bold = true }}\n\
         normal_alt  = {{ fg = \"{bg}\", bg = \"{primary}\", bold = true }}\n\
         select_main = {{ fg = \"{bg}\", bg = \"{green}\", bold = true }}\n\
         select_alt  = {{ fg = \"{bg}\", bg = \"{green}\", bold = true }}\n\
         unset_main  = {{ fg = \"{bg}\", bg = \"{magenta}\", bold = true }}\n\
         unset_alt   = {{ fg = \"{bg}\", bg = \"{magenta}\", bold = true }}\n\n\
         [status]\n\
         separator_open  = \"\"\n\
         separator_close = \"\"\n\
         separator_style = {{ fg = \"{sel_bg}\", bg = \"{sel_bg}\" }}\n\
         progress_label  = {{ fg = \"{fg}\", bold = true }}\n\
         progress_normal = {{ fg = \"{primary}\", bg = \"{sel_bg}\" }}\n\
         progress_error  = {{ fg = \"{red}\", bg = \"{sel_bg}\" }}\n\
         permissions_t   = {{ fg = \"{primary}\" }}\n\
         permissions_r   = {{ fg = \"{yellow}\" }}\n\
         permissions_w   = {{ fg = \"{red}\" }}\n\
         permissions_x   = {{ fg = \"{green}\" }}\n\
         permissions_s   = {{ fg = \"{gray}\" }}\n"
    );
    if let Some(p) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(output_path, cfg);
    println!("✓ Generated yazi flavor");
}

fn generate_fuzzel(colors: &BTreeMap<String, String>, output_path: &str) {
    let rgba = |k: &str, fb: &str| -> String {
        let hex = colors
            .get(k)
            .or_else(|| colors.get(fb))
            .map(String::as_str)
            .unwrap_or("#808080")
            .trim_start_matches('#');
        format!("{}ff", &hex[..hex.len().min(6)].to_lowercase())
    };
    let fg = rgba("app_foreground", "on_surface");
    let sub = rgba("app_subtext", "on_surface_variant");
    let acc = rgba("app_accent", "primary");
    let cfg = format!(
        "[colors]\n\
         background={}\n\
         text={fg}\n\
         input={fg}\n\
         prompt={acc}\n\
         placeholder={sub}\n\
         message={sub}\n\
         counter={sub}\n\
         match={acc}\n\
         selection={}\n\
         selection-text={}\n\
         selection-match={acc}\n\
         border={}\n",
        rgba("app_popover_bg", "surface_container"),
        rgba("app_selection", "secondary_container"),
        rgba("app_on_selection", "on_secondary_container"),
        rgba("app_border_subtle", "outline_variant"),
    );
    let path = Path::new(output_path);
    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
        let _ = fs::write(path, cfg);
        if let Some(fname) = path.file_name().map(|f| f.to_string_lossy()) {
            ensure_line_in_file(
                &p.join("fuzzel.ini"),
                &format!("include=~/.config/fuzzel/{fname}"),
                &fname,
                true,
            );
        }
    }
    println!("✓ Generated Fuzzel theme");
}

// ---------------------------------------------------------------------------
// 2. OpenCode Theme Generator (`inir-opencode-themegen`)
// ---------------------------------------------------------------------------

pub fn run_opencode_themegen_cli(args: &[String]) -> Result<(), String> {
    let home = env::var("HOME").unwrap_or_default();
    let scss_path = args
        .first()
        .cloned()
        .unwrap_or_else(|| format!("{home}/.local/state/quickshell/user/generated/material_colors.scss"));
    let palette_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| format!("{home}/.local/state/quickshell/user/generated/palette.json"));
    let terminal_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| format!("{home}/.local/state/quickshell/user/generated/terminal.json"));
    let output_dir = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| format!("{home}/.config/opencode/themes"));

    let mut colors = parse_scss_colors(&scss_path);
    if colors.is_empty() && !Path::new(&scss_path).exists() {
        return Err(format!("Error: {scss_path} not found"));
    }
    for (k, v) in load_json_colors(&palette_path) {
        colors.insert(k, v);
    }
    for (k, v) in load_json_colors(&terminal_path) {
        colors.insert(k, v);
    }

    let primary = pick(&colors, "primary", "#7aa2f7");
    let on_primary = pick(&colors, "onPrimary", pick(&colors, "on_primary", "#1a1b26"));
    let primary_container = pick(&colors, "primaryContainer", pick(&colors, "primary_container", "#004c6b"));
    let on_primary_container = pick(&colors, "onPrimaryContainer", pick(&colors, "on_primary_container", "#c6e7ff"));
    let secondary = pick(&colors, "secondary", "#bb9af7");
    let secondary_container = pick(&colors, "secondaryContainer", pick(&colors, "secondary_container", "#394b58"));
    let tertiary = pick(&colors, "tertiary", "#9ece6a");
    let tertiary_container = pick(&colors, "tertiaryContainer", pick(&colors, "tertiary_container", "#958bb1"));
    let mut surface = pick(&colors, "surface", "#0f1417").to_string();
    let surface_dim = pick(&colors, "surfaceDim", pick(&colors, "surface_dim", "#0f1417"));
    let surface_bright = pick(&colors, "surfaceBright", pick(&colors, "surface_bright", "#353a3d"));
    let mut surface_container = pick(&colors, "surfaceContainer", pick(&colors, "surface_container", "#1c2024")).to_string();
    let surface_container_low = pick(&colors, "surfaceContainerLow", pick(&colors, "surface_container_low", "#181c1f"));
    let mut surface_container_high = pick(&colors, "surfaceContainerHigh", pick(&colors, "surface_container_high", "#262b2e")).to_string();
    let surface_container_highest = pick(&colors, "surfaceContainerHighest", pick(&colors, "surface_container_highest", "#313539"));
    let mut on_surface = pick(&colors, "onSurface", pick(&colors, "on_surface", "#dfe3e7")).to_string();
    let mut on_surface_variant = pick(&colors, "onSurfaceVariant", pick(&colors, "on_surface_variant", "#c1c7ce")).to_string();
    let outline = pick(&colors, "outline", "#8b9298");
    let outline_variant = pick(&colors, "outlineVariant", pick(&colors, "outline_variant", "#41484d"));
    let shadow = pick(&colors, "shadow", "#000000");
    let error_col = pick(&colors, "error", "#ffb4ab");
    let error_container = pick(&colors, "errorContainer", pick(&colors, "error_container", "#93000a"));
    let success = pick(&colors, "success", "#b5ccba");
    let success_container = pick(&colors, "successContainer", pick(&colors, "success_container", "#374b3e"));
    let inverse_surface = pick(&colors, "inverseSurface", pick(&colors, "inverse_surface", "#dfe3e7"));
    let inverse_on_surface = pick(&colors, "inverseOnSurface", pick(&colors, "inverse_on_surface", "#2c3135"));
    let inverse_primary = pick(&colors, "inversePrimary", pick(&colors, "inverse_primary", primary));

    let term: Vec<String> = (0..16)
        .map(|i| pick(&colors, &format!("term{i}"), "#888888").to_string())
        .collect();
    if colors.contains_key("term15") {
        on_surface = term[15].clone();
        on_surface_variant = term[7].clone();
        surface = term[0].clone();
        surface_container = blend_hex_lower(&term[0], &term[15], 0.05);
        surface_container_high = blend_hex_lower(&term[0], &term[15], 0.10);
    }

    let diff_added_bg = blend_hex_lower(&surface_container, success, 0.15);
    let diff_removed_bg = blend_hex_lower(&surface_container, error_col, 0.15);
    let diff_added_line_bg = blend_hex_lower(&surface_container, success, 0.10);
    let diff_removed_line_bg = blend_hex_lower(&surface_container, error_col, 0.10);
    let warning = &term[3];

    let light_primary = inverse_primary;
    let light_secondary = blend_hex_lower(secondary, inverse_surface, 0.25);
    let light_accent = blend_hex_lower(tertiary, inverse_surface, 0.25);
    let light_error = blend_hex_lower(error_col, inverse_surface, 0.35);
    let light_warning = blend_hex_lower(warning, inverse_surface, 0.40);
    let light_success = blend_hex_lower(success, inverse_surface, 0.35);
    let light_text_muted = blend_hex_lower(inverse_on_surface, inverse_surface, 0.35);
    let light_bg_panel = blend_hex_lower(inverse_surface, "#ffffff", 0.06);
    let light_bg_element = blend_hex_lower(inverse_surface, "#ffffff", 0.12);
    let light_bg_menu = blend_hex_lower(inverse_surface, "#ffffff", 0.08);
    let light_border = blend_hex_lower(inverse_on_surface, inverse_surface, 0.55);
    let light_border_subtle = blend_hex_lower(inverse_on_surface, inverse_surface, 0.70);
    let light_diff_added_bg = blend_hex_lower(inverse_surface, &light_success, 0.18);
    let light_diff_removed_bg = blend_hex_lower(inverse_surface, &light_error, 0.18);
    let light_diff_added_line_bg = blend_hex_lower(inverse_surface, &light_success, 0.12);
    let light_diff_removed_line_bg = blend_hex_lower(inverse_surface, &light_error, 0.12);
    let light_md_link = blend_hex_lower(tertiary, inverse_surface, 0.20);
    let light_md_code = blend_hex_lower(&term[2], inverse_surface, 0.20);
    let light_syn_kw = blend_hex_lower(&term[5], inverse_surface, 0.25);
    let light_syn_fn = blend_hex_lower(&term[4], inverse_surface, 0.25);
    let light_syn_var = blend_hex_lower(&term[6], inverse_surface, 0.20);
    let light_syn_str = blend_hex_lower(&term[2], inverse_surface, 0.25);
    let light_syn_num = blend_hex_lower(&term[13], inverse_surface, 0.25);
    let light_syn_type = blend_hex_lower(&term[3], inverse_surface, 0.25);

    let tv = |d: &str, l: &str| json!({ "dark": d, "light": l });

    let doc = json!({
        "$schema": "https://opencode.ai/theme.json",
        "defs": {
            "m3Primary": primary,
            "m3OnPrimary": on_primary,
            "m3PrimaryContainer": primary_container,
            "m3OnPrimaryContainer": on_primary_container,
            "m3Secondary": secondary,
            "m3SecondaryContainer": secondary_container,
            "m3Tertiary": tertiary,
            "m3TertiaryContainer": tertiary_container,
            "m3Surface": surface,
            "m3SurfaceDim": surface_dim,
            "m3SurfaceBright": surface_bright,
            "m3SurfaceContainer": surface_container,
            "m3SurfaceContainerLow": surface_container_low,
            "m3SurfaceContainerHigh": surface_container_high,
            "m3SurfaceContainerHighest": surface_container_highest,
            "m3OnSurface": on_surface,
            "m3OnSurfaceVariant": on_surface_variant,
            "m3Outline": outline,
            "m3OutlineVariant": outline_variant,
            "m3Error": error_col,
            "m3ErrorContainer": error_container,
            "m3Success": success,
            "m3SuccessContainer": success_container,
            "m3Shadow": shadow,
            "m3InverseSurface": inverse_surface,
            "m3InverseOnSurface": inverse_on_surface,
            "m3InversePrimary": inverse_primary,
            "ansiRed": term[1],
            "ansiGreen": term[2],
            "ansiYellow": term[3],
            "ansiBlue": term[4],
            "ansiMagenta": term[5],
            "ansiCyan": term[6],
            "ansiBrightRed": term[9],
            "ansiBrightGreen": term[10],
            "ansiBrightYellow": term[11],
            "ansiBrightBlue": term[12],
            "ansiBrightMagenta": term[13],
            "ansiBrightCyan": term[14],
        },
        "theme": {
            "primary": tv("m3Primary", "m3InversePrimary"),
            "secondary": tv("m3Secondary", &light_secondary),
            "accent": tv("m3Tertiary", &light_accent),
            "error": tv("m3Error", &light_error),
            "warning": tv(warning, &light_warning),
            "success": tv("m3Success", &light_success),
            "info": tv("m3Primary", light_primary),
            "text": tv("m3OnSurface", "m3InverseOnSurface"),
            "textMuted": tv("m3OnSurfaceVariant", &light_text_muted),
            "selectedListItemText": tv("m3Surface", inverse_surface),
            "background": tv("m3Surface", "m3InverseSurface"),
            "backgroundPanel": tv("m3SurfaceContainer", &light_bg_panel),
            "backgroundElement": tv("m3SurfaceContainerHigh", &light_bg_element),
            "backgroundMenu": tv("m3SurfaceContainerHigh", &light_bg_menu),
            "border": tv("m3OutlineVariant", &light_border),
            "borderActive": tv("m3Primary", "m3InversePrimary"),
            "borderSubtle": tv(&blend_hex_lower(outline_variant, &surface, 0.3), &light_border_subtle),
            "diffAdded": tv("m3Success", &light_success),
            "diffRemoved": tv("m3Error", &light_error),
            "diffContext": tv("m3OnSurfaceVariant", &light_text_muted),
            "diffHunkHeader": tv("m3Outline", &light_border),
            "diffHighlightAdded": tv("ansiBrightGreen", "ansiGreen"),
            "diffHighlightRemoved": tv("ansiBrightRed", "ansiRed"),
            "diffAddedBg": tv(&diff_added_bg, &light_diff_added_bg),
            "diffRemovedBg": tv(&diff_removed_bg, &light_diff_removed_bg),
            "diffContextBg": tv("m3SurfaceContainer", &light_bg_panel),
            "diffLineNumber": tv("m3Outline", &light_border),
            "diffAddedLineNumberBg": tv(&diff_added_line_bg, &light_diff_added_line_bg),
            "diffRemovedLineNumberBg": tv(&diff_removed_line_bg, &light_diff_removed_line_bg),
            "markdownText": tv("m3OnSurface", "m3InverseOnSurface"),
            "markdownHeading": tv("m3Primary", "m3InversePrimary"),
            "markdownLink": tv("m3Tertiary", &light_md_link),
            "markdownLinkText": tv("ansiCyan", "ansiBlue"),
            "markdownCode": tv("ansiGreen", &light_md_code),
            "markdownBlockQuote": tv("m3OnSurfaceVariant", &light_text_muted),
            "markdownEmph": tv("ansiYellow", &light_warning),
            "markdownStrong": tv("ansiBrightYellow", warning),
            "markdownHorizontalRule": tv("m3OutlineVariant", &light_border),
            "markdownListItem": tv("m3Primary", "m3InversePrimary"),
            "markdownListEnumeration": tv("m3Secondary", &light_secondary),
            "markdownImage": tv("ansiMagenta", &light_accent),
            "markdownImageText": tv("ansiBrightMagenta", "ansiMagenta"),
            "markdownCodeBlock": tv("m3OnSurface", "m3InverseOnSurface"),
            "syntaxComment": tv("m3Outline", &light_text_muted),
            "syntaxKeyword": tv("ansiMagenta", &light_syn_kw),
            "syntaxFunction": tv("ansiBlue", &light_syn_fn),
            "syntaxVariable": tv("ansiCyan", &light_syn_var),
            "syntaxString": tv("ansiGreen", &light_syn_str),
            "syntaxNumber": tv("ansiBrightMagenta", &light_syn_num),
            "syntaxType": tv("ansiYellow", &light_syn_type),
            "syntaxOperator": tv("m3OnSurfaceVariant", &light_text_muted),
            "syntaxPunctuation": tv("m3OnSurfaceVariant", &light_text_muted),
            "thinkingOpacity": 0.6
        }
    });

    fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;
    let out_file = Path::new(&output_dir).join("inir.json");
    let text = format!("{}\n", serde_json::to_string_pretty(&doc).unwrap_or_default());
    fs::write(&out_file, text).map_err(|e| e.to_string())?;
    println!("[opencode] Theme generated: {}", out_file.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. System24 / Vesktop / Discord Theme Generator (`inir-system24-themegen`)
// ---------------------------------------------------------------------------

fn rgb_to_hls(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max_c = r.max(g).max(b);
    let min_c = r.min(g).min(b);
    let l = (min_c + max_c) / 2.0;
    if (min_c - max_c).abs() < 1e-9 {
        return (0.0, l, 0.0);
    }
    let s = if l <= 0.5 {
        (max_c - min_c) / (max_c + min_c)
    } else {
        (max_c - min_c) / (2.0 - max_c - min_c)
    };
    let rc = (max_c - r) / (max_c - min_c);
    let gc = (max_c - g) / (max_c - min_c);
    let bc = (max_c - b) / (max_c - min_c);
    let mut h = if (r - max_c).abs() < 1e-9 {
        bc - gc
    } else if (g - max_c).abs() < 1e-9 {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    h = (h / 6.0).rem_euclid(1.0);
    (h, l, s)
}

fn hls_to_rgb(h: f64, l: f64, s: f64) -> (f64, f64, f64) {
    if s == 0.0 {
        return (l, l, l);
    }
    let m2 = if l <= 0.5 {
        l * (1.0 + s)
    } else {
        l + s - (l * s)
    };
    let m1 = 2.0 * l - m2;
    let v = |mut hue: f64| -> f64 {
        hue = hue.rem_euclid(1.0);
        if hue < 1.0 / 6.0 {
            m1 + (m2 - m1) * hue * 6.0
        } else if hue < 0.5 {
            m2
        } else if hue < 2.0 / 3.0 {
            m1 + (m2 - m1) * (2.0 / 3.0 - hue) * 6.0
        } else {
            m1
        }
    };
    (v(h + 1.0 / 3.0), v(h), v(h - 1.0 / 3.0))
}

fn adjust_lightness_hls(hex: &str, delta: f64) -> String {
    let rgb = washi::parse_hex(hex).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    let (h, l, s) = rgb_to_hls(rgb.r, rgb.g, rgb.b);
    let l2 = (l + delta).clamp(0.0, 1.0);
    let (r2, g2, b2) = hls_to_rgb(h, l2, s);
    let r = (r2 * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8;
    let g = (g2 * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8;
    let b = (b2 * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

fn rgba_css(hex: &str, alpha: f64) -> String {
    let rgb = washi::parse_hex(hex).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    format!(
        "rgba({}, {}, {}, {:.2})",
        (rgb.r * 255.0).round() as u8,
        (rgb.g * 255.0).round() as u8,
        (rgb.b * 255.0).round() as u8,
        alpha
    )
}

pub fn run_system24_themegen_cli(_args: &[String]) -> Result<(), String> {
    let home = env::var("HOME").unwrap_or_default();
    let candidates = [
        env::var("QUICKSHELL_PALETTE_JSON")
            .unwrap_or_else(|_| format!("{home}/.local/state/quickshell/user/generated/app-palette.json")),
        env::var("QUICKSHELL_RAW_PALETTE_JSON")
            .unwrap_or_else(|_| format!("{home}/.local/state/quickshell/user/generated/palette.json")),
        env::var("QUICKSHELL_COLORS_JSON")
            .unwrap_or_else(|_| format!("{home}/.local/state/quickshell/user/generated/colors.json")),
    ];
    let source = candidates
        .iter()
        .find(|p| Path::new(p).exists())
        .ok_or_else(|| "Material colors file not found".to_string())?;

    let mut colors = BTreeMap::new();
    for (k, v) in load_json_colors(source) {
        colors.insert(k, v.to_lowercase());
    }
    let term_json_path = format!("{home}/.local/state/quickshell/user/generated/terminal.json");
    for (k, v) in load_json_colors(&term_json_path) {
        colors.insert(k, v.to_lowercase());
    }

    let primary = pick(&colors, "primary", "#7aa2f7");
    let secondary = pick(&colors, "secondary", "#c4a66f");
    let tertiary = pick(&colors, "tertiary", "#9ece6a");
    let error = pick(&colors, "error", "#f7768e");
    let surface = pick(&colors, "surface", "#141716");
    let surface_low = pick(&colors, "term0", pick(&colors, "surface_container_low", "#161918"));
    let surface_std = pick(&colors, "surface_container", "#1c201f");
    let surface_high = pick(&colors, "surface_container_high", "#252928");
    let surface_highest = pick(&colors, "surface_container_highest", "#303534");
    let outline = pick(&colors, "outline", "#7d776b");
    let on_surface = pick(&colors, "term15", pick(&colors, "on_surface", "#e3e5e3"));
    let raw_osv = pick(&colors, "on_surface_variant", "#b5a98e");
    let term7 = pick(&colors, "term7", secondary);
    let on_surface_variant_owned = blend_hex_lower(raw_osv, term7, 0.45);
    let on_surface_variant = on_surface_variant_owned.as_str();
    let on_primary = pick(&colors, "on_primary", "#141716");
    let success = pick(&colors, "success", tertiary);
    let warning = colors
        .get("app_warning")
        .cloned()
        .unwrap_or_else(|| blend_hex_lower(primary, tertiary, 0.3));
    let error_fill = pick(&colors, "error_fill", error);

    let mut pal = BTreeMap::new();
    pal.insert("--colors".to_string(), "on".to_string());
    pal.insert("--text-0".to_string(), on_primary.to_string());
    pal.insert("--text-1".to_string(), on_surface.to_string());
    pal.insert("--text-2".to_string(), blend_hex_lower(on_surface, on_surface_variant, 0.2));
    pal.insert("--text-3".to_string(), blend_hex_lower(on_surface, on_surface_variant, 0.4));
    pal.insert("--text-4".to_string(), on_surface_variant.to_string());
    pal.insert("--text-5".to_string(), outline.to_string());
    pal.insert("--bg-4".to_string(), surface.to_string());
    pal.insert("--bg-3".to_string(), surface_low.to_string());
    pal.insert("--bg-2".to_string(), surface_std.to_string());
    pal.insert("--bg-1".to_string(), surface_high.to_string());
    pal.insert("--bg-floating".to_string(), surface_highest.to_string());
    pal.insert("--hover".to_string(), rgba_css(primary, 0.10));
    pal.insert("--active".to_string(), rgba_css(primary, 0.20));
    pal.insert("--active-2".to_string(), rgba_css(primary, 0.30));
    pal.insert("--message-hover".to_string(), rgba_css(primary, 0.08));
    pal.insert("--accent-1".to_string(), adjust_lightness_hls(primary, 0.10));
    pal.insert("--accent-2".to_string(), primary.to_string());
    pal.insert("--accent-3".to_string(), adjust_lightness_hls(primary, -0.05));
    pal.insert("--accent-4".to_string(), adjust_lightness_hls(primary, -0.10));
    pal.insert("--accent-5".to_string(), adjust_lightness_hls(primary, -0.15));
    pal.insert(
        "--accent-new".to_string(),
        colors
            .get("error_fill")
            .cloned()
            .unwrap_or_else(|| primary.to_string()),
    );
    pal.insert(
        "--mention".to_string(),
        format!("linear-gradient(to right, color-mix(in hsl, {primary}, transparent 90%) 40%, transparent)"),
    );
    pal.insert(
        "--mention-hover".to_string(),
        format!("linear-gradient(to right, color-mix(in hsl, {primary}, transparent 95%) 40%, transparent)"),
    );
    pal.insert(
        "--reply".to_string(),
        format!("linear-gradient(to right, color-mix(in hsl, {on_surface_variant}, transparent 90%) 40%, transparent)"),
    );
    pal.insert(
        "--reply-hover".to_string(),
        format!("linear-gradient(to right, color-mix(in hsl, {on_surface_variant}, transparent 95%) 40%, transparent)"),
    );
    pal.insert("--online".to_string(), success.to_string());
    pal.insert("--dnd".to_string(), error.to_string());
    pal.insert("--idle".to_string(), secondary.to_string());
    pal.insert("--streaming".to_string(), adjust_lightness_hls(secondary, 0.10));
    pal.insert("--offline".to_string(), on_surface_variant.to_string());
    pal.insert("--border-light".to_string(), rgba_css(outline, 0.12));
    pal.insert("--border".to_string(), rgba_css(outline, 0.22));
    pal.insert(
        "--border-hover".to_string(),
        rgba_css(&blend_hex_lower(outline, primary, 0.35), 0.50),
    );
    pal.insert("--button-border".to_string(), rgba_css(outline, 0.16));

    let mut add_ladder = |prefix: &str, base: &str| {
        for (i, d) in [0.15, 0.08, 0.0, -0.08, -0.15].iter().enumerate() {
            let val = if *d == 0.0 {
                base.to_string()
            } else {
                adjust_lightness_hls(base, *d)
            };
            pal.insert(format!("--{prefix}-{}", i + 1), val);
        }
    };
    add_ladder("red", error_fill);
    add_ladder("green", success);
    add_ladder("blue", secondary);
    add_ladder("yellow", &warning);
    add_ladder("purple", &blend_hex_lower(primary, secondary, 0.5));
    add_ladder("orange", &blend_hex_lower(primary, error, 0.3));

    pal.insert("--brand-360".to_string(), adjust_lightness_hls(primary, 0.08));
    pal.insert("--brand-400".to_string(), adjust_lightness_hls(primary, 0.04));
    pal.insert("--brand-500".to_string(), primary.to_string());
    pal.insert("--brand-560".to_string(), adjust_lightness_hls(primary, -0.06));
    pal.insert("--brand-600".to_string(), adjust_lightness_hls(primary, -0.12));

    let mut css = String::from(":root, :root:root, body, #app-mount {\n");
    for (k, v) in &pal {
        css.push_str(&format!("    {k}: {v};\n"));
    }
    css.push_str("}\n\n:is(.theme-dark, .theme-light):not(.custom-user-profile-theme), :is(.theme-dark, .theme-light) .theme-dark, :is(.theme-dark, .theme-light) .theme-light {\n");
    for (k, v) in [
        ("--header-primary", "--text-1"),
        ("--header-secondary", "--text-3"),
        ("--text-strong", "--text-1"),
        ("--text-normal", "--text-2"),
        ("--text-default", "--text-2"),
        ("--text-muted", "--text-5"),
        ("--text-subtle", "--text-4"),
        ("--interactive-normal", "--text-4"),
        ("--interactive-hover", "--text-2"),
        ("--interactive-active", "--text-1"),
        ("--interactive-muted", "--text-5"),
        ("--channels-default", "--text-4"),
        ("--channel-icon", "--text-4"),
        ("--text-primary", "--text-1"),
        ("--text-secondary", "--text-3"),
        ("--text-tertiary", "--text-5"),
        ("--white", "--text-0"),
        ("--white-500", "--text-0"),
        ("--text-overlay-light", "--text-0"),
    ] {
        css.push_str(&format!("    {k}: var({v});\n"));
    }
    css.push('}');
    let bg_rgb = washi::parse_hex(surface).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    if (bg_rgb.r + bg_rgb.g + bg_rgb.b) > 1.5 {
        css.push_str("\n:is([class*=\"username\"], [class*=\"roleColor\"], [class*=\"mention\"])[style*=\"color: rgb(255, 255, 255)\"] { color: var(--text-1) !important; }");
    }

    let resolve_targets = |env_key: &str, basename: &str| -> Vec<PathBuf> {
        if let Ok(exp) = env::var(env_key) {
            if !exp.is_empty() {
                return vec![PathBuf::from(exp)];
            }
        }
        let dirs = ["vesktop", "Vesktop", "equicord", "Equicord", "equibop", "Equibop"];
        let mut existing = Vec::new();
        for d in dirs {
            let p = PathBuf::from(format!("{home}/.config/{d}/themes/{basename}"));
            if p.parent().map(| par | par.exists()).unwrap_or(false) {
                existing.push(p);
            }
        }
        if existing.is_empty() {
            vec![PathBuf::from(format!("{home}/.config/vesktop/themes/{basename}"))]
        } else {
            existing
        }
    };

    let write_theme_file = |out: &Path, header: &str, body: &str, local_copy: &str| {
        if let Some(p) = out.parent() {
            let _ = fs::create_dir_all(p);
        }
        let local_import = if out.parent().map(|p| p.join(local_copy).is_file()).unwrap_or(false) {
            format!("@import url('{local_copy}');\n")
        } else {
            String::new()
        };
        let content = format!("{header}{local_import}{body}");
        if fs::read_to_string(out).ok().as_deref() == Some(content.as_str()) {
            return;
        }
        if fs::write(out, content).is_ok() {
            println!("Generated: {}", out.display());
        }
    };

    for out in resolve_targets("SYSTEM24_PALETTE_CSS", "system24.theme.css") {
        let header = "/**\n * @name iNiR Material\n * @description Material Design Discord theme with Material You colors.\n * @author refact0r (system24 base), iNiR (Material adaptation)\n * @version 2.2.0\n * @source https://github.com/snowarch/iNiR\n */\n\n";
        let body = format!(
            "@import url('https://refact0r.github.io/system24/build/system24.css');\n@import url('https://fonts.googleapis.com/css2?family=Oxanium:wght@300;400;500;600;700&display=swap');\n\nbody {{\n    --font: 'Oxanium';\n    --code-font: 'Oxanium';\n    font-weight: 400;\n    letter-spacing: -0.02ch;\n    --gap: 12px;\n    --divider-thickness: 4px;\n    --border-thickness: 1px;\n    --animations: on;\n    --top-bar-height: 36px;\n    --custom-dms-background: color;\n    --dms-background-color: var(--accent-2);\n    --bg-floating: var(--bg-1);\n    --small-user-panel: on;\n    --custom-spotify-bar: on;\n}}\n\n/* Material You Palette - Auto-generated */\n{css}\n\n/* Material Design System */\n"
        );
        write_theme_file(&out, header, &body, "system24.local.css");
    }

    for out in resolve_targets("MIDNIGHT_DMS_CSS", "inir-midnight.theme.css") {
        let header = "/**\n * @name iNiR Midnight\n * @description iNiR Midnight Discord theme with Material You colors.\n * @author iNiR (Material palette injection)\n * @version 2.2.0\n * @source https://github.com/snowarch/iNiR\n */\n\n";
        let body = format!(
            "@import url('https://refact0r.github.io/midnight-discord/build/midnight.css');\n\n/* Material You Palette - Auto-generated */\n{css}\n\n:root, :root:root, body, #app-mount {{\n    --hover: color-mix(in hsl, var(--text-3), transparent 92%);\n    --active: color-mix(in hsl, var(--text-3), transparent 86%);\n    --active-2: color-mix(in hsl, var(--text-3), transparent 80%);\n    --message-hover: color-mix(in hsl, var(--text-3), transparent 94%);\n    --border-hover: var(--border);\n    --border-light: var(--border);\n    --button-border: var(--border);\n}}\n"
        );
        write_theme_file(&out, header, &body, "midnight-discord.local.css");
    }

    for out in resolve_targets("INIR_TUI_CSS", "inir-tui.theme.css") {
        let header = "/**\n * @name iNiR TUI\n * @description System24 TUI layout with iNiR Material You colors.\n * @author refact0r (system24 base), iNiR (palette integration)\n * @version 2.2.0\n * @source https://github.com/snowarch/inir\n */\n\n";
        let body = format!(
            "@import url('https://refact0r.github.io/system24/build/system24.css');\n\nbody {{\n    --font: 'JetBrainsMono Nerd Font';\n    --code-font: 'JetBrainsMono Nerd Font';\n    font-weight: 300;\n    letter-spacing: -0.05ch;\n    --gap: 12px;\n    --divider-thickness: 4px;\n    --border-thickness: 2px;\n    --animations: on;\n    --top-bar-height: var(--gap);\n    --top-bar-button-position: titlebar;\n    --bg-floating: var(--bg-3);\n    --small-user-panel: on;\n    --unrounding: on;\n    --custom-spotify-bar: on;\n    --ascii-titles: on;\n    --ascii-loader: system24;\n    --panel-labels: on;\n    --label-color: var(--text-5);\n    --label-font-weight: 500;\n}}\n\n/* Material You Palette - Auto-generated */\n{css}\n"
        );
        write_theme_file(&out, header, &body, "system24.local.css");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 4. Zed Editor Theme Generator (`inir-zed-themegen`)
// ---------------------------------------------------------------------------

pub fn run_zed_themegen_cli(args: &[String]) -> Result<(), String> {
    let home = env::var("HOME").unwrap_or_default();
    let mut scss_path = format!("{home}/.local/state/quickshell/user/generated/material_colors.scss");
    let mut colors_path = format!("{home}/.local/state/quickshell/user/generated/palette.json");
    let mut terminal_path = format!("{home}/.local/state/quickshell/user/generated/terminal.json");
    let mut output_path = format!("{home}/.config/zed/themes/ii-theme.json");

    let mut pos = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--scss" | "-scss" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    scss_path = v.clone();
                }
            }
            "--colors" | "-colors" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    colors_path = v.clone();
                }
            }
            "--terminal-json" | "-terminal-json" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    terminal_path = v.clone();
                }
            }
            "--output" | "-output" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    output_path = v.clone();
                }
            }
            "--template" | "-template" => {
                i += 1;
            }
            arg if !arg.starts_with('-') => {
                pos.push(arg.to_string());
            }
            _ => {}
        }
        i += 1;
    }
    if let Some(p0) = pos.first() {
        scss_path = p0.clone();
    }
    if let Some(p1) = pos.get(1) {
        colors_path = p1.clone();
    }
    if let Some(p2) = pos.get(2) {
        terminal_path = p2.clone();
    }

    let mut colors = parse_scss_colors(&scss_path);
    for (k, v) in load_json_colors(&colors_path) {
        colors.insert(k, v.to_lowercase());
    }
    let term_colors = load_json_colors(&terminal_path);

    let norm6 = |v: &str| -> String {
        let s = v.trim().trim_start_matches('#').to_lowercase();
        if s.len() >= 6 {
            format!("#{}", &s[..6])
        } else {
            "#000000".to_string()
        }
    };
    let alpha = |v: &str, a: &str| format!("{}{}", norm6(v), a);

    let dk_term0 = term_colors
        .get("term0")
        .map(String::as_str)
        .unwrap_or_else(|| pick(&colors, "surface_container_low", "#161918"));
    let dk_term7 = term_colors
        .get("term7")
        .map(String::as_str)
        .unwrap_or_else(|| pick(&colors, "secondary", "#c4a66f"));
    let dk_surface = norm6(dk_term0);
    let dk_low = norm6(pick(&colors, "surface", "#141716"));
    let dk_std = norm6(pick(&colors, "surface_container", "#1c201f"));
    let dk_high = norm6(pick(&colors, "surface_container_high", "#252928"));
    let dk_on = norm6(
        term_colors
            .get("term15")
            .map(String::as_str)
            .unwrap_or_else(|| pick(&colors, "on_surface", "#e3e5e3")),
    );
    let dk_on_var = blend_hex_lower(
        &norm6(pick(&colors, "on_surface_variant", "#b5a98e")),
        &norm6(dk_term7),
        0.50,
    );
    let dk_outline = norm6(pick(&colors, "outline", "#7d776b"));
    let dk_outline_var = blend_hex_lower(
        &norm6(pick(&colors, "outline_variant", "#3b3e3d")),
        &norm6(dk_term7),
        0.22,
    );
    let primary = norm6(pick(&colors, "primary", "#9cd49d"));
    let secondary = norm6(pick(&colors, "secondary", "#c4a66f"));
    let tertiary = norm6(pick(&colors, "tertiary", "#7cd3a5"));
    let _error_col = norm6(pick(&colors, "error", "#ffb4ab"));

    let lt_surface = norm6(pick(&colors, "inverse_surface", "#faf4f2"));
    let lt_low = blend_hex_lower(&lt_surface, "#000000", 0.03);
    let lt_std = blend_hex_lower(&lt_surface, "#000000", 0.06);
    let lt_high = blend_hex_lower(&lt_surface, "#000000", 0.09);
    let lt_on = norm6(pick(&colors, "inverse_on_surface", "#2a2022"));
    let lt_on_var = norm6(pick(&colors, "outline_variant", "#5a4b4e"));

    let build_zed_style = |surf: &str, low: &str, std_c: &str, high: &str, on_s: &str, on_v: &str, is_dark: bool| -> Value {
        let mut st = Map::new();
        st.insert("border".into(), json!(alpha(&dk_outline_var, "ff")));
        st.insert("border.variant".into(), json!(alpha(&dk_outline_var, "ff")));
        st.insert("border.focused".into(), json!(alpha(&primary, "ff")));
        st.insert("border.selected".into(), json!(alpha(&primary, "ff")));
        st.insert("border.transparent".into(), json!("#00000000"));
        st.insert("border.disabled".into(), json!(alpha(&dk_outline, "ff")));
        st.insert("elevated_surface.background".into(), json!(alpha(low, "ff")));
        st.insert("surface.background".into(), json!(alpha(low, "ff")));
        st.insert("background".into(), json!(alpha(surf, "ff")));
        st.insert("element.background".into(), json!(alpha(low, "ff")));
        st.insert("element.hover".into(), json!(alpha(std_c, "ff")));
        st.insert("element.active".into(), json!(alpha(high, "ff")));
        st.insert("element.selected".into(), json!(alpha(high, "ff")));
        st.insert("element.disabled".into(), json!(alpha(low, "ff")));
        st.insert("drop_target.background".into(), json!(alpha(&primary, "80")));
        st.insert("ghost_element.background".into(), json!("#00000000"));
        st.insert("ghost_element.hover".into(), json!(alpha(std_c, "ff")));
        st.insert("ghost_element.active".into(), json!(alpha(high, "ff")));
        st.insert("ghost_element.selected".into(), json!(alpha(high, "ff")));
        st.insert("ghost_element.disabled".into(), json!(alpha(low, "ff")));
        st.insert("text".into(), json!(alpha(on_s, "ff")));
        st.insert("text.muted".into(), json!(alpha(on_v, "ff")));
        st.insert("text.placeholder".into(), json!(alpha(on_v, "b0")));
        st.insert("text.disabled".into(), json!(alpha(on_v, "80")));
        st.insert("text.accent".into(), json!(alpha(&primary, "ff")));
        st.insert("icon".into(), json!(alpha(on_s, "ff")));
        st.insert("icon.muted".into(), json!(alpha(on_v, "ff")));
        st.insert("icon.disabled".into(), json!(alpha(on_v, "80")));
        st.insert("icon.placeholder".into(), json!(alpha(on_v, "ff")));
        st.insert("icon.accent".into(), json!(alpha(&primary, "ff")));
        st.insert("status_bar.background".into(), json!(alpha(surf, "ff")));
        st.insert("title_bar.background".into(), json!(alpha(surf, "ff")));
        st.insert("title_bar.inactive_background".into(), json!(alpha(low, "ff")));
        st.insert("toolbar.background".into(), json!(alpha(low, "ff")));
        st.insert("tab_bar.background".into(), json!(alpha(low, "ff")));
        st.insert("tab.inactive_background".into(), json!(alpha(low, "ff")));
        st.insert("tab.active_background".into(), json!(alpha(surf, "ff")));
        st.insert("search.match_background".into(), json!(alpha(&primary, "66")));
        st.insert("search.active_match_background".into(), json!(alpha(&tertiary, "66")));
        st.insert("panel.background".into(), json!(alpha(low, "ff")));
        st.insert("panel.focused_border".into(), Value::Null);
        st.insert("pane.focused_border".into(), Value::Null);
        st.insert("scrollbar.thumb.background".into(), json!(alpha(on_v, "4c")));
        st.insert("scrollbar.thumb.hover_background".into(), json!(alpha(high, "ff")));
        st.insert("scrollbar.thumb.border".into(), json!(alpha(std_c, "ff")));
        st.insert("scrollbar.track.background".into(), json!("#00000000"));
        st.insert("scrollbar.track.border".into(), json!(alpha(std_c, "ff")));
        st.insert("editor.foreground".into(), json!(alpha(on_s, "ff")));
        st.insert("editor.background".into(), json!(alpha(surf, "ff")));
        st.insert("editor.gutter.background".into(), json!(alpha(surf, "ff")));
        st.insert("editor.subheader.background".into(), json!(alpha(low, "ff")));
        st.insert("editor.active_line.background".into(), json!(alpha(low, "bf")));
        st.insert("editor.highlighted_line.background".into(), json!(alpha(std_c, "ff")));
        st.insert("editor.line_number".into(), json!(alpha(on_v, "ff")));
        st.insert("editor.active_line_number".into(), json!(alpha(on_s, "ff")));
        st.insert("terminal.background".into(), json!(alpha(surf, "ff")));
        st.insert("terminal.foreground".into(), json!(alpha(on_s, "ff")));
        for (name, idx) in [
            ("black", 0),
            ("red", 1),
            ("green", 2),
            ("yellow", 3),
            ("blue", 4),
            ("magenta", 5),
            ("cyan", 6),
            ("white", 7),
            ("bright_black", 8),
            ("bright_red", 9),
            ("bright_green", 10),
            ("bright_yellow", 11),
            ("bright_blue", 12),
            ("bright_magenta", 13),
            ("bright_cyan", 14),
            ("bright_white", 15),
        ] {
            let tc = term_colors
                .get(&format!("term{idx}"))
                .or_else(|| colors.get(&format!("term{idx}")))
                .map(String::as_str)
                .unwrap_or("#888888");
            st.insert(format!("terminal.ansi.{name}"), json!(alpha(tc, "ff")));
        }

        let mut syn = Map::new();
        let base_tokens: &[(&str, &str)] = if is_dark {
            &[
                ("attribute", "#74ade8"),
                ("boolean", "#bf956a"),
                ("comment", "#5d636f"),
                ("comment.doc", "#878e98"),
                ("constant", "#dfc184"),
                ("constructor", "#73ade9"),
                ("embedded", "#dce0e5"),
                ("emphasis", "#74ade8"),
                ("enum", "#6eb4bf"),
                ("function", "#73ade9"),
                ("hint", "#788ca6"),
                ("keyword", "#b477cf"),
                ("label", "#74ade8"),
                ("link_text", "#73ade9"),
                ("link_uri", "#6eb4bf"),
                ("namespace", "#dce0e5"),
                ("number", "#bf956a"),
                ("operator", "#6eb4bf"),
                ("primary", "#acb2be"),
                ("property", "#d07277"),
                ("punctuation", "#acb2be"),
                ("string", "#a1c181"),
                ("tag", "#74ade8"),
                ("title", "#d07277"),
                ("type", "#6eb4bf"),
                ("variable", "#acb2be"),
            ]
        } else {
            &[
                ("attribute", "#5c78e2"),
                ("boolean", "#ad6e25"),
                ("comment", "#a2a3a7"),
                ("comment.doc", "#7c7e86"),
                ("constant", "#c18401"),
                ("constructor", "#5c78e2"),
                ("embedded", "#242529"),
                ("emphasis", "#5c78e2"),
                ("enum", "#3882b7"),
                ("function", "#5b79e3"),
                ("hint", "#7274a7"),
                ("keyword", "#a449ab"),
                ("label", "#5c78e2"),
                ("link_text", "#5b79e3"),
                ("link_uri", "#3882b7"),
                ("namespace", "#242529"),
                ("number", "#ad6e25"),
                ("operator", "#3882b7"),
                ("primary", "#242529"),
                ("property", "#d3604f"),
                ("punctuation", "#242529"),
                ("string", "#649f57"),
                ("tag", "#5c78e2"),
                ("title", "#d3604f"),
                ("type", "#3882b7"),
                ("variable", "#242529"),
            ]
        };
        for &(tok, col) in base_tokens {
            let blended = blend_hex_lower(col, &primary, 0.50);
            syn.insert(
                tok.to_string(),
                json!({
                    "color": alpha(&blended, "ff"),
                    "font_style": Value::Null,
                    "font_weight": Value::Null
                }),
            );
        }
        st.insert("syntax".into(), Value::Object(syn));
        st.insert(
            "players".into(),
            json!([
                { "cursor": alpha(&primary, "ff"), "background": alpha(&primary, "ff"), "selection": alpha(&primary, "3d") },
                { "cursor": alpha(&secondary, "ff"), "background": alpha(&secondary, "ff"), "selection": alpha(&secondary, "3d") },
            ]),
        );
        Value::Object(st)
    };

    let dark_style = build_zed_style(&dk_surface, &dk_low, &dk_std, &dk_high, &dk_on, &dk_on_var, true);
    let light_style = build_zed_style(&lt_surface, &lt_low, &lt_std, &lt_high, &lt_on, &lt_on_var, false);

    let make_borderless = |v: &Value| -> Value {
        let mut clone = v.clone();
        if let Some(obj) = clone.as_object_mut() {
            for k in [
                "border",
                "border.variant",
                "border.focused",
                "border.selected",
                "border.transparent",
                "border.disabled",
                "scrollbar.thumb.border",
                "scrollbar.track.border",
            ] {
                obj.insert(k.to_string(), json!("#00000000"));
            }
        }
        clone
    };

    let doc = json!({
        "$schema": "https://zed.dev/schema/themes/v0.2.0.json",
        "name": "iNiR Material",
        "author": "iNiR Theme System",
        "themes": [
            { "name": "iNiR Dark", "appearance": "dark", "style": dark_style },
            { "name": "iNiR Light", "appearance": "light", "style": light_style },
            { "name": "iNiR Borderless Dark", "appearance": "dark", "style": make_borderless(&dark_style) },
            { "name": "iNiR Borderless Light", "appearance": "light", "style": make_borderless(&light_style) },
            { "name": "iNiR-alt Dark", "appearance": "dark", "style": dark_style },
            { "name": "iNiR-alt Light", "appearance": "light", "style": light_style },
        ]
    });

    if let Some(p) = Path::new(&output_path).parent() {
        let _ = fs::create_dir_all(p);
    }
    let text = format!("{}\n", serde_json::to_string_pretty(&doc).unwrap_or_default());
    fs::write(&output_path, text).map_err(|e| e.to_string())?;
    println!("✓ Generated Zed theme");
    Ok(())
}

// ---------------------------------------------------------------------------
// 5. VSCode & Forks Theme Generator (`inir-vscode-themegen`)
// ---------------------------------------------------------------------------

const VSCODE_FORKS: &[(&str, &str, &str)] = &[
    ("code", "Code", ".vscode/extensions"),
    ("codium", "VSCodium", ".vscode-oss/extensions"),
    ("code-oss", "Code - OSS", ".vscode-oss/extensions"),
    ("code-insiders", "Code - Insiders", ".vscode-insiders/extensions"),
    ("cursor", "Cursor", ".cursor/extensions"),
    ("windsurf", "Windsurf", ".windsurf/extensions"),
    ("windsurf-next", "Windsurf - Next", ".windsurf-next/extensions"),
    ("qoder", "Qoder", ".qoder/extensions"),
    ("antigravity", "Antigravity", ".antigravity/extensions"),
    ("positron", "Positron", ".positron/extensions"),
    ("void", "Void", ".void/extensions"),
    ("melty", "Melty", ".melty/extensions"),
    ("pearai", "PearAI", ".pearai/extensions"),
    ("aide", "Aide", ".aide/extensions"),
];

pub fn strip_jsonc_comments(data: &str) -> String {
    let mut out = String::with_capacity(data.len());
    let bytes = data.as_bytes();
    let n = bytes.len();
    let mut in_str = false;
    let mut esc = false;
    let mut i = 0;

    while i < n {
        let c = bytes[i];
        if in_str {
            let ch = data[i..].chars().next().unwrap();
            out.push(ch);
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            i += ch.len_utf8();
            continue;
        }
        if c == b'"' {
            in_str = true;
            out.push('"');
            i += 1;
        } else if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(n);
        } else if c == b',' && i + 1 < n {
            let mut j = i + 1;
            while j < n && matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r') {
                j += 1;
            }
            if j < n && (bytes[j] == b']' || bytes[j] == b'}') {
                i += 1;
            } else {
                out.push(',');
                i += 1;
            }
        } else {
            let ch = data[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

pub fn run_vscode_themegen_cli(args: &[String]) -> i32 {
    let home = env::var("HOME").unwrap_or_default();
    let cfg_dir = env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
    let mut colors_path = format!("{home}/.local/state/quickshell/user/generated/palette.json");
    let mut terminal_path = format!("{home}/.local/state/quickshell/user/generated/terminal.json");
    let mut scss_path = format!("{home}/.local/state/quickshell/user/generated/material_colors.scss");
    let mut list_forks = false;
    let mut strip_mode = false;
    let mut forks: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--colors" | "-colors" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    colors_path = v.clone();
                }
            }
            "--terminal-json" | "-terminal-json" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    terminal_path = v.clone();
                }
            }
            "--scss" | "-scss" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    scss_path = v.clone();
                }
            }
            "--list-forks" | "-list-forks" => list_forks = true,
            "--strip" | "-strip" => strip_mode = true,
            "--forks" | "-forks" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    forks.push(v.clone());
                }
            }
            _ => {}
        }
        i += 1;
    }

    if list_forks {
        println!("Known VSCode forks:");
        for &(key, name, ext_rel) in VSCODE_FORKS {
            let user_dir = PathBuf::from(format!("{cfg_dir}/{name}/User"));
            let inst = if user_dir.is_dir() { "✓" } else { "✗" };
            let ext_dir = PathBuf::from(format!("{home}/{ext_rel}/inir-material-theme"));
            let ext_inst = if ext_dir.is_dir() { "✓" } else { "✗" };
            println!("  [{inst}] {key}: {name} (ext: {ext_inst})");
        }
        return 0;
    }

    let colors = load_json_colors(&colors_path);
    let mut term_colors = parse_scss_colors(&scss_path);
    for (k, v) in load_json_colors(&terminal_path) {
        term_colors.insert(k, v);
    }

    let selected: Vec<(&str, &str, &str)> = if forks.is_empty() {
        VSCODE_FORKS
            .iter()
            .copied()
            .filter(|&(_k, name, ext_rel)| {
                let user_dir = PathBuf::from(format!("{cfg_dir}/{name}/User"));
                let ext_dir = PathBuf::from(format!("{home}/{ext_rel}/inir-material-theme"));
                user_dir.is_dir() || (strip_mode && ext_dir.is_dir())
            })
            .collect()
    } else {
        VSCODE_FORKS
            .iter()
            .copied()
            .filter(|&(k, _, _)| forks.iter().any(|f| f == k))
            .collect()
    };

    if strip_mode {
        let mut success = 0;
        for &(key, name, ext_rel) in &selected {
            let ext_dir = PathBuf::from(format!("{home}/{ext_rel}/inir-material-theme"));
            let _ = fs::remove_dir_all(&ext_dir);
            let settings_path = PathBuf::from(format!("{cfg_dir}/{name}/User/settings.json"));
            if let Ok(raw) = fs::read_to_string(&settings_path) {
                if let Ok(mut obj) = serde_json::from_str::<Map<String, Value>>(&strip_jsonc_comments(&raw)) {
                    if let Some(cur) = obj.get("workbench.colorTheme").and_then(|v| v.as_str()) {
                        if cur == "iNiR Material" || cur == "iNiR Material Light" {
                            if let Some(prev) = obj.remove("inir.previousColorTheme") {
                                obj.insert("workbench.colorTheme".to_string(), prev);
                            } else {
                                obj.remove("workbench.colorTheme");
                            }
                        }
                    }
                    let _ = fs::write(
                        &settings_path,
                        format!("{}\n", serde_json::to_string_pretty(&obj).unwrap_or_default()),
                    );
                }
            }
            println!("  ✓ {name}");
            let _ = key;
            success += 1;
        }
        println!("Stripped themes from {}/{} forks", success, selected.len());
        return if success > 0 { 0 } else { 1 };
    }

    if colors.is_empty() {
        eprintln!("could not read {colors_path}");
        return 1;
    }

    let raw_bg = pick(&colors, "background", pick(&colors, "surface", "#141716"));
    let raw_fg = pick(&colors, "on_background", pick(&colors, "on_surface", "#e3e5e3"));
    let surface = pick(&colors, "surface", "#141716");
    let surface_lowest = pick(&colors, "surface_container_lowest", surface);
    let surface_low = pick(&colors, "surface_container_low", "#161918");
    let surface_std = pick(&colors, "surface_container", "#1c201f");
    let surface_high = pick(&colors, "surface_container_high", "#252928");
    let _surface_highest = pick(&colors, "surface_container_highest", "#303534");
    let raw_on_surface = pick(&colors, "on_surface", "#e3e5e3");
    let raw_on_surface_var = pick(&colors, "on_surface_variant", "#b5a98e");
    let outline = pick(&colors, "outline", "#7d776b");
    let raw_outline_var = pick(&colors, "outline_variant", "#3b3e3d");
    let primary = pick(&colors, "primary", "#9cd49d");
    let on_primary = pick(&colors, "on_primary", "#162a18");
    let primary_container = pick(&colors, "primary_container", "#293e2a");
    let on_primary_container = pick(&colors, "on_primary_container", "#d4f0d3");
    let secondary = pick(&colors, "secondary", "#c4a66f");
    let tertiary = pick(&colors, "tertiary", "#7cd3a5");
    let error_col = pick(&colors, "error", "#ffb4ab");

    let bg_rgb = washi::parse_hex(raw_bg).unwrap_or(washi::RGB { r: 0.08, g: 0.09, b: 0.09 });
    let is_dark = (bg_rgb.r + bg_rgb.g + bg_rgb.b) < 1.5;
    let (active_name, active_file, theme_type) = if is_dark {
        ("iNiR Material", "inir-material-color-theme.json", "dark")
    } else {
        ("iNiR Material Light", "inir-material-light-color-theme.json", "light")
    };

    let t = |idx: usize, fb: &str| pick(&term_colors, &format!("term{idx}"), fb).to_string();
    let bg_owned = if is_dark { t(0, surface_low) } else { raw_bg.to_string() };
    let fg_owned = if is_dark { t(15, raw_fg) } else { raw_fg.to_string() };
    let on_surface_owned = if is_dark { t(15, raw_on_surface) } else { raw_on_surface.to_string() };
    let on_surface_var_owned = if is_dark {
        blend_hex_lower(raw_on_surface_var, &t(7, secondary), 0.55)
    } else {
        raw_on_surface_var.to_string()
    };
    let outline_var_owned = if is_dark {
        blend_hex_lower(raw_outline_var, &t(7, secondary), 0.20)
    } else {
        raw_outline_var.to_string()
    };
    let bg = bg_owned.as_str();
    let fg = fg_owned.as_str();
    let on_surface = on_surface_owned.as_str();
    let on_surface_var = on_surface_var_owned.as_str();
    let outline_var = outline_var_owned.as_str();

    let theme_json = json!({
        "$schema": "vscode://schemas/color-theme",
        "name": active_name,
        "type": theme_type,
        "semanticHighlighting": true,
        "colors": {
            "focusBorder": primary,
            "foreground": fg,
            "disabledForeground": format!("{on_surface_var}80"),
            "selection.background": format!("{primary}60"),
            "descriptionForeground": on_surface_var,
            "errorForeground": error_col,
            "icon.foreground": on_surface,
            "button.background": primary_container,
            "button.foreground": on_primary_container,
            "input.background": surface_low,
            "input.foreground": on_surface,
            "badge.background": primary_container,
            "badge.foreground": on_primary_container,
            "list.activeSelectionBackground": surface_high,
            "list.activeSelectionForeground": on_surface,
            "list.hoverBackground": surface_std,
            "list.inactiveSelectionBackground": surface_std,
            "activityBar.background": surface_lowest,
            "activityBar.foreground": on_surface,
            "activityBarBadge.background": primary,
            "activityBarBadge.foreground": on_primary,
            "sideBar.background": surface_lowest,
            "sideBar.foreground": on_surface,
            "editorGroupHeader.tabsBackground": surface_lowest,
            "tab.activeBackground": surface_low,
            "tab.activeForeground": on_surface,
            "tab.activeBorder": primary,
            "tab.inactiveBackground": surface_lowest,
            "tab.inactiveForeground": on_surface_var,
            "editor.background": bg,
            "editor.foreground": fg,
            "editorLineNumber.foreground": on_surface_var,
            "editorLineNumber.activeForeground": on_surface,
            "editorCursor.foreground": primary,
            "editor.selectionBackground": format!("{primary_container}66"),
            "editor.lineHighlightBackground": format!("{surface_low}80"),
            "editorIndentGuide.background": outline_var,
            "editorIndentGuide.activeBackground": outline,
            "editorWidget.background": surface_high,
            "panel.background": surface_lowest,
            "statusBar.background": surface_lowest,
            "statusBar.foreground": on_surface,
            "titleBar.activeBackground": surface_lowest,
            "titleBar.activeForeground": on_surface,
            "terminal.background": t(0, surface_lowest),
            "terminal.foreground": t(15, fg),
            "terminal.ansiBlack": t(0, surface_lowest),
            "terminal.ansiRed": t(1, error_col),
            "terminal.ansiGreen": t(2, tertiary),
            "terminal.ansiYellow": t(3, secondary),
            "terminal.ansiBlue": t(4, primary),
            "terminal.ansiMagenta": t(5, secondary),
            "terminal.ansiCyan": t(6, tertiary),
            "terminal.ansiWhite": t(7, on_surface),
            "terminal.ansiBrightBlack": t(8, outline_var),
            "terminal.ansiBrightRed": t(9, error_col),
            "terminal.ansiBrightGreen": t(10, tertiary),
            "terminal.ansiBrightYellow": t(11, secondary),
            "terminal.ansiBrightBlue": t(12, primary),
            "terminal.ansiBrightMagenta": t(13, secondary),
            "terminal.ansiBrightCyan": t(14, tertiary),
            "terminal.ansiBrightWhite": t(15, fg),
        },
        "tokenColors": [
            { "scope": ["comment", "punctuation.definition.comment"], "settings": { "foreground": format!("{on_surface_var}aa"), "fontStyle": "italic" } },
            { "scope": ["keyword", "storage.type", "storage.modifier"], "settings": { "foreground": t(5, secondary) } },
            { "scope": ["constant", "constant.numeric", "constant.language"], "settings": { "foreground": t(3, secondary) } },
            { "scope": ["string", "string.quoted"], "settings": { "foreground": t(2, tertiary) } },
            { "scope": ["entity.name.function", "support.function"], "settings": { "foreground": t(4, primary) } },
            { "scope": ["entity.name.type", "entity.name.class", "support.type"], "settings": { "foreground": t(6, tertiary) } },
            { "scope": ["entity.name.tag"], "settings": { "foreground": t(1, error_col) } },
            { "scope": ["variable", "variable.other"], "settings": { "foreground": on_surface } },
        ]
    });

    let manifest_json = json!({
        "name": "inir-material-theme",
        "displayName": "iNiR Material",
        "description": "Dynamic Material You theme — auto-generated by iNiR from wallpaper colors",
        "version": "1.0.0",
        "publisher": "inir",
        "engines": { "vscode": "^1.34.0" },
        "categories": ["Themes"],
        "contributes": {
            "themes": [
                { "label": "iNiR Material", "uiTheme": "vs-dark", "path": "./themes/inir-material-color-theme.json", "_watch": true },
                { "label": "iNiR Material Light", "uiTheme": "vs", "path": "./themes/inir-material-light-color-theme.json", "_watch": true }
            ]
        }
    });

    let mut success = 0;
    for &(_key, name, ext_rel) in &selected {
        let user_dir = PathBuf::from(format!("{cfg_dir}/{name}/User"));
        if !user_dir.is_dir() {
            continue;
        }
        let ext_dir = PathBuf::from(format!("{home}/{ext_rel}/inir-material-theme"));
        let themes_dir = ext_dir.join("themes");
        if fs::create_dir_all(&themes_dir).is_err() {
            continue;
        }
        let pkg_path = ext_dir.join("package.json");
        if !pkg_path.exists() {
            let _ = fs::write(
                &pkg_path,
                format!("{}\n", serde_json::to_string_pretty(&manifest_json).unwrap_or_default()),
            );
        }
        let theme_path = themes_dir.join(active_file);
        let tmp_theme = themes_dir.join(format!("{active_file}.tmp"));
        if fs::write(
            &tmp_theme,
            format!("{}\n", serde_json::to_string_pretty(&theme_json).unwrap_or_default()),
        )
        .is_ok()
        {
            let _ = fs::rename(&tmp_theme, &theme_path);
        }

        let settings_path = user_dir.join("settings.json");
        let mut settings: Map<String, Value> = fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&strip_jsonc_comments(&s)).ok())
            .unwrap_or_default();
        let cur = settings
            .get("workbench.colorTheme")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut changed = false;
        if !cur.is_empty() && cur != "iNiR Material" && cur != "iNiR Material Light" {
            settings.insert("inir.previousColorTheme".to_string(), json!(cur));
            changed = true;
        }
        if cur != active_name {
            settings.insert("workbench.colorTheme".to_string(), json!(active_name));
            changed = true;
        }
        if changed {
            let _ = fs::write(
                &settings_path,
                format!("{}\n", serde_json::to_string_pretty(&settings).unwrap_or_default()),
            );
        }
        println!("  ✓ {name}");
        success += 1;
    }

    println!("Generated themes for {}/{} forks", success, selected.len());
    if success > 0 { 0 } else { 1 }
}
