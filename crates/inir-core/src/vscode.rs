//! `scripts/colors/vscode_themegen/main.go`, line for line: `inir-vscode-themegen [--colors f] [--terminal-json f]
//! [--scss f] [--forks key]... [--list-forks] [--strip]`. JSON goes through Go 1.27's own reading (`gojsonv`), files
//! through Go's `os` (modes, error texts). Go walks its fork map in random order; the port walks it sorted.
//! This code was written by: Hikari.
use crate::gojsonv::{self, go_quote};
use crate::gomath;
use crate::gotheme::{self, go_err, hex_byte_b, marshal, marshal_indent, Colors, J};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

/// `stripJSONCComments`: `//` and `/* */` comments and trailing commas out, strings untouched.
pub fn strip_jsonc_comments(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut in_string = false;
    let mut escaped = false;
    let n = data.len();
    let mut i = 0;
    while i < n {
        let c = data[i];
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            out.push(c);
            i += 1;
        } else if c == b'/' && i + 1 < n && data[i + 1] == b'/' {
            while i < n && data[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && i + 1 < n && data[i + 1] == b'*' {
            i += 2;
            while i + 1 < n && !(data[i] == b'*' && data[i + 1] == b'/') {
                i += 1;
            }
            i = if i + 1 < n { i + 2 } else { n };
        } else if c == b',' && i + 1 < n {
            let mut j = i + 1;
            while j < n && matches!(data[j], b' ' | b'\t' | b'\n' | b'\r') {
                j += 1;
            }
            if !(j < n && (data[j] == b']' || data[j] == b'}')) {
                out.push(c);
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

const THEME_NAME: &str = "iNiR Material";
const THEME_NAME_LIGHT: &str = "iNiR Material Light";
const THEME_EXTENSION_ID: &str = "inir-material-theme";
const THEME_FILE_NAME: &str = "inir-material-color-theme.json";
const THEME_FILE_LIGHT: &str = "inir-material-light-color-theme.json";
const PREV_THEME_KEY: &str = "inir.previousColorTheme";

/// Config dir name → extensions dir relative to $HOME (`vscodeExtDirs`), and the config dir name (`vscodeForks`).
const FORKS: &[(&str, &str, &str)] = &[
    ("aide", ".aide/extensions", "Aide"),
    ("antigravity", ".antigravity/extensions", "Antigravity"),
    ("code", ".vscode/extensions", "Code"),
    ("code-insiders", ".vscode-insiders/extensions", "Code - Insiders"),
    ("code-oss", ".vscode-oss/extensions", "Code - OSS"),
    ("codium", ".vscode-oss/extensions", "VSCodium"),
    ("cursor", ".cursor/extensions", "Cursor"),
    ("melty", ".melty/extensions", "Melty"),
    ("pearai", ".pearai/extensions", "PearAI"),
    ("positron", ".positron/extensions", "Positron"),
    ("qoder", ".qoder/extensions", "Qoder"),
    ("void", ".void/extensions", "Void"),
    ("windsurf", ".windsurf/extensions", "Windsurf"),
    ("windsurf-next", ".windsurf-next/extensions", "Windsurf - Next"),
];

fn fork_name(key: &str) -> Option<&'static str> {
    FORKS.iter().find(|f| f.0 == key).map(|f| f.2)
}

// ── Color manipulation helpers ──────────────────────────────────────────

#[derive(Clone, Copy, Default)]
struct Hsl {
    h: f64,
    s: f64,
    l: f64,
}

fn trim_hash(s: &str) -> &[u8] {
    s.strip_prefix('#').unwrap_or(s).as_bytes()
}

fn hex_to_hsl(hex: &str) -> Hsl {
    let hex = trim_hash(hex);
    if hex.len() < 6 {
        return Hsl::default();
    }
    let r = hex_byte_b(&hex[0..2]) as f64 / 255.0;
    let g = hex_byte_b(&hex[2..4]) as f64 / 255.0;
    let b = hex_byte_b(&hex[4..6]) as f64 / 255.0;
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
    Hsl { h: h * 60.0, s, l }
}

/// `math.Max`/`math.Min` (the inputs are never NaN; ±0 ordered like Go).
pub(crate) fn go_max(a: f64, b: f64) -> f64 {
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { b } else { a };
    }
    if a > b {
        a
    } else {
        b
    }
}

pub(crate) fn go_min(a: f64, b: f64) -> f64 {
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b {
        a
    } else {
        b
    }
}

fn hsl_to_hex(c: Hsl) -> String {
    let s = go_max(0.0, go_min(1.0, c.s));
    let l = go_max(0.0, go_min(1.0, c.l));
    let mut h = c.h % 360.0;
    if h < 0.0 {
        h += 360.0;
    }
    let ch = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = ch * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - ch / 2.0;
    let (r, g, b) = if h < 60.0 {
        (ch, x, 0.0)
    } else if h < 120.0 {
        (x, ch, 0.0)
    } else if h < 180.0 {
        (0.0, ch, x)
    } else if h < 240.0 {
        (0.0, x, ch)
    } else if h < 300.0 {
        (x, 0.0, ch)
    } else {
        (ch, 0.0, x)
    };
    format!("#{:02x}{:02x}{:02x}", clamp255(r + m), clamp255(g + m), clamp255(b + m))
}

fn saturate_color(hex: &str, factor: f64, min_sat: f64) -> String {
    let mut c = hex_to_hsl(hex);
    if c.s < 0.01 {
        return hex.to_string();
    }
    c.s = go_max(min_sat, go_min(1.0, c.s * factor));
    hsl_to_hex(c)
}

fn blend_colors(base: &str, accent: &str, ratio: f64) -> String {
    let (b, a) = (trim_hash(base), trim_hash(accent));
    if b.len() < 6 || a.len() < 6 {
        return format!("#{}", base.strip_prefix('#').unwrap_or(base));
    }
    let mix = |i: usize| gotheme::go_int(hex_byte_b(&b[i..i + 2]) as f64 * (1.0 - ratio) + hex_byte_b(&a[i..i + 2]) as f64 * ratio);
    format!("#{:02x}{:02x}{:02x}", mix(0).clamp(0, 255), mix(2).clamp(0, 255), mix(4).clamp(0, 255))
}

fn adjust_lightness(hex: &str, min_l: f64, max_l: f64) -> String {
    let mut c = hex_to_hsl(hex);
    c.l = go_max(min_l, go_min(max_l, c.l));
    hsl_to_hex(c)
}

fn rel_luminance(hex: &str) -> f64 {
    let h = trim_hash(hex);
    if h.len() < 6 {
        return 0.0;
    }
    let lin = |v: i64| {
        let c = v as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            gomath::pow((c + 0.055) / 1.055, 2.4)
        }
    };
    0.2126 * lin(hex_byte_b(&h[0..2])) + 0.7152 * lin(hex_byte_b(&h[2..4])) + 0.0722 * lin(hex_byte_b(&h[4..6]))
}

fn contrast_hex(a: &str, b: &str) -> f64 {
    let (mut la, mut lb) = (rel_luminance(a), rel_luminance(b));
    if la < lb {
        std::mem::swap(&mut la, &mut lb);
    }
    (la + 0.05) / (lb + 0.05)
}

/// `withContrast`: lightness moves away from the background until the ratio holds.
fn with_contrast(hex: &str, bg: &str, ratio: f64, dark: bool) -> String {
    let mut c = hex_to_hsl(hex);
    let mut i = 0;
    while i < 60 && contrast_hex(&hsl_to_hex(c), bg) < ratio {
        c.l = if dark { go_min(0.96, c.l + 0.01) } else { go_max(0.04, c.l - 0.01) };
        i += 1;
    }
    hsl_to_hex(c)
}

/// `mutedInk`: `fg` at `alpha` over `bg` while that reads at 4.5:1, else the nearest solid colour that does; a short
/// or broken colour comes back as given (ec63bc1d in the shell).
fn muted_ink(fg: &str, bg: &str, alpha: i64, dark: bool) -> String {
    let fg = format!("#{}", fg.strip_prefix('#').unwrap_or(fg));
    let bg = format!("#{}", bg.strip_prefix('#').unwrap_or(bg));
    if fg.len() < 7 || bg.len() < 7 {
        return fg;
    }
    let a = alpha as f64 / 255.0;
    let (fb, bb) = (fg.as_bytes(), bg.as_bytes());
    let mix = |i: usize| {
        gotheme::go_int(hex_byte_b(&fb[i..i + 2]) as f64 * a + hex_byte_b(&bb[i..i + 2]) as f64 * (1.0 - a) + 0.5).clamp(0, 255)
    };
    let over = format!("#{:02x}{:02x}{:02x}", mix(1), mix(3), mix(5));
    if contrast_hex(&over, &bg) >= 4.5 {
        return format!("{fg}{alpha:02x}");
    }
    with_contrast(&fg, &bg, 4.5, dark)
}

/// `syntaxColor`: the ANSI hue of a slot, its lightness solved for the editor background.
fn syntax_color(term_colors: &Colors, primary: &str, bg: &str, term_idx: usize) -> String {
    let raw = pick(term_colors, &format!("term{term_idx}"), "#888888");
    let dark = hex_to_hsl(bg).l < 0.5;
    if dark {
        let mut tuned = saturate_color(&raw, 1.0, 0.42);
        let mut c = hex_to_hsl(&tuned);
        if c.s > 0.62 {
            c.s = 0.62;
            tuned = hsl_to_hex(c);
        }
        let blended = blend_colors(&tuned, primary, 0.25);
        return with_contrast(&adjust_lightness(&blended, 0.72, 0.86), bg, 4.5, true);
    }
    let boosted = saturate_color(&raw, 1.2, 0.45);
    let blended = blend_colors(&boosted, primary, 0.2);
    with_contrast(&adjust_lightness(&blended, 0.22, 0.46), bg, 4.5, false)
}

fn clamp255(v: f64) -> i64 {
    gotheme::go_int(v * 255.0).clamp(0, 255)
}

fn pick(m: &Colors, key: &str, fallback: &str) -> String {
    gotheme::pick(m, key, fallback)
}

// ── Go's os, as the original calls it ───────────────────────────────────

/// A Go `*PathError` text: `op path: errno text`.
pub(crate) fn path_err(op: &str, path: &str, e: &std::io::Error) -> String {
    format!("{op} {path}: {}", go_err(e))
}

fn is_dir(path: &str) -> bool {
    fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

fn exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

/// `os.MkdirAll(path, 0o755)`.
pub(crate) fn mkdir_all(path: &str) -> Result<(), String> {
    match fs::metadata(path) {
        Ok(m) if m.is_dir() => return Ok(()),
        Ok(_) => return Err(format!("mkdir {path}: not a directory")),
        Err(_) => {}
    }
    let b = path.as_bytes();
    let mut i = b.len();
    while i > 0 && b[i - 1] == b'/' {
        i -= 1;
    }
    let mut j = i;
    while j > 0 && b[j - 1] != b'/' {
        j -= 1;
    }
    if j > 1 {
        mkdir_all(&path[..j - 1])?;
    }
    if let Err(e) = fs::DirBuilder::new().mode(0o755).create(path) {
        if fs::symlink_metadata(path).map(|m| m.is_dir()).unwrap_or(false) {
            return Ok(());
        }
        return Err(path_err("mkdir", path, &e));
    }
    Ok(())
}

/// `os.WriteFile(path, data, 0o644)`.
pub(crate) fn write_file(path: &str, data: &[u8]) -> Result<(), String> {
    let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o644).open(path).map_err(|e| path_err("open", path, &e))?;
    f.write_all(data).map_err(|e| path_err("write", path, &e))
}

/// `os.Rename`, a `*LinkError` on failure.
fn rename(from: &str, to: &str) -> Result<(), String> {
    fs::rename(from, to).map_err(|e| format!("rename {from} {to}: {}", go_err(&e)))
}

/// `os.ReadFile`: `Err((text, is_not_exist))`.
pub(crate) fn read_file(path: &str) -> Result<Vec<u8>, (String, bool)> {
    let mut f = fs::File::open(path).map_err(|e| (path_err("open", path, &e), e.kind() == std::io::ErrorKind::NotFound))?;
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut out).map_err(|e| (path_err("read", path, &e), false))?;
    Ok(out)
}

/// A Go runtime panic: its first line (the goroutine dump that follows changes every run), exit code 2.
fn panic_nil_map() -> ! {
    let _ = std::io::stdout().flush();
    eprint!("panic: assignment to entry in nil map\n\ngoroutine 1 [running]:\nmain.main()\n");
    std::process::exit(2);
}

/// `common.ReadStringMapJSON`: the string values of a JSON object.
pub(crate) fn read_string_map_json(path: &str) -> Result<Colors, String> {
    let data = read_file(path).map_err(|e| e.0)?;
    let (raw, err) = gojsonv::unmarshal_map(&data);
    if let Some(e) = err {
        return Err(e);
    }
    let mut out = Colors::new();
    for (k, v) in raw.unwrap_or_default() {
        if let J::Str(s) = v {
            out.insert(k, s);
        }
    }
    Ok(out)
}

// ── Extension management ────────────────────────────────────────────────

const LEGACY_EXTENSION_DIRS: &[&str] = &["inir.inir-theme-1.0.0"];

pub(crate) fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// `filepath.Join`: the parts joined by `/`, cleaned.
pub(crate) fn join(parts: &[&str]) -> String {
    let joined = parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("/");
    clean(&joined)
}

/// `filepath.Clean`.
pub(crate) fn clean(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let rooted = p.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !rooted {
                    out.push("..");
                }
            }
            x => out.push(x),
        }
    }
    let body = out.join("/");
    match (rooted, body.is_empty()) {
        (true, _) => format!("/{body}"),
        (false, true) => ".".into(),
        (false, false) => body,
    }
}

pub(crate) fn dir_of(p: &str) -> String {
    match p.rfind('/') {
        Some(0) => "/".into(),
        Some(i) => clean(&p[..i]),
        None => ".".into(),
    }
}

fn get_extensions_dir(fork_key: &str) -> String {
    let rel = FORKS.iter().find(|f| f.0 == fork_key).map(|f| f.1.to_string()).unwrap_or(format!(".{fork_key}/extensions"));
    join(&[&home(), &rel])
}

fn is_dark_theme(colors: &Colors) -> bool {
    let bg = pick(colors, "background", &pick(colors, "surface", "#080809"));
    hex_to_hsl(&bg).l < 0.5
}

fn clean_legacy_extensions(fork_key: &str) {
    let ext_base = get_extensions_dir(fork_key);
    for legacy in LEGACY_EXTENSION_DIRS {
        let legacy_dir = join(&[&ext_base, legacy]);
        if is_dir(&legacy_dir) {
            let _ = fs::remove_dir_all(&legacy_dir);
        }
    }
}

fn entry_id(e: &J) -> Option<&str> {
    let J::Map(m) = e else { return None };
    let Some(J::Map(ident)) = m.get("identifier") else { return None };
    match ident.get("id") {
        Some(J::Str(s)) => Some(s),
        _ => None,
    }
}

/// `ensureExtensionRegistered`: our entry in the editor's extensions.json (Cursor and Windsurf load only those).
fn ensure_extension_registered(fork_key: &str, ext_dir: &str) -> Result<(), String> {
    let registry_path = join(&[&get_extensions_dir(fork_key), "extensions.json"]);
    let mut entries = match read_file(&registry_path) {
        Ok(data) => gojsonv::unmarshal_map_slice(&data).0.unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let ext_id = format!("inir.{THEME_EXTENSION_ID}");
    if entries.iter().any(|e| entry_id(e) == Some(&ext_id)) {
        return Ok(());
    }
    let location = crate::jmap! { "$mid" => 1.0, "path" => ext_dir, "scheme" => "file" };
    let mut entry = crate::jmap! { "identifier" => crate::jmap! { "id" => &ext_id }, "version" => "1.0.0", "relativeLocation" => THEME_EXTENSION_ID };
    if let J::Map(m) = &mut entry {
        m.insert("location".into(), location);
    }
    entries.push(entry);
    let data = marshal(&J::Arr(entries))?;
    let tmp = format!("{registry_path}.tmp");
    write_file(&tmp, data.as_bytes())?;
    rename(&tmp, &registry_path)
}

/// `unregisterExtension`.
fn unregister_extension(fork_key: &str) {
    let registry_path = join(&[&get_extensions_dir(fork_key), "extensions.json"]);
    let Ok(data) = read_file(&registry_path) else { return };
    let (entries, err) = gojsonv::unmarshal_map_slice(&data);
    if err.is_some() {
        return;
    }
    let entries = entries.unwrap_or_default();
    let ext_id = format!("inir.{THEME_EXTENSION_ID}");
    let n = entries.len();
    let filtered: Vec<J> = entries.into_iter().filter(|e| entry_id(e) != Some(&ext_id)).collect();
    if filtered.len() == n {
        return;
    }
    let Ok(out) = marshal(&J::Arr(filtered)) else { return };
    let tmp = format!("{registry_path}.tmp");
    if write_file(&tmp, out.as_bytes()).is_err() {
        return;
    }
    let _ = rename(&tmp, &registry_path);
}

fn write_extension_manifest(ext_dir: &str) -> Result<(), String> {
    let theme = |label: &str, ui: &str, file: &str| {
        crate::jmap! { "label" => label, "uiTheme" => ui, "path" => format!("./themes/{file}"), "_watch" => true }
    };
    let mut manifest = crate::jmap! {
        "name" => THEME_EXTENSION_ID,
        "displayName" => THEME_NAME,
        "description" => "Dynamic Material You theme — auto-generated by iNiR from wallpaper colors",
        "version" => "1.0.0",
        "publisher" => "inir",
    };
    if let J::Map(m) = &mut manifest {
        m.insert("engines".into(), crate::jmap! { "vscode" => "^1.34.0" });
        m.insert("categories".into(), J::Arr(vec![J::from("Themes")]));
        let themes = J::Arr(vec![theme(THEME_NAME, "vs-dark", THEME_FILE_NAME), theme(THEME_NAME_LIGHT, "vs", THEME_FILE_LIGHT)]);
        let mut contributes = BTreeMap::new();
        contributes.insert("themes".to_string(), themes);
        m.insert("contributes".into(), J::Map(contributes));
    }
    let mut data = marshal_indent(&manifest, "", "  ");
    data.push('\n');
    write_file(&join(&[ext_dir, "package.json"]), data.as_bytes())
}

/// `themeIdentity`: the entry VS Code knows the palette by (its uiTheme sets the base).
fn theme_identity(colors: &Colors) -> (&'static str, &'static str) {
    if is_dark_theme(colors) {
        (THEME_NAME, THEME_FILE_NAME)
    } else {
        (THEME_NAME_LIGHT, THEME_FILE_LIGHT)
    }
}

fn manifest_has_light(pkg_path: &str) -> bool {
    match read_file(pkg_path) {
        Ok(data) => data.windows(THEME_FILE_LIGHT.len()).any(|w| w == THEME_FILE_LIGHT.as_bytes()),
        Err(_) => false,
    }
}

fn write_theme_file(theme_path: &str, colors: &Colors, term_colors: &Colors) -> Result<(), String> {
    mkdir_all(&dir_of(theme_path))?;
    let theme_type = if is_dark_theme(colors) { "dark" } else { "light" };
    let (label, _) = theme_identity(colors);
    let strings = |m: BTreeMap<String, String>| J::Map(m.into_iter().map(|(k, v)| (k, J::Str(v))).collect());
    let mut theme = BTreeMap::new();
    theme.insert("$schema".to_string(), J::from("vscode://schemas/color-theme"));
    theme.insert("name".to_string(), J::from(label));
    theme.insert("type".to_string(), J::from(theme_type));
    theme.insert("colors".to_string(), strings(generate_colors(colors, term_colors)));
    theme.insert("tokenColors".to_string(), J::Arr(generate_syntax(colors, term_colors)));
    theme.insert("semanticHighlighting".to_string(), J::Bool(true));
    theme.insert("semanticTokenColors".to_string(), strings(generate_semantic(colors, term_colors)));
    let mut data = marshal_indent(&J::Map(theme), "", "  ");
    data.push('\n');
    // Atomic write: tmp + rename. VS Code's EventCoalescer turns DELETE+CREATE into UPDATED, so _watch reloads it.
    let tmp = format!("{theme_path}.tmp");
    write_file(&tmp, data.as_bytes())?;
    rename(&tmp, theme_path)
}

// ── Main ────────────────────────────────────────────────────────────────

struct Flags {
    colors: String,
    terminal_json: String,
    scss: String,
    list_forks: bool,
    strip: bool,
    forks: Vec<String>,
}

/// The `flag` package's usage text for this program.
fn usage(argv0: &str, defaults: &Flags) {
    eprintln!("Usage of {argv0}:");
    eprintln!("  -colors string\n    \t (default {})", go_quote(&defaults.colors));
    eprintln!("  -forks value\n    \tFork key(s) to target");
    eprintln!("  -list-forks\n    \tList known forks and exit");
    eprintln!("  -scss string\n    \t (default {})", go_quote(&defaults.scss));
    eprintln!("  -strip\n    \tRemove iNiR theme extension and clean settings.json");
    eprintln!("  -terminal-json string\n    \t (default {})", go_quote(&defaults.terminal_json));
}

fn parse_bool(s: &str) -> Option<bool> {
    match s {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// `flag.Parse()` on the command line: the flags, or the exit code `ExitOnError` ends with.
fn parse_flags(argv0: &str, args: &[String], defaults: Flags) -> Result<Flags, i32> {
    let mut f = Flags { colors: defaults.colors.clone(), terminal_json: defaults.terminal_json.clone(), scss: defaults.scss.clone(), list_forks: false, strip: false, forks: Vec::new() };
    let fail = |msg: String| {
        eprintln!("{msg}");
        usage(argv0, &defaults);
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
        match name {
            "list-forks" | "strip" => {
                let v = match &value {
                    Some(v) => match parse_bool(v) {
                        Some(b) => b,
                        None => return Err(fail(format!("invalid boolean value {} for -{name}: parse error", go_quote(v)))),
                    },
                    None => true,
                };
                if name == "strip" {
                    f.strip = v;
                } else {
                    f.list_forks = v;
                }
            }
            "colors" | "terminal-json" | "scss" | "forks" => {
                let v = match value {
                    Some(v) => v,
                    None if i < args.len() => {
                        i += 1;
                        args[i - 1].clone()
                    }
                    None => return Err(fail(format!("flag needs an argument: -{name}"))),
                };
                match name {
                    "colors" => f.colors = v,
                    "terminal-json" => f.terminal_json = v,
                    "scss" => f.scss = v,
                    _ => f.forks.push(v),
                }
            }
            "help" | "h" => {
                usage(argv0, &defaults);
                return Err(0);
            }
            _ => return Err(fail(format!("flag provided but not defined: -{name}"))),
        }
    }
    Ok(f)
}

pub fn run_cli(argv0: &str, args: &[String]) -> i32 {
    let home = home();
    let generated = join(&[&home, ".local/state/quickshell/user/generated"]);
    let defaults = Flags {
        colors: join(&[&generated, "palette.json"]),
        terminal_json: join(&[&generated, "terminal.json"]),
        scss: join(&[&generated, "material_colors.scss"]),
        list_forks: false,
        strip: false,
        forks: Vec::new(),
    };
    let flags = match parse_flags(argv0, args, defaults) {
        Ok(f) => f,
        Err(code) => return code,
    };

    if flags.strip {
        let results = strip_all_themes(&flags.forks);
        let success = results.values().filter(|ok| **ok).count();
        println!("Stripped themes from {success}/{} forks", results.len());
        return if success > 0 { 0 } else { 1 };
    }
    if flags.list_forks {
        println!("Known VSCode forks:");
        for (key, _, name) in FORKS {
            let installed = if is_dir(&dir_of(&get_settings_path(name))) { "✓" } else { "✗" };
            let ext_installed = if is_dir(&join(&[&get_extensions_dir(key), THEME_EXTENSION_ID])) { "✓" } else { "✗" };
            println!("  [{installed}] {key}: {name} (ext: {ext_installed})");
        }
        return 0;
    }

    let results = generate_all_themes(&flags.colors, &flags.terminal_json, &flags.scss, &flags.forks);
    let success = results.values().filter(|ok| **ok).count();
    println!("Generated themes for {success}/{} forks", results.len());
    if success > 0 {
        0
    } else {
        1
    }
}

fn generate_all_themes(colors_path: &str, terminal_json_path: &str, scss_path: &str, forks: &[String]) -> BTreeMap<String, bool> {
    let mut selected: Vec<String> = forks.to_vec();
    if selected.is_empty() {
        for (key, _, name) in FORKS {
            if is_dir(&dir_of(&get_settings_path(name))) {
                selected.push(key.to_string());
            }
        }
    }
    let mut results = BTreeMap::new();
    for fork_key in &selected {
        let Some(fork_name) = fork_name(fork_key) else {
            eprintln!("Unknown fork: {fork_key}");
            continue;
        };
        let settings_path = get_settings_path(fork_name);
        if !is_dir(&dir_of(&settings_path)) {
            continue;
        }
        match generate_theme_for_fork(colors_path, terminal_json_path, scss_path, &settings_path, fork_key) {
            Err(e) => {
                eprintln!("  ✗ {fork_name}: {e}");
                results.insert(fork_key.clone(), false);
            }
            Ok(ok) => {
                results.insert(fork_key.clone(), ok);
                if ok {
                    println!("  ✓ {fork_name}");
                } else {
                    eprintln!("  ✗ {fork_name}");
                }
            }
        }
    }
    results
}

fn generate_theme_for_fork(colors_path: &str, terminal_json_path: &str, scss_path: &str, settings_path: &str, fork_key: &str) -> Result<bool, String> {
    let colors = read_string_map_json(colors_path).map_err(|_| format!("could not read {colors_path}"))?;
    // ParseSCSS returns a nil map when the file is missing: writing the terminal colours into it panics in Go.
    let mut term_colors = gotheme::parse_scss(scss_path).ok();
    if !terminal_json_path.is_empty() {
        if let Ok(explicit) = read_string_map_json(terminal_json_path) {
            for (k, v) in explicit {
                match term_colors.as_mut() {
                    Some(t) => {
                        t.insert(k, v);
                    }
                    None => panic_nil_map(),
                }
            }
        }
    }
    let term_colors = term_colors.unwrap_or_default();

    // Clean up legacy extensions from older iNiR versions.
    clean_legacy_extensions(fork_key);

    // Install the local extension with _watch: true; package.json only once (rewriting it mid-session upsets the
    // extension host).
    let ext_dir = join(&[&get_extensions_dir(fork_key), THEME_EXTENSION_ID]);
    mkdir_all(&ext_dir).map_err(|e| format!("failed to create extension dir: {e}"))?;
    let pkg_path = join(&[&ext_dir, "package.json"]);
    let missing = matches!(fs::metadata(&pkg_path), Err(e) if e.kind() == std::io::ErrorKind::NotFound);
    if missing || !manifest_has_light(&pkg_path) {
        write_extension_manifest(&ext_dir).map_err(|e| format!("failed to write extension manifest: {e}"))?;
    }

    // Register in extensions.json (Cursor/Windsurf require it; VS Code is more lenient).
    if let Err(e) = ensure_extension_registered(fork_key, &ext_dir) {
        eprintln!("  warn: extensions.json registration failed: {e}");
    }

    // The theme file (atomic rename: _watch handles the live reload).
    let (active_name, active_file) = theme_identity(&colors);
    let theme_path = join(&[&ext_dir, "themes", active_file]);
    write_theme_file(&theme_path, &colors, &term_colors).map_err(|e| format!("failed to write theme: {e}"))?;

    // settings.json only when needed: first activation or legacy cleanup.
    let settings = load_settings(settings_path)?;
    let current_theme = match settings.as_ref().and_then(|s| s.get("workbench.colorTheme")) {
        Some(J::Str(s)) => s.clone(),
        _ => String::new(),
    };
    let mut changed = false;
    // `settings` is None for a file that holds `null`: Go's nil map, which panics on the first write.
    let mut settings = settings;
    let set = |settings: &mut Option<BTreeMap<String, J>>, k: &str, v: J| match settings {
        Some(m) => {
            m.insert(k.to_string(), v);
        }
        None => panic_nil_map(),
    };

    // Save the previous theme for restoration on strip.
    if !current_theme.is_empty() && current_theme != THEME_NAME && current_theme != THEME_NAME_LIGHT {
        set(&mut settings, PREV_THEME_KEY, J::Str(current_theme.clone()));
        changed = true;
    }
    // Old-style injection from previous iNiR versions.
    for key in ["workbench.colorCustomizations", "editor.tokenColorCustomizations", "editor.semanticTokenColorCustomizations"] {
        if let Some(m) = settings.as_mut() {
            if m.remove(key).is_some() {
                changed = true;
            }
        }
    }
    // Activate our theme if not already active; if it is, _watch reloads on file change.
    if current_theme != active_name {
        set(&mut settings, "workbench.colorTheme", J::from(active_name));
        changed = true;
    }
    if changed {
        write_settings(settings_path, &settings.unwrap_or_default())?;
    }
    Ok(true)
}

fn generate_colors(colors: &Colors, term_colors: &Colors) -> BTreeMap<String, String> {
    let p = |k: &str, f: &str| pick(colors, k, f);
    let bg = p("background", &p("surface", "#080809"));
    let fg = p("on_background", &p("on_surface", "#e3dfd9"));
    let surface = p("surface", "#080809");
    let surface_lowest = p("surface_container_lowest", &surface);
    let surface_low = p("surface_container_low", "#0c0c0e");
    let surface_std = p("surface_container", "#121115");
    let surface_high = p("surface_container_high", "#1a191d");
    let surface_highest = p("surface_container_highest", "#232126");
    let on_surface = p("on_surface", "#e3dfd9");
    let on_surface_variant = p("on_surface_variant", "#c4bfb8");
    let outline = p("outline", "#5c5862");
    let outline_variant = p("outline_variant", "#3a363e");
    let primary = p("primary", "#d4b796");
    let on_primary = p("on_primary", "#241c14");
    let primary_container = p("primary_container", "#33281d");
    let on_primary_container = p("on_primary_container", "#eddccb");
    let secondary = p("secondary", "#ccc2b2");
    let tertiary = p("tertiary", "#b8cbb8");
    let error_col = p("error", "#ffb4ab");
    let term_defaults = [
        &surface_lowest, &error_col, &tertiary, &secondary, &primary, &secondary, &tertiary, &on_surface, &outline_variant,
        &error_col, &tertiary, &secondary, &primary, &secondary, &tertiary, &fg,
    ];
    let term = |name: &str| {
        let idx: usize = name[4..].parse().unwrap();
        pick(term_colors, name, term_defaults[idx])
    };
    let term_bg = term("term0");
    let term_fg = term("term15");
    let transparent = "#00000000".to_string();
    let entries: Vec<(&str, String)> = vec![
        ("focusBorder", primary.clone()),
        ("foreground", fg.clone()),
        ("disabledForeground", format!("{}80", on_surface_variant)),
        ("widget.shadow", "#00000060".to_string()),
        ("selection.background", format!("{}60", primary)),
        ("descriptionForeground", on_surface_variant.clone()),
        ("errorForeground", error_col.clone()),
        ("icon.foreground", on_surface.clone()),
        ("window.activeBorder", transparent.clone()),
        ("window.inactiveBorder", transparent.clone()),
        ("textBlockQuote.background", surface_low.clone()),
        ("textBlockQuote.border", transparent.clone()),
        ("textCodeBlock.background", surface_low.clone()),
        ("textLink.activeForeground", primary.clone()),
        ("textLink.foreground", primary.clone()),
        ("textPreformat.foreground", tertiary.clone()),
        ("textSeparator.foreground", transparent.clone()),
        ("button.background", primary_container.clone()),
        ("button.foreground", on_primary_container.clone()),
        ("button.hoverBackground", format!("{}dd", primary_container)),
        ("button.secondaryBackground", surface_high.clone()),
        ("button.secondaryForeground", on_surface.clone()),
        ("button.secondaryHoverBackground", surface_highest.clone()),
        ("checkbox.background", surface_std.clone()),
        ("checkbox.border", transparent.clone()),
        ("checkbox.foreground", on_surface.clone()),
        ("dropdown.background", surface_low.clone()),
        ("dropdown.border", transparent.clone()),
        ("dropdown.foreground", on_surface.clone()),
        ("dropdown.listBackground", surface_std.clone()),
        ("input.background", surface_low.clone()),
        ("input.border", transparent.clone()),
        ("input.foreground", on_surface.clone()),
        ("input.placeholderForeground", format!("{}80", on_surface_variant)),
        ("inputOption.activeBackground", format!("{}40", primary)),
        ("inputOption.activeBorder", primary.clone()),
        ("inputOption.activeForeground", on_surface.clone()),
        ("inputValidation.errorBackground", format!("{}20", error_col)),
        ("inputValidation.errorBorder", error_col.clone()),
        ("scrollbar.shadow", "#00000040".to_string()),
        ("scrollbarSlider.activeBackground", format!("{}80", on_surface_variant)),
        ("scrollbarSlider.background", format!("{}40", on_surface_variant)),
        ("scrollbarSlider.hoverBackground", format!("{}60", on_surface_variant)),
        ("badge.background", primary_container.clone()),
        ("badge.foreground", on_primary_container.clone()),
        ("progressBar.background", primary.clone()),
        ("list.activeSelectionBackground", surface_high.clone()),
        ("list.activeSelectionForeground", on_surface.clone()),
        ("list.dropBackground", format!("{}40", primary)),
        ("list.focusBackground", surface_high.clone()),
        ("list.focusForeground", on_surface.clone()),
        ("list.highlightForeground", primary.clone()),
        ("list.hoverBackground", surface_std.clone()),
        ("list.hoverForeground", on_surface.clone()),
        ("list.inactiveSelectionBackground", surface_std.clone()),
        ("list.inactiveSelectionForeground", on_surface.clone()),
        ("list.invalidItemForeground", error_col.clone()),
        ("list.errorForeground", error_col.clone()),
        ("list.warningForeground", tertiary.clone()),
        ("listFilterWidget.background", surface_high.clone()),
        ("listFilterWidget.outline", primary.clone()),
        ("listFilterWidget.noMatchesOutline", error_col.clone()),
        ("list.filterMatchBackground", format!("{}40", primary)),
        ("tree.indentGuidesStroke", format!("{}40", outline_variant)),
        ("activityBar.background", surface_lowest.clone()),
        ("activityBar.foreground", on_surface.clone()),
        ("activityBar.inactiveForeground", on_surface_variant.clone()),
        ("activityBar.border", transparent.clone()),
        ("activityBarBadge.background", primary.clone()),
        ("activityBarBadge.foreground", on_primary.clone()),
        ("activityBar.activeBorder", primary.clone()),
        ("activityBar.activeBackground", surface_std.clone()),
        ("sideBar.background", surface_lowest.clone()),
        ("sideBar.foreground", on_surface.clone()),
        ("sideBar.border", transparent.clone()),
        ("sideBarTitle.foreground", on_surface.clone()),
        ("sideBarSectionHeader.background", surface_low.clone()),
        ("sideBarSectionHeader.foreground", on_surface.clone()),
        ("sideBarSectionHeader.border", transparent.clone()),
        ("editorGroup.border", transparent.clone()),
        ("editorGroup.dropBackground", format!("{}20", primary)),
        ("editorGroupHeader.noTabsBackground", surface_lowest.clone()),
        ("editorGroupHeader.tabsBackground", surface_lowest.clone()),
        ("editorGroupHeader.tabsBorder", transparent.clone()),
        ("editorGroupHeader.border", transparent.clone()),
        ("tab.activeBackground", surface_low.clone()),
        ("tab.activeForeground", on_surface.clone()),
        ("tab.border", transparent.clone()),
        ("tab.activeBorder", primary.clone()),
        ("tab.unfocusedActiveBorder", outline.clone()),
        ("tab.inactiveBackground", surface_lowest.clone()),
        ("tab.inactiveForeground", on_surface_variant.clone()),
        ("tab.unfocusedActiveForeground", on_surface_variant.clone()),
        ("tab.unfocusedInactiveForeground", format!("{}80", on_surface_variant)),
        ("tab.hoverBackground", surface_low.clone()),
        ("tab.unfocusedHoverBackground", surface_low.clone()),
        ("tab.hoverForeground", on_surface.clone()),
        ("tab.hoverBorder", outline.clone()),
        ("tab.lastPinnedBorder", outline.clone()),
        ("editor.background", bg.clone()),
        ("editor.foreground", fg.clone()),
        ("editorPane.background", bg.clone()),
        ("editorGutter.background", bg.clone()),
        ("editorOverviewRuler.background", bg.clone()),
        ("editorStickyScroll.background", bg.clone()),
        ("editorStickyScrollHover.background", surface_low.clone()),
        ("editorLineNumber.foreground", on_surface_variant.clone()),
        ("editorLineNumber.activeForeground", on_surface.clone()),
        ("editorCursor.foreground", primary.clone()),
        ("editor.selectionBackground", format!("{}66", primary_container)),
        ("editor.inactiveSelectionBackground", format!("{}33", primary_container)),
        ("editor.selectionHighlightBackground", format!("{}4d", primary_container)),
        ("editor.wordHighlightBackground", format!("{}30", secondary)),
        ("editor.wordHighlightStrongBackground", format!("{}50", secondary)),
        ("editor.findMatchBackground", format!("{}40", tertiary)),
        ("editor.findMatchHighlightBackground", format!("{}30", tertiary)),
        ("editor.findRangeHighlightBackground", format!("{}20", primary)),
        ("editor.hoverHighlightBackground", format!("{}20", primary)),
        ("editor.lineHighlightBackground", format!("{}80", surface_low)),
        ("editor.lineHighlightBorder", format!("{}00", outline_variant)),
        ("editorLink.activeForeground", primary.clone()),
        ("editor.rangeHighlightBackground", format!("{}40", surface_std)),
        ("editorWhitespace.foreground", format!("{}40", on_surface_variant)),
        ("editorIndentGuide.background", outline_variant.clone()),
        ("editorIndentGuide.activeBackground", outline.clone()),
        ("editorRuler.foreground", outline_variant.clone()),
        ("editorCodeLens.foreground", on_surface_variant.clone()),
        ("editorBracketMatch.background", format!("{}20", primary)),
        ("editorBracketMatch.border", primary.clone()),
        ("diffEditor.insertedTextBackground", format!("{}20", tertiary)),
        ("diffEditor.removedTextBackground", format!("{}20", error_col)),
        ("diffEditor.insertedLineBackground", format!("{}15", tertiary)),
        ("diffEditor.removedLineBackground", format!("{}15", error_col)),
        ("diffEditor.diagonalFill", format!("{}80", outline_variant)),
        ("diffEditor.border", transparent.clone()),
        ("editorWidget.background", surface_high.clone()),
        ("editorWidget.border", outline.clone()),
        ("editorWidget.foreground", on_surface.clone()),
        ("editorSuggestWidget.background", surface_high.clone()),
        ("editorSuggestWidget.border", outline.clone()),
        ("editorSuggestWidget.foreground", on_surface.clone()),
        ("editorSuggestWidget.highlightForeground", primary.clone()),
        ("editorSuggestWidget.selectedBackground", surface_highest.clone()),
        ("editorHoverWidget.background", surface_high.clone()),
        ("editorHoverWidget.border", outline.clone()),
        ("peekView.border", primary.clone()),
        ("peekViewEditor.background", surface_low.clone()),
        ("peekViewEditorGutter.background", surface_low.clone()),
        ("peekViewEditor.matchHighlightBackground", format!("{}40", primary)),
        ("peekViewResult.background", surface_std.clone()),
        ("peekViewResult.fileForeground", on_surface.clone()),
        ("peekViewResult.lineForeground", on_surface_variant.clone()),
        ("peekViewResult.matchHighlightBackground", format!("{}40", primary)),
        ("peekViewResult.selectionBackground", surface_high.clone()),
        ("peekViewResult.selectionForeground", on_surface.clone()),
        ("peekViewTitle.background", surface_std.clone()),
        ("peekViewTitleDescription.foreground", on_surface_variant.clone()),
        ("peekViewTitleLabel.foreground", on_surface.clone()),
        ("merge.currentHeaderBackground", format!("{}80", primary)),
        ("merge.currentContentBackground", format!("{}20", primary)),
        ("merge.incomingHeaderBackground", format!("{}80", secondary)),
        ("merge.incomingContentBackground", format!("{}20", secondary)),
        ("merge.border", outline.clone()),
        ("mergeEditor.background", bg.clone()),
        ("editorOverviewRuler.currentContentForeground", primary.clone()),
        ("editorOverviewRuler.incomingContentForeground", secondary.clone()),
        ("panel.background", surface_lowest.clone()),
        ("panel.border", transparent.clone()),
        ("panelTitle.activeBorder", primary.clone()),
        ("panelTitle.activeForeground", on_surface.clone()),
        ("panelTitle.inactiveForeground", on_surface_variant.clone()),
        ("panelInput.border", outline.clone()),
        ("statusBar.background", surface_lowest.clone()),
        ("statusBar.foreground", on_surface.clone()),
        ("statusBar.border", transparent.clone()),
        ("statusBar.debuggingBackground", error_col.clone()),
        ("statusBar.debuggingForeground", on_primary.clone()),
        ("statusBar.noFolderBackground", surface_lowest.clone()),
        ("statusBar.noFolderForeground", on_surface.clone()),
        ("statusBarItem.activeBackground", surface_high.clone()),
        ("statusBarItem.hoverBackground", surface_std.clone()),
        ("statusBarItem.prominentBackground", primary_container.clone()),
        ("statusBarItem.prominentForeground", on_primary_container.clone()),
        ("statusBarItem.prominentHoverBackground", format!("{}dd", primary_container)),
        ("titleBar.activeBackground", surface_lowest.clone()),
        ("titleBar.activeForeground", on_surface.clone()),
        ("titleBar.inactiveBackground", surface_lowest.clone()),
        ("titleBar.inactiveForeground", on_surface_variant.clone()),
        ("titleBar.border", transparent.clone()),
        ("menubar.selectionForeground", on_surface.clone()),
        ("menubar.selectionBackground", surface_std.clone()),
        ("menu.foreground", on_surface.clone()),
        ("menu.background", surface_high.clone()),
        ("menu.selectionForeground", on_surface.clone()),
        ("menu.selectionBackground", surface_highest.clone()),
        ("menu.separatorBackground", transparent.clone()),
        ("menu.border", transparent.clone()),
        ("notificationCenter.border", transparent.clone()),
        ("notificationCenterHeader.foreground", on_surface.clone()),
        ("notificationCenterHeader.background", surface_std.clone()),
        ("notificationToast.border", transparent.clone()),
        ("notifications.foreground", on_surface.clone()),
        ("notifications.background", surface_high.clone()),
        ("notifications.border", transparent.clone()),
        ("notificationLink.foreground", primary.clone()),
        ("extensionButton.prominentForeground", on_primary.clone()),
        ("extensionButton.prominentBackground", primary.clone()),
        ("extensionButton.prominentHoverBackground", format!("{}dd", primary)),
        ("pickerGroup.border", outline.clone()),
        ("pickerGroup.foreground", primary.clone()),
        ("quickInput.background", surface_high.clone()),
        ("quickInput.foreground", on_surface.clone()),
        ("terminal.background", term_bg.clone()),
        ("terminal.foreground", term_fg.clone()),
        ("terminal.ansiBlack", term("term0")),
        ("terminal.ansiRed", term("term1")),
        ("terminal.ansiGreen", term("term2")),
        ("terminal.ansiYellow", term("term3")),
        ("terminal.ansiBlue", term("term4")),
        ("terminal.ansiMagenta", term("term5")),
        ("terminal.ansiCyan", term("term6")),
        ("terminal.ansiWhite", term("term7")),
        ("terminal.ansiBrightBlack", term("term8")),
        ("terminal.ansiBrightRed", term("term9")),
        ("terminal.ansiBrightGreen", term("term10")),
        ("terminal.ansiBrightYellow", term("term11")),
        ("terminal.ansiBrightBlue", term("term12")),
        ("terminal.ansiBrightMagenta", term("term13")),
        ("terminal.ansiBrightCyan", term("term14")),
        ("terminal.ansiBrightWhite", term("term15")),
        ("terminal.selectionBackground", format!("{}40", primary)),
        ("terminalCursor.background", bg.clone()),
        ("terminalCursor.foreground", primary.clone()),
        ("debugToolBar.background", surface_high.clone()),
        ("debugToolBar.border", outline.clone()),
        ("editor.stackFrameHighlightBackground", format!("{}30", tertiary)),
        ("editor.focusedStackFrameHighlightBackground", format!("{}50", tertiary)),
        ("gitDecoration.addedResourceForeground", tertiary.clone()),
        ("gitDecoration.modifiedResourceForeground", secondary.clone()),
        ("gitDecoration.deletedResourceForeground", error_col.clone()),
        ("gitDecoration.untrackedResourceForeground", format!("{}cc", tertiary)),
        ("gitDecoration.ignoredResourceForeground", format!("{}80", on_surface_variant)),
        ("gitDecoration.conflictingResourceForeground", error_col.clone()),
        ("gitDecoration.submoduleResourceForeground", secondary.clone()),
        ("settings.headerForeground", on_surface.clone()),
        ("settings.modifiedItemIndicator", primary.clone()),
        ("settings.dropdownBackground", surface_low.clone()),
        ("settings.dropdownForeground", on_surface.clone()),
        ("settings.dropdownBorder", outline.clone()),
        ("settings.checkboxBackground", surface_std.clone()),
        ("settings.checkboxForeground", on_surface.clone()),
        ("settings.checkboxBorder", outline.clone()),
        ("settings.textInputBackground", surface_low.clone()),
        ("settings.textInputForeground", on_surface.clone()),
        ("settings.textInputBorder", outline.clone()),
        ("settings.numberInputBackground", surface_low.clone()),
        ("settings.numberInputForeground", on_surface.clone()),
        ("settings.numberInputBorder", outline.clone()),
        ("breadcrumb.foreground", on_surface_variant.clone()),
        ("breadcrumb.background", bg.clone()),
        ("breadcrumb.focusForeground", on_surface.clone()),
        ("breadcrumb.activeSelectionForeground", primary.clone()),
        ("breadcrumbPicker.background", surface_high.clone()),
        ("editor.snippetTabstopHighlightBackground", format!("{}30", primary)),
        ("editor.snippetTabstopHighlightBorder", primary.clone()),
        ("editor.snippetFinalTabstopHighlightBackground", format!("{}30", tertiary)),
        ("editor.snippetFinalTabstopHighlightBorder", tertiary.clone()),
        ("symbolIcon.arrayForeground", secondary.clone()),
        ("symbolIcon.booleanForeground", tertiary.clone()),
        ("symbolIcon.classForeground", primary.clone()),
        ("symbolIcon.colorForeground", secondary.clone()),
        ("symbolIcon.constantForeground", tertiary.clone()),
        ("symbolIcon.constructorForeground", primary.clone()),
        ("symbolIcon.enumeratorForeground", secondary.clone()),
        ("symbolIcon.enumeratorMemberForeground", tertiary.clone()),
        ("symbolIcon.eventForeground", error_col.clone()),
        ("symbolIcon.fieldForeground", secondary.clone()),
        ("symbolIcon.fileForeground", on_surface.clone()),
        ("symbolIcon.folderForeground", on_surface.clone()),
        ("symbolIcon.functionForeground", primary.clone()),
        ("symbolIcon.interfaceForeground", secondary.clone()),
        ("symbolIcon.keyForeground", tertiary.clone()),
        ("symbolIcon.keywordForeground", secondary.clone()),
        ("symbolIcon.methodForeground", primary.clone()),
        ("symbolIcon.moduleForeground", on_surface.clone()),
        ("symbolIcon.namespaceForeground", on_surface.clone()),
        ("symbolIcon.nullForeground", on_surface_variant.clone()),
        ("symbolIcon.numberForeground", tertiary.clone()),
        ("symbolIcon.objectForeground", secondary.clone()),
        ("symbolIcon.operatorForeground", secondary.clone()),
        ("symbolIcon.packageForeground", on_surface.clone()),
        ("symbolIcon.propertyForeground", secondary.clone()),
        ("symbolIcon.referenceForeground", secondary.clone()),
        ("symbolIcon.snippetForeground", tertiary.clone()),
        ("symbolIcon.stringForeground", tertiary.clone()),
        ("symbolIcon.structForeground", primary.clone()),
        ("symbolIcon.textForeground", on_surface.clone()),
        ("symbolIcon.typeParameterForeground", secondary.clone()),
        ("symbolIcon.unitForeground", tertiary.clone()),
        ("symbolIcon.variableForeground", on_surface.clone()),
        ("notebook.editorBackground", bg.clone()),
        ("notebook.cellEditorBackground", bg.clone()),
        ("notebook.cellBorderColor", transparent.clone()),
        ("notebook.cellToolbarSeparator", transparent.clone()),
        ("notebook.focusedCellBackground", bg.clone()),
        ("notebookStatusRunningIcon.foreground", primary.clone()),
    ];
    let mut m: BTreeMap<String, String> = entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    if hex_to_hsl(&bg).l >= 0.5 {
        lighten_edges(&mut m, &bg, &on_surface_variant, &outline, &outline_variant, &primary);
    }
    if contrast_hex(&term("term0"), &term_bg) < 3.0 {
        m.insert("terminal.ansiBlack".into(), outline.clone());
    }
    m
}

/// `lightenEdges`: a light theme gets the boundaries a tonal dark one has from its darker surfaces.
fn lighten_edges(m: &mut BTreeMap<String, String>, bg: &str, on_surface_variant: &str, outline: &str, outline_variant: &str, primary: &str) {
    for k in ["panel.border", "sideBar.border", "editorGroup.border", "editorGroupHeader.tabsBorder", "tab.border", "activityBar.border", "statusBar.border", "titleBar.border"] {
        m.insert(k.into(), outline_variant.into());
    }
    m.insert("input.border".into(), outline.into());
    m.insert("dropdown.border".into(), outline.into());
    m.insert("checkbox.border".into(), outline.into());
    m.insert("input.placeholderForeground".into(), with_contrast(on_surface_variant, bg, 4.5, false));
    m.insert("editor.selectionBackground".into(), format!("{primary}40"));
    m.insert("editor.inactiveSelectionBackground".into(), format!("{primary}22"));
}

/// A `tokenRule`: a struct, so `scope` then `settings` in field order.
fn rule(scope: &[&str], settings: Vec<(&str, String)>) -> J {
    let settings: BTreeMap<String, J> = settings.into_iter().map(|(k, v)| (k.to_string(), J::Str(v))).collect();
    J::Obj(vec![("scope".into(), J::Arr(scope.iter().map(|s| J::from(*s)).collect())), ("settings".into(), J::Map(settings))])
}

struct Syntax {
    keyword: String,
    string: String,
    function: String,
    ty: String,
    constant: String,
    tag: String,
    property: String,
}

fn syntax_set(term_colors: &Colors, primary: &str, editor_bg: &str) -> Syntax {
    let sc = |i| syntax_color(term_colors, primary, editor_bg, i);
    Syntax {
        keyword: sc(5),
        string: sc(2),
        function: sc(4),
        ty: sc(6),
        constant: sc(3),
        tag: sc(1),
        property: with_contrast(&blend_colors(&sc(4), &sc(6), 0.5), editor_bg, 4.5, hex_to_hsl(editor_bg).l < 0.5),
    }
}

fn generate_syntax(colors: &Colors, term_colors: &Colors) -> Vec<J> {
    let primary = pick(colors, "primary", "#d4b796");
    let on_surface = pick(colors, "on_surface", "#e3dfd9");
    let on_surface_variant = pick(colors, "on_surface_variant", "#c4bfb8");
    let error_col = saturate_color(&pick(colors, "error", "#ffb4ab"), 1.4, 0.50);
    let editor_bg = pick(colors, "background", &pick(colors, "surface", "#080809"));
    let s = syntax_set(term_colors, &primary, &editor_bg);
    let (col_keyword, col_string, col_function, col_type, col_constant, col_tag, col_property) = (&s.keyword, &s.string, &s.function, &s.ty, &s.constant, &s.tag, &s.property);
    let dark = hex_to_hsl(&editor_bg).l < 0.5;
    let muted_aa = muted_ink(&on_surface_variant, &editor_bg, 0xaa, dark);
    let muted_cc = muted_ink(&on_surface_variant, &editor_bg, 0xcc, dark);
    vec![
        rule(&["comment", "punctuation.definition.comment"], vec![("foreground", muted_aa.clone()), ("fontStyle", "italic".to_string())]),
        rule(&["comment.block.documentation", "comment.block.javadoc"], vec![("foreground", muted_cc.clone()), ("fontStyle", "italic".to_string())]),
        rule(&["keyword", "storage.type", "storage.modifier"], vec![("foreground", col_keyword.clone())]),
        rule(&["keyword.control", "keyword.control.flow"], vec![("foreground", col_keyword.clone())]),
        rule(&["keyword.operator", "keyword.operator.assignment"], vec![("foreground", col_keyword.clone())]),
        rule(&["keyword.other.unit"], vec![("foreground", col_constant.clone())]),
        rule(&["constant", "constant.language", "constant.character"], vec![("foreground", col_constant.clone())]),
        rule(&["constant.numeric", "constant.numeric.integer", "constant.numeric.float"], vec![("foreground", col_constant.clone())]),
        rule(&["constant.other.color", "constant.other.symbol"], vec![("foreground", col_constant.clone())]),
        rule(&["string", "string.quoted"], vec![("foreground", col_string.clone())]),
        rule(&["string.regexp"], vec![("foreground", col_tag.clone())]),
        rule(&["string.template", "string.interpolated"], vec![("foreground", col_string.clone())]),
        rule(&["string.other.link"], vec![("foreground", col_function.clone())]),
        rule(&["punctuation.definition.string"], vec![("foreground", col_string.clone())]),
        rule(&["constant.character.escape", "string.escape"], vec![("foreground", col_tag.clone())]),
        rule(&["entity.name.function", "support.function"], vec![("foreground", col_function.clone())]),
        rule(&["meta.function-call", "entity.name.function.call"], vec![("foreground", col_function.clone())]),
        rule(&["support.function.builtin"], vec![("foreground", col_function.clone())]),
        rule(&["entity.name.function.decorator", "meta.decorator"], vec![("foreground", col_tag.clone()), ("fontStyle", "italic".to_string())]),
        rule(&["entity.name.type", "entity.name.class", "support.type", "support.class"], vec![("foreground", col_type.clone())]),
        rule(&["entity.other.inherited-class"], vec![("foreground", col_type.clone()), ("fontStyle", "italic".to_string())]),
        rule(&["entity.name.type.interface"], vec![("foreground", col_type.clone())]),
        rule(&["entity.name.type.enum"], vec![("foreground", col_type.clone())]),
        rule(&["support.type.builtin", "support.type.primitive"], vec![("foreground", col_type.clone())]),
        rule(&["variable", "variable.other"], vec![("foreground", on_surface.clone())]),
        rule(&["variable.language"], vec![("foreground", col_tag.clone()), ("fontStyle", "italic".to_string())]),
        rule(&["variable.parameter", "variable.parameter.function"], vec![("foreground", on_surface.clone())]),
        rule(&["variable.other.property", "support.variable.property", "variable.other.object.property"], vec![("foreground", col_property.clone())]),
        rule(&["entity.other.attribute-name"], vec![("foreground", col_property.clone())]),
        rule(&["meta.object-literal.key"], vec![("foreground", col_property.clone())]),
        rule(&["entity.name.tag"], vec![("foreground", col_tag.clone())]),
        rule(&["punctuation.definition.tag"], vec![("foreground", col_tag.clone())]),
        rule(&["entity.name.tag.css"], vec![("foreground", col_type.clone())]),
        rule(&["support.type.property-name.css"], vec![("foreground", col_property.clone())]),
        rule(&["support.constant.property-value.css"], vec![("foreground", col_constant.clone())]),
        rule(&["punctuation"], vec![("foreground", on_surface.clone())]),
        rule(&["punctuation.separator", "punctuation.terminator"], vec![("foreground", on_surface_variant.clone())]),
        rule(&["punctuation.section.embedded"], vec![("foreground", col_tag.clone())]),
        rule(&["markup.heading"], vec![("foreground", col_function.clone()), ("fontStyle", "bold".to_string())]),
        rule(&["markup.bold"], vec![("foreground", on_surface.clone()), ("fontStyle", "bold".to_string())]),
        rule(&["markup.italic"], vec![("foreground", on_surface.clone()), ("fontStyle", "italic".to_string())]),
        rule(&["markup.underline.link"], vec![("foreground", col_function.clone()), ("fontStyle", "underline".to_string())]),
        rule(&["markup.inline.raw", "markup.fenced_code"], vec![("foreground", col_string.clone())]),
        rule(&["markup.list"], vec![("foreground", col_keyword.clone())]),
        rule(&["markup.deleted"], vec![("foreground", error_col.clone())]),
        rule(&["markup.inserted"], vec![("foreground", col_string.clone())]),
        rule(&["markup.changed"], vec![("foreground", col_constant.clone())]),
        rule(&["invalid"], vec![("foreground", error_col.clone())]),
        rule(&["invalid.deprecated"], vec![("foreground", error_col.clone()), ("fontStyle", "italic strikethrough".to_string())]),
    ]
}

fn generate_semantic(colors: &Colors, term_colors: &Colors) -> BTreeMap<String, String> {
    let primary = pick(colors, "primary", "#d4b796");
    let on_surface = pick(colors, "on_surface", "#e3dfd9");
    let on_surface_variant = pick(colors, "on_surface_variant", "#c4bfb8");
    let editor_bg = pick(colors, "background", &pick(colors, "surface", "#080809"));
    let s = syntax_set(term_colors, &primary, &editor_bg);
    let (col_keyword, col_string, col_function, col_type, col_constant, col_tag, col_property) = (&s.keyword, &s.string, &s.function, &s.ty, &s.constant, &s.tag, &s.property);
    let muted_aa = muted_ink(&on_surface_variant, &editor_bg, 0xaa, hex_to_hsl(&editor_bg).l < 0.5);
    let entries: Vec<(&str, String)> = vec![
        ("class", col_type.clone()),
        ("enum", col_type.clone()),
        ("enumMember", col_constant.clone()),
        ("function", col_function.clone()),
        ("method", col_function.clone()),
        ("interface", col_type.clone()),
        ("namespace", col_type.clone()),
        ("parameter", on_surface.clone()),
        ("property", col_property.clone()),
        ("struct", col_type.clone()),
        ("type", col_type.clone()),
        ("typeParameter", col_constant.clone()),
        ("variable", on_surface.clone()),
        ("variable.constant", col_constant.clone()),
        ("variable.defaultLibrary", col_tag.clone()),
        ("comment", muted_aa.clone()),
        ("keyword", col_keyword.clone()),
        ("keyword.control", col_keyword.clone()),
        ("number", col_constant.clone()),
        ("string", col_string.clone()),
        ("regexp", col_tag.clone()),
        ("operator", col_keyword.clone()),
        ("decorator", col_tag.clone()),
    ];
    entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// `stripThemeForFork`: the extension out and settings.json cleaned.
fn strip_theme_for_fork(settings_path: &str, fork_key: &str) -> bool {
    let ext_dir = join(&[&get_extensions_dir(fork_key), THEME_EXTENSION_ID]);
    if is_dir(&ext_dir) {
        if let Err(e) = fs::remove_dir_all(&ext_dir) {
            eprintln!("Failed to remove extension dir {ext_dir}: {}", path_err("unlinkat", &ext_dir, &e));
        }
    }
    unregister_extension(fork_key);
    clean_legacy_extensions(fork_key);

    let Ok(data) = read_file(settings_path) else { return true };
    let (settings, err) = gojsonv::unmarshal_map(&strip_jsonc_comments(&data));
    if err.is_some() {
        return false;
    }
    // A `null` file decodes to Go's nil map: reads and deletes work, nothing is there to change.
    let Some(mut settings) = settings else { return true };
    let mut changed = false;
    // Restore the previous theme if ours is the active one.
    if let Some(J::Str(current)) = settings.get("workbench.colorTheme") {
        if current == THEME_NAME || current == THEME_NAME_LIGHT {
            match settings.get(PREV_THEME_KEY) {
                Some(J::Str(prev)) if !prev.is_empty() => {
                    let prev = prev.clone();
                    settings.insert("workbench.colorTheme".into(), J::Str(prev));
                }
                _ => {
                    settings.remove("workbench.colorTheme");
                }
            }
            changed = true;
        }
    }
    if settings.remove(PREV_THEME_KEY).is_some() {
        changed = true;
    }
    for key in ["workbench.colorCustomizations", "editor.tokenColorCustomizations", "editor.semanticTokenColorCustomizations"] {
        if settings.remove(key).is_some() {
            changed = true;
        }
    }
    if !changed {
        return true;
    }
    if let Err(e) = write_settings(settings_path, &settings) {
        eprintln!("Failed to write {settings_path}: {e}");
        return false;
    }
    true
}

fn strip_all_themes(forks: &[String]) -> BTreeMap<String, bool> {
    let mut selected: Vec<String> = forks.to_vec();
    if selected.is_empty() {
        for (key, _, name) in FORKS {
            let ext_dir = join(&[&get_extensions_dir(key), THEME_EXTENSION_ID]);
            if exists(&get_settings_path(name)) || is_dir(&ext_dir) {
                selected.push(key.to_string());
            }
        }
    }
    let mut results = BTreeMap::new();
    for fork_key in &selected {
        let Some(fork_name) = fork_name(fork_key) else { continue };
        let ok = strip_theme_for_fork(&get_settings_path(fork_name), fork_key);
        results.insert(fork_key.clone(), ok);
        if ok {
            println!("  ✓ {fork_name}");
        } else {
            eprintln!("  ✗ {fork_name}");
        }
    }
    results
}

/// `loadSettings`: settings.json as JSONC, never rewritten when it does not parse. `Ok(None)` is a `null` file.
fn load_settings(path: &str) -> Result<Option<BTreeMap<String, J>>, String> {
    match read_file(path) {
        Ok(data) => {
            let (settings, err) = gojsonv::unmarshal_map(&strip_jsonc_comments(&data));
            if let Some(e) = err {
                return Err(format!("parse {path}: {e}"));
            }
            Ok(settings)
        }
        Err((_, true)) => Ok(Some(BTreeMap::new())),
        Err((e, false)) => Err(e),
    }
}

fn write_settings(path: &str, settings: &BTreeMap<String, J>) -> Result<(), String> {
    mkdir_all(&dir_of(path))?;
    let mut data = marshal_indent(&J::Map(settings.clone()), "", "  ");
    data.push('\n');
    write_file(path, data.as_bytes())
}

fn get_settings_path(fork_name: &str) -> String {
    join(&[&config_dir(), fork_name, "User", "settings.json"])
}

fn config_dir() -> String {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(x) if !x.is_empty() => x,
        _ => join(&[&home(), ".config"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc() {
        let s = strip_jsonc_comments(b"// c\n{\"a\": \"//x\", /* b */ \"b\": [1,],\n}");
        assert_eq!(String::from_utf8(s).unwrap(), "\n{\"a\": \"//x\",  \"b\": [1]\n}");
    }

    #[test]
    fn muted_short_colour_kept() {
        assert_eq!(muted_ink("abc", "#000000", 0xaa, true), "#abc");
    }
}
