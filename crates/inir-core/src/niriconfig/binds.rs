//! Keybinds in `config.d/70-binds.kdl` (read with a description and category each, set and commented out in
//! place) and who owns Alt+Tab: iNiR's switcher is a managed binds block at the end of the config.

use super::set::write_managed;
use super::tables::{
    ALT_TAB_OVERRIDE_END, ALT_TAB_OVERRIDE_START, KB_ACTION_MAP, KB_BROWSERS as BROWSERS, KB_CONFIG_ONLY_ACTIONS, KB_FILE_MANAGERS as FILE_MANAGERS, KB_IPC_MAP,
    KB_TERMINALS as TERMINALS,
};
use super::*;

/// Action names this niri knows, from its own CLI help. Empty when niri is missing.
fn niri_actions() -> Vec<String> {
    let Ok(r) = run(&["niri", "msg", "action", "--help"], Some(3)) else { return Vec::new() };
    let mut names = Vec::new();
    let mut listing = false;
    let name = rx(r"^  ([a-z][a-z0-9-]*)\b");
    for line in pyrt::splitlines(&r.out) {
        if line.starts_with("Actions:") {
            listing = true;
        } else if listing && !line.is_empty() && !line.starts_with(' ') {
            break;
        } else if listing {
            if let Some(c) = name.captures(line) {
                names.push(c[1].to_string());
            }
        }
    }
    if !names.is_empty() {
        names.extend(KB_CONFIG_ONLY_ACTIONS.iter().map(|a| a.to_string()));
    }
    names
}

/// Text that doesn't start with a niri action is a command: `code` becomes `spawn-sh "code"`.
fn normalize_action(action: &str) -> String {
    let action = strip(strip(action).trim_end_matches(';'));
    if action.is_empty() || action.contains(['{', '}', ';']) {
        return action.to_string();
    }
    let known = niri_actions();
    let first = action.split(py::is_space).find(|w| !w.is_empty()).unwrap_or_default();
    if known.is_empty() || known.iter().any(|k| k == first) {
        return action.to_string();
    }
    format!("spawn-sh \"{}\"", action.replace('\\', "\\\\").replace('"', "\\\""))
}

fn parse_inir_action(action: &str) -> Option<(String, String)> {
    let inir = r#"spawn\s+"(?:[^"]*/)?inir"\s+"#;
    if let Some(c) = rx(&format!(r#"{inir}"ipc"\s+"call"\s+"([\w-]+)"\s+"([\w-]+)""#)).captures(action) {
        return Some((c[1].to_string(), c[2].to_string()));
    }
    for (word, target, func) in [("settings", "settings", "open"), ("terminal", "launcher", "terminal"), ("close-window", "launcher", "close-window"), ("browser", "browser", "open")] {
        if rx(&format!(r#"{inir}"{word}"(?:\s|;|$)"#)).is_match(action) {
            return Some((target.into(), func.into()));
        }
    }
    let c = rx(&format!(r#"{inir}"([\w-]+)"\s+"([\w-]+)""#)).captures(action)?;
    Some((c[1].to_string(), c[2].to_string()))
}

fn ipc_name(target: &str, func: &str) -> String {
    KB_IPC_MAP.iter().find(|(t, f, _)| *t == target && *f == func).map_or_else(|| format!("{target} {func}"), |(_, _, n)| n.to_string())
}

/// A readable description of a niri action.
fn generate_comment(action: &str) -> String {
    let action = strip(action);
    if let Some((_, d)) = KB_ACTION_MAP.iter().find(|(a, _)| *a == action) {
        return d.to_string();
    }
    if let Some(c) = rx(r"^(focus-workspace|move-column-to-workspace)\s+([0-9]+)").captures(action) {
        let verb = if c[1].contains("focus") { "Focus" } else { "Move to" };
        return format!("{verb} workspace {}", &c[2]);
    }
    if let Some(c) = rx(r#"^set-(column-width|window-height)\s+"([+-][0-9]+%?)""#).captures(action) {
        let target = if c[1].contains("column") { "column" } else { "window" };
        let dir = if c[2].starts_with('-') { "Shrink" } else { "Grow" };
        return format!("{dir} {target} {}", c[2].trim_start_matches(['+', '-']));
    }
    if action.starts_with("spawn") {
        if let Some((t, f)) = parse_inir_action(action) {
            return ipc_name(&t, &f);
        }
        if let Some(c) = rx(r#"ipc.*call.*"(\w+)".*"(\w+)""#).captures(action) {
            return ipc_name(&c[1], &c[2]);
        }
        if action.contains("launch-terminal.sh") || TERMINALS.iter().any(|t| action.contains(t)) {
            return "Terminal".into();
        }
        if FILE_MANAGERS.iter().any(|f| action.contains(f)) {
            return "File manager".into();
        }
        if BROWSERS.iter().any(|b| action.contains(b)) {
            return "Browser".into();
        }
        if action.contains("wpctl") {
            return if action.contains("set-volume") { if action.contains('+') { "Volume up" } else { "Volume down" } } else { "Mute toggle" }.into();
        }
        if action.contains("brightnessctl") || action.contains("light") {
            return if action.contains('+') || action.contains("inc") { "Brightness up" } else { "Brightness down" }.into();
        }
        if action.contains("close-window") {
            return "Close window".into();
        }
        if let Some(c) = rx(r#"spawn\s+"([^"]+)""#).captures(action) {
            return c[1].rsplit('/').next().unwrap_or_default().to_string();
        }
    }
    if action.chars().count() > 30 { format!("{}...", action.chars().take(30).collect::<String>()) } else { action.to_string() }
}

fn categorize(description: &str, action: &str) -> &'static str {
    let desc = description.to_lowercase();
    let act = action.to_lowercase();
    let d_any = |xs: &[&str]| xs.iter().any(|x| desc.contains(x));
    let a_any = |xs: &[&str]| xs.iter().any(|x| act.contains(x));
    if d_any(&["niri overview", "quit niri", "inhibit", "power off", "hotkey overlay"]) {
        return "System";
    }
    if d_any(&["inir ", "clipboard", "lock screen", "wallpaper", "settings", "cheatsheet", "panel style"]) {
        return "iNiR Shell";
    }
    if let Some((target, _)) = parse_inir_action(action) {
        match target.as_str() {
            "overlay" | "overview" | "clipboard" | "lock" | "wallpaperSelector" | "settings" | "cheatsheet" | "panelFamily" | "session" => return "iNiR Shell",
            "altSwitcher" => return "Window Switcher",
            "audio" | "mpris" => return "Media",
            "brightness" => return "Brightness",
            _ => {}
        }
    }
    if rx(r"ipc.*call.*(overlay|overview|clipboard|lock|wallpaper|settings|cheatsheet|panelfamily)").is_match(&act) {
        return "iNiR Shell";
    }
    if desc.contains("window") && (desc.contains("next") || desc.contains("previous")) || act.contains("altswitcher") {
        return "Window Switcher";
    }
    if d_any(&["screenshot", "ocr", "image search"]) {
        return "Screenshots";
    }
    if d_any(&["terminal", "file manager", "browser"]) || a_any(TERMINALS) || a_any(FILE_MANAGERS) || a_any(BROWSERS) {
        return "Applications";
    }
    if d_any(&["close", "maximize", "fullscreen", "floating", "consume", "expel", "float/tile"]) || act.contains("close-window") {
        return "Window Management";
    }
    if d_any(&["cycle column", "cycle window", "reset window", "center column", "center visible", "expand to available", "tabbed"])
        || a_any(&["switch-preset-column", "switch-preset-window", "reset-window-height", "center-column", "center-visible", "expand-column", "toggle-column-tabbed"])
    {
        return "Layout";
    }
    if d_any(&["shrink column", "grow column", "shrink window", "grow window"]) || a_any(&["set-column-width", "set-window-height"]) {
        return "Resize";
    }
    if desc.contains("monitor") || a_any(&["focus-monitor", "move-column-to-monitor", "move-window-to-monitor", "move-workspace-to-monitor"]) {
        return "Monitors";
    }
    if desc.contains("focus") && !desc.contains("workspace") {
        return "Focus";
    }
    if desc.contains("move") && !desc.contains("workspace") && !desc.contains("track") {
        return "Move Windows";
    }
    if desc.contains("workspace") {
        return "Workspaces";
    }
    if d_any(&["volume", "mute", "play", "pause", "track", "audio", "microphone"]) || act.contains("mpris") || act.contains("audio") {
        return "Media";
    }
    if desc.contains("brightness") {
        return "Brightness";
    }
    "Other"
}

/// The lines a bind takes (its multi-line body included), active or commented out.
fn find_in_block(lines: &[&str], combo: &str, commented: bool) -> Option<(usize, usize)> {
    let head = rx(&format!(r"^{}(?:\s|$|\{{)", esc(combo)));
    let single = rx(r"\{[^}]*\}");
    for (i, raw) in lines.iter().enumerate() {
        let t = strip(raw);
        let candidate = match (commented, t.strip_prefix("//")) {
            (true, Some(rest)) => lstrip(rest),
            (false, None) => t,
            _ => continue,
        };
        if !head.is_match(candidate) {
            continue;
        }
        if single.is_match(candidate) || !candidate.contains('{') {
            return Some((i, i + 1));
        }
        let mut depth = candidate.matches('{').count() as i64 - candidate.matches('}').count() as i64;
        let mut j = i + 1;
        while j < lines.len() && depth > 0 {
            let inner = strip(lines[j]);
            depth += inner.matches('{').count() as i64 - inner.matches('}').count() as i64;
            j += 1;
        }
        return Some((i, j));
    }
    None
}

fn binds_file() -> Result<(PathBuf, String, Bounds), i32> {
    let file = section_file("config.d/70-binds.kdl");
    if !file.exists() {
        return Err(error(format!("Binds file not found: {}", path_str(&file))));
    }
    let content = pyrt::read_text(&file).map_err(pyrt::die)?;
    let Some(bounds) = find_block_bounds(&content, "binds", true) else { return Err(error("No binds { } block found in file")) };
    Ok((file, content, bounds))
}

const CATEGORY_ORDER: &[&str] = &[
    "System", "iNiR Shell", "Window Switcher", "Screenshots", "Applications", "Window Management", "Layout", "Resize", "Focus", "Move Windows", "Monitors", "Workspaces", "Media", "Brightness", "Other",
];

pub(super) fn get_binds() -> R<i32> {
    let (file, content, (_, a, b, _)) = match binds_file() {
        Ok(v) => v,
        Err(code) => return Ok(code),
    };
    let base_line = content[..a].matches('\n').count() + 1;
    let lines: Vec<&str> = content[a..b].split('\n').collect();
    let commented_bind = rx(r"^[A-Za-z0-9_][A-Za-z0-9+_]*\s*(?:[^{]*?)\{");
    let bind = rx(r"^([A-Za-z0-9_][A-Za-z0-9+_]*)\s*(.*?)(\{.*)$");
    let title = rx(r#"\s*hotkey-overlay-title="[^"]*""#);
    let single = rx(r"\{[^}]*\}");
    let body = rx(r"\{\s*(.*?)\s*\}");
    let mut all: Vec<(String, Json)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let t = strip(lines[i]);
        let mut commented = false;
        let mut candidate = t;
        if let Some(rest) = t.strip_prefix("//") {
            let inner = lstrip(rest);
            if commented_bind.is_match(inner) {
                commented = true;
                candidate = inner;
            }
        }
        if candidate.is_empty() || (t.starts_with("//") && !commented) {
            i += 1;
            continue;
        }
        let Some(c) = bind.captures(candidate) else {
            i += 1;
            continue;
        };
        let combo = c[1].to_string();
        let options = strip(&title.replace_all(strip(&c[2]), "")).to_string();
        let rest = c[3].to_string();
        let line_number = base_line + i;
        let (mut action_raw, mut action) = (String::new(), String::new());
        if single.is_match(&rest) {
            if let Some(m) = body.captures(&rest) {
                action_raw = strip(&m[1]).to_string();
                action = action_raw.trim_end_matches(';').to_string();
            }
        } else if rest.contains('{') {
            let mut parts = Vec::new();
            i += 1;
            while i < lines.len() {
                let inner = strip(lines[i]);
                if inner == "}" {
                    break;
                }
                if !inner.is_empty() && !inner.starts_with("//") {
                    parts.push(inner);
                }
                i += 1;
            }
            action_raw = parts.join(" ");
            action = parts.iter().map(|p| p.trim_end_matches(';')).collect::<Vec<_>>().join(" ");
        }
        let description = generate_comment(&action);
        let category = categorize(&description, &action);
        all.push((
            category.to_string(),
            obj(vec![
                ("key_combo", s(&combo)),
                ("options", s(&options)),
                ("action", s(&action)),
                ("action_raw", s(&action_raw)),
                ("category", s(category)),
                ("description", s(&description)),
                ("line_number", Json::Int(line_number as i128)),
                ("commented", Json::Bool(commented)),
            ]),
        ));
        i += 1;
    }
    let mut cat_map: Vec<(String, Vec<Json>)> = Vec::new();
    for (idx, (cat, _)) in all.iter().enumerate() {
        let n = Json::Int(idx as i128);
        match cat_map.iter_mut().find(|(c, _)| c == cat) {
            Some(slot) => slot.1.push(n),
            None => cat_map.push((cat.clone(), vec![n])),
        }
    }
    let mut categories: Vec<Json> = CATEGORY_ORDER
        .iter()
        .filter_map(|cat| cat_map.iter().find(|(c, _)| c == cat).map(|(c, v)| obj(vec![("name", s(c)), ("binds", Json::Arr(v.clone()))])))
        .collect();
    categories.extend(cat_map.iter().filter(|(c, _)| !CATEGORY_ORDER.contains(&c.as_str())).map(|(c, v)| obj(vec![("name", s(c)), ("binds", Json::Arr(v.clone()))])));
    emit(&obj(vec![("binds", Json::Arr(all.into_iter().map(|(_, b)| b).collect())), ("categories", Json::Arr(categories)), ("config_file", s(&path_str(&file)))]));
    Ok(0)
}

fn lead(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

pub(super) fn set_bind(args: &[&str]) -> R<i32> {
    if args.len() < 2 {
        return Ok(error("Usage: set-bind KEY_COMBO ACTION [--options OPTIONS]"));
    }
    let combo = args[0];
    let action = normalize_action(args[1]);
    let mut options = "";
    let mut idx = 2;
    while idx < args.len() {
        if args[idx] == "--options" && idx + 1 < args.len() {
            options = args[idx + 1];
            idx += 2;
        } else {
            idx += 1;
        }
    }
    let file = section_file("config.d/70-binds.kdl");
    if let Some(parent) = file.parent() {
        pyrt::makedirs(parent)?;
    }
    let (file, content, (_, a, b, _)) = match binds_file() {
        Ok(v) => v,
        Err(code) => return Ok(code),
    };
    let lines: Vec<&str> = content[a..b].split('\n').collect();
    let entry = |indent: &str| if options.is_empty() { format!("{indent}{combo} {{ {action}; }}") } else { format!("{indent}{combo} {options} {{ {action}; }}") };
    let span = find_in_block(&lines, combo, false).or_else(|| find_in_block(&lines, combo, true));
    let new_lines: Vec<String> = match span {
        Some((start, end)) => {
            let mut v: Vec<String> = lines[..start].iter().map(|l| l.to_string()).collect();
            v.push(entry(lead(lines[start])));
            v.extend(lines[end..].iter().map(|l| l.to_string()));
            v
        }
        None => {
            let mut at = lines.len();
            while at > 0 && strip(lines[at - 1]).is_empty() {
                at -= 1;
            }
            let mut v: Vec<String> = lines[..at].iter().map(|l| l.to_string()).collect();
            v.push(entry("    "));
            v.push(String::new());
            v.extend(lines[at..].iter().map(|l| l.to_string()));
            v
        }
    };
    write_validated(&file, &splice(&content, a, b, &new_lines.join("\n")))
}

pub(super) fn remove_bind(args: &[&str]) -> R<i32> {
    let Some(combo) = args.first() else { return Ok(error("Usage: remove-bind KEY_COMBO")) };
    let (file, content, (_, a, b, _)) = match binds_file() {
        Ok(v) => v,
        Err(code) => return Ok(code),
    };
    let lines: Vec<&str> = content[a..b].split('\n').collect();
    let Some((start, end)) = find_in_block(&lines, combo, false) else { return Ok(error(format!("Active bind not found: {combo}"))) };
    let comment = |line: &str| {
        let l = lead(line);
        format!("{l}// {}", &line[l.len()..])
    };
    let commented: Vec<String> = lines[start..end].iter().map(|l| if strip(l).is_empty() { l.to_string() } else { comment(l) }).collect();
    // A plain description comment right above (not a ═ section header) goes with the bind.
    let mut prev = start as isize - 1;
    while prev >= 0 && strip(lines[prev as usize]).is_empty() {
        prev -= 1;
    }
    let desc = (prev >= 0).then(|| prev as usize).filter(|&p| {
        let t = strip(lines[p]);
        t.starts_with("//") && !t.contains('═')
    });
    let head: Vec<String> = match desc {
        Some(p) => lines[..p].iter().map(|l| l.to_string()).chain([comment(lines[p])]).collect(),
        None => lines[..start].iter().map(|l| l.to_string()).collect(),
    };
    let new_lines: Vec<String> = head.into_iter().chain(commented).chain(lines[end..].iter().map(|l| l.to_string())).collect();
    write_validated(&file, &splice(&content, a, b, &new_lines.join("\n")))
}

// ---------------------------------------------------------------- alt-tab

fn alt_tab_block() -> String {
    format!(r"(?s)\n?{}.*?{}\n?", esc(ALT_TAB_OVERRIDE_START), esc(ALT_TAB_OVERRIDE_END))
}

/// An Alt+Tab in normal binds outside the managed block: (file, line).
fn alt_tab_user_binding() -> R<Option<(String, String)>> {
    let mut files = vec![config_path()];
    let config_d = config_dir().join("config.d");
    if config_d.is_dir() {
        files.extend(kdl_files(&config_d));
    }
    for path in files {
        let content = match pyrt::read_text(&path) {
            Ok(c) => c,
            Err(e) if e.ty.contains("Error") && !e.ty.starts_with("Unicode") => continue,
            Err(e) => return Err(e),
        };
        let content = sub(&alt_tab_block(), "\n", &content, 0)?;
        let Some((_, a, b, _)) = find_block_bounds(&content, "binds", true) else { continue };
        let lines: Vec<&str> = content[a..b].split('\n').collect();
        if let Some((start, _)) = find_in_block(&lines, "Alt+Tab", false) {
            return Ok(Some((path_str(&path), strip(lines[start]).to_string())));
        }
    }
    Ok(None)
}

pub(super) fn get_alt_tab() -> R<i32> {
    let target = section_file("config.d/90-user-extra.kdl");
    let content = read_or_empty(&target)?;
    let managed = content.contains(ALT_TAB_OVERRIDE_START);
    let user = alt_tab_user_binding()?;
    let source = match &user {
        _ if managed => "inir",
        Some((_, line)) => if line.contains("altSwitcher") { "inir" } else { "custom" },
        None => "niri",
    };
    let mut r = obj(vec![("source", s(source)), ("managed", Json::Bool(managed))]);
    if let Some((file, line)) = user {
        put(&mut r, &["userFile"], s(&file));
        put(&mut r, &["userBind"], s(&line));
    }
    emit(&r);
    Ok(0)
}

pub(super) fn set_alt_tab(args: &[&str]) -> R<i32> {
    let [mode @ ("inir" | "niri")] = args[..] else { return Ok(error("Usage: set-alt-tab inir|niri")) };
    let target = section_file("config.d/90-user-extra.kdl");
    let content = read_or_empty(&target)?;
    let mut next = rstrip(&sub(&alt_tab_block(), "\n", &content, 0)?).to_string();
    if mode == "inir" {
        let block = format!(
            "{ALT_TAB_OVERRIDE_START}\n// Alt+Tab opens iNiR's window switcher (Settings). Delete this block for Niri's.\nbinds {{\n    Alt+Tab {{ spawn \"inir\" \"altSwitcher\" \"next\"; }}\n    Alt+Shift+Tab {{ spawn \"inir\" \"altSwitcher\" \"previous\"; }}\n}}\n{ALT_TAB_OVERRIDE_END}"
        );
        next = if next.is_empty() { format!("{block}\n") } else { format!("{next}\n\n{block}\n") };
    } else if !next.is_empty() {
        next.push('\n');
    }
    if next != content {
        if let Some(parent) = target.parent() {
            pyrt::makedirs(parent)?;
        }
    }
    write_managed(&target, &content, &next)
}
