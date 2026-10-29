// This code was written by: Hikari.
//! Native Rust replacements for runtime helpers:
//! - `scripts/cava/extract_cover_colors.py`
//! - `scripts/cava/resolve_audio_source.py`
//! - `scripts/clipboard-store.py`
//! - Direct Unix domain socket IPC client for Quickshell (`qs -c inir ipc call ...`)

use crate::washi;
use image::imageops::FilterType;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

// ---------------------------------------------------------------------------
// 1. Cava Album Cover Color Extractor (`extract_cover_colors.py`)
// ---------------------------------------------------------------------------

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let rf = r as f64 / 255.0;
    let gf = g as f64 / 255.0;
    let bf = b as f64 / 255.0;
    let max_c = rf.max(gf).max(bf);
    let min_c = rf.min(gf).min(bf);
    let v = max_c;
    if (max_c - min_c).abs() < 1e-9 {
        return (0.0, 0.0, v);
    }
    let s = (max_c - min_c) / max_c;
    let rc = (max_c - rf) / (max_c - min_c);
    let gc = (max_c - gf) / (max_c - min_c);
    let bc = (max_c - bf) / (max_c - min_c);
    let mut h = if (rf - max_c).abs() < 1e-9 {
        bc - gc
    } else if (gf - max_c).abs() < 1e-9 {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    h = (h / 6.0).rem_euclid(1.0);
    (h, s, v)
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    if s <= 0.0 {
        let c = (v * 255.0) as u8;
        return (c, c, c);
    }
    let i = (h * 6.0).floor();
    let f = (h * 6.0) - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match (i as i32).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn saturate_rgb(r: u8, g: u8, b: u8, factor: f64) -> (u8, u8, u8) {
    let (h, s, v) = rgb_to_hsv(r, g, b);
    let s2 = (s * factor).min(1.0);
    let v2 = (v * 1.08).min(1.0);
    hsv_to_rgb(h, s2, v2)
}

pub fn run_extract_cover_colors_cli(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: extract_cover_colors.py <image_path> [count] [output_path]".to_string());
    }
    let image_path = &args[0];
    let count = args
        .get(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(8)
        .clamp(1, 8);
    let state_dir = env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_default();
        format!("{home}/.local/state")
    });
    let default_out = format!("{state_dir}/quickshell/user/generated/cover-colors.json");
    let output_path = args.get(2).map(String::as_str).unwrap_or(&default_out);

    if !Path::new(image_path).is_file() {
        return Err(format!("Image not found: {image_path}"));
    }

    let img = image::open(image_path)
        .map_err(|e| format!("Error: {e}"))?
        .resize_exact(150, 150, FilterType::Lanczos3)
        .to_rgb8();

    // Quantize pixels into 5-bit per channel buckets (32x32x32) and rank by frequency
    let mut bucket_counts: HashMap<u16, (u32, u64, u64, u64)> = HashMap::new();
    for px in img.pixels() {
        let [r, g, b] = px.0;
        let key = (((r >> 3) as u16) << 10) | (((g >> 3) as u16) << 5) | ((b >> 3) as u16);
        let entry = bucket_counts.entry(key).or_insert((0, 0, 0, 0));
        entry.0 += 1;
        entry.1 += r as u64;
        entry.2 += g as u64;
        entry.3 += b as u64;
    }

    let mut sorted_buckets: Vec<(u32, (u8, u8, u8))> = bucket_counts
        .into_values()
        .map(|(cnt, sr, sg, sb)| {
            (
                cnt,
                (
                    (sr / cnt as u64) as u8,
                    (sg / cnt as u64) as u8,
                    (sb / cnt as u64) as u8,
                ),
            )
        })
        .collect();
    sorted_buckets.sort_by(|a, b| b.0.cmp(&a.0));

    let mut chosen: Vec<String> = Vec::new();
    for (min_b, max_b) in [(20.0, 240.0), (10.0, 250.0), (0.0, 255.0)] {
        let mut ranked = Vec::new();
        let mut seen = BTreeSet::new();
        for &(_freq, (r, g, b)) in &sorted_buckets {
            let bness = (r as f64 * 299.0 + g as f64 * 587.0 + b as f64 * 114.0) / 1000.0;
            if bness < min_b || bness > max_b {
                continue;
            }
            let (sr, sg, sb) = saturate_rgb(r, g, b, 1.35);
            let hex = format!("#{:02x}{:02x}{:02x}", sr, sg, sb);
            if seen.insert(hex.clone()) {
                ranked.push(hex);
                if ranked.len() >= count {
                    break;
                }
            }
        }
        if ranked.len() >= count.min(2) {
            chosen = ranked;
            break;
        }
    }

    if chosen.is_empty() {
        return Err("Not enough distinct colors found".to_string());
    }

    let json_str = serde_json::to_string(&chosen).unwrap_or_else(|_| "[]".to_string());
    if let Some(parent) = Path::new(output_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(output_path, &json_str).map_err(|e| e.to_string())?;
    println!("{json_str}");
    Ok(())
}

// ---------------------------------------------------------------------------
// 2. Cava PipeWire/PulseAudio Source Resolver (`resolve_audio_source.py`)
// ---------------------------------------------------------------------------

const EXCLUDED_APP_NAMES: &[&str] = &[
    "quickshell",
    "discord",
    "vesktop",
    "webrtc",
    "teams",
    "zoom",
    "slack",
    "telegram-desktop",
    "element",
    "speech-dispatcher",
    "speech-dispatcher-dummy",
    "steam voice",
];

const EXCLUDED_BINARIES: &[&str] = &[
    "quickshell",
    "discord",
    "vesktop",
    "teams",
    "zoom",
    "slack",
    "sd_dummy",
];

const EXCLUDED_MEDIA_ROLES: &[&str] = &[
    "communication",
    "phone",
    "notification",
    "alert",
    "game",
    "production",
];

const PREFERRED_PLAYBACK_TOKENS: &[&str] = &[
    "spotify",
    "chromium",
    "chrome",
    "brave",
    "firefox",
    "zen",
    "mpv",
    "vlc",
    "strawberry",
    "audacious",
    "rhythmbox",
    "clementine",
    "haruna",
    "youtube music",
    "youtube-music",
    "youtube_music",
    "ytmusic",
    "ncspot",
];

#[derive(Clone, Debug)]
struct PulseClient {
    app_name: String,
    binary: String,
}

#[derive(Clone, Debug)]
struct SinkInput {
    index: i32,
    node_name: String,
    media_role: String,
    media_name: String,
    app_name: String,
    app_id: String,
    binary: String,
    corked: bool,
    sink_id: String,
}

fn run_cmd(cmd: &[&str]) -> String {
    Command::new(cmd[0])
        .args(&cmd[1..])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn parse_clients(text: &str) -> HashMap<String, PulseClient> {
    let mut clients = HashMap::new();
    let mut client_id = String::new();
    let mut block: HashMap<String, String> = HashMap::new();

    let flush = |cid: &mut String, blk: &mut HashMap<String, String>, out: &mut HashMap<String, PulseClient>| {
        if !cid.is_empty() {
            out.insert(
                cid.clone(),
                PulseClient {
                    app_name: blk.get("application.name").cloned().unwrap_or_default(),
                    binary: blk.get("application.process.binary").cloned().unwrap_or_default(),
                },
            );
            cid.clear();
            blk.clear();
        }
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(num) = trimmed.strip_prefix("Client #") {
            if num.chars().all(|c| c.is_ascii_digit()) {
                flush(&mut client_id, &mut block, &mut clients);
                client_id = num.to_string();
                continue;
            }
        }
        if !client_id.is_empty() {
            if let Some((k, v)) = trimmed.split_once(" = \"") {
                if let Some(val) = v.strip_suffix('"') {
                    block.insert(k.to_string(), val.to_string());
                }
            }
        }
    }
    flush(&mut client_id, &mut block, &mut clients);
    clients
}

fn parse_sink_inputs(text: &str, clients: &HashMap<String, PulseClient>) -> Vec<SinkInput> {
    let mut streams = Vec::new();
    let mut sink_idx = String::new();
    let mut block: HashMap<String, String> = HashMap::new();

    let flush = |sidx: &mut String,
                 blk: &mut HashMap<String, String>,
                 out: &mut Vec<SinkInput>| {
        if sidx.is_empty() {
            blk.clear();
            return;
        }
        let node_name = blk.get("node.name").cloned().unwrap_or_default();
        if !node_name.is_empty() {
            let cid = blk
                .get("client.id")
                .or_else(|| blk.get("Client"))
                .cloned()
                .unwrap_or_default();
            let client = clients.get(&cid);
            let app_name = blk
                .get("application.name")
                .cloned()
                .or_else(|| client.map(|c| c.app_name.clone()))
                .unwrap_or_default()
                .to_lowercase();
            let binary = blk
                .get("application.process.binary")
                .cloned()
                .or_else(|| client.map(|c| c.binary.clone()))
                .unwrap_or_default()
                .to_lowercase();
            let corked_raw = blk
                .get("Corked")
                .or_else(|| blk.get("pulse.corked"))
                .map(|s| s.to_lowercase())
                .unwrap_or_else(|| "false".to_string());
            let corked = matches!(corked_raw.as_str(), "yes" | "true" | "1");
            out.push(SinkInput {
                index: sidx.parse::<i32>().unwrap_or(0),
                node_name,
                media_role: blk.get("media.role").cloned().unwrap_or_default().to_lowercase(),
                media_name: blk.get("media.name").cloned().unwrap_or_default().to_lowercase(),
                app_name,
                app_id: blk.get("application.id").cloned().unwrap_or_default().to_lowercase(),
                binary,
                corked,
                sink_id: blk.get("Sink").cloned().unwrap_or_default(),
            });
        }
        sidx.clear();
        blk.clear();
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(num) = trimmed.strip_prefix("Sink Input #") {
            if num.chars().all(|c| c.is_ascii_digit()) {
                flush(&mut sink_idx, &mut block, &mut streams);
                sink_idx = num.to_string();
                continue;
            }
        }
        if sink_idx.is_empty() {
            continue;
        }
        if let Some(corked_val) = trimmed.strip_prefix("Corked:") {
            block.insert("Corked".to_string(), corked_val.trim().to_string());
        } else if let Some(client_val) = trimmed.strip_prefix("Client:") {
            block.insert("Client".to_string(), client_val.trim().to_string());
        } else if let Some(sink_val) = trimmed.strip_prefix("Sink:") {
            block.insert("Sink".to_string(), sink_val.trim().to_string());
        } else if let Some((k, v)) = trimmed.split_once(" = \"") {
            if let Some(val) = v.strip_suffix('"') {
                block.insert(k.to_string(), val.to_string());
            }
        }
    }
    flush(&mut sink_idx, &mut block, &mut streams);
    streams
}

fn parse_sink_monitors(text: &str) -> HashMap<String, String> {
    let mut monitors = HashMap::new();
    let mut sink_id = String::new();
    let mut monitor = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(num) = trimmed.strip_prefix("Sink #") {
            if num.chars().all(|c| c.is_ascii_digit()) {
                if !sink_id.is_empty() && !monitor.is_empty() {
                    monitors.insert(sink_id.clone(), monitor.clone());
                }
                sink_id = num.to_string();
                monitor.clear();
                continue;
            }
        }
        if !sink_id.is_empty() {
            if let Some(m) = trimmed.strip_prefix("Monitor Source:") {
                monitor = m.trim().to_string();
            }
        }
    }
    if !sink_id.is_empty() && !monitor.is_empty() {
        monitors.insert(sink_id, monitor);
    }
    monitors
}

fn desktop_entry_binaries() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("spotify", &["spotify"]),
        ("mpv", &["mpv"]),
        ("vlc", &["vlc"]),
        ("firefox", &["firefox", "zen"]),
        (
            "chromium",
            &[
                "chromium",
                "chrome",
                "brave",
                "google-chrome",
                "google-chrome-stable",
            ],
        ),
        (
            "chrome",
            &["chrome", "google-chrome", "google-chrome-stable", "brave"],
        ),
        ("brave", &["brave"]),
        ("org.chromium.Chromium", &["chromium", "chrome"]),
        (
            "com.google.Chrome",
            &["chrome", "google-chrome", "google-chrome-stable"],
        ),
        ("org.mozilla.firefox", &["firefox"]),
        ("strawberry", &["strawberry"]),
        ("audacious", &["audacious"]),
        ("deadbeef", &["deadbeef"]),
        ("rhythmbox", &["rhythmbox"]),
        ("clementine", &["clementine"]),
        ("haruna", &["haruna"]),
        ("com.github.thitzekai.Mooz", &["mooz"]),
        (
            "youtube-music",
            &["youtube-music", "youtube_music", "ytmusic", "mpv"],
        ),
        (
            "youtube_music",
            &["youtube-music", "youtube_music", "ytmusic", "mpv"],
        ),
        (
            "ytmusic",
            &["youtube-music", "youtube_music", "ytmusic", "mpv"],
        ),
        ("ncspot", &["ncspot"]),
        (
            "com.github.th_ch.youtube_music",
            &["youtube-music", "youtube_music", "ytmusic"],
        ),
    ]
}

fn hint_binaries(desktop_entry: &str) -> BTreeSet<String> {
    let entry = desktop_entry.trim().to_lowercase();
    if entry.is_empty() || entry == "__inir_music_player__" {
        return BTreeSet::new();
    }
    let mut hints = BTreeSet::new();
    for (key, bins) in desktop_entry_binaries() {
        let kl = key.to_lowercase();
        if kl.contains(&entry) || entry.contains(&kl) {
            for b in bins {
                hints.insert((*b).to_string());
            }
        }
    }
    if let Some(base) = entry.rsplit('.').next() {
        if !base.is_empty() && base != "desktop" && base != "client" {
            hints.insert(base.to_string());
        }
    }
    if entry.contains("chrom") || entry.contains("brave") {
        for b in [
            "chromium",
            "chrome",
            "google-chrome",
            "google-chrome-stable",
            "brave",
            "electron",
        ] {
            hints.insert(b.to_string());
        }
    }
    if entry.contains("firefox") || entry.contains("zen") {
        hints.insert("firefox".to_string());
        hints.insert("zen".to_string());
    }
    if entry.contains("youtube") || entry.contains("ytmusic") || entry.contains("yt-music") {
        for b in ["youtube-music", "youtube_music", "ytmusic", "mpv", "electron"] {
            hints.insert(b.to_string());
        }
    }
    hints
}

fn normalize_identity(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn matches_blocked(stream: &SinkInput, blocked_apps: &[String]) -> bool {
    if blocked_apps.is_empty() {
        return false;
    }
    let mut tokens = BTreeSet::new();
    let mut aliases = BTreeSet::new();
    for app in blocked_apps {
        let norm = normalize_identity(app);
        if !norm.is_empty() {
            tokens.insert(norm);
        }
        let mut entry = app.trim().to_lowercase();
        if let Some(stripped) = entry.strip_suffix(".desktop") {
            entry = stripped.to_string();
        }
        if entry.contains('.') {
            for (k, bins) in desktop_entry_binaries() {
                if k.to_lowercase() == entry {
                    for b in bins {
                        aliases.insert((*b).to_string());
                    }
                    break;
                }
            }
        }
    }

    let identity = format!(
        "{} {} {} {} {}",
        stream.node_name, stream.media_name, stream.app_name, stream.app_id, stream.binary
    )
    .to_lowercase();
    let norm_identity = normalize_identity(&identity);
    if tokens.iter().any(|t| norm_identity.contains(t)) {
        return true;
    }
    let norm_fields: BTreeSet<String> = [
        &stream.node_name,
        &stream.app_name,
        &stream.app_id,
        &stream.binary,
    ]
    .into_iter()
    .filter(|v| !v.is_empty())
    .map(|v| normalize_identity(v))
    .collect();

    aliases
        .iter()
        .any(|a| !a.is_empty() && norm_fields.contains(&normalize_identity(a)))
}

fn is_excluded(stream: &SinkInput) -> bool {
    if EXCLUDED_APP_NAMES
        .iter()
        .any(|t| stream.app_name.contains(&t.to_lowercase()))
    {
        return true;
    }
    if EXCLUDED_BINARIES.contains(&stream.binary.as_str()) {
        return true;
    }
    if EXCLUDED_MEDIA_ROLES.contains(&stream.media_role.as_str()) {
        return true;
    }
    if stream.app_name.contains("voice") && stream.app_name.contains("steam") {
        return true;
    }
    false
}

fn score_stream(stream: &SinkInput, hints: &BTreeSet<String>, blocked_apps: &[String]) -> i32 {
    if is_excluded(stream) {
        return -10_000;
    }
    if stream.corked {
        return -5_000;
    }
    if matches_blocked(stream, blocked_apps) {
        return -10_000;
    }
    let mut score = 180;
    if stream.media_role == "music" {
        score += 120;
    } else if stream.media_role == "video" || stream.media_role == "multimedia" {
        score += 50;
    } else if !stream.media_role.is_empty() {
        score -= 20;
    }

    let node_low = stream.node_name.to_lowercase();
    let identity = format!(
        "{} {} {} {} {}",
        node_low, stream.media_name, stream.app_name, stream.app_id, stream.binary
    );
    let hint_match = hints.is_empty()
        || hints.contains(&stream.binary)
        || hints.contains(&node_low)
        || hints.contains(&stream.app_id)
        || hints.iter().any(|h| identity.contains(h));
    if !hint_match {
        return -1_000;
    }
    if !hints.is_empty() {
        if hints.contains(&stream.binary) {
            score += 500;
        }
        if hints.contains(&node_low) || hints.contains(&stream.app_id) {
            score += 420;
        }
        if hints.iter().any(|h| identity.contains(h)) {
            score += 260;
        }
    }
    if PREFERRED_PLAYBACK_TOKENS
        .iter()
        .any(|t| identity.contains(t))
    {
        score += 40;
    }
    score
}

pub fn run_resolve_audio_source_cli(args: &[String]) -> i32 {
    let mut desktop_entry = String::new();
    let mut blocked_json = "[]".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--desktop-entry" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    desktop_entry = v.clone();
                }
            }
            "--blocked-apps-json" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    blocked_json = v.clone();
                }
            }
            _ => {}
        }
        i += 1;
    }

    let blocked_apps: Vec<String> = serde_json::from_str::<Vec<Value>>(&blocked_json)
        .ok()
        .map(|arr| {
            arr.into_iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let server_info = run_cmd(&["pactl", "info"]);
    if server_info.is_empty() {
        if !blocked_apps.is_empty() {
            return 3;
        }
        println!("auto");
        return 0;
    }

    let clients = parse_clients(&run_cmd(&["pactl", "list", "clients"]));
    let streams = parse_sink_inputs(&run_cmd(&["pactl", "list", "sink-inputs"]), &clients);
    let sink_monitors = parse_sink_monitors(&run_cmd(&["pactl", "list", "sinks"]));
    let hints = hint_binaries(&desktop_entry);

    let mut ranked: Vec<(i32, &SinkInput)> = streams
        .iter()
        .map(|s| (score_stream(s, &hints, &blocked_apps), s))
        .collect();
    ranked.sort_by(|a, b| (b.0, b.1.index).cmp(&(a.0, a.1.index)));

    for (score, stream) in &ranked {
        if *score > 0 && !stream.node_name.is_empty() {
            if let Some(mon) = sink_monitors.get(&stream.sink_id) {
                if !mon.is_empty() {
                    println!("{mon}");
                    return 0;
                }
            }
        }
    }

    if !blocked_apps.is_empty() && !hints.is_empty() {
        let empty_hints = BTreeSet::new();
        let mut fallback: Vec<(i32, &SinkInput)> = streams
            .iter()
            .map(|s| (score_stream(s, &empty_hints, &blocked_apps), s))
            .collect();
        fallback.sort_by(|a, b| (b.0, b.1.index).cmp(&(a.0, a.1.index)));
        for (score, stream) in &fallback {
            if *score > 0 && !stream.node_name.is_empty() {
                if let Some(mon) = sink_monitors.get(&stream.sink_id) {
                    if !mon.is_empty() {
                        println!("{mon}");
                        return 0;
                    }
                }
            }
        }
    }

    if !streams.is_empty() || !blocked_apps.is_empty() {
        if !blocked_apps.is_empty() {
            return 3;
        }
        println!();
        return 0;
    }

    let sink = run_cmd(&["pactl", "get-default-sink"]).trim().to_string();
    if !sink.is_empty() {
        println!("{sink}.monitor");
    } else {
        println!("auto");
    }
    0
}

// ---------------------------------------------------------------------------
// 3. Clipboard HTML Sanitizer (`clipboard-store.py`)
// ---------------------------------------------------------------------------

const HTML_PAYLOAD_PREFIX: &[u8] = b"<meta http-equiv=\"content-type\" content=\"text/html";
const HTML_FRAGMENT_MARKER: &[u8] = b"<!--StartFragment-->";

pub fn is_browser_markup(payload: &[u8]) -> bool {
    payload.starts_with(HTML_PAYLOAD_PREFIX)
        || payload
            .windows(HTML_FRAGMENT_MARKER.len())
            .any(|w| w == HTML_FRAGMENT_MARKER)
}

fn unescape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        if let Some(semi) = rest[..rest.len().min(12)].find(';') {
            let entity = &rest[1..semi];
            let decoded = match entity {
                "amp" => Some("&".to_string()),
                "lt" => Some("<".to_string()),
                "gt" => Some(">".to_string()),
                "quot" => Some("\"".to_string()),
                "apos" | "#39" | "#x27" => Some("'".to_string()),
                "nbsp" | "#160" | "#xa0" => Some("\u{00a0}".to_string()),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                        .map(|c| c.to_string())
                }
                _ if entity.starts_with('#') => entity[1..]
                    .parse::<u32>()
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string()),
                _ => None,
            };
            if let Some(d) = decoded {
                out.push_str(&d);
                rest = &rest[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

pub fn strip_browser_markup(markup: &str) -> String {
    let mut s = markup.to_string();
    if s.starts_with("<meta") {
        if let Some(gt) = s.find('>') {
            s = s[gt + 1..].to_string();
        }
    }

    // <script>/<style> blocks before comments, as clipboard-store.py did; `<scripts>` is not one
    for tag in ["script", "style"] {
        let open_pat = format!("<{tag}");
        let close_pat = format!("</{tag}>");
        let mut from = 0;
        loop {
            let low = s.to_ascii_lowercase();
            let Some(rel) = low[from..].find(&open_pat) else { break };
            let start = from + rel;
            let next = low[start + open_pat.len()..].chars().next();
            if next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                from = start + open_pat.len();
                continue;
            }
            let Some(end) = low[start..].find(&close_pat) else { break };
            s.replace_range(start..start + end + close_pat.len(), "");
            from = start;
        }
    }

    // Remove <!-- ... --> comments
    while let Some(start) = s.find("<!--") {
        if let Some(end) = s[start + 4..].find("-->") {
            s.replace_range(start..start + 4 + end + 3, "");
        } else {
            break;
        }
    }

    // Parse remaining tags, converting <br>, closing block tags, and <img src="...">
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(rel_end) = s[i..].find('>') {
                let tag_body = s[i + 1..i + rel_end].trim();
                let tag_low = tag_body.to_ascii_lowercase();
                if tag_low == "br" || tag_low.starts_with("br ") || tag_low.starts_with("br/") {
                    out.push('\n');
                } else if matches!(
                    tag_low.as_str(),
                    "/p" | "/div"
                        | "/li"
                        | "/tr"
                        | "/h1"
                        | "/h2"
                        | "/h3"
                        | "/h4"
                        | "/h5"
                        | "/h6"
                        | "/blockquote"
                        | "/pre"
                ) {
                    out.push('\n');
                } else if tag_low.starts_with("img") {
                    if let Some(src_pos) = tag_low.find("src=") {
                        let after = &tag_body[src_pos + 4..].trim_start();
                        if let Some(q) = after.chars().next() {
                            if q == '"' || q == '\'' {
                                if let Some(end_q) = after[1..].find(q) {
                                    out.push_str(&after[1..1 + end_q]);
                                }
                            }
                        }
                    }
                }
                i += rel_end + 1;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    let unescaped = unescape_html(&out);
    let mut lines_cleaned = String::with_capacity(unescaped.len());
    for (idx, line) in unescaped.split('\n').enumerate() {
        if idx > 0 {
            lines_cleaned.push('\n');
        }
        lines_cleaned.push_str(line.trim_end_matches([' ', '\t']));
    }

    // Collapse 3+ newlines into 2
    let mut collapsed = String::with_capacity(lines_cleaned.len());
    let mut nl_run = 0usize;
    for ch in lines_cleaned.chars() {
        if ch == '\n' {
            nl_run += 1;
            if nl_run <= 2 {
                collapsed.push(ch);
            }
        } else {
            nl_run = 0;
            collapsed.push(ch);
        }
    }
    collapsed.trim().to_string()
}

pub fn run_clipboard_store_cli(args: &[String]) -> i32 {
    let mut payload = Vec::new();
    if io::stdin().read_to_end(&mut payload).is_err() {
        return 1;
    }

    if is_browser_markup(&payload) {
        let text = strip_browser_markup(&String::from_utf8_lossy(&payload));
        if text.is_empty() {
            return 0;
        }
        payload = text.into_bytes();
    }

    if args.iter().any(|a| a == "--filter") {
        let _ = io::stdout().write_all(&payload);
        return 0;
    }

    let mut child = match Command::new("cliphist")
        .arg("store")
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return 1,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&payload);
    }
    child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
}

// ---------------------------------------------------------------------------
// 4. Claude Code Theme Generator (`claude_code_theme.py`)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct HctApprox {
    h: f64,
    c: f64,
    t: f64,
}

fn hct_hex(hue: f64, chroma: f64, tone: f64) -> String {
    let lch = washi::LCH {
        l: (tone / 100.0).clamp(0.0, 1.0),
        c: (chroma.max(0.0) / 300.0).clamp(0.0, 0.35),
        h: hue.rem_euclid(360.0),
    };
    lch.rgb().hex().to_uppercase()
}

fn to_hct(hex: &str) -> HctApprox {
    let rgb = washi::parse_hex(hex).unwrap_or(washi::RGB { r: 0.0, g: 0.0, b: 0.0 });
    let lch = rgb.lch();
    HctApprox {
        h: lch.h,
        c: lch.c * 300.0,
        t: lch.l * 100.0,
    }
}

fn retone(hex: &str, tone: f64, chroma_scale: f64, min_chroma: f64) -> String {
    let h = to_hct(hex);
    hct_hex(h.h, (h.c * chroma_scale).max(min_chroma), tone)
}

fn ansi_name(slot: usize) -> String {
    const ANSI: [&str; 8] = [
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    ];
    if slot < 8 {
        format!("ansi:{}", ANSI[slot])
    } else {
        format!("ansi:{}Bright", ANSI[slot - 8])
    }
}

pub fn run_claude_code_theme_cli(args: &[String]) -> Result<(), String> {
    let mut terminal_path = String::new();
    let mut palette_path = String::new();
    let mut mode = "dark".to_string();
    let mut out_dir = String::new();
    let mut print_only = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--terminal" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    terminal_path = v.clone();
                }
            }
            "--palette" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    palette_path = v.clone();
                }
            }
            "--mode" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    mode = v.clone();
                }
            }
            "--out-dir" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    out_dir = v.clone();
                }
            }
            "--print" => {
                print_only = true;
            }
            _ => {}
        }
        i += 1;
    }

    if terminal_path.is_empty() || palette_path.is_empty() || out_dir.is_empty() {
        return Err("Missing required arguments (--terminal, --palette, --out-dir)".to_string());
    }

    let term: BTreeMap<String, String> = serde_json::from_str(
        &fs::read_to_string(&terminal_path).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let palette_raw: Value = serde_json::from_str(
        &fs::read_to_string(&palette_path).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut palette = BTreeMap::new();
    if let Some(obj) = palette_raw.as_object() {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                palette.insert(k.clone(), s.to_string());
            }
        }
    }

    let build_dynamic = |name: &str, soft: bool| -> Value {
        let dark = mode != "light";
        let t0_hex = term.get("term0").map(String::as_str).unwrap_or("#1D2021");
        let t15_hex = term.get("term15").map(String::as_str).unwrap_or("#EBDBB2");
        let bg = to_hct(t0_hex);
        let t0 = bg.t;
        let sign = if dark { 1.0 } else { -1.0 };
        let primary = to_hct(palette.get("primary").map(String::as_str).unwrap_or("#7AA2F7"));
        let ph = primary.h;

        let accent = hct_hex(
            ph,
            if soft {
                30.0
            } else {
                primary.c.clamp(36.0, 60.0)
            },
            if soft {
                if dark { 70.0 } else { 46.0 }
            } else if dark {
                74.0
            } else {
                42.0
            },
        );
        let accent_hi = retone(&accent, if dark { 86.0 } else { 55.0 }, 1.0, 0.0);

        let slots: &[(&str, usize)] = if dark {
            &[
                ("success", 10),
                ("error", 9),
                ("warning", 11),
                ("permission", 12),
                ("suggestion", 12),
                ("remember", 12),
                ("planMode", 14),
                ("autoAccept", 13),
                ("skill", 13),
                ("bashBorder", 13),
                ("merged", 13),
                ("ide", 4),
            ]
        } else {
            &[
                ("success", 2),
                ("error", 1),
                ("warning", 3),
                ("permission", 4),
                ("suggestion", 4),
                ("remember", 4),
                ("planMode", 6),
                ("autoAccept", 5),
                ("skill", 5),
                ("bashBorder", 5),
                ("merged", 5),
                ("ide", 4),
            ]
        };

        let mut o = serde_json::Map::new();
        o.insert("text".into(), json!(t15_hex));
        o.insert("inverseText".into(), json!(t0_hex));
        o.insert("claude".into(), json!(accent));
        o.insert("claudeShimmer".into(), json!(accent_hi));
        o.insert("clawd_body".into(), json!(accent));
        o.insert("rate_limit_fill".into(), json!(accent));
        o.insert(
            "rate_limit_empty".into(),
            json!(hct_hex(bg.h, bg.c.min(14.0), t0 + sign * 12.0)),
        );
        o.insert(
            "promptBorder".into(),
            json!(hct_hex(
                ph,
                if soft { 10.0 } else { 16.0 },
                if dark { 48.0 } else { 56.0 }
            )),
        );
        o.insert(
            "promptBorderShimmer".into(),
            json!(hct_hex(
                ph,
                if soft { 14.0 } else { 22.0 },
                if dark { 62.0 } else { 44.0 }
            )),
        );
        o.insert(
            "subtle".into(),
            json!(hct_hex(bg.h, bg.c.min(8.0), if dark { 40.0 } else { 72.0 })),
        );
        o.insert("inactive".into(), json!("ansi:blackBright"));
        o.insert(
            "inactiveShimmer".into(),
            json!(if dark { "ansi:white" } else { "ansi:black" }),
        );
        o.insert("professionalBlue".into(), json!(ansi_name(4)));
        o.insert(
            "userMessageBackground".into(),
            json!(hct_hex(
                ph,
                if soft { 10.0 } else { 14.0 },
                t0 + sign * 11.0
            )),
        );
        o.insert(
            "userMessageBackgroundHover".into(),
            json!(hct_hex(
                ph,
                if soft { 12.0 } else { 16.0 },
                t0 + sign * 15.0
            )),
        );
        o.insert(
            "composerSidebarBackground".into(),
            json!(hct_hex(bg.h, bg.c.min(12.0), t0 + sign * 5.0)),
        );
        let t5_hue = to_hct(term.get("term5").map(String::as_str).unwrap_or("#B16286")).h;
        let t6_hue = to_hct(term.get("term6").map(String::as_str).unwrap_or("#689D6A")).h;
        o.insert(
            "bashMessageBackgroundColor".into(),
            json!(hct_hex(
                t5_hue,
                if dark { 14.0 } else { 10.0 },
                t0 + sign * 10.0
            )),
        );
        o.insert(
            "memoryBackgroundColor".into(),
            json!(hct_hex(
                t6_hue,
                if dark { 12.0 } else { 9.0 },
                t0 + sign * 10.0
            )),
        );
        o.insert(
            "selectionBg".into(),
            json!(hct_hex(
                ph,
                if dark { 24.0 } else { 18.0 },
                if dark { 34.0 } else { 84.0 }
            )),
        );

        let add_hue = to_hct(term.get("term2").map(String::as_str).unwrap_or("#98971A")).h;
        let del_hue = to_hct(term.get("term1").map(String::as_str).unwrap_or("#CC241D")).h;
        for (key, hue, strength) in [("Added", add_hue, 1.0), ("Removed", del_hue, 0.7)] {
            o.insert(
                format!("diff{key}"),
                json!(hct_hex(
                    hue,
                    (if dark { 18.0 } else { 14.0 }) * strength,
                    t0 + sign * 8.0
                )),
            );
            o.insert(
                format!("diff{key}Word"),
                json!(hct_hex(
                    hue,
                    (if dark { 30.0 } else { 24.0 }) * strength,
                    t0 + sign * 16.0
                )),
            );
            o.insert(
                format!("diff{key}Dimmed"),
                json!(hct_hex(hue, 8.0 * strength, t0 + sign * 5.0)),
            );
        }

        if soft {
            let fg = to_hct(t15_hex);
            let text = hct_hex(
                fg.h,
                fg.c,
                if dark { fg.t - 4.0 } else { fg.t + 4.0 },
            );
            o.insert("text".into(), json!(text));
            for &(role, s) in slots {
                let n = if s >= 9 { s - 8 } else { s };
                let tn = term
                    .get(&format!("term{n}"))
                    .map(String::as_str)
                    .unwrap_or("#888888");
                let retoned = retone(
                    tn,
                    to_hct(tn).t + if dark { 0.0 } else { 3.0 },
                    0.8,
                    0.0,
                );
                o.insert(role.to_string(), json!(retoned));
            }
            if let Some(ide_val) = o.get("ide").cloned() {
                o.insert("professionalBlue".into(), ide_val);
            }
            for (shim, role) in [("warningShimmer", "warning"), ("permissionShimmer", "permission")]
            {
                if let Some(r_str) = o.get(role).and_then(|v| v.as_str()) {
                    let sh = retone(r_str, if dark { 84.0 } else { 52.0 }, 1.0, 0.0);
                    o.insert(shim.to_string(), json!(sh));
                }
            }
        }

        json!({
            "name": name,
            "base": if dark { "dark-ansi" } else { "light-ansi" },
            "overrides": o,
        })
    };

    let files = [
        ("inir.json", build_dynamic("iNiR", false)),
        ("inir-soft.json", build_dynamic("iNiR Soft", true)),
    ];

    let out_path = Path::new(&out_dir);
    let mut changed = Vec::new();
    for (fname, val) in &files {
        let text = format!("{}\n", serde_json::to_string_pretty(val).unwrap_or_default());
        if print_only {
            println!("{fname} {text}");
            continue;
        }
        let _ = fs::create_dir_all(out_path);
        let fp = out_path.join(fname);
        if fp.exists() && fs::read_to_string(&fp).ok().as_deref() == Some(text.as_str()) {
            continue;
        }
        fs::write(&fp, &text).map_err(|e| e.to_string())?;
        changed.push(*fname);
    }
    if changed.is_empty() {
        println!(
            "{}",
            files
                .iter()
                .map(|(k, _)| *k)
                .collect::<Vec<_>>()
                .join("\n")
        );
    } else {
        println!("changed: {}", changed.join(" "));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 5. Direct Quickshell Unix Socket IPC Client (`inir-accel ipc ...`)
// ---------------------------------------------------------------------------

pub fn find_inir_ipc_socket() -> Option<PathBuf> {
    let runtime_dir = env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let uid = unsafe { libc::getuid() };
        format!("/run/user/{uid}")
    });
    let by_shell = Path::new(&runtime_dir).join("quickshell/by-shell/inir");
    if let Ok(entries) = fs::read_dir(&by_shell) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path().join("ipc.sock"))
            .filter(|p| p.exists())
            .collect();
        dirs.sort();
        if let Some(last) = dirs.pop() {
            return Some(last);
        }
    }
    let by_pid = Path::new(&runtime_dir).join("quickshell/by-pid");
    if let Ok(entries) = fs::read_dir(&by_pid) {
        for entry in entries.flatten() {
            let sock = entry.path().join("ipc.sock");
            if sock.exists() {
                return Some(sock);
            }
        }
    }
    None
}

pub fn check_ipc_socket_alive() -> Result<PathBuf, String> {
    let sock = find_inir_ipc_socket().ok_or_else(|| "No active iNiR IPC socket found".to_string())?;
    let stream = UnixStream::connect(&sock).map_err(|e| format!("Socket connect failed: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    Ok(sock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_markup_order_and_tag_boundary() {
        assert_eq!(strip_browser_markup("a<script>x<!--</script>-->b"), "a-->b");
        assert_eq!(strip_browser_markup("<scripts>keep</scripts>"), "keep");
        assert_eq!(strip_browser_markup("<STYLE>p{}</STYLE>ok"), "ok");
    }
}
