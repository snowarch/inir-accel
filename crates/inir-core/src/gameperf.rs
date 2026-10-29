//! Live MangoHud samples for the game the compositor has focused, ported from `game-performance-monitor.py`.
//! MangoHud writes its CSV log from inside the game, so this finds the CSV held open by the game's process tree,
//! then falls back to the configured log directories for XWayland clients (whose compositor PID is the shared
//! XWayland server). Discovery and static metadata are cached while the target stays; each line is the latest
//! sample as JSON.

use crate::material::py::expanduser;
use crate::pyrt::{self, splitlines};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const METRIC_FIELDS: [&str; 17] = [
    "fps", "frametime", "cpu_load", "cpu_power", "gpu_load", "cpu_temp", "gpu_temp", "gpu_core_clock", "gpu_mem_clock",
    "gpu_vram_used", "gpu_power", "gpu_voltage", "ram_used", "swap_used", "process_rss", "cpu_mhz", "elapsed",
];
const METADATA_FIELDS: [(&str, &str); 6] = [
    ("os", "osName"),
    ("cpu", "cpuName"),
    ("gpu", "gpuName"),
    ("kernel", "kernelVersion"),
    ("driver", "gpuDriver"),
    ("cpuscheduler", "cpuScheduler"),
];
const MAX_FRESH_LOG_AGE_MS: f64 = 3000.0;
const IGNORED_IDENTITY_TOKENS: [&str; 9] = ["app", "com", "game", "gamescope", "net", "org", "proton", "steam", "wine"];

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn mtime(path: &Path) -> Option<f64> {
    fs::metadata(path).ok().map(|m| m.mtime() as f64 + m.mtime_nsec() as f64 / 1e9)
}

// csv.reader's default dialect on one line (comma, double quotes, "" inside them), each field stripped
fn csv_fields(line: &str) -> Vec<String> {
    if line.is_empty() {
        return Vec::new();
    }
    let (mut fields, mut field) = (Vec::new(), String::new());
    let (mut quoted, mut at_start) = (false, true);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c != '"' {
                field.push(c);
            } else if chars.peek() == Some(&'"') {
                chars.next();
                field.push('"');
            } else {
                quoted = false;
            }
        } else if c == ',' {
            fields.push(std::mem::take(&mut field));
            at_start = true;
            continue;
        } else if c == '"' && at_start {
            quoted = true;
        } else {
            field.push(c);
        }
        at_start = false;
    }
    fields.push(field);
    fields.iter().map(|f| f.trim().to_string()).collect()
}

fn number(value: Option<&str>) -> Option<f64> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    value.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn process_tree(root: i32) -> Vec<i32> {
    let (mut pending, mut seen, mut out) = (vec![root], HashSet::new(), Vec::new());
    while let Some(pid) = pending.pop() {
        if pid <= 0 || !seen.insert(pid) {
            continue;
        }
        out.push(pid);
        let Ok(children) = fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")) else { continue };
        pending.extend(children.split_whitespace().filter_map(|c| c.parse::<i32>().ok()));
    }
    out
}

fn process_ancestors(root: i32) -> Vec<i32> {
    let (mut out, mut seen, mut current) = (Vec::new(), HashSet::new(), root);
    while current > 0 && seen.insert(current) {
        out.push(current);
        let Ok(status) = fs::read_to_string(format!("/proc/{current}/status")) else { break };
        let parent = status.lines().find_map(|l| l.strip_prefix("PPid:\t")).unwrap_or("0");
        match parent.trim().parse() {
            Ok(p) => current = p,
            Err(_) => break,
        }
    }
    out
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|p| p.as_os_str() == path.as_os_str()) {
        paths.push(path);
    }
}

fn is_log(path: &Path) -> bool {
    !path.file_name().is_some_and(|n| n.to_string_lossy().ends_with("_summary.csv")) && path.is_file()
}

// non-summary CSV files held open by the process tree
fn open_log_files(root: i32) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for pid in process_tree(root) {
        let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fd")) else { continue };
        for fd in fds.flatten() {
            let Ok(target) = fs::read_link(fd.path()) else { continue };
            let text = target.to_string_lossy();
            if text.ends_with(" (deleted)") || !text.to_lowercase().ends_with(".csv") || !is_log(&target) {
                continue;
            }
            push_unique(&mut out, target);
        }
    }
    out
}

fn directory_log_files(directory: &str) -> Vec<PathBuf> {
    if directory.is_empty() {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(expanduser(directory)) else { return Vec::new() };
    entries.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".csv") && is_log(p)).collect()
}

fn read_prefix(path: &Path, limit: u64) -> Option<String> {
    let mut buf = Vec::new();
    File::open(path).ok()?.take(limit).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn header_for_log(path: &Path) -> Vec<String> {
    let prefix = read_prefix(path, 65536).unwrap_or_default();
    splitlines(&prefix)
        .into_iter()
        .map(csv_fields)
        .find(|f| f.len() >= 2 && f[0] == "fps" && f[1] == "frametime")
        .unwrap_or_default()
}

fn metadata_for_log(path: &Path) -> Map<String, Value> {
    let prefix = read_prefix(path, 65536).unwrap_or_default();
    let lines = splitlines(&prefix);
    let mut metadata = Map::new();
    for (i, line) in lines.iter().enumerate().take(lines.len().saturating_sub(1)) {
        let header = csv_fields(line);
        if header.first().is_none_or(|h| h != "os") {
            continue;
        }
        let values = csv_fields(lines[i + 1]);
        for (at, name) in header.iter().enumerate() {
            if let (Some((_, key)), Some(value)) = (METADATA_FIELDS.iter().find(|(f, _)| f == name), values.get(at)) {
                metadata.insert(key.to_string(), json!(value));
            }
        }
        break;
    }
    metadata
}

fn mangohud_version(path: &Path) -> String {
    static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^v\d+(?:\.\d+)+$").unwrap());
    let prefix = read_prefix(path, 4096).unwrap_or_default();
    splitlines(&prefix).into_iter().map(str::trim).find(|v| VERSION.is_match(v)).unwrap_or_default().to_string()
}

fn latest_row(path: &Path, header_len: usize) -> Vec<String> {
    let tail = (|| {
        let mut file = File::open(path).ok()?;
        let size = file.seek(SeekFrom::End(0)).ok()?;
        file.seek(SeekFrom::Start(size.saturating_sub(131072))).ok()?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    })()
    .unwrap_or_default();
    splitlines(&tail)
        .into_iter()
        .rev()
        .map(csv_fields)
        .find(|f| f.len() >= header_len && number(f.first().map(String::as_str)).is_some() && number(f.get(1).map(String::as_str)).is_some())
        .unwrap_or_default()
}

fn read_log(path: &Path, header: &[String], static_metadata: &Map<String, Value>) -> Map<String, Value> {
    let mut values = Map::new();
    let row = if header.is_empty() { Vec::new() } else { latest_row(path, header.len()) };
    if row.is_empty() {
        values.insert("available".into(), json!(false));
        return values;
    }
    values.insert("available".into(), json!(true));
    for field in METRIC_FIELDS {
        let value = header.iter().position(|h| h == field).and_then(|i| number(row.get(i).map(String::as_str)));
        values.insert(field.into(), value.map_or(Value::Null, |n| json!(n)));
    }
    values.extend(static_metadata.clone());
    match fs::metadata(path) {
        Ok(st) => {
            let mtime_ns = st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128;
            values.insert("sampleId".into(), json!(format!("{}:{}:{mtime_ns}", st.ino(), st.size())));
            let age = ((now() - (st.mtime() as f64 + st.mtime_nsec() as f64 / 1e9)) * 1000.0).trunc() as i64;
            values.insert("logAgeMs".into(), json!(age.max(0)));
        }
        Err(_) => {
            values.insert("available".into(), json!(false));
        }
    }
    values
}

fn process_command(pid: i32) -> String {
    let raw = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    raw.split(|&b| b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect::<Vec<_>>().join(" ")
}

fn is_xwayland_process(pid: i32) -> bool {
    static XWAYLAND: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"(?:^|/)xwayland(?:-satellite)?(?:\s|$)"));
    let name = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim().to_lowercase();
    name.starts_with("xwayland") || XWAYLAND.is_match(&process_command(pid).to_lowercase())
}

fn process_details(pid: i32) -> Map<String, Value> {
    static PROTON: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"(proton[^/\s]*)"));
    if pid <= 0 {
        return Map::new();
    }
    let mut pids = process_tree(pid);
    for ancestor in process_ancestors(pid) {
        if !pids.contains(&ancestor) {
            pids.push(ancestor);
        }
    }
    let commands: Vec<String> = pids.iter().map(|&p| process_command(p)).collect();
    let lower_commands = commands.iter().filter(|c| !c.is_empty()).cloned().collect::<Vec<_>>().join("\n").to_lowercase();
    let lower_maps = pids
        .iter()
        .map(|&p| fs::read(format!("/proc/{p}/maps")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    let graphics_api = if lower_maps.contains("libvulkan") || lower_maps.contains("winevulkan") {
        "Vulkan"
    } else if ["libgl.so", "libglx", "libgles"].iter().any(|t| lower_maps.contains(t)) {
        "OpenGL"
    } else {
        ""
    };
    let mut wine_proton = String::new();
    for command in &commands {
        let lower = command.to_lowercase();
        if lower.contains("proton") {
            wine_proton = PROTON.captures(&lower).map_or("Proton".to_string(), |m| m[1].to_string());
            break;
        }
        if lower.contains("wine") {
            wine_proton = "Wine".into();
        }
    }
    let fex = if lower_commands.contains("fex") || lower_maps.contains("fex") { "FEX detected" } else { "" };
    let mut details = Map::new();
    details.insert("sourcePid".into(), json!(pid));
    details.insert("graphicsApi".into(), json!(graphics_api));
    details.insert("wineProton".into(), json!(wine_proton));
    details.insert("fexStats".into(), json!(fex));
    details
}

fn log_owner_pids(path: &Path) -> Vec<i32> {
    let mut owners: Vec<i32> = fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
                .filter(|pid| {
                    fs::read_dir(format!("/proc/{pid}/fd"))
                        .is_ok_and(|fds| fds.flatten().any(|fd| fs::read_link(fd.path()).is_ok_and(|t| t.as_os_str() == path.as_os_str())))
                })
                .collect()
        })
        .unwrap_or_default();
    owners.sort();
    owners
}

fn identity_tokens(values: &[&str]) -> HashSet<String> {
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9]+").unwrap());
    values
        .iter()
        .flat_map(|v| TOKEN.find_iter(&v.to_lowercase()).map(|m| m.as_str().to_string()).collect::<Vec<_>>())
        .filter(|t| t.len() >= 3 && t.chars().any(|c| c.is_ascii_alphabetic()) && !IGNORED_IDENTITY_TOKENS.contains(&t.as_str()))
        .collect()
}

struct Monitor {
    pid: i64,
    target_key: String,
    identity: HashSet<String>,
    allow_directory_fallback: bool,
    log_file: Option<String>,
    log_dirs: Vec<String>,
    discovery_interval: f64,
    path: Option<PathBuf>,
    header: Vec<String>,
    static_metadata: Map<String, Value>,
    process_details: Map<String, Value>,
    last_discovery: f64,
    ambiguous: bool,
}

impl Monitor {
    fn clear_path(&mut self) {
        self.path = None;
        self.header.clear();
        self.static_metadata.clear();
        self.process_details.clear();
    }

    fn cache_path(&mut self, path: PathBuf, process_owned: bool) -> bool {
        let header = header_for_log(&path);
        if header.is_empty() {
            return false;
        }
        let mut metadata = metadata_for_log(&path);
        metadata.insert("mangoHudVersion".into(), json!(mangohud_version(&path)));
        let owners = if process_owned { Vec::new() } else { log_owner_pids(&path) };
        self.process_details = match owners.first() {
            Some(&owner) => process_details(owner),
            None if self.pid > 0 => process_details(self.pid as i32),
            None => Map::new(),
        };
        self.path = Some(path);
        self.header = header;
        self.static_metadata = metadata;
        true
    }

    fn discover(&mut self, now: f64) {
        self.last_discovery = now;
        self.ambiguous = false;
        if let Some(file) = self.log_file.clone().filter(|f| !f.is_empty()) {
            self.cache_path(PathBuf::from(file), false);
            return;
        }
        let newest = open_log_files(self.pid as i32)
            .into_iter()
            .filter_map(|p| mtime(&p).map(|m| (p, m)))
            .fold(None::<(PathBuf, f64)>, |best, (p, m)| if best.as_ref().is_none_or(|(_, b)| m > *b) { Some((p, m)) } else { best });
        if let Some((path, _)) = newest {
            if self.cache_path(path, true) {
                return;
            }
        }
        if !self.allow_directory_fallback || !is_xwayland_process(self.pid as i32) {
            return;
        }
        let mut candidates = Vec::new();
        for path in self.log_dirs.iter().flat_map(|d| directory_log_files(d)) {
            push_unique(&mut candidates, path);
        }
        let valid: Vec<PathBuf> = candidates
            .into_iter()
            .filter(|c| {
                let stem: String = c
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default()
                    .chars()
                    .filter(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
                    .collect();
                self.identity.iter().any(|t| stem.contains(t.as_str()))
            })
            .filter(|c| mtime(c).is_some_and(|m| (0.0..=MAX_FRESH_LOG_AGE_MS).contains(&((now - m) * 1000.0))))
            .filter(|c| !header_for_log(c).is_empty())
            .collect();
        match valid.len() {
            1 => {
                self.cache_path(valid[0].clone(), false);
            }
            0 => {}
            _ => self.ambiguous = true,
        }
    }

    fn snapshot(&mut self) -> Map<String, Value> {
        let now = now();
        if self.path.is_none() && now - self.last_discovery >= self.discovery_interval {
            self.discover(now);
        }
        let mut payload = Map::new();
        payload.insert("pid".into(), json!(self.pid));
        payload.insert("targetKey".into(), json!(self.target_key));
        payload.insert("available".into(), json!(false));
        payload.insert("logAgeMs".into(), json!(-1));
        if self.ambiguous {
            payload.insert("ambiguous".into(), json!(true));
        }
        let Some(path) = self.path.clone() else { return payload };
        payload.extend(read_log(&path, &self.header, &self.static_metadata));
        payload.extend(self.process_details.clone());
        let live = payload.get("available") == Some(&json!(true))
            && payload.get("logAgeMs").and_then(Value::as_i64).is_some_and(|a| a as f64 <= MAX_FRESH_LOG_AGE_MS);
        if !live {
            self.clear_path();
        }
        payload
    }
}

pub fn run_cli(args: &[String]) -> i32 {
    let (mut pid, mut target_key, mut app_id, mut name, mut log_file) = (None, String::new(), String::new(), String::new(), None);
    let (mut allow_dir, mut allow_title, mut watch, mut log_dirs) = (false, false, false, Vec::new());
    let (mut interval_ms, mut discovery_ms) = (500i64, 2000i64);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = || inline.clone().or_else(|| it.next().cloned()).unwrap_or_default();
        let int = |v: String| v.trim().parse::<i64>().ok();
        match flag {
            "--pid" => pid = int(value()),
            "--target-key" => target_key = value(),
            "--target-app-id" => app_id = value(),
            "--target-name" => name = value(),
            "--log-file" => log_file = Some(value()),
            "--log-dir" => log_dirs.push(value()),
            "--interval-ms" => interval_ms = int(value()).unwrap_or(interval_ms),
            "--discovery-interval-ms" => discovery_ms = int(value()).unwrap_or(discovery_ms),
            "--allow-directory-fallback" => allow_dir = true,
            "--allow-title-match" => allow_title = true,
            "--watch" => watch = true,
            _ => {
                eprintln!("game-performance-monitor: error: unrecognized arguments: {arg}");
                return 2;
            }
        }
    }
    let Some(pid) = pid else {
        eprintln!("game-performance-monitor: error: the following arguments are required: --pid");
        return 2;
    };
    let mut monitor = Monitor {
        pid,
        target_key,
        identity: identity_tokens(&[app_id.as_str(), if allow_title { name.as_str() } else { "" }]),
        allow_directory_fallback: allow_dir,
        log_file,
        log_dirs,
        discovery_interval: (discovery_ms as f64 / 1000.0).max(0.1),
        path: None,
        header: Vec::new(),
        static_metadata: Map::new(),
        process_details: Map::new(),
        last_discovery: 0.0,
        ambiguous: false,
    };
    let interval = (interval_ms as f64 / 1000.0).max(0.05);
    let stdout = std::io::stdout();
    loop {
        let started = Instant::now();
        let line = Value::Object(monitor.snapshot());
        let mut out = stdout.lock();
        if writeln!(out, "{line}").and_then(|_| out.flush()).is_err() {
            return 0;
        }
        drop(out);
        if !watch {
            return 0;
        }
        let left = interval - started.elapsed().as_secs_f64();
        if left > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(left));
        }
    }
}
