//! `niri-config <command> ...`: niri's config for iNiR's Settings, ported line for line from inir's
//! `scripts/niri-config.py` (every write is a surgical edit: comments, unknown settings and layout stay; a change
//! is validated by `niri validate` on a scratch copy before the live file, which niri watches, sees it).
//! Shell files (the defaults, the animation presets) come from `INIR_SHELL`, which `accel-run` sets.
//! Tested by `test/parity.py niri-config` against the Python on copies of real configs.

mod binds;
mod outputs;
mod read;
mod set;
mod tables;

use crate::material::py::{self, Json};
use crate::pyrt::{self, R, exc};
use regex::Regex;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

pub fn run_cli(args: &[String]) -> i32 {
    let Some(cmd) = args.first() else {
        return error(
            "No command. Use: outputs, apply-output, persist-output, persist-layout, get-input, get-hot-corners, get-layout, get-animations, get-window-rules, list-cursor-themes, sync-cursor, validate, detect-customizations, set, get-binds, set-bind, remove-bind, get-alt-tab, set-alt-tab",
        );
    };
    let a: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    let ran = match cmd.as_str() {
        "outputs" => outputs::outputs(),
        "apply-output" => outputs::apply_output(&a),
        "persist-output" => outputs::persist_output(&a),
        "persist-layout" => outputs::persist_layout(&a),
        "get-input" => read::get_input(),
        "get-hot-corners" => outputs::get_hot_corners(),
        "get-layout" => read::get_layout(),
        "get-animations" => read::get_animations(),
        "get-animation-presets" => set::get_animation_presets(),
        "apply-animation-preset" => set::apply_animation_preset(&a),
        "get-window-rules" => read::get_window_rules(),
        "get-blur" => read::get_blur(),
        "list-cursor-themes" => read::list_cursor_themes(),
        "sync-cursor" => set::sync_cursor(),
        "validate" => read::validate(),
        "detect-customizations" => read::detect_customizations(),
        "sync-backdrop-overview-shadow" => set::sync_backdrop_overview_shadow(&a),
        "set" => set::set(&a),
        "get-binds" => binds::get_binds(),
        "set-bind" => binds::set_bind(&a),
        "remove-bind" => binds::remove_bind(&a),
        "get-alt-tab" => binds::get_alt_tab(),
        "set-alt-tab" => binds::set_alt_tab(&a),
        other => return error(format!("Unknown command: {other}")),
    };
    ran.unwrap_or_else(pyrt::die)
}

// ---------------------------------------------------------------- output

pub(crate) fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

pub(crate) fn obj(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// `print(json.dumps(v))`.
pub(crate) fn emit(v: &Json) {
    println!("{}", py::json_dumps(v));
}

pub(crate) fn error(msg: impl Into<String>) -> i32 {
    emit(&obj(vec![("error", Json::Str(msg.into()))]));
    1
}

/// `d[a][b] = v` on a dict built up front (the key keeps its place).
pub(crate) fn put(o: &mut Json, path: &[&str], v: Json) {
    let Json::Obj(pairs) = o else { return };
    let (k, rest) = path.split_first().expect("path");
    match pairs.iter_mut().find(|(kk, _)| kk == k) {
        Some(slot) if rest.is_empty() => slot.1 = v,
        Some(slot) => put(&mut slot.1, rest, v),
        None if rest.is_empty() => pairs.push((k.to_string(), v)),
        None => {}
    }
}

pub(crate) fn has_key(o: &Json, path: &[&str]) -> bool {
    path.iter().try_fold(o, |o, k| o.get(k)).is_some()
}

// ---------------------------------------------------------------- python

pub(crate) fn rx(p: &str) -> Regex {
    pyrt::re(p)
}

pub(crate) fn esc(s: &str) -> String {
    regex::escape(s)
}

/// `re.sub(p, repl, text, count)` with Python's template rules.
pub(crate) fn sub(p: &str, repl: &str, text: &str, count: usize) -> R<String> {
    pyrt::sub(&rx(p), repl, text, count)
}

pub(crate) fn strip(t: &str) -> &str {
    py::strip(t)
}

pub(crate) fn rstrip(t: &str) -> &str {
    t.trim_end_matches(py::is_space)
}

pub(crate) fn lstrip(t: &str) -> &str {
    t.trim_start_matches(py::is_space)
}

/// `str.splitlines(keepends=True)`.
pub(crate) fn splitlines_keep(t: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = t.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            let mut end = i + c.len_utf8();
            if c == '\r' && chars.peek().is_some_and(|&(_, n)| n == '\n') {
                chars.next();
                end += 1;
            }
            out.push(&t[start..end]);
            start = end;
        }
    }
    if start < t.len() {
        out.push(&t[start..]);
    }
    out
}

/// `float(m.group(n))` on digits a pattern already picked.
pub(crate) fn float_of(t: &str) -> R<f64> {
    py::parse_float(t).ok_or_else(|| exc("ValueError", format!("could not convert string to float: {}", pyrt::repr_str(t))))
}

/// `int(t)` (`str`), as Python reads it.
pub(crate) fn int_of(t: &str) -> R<i128> {
    let digits = strip(t);
    if digits.bytes().all(|c| c.is_ascii_digit()) && !digits.is_empty() {
        if let Ok(v) = digits.parse::<i128>() {
            return Ok(v);
        }
    }
    py::parse_int(t)
        .map(i128::from)
        .ok_or_else(|| exc("ValueError", format!("invalid literal for int() with base 10: {}", pyrt::repr_str(t))))
}

/// `int(x)` for a float.
pub(crate) fn trunc(f: f64) -> R<i128> {
    if f.is_nan() {
        return Err(exc("ValueError", "cannot convert float NaN to integer"));
    }
    if f.is_infinite() {
        return Err(exc("OverflowError", "cannot convert float infinity to integer"));
    }
    Ok(f.trunc() as i128)
}

/// `float(v)` of a JSON value.
pub(crate) fn json_float(v: &Json) -> R<f64> {
    match v {
        Json::Float(f) => Ok(*f),
        Json::Int(i) => Ok(*i as f64),
        Json::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Json::Str(t) => float_of(t),
        other => Err(exc("TypeError", format!("float() argument must be a string or a real number, not '{}'", pyrt::type_name(other)))),
    }
}

/// `int(v)` of a JSON value.
pub(crate) fn json_int(v: &Json) -> R<i128> {
    match v {
        Json::Int(i) => Ok(*i),
        Json::Float(f) => trunc(*f),
        Json::Bool(b) => Ok(i128::from(*b)),
        Json::Str(t) => int_of(t),
        other => Err(exc("TypeError", format!("int() argument must be a string, a bytes-like object or a real number, not '{}'", pyrt::type_name(other)))),
    }
}

/// A path as pathlib spells it: no doubled slashes, no `.` parts, no trailing slash.
pub(crate) fn pathlib(p: impl AsRef<Path>) -> PathBuf {
    let p = p.as_ref();
    let raw = p.to_string_lossy();
    let mut out = PathBuf::new();
    if raw.starts_with("//") && !raw.starts_with("///") {
        out.push("//");
    }
    for c in p.components() {
        if c != Component::CurDir {
            out.push(c.as_os_str());
        }
    }
    if out.as_os_str().is_empty() { PathBuf::from(".") } else { out }
}

pub(crate) fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// `Path.resolve()` (not strict): links resolved as far as the path exists, the rest kept.
pub(crate) fn resolve(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
    if let Ok(r) = std::fs::canonicalize(&abs) {
        return r;
    }
    match (abs.parent(), abs.file_name()) {
        (Some(parent), Some(name)) => resolve(parent).join(name),
        _ => pathlib(abs),
    }
}

fn env_or(key: &str, default: impl FnOnce() -> String) -> String {
    std::env::var(key).unwrap_or_else(|_| default())
}

pub(crate) fn home() -> PathBuf {
    PathBuf::from(py::expanduser("~"))
}

pub(crate) fn xdg_config() -> PathBuf {
    pathlib(env_or("XDG_CONFIG_HOME", || py::expanduser("~/.config")))
}

pub(crate) fn config_dir() -> PathBuf {
    xdg_config().join("niri")
}

pub(crate) fn config_path() -> PathBuf {
    config_dir().join("config.kdl")
}

/// The running iNiR (`scripts/..` of the Python).
pub(crate) fn shell_root() -> PathBuf {
    crate::prewarm::shell_dir()
}

fn root_includes(rel: &str) -> bool {
    let cfg = config_path();
    if !cfg.exists() {
        return false;
    }
    let Ok(content) = pyrt::read_text(&cfg) else { return false };
    rx(&format!(r#"(?m)^\s*include\s+"{}"\s*$"#, esc(rel))).is_match(&content)
}

/// The file a section lives in: its own `config.d` file when the root config includes it, else the root.
pub(crate) fn section_file(rel: &str) -> PathBuf {
    let dir = config_dir();
    let root = dir.join("config.kdl");
    let modular = dir.join(rel);
    if pyrt::exists(&modular) && root_includes(rel) {
        return modular;
    }
    if pyrt::exists(&root) {
        return root;
    }
    modular
}

pub(crate) fn read_or_empty(p: &Path) -> R<String> {
    if pyrt::exists(p) { pyrt::read_text(p) } else { Ok(String::new()) }
}

/// `sorted(dir.glob("*.kdl"))`: pathlib's `*` takes dot files too.
pub(crate) fn kdl_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> =
        rd.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".kdl")).map(|e| dir.join(e.file_name())).collect();
    out.sort();
    out
}

// ---------------------------------------------------------------- processes

pub(crate) struct Ran {
    pub out: String,
    pub err: String,
    pub code: i32,
}

/// `subprocess.run(cmd, capture_output=True, text=True, timeout=secs)`; `Err` is `str()` of what it raised.
pub(crate) fn run(cmd: &[&str], secs: Option<u64>) -> Result<Ran, String> {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};
    let mut child = match Command::new(cmd[0]).args(&cmd[1..]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            return Err(match e.raw_os_error() {
                Some(code) => format!("[Errno {code}] {}: {}", pyrt::strerror(code), pyrt::repr_str(cmd[0])),
                None => e.to_string(),
            });
        }
    };
    let mut pipes = [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as _)];
    let readers: Vec<_> = pipes
        .iter_mut()
        .map(|p| {
            let mut p = p.take();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(p) = p.as_mut() {
                    let _ = p.read_to_end(&mut buf);
                }
                String::from_utf8_lossy(&buf).into_owned()
            })
        })
        .collect();
    let deadline = secs.map(|s| Instant::now() + Duration::from_secs(s));
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if deadline.is_some_and(|d| Instant::now() >= d) => {
                let _ = child.kill();
                let _ = child.wait();
                let list = cmd.iter().map(|c| pyrt::repr_str(c)).collect::<Vec<_>>().join(", ");
                return Err(format!("Command '[{list}]' timed out after {} seconds", secs.unwrap_or_default()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let mut texts = readers.into_iter().map(|r| r.join().unwrap_or_default());
    let (out, err) = (texts.next().unwrap_or_default(), texts.next().unwrap_or_default());
    let code = status.code().unwrap_or_else(|| -status.signal().unwrap_or(0));
    Ok(Ran { out, err, code })
}

/// `niri msg <args>`: its stripped stdout and exit code, or the error text and 1.
pub(crate) fn run_niri(args: &[&str]) -> (String, i32) {
    let cmd: Vec<&str> = ["niri", "msg"].into_iter().chain(args.iter().copied()).collect();
    match run(&cmd, Some(5)) {
        Ok(r) => (strip(&r.out).to_string(), r.code),
        Err(e) => (e, 1),
    }
}

// ---------------------------------------------------------------- validated writes

/// `niri validate` on the live config: (valid, its report). No config or no niri: valid.
pub(crate) fn validate_config() -> (bool, String) {
    let cfg = config_path();
    if !cfg.exists() {
        return (true, String::new());
    }
    match run(&["niri", "validate", "-c", &path_str(&cfg)], Some(5)) {
        Ok(r) => (r.code == 0, strip(&(r.out + &r.err)).to_string()),
        Err(_) => (true, String::new()),
    }
}

/// `shutil.copytree(src, dst, symlinks=True)`.
fn copytree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let (from, to) = (e.path(), dst.join(e.file_name()));
        let ft = e.file_type()?;
        if ft.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&from)?, &to)?;
        } else if ft.is_dir() {
            copytree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mkdtemp(prefix: &str) -> R<TempDir> {
    let template = std::env::temp_dir().join(format!("{prefix}XXXXXX"));
    let mut raw = std::ffi::CString::new(template.as_os_str().as_encoded_bytes()).expect("path").into_bytes_with_nul();
    if unsafe { libc::mkdtemp(raw.as_mut_ptr().cast()) }.is_null() {
        return Err(pyrt::os_exc(&std::io::Error::last_os_error(), &template));
    }
    raw.pop();
    Ok(TempDir(PathBuf::from(String::from_utf8_lossy(&raw).into_owned())))
}

/// `content` validated as if it were `path`, on a scratch copy of the niri dir; `None` for a file outside it.
fn validate_candidate(path: &Path, content: &str) -> R<Option<(bool, String)>> {
    let dir = config_dir();
    let cfg = config_path();
    let Ok(rel) = resolve(path).strip_prefix(resolve(&dir)).map(Path::to_path_buf) else { return Ok(None) };
    if !cfg.exists() {
        return Ok(Some((true, String::new())));
    }
    let tmp = mkdtemp("inir-niri-validate-")?;
    let scratch = tmp.0.join("niri");
    copytree(&dir, &scratch).map_err(|e| exc("shutil.Error", e.to_string()))?;
    let target = scratch.join(&rel);
    if let Some(parent) = target.parent() {
        pyrt::makedirs(parent)?;
    }
    pyrt::write_text(&target, content)?;
    let checked = match run(&["niri", "validate", "-c", &path_str(&scratch.join("config.kdl"))], Some(5)) {
        Ok(r) => (r.code == 0, strip(&(r.out + &r.err)).to_string()),
        Err(_) => (true, String::new()),
    };
    Ok(Some(checked))
}

/// The one line of niri's miette report a person can act on.
pub(crate) fn validation_summary(err: &str) -> String {
    let plain = rx(r"\x1b\[[0-9;]*m").replace_all(err, "");
    let lead = rx(r"^Error:\s*");
    let lines: Vec<String> = pyrt::splitlines(&plain).iter().map(|l| lead.replace(strip(l), "").into_owned()).collect();
    let marks = rx(r"^[×╰─▶ ]+");
    let reasons: Vec<String> =
        lines.iter().filter(|l| l.starts_with('×') || l.starts_with("╰─▶")).map(|l| marks.replace(l, "").into_owned()).collect();
    let generic = ["error loading config", "error parsing", "failed to parse included config"];
    let specific: Vec<&String> = reasons.iter().filter(|r| !r.is_empty() && !generic.iter().any(|g| r.starts_with(g))).collect();
    let dots = rx(r"^[·╰─ ]+");
    let labels: Vec<String> = lines.iter().filter(|l| l.contains("╰──")).map(|l| dots.replace(l, "").into_owned()).collect();
    let mut summary = match (specific.last(), reasons.last()) {
        (Some(s), _) => (*s).clone(),
        (None, Some(r)) => r.clone(),
        _ => "invalid config".to_string(),
    };
    if let Some(label) = labels.last().filter(|l| !l.is_empty() && !summary.contains(l.as_str())) {
        summary = format!("{summary} ({label})");
    }
    summary
}

fn invalid(err: &str) -> i32 {
    emit(&obj(vec![("success", Json::Bool(false)), ("error", s(&validation_summary(err))), ("detail", s(err))]));
    1
}

/// Validate first, then write; a file outside the niri dir is written, validated and put back.
pub(crate) fn write_validated(path: &Path, content: &str) -> R<i32> {
    if let Some((valid, err)) = validate_candidate(path, content)? {
        if !valid {
            return Ok(invalid(&err));
        }
        pyrt::write_text(path, content)?;
        emit(&obj(vec![("success", Json::Bool(true)), ("file", s(&path_str(path)))]));
        return Ok(0);
    }
    let backup = if pyrt::exists(path) { Some(pyrt::read_text(path)?) } else { None };
    pyrt::write_text(path, content)?;
    let (valid, err) = validate_config();
    if !valid {
        match backup {
            Some(b) => pyrt::write_text(path, &b)?,
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
        return Ok(invalid(&err));
    }
    emit(&obj(vec![("success", Json::Bool(true)), ("file", s(&path_str(path)))]));
    Ok(0)
}

// ---------------------------------------------------------------- kdl text

/// (block start, inner start, inner end, block end) of `name { ... }`.
pub(crate) type Bounds = (usize, usize, usize, usize);

fn close_brace(content: &str, inner_start: usize) -> Option<usize> {
    let b = content.as_bytes();
    let (mut depth, mut i) = (1, inner_start);
    while i < b.len() && depth > 0 {
        match b[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    (depth == 0).then_some(i)
}

pub(crate) fn brace_depth_before(content: &str, pos: usize) -> i32 {
    let mut depth = 0;
    for &c in &content.as_bytes()[..pos] {
        if c == b'{' {
            depth += 1;
        } else if c == b'}' && depth > 0 {
            depth -= 1;
        }
    }
    depth
}

pub(crate) fn find_block_bounds(content: &str, name: &str, top: bool) -> Option<Bounds> {
    for m in rx(&format!(r"(?:^|\n)\s*{}\s*\{{", esc(name))).find_iter(content) {
        if top && brace_depth_before(content, m.start()) != 0 {
            continue;
        }
        if let Some(i) = close_brace(content, m.end()) {
            return Some((m.start(), m.end(), i - 1, i));
        }
    }
    None
}

pub(crate) fn extract_block<'a>(content: &'a str, name: &str, top: bool) -> Option<&'a str> {
    find_block_bounds(content, name, top).map(|(_, a, b, _)| &content[a..b])
}

pub(crate) fn has_top_level_flag(block: &str, flag: &str) -> bool {
    let mut depth: i64 = 0;
    for line in pyrt::splitlines(block) {
        if depth == 0 && strip(line) == flag {
            return true;
        }
        depth += line.matches('{').count() as i64 - line.matches('}').count() as i64;
    }
    false
}

/// `//` comments out, quoted strings and line structure kept.
pub(crate) fn strip_kdl_line_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for line in splitlines_keep(content) {
        let b = line.as_bytes();
        let (mut in_string, mut escaped, mut cut) = (false, false, None);
        for i in 0..b.len() {
            let ch = b[i];
            if escaped {
                escaped = false;
            } else if ch == b'\\' && in_string {
                escaped = true;
            } else if ch == b'"' {
                in_string = !in_string;
            } else if ch == b'/' && !in_string && b.get(i + 1) == Some(&b'/') {
                cut = Some(i);
                break;
            }
        }
        match cut {
            None => out.push_str(line),
            Some(c) => {
                out.push_str(&line[..c]);
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// The same text with every `//` comment blanked, so offsets still line up with the original.
pub(crate) fn mask_kdl_comments(text: &str) -> String {
    rx(r"//[^\n]*").replace_all(text, |c: &regex::Captures| " ".repeat(c[0].len())).into_owned()
}

/// `key value` set in a block: its first line replaced, else added at the end.
pub(crate) fn set_in_block(block: &str, key: &str, value: &str) -> R<String> {
    let pattern = format!(r"(?m)^([ \t]*){}\b[^\n]*", esc(key));
    if rx(&pattern).is_match(block) {
        let repl = if value.is_empty() { format!(r"\g<1>{key}") } else { format!(r"\g<1>{key} {value}") };
        return sub(&pattern, &repl, block, 1);
    }
    Ok(if value.is_empty() { format!("{}\n    {key}\n", rstrip(block)) } else { format!("{}\n    {key} {value}\n", rstrip(block)) })
}

pub(crate) fn splice(content: &str, a: usize, b: usize, mid: &str) -> String {
    format!("{}{mid}{}", &content[..a], &content[b..])
}

/// A bare flag (`tap`, `numlock`) on or off inside a block: uncommented or added, removed when off.
pub(crate) fn toggle_flag(content: &str, parent: &str, flag: &str, enable: bool, top: bool) -> R<String> {
    let f = esc(flag);
    let Some((_, a, b, _)) = find_block_bounds(content, parent, top) else { return Ok(content.to_string()) };
    let block = &content[a..b];
    let has_flag = rx(&format!(r"(?m)^\s*{f}\s*$")).is_match(block);
    let has_commented = rx(&format!(r"(?m)^\s*//\s*{f}\s*$")).is_match(block);
    let new = if enable && has_flag {
        return Ok(content.to_string());
    } else if enable && has_commented {
        sub(&format!(r"(?m)^(\s*)//\s*{f}\s*$"), &format!(r"\g<1>{flag}"), block, 1)?
    } else if enable {
        format!("{}\n        {flag}\n    ", rstrip(block))
    } else if has_flag {
        sub(&format!(r"(?m)^[ \t]*{f}[ \t]*\n"), "", block, 1)?
    } else {
        return Ok(content.to_string());
    };
    Ok(splice(content, a, b, &new))
}

pub(crate) fn remove_key_from_block_content(block: &str, key: &str) -> R<String> {
    sub(&format!(r"(?m)^[ \t]*{}\b[^\n]*\n?", esc(key)), "", block, 1)
}

pub(crate) fn remove_key_from_section(content: &str, section: &str, prop: &str, top: bool) -> R<String> {
    let Some((_, a, b, _)) = find_block_bounds(content, section, top) else { return Ok(content.to_string()) };
    Ok(splice(content, a, b, &remove_key_from_block_content(&content[a..b], prop)?))
}

pub(crate) fn ensure_subsection(content: &str, parent: &str, sub_name: &str) -> String {
    let Some((_, a, b, _)) = find_block_bounds(content, parent, true) else { return content.to_string() };
    if find_block_bounds(content, sub_name, false).is_some() {
        return content.to_string();
    }
    splice(content, a, b, &format!("{}\n    {sub_name} {{\n    }}\n", rstrip(&content[a..b])))
}

pub(crate) fn set_value_in_block(content: &str, section: &str, prop: &str, value: &str, top: bool) -> R<String> {
    let Some((_, a, b, _)) = find_block_bounds(content, section, top) else { return Ok(content.to_string()) };
    Ok(splice(content, a, b, &set_in_block(&content[a..b], prop, value)?))
}

/// The last bare `on`/`off` line wins, as in niri; without one the block keeps niri's default.
pub(crate) fn flag_state(block: &str, default: bool) -> bool {
    let masked = mask_kdl_comments(block);
    match rx(r"(?m)^[ \t]*(on|off)[ \t]*$").captures_iter(&masked).last() {
        Some(c) => &c[1] == "on",
        None => default,
    }
}

/// Exactly one explicit flag left in a subsection block (border, focus-ring, shadow).
pub(crate) fn toggle_subsection_enabled(content: &str, section: &str, enable: bool) -> String {
    let Some((_, a, b, _)) = find_block_bounds(content, section, false) else { return content.to_string() };
    let block = &content[a..b];
    let masked = mask_kdl_comments(block);
    let bare_flag = rx(r"^(?:[ \t]*(on|off)[ \t]*\n?)$");
    let kept: String =
        splitlines_keep(block).into_iter().zip(splitlines_keep(&masked)).filter(|(_, bare)| !bare_flag.is_match(bare)).map(|(raw, _)| raw).collect();
    let indent = rx(r"(?m)^([ \t]+)\S").captures(block).map_or("        ".to_string(), |c| c[1].to_string());
    let flag = format!("{indent}{}\n", if enable { "on" } else { "off" });
    splice(content, a, b, &format!("\n{flag}{}", kept.trim_start_matches('\n')))
}

pub(crate) fn set_xkb_value(content: &str, prop: &str, value: &str) -> R<String> {
    let Some((kb_start, kb_a, kb_b, _)) = find_block_bounds(content, "keyboard", false) else { return Ok(content.to_string()) };
    let kb = &content[kb_a..kb_b];
    let Some((_, xa, xb, _)) = find_block_bounds(kb, "xkb", false) else {
        if value.is_empty() {
            return Ok(content.to_string());
        }
        let opening = pyrt::splitlines(&content[kb_start..kb_a]).last().copied().unwrap_or_default();
        let kb_indent: String = opening.chars().take_while(|c| py::is_space(*c)).collect();
        let xkb_indent = format!("{kb_indent}    ");
        let mut trailing = kb[rstrip(kb).len()..].to_string();
        if trailing.is_empty() {
            trailing = format!("\n{kb_indent}");
        }
        let block = format!("{xkb_indent}xkb {{\n{xkb_indent}    {prop} \"{value}\"\n{xkb_indent}}}");
        return Ok(splice(content, kb_a, kb_b, &format!("{}\n{block}{trailing}", rstrip(kb))));
    };
    let xkb = &kb[xa..xb];
    let new_xkb = if value.is_empty() { remove_key_from_block_content(xkb, prop)? } else { set_in_block(xkb, prop, &format!("\"{value}\""))? };
    Ok(splice(content, kb_a, kb_b, &splice(kb, xa, xb, &new_xkb)))
}

pub(crate) fn set_shadow_offset(content: &str, value: &str) -> R<String> {
    let parts: Vec<&str> = value.splitn(2, ',').map(strip).collect();
    if parts.len() != 2 {
        return Ok(content.to_string());
    }
    set_value_in_block(content, "shadow", "offset", &format!("x={} y={}", parts[0], parts[1]), false)
}

/// Every top-level `output "name" { ... }`: (name, inner text).
pub(crate) fn iter_output_blocks(content: &str) -> Vec<(String, &str)> {
    let mut out = Vec::new();
    for c in rx(r#"(?:^|\n)\s*output\s+"([^"]+)"\s*\{"#).captures_iter(content) {
        let m = c.get(0).unwrap();
        if brace_depth_before(content, m.start()) != 0 {
            continue;
        }
        if let Some(i) = close_brace(content, m.end()) {
            out.push((c[1].to_string(), &content[m.end()..i - 1]));
        }
    }
    out
}

pub(crate) fn find_output_block_bounds(content: &str, name: &str) -> Option<Bounds> {
    for m in rx(&format!(r#"(?m)^[ \t]*output\s+"{}"\s*\{{"#, esc(name))).find_iter(content) {
        if brace_depth_before(content, m.start()) != 0 {
            continue;
        }
        if let Some(i) = close_brace(content, m.end()) {
            return Some((m.start(), m.end(), i - 1, i));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_and_flags() {
        let c = "input {\n    keyboard {\n        numlock\n    }\n    touchpad {\n        // tap\n    }\n}\n";
        assert_eq!(extract_block(c, "keyboard", false), Some("\n        numlock\n    "));
        assert!(find_block_bounds(c, "keyboard", true).is_none());
        let on = toggle_flag(c, "touchpad", "tap", true, false).unwrap();
        assert!(on.contains("touchpad {\n        tap}"));
        assert_eq!(strip_kdl_line_comments("a \"//x\" // y\nb"), "a \"//x\" \nb");
        assert_eq!(set_in_block("\n    gaps 10\n", "gaps", "25").unwrap(), "\n    gaps 25\n");
    }

    #[test]
    fn python_float_formatting() {
        // `{:.4}` and `{:.1}` round ties to even, as Python's format does.
        assert_eq!(format!("{:.1}", 0.25), "0.2");
        assert_eq!(format!("{:.0}", 2.5), "2");
        assert_eq!(format!("{:.4}", 0.00005), "0.0001");
    }
}
