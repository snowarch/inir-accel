//! Port of `scripts/colors/generate_terminal_configs.py` (its terminal half; VSCode and Zed come from `vscode.rs` and
//! `zed.rs`), line for line: the same files, the same edits to the person's configs, the same lines printed, the same
//! exception where the original raises. `test/parity.py terminals` runs both on generated inputs.

use crate::material::py::{self, Json};
use crate::pyrt::{self, as_str, exc, exists, makedirs, re, read_text, sub, write_text, R};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ALL: [&str; 12] = ["kitty", "alacritty", "foot", "wezterm", "ghostty", "konsole", "starship", "omp", "btop", "lazygit", "yazi", "fuzzel"];

/// A Python dict: insertion order, an update keeps a key's first place.
#[derive(Clone, Default)]
struct Dict(Vec<(String, Json)>);

impl Dict {
    fn get(&self, k: &str) -> Option<&Json> {
        self.0.iter().find(|(key, _)| key == k).map(|(_, v)| v)
    }
    fn set(&mut self, k: &str, v: Json) {
        match self.0.iter_mut().find(|(key, _)| key == k) {
            Some(slot) => slot.1 = v,
            None => self.0.push((k.to_string(), v)),
        }
    }
    fn update(&mut self, other: &Dict) {
        for (k, v) in &other.0 {
            self.set(k, v.clone());
        }
    }
    /// `d.get(k, default)`
    fn or(&self, k: &str, default: Json) -> Json {
        self.get(k).cloned().unwrap_or(default)
    }
    /// `d.get(k, "<hex>")` where every value is a str
    fn s(&self, k: &str, default: &str) -> String {
        match self.get(k) {
            Some(v) => pyrt::str_of(v),
            None => default.to_string(),
        }
    }
    fn from_json(v: Json) -> R<Dict> {
        match v {
            Json::Obj(o) => {
                let mut d = Dict::default();
                for (k, v) in o {
                    d.set(&k, v);
                }
                Ok(d)
            }
            other => Err(exc("AttributeError", format!("'{}' object has no attribute 'items'", pyrt::type_name(&other)))),
        }
    }
}

fn st(s: &str) -> Json {
    Json::Str(s.to_string())
}

fn parse_scss_colors(path: &str) -> R<Dict> {
    let text = match read_text(Path::new(path)) {
        Ok(t) => t,
        Err(e) if e.ty == "FileNotFoundError" => {
            eprintln!("Error: Could not find {path}");
            return Err(exc("SystemExit", "1"));
        }
        Err(e) => return Err(e),
    };
    let rx = re(r"^\$(\w+):\s*(#[A-Fa-f0-9]{6});");
    let mut colors = Dict::default();
    for line in text.split_inclusive('\n') {
        if let Some(c) = rx.captures(py::strip(line)) {
            colors.set(&c[1], st(&c[2]));
        }
    }
    Ok(colors)
}

fn load_json_colors(path: &str) -> R<Dict> {
    match pyrt::json_load(Path::new(path)) {
        Ok(v) => {
            let all = Dict::from_json(v)?;
            Ok(Dict(all.0.into_iter().filter(|(_, v)| matches!(v, Json::Str(_))).collect()))
        }
        Err(e) if e.ty == "FileNotFoundError" => Ok(Dict::default()),
        Err(e) => Err(e),
    }
}

fn load_generator_colors(scss: &str, palette: &str, terminal: &str) -> R<Dict> {
    let mut colors = if scss.is_empty() { Dict::default() } else { parse_scss_colors(scss)? };
    let palette_colors = load_json_colors(palette)?;
    let terminal_colors = load_json_colors(terminal)?;
    colors.update(&palette_colors);
    colors.update(&terminal_colors);
    let (term0, term15) = (colors.or("term0", st("#282828")), colors.or("term15", st("#EBDBB2")));
    let primary = colors.or("primary", st("#458588"));
    let mut factor = 0.35;
    while factor > 0.05 && contrast_hex(&term15, &st(&blend_hex(&term0, &primary, factor)?))? < 4.5 {
        factor -= 0.03;
    }
    let sel = blend_hex(&term0, &primary, factor)?;
    if colors.get("selectionBg").is_none() {
        colors.set("selectionBg", st(&sel));
    }
    let tab = blend_hex(&term0, &term15, 0.06)?;
    if colors.get("tabInactiveBg").is_none() {
        colors.set("tabInactiveBg", st(&tab));
    }
    Ok(colors)
}

fn lum(h: &Json) -> R<f64> {
    let h = as_str(h, "lstrip")?.trim_start_matches('#');
    let mut c = [0.0; 3];
    for (n, i) in [0, 2, 4].into_iter().enumerate() {
        let x = pyrt::int16(&pyrt::slice(h, i, Some(i + 2)))? as f64 / 255.0;
        c[n] = if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) };
    }
    Ok(0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2])
}

fn contrast_hex(a: &Json, b: &Json) -> R<f64> {
    let (la, lb) = (lum(a)?, lum(b)?);
    Ok((la.max(lb) + 0.05) / (la.min(lb) + 0.05))
}

/// `format(v, "02X")`
fn hex2(v: i64) -> String {
    if v < 0 { format!("-{:X}", -v) } else { format!("{v:02X}") }
}

fn blend_hex(c1: &Json, c2: &Json, factor: f64) -> R<String> {
    let factor = 0.0f64.max(1.0f64.min(factor));
    let c1 = as_str(c1, "lstrip")?.trim_start_matches('#');
    let c2 = as_str(c2, "lstrip")?.trim_start_matches('#');
    let p = |s: &str, i: usize| pyrt::int16(&pyrt::slice(s, i, Some(i + 2)));
    let (r1, g1, b1) = (p(c1, 0)?, p(c1, 2)?, p(c1, 4)?);
    let (r2, g2, b2) = (p(c2, 0)?, p(c2, 2)?, p(c2, 4)?);
    let mix = |a: i64, b: i64| (a as f64 + (b - a) as f64 * factor) as i64;
    Ok(format!("#{}{}{}", hex2(mix(r1, r2)), hex2(mix(g1, g2)), hex2(mix(b1, b2))))
}

/// `ensure_line_in_file`: true when the line was added.
fn ensure_line_in_file(path: &Path, line: &str, check: Option<&str>, at_top: bool) -> R<bool> {
    makedirs(path.parent().unwrap_or(Path::new("")))?;
    let mut content = String::new();
    if exists(path) {
        content = read_text(path)?;
        let present = match check {
            Some(p) => re(p).is_match(&content),
            None => content.contains(line),
        };
        if present {
            return Ok(false);
        }
    }
    if at_top {
        content = format!("{line}\n{content}");
    } else {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(line);
        content.push('\n');
    }
    write_text(path, &content)?;
    Ok(true)
}

fn write_out(path: &str, text: &str) -> R<()> {
    makedirs(Path::new(&py::dirname(path)))?;
    write_text(Path::new(path), text)
}

fn hexes(c: &Dict, k: &str, d: &str) -> String {
    c.s(k, d)
}

/// `colors.get(k, d)[1:]`
fn bare(c: &Dict, k: &str, d: &str) -> String {
    pyrt::slice(&c.s(k, d), 1, None)
}

fn kitty(c: &Dict, out: &str, home: &str) -> R<()> {
    let g = |k: &str, d: &str| hexes(c, k, d);
    let inactive_fg = if contrast_hex(&st(&g("term7", "#A89984")), c.get("tabInactiveBg").unwrap())? >= 4.5 { g("term7", "#A89984") } else { g("term15", "#EBDBB2") };
    let config = format!(
        "# Auto-generated by ii wallpaper theming system\n# Do not edit manually - changes will be overwritten\n\n# The basic colors\n\
foreground              {}\nbackground              {}\nselection_foreground    {}\nselection_background    {}\n\n\
# Cursor colors\ncursor                  {}\ncursor_text_color       {}\n\n\
# URL underline color when hovering with mouse\nurl_color               {}\n\n\
# Kitty window border colors\nactive_border_color     {}\ninactive_border_color   {}\nbell_border_color       {}\n\n\
# Tab bar colors\nactive_tab_foreground   {}\nactive_tab_background   {}\ninactive_tab_foreground {}\ninactive_tab_background {}\ntab_bar_background      {}\n\n\
# The 16 terminal colors\n\n# black\ncolor0 {}\ncolor8 {}\n\n# red\ncolor1 {}\ncolor9 {}\n\n# green\ncolor2  {}\ncolor10 {}\n\n\
# yellow\ncolor3  {}\ncolor11 {}\n\n# blue\ncolor4  {}\ncolor12 {}\n\n# magenta\ncolor5  {}\ncolor13 {}\n\n\
# cyan\ncolor6  {}\ncolor14 {}\n\n# white\ncolor7  {}\ncolor15 {}\n",
        g("term15", "#EBDBB2"), g("term0", "#282828"), g("term15", "#EBDBB2"), c.s("selectionBg", ""),
        g("term15", "#EBDBB2"), g("term0", "#282828"), g("term4", "#458588"),
        g("primary", "#458588"), g("term8", "#928374"), g("term1", "#CC241D"),
        g("onPrimary", "#FFFFFF"), g("primary", "#458588"), inactive_fg, c.s("tabInactiveBg", ""), g("term0", "#282828"),
        g("term0", "#282828"), g("term8", "#928374"), g("term1", "#CC241D"), g("term9", "#FB4934"),
        g("term2", "#98971A"), g("term10", "#B8BB26"), g("term3", "#D79921"), g("term11", "#FABD2F"),
        g("term4", "#458588"), g("term12", "#83A598"), g("term5", "#B16286"), g("term13", "#D3869B"),
        g("term6", "#689D6A"), g("term14", "#8EC07C"), g("term7", "#A89984"), g("term15", "#EBDBB2"),
    );
    let dir = py::dirname(out);
    makedirs(Path::new(&dir))?;
    let theme_conf = py::join(&dir, "theme.conf");
    write_text(Path::new(&theme_conf), &config)?;
    let tmp = PathBuf::from(format!("{out}.{}.tmp", std::process::id()));
    if tmp.symlink_metadata().is_ok() {
        std::fs::remove_file(&tmp).map_err(|e| pyrt::os_exc(&e, &tmp))?;
    }
    std::os::unix::fs::symlink("theme.conf", &tmp).map_err(|e| pyrt::os_exc(&e, &tmp))?;
    std::fs::rename(&tmp, out).map_err(|e| pyrt::os_exc(&e, &tmp))?;
    let conf = PathBuf::from(format!("{home}/.config/kitty/kitty.conf"));
    if ensure_line_in_file(&conf, "include current-theme.conf", Some(r"include\s+current-theme\.conf"), false)? {
        println!("✓ Generated Kitty config and auto-integrated");
    } else {
        println!("✓ Generated Kitty config (already integrated)");
    }
    // subprocess.run(..., timeout=2) inside a try: a missing pkill or a timeout prints nothing
    if let Ok(mut child) = Command::new("pkill").args(["--signal", "SIGUSR1", "-x", "kitty"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    println!("  → Live-reloaded Kitty config (SIGUSR1)");
                    break;
                }
                Ok(None) if start.elapsed() < Duration::from_secs(2) => std::thread::sleep(Duration::from_millis(5)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
    }
    Ok(())
}

/// `fix_alacritty_import_order`: (modified, message)
fn fix_alacritty_import_order(path: &Path) -> R<(bool, String)> {
    let content = match read_text(path) {
        Ok(c) => c,
        Err(e) if e.ty == "FileNotFoundError" => return Ok((false, "Config file not found".into())),
        Err(e) if e.ty == "PermissionError" => return Ok((false, "Permission denied".into())),
        Err(e) => return Err(e),
    };
    let import_line = r#"import = ["~/.config/alacritty/colors.toml"]"#;
    let bare_import = re(r"^import\s*=\s*\[.*?colors\.toml.*?\]");
    let general_import = re(r"^import\s*=\s*\[.*?colors\.toml.*?\]");
    let general_option = re(r"^(working_directory|live_config_reload|ipc_socket)\s*=");
    let mut hoisted: Vec<String> = vec![];
    let mut hoisted_keys: Vec<String> = vec![];
    let mut hoist = |line: &str, hoisted: &mut Vec<String>| match general_option.captures(line) {
        None => hoisted.push(line.to_string()),
        Some(m) => {
            if !hoisted_keys.iter().any(|k| k == &m[1]) {
                hoisted_keys.push(m[1].to_string());
                hoisted.push(line.to_string());
            }
        }
    };
    let has_hardcoded = re(r"(?m)^\[colors\.(primary|normal|bright)\]").is_match(&content);
    let lines: Vec<&str> = content.split('\n').collect();
    let top: Vec<&str> = lines.iter().take(10).map(|l| py::strip(l)).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let misplaced = re(r"(?m)^\s*(working_directory|live_config_reload|ipc_socket)\s*=").is_match(&content)
        && !re(r"(?m)^\s*\[general\](?:\n|\r\n)(?:[^\[]*\n|\r\n)*(?:\s*(working_directory|live_config_reload|ipc_socket)\s*=)").is_match(&content);
    let correct = top.len() >= 2 && top[0] == "[general]" && general_import.is_match(top[1]) && !has_hardcoded && !misplaced;
    if correct {
        return Ok((false, "Config is already correct".into()));
    }
    let backup = format!("{}.backup_{}", path.display(), pyrt::now_strftime("%Y%m%d_%H%M%S"));
    if let Err(e) = write_text(Path::new(&backup), &content) {
        return Ok((false, format!("Failed to create backup: {}", e.msg)));
    }
    let mut out: Vec<String> = vec!["# iNiR wallpaper theming - import must be at top to override colors".into(), "[general]".into(), import_line.into()];
    let (mut in_general, mut in_colors, mut added_comment) = (false, false, false);
    let colors_header = re(r"^\[colors\.(primary|normal|bright|cursor|selection)\]");
    for line in &lines {
        let stripped = py::strip(line);
        if bare_import.is_match(stripped) || stripped.starts_with("# iNiR wallpaper theming") {
            continue;
        }
        if stripped == "[general]" {
            in_general = true;
            continue;
        }
        if in_general {
            if stripped.starts_with('[') && stripped != "[general]" {
                in_general = false;
            } else if general_import.is_match(stripped) {
                continue;
            } else if !stripped.is_empty() {
                hoist(stripped, &mut hoisted);
                continue;
            } else {
                continue;
            }
        }
        if general_option.is_match(stripped) {
            hoist(stripped, &mut hoisted);
            continue;
        }
        if colors_header.is_match(stripped) {
            if !added_comment {
                in_colors = true;
                added_comment = true;
                out.push(String::new());
                out.push("# Color definitions commented out by iNiR wallpaper theming".into());
                out.push("# Colors are managed via the import in [general] above".into());
                out.push("#".into());
            }
            out.push(format!("# {line}"));
            continue;
        }
        if in_colors {
            if stripped.starts_with('[') && !stripped.starts_with("[colors") {
                in_colors = false;
                out.push(String::new());
                out.push(line.to_string());
            } else if !stripped.is_empty() && stripped.contains('=') {
                out.push(format!("# {line}"));
            } else {
                out.push(if stripped.is_empty() { String::new() } else { format!("# {line}") });
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !hoisted.is_empty() {
        let rest = out.split_off(3);
        out.extend(hoisted);
        out.push(String::new());
        out.extend(rest);
    }
    if let Err(e) = write_text(path, &out.join("\n")) {
        write_text(path, &content)?;
        return Ok((false, format!("Failed to write config: {}", e.msg)));
    }
    Ok((true, format!("Fixed config (backup: {backup})")))
}

fn alacritty(c: &Dict, out: &str, home: &str) -> R<()> {
    let g = |k: &str, d: &str| hexes(c, k, d);
    let config = format!(
        "# Auto-generated by ii wallpaper theming system\n# Do not edit manually - changes will be overwritten\n\n\
[colors.primary]\nbackground = '{}'\nforeground = '{}'\n\n[colors.cursor]\ntext   = '{}'\ncursor = '{}'\n\n\
[colors.selection]\ntext       = '{}'\nbackground = '{}'\n\n[colors.normal]\nblack   = '{}'\nred     = '{}'\ngreen   = '{}'\n\
yellow  = '{}'\nblue    = '{}'\nmagenta = '{}'\ncyan    = '{}'\nwhite   = '{}'\n\n[colors.bright]\nblack   = '{}'\nred     = '{}'\n\
green   = '{}'\nyellow  = '{}'\nblue    = '{}'\nmagenta = '{}'\ncyan    = '{}'\nwhite   = '{}'\n",
        g("term0", "#282828"), g("term15", "#EBDBB2"), g("term0", "#282828"), g("term15", "#EBDBB2"),
        g("term15", "#EBDBB2"), c.s("selectionBg", ""),
        g("term0", "#282828"), g("term1", "#CC241D"), g("term2", "#98971A"), g("term3", "#D79921"),
        g("term4", "#458588"), g("term5", "#B16286"), g("term6", "#689D6A"), g("term7", "#A89984"),
        g("term8", "#928374"), g("term9", "#FB4934"), g("term10", "#B8BB26"), g("term11", "#FABD2F"),
        g("term12", "#83A598"), g("term13", "#D3869B"), g("term14", "#8EC07C"), g("term15", "#EBDBB2"),
    );
    write_out(out, &config)?;
    let conf = PathBuf::from(format!("{home}/.config/alacritty/alacritty.toml"));
    if exists(&conf) {
        let (_, message) = fix_alacritty_import_order(&conf)?;
        println!("✓ Generated Alacritty config ({message})");
    } else {
        makedirs(conf.parent().unwrap())?;
        write_text(&conf, "# iNiR wallpaper theming\n[general]\nimport = [\"~/.config/alacritty/colors.toml\"]\n")?;
        println!("✓ Generated Alacritty config and created new config file");
    }
    Ok(())
}

fn foot(c: &Dict, out: &str, home: &str) -> R<()> {
    let b = |k: &str, d: &str| bare(c, k, d);
    let config = format!(
        "# Auto-generated by ii wallpaper theming system\n# Do not edit manually - changes will be overwritten\n\n\
[colors-dark]\nforeground={}\nbackground={}\n\n## Normal/regular colors (color palette 0-7)\n\
regular0={}  # black\nregular1={}  # red\nregular2={}  # green\nregular3={}  # yellow\nregular4={}  # blue\n\
regular5={}  # magenta\nregular6={}  # cyan\nregular7={}  # white\n\n## Bright colors (color palette 8-15)\n\
bright0={}   # bright black\nbright1={}   # bright red\nbright2={}  # bright green\nbright3={}  # bright yellow\n\
bright4={}  # bright blue\nbright5={}  # bright magenta\nbright6={}  # bright cyan\nbright7={}  # bright white\n\n\
## Cursor and selection colors\nselection-foreground={}\nselection-background={}\njump-labels={} {}\nurls={}\n",
        b("term15", "#EBDBB2"), b("term0", "#282828"),
        b("term0", "#282828"), b("term1", "#CC241D"), b("term2", "#98971A"), b("term3", "#D79921"),
        b("term4", "#458588"), b("term5", "#B16286"), b("term6", "#689D6A"), b("term7", "#A89984"),
        b("term8", "#928374"), b("term9", "#FB4934"), b("term10", "#B8BB26"), b("term11", "#FABD2F"),
        b("term12", "#83A598"), b("term13", "#D3869B"), b("term14", "#8EC07C"), b("term15", "#EBDBB2"),
        b("term15", "#EBDBB2"), pyrt::slice(&c.s("selectionBg", ""), 1, None), b("term0", "#282828"), b("term3", "#D79921"),
        b("term4", "#458588"),
    );
    write_out(out, &config)?;
    let foot_path = PathBuf::from(format!("{home}/.config/foot/foot.ini"));
    makedirs(foot_path.parent().unwrap())?;
    let content = if exists(&foot_path) { read_text(&foot_path)? } else { String::new() };
    let stripped = sub(&re(r"(?m)^include\s*=.*(?:/|^)(?:inir-)?colors\.ini\s*$"), "", &content, 0)?;
    let updated = format!("include=~/.config/foot/inir-colors.ini\n{}", stripped.trim_start_matches('\n'));
    if updated != content {
        write_text(&foot_path, &updated)?;
        println!("✓ Generated Foot config and auto-integrated");
    } else {
        println!("✓ Generated Foot config (already integrated)");
    }
    Ok(())
}

fn wezterm(c: &Dict, out: &str, home: &str) -> R<()> {
    let g = |k: &str, d: &str| hexes(c, k, d);
    let config = format!(
        r#"-- Auto-generated by ii wallpaper theming system
-- Do not edit manually - changes will be overwritten

return {{
  foreground = '{}',
  background = '{}',

  cursor_bg = '{}',
  cursor_fg = '{}',
  cursor_border = '{}',

  selection_fg = '{}',
  selection_bg = '{}',

  scrollbar_thumb = '{}',
  split = '{}',

  ansi = {{
    '{}',  -- black
    '{}',  -- red
    '{}',  -- green
    '{}',  -- yellow
    '{}',  -- blue
    '{}',  -- magenta
    '{}',  -- cyan
    '{}',  -- white
  }},

  brights = {{
    '{}',  -- bright black
    '{}',  -- bright red
    '{}', -- bright green
    '{}', -- bright yellow
    '{}', -- bright blue
    '{}', -- bright magenta
    '{}', -- bright cyan
    '{}', -- bright white
  }},

  tab_bar = {{
    background = '{}',
    active_tab = {{
      bg_color = '{}',
      fg_color = '{}',
    }},
    inactive_tab = {{
      bg_color = '{}',
      fg_color = '{}',
    }},
    inactive_tab_hover = {{
      bg_color = '{}',
      fg_color = '{}',
    }},
  }},
}}
"#,
        g("term15", "#EBDBB2"), g("term0", "#282828"), g("term15", "#EBDBB2"), g("term0", "#282828"), g("term15", "#EBDBB2"),
        g("term15", "#EBDBB2"), c.s("selectionBg", ""), g("term8", "#928374"), g("term8", "#928374"),
        g("term0", "#282828"), g("term1", "#CC241D"), g("term2", "#98971A"), g("term3", "#D79921"),
        g("term4", "#458588"), g("term5", "#B16286"), g("term6", "#689D6A"), g("term7", "#A89984"),
        g("term8", "#928374"), g("term9", "#FB4934"), g("term10", "#B8BB26"), g("term11", "#FABD2F"),
        g("term12", "#83A598"), g("term13", "#D3869B"), g("term14", "#8EC07C"), g("term15", "#EBDBB2"),
        g("term0", "#282828"), g("primary", "#458588"), g("onPrimary", "#FFFFFF"),
        g("term8", "#928374"), g("term7", "#A89984"), g("term8", "#928374"), g("term15", "#EBDBB2"),
    );
    write_out(out, &config)?;
    let path = PathBuf::from(format!("{home}/.config/wezterm/wezterm.lua"));
    if exists(&path) {
        let content = read_text(&path)?;
        if !content.contains("require('colors')") {
            let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
            if let Some(i) = lines.iter().position(|l| l.contains("return config") || l.contains("return {")) {
                lines.insert(i, "local colors = require('colors')".into());
                lines.insert(i + 1, "config.colors = colors".into());
            }
            write_text(&path, &lines.join("\n"))?;
            println!("✓ Generated WezTerm config and auto-integrated");
        } else {
            println!("✓ Generated WezTerm config (already integrated)");
        }
    } else {
        makedirs(path.parent().unwrap())?;
        write_text(&path, "local wezterm = require('wezterm')\nlocal config = wezterm.config_builder()\nlocal colors = require('colors')\nconfig.colors = colors\nreturn config\n")?;
        println!("✓ Generated WezTerm config and auto-integrated (created config)");
    }
    Ok(())
}

fn ghostty(c: &Dict, out: &str, home: &str) -> R<()> {
    let g = |k: &str, d: &str| hexes(c, k, d);
    let config = format!(
        "# Auto-generated by ii wallpaper theming system\n# Do not edit manually - changes will be overwritten\n\n\
background = {}\nforeground = {}\n\ncursor-color = {}\ncursor-text = {}\n\nselection-background = {}\nselection-foreground = {}\n\n\
# Black\npalette = 0={}\npalette = 8={}\n\n# Red\npalette = 1={}\npalette = 9={}\n\n# Green\npalette = 2={}\npalette = 10={}\n\n\
# Yellow\npalette = 3={}\npalette = 11={}\n\n# Blue\npalette = 4={}\npalette = 12={}\n\n# Magenta\npalette = 5={}\npalette = 13={}\n\n\
# Cyan\npalette = 6={}\npalette = 14={}\n\n# White\npalette = 7={}\npalette = 15={}\n",
        g("term0", "#282828"), g("term15", "#EBDBB2"), g("term15", "#EBDBB2"), g("term0", "#282828"),
        c.s("selectionBg", ""), g("term15", "#EBDBB2"),
        g("term0", "#282828"), g("term8", "#928374"), g("term1", "#CC241D"), g("term9", "#FB4934"),
        g("term2", "#98971A"), g("term10", "#B8BB26"), g("term3", "#D79921"), g("term11", "#FABD2F"),
        g("term4", "#458588"), g("term12", "#83A598"), g("term5", "#B16286"), g("term13", "#D3869B"),
        g("term6", "#689D6A"), g("term14", "#8EC07C"), g("term7", "#A89984"), g("term15", "#EBDBB2"),
    );
    write_out(out, &config)?;
    let conf = PathBuf::from(format!("{home}/.config/ghostty/config"));
    if ensure_line_in_file(&conf, "theme = ii-auto", Some(r"theme\s*=\s*ii-auto"), false)? {
        println!("✓ Generated Ghostty config and auto-integrated");
    } else {
        println!("✓ Generated Ghostty config (already integrated)");
    }
    Ok(())
}

fn konsole(c: &Dict, out: &str) -> R<()> {
    let b = |k: &str, d: &str| bare(c, k, d);
    let mut config = format!(
        "[Background]\nColor={}\n\n[BackgroundIntense]\nColor={}\n\n[Foreground]\nColor={}\n\n[ForegroundIntense]\nColor={}\n\n",
        b("term0", "#282828"), b("term0", "#282828"), b("term15", "#EBDBB2"), b("term15", "#EBDBB2")
    );
    let pairs = [("term0", "#282828", "term8", "#928374"), ("term1", "#CC241D", "term9", "#FB4934"), ("term2", "#98971A", "term10", "#B8BB26"),
        ("term3", "#D79921", "term11", "#FABD2F"), ("term4", "#458588", "term12", "#83A598"), ("term5", "#B16286", "term13", "#D3869B"),
        ("term6", "#689D6A", "term14", "#8EC07C"), ("term7", "#A89984", "term15", "#EBDBB2")];
    for (i, (k, d, ki, di)) in pairs.iter().enumerate() {
        config.push_str(&format!("[Color{i}]\nColor={}\n\n[Color{i}Intense]\nColor={}\n\n", b(k, d), b(ki, di)));
    }
    config.push_str("[General]\nDescription=ii Auto-generated Theme\nOpacity=1.0\nWallpaper=\n");
    write_out(out, &config)?;
    println!("✓ Generated Konsole config");
    Ok(())
}

fn starship(c: &Dict, out: &str, home: &str) -> R<()> {
    let j = |k: &str, d: &str| c.or(k, st(d));
    let red = blend_hex(&j("term1", "#CC241D"), &j("secondary", "#83A598"), 0.45)?;
    let green = blend_hex(&j("term2", "#98971A"), &j("primary", "#458588"), 0.45)?;
    let yellow = blend_hex(&j("term3", "#D79921"), &j("tertiary", "#D3869B"), 0.45)?;
    let blue = blend_hex(&j("term4", "#458588"), &j("primary", "#458588"), 0.45)?;
    let magenta = blend_hex(&j("term5", "#B16286"), &j("secondary", "#83A598"), 0.45)?;
    let cyan = blend_hex(&j("term6", "#689D6A"), &j("tertiary", "#D3869B"), 0.45)?;
    let bg = j("term0", "#282828");
    let red = blend_hex(&st(&red), &bg, 0.20)?;
    let green = blend_hex(&st(&green), &bg, 0.20)?;
    let yellow = blend_hex(&st(&yellow), &bg, 0.20)?;
    let blue = blend_hex(&st(&blue), &bg, 0.20)?;
    let magenta = blend_hex(&st(&magenta), &bg, 0.20)?;
    let cyan = blend_hex(&st(&cyan), &bg, 0.20)?;
    let g = |k: &str, d: &str| hexes(c, k, d);
    let on_surface = c.get("term15").map(pyrt::str_of).unwrap_or_else(|| g("onSurface", "#EBDBB2"));
    let config = format!(
        "# Auto-generated by ii wallpaper theming system\n# Do not edit manually - changes will be overwritten\n\
# This file provides Material You colors to your Starship prompt\n\n[palettes.ii]\nprimary = '{}'\nonPrimary = '{}'\n\
secondary = '{}'\nonSecondary = '{}'\ntertiary = '{}'\nonTertiary = '{}'\nsurface = '{}'\nonSurface = '{}'\nbackground = '{}'\n\
foreground = '{}'\nblack = '{}'\nred = '{red}'\ngreen = '{green}'\nyellow = '{yellow}'\nblue = '{blue}'\nmagenta = '{magenta}'\n\
cyan = '{cyan}'\nwhite = '{}'\nbright_black = '{}'\nbright_red = '{}'\nbright_green = '{}'\nbright_yellow = '{}'\n\
bright_blue = '{}'\nbright_magenta = '{}'\nbright_cyan = '{}'\nbright_white = '{}'\n",
        g("primary", "#458588"), g("onPrimary", "#FFFFFF"), g("secondary", "#83A598"), g("onSecondary", "#1D2021"),
        g("tertiary", "#D3869B"), g("onTertiary", "#1D2021"), g("surface", "#1D2021"), on_surface, g("term0", "#282828"),
        g("term15", "#EBDBB2"), g("term0", "#282828"), g("term7", "#A89984"), g("term8", "#928374"), g("term9", "#FB4934"),
        g("term10", "#B8BB26"), g("term11", "#FABD2F"), g("term12", "#83A598"), g("term13", "#D3869B"), g("term14", "#8EC07C"),
        g("term15", "#EBDBB2"),
    );
    write_out(out, &config)?;
    let conf = PathBuf::from(format!("{home}/.config/starship.toml"));
    if !exists(&conf) {
        println!("✓ Generated Starship palette (starship.toml not found - create it and add 'palette = \"ii\"')");
        return Ok(());
    }
    let mut content = read_text(&conf)?;
    if content.contains("palette = \"ii\"") {
        println!("✓ Generated Starship palette (already using ii palette)");
    } else if !content.contains("palette =") {
        let new_content = format!("palette = \"ii\"\n\n{content}");
        write_text(&conf, &new_content)?;
        content = new_content;
        println!("✓ Generated Starship palette and set as active");
    } else {
        println!("✓ Generated Starship palette (using different palette, change to 'palette = \"ii\"' to use)");
    }
    if content.contains("[palettes.ii]") {
        let block: Vec<&str> = py::strip(&config).split('\n').collect();
        let start = block.iter().position(|l| l.starts_with("[palettes.ii]")).unwrap();
        // re.sub(r"\[palettes\.ii\].*?(?=\n\[|\Z)", block, content, flags=re.DOTALL), lookahead done by hand
        let repl = pyrt::template_text(&block[start..].join("\n"))?;
        let mut new_content = String::with_capacity(content.len());
        let mut pos = 0;
        while let Some(off) = content[pos..].find("[palettes.ii]") {
            let s = pos + off;
            let after = s + "[palettes.ii]".len();
            let e = content[after..].find("\n[").map_or(content.len(), |k| after + k);
            new_content.push_str(&content[pos..s]);
            new_content.push_str(&repl);
            pos = e;
        }
        new_content.push_str(&content[pos..]);
        if new_content != content {
            write_text(&conf, &new_content)?;
            println!("  → Updated ii palette in starship.toml");
        }
    } else {
        pyrt::append_text(&conf, &format!("\n{config}"))?;
        println!("  → Appended ii palette to starship.toml");
    }
    Ok(())
}

fn btop(c: &Dict, out: &str, home: &str) -> R<()> {
    let bg = c.or("term0", c.or("surface", c.or("background", Json::Null)));
    let surface_low = blend_hex(&bg, &c.or("term15", st("#EBDBB2")), 0.06)?;
    let _surface_std = c.or("surface_container", Json::Null);
    let surface_high = blend_hex(&bg, &c.or("term15", st("#EBDBB2")), 0.14)?;
    let s = |v: Json| pyrt::str_of(&v);
    let on_surface = s(c.or("term15", c.or("on_surface", Json::Null)));
    let on_variant = s(c.or("term7", c.or("on_surface_variant", Json::Null)));
    let outline_variant = s(c.or("outline_variant", Json::Null));
    let primary = s(c.or("primary", Json::Null));
    let primary_dim = s(c.or("primary_fixed_dim", Json::Null));
    let secondary_dim = s(c.or("secondary_fixed_dim", Json::Null));
    let tertiary_dim = s(c.or("tertiary_fixed_dim", Json::Null));
    let error = s(c.or("error", Json::Null));
    let bg = s(bg);
    let config = format!(
        "# Auto-generated by ii wallpaper theming system\n# Do not edit manually\n\n\
theme[main_bg]=\"{bg}\"\ntheme[main_fg]=\"{on_surface}\"\ntheme[title]=\"{on_surface}\"\ntheme[hi_fg]=\"{primary}\"\n\
theme[selected_bg]=\"{surface_high}\"\ntheme[selected_fg]=\"{on_surface}\"\ntheme[inactive_fg]=\"{on_variant}\"\n\
theme[graph_text]=\"{on_variant}\"\ntheme[meter_bg]=\"{surface_low}\"\ntheme[proc_misc]=\"{on_variant}\"\n\n\
theme[cpu_box]=\"{outline_variant}\"\ntheme[mem_box]=\"{outline_variant}\"\ntheme[net_box]=\"{outline_variant}\"\n\
theme[proc_box]=\"{outline_variant}\"\ntheme[div_line]=\"{outline_variant}\"\n\n\
theme[temp_start]=\"{tertiary_dim}\"\ntheme[temp_mid]=\"{secondary_dim}\"\ntheme[temp_end]=\"{error}\"\n\n\
theme[cpu_start]=\"{tertiary_dim}\"\ntheme[cpu_mid]=\"{secondary_dim}\"\ntheme[cpu_end]=\"{error}\"\n\n\
theme[free_start]=\"{tertiary_dim}\"\ntheme[free_mid]=\"{tertiary_dim}\"\ntheme[free_end]=\"{tertiary_dim}\"\n\
theme[cached_start]=\"{primary_dim}\"\ntheme[cached_mid]=\"{primary_dim}\"\ntheme[cached_end]=\"{primary_dim}\"\n\
theme[available_start]=\"{secondary_dim}\"\ntheme[available_mid]=\"{secondary_dim}\"\ntheme[available_end]=\"{secondary_dim}\"\n\
theme[used_start]=\"{tertiary_dim}\"\ntheme[used_mid]=\"{secondary_dim}\"\ntheme[used_end]=\"{error}\"\n\n\
theme[download_start]=\"{tertiary_dim}\"\ntheme[download_mid]=\"{tertiary_dim}\"\ntheme[download_end]=\"{tertiary_dim}\"\n\
theme[upload_start]=\"{secondary_dim}\"\ntheme[upload_mid]=\"{secondary_dim}\"\ntheme[upload_end]=\"{secondary_dim}\"\n\n\
theme[process_start]=\"{primary_dim}\"\ntheme[process_mid]=\"{primary_dim}\"\ntheme[process_end]=\"{primary_dim}\"\n"
    );
    write_out(out, &config)?;
    let conf = PathBuf::from(format!("{home}/.config/btop/btop.conf"));
    let new_line = "color_theme = \"ii-auto\"";
    if exists(&conf) {
        let content = read_text(&conf)?;
        if re(r"(?m)^color_theme\s*=").is_match(&content) {
            let new_content = sub(&re(r"(?m)^color_theme\s*=.*$"), new_line, &content, 0)?;
            if new_content != content {
                write_text(&conf, &new_content)?;
                println!("✓ Generated btop theme and updated btop.conf");
            } else {
                println!("✓ Generated btop theme (already using ii-auto)");
            }
        } else {
            pyrt::append_text(&conf, &format!("\n{new_line}\n"))?;
            println!("✓ Generated btop theme and added to btop.conf");
        }
    } else {
        makedirs(conf.parent().unwrap())?;
        write_text(&conf, "color_theme = \"ii-auto\"\n")?;
        println!("✓ Generated btop theme and created btop.conf");
    }
    Ok(())
}

fn omp(c: &Dict, out: &str) -> R<()> {
    let term0 = c.get("term0").cloned().unwrap_or(Json::Null);
    let term15 = c.get("term15").cloned().unwrap_or(Json::Null);
    let both = term0.truthy() && term15.truthy();
    let surface_container = if both { st(&blend_hex(&term0, &term15, 0.08)?) } else { c.or("surface_container", Json::Null) };
    let surface_container_high = if both { st(&blend_hex(&term0, &term15, 0.14)?) } else { c.or("surface_container_high", Json::Null) };
    let on_surface = if term15.truthy() { term15.clone() } else { c.or("on_surface", Json::Null) };
    let on_variant = c.or("term7", c.or("on_surface_variant", Json::Null));
    let g = |k: &str| c.or(k, Json::Null);
    let o = |pairs: Vec<(&str, Json)>| Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    let theme = o(vec![
        ("$schema", st("https://raw.githubusercontent.com/JanDeDobbeleer/oh-my-posh/main/themes/schema.json")),
        ("version", Json::Int(4)),
        ("blocks", Json::Arr(vec![
            o(vec![
                ("type", st("prompt")),
                ("alignment", st("left")),
                ("segments", Json::Arr(vec![
                    o(vec![("type", st("executiontime")), ("style", st("plain")), ("foreground", on_variant.clone()),
                        ("background", surface_container_high), ("template", st(" {{ .FormattedMs }} ")),
                        ("properties", o(vec![("style", st("austin")), ("threshold", Json::Int(500))]))]),
                    o(vec![("type", st("status")), ("style", st("plain")), ("foreground", g("on_error_container")),
                        ("background", g("error_container")), ("template", st(" {{ if gt .Code 0 }}✗ {{ .Code }}{{ end }} "))]),
                    o(vec![("type", st("path")), ("style", st("plain")), ("foreground", on_surface), ("background", surface_container),
                        ("template", st(" {{ .Path }} ")), ("properties", o(vec![("style", st("full")), ("folder_separator_icon", st("/"))]))]),
                    o(vec![("type", st("git")), ("style", st("plain")), ("foreground", g("on_primary_container")),
                        ("background", g("primary_container")),
                        ("template", st(" {{ .HEAD }}{{ if .Working.Changed }} ●{{ end }}{{ if .Staging.Changed }} ✚{{ end }} ")),
                        ("properties", o(vec![("branch_icon", st("")), ("fetch_status", Json::Bool(true))]))]),
                ])),
            ]),
            o(vec![
                ("type", st("rprompt")),
                ("segments", Json::Arr(vec![o(vec![("type", st("time")), ("style", st("plain")), ("foreground", on_variant),
                    ("template", st(" {{ .CurrentDate | date \"15:04\" }} "))])])),
            ]),
            o(vec![
                ("type", st("prompt")),
                ("alignment", st("left")),
                ("newline", Json::Bool(true)),
                ("segments", Json::Arr(vec![o(vec![("type", st("text")), ("style", st("plain")), ("foreground", g("primary")),
                    ("template", st("❯ "))])])),
            ]),
        ])),
    ]);
    write_out(out, &py::json_dump(&theme))?;
    println!("✓ Generated oh-my-posh theme");
    Ok(())
}

fn lazygit(c: &Dict, out: &str, home: &str) -> R<()> {
    let primary = c.s("primary", "#458588");
    let fg = c.s("term15", "#EBDBB2");
    let gray = c.s("term8", "#928374");
    let sel_bg = c.get("surfaceContainer").map(pyrt::str_of).unwrap_or_else(|| c.s("term8", "#3C3836"));
    let red = c.s("term1", "#CC241D");
    let yellow = c.s("term3", "#D79921");
    let theme_yaml = format!(
        r#"    theme:
      activeBorderColor:
        - "{primary}"
        - bold
      inactiveBorderColor:
        - "{gray}"
      optionsTextColor:
        - "{primary}"
      selectedLineBgColor:
        - "{sel_bg}"
      selectedRangeBgColor:
        - "{sel_bg}"
      cherryPickedCommitFgColor:
        - "{primary}"
      cherryPickedCommitBgColor:
        - "{sel_bg}"
      markedBaseCommitFgColor:
        - "{yellow}"
      markedBaseCommitBgColor:
        - "{sel_bg}"
      unstagedChangesColor:
        - "{red}"
      defaultFgColor:
        - "{fg}"
"#
    );
    let config_path = PathBuf::from(format!("{home}/.config/lazygit/config.yml"));
    write_out(out, &format!("# Auto-generated by ii wallpaper theming system\n# This is the theme section for lazygit config.yml\ngui:\n{theme_yaml}\n"))?;
    if exists(&config_path) {
        let content = read_text(&config_path)?;
        if re(r"(?m)^\s*theme:").is_match(&content) {
            let new_content = sub(&re(r"(    theme:\n(?:      .*\n)*(?:        .*\n)*)"), &format!("{theme_yaml}\n"), &content, 1)?;
            if new_content != content {
                write_text(&config_path, &new_content)?;
                println!("✓ Generated lazygit theme and updated config.yml");
            } else {
                println!("✓ Generated lazygit theme (config.yml unchanged)");
            }
        } else if re(r"(?m)^gui:").is_match(&content) {
            let new_content = sub(&re(r"(?m)^(gui:.*)"), &format!("\\1\\n{theme_yaml}"), &content, 1)?;
            write_text(&config_path, &new_content)?;
            println!("✓ Generated lazygit theme and added to gui section");
        } else {
            pyrt::append_text(&config_path, &format!("\ngui:\n{theme_yaml}\n"))?;
            println!("✓ Generated lazygit theme and appended gui section");
        }
    } else {
        makedirs(config_path.parent().unwrap())?;
        write_text(&config_path, &format!("gui:\n{theme_yaml}\n"))?;
        println!("✓ Generated lazygit config with theme");
    }
    Ok(())
}

fn yazi(c: &Dict, out: &str, home: &str) -> R<()> {
    let bg = c.s("term0", "#282828");
    let fg = c.s("term15", "#EBDBB2");
    let fg_dim = c.s("term7", "#A89984");
    let gray = c.s("term8", "#928374");
    let primary = c.s("primary", "#458588");
    let sel_bg = c.get("surfaceContainer").map(pyrt::str_of).unwrap_or_else(|| c.s("term8", "#3C3836"));
    let red = c.s("term1", "#CC241D");
    let green = c.s("term2", "#98971A");
    let yellow = c.s("term3", "#D79921");
    let magenta = c.s("term5", "#B16286");
    let cyan = c.s("term6", "#689D6A");
    let config = format!(
        r#"# Auto-generated by ii wallpaper theming system
# Do not edit manually - changes will be overwritten

[mgr]
cwd = {{ fg = "{primary}" }}
hovered         = {{ fg = "{bg}", bg = "{primary}" }}
preview_hovered = {{ underline = true }}
find_keyword    = {{ fg = "{yellow}", italic = true }}
find_position   = {{ fg = "{magenta}", bg = "reset", italic = true }}
marker_selected = {{ fg = "{green}", bg = "{green}" }}
marker_copied   = {{ fg = "{yellow}", bg = "{yellow}" }}
marker_cut      = {{ fg = "{red}", bg = "{red}" }}
tab_active      = {{ fg = "{bg}", bg = "{sel_bg}" }}
tab_inactive    = {{ fg = "{fg_dim}", bg = "{sel_bg}" }}
tab_width       = 1
border_symbol   = "│"
border_style    = {{ fg = "{gray}" }}
count_copied    = {{ fg = "{bg}", bg = "{yellow}" }}
count_cut       = {{ fg = "{bg}", bg = "{red}" }}
count_selected  = {{ fg = "{bg}", bg = "{primary}" }}

[mode]
normal_main = {{ fg = "{bg}", bg = "{primary}", bold = true }}
normal_alt  = {{ fg = "{bg}", bg = "{primary}", bold = true }}
select_main = {{ fg = "{bg}", bg = "{green}", bold = true }}
select_alt  = {{ fg = "{bg}", bg = "{green}", bold = true }}
unset_main  = {{ fg = "{bg}", bg = "{magenta}", bold = true }}
unset_alt   = {{ fg = "{bg}", bg = "{magenta}", bold = true }}

[status]
separator_open  = ""
separator_close = ""
separator_style = {{ fg = "{sel_bg}", bg = "{sel_bg}" }}
progress_label  = {{ fg = "{fg}", bold = true }}
progress_normal = {{ fg = "{primary}", bg = "{sel_bg}" }}
progress_error  = {{ fg = "{red}", bg = "{sel_bg}" }}
permissions_t   = {{ fg = "{primary}" }}
permissions_r   = {{ fg = "{yellow}" }}
permissions_w   = {{ fg = "{red}" }}
permissions_x   = {{ fg = "{green}" }}
permissions_s   = {{ fg = "{gray}" }}

[input]
border   = {{ fg = "{primary}" }}
title    = {{}}
value    = {{}}
selected = {{ reversed = true }}

[select]
border   = {{ fg = "{primary}" }}
active   = {{ fg = "{magenta}", bold = true }}
inactive = {{}}

[tasks]
border  = {{ fg = "{primary}" }}
title   = {{}}
hovered = {{ underline = true }}

[which]
mask            = {{ bg = "{sel_bg}" }}
cand            = {{ fg = "{cyan}" }}
rest            = {{ fg = "{gray}" }}
desc            = {{ fg = "{magenta}" }}
separator       = "  "
separator_style = {{ fg = "{gray}" }}

[help]
on      = {{ fg = "{magenta}" }}
run     = {{ fg = "{cyan}" }}
desc    = {{ fg = "{gray}" }}
hovered = {{ bg = "{sel_bg}", bold = true }}
footer  = {{ fg = "{sel_bg}", bg = "{fg_dim}" }}

[filetype]
rules = [
  {{ mime = "image/*", fg = "{magenta}" }},
  {{ mime = "video/*", fg = "{cyan}" }},
  {{ mime = "audio/*", fg = "{cyan}" }},
  {{ mime = "application/zip",             fg = "{red}" }},
  {{ mime = "application/gzip",            fg = "{red}" }},
  {{ mime = "application/x-tar",           fg = "{red}" }},
  {{ mime = "application/x-bzip",          fg = "{red}" }},
  {{ mime = "application/x-bzip2",         fg = "{red}" }},
  {{ mime = "application/x-7z-compressed", fg = "{red}" }},
  {{ mime = "application/x-rar",           fg = "{red}" }},
  {{ name = "*",  fg = "{fg_dim}" }},
  {{ name = "*/", fg = "{primary}" }},
]
"#
    );
    write_out(out, &config)?;
    let theme = PathBuf::from(format!("{home}/.config/yazi/theme.toml"));
    let flavor = "use = \"ii-auto\"";
    if exists(&theme) {
        let content = read_text(&theme)?;
        if content.contains("[flavor]") {
            if re(r#"(?m)^use\s*=\s*"ii-auto""#).is_match(&content) {
                println!("✓ Generated yazi flavor (already using ii-auto)");
            } else if re(r"(?m)^use\s*=").is_match(&content) {
                write_text(&theme, &sub(&re(r"(?m)^(use\s*=).*$"), "use = \"ii-auto\"", &content, 1)?)?;
                println!("✓ Generated yazi flavor and updated theme.toml");
            } else {
                write_text(&theme, &content.replace("[flavor]", &format!("[flavor]\n{flavor}")))?;
                println!("✓ Generated yazi flavor and added use directive");
            }
        } else {
            pyrt::append_text(&theme, &format!("\n[flavor]\n{flavor}\n"))?;
            println!("✓ Generated yazi flavor and added [flavor] section");
        }
    } else {
        makedirs(theme.parent().unwrap())?;
        write_text(&theme, &format!("[flavor]\n{flavor}\n"))?;
        println!("✓ Generated yazi flavor and created theme.toml");
    }
    Ok(())
}

fn fuzzel(c: &Dict, out: &str) -> R<()> {
    let rgba = |k: &str, fb: &str| {
        let pick = [c.get(k), c.get(fb)].into_iter().flatten().find(|v| v.truthy()).map(pyrt::str_of).unwrap_or_else(|| "#808080".into());
        format!("{}ff", pyrt::slice(pick.trim_start_matches('#'), 0, Some(6)).to_lowercase())
    };
    let fg = rgba("app_foreground", "on_surface");
    let subtext = rgba("app_subtext", "on_surface_variant");
    let accent = rgba("app_accent", "primary");
    let config = format!(
        "[colors]\nbackground={}\ntext={fg}\ninput={fg}\nprompt={accent}\nplaceholder={subtext}\nmessage={subtext}\ncounter={subtext}\n\
match={accent}\nselection={}\nselection-text={}\nselection-match={accent}\nborder={}\n",
        rgba("app_popover_bg", "surface_container"), rgba("app_selection", "secondary_container"),
        rgba("app_on_selection", "on_secondary_container"), rgba("app_border_subtle", "outline_variant"),
    );
    let path = Path::new(out);
    if !exists(path) || read_text(path)? != config {
        makedirs(path.parent().unwrap())?;
        write_text(path, &config)?;
    }
    let name = path.file_name().unwrap().to_string_lossy();
    ensure_line_in_file(&path.parent().unwrap().join("fuzzel.ini"), &format!("include=~/.config/fuzzel/{name}"),
        Some(&format!(r"(?m)^\s*include\s*=.*{}", regex::escape(&name))), true)?;
    println!("✓ Generated Fuzzel theme");
    Ok(())
}

/// `{**colors, **json.load(open(path))}`, plus terminal.json for omp; FileNotFoundError means the warning path.
fn merged(colors: &Dict, palette: &str, terminal: Option<&str>) -> R<Dict> {
    let m3 = pyrt::json_load(Path::new(palette))?;
    let mut d = colors.clone();
    match m3 {
        Json::Obj(_) => d.update(&Dict::from_json(m3)?),
        other => return Err(exc("TypeError", format!("'{}' object is not a mapping", pyrt::type_name(&other)))),
    }
    if let Some(t) = terminal {
        if exists(Path::new(t)) {
            match pyrt::json_load(Path::new(t))? {
                Json::Obj(o) => d.update(&Dict(o)),
                other => return Err(exc("TypeError", format!("'{}' object is not iterable", pyrt::type_name(&other)))),
            }
        }
    }
    Ok(d)
}

struct Args {
    scss: String,
    colors: String,
    terminal_json: String,
    terminals: Option<Vec<String>>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let home_state = |f: &str| py::expanduser(&format!("~/.local/state/quickshell/user/generated/{f}"));
    let mut a = Args { scss: home_state("material_colors.scss"), colors: home_state("palette.json"), terminal_json: home_state("terminal.json"), terminals: None };
    let mut i = 0;
    while i < args.len() {
        let (flag, inline) = match args[i].split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (args[i].clone(), None),
        };
        let value = |i: &mut usize| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            *i += 1;
            args.get(*i).filter(|v| !v.starts_with('-') || v.as_str() == "-").cloned().ok_or(format!("argument {flag}: expected one argument"))
        };
        match flag.as_str() {
            "--scss" => a.scss = value(&mut i)?,
            "--colors" => a.colors = value(&mut i)?,
            "--terminal-json" => a.terminal_json = value(&mut i)?,
            "--terminals" => {
                let mut list = vec![];
                if let Some(v) = inline.clone() {
                    list.push(v);
                }
                while i + 1 < args.len() && (!args[i + 1].starts_with('-') || args[i + 1] == "-") {
                    i += 1;
                    list.push(args[i].clone());
                }
                if list.is_empty() {
                    return Err("argument --terminals: expected at least one argument".into());
                }
                if let Some(bad) = list.iter().find(|t| t.as_str() != "all" && !ALL.contains(&t.as_str())) {
                    return Err(format!("argument --terminals: invalid choice: {} (choose from {}, 'all')", pyrt::repr_str(bad),
                        ALL.iter().map(|t| format!("'{t}'")).collect::<Vec<_>>().join(", ")));
                }
                a.terminals = Some(list);
            }
            other => return Err(format!("unrecognized arguments: {other} (VSCode and Zed themes come from vscode-themegen and zed-themegen)")),
        }
        i += 1;
    }
    Ok(a)
}

fn run(a: &Args) -> R<()> {
    let colors = load_generator_colors(&a.scss, &a.colors, &a.terminal_json)?;
    let home = py::expanduser("~");
    let terminals: Vec<String> = match &a.terminals {
        Some(t) if !t.iter().any(|x| x == "all") => t.clone(),
        _ => ALL.iter().map(|s| s.to_string()).collect(),
    };
    let on = |t: &str| terminals.iter().any(|x| x == t);
    if on("kitty") {
        kitty(&colors, &format!("{home}/.config/kitty/current-theme.conf"), &home)?;
    }
    if on("alacritty") {
        alacritty(&colors, &format!("{home}/.config/alacritty/colors.toml"), &home)?;
    }
    if on("foot") {
        foot(&colors, &format!("{home}/.config/foot/inir-colors.ini"), &home)?;
    }
    if on("wezterm") {
        wezterm(&colors, &format!("{home}/.config/wezterm/colors.lua"), &home)?;
    }
    if on("ghostty") {
        ghostty(&colors, &format!("{home}/.config/ghostty/themes/ii-auto"), &home)?;
    }
    if on("konsole") {
        konsole(&colors, &format!("{home}/.local/share/konsole/ii-auto.colorscheme"))?;
    }
    if on("starship") {
        starship(&colors, &format!("{home}/.config/starship/ii-palette.toml"), &home)?;
    }
    if on("omp") {
        let out = format!("{home}/.config/oh-my-posh/ii-auto.json");
        if let Err(e) = merged(&colors, &a.colors, Some(&a.terminal_json)).and_then(|d| omp(&d, &out)) {
            if e.ty != "FileNotFoundError" {
                return Err(e);
            }
            println!("Warning: {} not found, oh-my-posh theme may be incomplete", a.colors);
            omp(&colors, &out)?;
        }
    }
    if on("btop") {
        let out = format!("{home}/.config/btop/themes/ii-auto.theme");
        if let Err(e) = merged(&colors, &a.colors, None).and_then(|d| btop(&d, &out, &home)) {
            if e.ty != "FileNotFoundError" {
                return Err(e);
            }
            println!("Warning: {} not found, btop theme may be incomplete", a.colors);
            btop(&colors, &out, &home)?;
        }
    }
    if on("lazygit") {
        lazygit(&colors, &format!("{home}/.config/lazygit/ii-theme.yml"), &home)?;
    }
    if on("yazi") {
        yazi(&colors, &format!("{home}/.config/yazi/flavors/ii-auto.yazi/flavor.toml"), &home)?;
    }
    if on("fuzzel") {
        fuzzel(&colors, &format!("{home}/.config/fuzzel/fuzzel_theme.ini"))?;
    }
    Ok(())
}

/// `inir-accel terminals ...`: the exit code.
pub fn run_cli(args: &[String]) -> i32 {
    let a = match parse_args(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("inir-accel terminals: error: {e}");
            return 2;
        }
    };
    match run(&a) {
        Ok(()) => 0,
        Err(e) if e.ty == "SystemExit" => e.msg.parse().unwrap_or(1),
        Err(e) => pyrt::die(e),
    }
}
