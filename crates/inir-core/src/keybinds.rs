//! The keybind cheatsheets, ported from `parse_niri_keybinds.py` (niri's config.kdl) and `hyprland/get_keybinds.py`:
//! the JSON the shell's NiriKeybinds and HyprlandKeybinds services read.

use crate::material::py::expanduser;
use crate::pyrt::{self, R};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

struct Bind {
    mods: Vec<String>,
    key: String,
    action: String,
    comment: String,
}

const TERMINALS: [&str; 7] = ["foot", "kitty", "alacritty", "wezterm", "ghostty", "konsole", "gnome-terminal"];
const FILE_MANAGERS: [&str; 6] = ["dolphin", "nautilus", "thunar", "nemo", "pcmanfm", "ranger"];
const BROWSERS: [&str; 5] = ["firefox", "zen-browser", "chromium", "brave", "vivaldi"];

fn action_name(action: &str) -> Option<&'static str> {
    Some(match action {
        "toggle-overview" => "Niri Overview",
        "quit" => "Quit Niri",
        "toggle-keyboard-shortcuts-inhibit" => "Toggle shortcuts inhibit",
        "power-off-monitors" => "Power off monitors",
        "show-hotkey-overlay" => "Niri hotkey overlay",
        "close-window" => "Close window",
        "maximize-column" => "Maximize column",
        "maximize-window-to-edges" => "Maximize to edges",
        "fullscreen-window" => "Fullscreen",
        "toggle-window-floating" => "Toggle floating",
        "switch-focus-between-floating-and-tiling" => "Switch float/tile focus",
        "center-column" => "Center column",
        "center-visible-columns" => "Center visible columns",
        "expand-column-to-available-width" => "Expand to available width",
        "consume-or-expel-window-left" => "Consume/expel left",
        "consume-or-expel-window-right" => "Consume/expel right",
        "expel-window-from-column" => "Expel from column",
        "consume-window-into-column" => "Consume into column",
        "switch-preset-column-width" => "Cycle column width",
        "switch-preset-window-height" => "Cycle window height",
        "reset-window-height" => "Reset window height",
        "toggle-column-tabbed-display" => "Toggle tabbed display",
        "focus-column-left" => "Focus left",
        "focus-column-right" => "Focus right",
        "focus-window-up" => "Focus up",
        "focus-window-down" => "Focus down",
        "focus-column-first" => "Focus first column",
        "focus-column-last" => "Focus last column",
        "focus-monitor-left" => "Focus monitor left",
        "focus-monitor-right" => "Focus monitor right",
        "focus-monitor-up" => "Focus monitor up",
        "focus-monitor-down" => "Focus monitor down",
        "move-column-left" => "Move left",
        "move-column-right" => "Move right",
        "move-window-up" => "Move up",
        "move-window-down" => "Move down",
        "move-column-to-first" => "Move to first",
        "move-column-to-last" => "Move to last",
        "move-column-to-monitor-left" => "Move to monitor left",
        "move-column-to-monitor-right" => "Move to monitor right",
        "move-column-to-monitor-up" => "Move to monitor up",
        "move-column-to-monitor-down" => "Move to monitor down",
        "focus-workspace-up" => "Previous workspace",
        "focus-workspace-down" => "Next workspace",
        "move-column-to-workspace-up" => "Move to prev workspace",
        "move-column-to-workspace-down" => "Move to next workspace",
        "move-workspace-up" => "Move workspace up",
        "move-workspace-down" => "Move workspace down",
        "screenshot" => "Screenshot",
        "screenshot-screen" => "Screenshot screen",
        "screenshot-window" => "Screenshot window",
        _ => return None,
    })
}

fn ipc_name(target: &str, func: &str) -> Option<&'static str> {
    Some(match (target, func) {
        ("altSwitcher", "next") => "Next window",
        ("altSwitcher", "previous") => "Previous window",
        ("overlay", "toggle") => "iNiR Overlay",
        ("overview", "toggle") => "iNiR Overview",
        ("clipboard", "toggle") => "Clipboard",
        ("lock", "activate") => "Lock screen",
        ("region", "screenshot") => "Screenshot region",
        ("region", "ocr") => "OCR region",
        ("region", "search") => "Reverse image search",
        ("wallpaperSelector", "toggle") => "Wallpaper selector",
        ("settings", "open") => "Settings",
        ("cheatsheet", "toggle") => "Cheatsheet",
        ("panelFamily", "cycle") => "Cycle panel style",
        ("session", "toggle") => "Session dialog",
        ("browser", "open") => "Browser",
        ("audio", "volumeUp") => "Volume up",
        ("audio", "volumeDown") => "Volume down",
        ("audio", "mute") => "Mute audio",
        ("audio", "micMute") => "Mute microphone",
        ("brightness", "increment") => "Brightness up",
        ("brightness", "decrement") => "Brightness down",
        ("mpris", "playPause") => "Play/Pause",
        ("mpris", "next") => "Next track",
        ("mpris", "previous") => "Previous track",
        ("notifications", "clearAll") => "Clear notifications",
        ("gamemode", "toggle") => "Toggle game mode",
        ("launcher", "terminal") => "Terminal",
        ("launcher", "close-window") => "Close window",
        _ => return None,
    })
}

fn parse_inir_action(action: &str) -> Option<(String, String)> {
    static IPC: LazyLock<Regex> =
        LazyLock::new(|| pyrt::re(r#"spawn\s+"(?:[^"]*/)?inir"\s+"ipc"\s+"call"\s+"([\w-]+)"\s+"([\w-]+)""#));
    static FIXED: LazyLock<Vec<(Regex, &str, &str)>> = LazyLock::new(|| {
        [("settings", "settings", "open"), ("terminal", "launcher", "terminal"), ("close-window", "launcher", "close-window"), ("browser", "browser", "open")]
            .into_iter()
            .map(|(word, target, func)| (pyrt::re(&format!(r#"spawn\s+"(?:[^"]*/)?inir"\s+"{word}"(?:\s|;|$)"#)), target, func))
            .collect()
    });
    static DIRECT: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r#"spawn\s+"(?:[^"]*/)?inir"\s+"([\w-]+)"\s+"([\w-]+)""#));
    if let Some(c) = IPC.captures(action) {
        return Some((c[1].to_string(), c[2].to_string()));
    }
    if let Some((_, target, func)) = FIXED.iter().find(|(rx, _, _)| rx.is_match(action)) {
        return Some((target.to_string(), func.to_string()));
    }
    DIRECT.captures(action).map(|c| (c[1].to_string(), c[2].to_string()))
}

fn generate_comment(action: &str) -> String {
    static RECENT: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"^(next|previous)-window\b(.*)"));
    static WORKSPACE: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"^(focus-workspace|move-column-to-workspace)\s+(\d+)"));
    static RESIZE: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r#"^set-(column-width|window-height)\s+"([+-]\d+%?)""#));
    static IPC_ANY: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r#"ipc.*call.*"(\w+)".*"(\w+)""#));
    static SPAWN: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r#"spawn\s+"([^"]+)""#));
    let action = action.trim();
    if let Some(name) = action_name(action) {
        return name.into();
    }
    if let Some(c) = RECENT.captures(action) {
        let same_app = if c[2].contains("app-id") { " of this app" } else { "" };
        return format!("Recent windows{same_app} ({})", &c[1]);
    }
    if let Some(c) = WORKSPACE.captures(action) {
        let verb = if c[1].contains("focus") { "Focus" } else { "Move to" };
        return format!("{verb} workspace {}", &c[2]);
    }
    if let Some(c) = RESIZE.captures(action) {
        let target = if c[1].contains("column") { "column" } else { "window" };
        let direction = if c[2].starts_with('-') { "Shrink" } else { "Grow" };
        return format!("{direction} {target} {}", c[2].trim_start_matches(&['+', '-'][..]));
    }
    if action.starts_with("spawn") {
        let ipc = parse_inir_action(action).or_else(|| IPC_ANY.captures(action).map(|c| (c[1].to_string(), c[2].to_string())));
        if let Some((target, func)) = ipc {
            return ipc_name(&target, &func).map_or_else(|| format!("{target} {func}"), String::from);
        }
        let has = |names: &[&str]| names.iter().any(|n| action.contains(n));
        if action.contains("launch-terminal.sh") || has(&TERMINALS) {
            return "Terminal".into();
        }
        if has(&FILE_MANAGERS) {
            return "File manager".into();
        }
        if has(&BROWSERS) {
            return "Browser".into();
        }
        if action.contains("wpctl") {
            if action.contains("set-volume") {
                return if action.contains('+') { "Volume up" } else { "Volume down" }.into();
            }
            if action.contains("set-mute") {
                return "Mute toggle".into();
            }
        }
        if action.contains("brightnessctl") || action.contains("light") {
            return if action.contains('+') || action.contains("inc") { "Brightness up" } else { "Brightness down" }.into();
        }
        if action.contains("close-window") {
            return "Close window".into();
        }
        if let Some(c) = SPAWN.captures(action) {
            return c[1].rsplit('/').next().unwrap_or_default().to_string();
        }
    }
    if action.chars().count() > 30 { format!("{}...", action.chars().take(30).collect::<String>()) } else { action.to_string() }
}

fn categorize(bind: &Bind) -> &'static str {
    static IPC_SHELL: LazyLock<Regex> =
        LazyLock::new(|| pyrt::re(r"ipc.*call.*(overlay|overview|clipboard|lock|wallpaper|settings|cheatsheet|panelfamily)"));
    let comment = bind.comment.to_lowercase();
    let action = bind.action.to_lowercase();
    let in_comment = |words: &[&str]| words.iter().any(|w| comment.contains(w));
    let in_action = |words: &[&str]| words.iter().any(|w| action.contains(w));
    if in_comment(&["niri overview", "quit niri", "inhibit", "power off", "hotkey overlay"]) {
        return "System";
    }
    if in_comment(&["inir ", "clipboard", "lock screen", "wallpaper", "settings", "cheatsheet", "panel style"]) {
        return "iNiR Shell";
    }
    if let Some((target, _)) = parse_inir_action(&action) {
        match target.as_str() {
            "overlay" | "overview" | "clipboard" | "lock" | "wallpaperSelector" | "settings" | "cheatsheet" | "panelFamily"
            | "session" => return "iNiR Shell",
            "altSwitcher" => return "Window Switcher",
            "audio" | "mpris" => return "Media",
            "brightness" => return "Brightness",
            _ => {}
        }
    }
    if IPC_SHELL.is_match(&action) {
        return "iNiR Shell";
    }
    if comment.contains("window") && (comment.contains("next") || comment.contains("previous")) || action.contains("altswitcher") {
        return "Window Switcher";
    }
    if in_comment(&["screenshot", "ocr", "image search"]) {
        return "Screenshots";
    }
    if in_comment(&["terminal", "file manager", "browser"]) || in_action(&TERMINALS) || in_action(&FILE_MANAGERS) || in_action(&BROWSERS) {
        return "Applications";
    }
    if in_comment(&["close", "maximize", "fullscreen", "floating", "consume", "expel", "float/tile"]) || action.contains("close-window") {
        return "Window Management";
    }
    if in_comment(&["cycle column", "cycle window", "reset window", "center column", "center visible", "expand to available", "tabbed"])
        || in_action(&["switch-preset-column", "switch-preset-window", "reset-window-height", "center-column", "center-visible", "expand-column", "toggle-column-tabbed"])
    {
        return "Layout";
    }
    if in_comment(&["shrink column", "grow column", "shrink window", "grow window"]) || in_action(&["set-column-width", "set-window-height"]) {
        return "Resize";
    }
    if comment.contains("monitor") || in_action(&["focus-monitor", "move-column-to-monitor", "move-window-to-monitor", "move-workspace-to-monitor"]) {
        return "Monitors";
    }
    if comment.contains("focus") && !comment.contains("workspace") {
        return "Focus";
    }
    if comment.contains("move") && !comment.contains("workspace") && !comment.contains("track") {
        return "Move Windows";
    }
    if comment.contains("workspace") {
        return "Workspaces";
    }
    if in_comment(&["volume", "mute", "play", "pause", "track", "audio", "microphone"]) || action.contains("mpris") || action.contains("audio") {
        return "Media";
    }
    if comment.contains("brightness") {
        return "Brightness";
    }
    "Other"
}

fn parse_block(block: &str) -> Vec<Bind> {
    static START: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"^([A-Za-z0-9+_]+)\s*(.*?)(\{.*)$"));
    static TITLE: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r#"hotkey-overlay-title="([^"]+)""#));
    static WHOLE: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"\{[^}]+\}"));
    static ACTION: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"\{\s*([^}]+)\s*\}"));
    let lines: Vec<&str> = block.split('\n').collect();
    let mut binds = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let Some(m) = (!line.is_empty() && !line.starts_with("//")).then(|| START.captures(line)).flatten() else {
            i += 1;
            continue;
        };
        let title = TITLE.captures(m[2].trim()).map(|t| t[1].to_string());
        let rest = &m[3];
        let action = if WHOLE.is_match(rest) {
            ACTION.captures(rest).map(|a| a[1].trim().trim_end_matches(';').to_string()).unwrap_or_default()
        } else {
            let mut parts = Vec::new();
            i += 1;
            while i < lines.len() {
                let inner = lines[i].trim();
                if inner == "}" {
                    break;
                }
                if !inner.is_empty() && !inner.starts_with("//") {
                    parts.push(inner.trim_end_matches(';'));
                }
                i += 1;
            }
            parts.join(" ")
        };
        let combo: Vec<&str> = m[1].split('+').collect();
        let (key, mods) = combo.split_last().unwrap();
        let mods = mods.iter().map(|p| if matches!(*p, "Mod" | "Super") { "Super".to_string() } else { p.to_string() }).collect();
        let key = if key.starts_with("XF86Audio") {
            key.replace("XF86Audio", "").replace("RaiseVolume", "Vol+").replace("LowerVolume", "Vol-")
        } else if key.starts_with("XF86MonBrightness") {
            key.replace("XF86MonBrightness", "Brightness").replace("Up", "+").replace("Down", "-")
        } else if key.starts_with("XF86") {
            key.replace("XF86", "")
        } else {
            key.to_string()
        };
        let comment = title.unwrap_or_else(|| generate_comment(&action));
        binds.push(Bind { mods, key, action, comment });
        i += 1;
    }
    binds
}

// The name before a `{`: the longest run of name characters, then spaces, ending right at it (64 back at most).
fn header_name(text: &[char], brace: usize) -> String {
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut name = String::new();
    for back in (brace.saturating_sub(63)..brace).rev() {
        let mut j = back;
        while j < brace && is_name(text[j]) {
            j += 1;
        }
        let name_end = j;
        while j < brace && pyrt::is_space(text[j]) {
            j += 1;
        }
        if name_end > back && j == brace {
            name = text[back..name_end].iter().collect();
        } else if !name.is_empty() {
            break;
        }
    }
    name
}

fn block_end(text: &[char], start: usize) -> usize {
    let mut depth = 1;
    let mut i = start;
    while i < text.len() && depth > 0 {
        match text[i] {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    i - 1
}

// Every top-level `binds { }` in file order, and the `binds { }` inside `recent-windows { }`.
fn binds_blocks(content: &str) -> (Vec<String>, Vec<String>) {
    let text: Vec<char> = content
        .split('\n')
        .map(|l| if l.trim_start().starts_with("//") { "" } else { l })
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .collect();
    let (mut normal, mut recent) = (Vec::new(), Vec::new());
    let mut parents: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            '{' => {
                let name = header_name(&text, i);
                let in_recent = depth == 1 && parents.last().is_some_and(|p| p == "recent-windows");
                if name == "binds" && (depth == 0 || in_recent) {
                    let end = block_end(&text, i + 1);
                    let block = text[i + 1..end.max(i + 1)].iter().collect();
                    if depth == 0 { normal.push(block) } else { recent.push(block) }
                    i = end + 1;
                    continue;
                }
                parents.push(name);
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                parents.pop();
            }
            _ => {}
        }
        i += 1;
    }
    (normal, recent)
}

fn resolve_includes(content: &str, dir: &Path) -> R<String> {
    static INCLUDE: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r#"(?m)^include\s+"([^"]+)"\s*$"#));
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    for c in INCLUDE.captures_iter(content) {
        let whole = c.get(0).unwrap();
        out.push_str(&content[last..whole.start()]);
        let path = dir.join(&c[1]);
        if path.exists() {
            out.push_str(&pyrt::read_text(&path)?);
        }
        last = whole.end();
    }
    out.push_str(&content[last..]);
    Ok(out)
}

pub fn niri_keybinds(path: &Path) -> R<Value> {
    if !path.exists() {
        return Ok(json!({"error": format!("Config not found: {}", path.display()), "children": []}));
    }
    let content = resolve_includes(&pyrt::read_text(path)?, path.parent().unwrap_or(Path::new("")))?;
    let (normal, recent) = binds_blocks(&content);
    if normal.is_empty() && recent.is_empty() {
        return Ok(json!({"error": "No binds block found", "children": []}));
    }
    // niri: a later binds block overrides an earlier one on the same keys; recent-windows binds only fill gaps
    let mut merged: Vec<Bind> = Vec::new();
    let mut at: HashMap<(Vec<String>, String), usize> = HashMap::new();
    for (blocks, later_wins) in [(&normal, true), (&recent, false)] {
        for bind in blocks.iter().flat_map(|b| parse_block(b)) {
            match at.get(&(bind.mods.clone(), bind.key.clone())) {
                Some(&i) if later_wins => merged[i] = bind,
                Some(_) => {}
                None => {
                    at.insert((bind.mods.clone(), bind.key.clone()), merged.len());
                    merged.push(bind);
                }
            }
        }
    }
    const ORDER: [&str; 15] = [
        "System", "iNiR Shell", "Window Switcher", "Screenshots", "Applications", "Window Management", "Layout", "Resize",
        "Focus", "Move Windows", "Monitors", "Workspaces", "Media", "Brightness", "Other",
    ];
    let mut by_category: HashMap<&str, Vec<Value>> = HashMap::new();
    for bind in &merged {
        by_category.entry(categorize(bind)).or_default().push(json!({"mods": bind.mods, "key": bind.key, "comment": bind.comment}));
    }
    let children: Vec<Value> = ORDER
        .iter()
        .filter_map(|cat| by_category.remove(cat).map(|binds| json!({"name": cat, "children": [{"keybinds": binds}]})))
        .collect();
    Ok(json!({"children": children, "configPath": path.to_string_lossy()}))
}

pub fn run_niri_cli(args: &[String]) -> i32 {
    let path = match args.first() {
        Some(p) => PathBuf::from(p),
        None => {
            let base = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| expanduser("~/.config"));
            Path::new(&base).join("niri").join("config.kdl")
        }
    };
    match niri_keybinds(&path) {
        Ok(v) => {
            println!("{v}");
            0
        }
        Err(e) => pyrt::die(e),
    }
}

// ---------------------------------------------------------------- Hyprland

fn hypr_comment(dispatcher: &str, params: &str) -> String {
    let direction = || match params {
        "l" => "left",
        "r" => "right",
        "u" => "up",
        "d" => "down",
        _ => "null",
    };
    match dispatcher {
        "resizewindow" => "Resize window".into(),
        "movewindow" if params.is_empty() => "Move window".into(),
        "movewindow" => format!("Window: move in {} direction", direction()),
        "pin" => "Window: pin (show on all workspaces)".into(),
        "splitratio" => format!("Window split ratio {params}"),
        "togglefloating" => "Float/unfloat window".into(),
        "resizeactive" => format!("Resize window by {params}"),
        "killactive" => "Close window".into(),
        "fullscreen" => format!(
            "Toggle {}",
            match params {
                "0" => "fullscreen",
                "1" => "maximization",
                "2" => "fullscreen on Hyprland's side",
                _ => "null",
            }
        ),
        "fakefullscreen" => "Toggle fake fullscreen".into(),
        "workspace" => match params {
            "+1" => "Workspace: focus right".into(),
            "-1" => "Workspace: focus left".into(),
            _ => format!("Focus workspace {params}"),
        },
        "movefocus" => format!("Window: move focus {}", direction()),
        "swapwindow" => format!("Window: swap in {} direction", direction()),
        "movetoworkspace" => match params {
            "+1" => "Window: move to right workspace (non-silent)".into(),
            "-1" => "Window: move to left workspace (non-silent)".into(),
            _ => format!("Window: move to workspace {params} (non-silent)"),
        },
        "movetoworkspacesilent" => match params {
            "+1" => "Window: move to right workspace".into(),
            "-1" => "Window: move to left workspace".into(),
            _ => format!("Window: move to workspace {params}"),
        },
        "togglespecialworkspace" => "Workspace: toggle special".into(),
        "exec" => format!("Execute: {params}"),
        _ => String::new(),
    }
}

fn hypr_bind(line: &str) -> R<Option<Value>> {
    let Some((_, keys)) = line.split_once('=') else {
        return Err(pyrt::exc("ValueError", "not enough values to unpack (expected 2, got 1)"));
    };
    let (keys, comment) = match keys.split_once('#') {
        Some((k, c)) => (k, Some(c.trim())),
        None => (keys, None),
    };
    let fields: Vec<&str> = keys.splitn(5, ',').map(str::trim).collect();
    if fields.len() < 3 {
        return Err(pyrt::exc("ValueError", format!("not enough values to unpack (expected at least 3, got {})", fields.len())));
    }
    let params = fields[3..].concat();
    let comment = match comment {
        Some(c) if c.starts_with("[hidden]") => return Ok(None),
        Some(c) => c.to_string(),
        None => hypr_comment(fields[2], &params),
    };
    // a mod is what sits between separators; one-letter runs are dropped, as the python did
    let chars: Vec<char> = format!("{}+", fields[0]).chars().collect();
    let mut mods = Vec::new();
    let mut p = 0;
    if !fields[0].is_empty() {
        for (i, c) in chars.iter().enumerate() {
            if matches!(c, '+' | ' ') {
                if i - p > 1 {
                    mods.push(chars[p..i].iter().collect::<String>());
                }
                p = i + 1;
            }
        }
    }
    Ok(Some(json!({"mods": mods, "key": fields[1], "dispatcher": fields[2], "params": params, "comment": comment})))
}

struct Hypr {
    lines: Vec<String>,
    pos: usize,
}

impl Hypr {
    // `#!` opens a section, `##!` one level down; a heading at or above `scope` ends this one
    fn section(&mut self, name: String, scope: usize) -> R<Value> {
        static HEADING: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"^#+!"));
        let (mut children, mut keybinds) = (Vec::new(), Vec::new());
        while self.pos < self.lines.len() {
            let line = self.lines[self.pos].clone();
            if HEADING.is_match(&line) {
                let level = line.find('!').unwrap();
                if level <= scope {
                    self.pos -= 1;
                    break;
                }
                self.pos += 1;
                children.push(self.section(line[level + 1..].trim().to_string(), level)?);
            } else if line.starts_with("#/#") || line.trim_start().starts_with("bind") {
                keybinds.extend(hypr_bind(&line)?);
            }
            self.pos += 1;
        }
        Ok(json!({"children": children, "keybinds": keybinds, "name": name}))
    }
}

fn expandvars(path: &str) -> String {
    static VAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$([A-Za-z0-9_]+|\{[^}]*\})").unwrap());
    VAR.replace_all(path, |c: &regex::Captures| {
        let name = c[1].trim_start_matches('{').trim_end_matches('}');
        std::env::var(name).unwrap_or_else(|_| c[0].to_string())
    })
    .into_owned()
}

pub fn run_hyprland_cli(args: &[String]) -> i32 {
    let mut path = "$HOME/.config/hypr/hyprland.conf".to_string();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--path" {
            path = it.next().cloned().unwrap_or_default();
        } else if let Some(v) = a.strip_prefix("--path=") {
            path = v.to_string();
        }
    }
    let path = expanduser(&expandvars(&path));
    let readable = CString::new(path.as_str()).is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::R_OK) } == 0);
    let text = if readable {
        match pyrt::read_text(Path::new(&path)) {
            Ok(t) => t,
            Err(e) => return pyrt::die(e),
        }
    } else {
        "error".to_string()
    };
    let lines: Vec<String> = pyrt::splitlines(&text).into_iter().map(String::from).collect();
    match lines.first().map(String::as_str) {
        None => return pyrt::die(pyrt::exc("IndexError", "list index out of range")),
        Some("error") => {
            println!("\"error\"");
            return 0;
        }
        _ => {}
    }
    match (Hypr { lines, pos: 0 }).section(String::new(), 0) {
        Ok(v) => {
            println!("{v}");
            0
        }
        Err(e) => pyrt::die(e),
    }
}
