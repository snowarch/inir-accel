//! Synchronized lyrics from LRCLIB, ported from `lyrics/lyrics.py`: one JSON line with the timed lines.

use crate::net;
use crate::pyrt;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::LazyLock;
use std::thread::sleep;
use std::time::Duration;

const API: &str = "https://lrclib.net/api";
const TIMEOUT: f64 = 12.0;
const REQUEST_DELAY: Duration = Duration::from_millis(250);
const USER_AGENT: &str = concat!("inir-accel/", env!("CARGO_PKG_VERSION"), " (https://github.com/snowarch/inir)");

fn get_json(url: &str) -> Result<Value, String> {
    let headers = [("User-Agent", USER_AGENT)];
    let mut response = net::get(url, &headers, TIMEOUT)?;
    if response.status == 429 {
        sleep(Duration::from_secs_f64(response.retry_after.unwrap_or(1.0).max(0.0)));
        response = net::get(url, &headers, TIMEOUT)?;
    }
    if response.status >= 400 {
        return Err(format!("HTTP Error {}", response.status));
    }
    serde_json::from_slice(String::from_utf8_lossy(&response.body).as_bytes()).map_err(|e| e.to_string())
}

fn parse_lrc(lrc: &str) -> Vec<Value> {
    static STAMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[(\d{1,3}):(\d{2}(?:[.:]\d{1,3})?)\]").unwrap());
    let mut out: Vec<(f64, String)> = Vec::new();
    for raw in pyrt::splitlines(lrc) {
        let stamps: Vec<regex::Captures> = STAMP.captures_iter(raw).collect();
        let Some(last) = stamps.last() else { continue };
        let text = raw[last.get(0).unwrap().end()..].trim().to_string();
        for stamp in &stamps {
            let minutes: f64 = stamp[1].parse().unwrap_or(0.0);
            let seconds: f64 = stamp[2].replace(':', ".").parse().unwrap_or(0.0);
            out.push((((minutes * 60.0 + seconds) * 1000.0).round() / 1000.0, text.clone()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(t, text)| json!({"t": t, "text": text})).collect()
}

fn normalize(value: &str) -> String {
    static PUNCT: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"[^\w\s]"));
    PUNCT.replace_all(&value.to_lowercase(), " ").trim().to_string()
}

fn overlaps(wanted: &str, got: &str) -> bool {
    let (wanted, got) = (normalize(wanted), normalize(got));
    if wanted.is_empty() || got.is_empty() {
        return false;
    }
    if wanted.contains(&got) || got.contains(&wanted) {
        return true;
    }
    let got_words: HashSet<&str> = got.split_whitespace().collect();
    wanted.split_whitespace().any(|w| w.chars().count() > 3 && got_words.contains(w))
}

fn looks_like(candidate: &Value, title: &str, artist: &str) -> bool {
    let field = |k: &str| candidate.get(k).and_then(Value::as_str).unwrap_or_default();
    !field("syncedLyrics").is_empty() && overlaps(title, field("trackName")) && overlaps(artist, field("artistName"))
}

fn find_lyrics(title: &str, artist: &str, album: &str, duration: Option<f64>) -> Result<Vec<Value>, String> {
    let q = net::quote;
    let mut attempts = Vec::new();
    if let Some(d) = duration.filter(|d| *d != 0.0 && !album.is_empty()) {
        attempts.push(format!("{API}/get?track_name={}&artist_name={}&album_name={}&duration={}", q(title), q(artist), q(album), d.trunc() as i64));
    }
    attempts.push(format!("{API}/search?track_name={}&artist_name={}", q(title), q(artist)));
    attempts.push(format!("{API}/search?q={}", q(&format!("{title} {artist}"))));

    let (mut had_response, mut last_error) = (false, None);
    for (index, url) in attempts.iter().enumerate() {
        if index > 0 {
            sleep(REQUEST_DELAY);
        }
        let payload = match get_json(url) {
            Ok(p) => {
                had_response = true;
                p
            }
            Err(e) => {
                last_error = Some(e);
                continue;
            }
        };
        let candidates = match payload {
            Value::Array(list) => list,
            other => vec![other],
        };
        for candidate in candidates.iter().filter(|c| c.is_object() && looks_like(c, title, artist)) {
            let lines = parse_lrc(candidate.get("syncedLyrics").and_then(Value::as_str).unwrap_or_default());
            if !lines.is_empty() {
                return Ok(lines);
            }
        }
    }
    match last_error {
        Some(e) if !had_response => Err(e),
        _ => Ok(Vec::new()),
    }
}

/// `<title> <artist> [album] [duration] [request id]`
pub fn run_cli(args: &[String]) -> i32 {
    let arg = |i: usize| args.get(i).map(|a| a.trim().to_string()).unwrap_or_default();
    let (title, artist, album, request_id) = (arg(0), arg(1), arg(2), arg(4));
    let emit = |mut payload: Value| {
        if !request_id.is_empty() {
            payload["requestId"] = json!(request_id);
        }
        println!("{payload}");
    };
    if title.is_empty() || artist.is_empty() {
        emit(json!({"status": "no_info"}));
        return 0;
    }
    let duration = args.get(3).filter(|d| !d.is_empty()).and_then(|d| d.trim().parse::<f64>().ok()).filter(|d| d.is_finite());
    match find_lyrics(&title, &artist, &album, duration) {
        Err(message) => emit(json!({"status": "error", "message": message})),
        Ok(lines) if lines.is_empty() => emit(json!({"status": "not_found"})),
        Ok(lines) => emit(json!({"status": "ok", "lines": lines})),
    }
    0
}
