//! Outputs: niri's live state with the configured VRR mode, live changes through `niri msg`, persisted blocks in
//! `config.d/15-outputs.kdl`, and the effective overview hot corners.

use super::tables::VRR_WINDOW_RULE;
use super::*;

/// Output name -> configured VRR mode: niri's IPC only says whether VRR is active right now.
fn read_vrr_modes() -> Vec<(String, &'static str)> {
    let file = section_file("config.d/15-outputs.kdl");
    let Ok(content) = pyrt::read_text(&file) else { return Vec::new() };
    let flat = strip_kdl_line_comments(&content);
    let vrr = rx(r"(?m)^\s*variable-refresh-rate([^\n]*)");
    let mut modes: Vec<(String, &'static str)> = Vec::new();
    for (name, block) in iter_output_blocks(&flat) {
        let mode = match vrr.captures(block) {
            None => "off",
            Some(c) if c[1].contains("on-demand=true") => "on-demand",
            Some(_) => "on",
        };
        match modes.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = mode,
            None => modes.push((name, mode)),
        }
    }
    modes
}

fn get<'a>(o: &'a Json, key: &str) -> R<&'a Json> {
    o.get(key).ok_or_else(|| exc("KeyError", pyrt::repr_str(key)))
}

fn get_or(o: &Json, key: &str, default: Json) -> R<Json> {
    match o {
        Json::Obj(_) => Ok(o.get(key).cloned().unwrap_or(default)),
        other => Err(exc("AttributeError", format!("'{}' object has no attribute 'get'", pyrt::type_name(other)))),
    }
}

/// `round(refresh_rate / 1000, 3)`.
fn rate_of(m: &Json) -> R<f64> {
    let r = json_float(get(m, "refresh_rate")?)? / 1000.0;
    Ok(format!("{r:.3}").parse().unwrap_or(r))
}

pub(super) fn outputs() -> R<i32> {
    let (raw, rc) = run_niri(&["-j", "outputs"]);
    if rc != 0 {
        return Ok(error(format!("niri msg failed: {raw}")));
    }
    let data = py::json_loads(&raw).ok_or_else(|| exc("json.decoder.JSONDecodeError", "Expecting value"))?;
    let Json::Obj(items) = &data else { return Err(exc("AttributeError", "'list' object has no attribute 'items'")) };
    let vrr_modes = read_vrr_modes();
    let mut result = Vec::new();
    for (name, out) in items {
        let modes = get_or(out, "modes", Json::Arr(vec![]))?;
        let modes = match &modes {
            Json::Arr(a) => a.clone(),
            _ => Vec::new(),
        };
        let current_idx = get_or(out, "current_mode", Json::Null)?;
        let logical = get_or(out, "logical", Json::Null)?;
        let logical = if logical.truthy() { logical } else { Json::Obj(vec![]) };

        let mut res_map: Vec<(String, Json)> = Vec::new();
        for (i, m) in modes.iter().enumerate() {
            let key = format!("{}x{}", pyrt::str_of(get(m, "width")?), pyrt::str_of(get(m, "height")?));
            let rate = rate_of(m)?;
            let preferred = get_or(m, "is_preferred", Json::Bool(false))?;
            if !res_map.iter().any(|(k, _)| *k == key) {
                res_map.push((
                    key.clone(),
                    obj(vec![
                        ("width", get(m, "width")?.clone()),
                        ("height", get(m, "height")?.clone()),
                        ("rates", Json::Arr(vec![])),
                        ("preferred", preferred.clone()),
                    ]),
                ));
            }
            let entry = &mut res_map.iter_mut().find(|(k, _)| *k == key).unwrap().1;
            let known = matches!(entry.get("rates"), Some(Json::Arr(r)) if r.iter().any(|x| matches!(x.get("rate"), Some(Json::Float(f)) if *f == rate)));
            if !known {
                let new = obj(vec![
                    ("rate", Json::Float(rate)),
                    ("rate_string", s(&format!("{rate:.3}"))),
                    ("mode_index", Json::Int(i as i128)),
                    ("preferred", preferred),
                ]);
                if let Json::Obj(pairs) = entry {
                    if let Some((_, Json::Arr(r))) = pairs.iter_mut().find(|(k, _)| k == "rates") {
                        r.push(new);
                    }
                }
            } else if preferred.truthy() {
                put(entry, &["preferred"], Json::Bool(true));
            }
        }

        let current_mode = match current_idx {
            Json::Null => None,
            Json::Int(i) if i < modes.len() as i128 => {
                let at = if i < 0 { modes.len() as i128 + i } else { i };
                Some(modes.get(usize::try_from(at).map_err(|_| exc("IndexError", "list index out of range"))?).cloned().ok_or_else(|| exc("IndexError", "list index out of range"))?)
            }
            Json::Int(_) => None,
            _ => return Err(exc("TypeError", "list indices must be integers or slices")),
        };
        let (mut cur_res, mut cur_rate, mut cur_rate_s) = (String::new(), 0.0, String::new());
        if let Some(cm) = current_mode.filter(Json::truthy) {
            cur_res = format!("{}x{}", pyrt::str_of(get(&cm, "width")?), pyrt::str_of(get(&cm, "height")?));
            cur_rate = rate_of(&cm)?;
            cur_rate_s = format!("{cur_rate:.3}");
        }
        let vrr_enabled = get_or(out, "vrr_enabled", Json::Bool(false))?;
        let vrr_mode = vrr_modes.iter().find(|(n, _)| n == name).map_or(if vrr_enabled.truthy() { "on" } else { "off" }, |(_, m)| *m);
        result.push(obj(vec![
            ("name", s(name)),
            ("make", get_or(out, "make", s(""))?),
            ("model", get_or(out, "model", s(""))?),
            ("serial", get_or(out, "serial", s(""))?),
            ("physical_size", get_or(out, "physical_size", Json::Arr(vec![Json::Int(0), Json::Int(0)]))?),
            ("current_resolution", s(&cur_res)),
            ("current_rate", Json::Float(cur_rate)),
            ("current_rate_string", s(&cur_rate_s)),
            ("scale", get_or(&logical, "scale", Json::Float(1.0))?),
            ("transform", get_or(&logical, "transform", s("Normal"))?),
            ("position", obj(vec![("x", get_or(&logical, "x", Json::Int(0))?), ("y", get_or(&logical, "y", Json::Int(0))?)])),
            ("vrr_supported", get_or(out, "vrr_supported", Json::Bool(false))?),
            ("vrr_enabled", vrr_enabled),
            ("vrr_mode", s(vrr_mode)),
            ("resolutions", Json::Arr(res_map.into_iter().map(|(_, v)| v).collect())),
        ]));
    }
    emit(&Json::Arr(result));
    Ok(0)
}

pub(super) fn apply_output(args: &[&str]) -> R<i32> {
    if args.len() < 2 {
        return Ok(error("Usage: apply-output <name> <key=value>..."));
    }
    let name = args[0];
    let mut results = Vec::new();
    for change in &args[1..] {
        let (key, value) = change.split_once('=').unwrap_or((change, ""));
        if value.is_empty() {
            results.push(obj(vec![("key", s(key)), ("error", s("missing value"))]));
            continue;
        }
        let (out, rc) = match key {
            "mode" | "scale" | "transform" => run_niri(&["output", name, key, value]),
            // `vrr [--on-demand] <on|off>`: on-demand is a flag on top of "on".
            "vrr" if value == "on-demand" => run_niri(&["output", name, "vrr", "--on-demand", "on"]),
            "vrr" => run_niri(&["output", name, "vrr", value]),
            "position" => match value.split(',').collect::<Vec<_>>()[..] {
                [x, y] => run_niri(&["output", name, "position", "set", x, y]),
                _ => run_niri(&["output", name, "position", "auto"]),
            },
            "dpms" => run_niri(&["output", name, value]),
            _ => {
                results.push(obj(vec![("key", s(key)), ("error", s("unknown key"))]));
                continue;
            }
        };
        results.push(obj(vec![("key", s(key)), ("value", s(value)), ("success", Json::Bool(rc == 0)), ("output", s(&out))]));
    }
    let failed = results.iter().any(|r| r.get("success").is_some_and(|v| !v.truthy()));
    emit(&obj(vec![("results", Json::Arr(results))]));
    Ok(i32::from(failed))
}

/// The default on-demand VRR window rule, when no rule opts a window in yet; put back if niri refuses it.
fn ensure_vrr_window_rule() -> R<()> {
    let file = section_file("config.d/30-window-rules.kdl");
    let existed = pyrt::exists(&file);
    let existing = read_or_empty(&file)?;
    if rx(r"(?m)^\s*variable-refresh-rate\b").is_match(&existing) {
        return Ok(());
    }
    if let Some(parent) = file.parent() {
        pyrt::makedirs(parent)?;
    }
    pyrt::write_text(&file, &format!("{}\n{VRR_WINDOW_RULE}", rstrip(&existing)))?;
    if !validate_config().0 {
        if existed {
            pyrt::write_text(&file, &existing)?;
        } else {
            let _ = std::fs::remove_file(&file);
        }
    }
    Ok(())
}

fn append_block(result: &str, block: &str) -> String {
    if strip(result).is_empty() { format!("{block}\n") } else { format!("{}\n\n{block}\n", rstrip(result)) }
}

pub(super) fn persist_output(args: &[&str]) -> R<i32> {
    if args.len() < 2 {
        return Ok(error("Usage: persist-output <name> <key=value>..."));
    }
    let name = args[0];
    let invalid: Vec<&str> = args[1..].iter().copied().filter(|a| !a.contains('=') || a.starts_with('=')).collect();
    if !invalid.is_empty() {
        return Ok(error(format!("Invalid output change arguments: {}", invalid.join(", "))));
    }
    let mut changes: Vec<(&str, &str)> = Vec::new();
    for a in &args[1..] {
        let (k, v) = a.split_once('=').unwrap();
        match changes.iter_mut().find(|(kk, _)| *kk == k) {
            Some(slot) => slot.1 = v,
            None => changes.push((k, v)),
        }
    }
    let allowed = ["mode", "scale", "transform", "vrr", "position"];
    let mut unknown: Vec<&str> = changes.iter().map(|(k, _)| *k).filter(|k| !allowed.contains(k)).collect();
    unknown.sort_unstable();
    if !unknown.is_empty() {
        return Ok(error(format!("Unknown output key(s): {}", unknown.join(", "))));
    }
    let change = |k: &str| changes.iter().find(|(kk, _)| *kk == k).map(|(_, v)| *v);
    // On-demand VRR does nothing unless some window rule opts a window into it.
    if change("vrr") == Some("on-demand") {
        ensure_vrr_window_rule()?;
    }
    let file = section_file("config.d/15-outputs.kdl");
    if let Some(parent) = file.parent() {
        pyrt::makedirs(parent)?;
    }
    let existing = read_or_empty(&file)?;
    let result = if let Some((_, a, b, _)) = find_output_block_bounds(&existing, name) {
        let mut block = existing[a..b].to_string();
        for (key, value) in &changes {
            block = match *key {
                "mode" => set_in_block(&block, "mode", &format!("\"{value}\""))?,
                "scale" => set_in_block(&block, "scale", value)?,
                "transform" => set_in_block(&block, "transform", &format!("\"{value}\""))?,
                "vrr" if *value == "off" => sub(r"\n?\s*variable-refresh-rate[^\n]*", "", &block, 0)?,
                "vrr" if *value == "on-demand" => set_in_block(&block, "variable-refresh-rate", "on-demand=true")?,
                "vrr" => set_in_block(&block, "variable-refresh-rate", "")?,
                _ => match value.split(',').collect::<Vec<_>>()[..] {
                    [x, y] => set_in_block(&block, "position", &format!("x={x} y={y}"))?,
                    _ => block,
                },
            };
        }
        splice(&existing, a, b, &block)
    } else {
        let mut lines = Vec::new();
        if let Some(v) = change("mode") {
            lines.push(format!("    mode \"{v}\""));
        }
        if let Some(v) = change("scale") {
            lines.push(format!("    scale {v}"));
        }
        if let Some(v) = change("transform") {
            lines.push(format!("    transform \"{v}\""));
        }
        if let Some(v) = change("position") {
            if let [x, y] = v.split(',').collect::<Vec<_>>()[..] {
                lines.push(format!("    position x={x} y={y}"));
            }
        }
        match change("vrr") {
            Some("on-demand") => lines.push("    variable-refresh-rate on-demand=true".into()),
            Some("off") | None => {}
            Some(_) => lines.push("    variable-refresh-rate".into()),
        }
        append_block(&existing, &format!("output \"{name}\" {{\n{}\n}}", lines.join("\n")))
    };
    write_validated(&file, &result)
}

/// Every connected output's position in one validated write: niri re-places outputs automatically whenever the
/// output config changes, so a durable layout makes every position explicit together.
pub(super) fn persist_layout(args: &[&str]) -> R<i32> {
    if args.len() != 1 {
        return Ok(error("Usage: persist-layout <json-object>"));
    }
    let Some(layout) = py::json_loads(args[0]) else { return Ok(error("Invalid layout JSON: Expecting value")) };
    let pairs = match &layout {
        Json::Obj(p) if !p.is_empty() => p,
        _ => return Ok(error("Layout must be a non-empty object.")),
    };
    let mut normalized: Vec<(&str, i128, i128)> = Vec::new();
    for (name, pos) in pairs {
        if name.is_empty() {
            return Ok(error("Every output must have a non-empty name."));
        }
        let (Some(x), Some(y)) = (pos.get("x"), pos.get("y")) else {
            return Ok(error(format!("Missing x/y position for {name}.")));
        };
        // `int()` raising TypeError or ValueError is the message; infinity's OverflowError is not caught.
        let coord = |v: &Json| match v {
            Json::Float(f) if f.is_infinite() => Some(trunc(*f)),
            Json::Null | Json::Arr(_) | Json::Obj(_) => None,
            other => json_int(other).ok().map(Ok),
        };
        let Some(x) = coord(x) else { return Ok(error(format!("Invalid x/y position for {name}."))) };
        let x = x?;
        let Some(y) = coord(y) else { return Ok(error(format!("Invalid x/y position for {name}."))) };
        normalized.push((name, x, y?));
    }
    let file = section_file("config.d/15-outputs.kdl");
    if let Some(parent) = file.parent() {
        pyrt::makedirs(parent)?;
    }
    let mut result = read_or_empty(&file)?;
    for (name, x, y) in normalized {
        result = match find_output_block_bounds(&result, name) {
            Some((_, a, b, _)) => splice(&result, a, b, &set_in_block(&result[a..b], "position", &format!("x={x} y={y}"))?),
            None => append_block(&result, &format!("output \"{name}\" {{\n    position x={x} y={y}\n}}")),
        };
    }
    write_validated(&file, &result)
}

// ---------------------------------------------------------------- hot corners

fn expandvars(p: &str) -> String {
    rx(r"\$([A-Za-z0-9_]+|\{[^}]*\})")
        .replace_all(p, |c: &regex::Captures| {
            let name = c[1].trim_start_matches('{').trim_end_matches('}');
            std::env::var(name).unwrap_or_else(|_| c[0].to_string())
        })
        .into_owned()
}

/// `fnmatch` of one path part.
fn part_rx(part: &str) -> Regex {
    let mut out = String::from("^(?s:");
    let chars: Vec<char> = part.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '[' => match chars[i + 1..].iter().position(|&c| c == ']').map(|j| i + 1 + j) {
                Some(end) if end > i + 1 => {
                    let mut set: String = chars[i + 1..end].iter().collect();
                    if let Some(rest) = set.strip_prefix('!') {
                        set = format!("^{rest}");
                    }
                    out.push('[');
                    out.push_str(&set.replace('\\', r"\\").replace('[', r"\["));
                    out.push(']');
                    i = end;
                }
                _ => out.push_str(r"\["),
            },
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    out.push_str(")$");
    Regex::new(&out).unwrap_or_else(|_| Regex::new("^$").unwrap())
}

/// `glob.glob(pattern)` (unsorted, hidden names only for a part that starts with a dot).
fn glob(pattern: &str) -> Vec<String> {
    let magic = |t: &str| t.contains(['*', '?', '[']);
    if !magic(pattern) {
        return if std::fs::symlink_metadata(pattern).is_ok() { vec![pattern.to_string()] } else { Vec::new() };
    }
    let (root, rest) = if let Some(r) = pattern.strip_prefix('/') { ("/".to_string(), r) } else { (String::new(), pattern) };
    let mut found = vec![root];
    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    for (n, part) in parts.iter().enumerate() {
        let last = n + 1 == parts.len();
        let mut next = Vec::new();
        for base in &found {
            let join = |name: &str| if base.is_empty() { name.to_string() } else if base.ends_with('/') { format!("{base}{name}") } else { format!("{base}/{name}") };
            if !magic(part) {
                let p = join(part);
                if (last && std::fs::symlink_metadata(&p).is_ok()) || (!last && Path::new(&p).is_dir()) {
                    next.push(p);
                }
                continue;
            }
            let dir = if base.is_empty() { "." } else { base.as_str() };
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            let m = part_rx(part);
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') && !part.starts_with('.') {
                    continue;
                }
                if m.is_match(&name) && (last || Path::new(&join(&name)).is_dir()) {
                    next.push(join(&name));
                }
            }
        }
        found = next;
    }
    found
}

/// The config with its active includes expanded, for reading only.
fn flatten(path: &Path, seen: &mut Vec<PathBuf>) -> String {
    let resolved = resolve(Path::new(&py::expanduser(&path_str(path))));
    if seen.contains(&resolved) || !resolved.exists() {
        return String::new();
    }
    seen.push(resolved.clone());
    let Ok(content) = pyrt::read_text(&resolved) else { return String::new() };
    let include = rx(r#"^\s*include\s+"([^"]+)"\s*$"#);
    let mut out = String::new();
    for line in splitlines_keep(&content) {
        let stripped = strip_kdl_line_comments(line);
        let Some(c) = include.captures(strip(&stripped)) else {
            out.push_str(line);
            continue;
        };
        let mut pattern = expandvars(&py::expanduser(&c[1]));
        if !pattern.starts_with('/') {
            pattern = path_str(&pathlib(resolved.parent().unwrap_or(Path::new("/")).join(&pattern)));
        }
        let mut hits = glob(&pattern);
        hits.sort();
        for hit in hits {
            let nested = flatten(Path::new(&hit), seen);
            out.push_str(&nested);
            if !nested.is_empty() && !nested.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

fn parse_hot_corners(block: Option<&str>) -> Option<Vec<&'static str>> {
    let block = block?;
    let names = ["off", "top-left", "top-right", "bottom-left", "bottom-right"];
    let entries: Vec<&str> = pyrt::splitlines(block).into_iter().map(strip).filter(|t| names.contains(t)).collect();
    if entries.contains(&"off") {
        return Some(Vec::new());
    }
    let corners: Vec<&'static str> = entries
        .iter()
        .filter_map(|t| match *t {
            "top-left" => Some("topLeft"),
            "top-right" => Some("topRight"),
            "bottom-left" => Some("bottomLeft"),
            "bottom-right" => Some("bottomRight"),
            _ => None,
        })
        .collect();
    Some(if corners.is_empty() { vec!["topLeft"] } else { corners })
}

fn corner_list(c: &[&str]) -> Json {
    Json::Arr(c.iter().map(|x| s(x)).collect())
}

pub(super) fn get_hot_corners() -> R<i32> {
    let flat = strip_kdl_line_comments(&flatten(&config_path(), &mut Vec::new()));
    let gestures = extract_block(&flat, "gestures", true);
    let global = parse_hot_corners(gestures.and_then(|g| extract_block(g, "hot-corners", true))).unwrap_or_else(|| vec!["topLeft"]);
    let mut overrides: Vec<(String, Vec<&str>)> = Vec::new();
    for (name, block) in iter_output_blocks(&flat) {
        if let Some(o) = parse_hot_corners(extract_block(block, "hot-corners", true)) {
            match overrides.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = o,
                None => overrides.push((name, o)),
            }
        }
    }
    let mut effective = Vec::new();
    let (raw, rc) = run_niri(&["-j", "outputs"]);
    if rc == 0 {
        let data = py::json_loads(&raw).unwrap_or(Json::Obj(vec![]));
        if let Json::Obj(items) = &data {
            for (connector, d) in items {
                let identity: Vec<String> = ["make", "model", "serial"]
                    .iter()
                    .map(|k| strip(&d.get(k).map_or(String::new(), pyrt::str_of)).to_string())
                    .filter(|v| !v.is_empty())
                    .collect();
                let identity = identity.join(" ");
                // Python reads a set of the two names: with both configured its pick is random; the identity wins here.
                let mut chosen = None;
                for alias in [connector.as_str(), identity.as_str()] {
                    if let Some((_, o)) = overrides.iter().find(|(n, _)| n == alias) {
                        chosen = Some(o.clone());
                    }
                }
                effective.push((connector.clone(), corner_list(chosen.as_deref().unwrap_or(&global))));
            }
        }
    }
    emit(&obj(vec![
        ("global", corner_list(&global)),
        ("overrides", Json::Obj(overrides.into_iter().map(|(n, o)| (n, corner_list(&o))).collect())),
        ("effective", Json::Obj(effective)),
    ]));
    Ok(0)
}
