//! What Settings reads: input, layout, animations, window rules, blur, cursor themes, `niri validate`, and the
//! files that differ from iNiR's shipped defaults.

use super::tables::{ANIMATION_TYPES, BLUR_STRENGTH, DEFAULT_NIRI_FILES, animation_default};
use super::*;

const B: fn(bool) -> Json = Json::Bool;
const I: fn(i128) -> Json = Json::Int;
const F: fn(f64) -> Json = Json::Float;

fn found(p: &str, text: &str) -> bool {
    rx(p).is_match(text)
}

fn cap(p: &str, text: &str) -> Option<String> {
    rx(p).captures(text).map(|c| c[1].to_string())
}

pub(super) fn get_input() -> R<i32> {
    let file = section_file("config.d/10-input-and-cursor.kdl");
    let mut r = obj(vec![
        ("keyboard", obj(vec![("layout", s("us")), ("variant", s("")), ("options", s("")), ("track_layout", s("global")), ("repeat_delay", I(250)), ("repeat_rate", I(50)), ("numlock", B(false))])),
        (
            "touchpad",
            obj(vec![
                ("tap", B(true)),
                ("natural_scroll", B(false)),
                ("dwt", B(false)),
                ("dwtp", B(false)),
                ("drag_lock", B(false)),
                ("disabled_on_external_mouse", B(false)),
                ("left_handed", B(false)),
                ("middle_emulation", B(false)),
                ("accel_profile", s("adaptive")),
                ("accel_speed", F(0.0)),
                ("tap_button_map", s("left-right-middle")),
                ("click_method", s("button-areas")),
                ("scroll_method", s("two-finger")),
                ("scroll_button_lock", B(false)),
            ]),
        ),
        ("mouse", obj(vec![("natural_scroll", B(false)), ("left_handed", B(false)), ("middle_emulation", B(false)), ("scroll_button_lock", B(false)), ("accel_profile", s("flat")), ("accel_speed", F(0.0)), ("scroll_method", s("no-scroll"))])),
        ("trackpoint", obj(vec![("natural_scroll", B(false)), ("left_handed", B(false)), ("middle_emulation", B(false)), ("scroll_button_lock", B(false)), ("accel_profile", s("flat")), ("accel_speed", F(0.0)), ("scroll_method", s("on-button-down"))])),
        ("cursor", obj(vec![("theme", s("capitaine-cursors-light")), ("size", I(24)), ("hide_when_typing", B(true))])),
        (
            "general",
            obj(vec![
                ("disable_power_key_handling", B(false)),
                ("warp_mouse_to_focus", B(false)),
                ("warp_mouse_to_focus_mode", s("separate")),
                ("focus_follows_mouse", B(false)),
                ("focus_follows_mouse_max_scroll", I(0)),
                ("workspace_auto_back_and_forth", B(false)),
                ("mod_key", s("Super")),
                ("mod_key_nested", s("Alt")),
            ]),
        ),
    ]);
    if !file.exists() {
        emit(&r);
        return Ok(0);
    }
    let content = strip_kdl_line_comments(&pyrt::read_text(&file)?);
    let input = extract_block(&content, "input", true);
    let cursor = extract_block(&content, "cursor", true);
    if let Some(input) = input.filter(|b| !b.is_empty()) {
        let flag = |block: &str, name: &str| B(found(&format!(r"(?m)^\s*{}\s*$", esc(name)), block));
        if let Some(kb) = extract_block(input, "keyboard", false).filter(|b| !b.is_empty()) {
            if let Some(xkb) = extract_block(kb, "xkb", false).filter(|b| !b.is_empty()) {
                for key in ["layout", "variant", "options"] {
                    if let Some(v) = cap(&format!(r#"(?m)^[ \t]*{key}\s+"([^"]*)""#), xkb) {
                        put(&mut r, &["keyboard", key], s(&v));
                    }
                }
            }
            if let Some(v) = cap(r"repeat-delay\s+([0-9]+)", kb) {
                put(&mut r, &["keyboard", "repeat_delay"], I(int_of(&v)?));
            }
            if let Some(v) = cap(r"repeat-rate\s+([0-9]+)", kb) {
                put(&mut r, &["keyboard", "repeat_rate"], I(int_of(&v)?));
            }
            if let Some(v) = cap(r#"track-layout\s+"([^"]*)""#, kb) {
                put(&mut r, &["keyboard", "track_layout"], s(&v));
            }
            put(&mut r, &["keyboard", "numlock"], flag(kb, "numlock"));
        }
        if let Some(tp) = extract_block(input, "touchpad", false).filter(|b| !b.is_empty()) {
            for (key, name) in [("tap", "tap"), ("natural_scroll", "natural-scroll"), ("dwt", "dwt"), ("dwtp", "dwtp"), ("drag_lock", "drag-lock"), ("disabled_on_external_mouse", "disabled-on-external-mouse"), ("left_handed", "left-handed"), ("middle_emulation", "middle-emulation")] {
                put(&mut r, &["touchpad", key], flag(tp, name));
            }
            if let Some(v) = cap(r#"accel-profile\s+"([^"]*)""#, tp) {
                put(&mut r, &["touchpad", "accel_profile"], s(&v));
            }
            if let Some(v) = cap(r"accel-speed\s+([0-9.-]+)", tp) {
                put(&mut r, &["touchpad", "accel_speed"], F(float_of(&v)?));
            }
            for (key, name) in [("tap_button_map", "tap-button-map"), ("click_method", "click-method"), ("scroll_method", "scroll-method")] {
                if let Some(v) = cap(&format!(r#"{name}\s+"([^"]*)""#), tp) {
                    put(&mut r, &["touchpad", key], s(&v));
                }
            }
            put(&mut r, &["touchpad", "scroll_button_lock"], flag(tp, "scroll-button-lock"));
        }
        for dev in ["mouse", "trackpoint"] {
            let Some(block) = extract_block(input, dev, false).filter(|b| !b.is_empty()) else { continue };
            if let Some(v) = cap(r#"accel-profile\s+"([^"]*)""#, block) {
                put(&mut r, &[dev, "accel_profile"], s(&v));
            }
            put(&mut r, &[dev, "natural_scroll"], flag(block, "natural-scroll"));
            if let Some(v) = cap(r"accel-speed\s+([0-9.-]+)", block) {
                put(&mut r, &[dev, "accel_speed"], F(float_of(&v)?));
            }
            if let Some(v) = cap(r#"scroll-method\s+"([^"]*)""#, block) {
                put(&mut r, &[dev, "scroll_method"], s(&v));
            }
            put(&mut r, &[dev, "left_handed"], flag(block, "left-handed"));
            put(&mut r, &[dev, "middle_emulation"], flag(block, "middle-emulation"));
            put(&mut r, &[dev, "scroll_button_lock"], flag(block, "scroll-button-lock"));
        }
        put(&mut r, &["general", "disable_power_key_handling"], flag(input, "disable-power-key-handling"));
        put(&mut r, &["general", "workspace_auto_back_and_forth"], flag(input, "workspace-auto-back-and-forth"));
        if let Some(v) = cap(r#"(?m)^\s*mod-key\s+"([^"]*)""#, input) {
            put(&mut r, &["general", "mod_key"], s(&v));
        }
        if let Some(v) = cap(r#"(?m)^\s*mod-key-nested\s+"([^"]*)""#, input) {
            put(&mut r, &["general", "mod_key_nested"], s(&v));
        }
        if let Some(c) = rx(r#"(?m)^\s*warp-mouse-to-focus(?:\s+mode="([^"]*)")?\s*$"#).captures(input) {
            put(&mut r, &["general", "warp_mouse_to_focus"], B(true));
            let mode = c.get(1).map(|m| m.as_str()).filter(|m| !m.is_empty()).unwrap_or("separate");
            put(&mut r, &["general", "warp_mouse_to_focus_mode"], s(mode));
        }
        if let Some(c) = rx(r#"(?m)^\s*focus-follows-mouse(?:\s+max-scroll-amount="([0-9]+)%")?\s*$"#).captures(input) {
            put(&mut r, &["general", "focus_follows_mouse"], B(true));
            if let Some(m) = c.get(1).filter(|m| !m.as_str().is_empty()) {
                put(&mut r, &["general", "focus_follows_mouse_max_scroll"], I(int_of(m.as_str())?));
            }
        }
    }
    if let Some(cur) = cursor.filter(|b| !b.is_empty()) {
        if let Some(v) = cap(r#"xcursor-theme\s+"([^"]*)""#, cur) {
            put(&mut r, &["cursor", "theme"], s(&v));
        }
        if let Some(v) = cap(r"xcursor-size\s+([0-9]+)", cur) {
            put(&mut r, &["cursor", "size"], I(int_of(&v)?));
        }
        put(&mut r, &["cursor", "hide_when_typing"], B(found(r"(?m)^\s*hide-when-typing\s*$", cur)));
    }
    emit(&r);
    Ok(0)
}

/// What niri does when a block carries neither flag: the focus ring is on, border and shadow are off.
fn flag_default(section: &str) -> bool {
    section == "focus-ring"
}

pub(super) fn get_layout() -> R<i32> {
    let file = section_file("config.d/20-layout-and-overview.kdl");
    let mut r = obj(vec![
        ("gaps", I(25)),
        ("center_focused", s("never")),
        ("always_center_single_column", B(true)),
        ("empty_workspace_above_first", B(false)),
        ("default_column_display", s("normal")),
        ("border", obj(vec![("enabled", B(false)), ("width", I(4)), ("active_color", s("#707070")), ("inactive_color", s("#d0d0d0")), ("urgent_color", s("#cc4444"))])),
        ("focus_ring", obj(vec![("enabled", B(false)), ("width", I(1)), ("active_color", s("#808080")), ("inactive_color", s("#505050"))])),
        ("shadow", obj(vec![("enabled", B(true)), ("softness", I(30)), ("spread", I(5)), ("offset_x", I(0)), ("offset_y", I(5)), ("color", s("#0007"))])),
        ("struts", obj(vec![("left", I(0)), ("right", I(0)), ("top", I(0)), ("bottom", I(0))])),
        ("overview_zoom", F(0.75)),
    ]);
    if !file.exists() {
        emit(&r);
        return Ok(0);
    }
    let content = strip_kdl_line_comments(&pyrt::read_text(&file)?);
    if let Some(layout) = extract_block(&content, "layout", true).filter(|b| !b.is_empty()) {
        if let Some(v) = cap(r"gaps\s+([0-9.]+)", layout) {
            put(&mut r, &["gaps"], I(trunc(float_of(&v)?)?));
        }
        if let Some(v) = cap(r#"center-focused-column\s+"([^"]*)""#, layout) {
            put(&mut r, &["center_focused"], s(&v));
        }
        put(&mut r, &["always_center_single_column"], B(found(r"(?m)^\s*always-center-single-column(?:\s+true)?\s*$", layout)));
        put(&mut r, &["empty_workspace_above_first"], B(found(r"(?m)^\s*empty-workspace-above-first(?:\s+true)?\s*$", layout)));
        if let Some(v) = cap(r#"default-column-display\s+"([^"]*)""#, layout) {
            put(&mut r, &["default_column_display"], s(&v));
        }
        for section in ["border", "focus-ring", "shadow"] {
            let Some(block) = extract_block(layout, section, false) else { continue };
            let key = section.replace('-', "_");
            put(&mut r, &[&key, "enabled"], B(flag_state(block, flag_default(section))));
            if let Some(v) = cap(r"width\s+([0-9]+)", block) {
                if has_key(&r, &[&key, "width"]) {
                    put(&mut r, &[&key, "width"], I(int_of(&v)?));
                }
            }
            for color in ["active-color", "inactive-color", "urgent-color", "color"] {
                if let Some(v) = cap(&format!(r#"{}\s+"([^"]*)""#, esc(color)), block) {
                    let ck = color.replace('-', "_");
                    if has_key(&r, &[&key, &ck]) {
                        put(&mut r, &[&key, &ck], s(&v));
                    }
                }
            }
            if key == "shadow" {
                for setting in ["softness", "spread"] {
                    if let Some(v) = cap(&format!(r"{setting}\s+([0-9.-]+)"), block) {
                        put(&mut r, &["shadow", setting], I(trunc(float_of(&v)?)?));
                    }
                }
                if let Some(c) = rx(r"offset\s+x=([0-9.-]+)\s+y=([0-9.-]+)").captures(block) {
                    put(&mut r, &["shadow", "offset_x"], I(trunc(float_of(&c[1])?)?));
                    put(&mut r, &["shadow", "offset_y"], I(trunc(float_of(&c[2])?)?));
                }
            }
        }
        if let Some(struts) = extract_block(layout, "struts", false).filter(|b| !b.is_empty()) {
            for edge in ["left", "right", "top", "bottom"] {
                if let Some(v) = cap(&format!(r"{edge}\s+([0-9]+)"), struts) {
                    put(&mut r, &["struts", edge], I(int_of(&v)?));
                }
            }
        }
    }
    if let Some(ov) = extract_block(&content, "overview", true).filter(|b| !b.is_empty()) {
        if let Some(v) = cap(r"zoom\s+([0-9.]+)", ov) {
            put(&mut r, &["overview_zoom"], F(float_of(&v)?));
        }
    }
    emit(&r);
    Ok(0)
}

fn default_types() -> Json {
    Json::Obj(ANIMATION_TYPES.iter().map(|t| (t.to_string(), animation_default(t))).collect())
}

pub(super) fn get_animations() -> R<i32> {
    let file = section_file("config.d/60-animations.kdl");
    let mut r = obj(vec![("enabled", B(true)), ("slowdown", F(1.0)), ("types", Json::Obj(vec![]))]);
    if !file.exists() {
        put(&mut r, &["types"], default_types());
        emit(&r);
        return Ok(0);
    }
    let content = strip_kdl_line_comments(&pyrt::read_text(&file)?);
    match extract_block(&content, "animations", true).filter(|b| !b.is_empty()) {
        Some(anim) => {
            put(&mut r, &["enabled"], B(!has_top_level_flag(anim, "off")));
            if let Some(v) = cap(r"slowdown\s+([0-9.]+)", anim) {
                put(&mut r, &["slowdown"], F(float_of(&v)?));
            }
            let mut types = Vec::new();
            for t in ANIMATION_TYPES {
                let mut settings = animation_default(t);
                if let Some(block) = extract_block(anim, t, false).filter(|b| !b.is_empty()) {
                    if let Some(params) = cap(r"spring\s+(.*)", block) {
                        put(&mut settings, &["mode"], s("spring"));
                        for (param, key) in [("damping-ratio", "damping_ratio"), ("stiffness", "stiffness"), ("epsilon", "epsilon")] {
                            if let Some(v) = cap(&format!(r"{param}=([0-9.]+)"), &params) {
                                put(&mut settings, &[key], F(float_of(&v)?));
                            }
                        }
                    } else {
                        let duration = cap(r"duration-ms\s+([0-9]+)", block);
                        let curve = rx(r#"curve\s+"([^"]*)"(.*)"#).captures(block).map(|c| (c[1].to_string(), strip(&c[2]).to_string()));
                        if duration.is_some() || curve.is_some() {
                            settings = obj(vec![
                                ("mode", s("easing")),
                                ("duration_ms", I(duration.as_deref().map(int_of).transpose()?.unwrap_or(150))),
                                ("curve", s(curve.as_ref().map_or("ease-out-expo", |c| c.0.as_str()))),
                                ("curve_args", s(curve.as_ref().map_or("", |c| c.1.as_str()))),
                            ]);
                        }
                    }
                    if found(r"(?m)^\s*off\s*$", block) {
                        put(&mut settings, &["off"], B(true));
                    }
                }
                types.push((t.to_string(), settings));
            }
            put(&mut r, &["types"], Json::Obj(types));
        }
        None => put(&mut r, &["types"], default_types()),
    }
    emit(&r);
    Ok(0)
}

pub(super) fn get_window_rules() -> R<i32> {
    let file = section_file("config.d/30-window-rules.kdl");
    let mut r = obj(vec![
        ("corner_radius", I(16)),
        ("clip_to_geometry", B(true)),
        ("inactive_opacity", F(0.9)),
        ("active_opacity", F(1.0)),
        ("blur", B(false)),
        ("xray", B(true)),
        ("border_behind", B(false)),
    ]);
    if !file.exists() {
        emit(&r);
        return Ok(0);
    }
    let content = pyrt::read_text(&file)?;
    let mut global_seen = false;
    for (_, _, block) in set::window_rule_blocks(&content) {
        let global = set::is_global_rule(&block);
        if found(r"match\s+is-active\s*=\s*false", &block) {
            if let Some(v) = cap(r"opacity\s+([0-9.]+)", &block) {
                put(&mut r, &["inactive_opacity"], F(float_of(&v)?));
            }
        } else if global && !global_seen {
            // Only the rule every window matches is the global value; app- and game-scoped rules must not read back.
            if let Some(v) = cap(r"geometry-corner-radius\s+([0-9]+)", &block) {
                put(&mut r, &["corner_radius"], I(int_of(&v)?));
                global_seen = true;
            }
            if let Some(v) = cap(r"clip-to-geometry\s+(true|false)", &block) {
                put(&mut r, &["clip_to_geometry"], B(v == "true"));
                global_seen = true;
            }
        }
        if global {
            // Later global rules win in niri, so the last value read is the effective one.
            if let Some(v) = cap(r"(?m)^\s*opacity\s+([0-9.]+)", &block) {
                put(&mut r, &["active_opacity"], F(float_of(&v)?));
            }
            if let Some(effect) = cap(r"background-effect\s*\{([^}]*)\}", &block) {
                if let Some(v) = cap(r"blur\s+(true|false)", &effect) {
                    put(&mut r, &["blur"], B(v == "true"));
                }
                if let Some(v) = cap(r"xray\s+(true|false)", &effect) {
                    put(&mut r, &["xray"], B(v == "true"));
                }
            }
            if let Some(v) = cap(r"draw-border-with-background\s+(true|false)", &block) {
                put(&mut r, &["border_behind"], B(v == "false"));
            }
        }
    }
    emit(&r);
    Ok(0)
}

pub(super) fn list_cursor_themes() -> R<i32> {
    let data = env_or_home("XDG_DATA_HOME", "~/.local/share");
    let mut themes: Vec<String> = Vec::new();
    for dir in [pathlib(data).join("icons"), PathBuf::from("/usr/share/icons"), PathBuf::from(py::expanduser("~/.icons"))] {
        if !dir.is_dir() {
            continue;
        }
        for e in std::fs::read_dir(&dir).map_err(|e| pyrt::os_exc(&e, &dir))?.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() && p.join("cursors").is_dir() && !themes.contains(&name) {
                themes.push(name);
            }
        }
    }
    themes.sort();
    emit(&Json::Arr(themes.iter().map(|t| s(t)).collect()));
    Ok(0)
}

fn env_or_home(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| py::expanduser(default))
}

pub(super) fn validate() -> R<i32> {
    let cfg = config_path();
    let path = s(&path_str(&cfg));
    if !cfg.exists() {
        emit(&obj(vec![("valid", B(false)), ("config_path", path), ("output", s("config.kdl not found"))]));
        return Ok(0);
    }
    match run(&["niri", "validate", "-c", &path_str(&cfg)], Some(5)) {
        Ok(r) => emit(&obj(vec![("valid", B(r.code == 0)), ("config_path", path), ("output", s(strip(&(r.out + &r.err))))])),
        Err(e) => emit(&obj(vec![("valid", B(false)), ("config_path", path), ("output", s(&e))])),
    }
    Ok(0)
}

// ---------------------------------------------------------------- customizations

fn meaningful_lines(text: &str) -> Vec<String> {
    pyrt::splitlines(text).into_iter().filter(|l| !strip(l).is_empty() && !lstrip(l).starts_with("//")).map(|l| rstrip(l).to_string()).collect()
}

/// difflib's `SequenceMatcher(None, a, b).get_opcodes()` (autojunk on), only the changed spans.
fn changed_spans(a: &[String], b: &[String]) -> Vec<(usize, usize, usize, usize)> {
    use std::collections::HashMap;
    let mut b2j: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, x) in b.iter().enumerate() {
        b2j.entry(x).or_default().push(i);
    }
    if b.len() >= 200 {
        let ntest = b.len() / 100 + 1;
        b2j.retain(|_, v| v.len() <= ntest);
    }
    let longest = |alo: usize, ahi: usize, blo: usize, bhi: usize| {
        let (mut bi, mut bj, mut bsize) = (alo, blo, 0);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut next: HashMap<usize, usize> = HashMap::new();
            for &j in b2j.get(a[i].as_str()).map_or(&[][..], Vec::as_slice) {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = j.checked_sub(1).and_then(|p| j2len.get(&p)).copied().unwrap_or(0) + 1;
                next.insert(j, k);
                if k > bsize {
                    (bi, bj, bsize) = (i + 1 - k, j + 1 - k, k);
                }
            }
            j2len = next;
        }
        while bi > alo && bj > blo && a[bi - 1] == b[bj - 1] {
            (bi, bj, bsize) = (bi - 1, bj - 1, bsize + 1);
        }
        while bi + bsize < ahi && bj + bsize < bhi && a[bi + bsize] == b[bj + bsize] {
            bsize += 1;
        }
        (bi, bj, bsize)
    };
    let mut queue = vec![(0, a.len(), 0, b.len())];
    let mut blocks = Vec::new();
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = longest(alo, ahi, blo, bhi);
        if k > 0 {
            blocks.push((i, j, k));
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    blocks.sort_unstable();
    blocks.push((a.len(), b.len(), 0));
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    for (ai, bj, size) in blocks {
        if i < ai || j < bj {
            out.push((i, ai, j, bj));
        }
        (i, j) = (ai + size, bj + size);
    }
    out
}

/// The first `+`/`-` lines of `unified_diff(default, user)` (lines that read as a diff header are skipped).
fn preview_diff(default_text: &str, user_text: &str) -> Vec<String> {
    let (a, b) = (meaningful_lines(default_text), meaningful_lines(user_text));
    let mut out = Vec::new();
    for (i1, i2, j1, j2) in changed_spans(&a, &b) {
        let lines = a[i1..i2].iter().map(|l| format!("-{l}")).chain(b[j1..j2].iter().map(|l| format!("+{l}")));
        for line in lines {
            if line.starts_with("---") || line.starts_with("+++") {
                continue;
            }
            out.push(line);
            if out.len() >= 8 {
                return out;
            }
        }
    }
    out
}

fn preview_lines(text: &str) -> Vec<String> {
    meaningful_lines(text).into_iter().take(8).collect()
}

fn lines_json(v: &[String]) -> Json {
    Json::Arr(v.iter().map(|l| s(l)).collect())
}

pub(super) fn detect_customizations() -> R<i32> {
    let dir = config_dir();
    let defaults = shell_root().join("defaults/niri");
    let mut files = Vec::new();
    let mut summary = [("managed_override", 0i128), ("extra_file", 0), ("expected_generated", 0), ("user_extra", 0)];
    let mut add = |entry: Json, kind: &str, files: &mut Vec<Json>| {
        let key = kind.replace('-', "_");
        if let Some(slot) = summary.iter_mut().find(|(k, _)| *k == key) {
            slot.1 += 1;
        }
        files.push(entry);
    };
    for rel in DEFAULT_NIRI_FILES {
        let (user, default) = (dir.join(rel), defaults.join(rel));
        if !user.exists() || !default.exists() {
            continue;
        }
        let user_text = pyrt::read_text(&user)?;
        let default_text = pyrt::read_text(&default)?;
        let count = I(meaningful_lines(&user_text).len() as i128);
        if *rel == "config.d/90-user-extra.kdl" {
            let preview = preview_lines(&user_text);
            if !preview.is_empty() {
                let e = obj(vec![("path", s(rel)), ("kind", s("user-extra")), ("reason", s("User-owned extension file for personal Niri rules.")), ("preview", lines_json(&preview)), ("line_count", count)]);
                add(e, "user-extra", &mut files);
            }
            continue;
        }
        if meaningful_lines(&user_text) == meaningful_lines(&default_text) {
            continue;
        }
        let mut preview = preview_diff(&default_text, &user_text);
        if preview.is_empty() {
            preview = preview_lines(&user_text);
        }
        let e = obj(vec![("path", s(rel)), ("kind", s("managed-override")), ("reason", s("This managed Niri file differs from the shipped iNiR default.")), ("preview", lines_json(&preview)), ("line_count", count)]);
        add(e, "managed-override", &mut files);
    }
    let config_d = dir.join("config.d");
    if config_d.exists() {
        for extra in kdl_files(&config_d) {
            let rel = format!("config.d/{}", extra.file_name().unwrap_or_default().to_string_lossy());
            if DEFAULT_NIRI_FILES.contains(&rel.as_str()) {
                continue;
            }
            let text = pyrt::read_text(&extra)?;
            let preview = preview_lines(&text);
            if preview.is_empty() {
                continue;
            }
            let outputs = rel == "config.d/15-outputs.kdl";
            let kind = if outputs { "expected-generated" } else { "extra-file" };
            let reason = if outputs { "Generated by Niri output settings and expected in customized setups." } else { "Additional user config file not shipped by iNiR defaults." };
            let e = obj(vec![("path", s(&rel)), ("kind", s(kind)), ("reason", s(reason)), ("preview", lines_json(&preview)), ("line_count", I(meaningful_lines(&text).len() as i128))]);
            add(e, kind, &mut files);
        }
    }
    let actionable = summary[0].1 + summary[1].1;
    let mut sum: Vec<(String, Json)> = summary.iter().map(|(k, v)| (k.to_string(), I(*v))).collect();
    sum.push(("actionable".into(), I(actionable)));
    sum.push(("total".into(), I(files.len() as i128)));
    emit(&obj(vec![("customized", B(actionable > 0)), ("config_dir", s(&path_str(&dir))), ("summary", Json::Obj(sum)), ("files", Json::Arr(files))]));
    Ok(0)
}

// ---------------------------------------------------------------- blur

pub(super) const BLUR_FILE: &str = "config.d/80-layer-rules.kdl";

fn niri_version() -> Option<(i128, i128)> {
    let out = run(&["niri", "--version"], Some(5)).ok()?.out;
    let c = rx(r"([0-9]+)\.([0-9]+)").captures(&out)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?))
}

fn blur_value(block: Option<&str>, key: &str, fallback: f64) -> R<f64> {
    match cap(&format!(r"(?m)^\s*{}\s+([-0-9.]+)", esc(key)), block.unwrap_or_default()) {
        Some(v) => float_of(&v),
        None => Ok(fallback),
    }
}

pub(super) fn get_blur() -> R<i32> {
    let version = niri_version();
    let file = section_file(BLUR_FILE);
    let content = if file.exists() { strip_kdl_line_comments(&pyrt::read_text(&file)?) } else { String::new() };
    let block = extract_block(&content, "blur", true);
    let mut elsewhere = Vec::new();
    let own = resolve(&file);
    for candidate in std::iter::once(config_path()).chain(kdl_files(&config_dir().join("config.d"))) {
        if !candidate.exists() || resolve(&candidate) == own {
            continue;
        }
        if extract_block(&strip_kdl_line_comments(&pyrt::read_text(&candidate)?), "blur", true).is_some() {
            elsewhere.push(s(&candidate.file_name().unwrap_or_default().to_string_lossy()));
        }
    }
    let passes = blur_value(block, "passes", 3.0)?;
    let offset = blur_value(block, "offset", 3.0)?;
    let strength = BLUR_STRENGTH
        .iter()
        .find(|(_, p, o)| p.parse::<f64>().ok() == Some(passes) && o.parse::<f64>().ok() == Some(offset))
        .map_or("custom", |(n, _, _)| n);
    emit(&obj(vec![
        ("available", B(version.is_some_and(|v| v >= (26, 4)))),
        ("version", s(&version.map_or(String::new(), |(a, b)| format!("{a}.{b}")))),
        ("enabled", B(!block.is_some_and(|b| has_top_level_flag(b, "off")))),
        ("strength", s(strength)),
        ("noise", F(blur_value(block, "noise", 0.02)?)),
        ("saturation", F(blur_value(block, "saturation", 1.5)?)),
        ("elsewhere", Json::Arr(elsewhere)),
    ]));
    Ok(0)
}
