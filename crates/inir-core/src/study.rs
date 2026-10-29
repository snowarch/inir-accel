//! A small curated study-deck catalog, ported from `study-decks.py`. Deck bytes stay upstream: this finds the
//! latest GitHub release asset, downloads it into the person's Downloads folder and optionally opens it.

use crate::net;
use regex::RegexBuilder;
use serde_json::{Value, json};
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const USER_AGENT: &str = "iNiR-study-decks/1";

struct Deck {
    id: &'static str,
    title: &'static str,
    study: &'static str,
    base: &'static str,
    repo: &'static str,
    description: &'static str,
    license: &'static str,
    prefer: &'static [&'static str],
}

const UPSTREAM_TERMS: &str = "Downloaded from the upstream project release; deck content retains upstream terms.";
const CATALOG: [Deck; 3] = [
    Deck {
        id: "kaishi",
        title: "Kaishi 1.5k",
        study: "ja",
        base: "en",
        repo: "donkuri/Kaishi",
        description: "Beginner Japanese vocabulary with sentences, audio, furigana and pitch data.",
        license: UPSTREAM_TERMS,
        prefer: &[r"\.apkg$"],
    },
    Deck {
        id: "manabi",
        title: "Manabi 2.7k",
        study: "ja",
        base: "en",
        repo: "fafner8/Manabi",
        description: "Frequency-sorted i+1 Japanese vocabulary and sentences with audio and furigana.",
        license: UPSTREAM_TERMS,
        prefer: &[r"\.apkg$"],
    },
    Deck {
        id: "niponismo",
        title: "Niponismo",
        study: "ja",
        base: "es",
        repo: "andresangelini/niponismo",
        description: "Japanese grammar decks for Spanish speakers.",
        license: "Project is CC BY-SA 4.0; third-party example material keeps its credited licenses.",
        prefer: &[r"Niponismo.*\.apkg$", r"\.apkg$"],
    },
];

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn downloads_dir() -> PathBuf {
    let config = PathBuf::from(home()).join(".config/user-dirs.dirs");
    if let Ok(text) = fs::read(&config) {
        let text = String::from_utf8_lossy(&text);
        if let Some(dir) = text.lines().find_map(|l| l.strip_prefix("XDG_DOWNLOAD_DIR=\"")?.split('"').next()) {
            return PathBuf::from(crate::material::py::expanduser(&dir.replace("$HOME", &home())));
        }
    }
    PathBuf::from(home()).join("Downloads")
}

fn download(deck: &Deck, open_after: bool) -> Value {
    let releases = format!("https://github.com/{}/releases", deck.repo);
    let release: Value = match net::get(&format!("https://api.github.com/repos/{}/releases/latest", deck.repo), &[("User-Agent", USER_AGENT)], 15.0)
        .and_then(|r| if r.status >= 400 { Err(format!("HTTP Error {}", r.status)) } else { Ok(r) })
        .and_then(|r| serde_json::from_slice(&r.body).map_err(|e| e.to_string()))
    {
        Ok(v) => v,
        Err(e) => return json!({"ok": false, "error": format!("Could not query the latest upstream release: {e}"), "sourceUrl": releases}),
    };
    let source = release.get("html_url").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(&releases).to_string();
    let assets = release.get("assets").and_then(Value::as_array).cloned().unwrap_or_default();
    let asset = deck.prefer.iter().find_map(|pattern| {
        let rx = RegexBuilder::new(pattern).case_insensitive(true).build().unwrap();
        assets.iter().find(|a| rx.is_match(a.get("name").and_then(Value::as_str).unwrap_or_default())).cloned()
    });
    let Some(asset) = asset else {
        return json!({"ok": false, "error": "The latest upstream release has no compatible .apkg asset", "sourceUrl": source});
    };
    let url = asset.get("browser_download_url").and_then(Value::as_str).unwrap_or_default().to_string();
    let name = asset.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).map_or(format!("{}.apkg", deck.id), String::from);
    let dir = downloads_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        return json!({"ok": false, "error": e.to_string()});
    }
    let dest = dir.join(&name);
    let part = dir.join(format!("{name}.part"));
    if let Err(e) = net::download(&url, &[("User-Agent", USER_AGENT)], &part, 30.0).and_then(|_| fs::rename(&part, &dest).map_err(|e| e.to_string())) {
        let _ = fs::remove_file(&part);
        return json!({"ok": false, "error": format!("Deck download failed: {e}"), "sourceUrl": url});
    }
    if open_after {
        let _ = Command::new("xdg-open").arg(&dest).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn();
    }
    json!({"ok": true, "id": deck.id, "title": deck.title, "path": dest.to_string_lossy(), "asset": name, "sourceUrl": source})
}

/// `list` | `download <id> [--no-open]`
pub fn run_cli(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("list") => {
            let decks: Vec<Value> = CATALOG
                .iter()
                .map(|d| {
                    json!({"id": d.id, "title": d.title, "studyLanguage": d.study, "baseLanguage": d.base, "repo": d.repo,
                        "description": d.description, "licenseNote": d.license, "assetPrefer": d.prefer})
                })
                .collect();
            println!("{}", json!({"ok": true, "decks": decks}));
            0
        }
        Some("download") if args.len() >= 2 => {
            let open_after = !args[2..].iter().any(|a| a == "--no-open");
            let payload = match CATALOG.iter().find(|d| d.id == args[1]) {
                Some(deck) => download(deck, open_after),
                None => json!({"ok": false, "error": format!("Unknown study deck: {}", args[1])}),
            };
            println!("{payload}");
            if payload["ok"] == json!(true) { 0 } else { 1 }
        }
        _ => {
            eprintln!("usage: inir-accel study-decks {{list,download}} ...");
            2
        }
    }
}
