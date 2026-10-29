//! The supervised family switch, ported from `switch-family-restart.py`. Qt 6.11 can crash tearing layer-shell windows
//! down in-process, so the shell is stopped, config.json is rewritten atomically, and the shell is started again.

use crate::material::py::{self, Json};
use crate::pyrt;
use regex::Regex;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

fn launcher(fallback: Option<&str>) -> Option<PathBuf> {
    let set = std::env::var("INIR_FAMILY_SWITCH_INIR").unwrap_or_default();
    if !set.trim().is_empty() {
        return Some(PathBuf::from(set.trim()));
    }
    let exec = |p: &Path| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    let bin = std::env::var_os("XDG_BIN_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/bin"));
    let candidates = [Some(bin.join("inir")), pyrt::which("inir"), fallback.map(PathBuf::from)];
    candidates.into_iter().flatten().find(|p| exec(p))
}

fn merged(current: Option<&Json>, base: &[String]) -> Json {
    let mut values = match current {
        Some(Json::Arr(a)) => a.clone(),
        _ => Vec::new(),
    };
    if values.is_empty() {
        return Json::Arr(base.iter().cloned().map(Json::Str).collect());
    }
    for panel in base {
        if !values.iter().any(|v| matches!(v, Json::Str(s) if s == panel)) {
            values.push(Json::Str(panel.clone()));
        }
    }
    Json::Arr(values)
}

fn set(obj: &mut Vec<(String, Json)>, key: &str, value: Json) {
    match obj.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = value,
        None => obj.push((key.to_string(), value)),
    }
}

/// `json.dumps(s, ensure_ascii=False)`
fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The shell's own layout (Qt's indented JSON), so its first save after the restart rewrites nothing.
fn qt_json(out: &mut String, v: &Json, depth: usize) {
    let (pad, inner) = ("    ".repeat(depth), "    ".repeat(depth + 1));
    match v {
        Json::Obj(o) if o.is_empty() => out.push_str(&format!("{{\n{pad}}}")),
        Json::Arr(a) if a.is_empty() => out.push_str(&format!("[\n{pad}]")),
        Json::Obj(o) => {
            out.push_str("{\n");
            for (i, (k, x)) in o.iter().enumerate() {
                out.push_str(if i == 0 { "" } else { ",\n" });
                out.push_str(&inner);
                write_str(out, k);
                out.push_str(": ");
                qt_json(out, x, depth + 1);
            }
            out.push_str(&format!("\n{pad}}}"));
        }
        Json::Arr(a) => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                out.push_str(if i == 0 { "" } else { ",\n" });
                out.push_str(&inner);
                qt_json(out, x, depth + 1);
            }
            out.push_str(&format!("\n{pad}]"));
        }
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::Float(f) if f.is_nan() => out.push_str("NaN"),
        Json::Float(f) if f.is_infinite() => out.push_str(if *f > 0.0 { "Infinity" } else { "-Infinity" }),
        Json::Float(f) => out.push_str(&py::float_repr(*f)),
        Json::Str(s) => write_str(out, s),
    }
}

fn write_atomic(path: &Path, data: &Json) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mode = fs::metadata(path).map_or(0o600, |m| m.permissions().mode() & 0o777);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("config.json");
    let (mut n, pid) = (0u32, std::process::id());
    let (tmp, mut f) = loop {
        let tmp = dir.join(format!(".{name}.{pid}.{n}"));
        match fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp) {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 100 => n += 1,
            Err(e) => return Err(e),
        }
    };
    let mut text = String::new();
    qt_json(&mut text, data, 0);
    text.push('\n');
    let done = f.write_all(text.as_bytes()).and_then(|()| f.sync_all()).and_then(|()| fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))).and_then(|()| fs::rename(&tmp, path));
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done
}

fn service(inir: &Path, verb: &str) -> std::io::Result<bool> {
    Command::new(inir).args(["service", verb]).status().map(|s| s.success())
}

/// `switch-family <family> <base-panels-json> [fallback-launcher]`; exit codes as the python's: 1 no launcher, 2 usage,
/// 3 config, 4 stop failed, 5 write failed, 6 restart failed.
pub fn run_cli(args: &[String]) -> i32 {
    if !(2..=3).contains(&args.len()) {
        eprintln!("usage: inir-accel switch-family <family> <base-panels-json> [fallback-launcher]");
        return 2;
    }
    let family = &args[0];
    // a family is a hub plugin: its id follows the hub's plugin id rule
    if !Regex::new(r"^[a-z0-9][a-z0-9-]{0,46}[a-z0-9]$").unwrap().is_match(family) {
        eprintln!("invalid panel family: {family}");
        return 2;
    }
    let base: Vec<String> = match py::json_loads(&args[1]) {
        Some(Json::Arr(a)) if a.iter().all(|v| matches!(v, Json::Str(_))) => a.into_iter().filter_map(|v| if let Json::Str(s) = v { Some(s) } else { None }).collect(),
        Some(_) => {
            eprintln!("base panels must be a JSON string array");
            return 2;
        }
        None => {
            eprintln!("invalid panel list: not JSON");
            return 2;
        }
    };
    let base_dir = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
    let path = base_dir.join("illogical-impulse/config.json");
    let mut config = match fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| py::json_loads(&t).ok_or_else(|| "not JSON".to_string())) {
        Ok(Json::Obj(o)) => o,
        Ok(_) => {
            eprintln!("config root is not an object");
            return 3;
        }
        Err(e) => {
            eprintln!("cannot read config: {e}");
            return 3;
        }
    };
    let Some(inir) = launcher(args.get(2).map(String::as_str)) else {
        eprintln!("family switch failed: inir launcher not found");
        return 1;
    };
    match service(&inir, "stop") {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("could not stop supervised iNiR service");
            return 4;
        }
        Err(e) => {
            eprintln!("family switch failed: {e}");
            return 5;
        }
    }
    let mut result = 0;
    let current = config.iter().find(|(k, _)| k == "enabledPanels").map(|(_, v)| v.clone());
    set(&mut config, "panelFamily", Json::Str(family.clone()));
    set(&mut config, "enabledPanels", merged(current.as_ref(), &base));
    if matches!(&current, Some(Json::Arr(a)) if !a.is_empty()) {
        let known = config.iter().find(|(k, _)| k == "knownPanels").map(|(_, v)| v.clone());
        set(&mut config, "knownPanels", merged(known.as_ref(), &base));
    }
    if let Err(e) = write_atomic(&path, &Json::Obj(config)) {
        eprintln!("family switch failed: {e}");
        result = 5;
    }
    let mut restarted = false;
    for attempt in 0..2 {
        restarted = service(&inir, "start").unwrap_or(false);
        if restarted {
            break;
        }
        if attempt == 0 {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    if !restarted {
        eprintln!("could not restart supervised iNiR service");
        if result == 0 {
            result = 6;
        }
    }
    result
}
