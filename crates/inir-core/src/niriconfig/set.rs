//! What Settings writes: `set <section> <key> <value>` (input, layout, animations, window rules, blur, outputs),
//! the cursor everywhere apps read it, the backdrop-only overview shadow, and the animation presets.

use super::read::BLUR_FILE;
use super::tables::{ANIMATION_TYPES, BACKDROP_SHADOW_OVERRIDE_END, BACKDROP_SHADOW_OVERRIDE_START, BLUR_STRENGTH};
use super::*;

pub(super) fn set(args: &[&str]) -> R<i32> {
    let [section, key, value, ..] = args[..] else { return Ok(error("Usage: set <section> <key> <value>")) };
    match section {
        "input" => set_input(key, value),
        "layout" => set_layout(key, value),
        "animations" => set_animations(key, value),
        "window-rules" => set_window_rules(key, value),
        "blur" => set_blur(key, value),
        "output" => {
            let Some((name, prop)) = key.split_once('.') else { return Ok(error("output key must be <name>.<prop>")) };
            if !["mode", "scale", "transform", "vrr", "position"].contains(&prop) {
                return Ok(error(format!("Unknown output prop: {prop}")));
            }
            outputs::persist_output(&[name, &format!("{prop}={value}")])
        }
        other => Ok(error(format!("Unknown section: {other}"))),
    }
}

fn q(v: &str) -> String {
    format!("\"{v}\"")
}

fn set_input(key: &str, value: &str) -> R<i32> {
    let file = section_file("config.d/10-input-and-cursor.kdl");
    if !file.exists() {
        return Ok(error("input config file not found"));
    }
    let mut c = pyrt::read_text(&file)?;
    let on = value == "on";
    let Some((sub_name, prop)) = key.split_once('.') else {
        c = match key {
            "disable-power-key-handling" | "workspace-auto-back-and-forth" => toggle_flag(&c, "input", key, on, true)?,
            "warp-mouse-to-focus" | "focus-follows-mouse" if value == "off" => remove_key_from_section(&c, "input", key, true)?,
            "warp-mouse-to-focus" => {
                let mode = match value {
                    "center-xy" => "mode=\"center-xy\"",
                    "center-xy-always" => "mode=\"center-xy-always\"",
                    _ => "",
                };
                set_value_in_block(&c, "input", key, mode, true)?
            }
            "focus-follows-mouse" => set_value_in_block(&c, "input", key, value, true)?,
            "mod-key" | "mod-key-nested" => set_value_in_block(&c, "input", key, &q(value), true)?,
            _ => return Ok(error(format!("Unknown input key: {key}"))),
        };
        return write_validated(&file, &c);
    };
    let flags = ["drag-lock", "disabled-on-external-mouse", "left-handed", "middle-emulation", "scroll-button-lock"];
    c = match sub_name {
        "keyboard" => match prop {
            "layout" | "variant" | "options" => set_xkb_value(&c, prop, value)?,
            "repeat-delay" | "repeat-rate" => set_value_in_block(&c, "keyboard", prop, value, false)?,
            "track-layout" => set_value_in_block(&c, "keyboard", prop, &q(value), false)?,
            "numlock" => toggle_flag(&c, "keyboard", "numlock", on, false)?,
            _ => return Ok(error(format!("Unknown keyboard prop: {prop}"))),
        },
        "touchpad" => match prop {
            "tap" | "natural-scroll" | "dwt" | "dwtp" => toggle_flag(&c, "touchpad", prop, on, false)?,
            p if flags.contains(&p) => toggle_flag(&c, "touchpad", prop, on, false)?,
            "accel-profile" | "tap-button-map" | "click-method" | "scroll-method" => set_value_in_block(&c, "touchpad", prop, &q(value), false)?,
            "accel-speed" => set_value_in_block(&c, "touchpad", prop, value, false)?,
            _ => return Ok(error(format!("Unknown touchpad prop: {prop}"))),
        },
        "mouse" | "trackpoint" => {
            if sub_name == "trackpoint" {
                c = ensure_subsection(&c, "input", "trackpoint");
            }
            match prop {
                "natural-scroll" | "left-handed" | "middle-emulation" | "scroll-button-lock" => toggle_flag(&c, sub_name, prop, on, false)?,
                "accel-profile" | "scroll-method" => set_value_in_block(&c, sub_name, prop, &q(value), false)?,
                "accel-speed" => set_value_in_block(&c, sub_name, prop, value, false)?,
                _ => return Ok(error(format!("Unknown {sub_name} prop: {prop}"))),
            }
        }
        "cursor" => match prop {
            "xcursor-theme" => {
                let c = set_value_in_block(&c, "cursor", prop, &q(value), true)?;
                sync_cursor_env(Some(value), None)?;
                c
            }
            "xcursor-size" => {
                let c = set_value_in_block(&c, "cursor", prop, value, true)?;
                sync_cursor_env(None, Some(value))?;
                c
            }
            "hide-when-typing" => toggle_flag(&c, "cursor", "hide-when-typing", on, true)?,
            _ => return Ok(error(format!("Unknown cursor prop: {prop}"))),
        },
        _ => return Ok(error(format!("Unknown input subsection: {sub_name}"))),
    };
    write_validated(&file, &c)
}

fn set_layout(key: &str, value: &str) -> R<i32> {
    let file = section_file("config.d/20-layout-and-overview.kdl");
    if !file.exists() {
        return Ok(error("layout config file not found"));
    }
    let c = pyrt::read_text(&file)?;
    let on = value == "on";
    let c = match key {
        "gaps" => set_value_in_block(&c, "layout", "gaps", value, true)?,
        "center-focused-column" | "default-column-display" => set_value_in_block(&c, "layout", key, &q(value), true)?,
        "always-center-single-column" | "empty-workspace-above-first" => toggle_flag(&c, "layout", key, on, true)?,
        "overview.zoom" | "overview-zoom" => set_value_in_block(&c, "overview", "zoom", value, true)?,
        _ => {
            let Some((sub_name, prop)) = key.split_once('.') else { return Ok(error(format!("Unknown layout key: {key}"))) };
            match (sub_name, prop) {
                (_, "enabled") => toggle_subsection_enabled(&c, sub_name, on),
                (_, "width") => set_value_in_block(&c, sub_name, "width", value, false)?,
                (_, "active-color" | "inactive-color" | "urgent-color" | "color") => set_value_in_block(&c, sub_name, prop, &q(value), false)?,
                ("shadow", "softness" | "spread") => set_value_in_block(&c, sub_name, prop, value, false)?,
                ("shadow", "offset") => set_shadow_offset(&c, value)?,
                ("struts", "left" | "right" | "top" | "bottom") => set_value_in_block(&ensure_subsection(&c, "layout", "struts"), "struts", prop, value, false)?,
                _ => return Ok(error(format!("Unknown layout sub-prop: {key}"))),
            }
        }
    };
    write_validated(&file, &c)
}

fn set_animations(key: &str, value: &str) -> R<i32> {
    let file = section_file("config.d/60-animations.kdl");
    if !file.exists() {
        return Ok(error("animations config file not found"));
    }
    let mut c = pyrt::read_text(&file)?;
    if key == "enabled" {
        let Some(anim) = extract_block(&c, "animations", true) else { return Ok(error("animations block not found")) };
        let has_off = has_top_level_flag(&strip_kdl_line_comments(anim), "off");
        if value == "on" && has_off {
            c = remove_key_from_section(&c, "animations", "off", true)?;
        } else if value == "off" && !has_off {
            c = set_value_in_block(&c, "animations", "off", "", true)?;
        }
    } else if key == "slowdown" {
        c = set_value_in_block(&c, "animations", "slowdown", value, true)?;
    } else if let Some((anim_type, param)) = key.split_once('.') {
        if !ANIMATION_TYPES.contains(&anim_type) {
            return Ok(error(format!("Unknown animation type: {anim_type}")));
        }
        if !["enabled", "damping-ratio", "stiffness", "epsilon"].contains(&param) {
            return Ok(error(format!("Unknown spring param: {param}")));
        }
        let Some(anim) = extract_block(&c, "animations", true) else { return Ok(error("animations block not found")) };
        let type_block = extract_block(anim, anim_type, false).filter(|b| !b.is_empty()).map(str::to_string);
        let head = r"(animations\s*\{)\s*\n";
        if param == "enabled" {
            if type_block.is_some() {
                c = toggle_flag(&c, anim_type, "off", value != "on", false)?;
            } else if value != "on" {
                c = sub(head, &format!("\\g<1>\n    {anim_type} {{\n        off\n    }}\n"), &c, 1)?;
            }
            return write_validated(&file, &c);
        }
        match type_block {
            Some(block) => match rx(r"spring\s+(.*)").find(&block).map(|m| m.as_str().to_string()) {
                Some(old) => {
                    let p = format!(r"{}=[0-9.]+", esc(param));
                    let new = if rx(&p).is_match(&old) { sub(&p, &format!("{param}={value}"), &old, 0)? } else { format!("{old} {param}={value}") };
                    c = c.replacen(&old, &new, 1);
                }
                None => c = sub(&format!(r"({}\s*\{{)", esc(anim_type)), &format!("\\g<1>\n        spring {param}={value}"), &c, 1)?,
            },
            None => c = sub(head, &format!("\\g<1>\n    {anim_type} {{\n        spring {param}={value}\n    }}\n"), &c, 1)?,
        }
    } else {
        return Ok(error(format!("Unknown animations key: {key}")));
    }
    write_validated(&file, &c)
}

// ---------------------------------------------------------------- window rules

/// (body start, body end, body with comments blanked) of every real window-rule block, in file order.
pub(super) fn window_rule_blocks(text: &str) -> Vec<(usize, usize, String)> {
    let masked = mask_kdl_comments(text);
    let b = masked.as_bytes();
    let mut out = Vec::new();
    for head in rx(r"window-rule\s*\{").find_iter(&masked) {
        let start = head.end();
        let (mut depth, mut i) = (1, start);
        while i < b.len() && depth > 0 {
            match b[i] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        if depth == 0 {
            out.push((start, i - 1, masked[start..i - 1].to_string()));
        }
    }
    out
}

pub(super) fn is_global_rule(body: &str) -> bool {
    !rx(r"(?m)^\s*(match|exclude)\b").is_match(body)
}

fn global_rules(content: &str) -> Vec<(usize, usize, String)> {
    window_rule_blocks(content).into_iter().filter(|(_, _, b)| is_global_rule(b)).collect()
}

/// `prop` in the first rule that matches every window (made if missing); rules scoped by `match` never are.
fn set_global_rule_property(content: &str, prop: &str, value: &str) -> String {
    for (start, _, body) in window_rule_blocks(content) {
        if !is_global_rule(&body) {
            continue;
        }
        if let Some(c) = rx(&format!(r"(?m)^([ \t]*){}[ \t]+[^\s]+", esc(prop))).captures(&body) {
            let m = c.get(0).unwrap();
            return splice(content, start + m.start(), start + m.end(), &format!("{}{prop} {value}", &c[1]));
        }
        return splice(content, start, start, &format!("\n    {prop} {value}"));
    }
    format!("{}\n\nwindow-rule {{\n    {prop} {value}\n}}\n", rstrip(content))
}

/// One opacity for every window: set in the first global rule, dropped from the later ones (later rules win).
fn set_global_opacity(content: &str, value: f64) -> R<String> {
    let mut content = set_global_rule_property(content, "opacity", &py::float_repr(value));
    let rules = global_rules(&content);
    for (start, end, _) in rules.iter().skip(1).rev() {
        let cleaned = sub(r"(?m)^[ \t]*opacity[ \t]+[^\s]+[ \t]*\n?", "", &content[*start..*end], 0)?;
        content = splice(&content, *start, *end, &cleaned);
    }
    Ok(content)
}

/// `prop` set (or with `None` removed) in the first global rule's background-effect block.
fn set_global_effect(content: &str, prop: &str, value: Option<&str>) -> R<String> {
    let mut content = content.to_string();
    let mut rules = global_rules(&content);
    if rules.is_empty() {
        content = format!("{}\n\nwindow-rule {{\n}}\n", rstrip(&content));
        rules = global_rules(&content);
    }
    let (start, end, body) = &rules[0];
    let Some(block) = rx(r"background-effect\s*\{([^}]*)\}").captures(body) else {
        let Some(v) = value else { return Ok(content) };
        let insert = format!("\n    background-effect {{\n        {prop} {v}\n    }}");
        return Ok(format!("{}{insert}\n{}", rstrip(&content[..*end]), &content[*end..]));
    };
    let mut inner = block[1].to_string();
    let line = rx(&format!(r"(?m)^([ \t]*){}[ \t]+[^\s]+[ \t]*$", esc(prop))).captures(&inner).map(|c| (c.get(0).unwrap().range(), c[1].to_string()));
    inner = match (value, line) {
        (None, _) => sub(&format!(r"(?m)^[ \t]*{}[ \t]+[^\s]+[ \t]*\n?", esc(prop)), "", &inner, 0)?,
        (Some(v), Some((range, indent))) => splice(&inner, range.start, range.end, &format!("{indent}{prop} {v}")),
        (Some(v), None) => format!("{}\n        {prop} {v}\n    ", rstrip(&inner)),
    };
    let whole = block.get(0).unwrap();
    if !rx(r"\S").is_match(&inner) {
        return Ok(format!("{}{}", rstrip(&content[..start + whole.start()]), &content[start + whole.end()..]));
    }
    let g = block.get(1).unwrap();
    Ok(splice(&content, start + g.start(), start + g.end(), &inner))
}

/// Fullscreen games keep direct scanout: iNiR's games rule stays opaque and unblurred.
fn protect_games_rule(content: &str) -> String {
    let mut content = content.to_string();
    for _ in 0..3 {
        let rule = window_rule_blocks(&content).into_iter().find(|(_, _, b)| b.contains("steam_app_") && rx(r"(?m)^\s*match\b").is_match(b));
        let Some((_, end, body)) = rule else { return content };
        if !rx(r"(?m)^\s*opacity\b").is_match(&body) {
            content = format!("{}\n    opacity 1.0\n{}", rstrip(&content[..end]), &content[end..]);
        } else if !body.contains("background-effect") {
            content = format!("{}\n    background-effect {{\n        blur false\n    }}\n{}", rstrip(&content[..end]), &content[end..]);
        } else {
            return content;
        }
    }
    content
}

fn set_window_rules(key: &str, value: &str) -> R<i32> {
    let file = section_file("config.d/30-window-rules.kdl");
    if !file.exists() {
        return Ok(error("window rules config file not found"));
    }
    let mut c = pyrt::read_text(&file)?;
    let truthy = ["on", "true", "1"].contains(&value);
    c = match key {
        "corner-radius" => set_global_rule_property(&c, "geometry-corner-radius", &trunc(float_of(value)?)?.to_string()),
        "inactive-opacity" => {
            if rx(r"(?s)(window-rule\s*\{[^}]*match\s+is-active\s*=\s*false[^}]*)(opacity\s+[0-9.]+)").is_match(&c) {
                sub(r"(match\s+is-active\s*=\s*false\s*\n\s*)(opacity\s+)[0-9.]+", &format!(r"\g<1>\g<2>{value}"), &c, 1)?
            } else {
                format!("{}\n\nwindow-rule {{\n    match is-active=false\n    opacity {value}\n}}\n", rstrip(&c))
            }
        }
        "clip-to-geometry" => set_global_rule_property(&c, "clip-to-geometry", if value == "true" { "true" } else { "false" }),
        "active-opacity" => protect_games_rule(&set_global_opacity(&c, float_of(value)?)?),
        "blur" => {
            let c = set_global_effect(&c, "blur", truthy.then_some("true"))?;
            if truthy { protect_games_rule(&c) } else { c }
        }
        // Niri turns xray on by itself once blur is on; only "off" needs writing.
        "xray" => set_global_effect(&c, "xray", (!truthy).then_some("false"))?,
        "border-behind" if truthy => set_global_rule_property(&c, "draw-border-with-background", "false"),
        "border-behind" => {
            for (start, end, _) in global_rules(&c).into_iter().rev() {
                let cleaned = sub(r"(?m)^[ \t]*draw-border-with-background[ \t]+[^\s]+[ \t]*\n?", "", &c[start..end], 0)?;
                c = splice(&c, start, end, &cleaned);
            }
            c
        }
        _ => return Ok(error(format!("Unknown window-rules key: {key}"))),
    };
    write_validated(&file, &c)
}

// ---------------------------------------------------------------- blur

fn set_blur(key: &str, value: &str) -> R<i32> {
    let file = section_file(BLUR_FILE);
    if !file.exists() {
        return Ok(error("layer rules config file not found"));
    }
    let mut c = pyrt::read_text(&file)?;
    if find_block_bounds(&c, "blur", true).is_none() {
        c = format!("{}\n\n// Niri's blur behind surfaces that ask for it (iRiS glass set to Blur).\nblur {{\n}}\n", c.trim_end_matches('\n'));
    }
    match key {
        "enabled" => {
            let block = extract_block(&c, "blur", true).unwrap_or_default();
            let has_off = has_top_level_flag(&strip_kdl_line_comments(block), "off");
            if value == "on" && has_off {
                c = remove_key_from_section(&c, "blur", "off", true)?;
            } else if value == "off" && !has_off {
                c = set_value_in_block(&c, "blur", "off", "", true)?;
            }
        }
        "strength" => {
            let Some((_, passes, offset)) = BLUR_STRENGTH.iter().find(|(n, _, _)| *n == value) else {
                return Ok(error(format!("unknown blur strength: {value}")));
            };
            c = set_value_in_block(&c, "blur", "passes", passes, true)?;
            c = set_value_in_block(&c, "blur", "offset", offset, true)?;
        }
        "noise" | "saturation" => {
            if py::parse_float(value).is_none() {
                return Ok(error(format!("invalid {key}: {value}")));
            }
            c = set_value_in_block(&c, "blur", key, value, true)?;
        }
        _ => return Ok(error(format!("unknown blur key: {key}"))),
    }
    pyrt::write_text(&file, &c)?;
    emit(&obj(vec![("ok", Json::Bool(true))]));
    Ok(0)
}

// ---------------------------------------------------------------- overview shadow

fn managed_block(start: &str, end: &str) -> String {
    format!(r"(?s)\n?{}.*?{}\n?", esc(start), esc(end))
}

/// `next_content` written unless it is what the file already says.
pub(super) fn write_managed(target: &Path, content: &str, next: &str) -> R<i32> {
    if next == content {
        emit(&obj(vec![("success", Json::Bool(true)), ("file", s(&path_str(target))), ("changed", Json::Bool(false))]));
        return Ok(0);
    }
    write_validated(target, next)
}

pub(super) fn sync_backdrop_overview_shadow(args: &[&str]) -> R<i32> {
    let [mode @ ("on" | "off")] = args[..] else { return Ok(error("Usage: sync-backdrop-overview-shadow on|off")) };
    let target = section_file("config.d/90-user-extra.kdl");
    let content = read_or_empty(&target)?;
    let mut next = rstrip(&sub(&managed_block(BACKDROP_SHADOW_OVERRIDE_START, BACKDROP_SHADOW_OVERRIDE_END), "\n", &content, 0)?).to_string();
    if mode == "on" {
        let block = format!("{BACKDROP_SHADOW_OVERRIDE_START}\noverview {{\n    workspace-shadow {{\n        off\n    }}\n}}\n{BACKDROP_SHADOW_OVERRIDE_END}");
        next = if next.is_empty() { format!("{block}\n") } else { format!("{next}\n\n{block}\n") };
    } else if !next.is_empty() {
        next.push('\n');
    }
    write_managed(&target, &content, &next)
}

// ---------------------------------------------------------------- cursor

fn is_os_error(e: &pyrt::Exc) -> bool {
    e.ty.ends_with("Error") && (e.ty == "OSError" || e.ty.contains("File") || e.ty.contains("Directory") || e.ty == "PermissionError")
}

/// The cursor everywhere apps read it: environment.d, gsettings, the running session, X's default theme.
fn sync_cursor_env(theme: Option<&str>, size: Option<&str>) -> R<()> {
    let env_dir = home().join(".config/environment.d");
    pyrt::makedirs(&env_dir)?;
    let mut target = None;
    for name in ["inir.conf", "cursor.conf"] {
        let p = env_dir.join(name);
        if p.exists() {
            let t = pyrt::read_text(&p)?;
            if t.contains("XCURSOR_THEME=") || t.contains("XCURSOR_SIZE=") {
                target = Some(p);
                break;
            }
        }
    }
    let target = target.unwrap_or_else(|| env_dir.join("cursor.conf"));
    let (mut lines, mut found_theme, mut found_size) = (Vec::new(), false, false);
    if target.exists() {
        for line in pyrt::splitlines(&pyrt::read_text(&target)?) {
            let t = strip(line);
            match (theme, size) {
                (Some(th), _) if t.starts_with("XCURSOR_THEME=") => {
                    lines.push(format!("XCURSOR_THEME={th}"));
                    found_theme = true;
                }
                (_, Some(sz)) if t.starts_with("XCURSOR_SIZE=") => {
                    lines.push(format!("XCURSOR_SIZE={sz}"));
                    found_size = true;
                }
                _ => lines.push(line.to_string()),
            }
        }
    }
    let mut env_vars = Vec::new();
    if let Some(th) = theme {
        if !found_theme {
            lines.push(format!("XCURSOR_THEME={th}"));
        }
        env_vars.push(format!("XCURSOR_THEME={th}"));
    }
    if let Some(sz) = size {
        if !found_size {
            lines.push(format!("XCURSOR_SIZE={sz}"));
        }
        env_vars.push(format!("XCURSOR_SIZE={sz}"));
    }
    pyrt::write_text(&target, &format!("{}\n", lines.join("\n")))?;
    // GTK apps. The Python died here on a system without gsettings, before the KDL was written.
    if pyrt::which("gsettings").is_some() {
        if let Some(th) = theme {
            let _ = run(&["gsettings", "set", "org.gnome.desktop.interface", "cursor-theme", th], None);
        }
        if let Some(sz) = size {
            let _ = run(&["gsettings", "set", "org.gnome.desktop.interface", "cursor-size", sz], None);
        }
    }
    // Newly launched D-Bus apps see the activation environment.
    if !env_vars.is_empty() {
        use std::os::unix::fs::FileTypeExt;
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
        let socket = PathBuf::from(runtime).join("systemd/private");
        let is_socket = std::fs::metadata(&socket).is_ok_and(|m| m.file_type().is_socket());
        let mut probe = vec!["systemctl", "--user", "show-environment"];
        if pyrt::which("timeout").is_some() {
            probe.splice(0..0, ["timeout", "3s"]);
        }
        let usable = is_socket && pyrt::which("systemctl").is_some() && pyrt::which("timeout").is_some() && run(&probe, None).is_ok_and(|r| r.code == 0);
        let vars: Vec<&str> = env_vars.iter().map(String::as_str).collect();
        if usable {
            let _ = run(&[&["systemctl", "--user", "set-environment"][..], &vars].concat(), None);
        } else if pyrt::which("dbus-update-activation-environment").is_some() {
            let _ = run(&[&["dbus-update-activation-environment"][..], &vars].concat(), None);
        }
    }
    // XWayland apps asking for X's core cursor never read XCURSOR_THEME: libXcursor resolves ~/.icons/default.
    if let Some(th) = theme {
        ensure_default_cursor_inherits(th)?;
    }
    Ok(())
}

fn ensure_default_cursor_inherits(theme: &str) -> R<()> {
    if theme.is_empty() || theme == "default" {
        return Ok(());
    }
    let dir = home().join(".local/share/icons/default");
    pyrt::makedirs(&dir)?;
    let index = dir.join("index.theme");
    let desired = format!("[Icon Theme]\nName=Default\nComment=Default cursor theme\nInherits={theme}\n");
    if index.exists() {
        match pyrt::read_text(&index) {
            Ok(t) if t == desired => return Ok(()),
            Ok(_) => {}
            Err(e) if is_os_error(&e) => return Ok(()),
            Err(e) => return Err(e),
        }
    }
    let _ = pyrt::write_text(&index, &desired);
    Ok(())
}

/// The KDL's cursor theme and size materialized everywhere; setup and update call it on every run.
pub(super) fn sync_cursor() -> R<i32> {
    let file = section_file("config.d/10-input-and-cursor.kdl");
    let (mut theme, mut size) = (None, None);
    match pyrt::read_text(&file) {
        Ok(content) => {
            if let Some(cursor) = extract_block(&content, "cursor", true).filter(|b| !b.is_empty()) {
                theme = rx(r#"xcursor-theme\s+"([^"]*)""#).captures(cursor).map(|c| c[1].to_string());
                if let Some(c) = rx(r"xcursor-size\s+([0-9]+)").captures(cursor) {
                    size = Some(int_of(&c[1])?);
                }
            }
        }
        Err(e) if is_os_error(&e) => {}
        Err(e) => return Err(e),
    }
    if theme.is_none() && size.is_none() {
        return Ok(error("No cursor theme/size found in niri config"));
    }
    let size_s = size.map(|v| v.to_string());
    sync_cursor_env(theme.as_deref(), size_s.as_deref())?;
    emit(&obj(vec![("synced", Json::Bool(true)), ("theme", theme.map_or(Json::Null, |t| s(&t))), ("size", size.map_or(Json::Null, Json::Int))]));
    Ok(0)
}

// ---------------------------------------------------------------- animation presets

/// Python's `==` on JSON values (1 == 1.0 == True; dicts in any order).
fn py_eq(a: &Json, b: &Json) -> bool {
    let num = |v: &Json| match v {
        Json::Int(i) => Some(*i as f64),
        Json::Float(f) => Some(*f),
        Json::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    };
    match (a, b) {
        (Json::Null, Json::Null) => true,
        (Json::Str(x), Json::Str(y)) => x == y,
        (Json::Arr(x), Json::Arr(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| py_eq(p, q)),
        (Json::Obj(x), Json::Obj(y)) => x.len() == y.len() && x.iter().all(|(k, v)| b.get(k).is_some_and(|w| py_eq(v, w))),
        _ => matches!((num(a), num(b)), (Some(x), Some(y)) if x == y),
    }
}

fn not_dict(v: &Json, attr: &str) -> pyrt::Exc {
    exc("AttributeError", format!("'{}' object has no attribute '{attr}'", pyrt::type_name(v)))
}

fn dict_get(v: &Json, key: &str) -> R<Option<Json>> {
    match v {
        Json::Obj(_) => Ok(v.get(key).cloned()),
        other => Err(not_dict(other, "get")),
    }
}

fn load_json(p: &Path) -> Option<Json> {
    pyrt::read_text(p).ok().and_then(|t| py::json_loads(&t))
}

fn user_presets_file() -> PathBuf {
    let xdg = xdg_config();
    let legacy = xdg.join("illogical-impulse");
    (if legacy.exists() { legacy } else { xdg.join("inir") }).join("niri-animation-presets.json")
}

/// Shipped presets from the running iNiR; the person's own file adds or overrides by id.
fn load_presets() -> R<Json> {
    let mut registry = load_json(&shell_root().join("defaults/niri-animation-presets.json"))
        .unwrap_or_else(|| obj(vec![("default", s("")), ("presets", Json::Arr(vec![]))]));
    let user = load_json(&user_presets_file()).unwrap_or(Json::Obj(vec![]));
    let mut presets = match dict_get(&registry, "presets")? {
        None => Vec::new(),
        Some(Json::Arr(a)) => a,
        Some(other) => return Err(exc("TypeError", format!("'{}' object is not iterable", pyrt::type_name(&other)))),
    };
    let theirs = match &user {
        Json::Obj(_) => match user.get("presets") {
            None | Some(Json::Obj(_) | Json::Str(_)) => Vec::new(),
            Some(Json::Arr(a)) => a.clone(),
            Some(other) => return Err(exc("TypeError", format!("'{}' object is not iterable", pyrt::type_name(other)))),
        },
        _ => Vec::new(),
    };
    for preset in theirs {
        let ok = matches!(&preset, Json::Obj(_)) && preset.get("id").is_some_and(Json::truthy) && matches!(preset.get("types"), Some(Json::Obj(_)));
        if !ok {
            continue;
        }
        let mut preset = preset;
        put(&mut preset, &["user"], Json::Bool(true));
        let id = preset.get("id").cloned().unwrap_or(Json::Null);
        let mut index = None;
        for (i, p) in presets.iter().enumerate() {
            if dict_get(p, "id")?.is_some_and(|v| py_eq(&v, &id)) {
                index = Some(i);
                break;
            }
        }
        match index {
            Some(i) => presets[i] = preset,
            None => presets.push(preset),
        }
    }
    put(&mut registry, &["presets"], Json::Arr(presets));
    Ok(registry)
}

fn kdl_number(v: &Json) -> R<String> {
    let f = json_float(v)?;
    trunc(f)?;
    Ok(if f.fract() != 0.0 { format!("{f:.4}").trim_end_matches('0').trim_end_matches('.').to_string() } else { format!("{f:.1}") })
}

fn items(v: &Json) -> R<&[(String, Json)]> {
    match v {
        Json::Obj(o) => Ok(o),
        other => Err(not_dict(other, "items")),
    }
}

fn key<'a>(v: &'a Json, k: &str) -> R<&'a Json> {
    v.get(k).ok_or_else(|| exc("KeyError", pyrt::repr_str(k)))
}

fn animation_type_lines(spec: &Json) -> R<Vec<String>> {
    items(spec)?;
    if let Some(spring) = spec.get("spring") {
        let Json::Arr(parts) = spring else { return Err(exc("TypeError", "cannot unpack non-iterable value")) };
        let [d, st, e] = &parts[..] else { return Err(exc("ValueError", format!("expected 3 values to unpack, got {}", parts.len()))) };
        return Ok(vec![format!("spring damping-ratio={} stiffness={} epsilon={}", kdl_number(d)?, json_int(st)?, kdl_number(e)?)]);
    }
    let mut lines = vec![format!("duration-ms {}", json_int(key(spec, "duration-ms")?)?)];
    let mut curve = format!("curve \"{}\"", pyrt::str_of(key(spec, "curve")?));
    if let Some(args) = spec.get("curve-args").filter(|a| a.truthy()) {
        let Json::Arr(args) = args else { return Err(exc("TypeError", "curve-args is not a list")) };
        curve += &format!(" {}", args.iter().map(kdl_number).collect::<R<Vec<_>>>()?.join(" "));
    }
    lines.push(curve);
    if let Some(shader) = spec.get("custom-shader").filter(|v| v.truthy()) {
        let shader = pyrt::as_str(shader, "splitlines")?;
        let body: Vec<String> = pyrt::splitlines(shader).iter().map(|l| if l.is_empty() { String::new() } else { format!("            {l}") }).collect();
        lines.push(format!("custom-shader r\"\n{}\n        \"", body.join("\n")));
    }
    Ok(lines)
}

fn normalize_animation_block(text: &str) -> String {
    let numbers = rx(r"-?[0-9]+(?:\.[0-9]+)?").replace_all(text, |c: &regex::Captures| py::float_repr(c[0].parse().unwrap_or(f64::INFINITY)));
    strip(&rx(r"\s+").replace_all(&numbers, " ")).to_string()
}

fn active_preset(anim: &str, presets: &[Json]) -> R<Json> {
    for preset in presets {
        let types = dict_get(preset, "types")?.unwrap_or(Json::Obj(vec![]));
        let mut matches = true;
        for (t, spec) in items(&types)? {
            let expected = animation_type_lines(spec)?.join("\n");
            match extract_block(anim, t, false) {
                Some(current) if normalize_animation_block(current) == normalize_animation_block(&expected) => {}
                _ => {
                    matches = false;
                    break;
                }
            }
        }
        if matches {
            return key(preset, "id").cloned();
        }
    }
    Ok(s(""))
}

pub(super) fn get_animation_presets() -> R<i32> {
    let registry = load_presets()?;
    let presets = match registry.get("presets") {
        Some(Json::Arr(a)) => a.clone(),
        _ => Vec::new(),
    };
    let file = section_file("config.d/60-animations.kdl");
    let mut active = s("");
    if file.exists() {
        let content = pyrt::read_text(&file)?;
        if let Some(anim) = extract_block(&content, "animations", true) {
            active = active_preset(anim, &presets)?;
        }
    }
    emit(&obj(vec![("default", registry.get("default").cloned().unwrap_or_else(|| s(""))), ("active", active), ("presets", Json::Arr(presets))]));
    Ok(0)
}

pub(super) fn apply_animation_preset(args: &[&str]) -> R<i32> {
    let fail = |msg: String| {
        emit(&obj(vec![("success", Json::Bool(false)), ("error", Json::Str(msg))]));
        Ok(1)
    };
    let Some(id) = args.first() else { return fail("Usage: apply-animation-preset ID".into()) };
    let registry = load_presets()?;
    let mut preset = None;
    for p in registry.get("presets").and_then(|v| if let Json::Arr(a) = v { Some(a.as_slice()) } else { None }).unwrap_or_default() {
        if matches!(dict_get(p, "id")?, Some(Json::Str(v)) if v == *id) {
            preset = Some(p.clone());
            break;
        }
    }
    let Some(preset) = preset else { return fail(format!("Unknown animation preset: {id}")) };
    let file = section_file("config.d/60-animations.kdl");
    let content = read_or_empty(&file)?;
    let bounds = find_block_bounds(&content, "animations", true);
    let mut kept = Vec::new();
    if let Some((_, a, b, _)) = bounds {
        let mut depth: i64 = 0;
        for line in pyrt::splitlines(&content[a..b]) {
            let t = strip(line);
            if depth == 0 && (t == "off" || t.starts_with("slowdown")) {
                kept.push(format!("    {t}"));
            }
            depth += line.matches('{').count() as i64 - line.matches('}').count() as i64;
        }
    }
    // GameMode toggles animations by commenting and uncommenting this line.
    if !kept.iter().any(|l| strip(l) == "off") {
        kept.insert(0, "    // off".into());
    }
    let mut body = vec!["animations {".to_string()];
    body.extend(kept);
    for (t, spec) in items(key(&preset, "types")?)? {
        if body.len() > 1 {
            body.push(String::new());
        }
        body.push(format!("    {t} {{"));
        body.extend(animation_type_lines(spec)?.into_iter().map(|l| format!("        {l}")));
        body.push("    }".into());
    }
    body.push("}".into());
    let block = body.join("\n");
    let next = match bounds {
        Some((start, _, _, end)) => {
            let start = start + usize::from(content.as_bytes()[start] == b'\n');
            format!("{}{block}{}", &content[..start], &content[end..])
        }
        None => format!("{}{}{block}\n", rstrip(&content), if strip(&content).is_empty() { "" } else { "\n\n" }),
    };
    write_validated(&file, &next)
}
