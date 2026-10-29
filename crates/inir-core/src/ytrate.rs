//! YouTube Music likes through the YouTube Data API v3 and the person's own OAuth client, ported from
//! `ytmusic_rate.py`. Prints one JSON object, or JSON lines closed by {"_done":true,"count":N} for fetch-liked.

use crate::net::{self, Body};
use regex::Regex;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API: &str = "https://www.googleapis.com/youtube/v3";

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn oauth_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(base).join("illogical-impulse/ytmusic_oauth.json")
}

fn exit_with(v: Value) -> ! {
    println!("{v}");
    std::process::exit(1)
}

fn load() -> Value {
    let Ok(text) = fs::read_to_string(oauth_path()) else { exit_with(json!({"error": "OAuth not configured"})) };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) if v.is_object() => v,
        _ => exit_with(json!({"error": "OAuth file is not a JSON object"})),
    }
}

fn save(oauth: &Value) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let path = oauth_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // born 600 so the token never sits readable, then 600 again for a file an older version made
    let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path)?;
    f.write_all(serde_json::to_string_pretty(oauth).unwrap_or_default().as_bytes())?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
}

fn form(fields: &[(&str, &str)]) -> String {
    let q = |s: &str| net::quote(s, " ").replace(' ', "+");
    fields.iter().map(|(k, v)| format!("{}={}", q(k), q(v))).collect::<Vec<_>>().join("&")
}

fn post_form(url: &str, fields: &[(&str, &str)], timeout: f64) -> Result<net::Response, String> {
    net::request(url, &[("Content-Type", "application/x-www-form-urlencoded")], Body::Text(&form(fields)), timeout)
}

fn field<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or_default()
}

fn body_text(r: &net::Response, n: usize) -> String {
    String::from_utf8_lossy(&r.body).chars().take(n).collect()
}

fn refresh(mut oauth: Value) -> Value {
    let fields = [("client_id", field(&oauth, "client_id")), ("client_secret", field(&oauth, "client_secret")), ("refresh_token", field(&oauth, "refresh_token")), ("grant_type", "refresh_token")];
    let result = post_form(TOKEN_URL, &fields, 10.0).and_then(|r| {
        if r.status >= 400 {
            return Err(format!("HTTP Error {}", r.status));
        }
        let v: Value = serde_json::from_slice(&r.body).map_err(|e| e.to_string())?;
        v["access_token"].as_str().map(|a| (a.to_string(), v["expires_in"].as_i64().unwrap_or(3600))).ok_or_else(|| "'access_token'".to_string())
    });
    match result {
        Ok((access, expires_in)) => {
            oauth["access_token"] = json!(access);
            oauth["expires_at"] = json!(now() + expires_in);
            if let Err(e) = save(&oauth) {
                exit_with(json!({"error": format!("Token refresh failed: {e}")}));
            }
            oauth
        }
        Err(e) => exit_with(json!({"error": format!("Token refresh failed: {e}")})),
    }
}

fn valid_token() -> Value {
    let oauth = load();
    if now() > oauth["expires_at"].as_f64().unwrap_or(0.0) as i64 - 60 { refresh(oauth) } else { oauth }
}

fn bearer(oauth: &Value) -> String {
    format!("Bearer {}", field(oauth, "access_token"))
}

fn rate(video_id: &str, rating: &str) {
    let oauth = valid_token();
    let url = format!("{API}/videos/rate?id={}&rating={rating}", net::quote(video_id, ""));
    match net::request(&url, &[("Authorization", &bearer(&oauth))], Body::Text(""), 10.0) {
        Ok(r) if r.status < 400 => println!("{}", json!({"status": "ok", "rating": rating, "videoId": video_id})),
        Ok(r) => exit_with(json!({"error": format!("HTTP {}", r.status), "detail": body_text(&r, 200)})),
        Err(e) => exit_with(json!({"error": e})),
    }
}

fn check() {
    let Some(oauth) = fs::read_to_string(oauth_path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object) else {
        return println!("{}", json!({"configured": false}));
    };
    let has_token = !field(&oauth, "access_token").is_empty() && !field(&oauth, "refresh_token").is_empty();
    let mut channel = String::new();
    if has_token {
        let expired = now() > oauth["expires_at"].as_f64().unwrap_or(0.0) as i64 - 60;
        let oauth = if expired { refresh(oauth) } else { oauth };
        if let Ok(r) = net::get(&format!("{API}/channels?part=snippet&mine=true"), &[("Authorization", &bearer(&oauth))], 10.0) {
            if let Ok(v) = serde_json::from_slice::<Value>(&r.body) {
                channel = v.pointer("/items/0/snippet/title").and_then(Value::as_str).unwrap_or_default().to_string();
            }
        }
    }
    println!("{}", json!({"configured": has_token, "channel": channel}));
}

fn setup_request(client_id: &str, client_secret: &str) {
    let r = post_form("https://oauth2.googleapis.com/device/code", &[("client_id", client_id), ("scope", "https://www.googleapis.com/auth/youtube")], 15.0)
        .unwrap_or_else(|e| exit_with(json!({"error": e})));
    if r.status >= 400 {
        exit_with(json!({"error": format!("HTTP {}", r.status), "detail": body_text(&r, 300)}));
    }
    let v: Value = serde_json::from_slice(&r.body).unwrap_or_else(|e| exit_with(json!({"error": e.to_string()})));
    let (Some(user_code), Some(url), Some(device_code)) = (v["user_code"].as_str(), v["verification_url"].as_str(), v["device_code"].as_str()) else {
        exit_with(json!({"error": "the device code answer is incomplete"}))
    };
    if let Err(e) = save(&json!({"client_id": client_id, "client_secret": client_secret})) {
        exit_with(json!({"error": e.to_string()}));
    }
    println!("{}", json!({"status": "ok", "user_code": user_code, "verification_url": url, "device_code": device_code,
        "interval": v.get("interval").cloned().unwrap_or(json!(5)), "expires_in": v.get("expires_in").cloned().unwrap_or(json!(1800))}));
}

fn setup_poll(client_id: &str, client_secret: &str, device_code: &str) {
    let fields = [("client_id", client_id), ("client_secret", client_secret), ("device_code", device_code), ("grant_type", "urn:ietf:params:oauth:grant_type:device_code")];
    let r = match post_form(TOKEN_URL, &fields, 10.0) {
        Ok(r) => r,
        Err(e) => return println!("{}", json!({"status": "error", "error": e})),
    };
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    if r.status >= 400 {
        let out = match v["error"].as_str().unwrap_or("unknown") {
            "authorization_pending" => json!({"status": "pending"}),
            "slow_down" => json!({"status": "slow_down"}),
            "access_denied" => json!({"status": "denied", "error": "User denied access"}),
            "expired_token" => json!({"status": "expired", "error": "Code expired, try again"}),
            other => json!({"status": "error", "error": other}),
        };
        return println!("{out}");
    }
    let Some(access) = v["access_token"].as_str() else { return println!("{}", json!({"status": "error", "error": "'access_token'"})) };
    let oauth = json!({"client_id": client_id, "client_secret": client_secret, "access_token": access, "refresh_token": field(&v, "refresh_token"),
        "expires_at": now() + v["expires_in"].as_i64().unwrap_or(3600)});
    match save(&oauth) {
        Ok(()) => println!("{}", json!({"status": "authorized"})),
        Err(e) => println!("{}", json!({"status": "error", "error": e.to_string()})),
    }
}

fn iso_seconds(d: &str) -> i64 {
    static ISO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?").unwrap());
    ISO.captures(d).map_or(0, |m| {
        let n = |i: usize| m.get(i).and_then(|g| g.as_str().parse::<i64>().ok()).unwrap_or(0);
        n(1) * 3600 + n(2) * 60 + n(3)
    })
}

fn fetch_liked() {
    let oauth = valid_token();
    let (mut page_token, mut count) = (String::new(), 0);
    // a safety cap, checked per page like the python
    while count < 200 {
        let mut url = format!("{API}/videos?part=snippet,contentDetails&myRating=like&maxResults=50");
        if !page_token.is_empty() {
            url += &format!("&pageToken={}", net::quote(&page_token, ""));
        }
        let data: Value = match net::get(&url, &[("Authorization", &bearer(&oauth))], 15.0) {
            Ok(r) if r.status >= 400 => {
                println!("{}", json!({"_error": format!("HTTP {}", r.status), "detail": body_text(&r, 200)}));
                break;
            }
            Ok(r) => match serde_json::from_slice(&r.body) {
                Ok(v) => v,
                Err(e) => {
                    println!("{}", json!({"_error": e.to_string()}));
                    break;
                }
            },
            Err(e) => {
                println!("{}", json!({"_error": e}));
                break;
            }
        };
        for item in data["items"].as_array().into_iter().flatten() {
            let duration = iso_seconds(item.pointer("/contentDetails/duration").and_then(Value::as_str).unwrap_or("PT0S"));
            if !(30..=900).contains(&duration) {
                continue;
            }
            let thumbs = &item["snippet"]["thumbnails"];
            let thumb = [&thumbs["medium"], &thumbs["default"]].into_iter().find(|t| t.as_object().is_some_and(|o| !o.is_empty())).and_then(|t| t["url"].as_str()).unwrap_or_default();
            let id = field(item, "id");
            println!("{}", json!({"videoId": id, "title": item["snippet"]["title"].as_str().unwrap_or("Unknown"), "artist": field(&item["snippet"], "channelTitle"),
                "thumbnail": thumb, "duration": duration, "url": format!("https://music.youtube.com/watch?v={id}")}));
            count += 1;
        }
        page_token = field(&data, "nextPageToken").to_string();
        if page_token.is_empty() {
            break;
        }
    }
    println!("{}", json!({"_done": true, "count": count}));
}

pub fn run_cli(args: &[String]) -> i32 {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["like", id] => rate(id, "like"),
        ["unlike", id] => rate(id, "none"),
        ["check", ..] => check(),
        ["setup-request", id, secret] => setup_request(id, secret),
        ["setup-poll", id, secret, code] => setup_poll(id, secret, code),
        ["fetch-liked", ..] => fetch_liked(),
        [] => exit_with(json!({"error": "Usage: inir-accel ytmusic-rate <command> [args]"})),
        _ => exit_with(json!({"error": format!("Unknown command: {}", a.join(" "))})),
    }
    0
}
