//! YouTube Music for the shell, ported from `innertube.py` and the ytmusicapi parsers it used: InnerTube over curl,
//! each page trimmed to the fields iNiR shows. Prints JSON, or JSON lines closed by {"_done":true,"count":N} for
//! lists; an error prints {"error":...} and exits 1. Public browsing needs no account; the browser session in
//! yt-cookies.txt (or an OAuth token) adds the person's library.

use crate::net::{self, Body};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DOMAIN: &str = "https://music.youtube.com";
const API_KEY: &str = "AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0";
const BROWSER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0";
// The public "YouTube on TV" client ytmusicapi shipped: sign in with a code at youtube.com/activate.
const TV_CLIENT_ID: &str = "861556708454-d6dlm3lh05idd8npek18k6be8ba3oc68.apps.googleusercontent.com";
const TV_CLIENT_KEY: &str = "SboVhoG9s0rNafixCSGGKXAT";
const OAUTH_CODE_URL: &str = "https://www.youtube.com/o/oauth2/device/code";
const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const OAUTH_SCOPE: &str = "https://www.googleapis.com/auth/youtube";
const MRLIR: &str = "musicResponsiveListItemRenderer";
const MTRIR: &str = "musicTwoRowItemRenderer";
const TITLE_TEXT: &str = "title/runs/0/text";
const TITLE_BROWSE_ID: &str = "title/runs/0/navigationEndpoint/browseEndpoint/browseId";
const NAV_BROWSE_ID: &str = "navigationEndpoint/browseEndpoint/browseId";
const PAGE_TYPE: &str = "browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType";
const THUMBNAILS: &str = "thumbnail/musicThumbnailRenderer/thumbnail/thumbnails";
const THUMBNAIL_RENDERER: &str = "thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails";
const PLAY_NAV: &str = "overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint";
const SINGLE_COLUMN_TAB: &str = "contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content";
const TWO_COLUMN: &str = "contents/twoColumnBrowseResultsRenderer";
const MENU_ITEMS: &str = "menu/menuRenderer/items";

// Only these belong in the Cookie header: the whole google.com jar (~190 KB) gets HTTP 413 and an empty body.
const HEADER_COOKIES: [&str; 19] = [
    "SID", "HSID", "SSID", "APISID", "SAPISID", "__Secure-1PSID", "__Secure-3PSID", "__Secure-1PAPISID", "__Secure-3PAPISID",
    "__Secure-1PSIDCC", "__Secure-3PSIDCC", "SIDCC", "__Secure-1PSIDTS", "__Secure-3PSIDTS", "LOGIN_INFO", "VISITOR_INFO1_LIVE",
    "VISITOR_PRIVACY_METADATA", "YSC", "PREF",
];

type R<T> = Result<T, String>;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn cfg_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(base).join("illogical-impulse")
}

fn oauth_path() -> PathBuf {
    cfg_dir().join("ytmusic_oauth.json")
}

fn itube_oauth_path() -> PathBuf {
    cfg_dir().join("innertube_oauth.json")
}

fn cookie_path() -> PathBuf {
    cfg_dir().join("yt-cookies.txt")
}

fn emit(v: &Value) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{v}").and_then(|_| out.flush()).is_err() {
        std::process::exit(0);
    }
}

fn fail(msg: &str, detail: &str) -> ! {
    let mut out = json!({"error": msg});
    if !detail.is_empty() {
        out["detail"] = json!(detail.chars().take(300).collect::<String>());
    }
    emit(&out);
    std::process::exit(1)
}

// ---- json paths (ytmusicapi's nav)

fn at<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('/').try_fold(root, |v, k| match k.parse::<usize>() {
        Ok(i) if v.is_array() => v.get(i),
        _ => v.get(k),
    })
}

fn text<'a>(root: &'a Value, path: &str) -> Option<&'a str> {
    at(root, path)?.as_str()
}

fn list<'a>(root: &'a Value, path: &str) -> &'a [Value] {
    at(root, path).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
    }
}

fn or_null(v: Option<&Value>) -> Value {
    v.cloned().unwrap_or(Value::Null)
}

/// `item.get(key, default)`: a present null stays null.
fn get_or(item: &Value, key: &str, default: Value) -> Value {
    item.get(key).cloned().unwrap_or(default)
}

// ---- ytmusicapi's run parsers

enum Run {
    Album(Value),
    Artist(Value),
    Duration(String),
    Year(String),
    Views,
}

fn is_views(text: &str) -> bool {
    static PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\D*?[\s:\x{ff1a}\x{200e}-\x{200f}\x{202a}-\x{202e}]").unwrap());
    static DIGIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d").unwrap());
    let mut text = text;
    let mut prefixed = false;
    if !text.chars().any(|c| c.is_ascii_alphabetic()) {
        if let Some(m) = PREFIX.find(text) {
            text = &text[m.end()..];
            prefixed = true;
        }
    }
    DIGIT.is_match(text) && (prefixed || !text.is_ascii() || text.contains(' '))
}

fn song_run(run: &Value) -> Run {
    static DURATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+:)*\d+:\d+$").unwrap());
    static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}$").unwrap());
    let t = run["text"].as_str().unwrap_or_default();
    if run.get("navigationEndpoint").is_some() {
        let id = text(run, NAV_BROWSE_ID);
        let item = json!({"name": t, "id": id});
        return if id.is_some_and(|i| i.starts_with("MPRE") || i.contains("release_detail")) { Run::Album(item) } else { Run::Artist(item) };
    }
    if DURATION.is_match(t) {
        Run::Duration(t.into())
    } else if YEAR.is_match(t) {
        Run::Year(t.into())
    } else if is_views(t) {
        Run::Views
    } else {
        Run::Artist(json!({"name": t, "id": null}))
    }
}

fn is_dot(run: &Value) -> bool {
    run.as_object().is_some_and(|o| o.len() == 1) && run["text"] == " • "
}

fn parse_duration(duration: &str) -> Option<i64> {
    let parts: Vec<&str> = duration.trim().split(':').collect();
    if duration.trim().is_empty() || !parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(parts.iter().rev().zip([1, 60, 3600]).map(|(p, m)| m * p.parse::<i64>().unwrap_or(0)).sum())
}

fn song_runs(runs: &[Value], skip_type_spec: bool) -> serde_json::Map<String, Value> {
    let mut runs = runs;
    if skip_type_spec
        && runs.len() > 2
        && runs[0].get("navigationEndpoint").is_none()
        && matches!(song_run(&runs[0]), Run::Artist(_))
        && is_dot(&runs[1])
        && matches!(song_run(&runs[2]), Run::Artist(_) | Run::Duration(_) | Run::Views | Run::Year(_))
    {
        runs = &runs[2..];
    }
    let mut parsed = serde_json::Map::new();
    for run in runs.iter().step_by(2) {
        match song_run(run) {
            Run::Album(d) => {
                parsed.insert("album".into(), d);
            }
            Run::Artist(d) => {
                let artists = parsed.entry("artists").or_insert_with(|| json!([]));
                artists.as_array_mut().unwrap().push(d);
            }
            Run::Duration(t) => {
                parsed.insert("duration_seconds".into(), json!(parse_duration(&t)));
                parsed.insert("duration".into(), json!(t));
            }
            Run::Year(t) => {
                parsed.insert("year".into(), json!(t));
            }
            Run::Views => {}
        }
    }
    parsed
}

fn merge(item: &mut Value, extra: serde_json::Map<String, Value>) {
    if let Some(o) = item.as_object_mut() {
        o.extend(extra);
    }
}

fn artists_runs(runs: &[Value]) -> Vec<Value> {
    runs.iter().step_by(2).map(|r| json!({"name": r.get("text"), "id": text(r, NAV_BROWSE_ID)})).collect()
}

fn flex_column(item: &Value, index: usize) -> Option<&Value> {
    let col = at(item, &format!("flexColumns/{index}/musicResponsiveListItemFlexColumnRenderer"))?;
    at(col, "text/runs").map(|_| col)
}

fn item_text(item: &Value, index: usize, run: usize) -> Option<&str> {
    text(flex_column(item, index)?, &format!("text/runs/{run}/text"))
}

fn album_playlist_id(data: Option<&Value>) -> Value {
    let data = data.filter(|d| truthy(Some(d)));
    let pick = |p: &str| data.and_then(|d| at(d, p)).filter(|v| truthy(Some(v))).cloned();
    pick("watchPlaylistEndpoint/playlistId").or_else(|| pick("watchEndpoint/playlistId")).unwrap_or(Value::Null)
}

// ---- iNiR's shapes (innertube.py's _track and _card)

fn best_thumb(thumbs: Option<&Value>) -> String {
    let Some(thumbs) = thumbs.and_then(Value::as_array).filter(|t| !t.is_empty()) else { return String::new() };
    let area = |t: &Value| t["width"].as_i64().unwrap_or(0) * t["height"].as_i64().unwrap_or(0);
    let best = thumbs.iter().fold(&thumbs[0], |best, t| if area(t) > area(best) { t } else { best });
    best["url"].as_str().unwrap_or_default().to_string()
}

fn artist_names(item: &Value) -> String {
    let artists = item.get("artists").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    artists.iter().filter_map(|a| a["name"].as_str().filter(|n| !n.is_empty())).collect::<Vec<_>>().join(", ")
}

fn track_seconds(item: &Value) -> i64 {
    if let Some(s) = item.get("duration_seconds").and_then(Value::as_i64) {
        return s;
    }
    match item.get("duration").and_then(Value::as_str) {
        Some(d) if d.contains(':') => d.split(':').filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())).fold(0, |acc, p| acc * 60 + p.parse::<i64>().unwrap_or(0)),
        _ => 0,
    }
}

fn track(item: &Value) -> Value {
    let vid = item.get("videoId").and_then(Value::as_str).unwrap_or_default();
    let album = item.get("album").filter(|a| truthy(Some(a)));
    let album_field = |k: &str| album.filter(|a| a.is_object()).map_or(json!(""), |a| get_or(a, k, json!("")));
    json!({"type": "song", "videoId": vid, "title": get_or(item, "title", json!("")), "artist": artist_names(item),
        "thumbnail": best_thumb(item.get("thumbnails")), "duration": track_seconds(item),
        "url": if vid.is_empty() { String::new() } else { format!("https://music.youtube.com/watch?v={vid}") },
        "album": album_field("name"), "albumId": album_field("id")})
}

fn card(item: &Value) -> Value {
    if truthy(item.get("videoId")) || !(truthy(item.get("playlistId")) || truthy(item.get("browseId"))) {
        return track(item);
    }
    let bid = item.get("browseId").and_then(Value::as_str).unwrap_or_default();
    // browseId prefixes are stable: UC… artist, MPREb… album, the rest playlists.
    let kind = if item.get("subscribers").is_some_and(|s| !s.is_null()) || item["type"] == "artist" || bid.starts_with("UC") {
        "artist"
    } else if item["type"] == "album" || bid.starts_with("MPREb") {
        "album"
    } else {
        "playlist"
    };
    let title = item.get("title").filter(|t| truthy(Some(t))).cloned().unwrap_or_else(|| get_or(item, "artist", json!("")));
    let names = artist_names(item);
    let subtitle = if names.is_empty() { get_or(item, "description", json!("")) } else { json!(names) };
    json!({"type": kind, "title": title, "subtitle": subtitle, "thumbnail": best_thumb(item.get("thumbnails")),
        "browseId": get_or(item, "browseId", json!("")), "playlistId": get_or(item, "playlistId", json!(""))})
}

// ---- the client

struct OAuthToken {
    token: Value,
    client_id: String,
    secret: String,
    cache: Option<PathBuf>,
}

enum Auth {
    Public,
    Browser { cookie: String, sapisid: String },
    OAuth(OAuthToken),
}

struct Client {
    auth: Auth,
    visitor: Option<String>,
}

fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend(((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// days since 1970-01-01 as a UTC (year, month, day)
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let (era, doe) = (z.div_euclid(146097), z.rem_euclid(146097));
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn context() -> Value {
    let (y, m, d) = civil(now().div_euclid(86400));
    json!({"client": {"clientName": "WEB_REMIX", "clientVersion": format!("1.{y:04}{m:02}{d:02}.01.00"), "hl": "en"}, "user": {}})
}

fn base_headers() -> Vec<(String, String)> {
    [("user-agent", USER_AGENT), ("accept", "*/*"), ("content-type", "application/json"), ("origin", DOMAIN)].map(|(k, v)| (k.into(), v.into())).into()
}

impl Client {
    fn public() -> Client {
        Client { auth: Auth::Public, visitor: None }
    }

    fn visitor(&mut self) -> R<String> {
        if let Some(v) = &self.visitor {
            return Ok(v.clone());
        }
        static YTCFG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"ytcfg\.set\s*\(\s*(\{.+?\})\s*\)\s*;").unwrap());
        let mut headers = base_headers();
        headers.push(("cookie".into(), "SOCS=CAI".into()));
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let page = net::get(DOMAIN, &refs, 30.0)?;
        let page = String::from_utf8_lossy(&page.body);
        let visitor = match YTCFG.captures(&page) {
            Some(c) => serde_json::from_str::<Value>(&c[1]).map_err(|e| e.to_string())?["VISITOR_DATA"].as_str().unwrap_or_default().to_string(),
            None => String::new(),
        };
        self.visitor = Some(visitor.clone());
        Ok(visitor)
    }

    fn headers(&mut self) -> R<Vec<(String, String)>> {
        let visitor = self.visitor()?;
        let mut headers = match &mut self.auth {
            Auth::Public => {
                let mut h = base_headers();
                h.push(("cookie".into(), "SOCS=CAI".into()));
                h
            }
            Auth::Browser { cookie, sapisid } => {
                let ts = now();
                let hash = sha1_hex(format!("{ts} {sapisid} {DOMAIN}").as_bytes());
                [
                    ("cookie", cookie.as_str()),
                    ("authorization", &format!("SAPISIDHASH {ts}_{hash}")),
                    ("x-goog-authuser", "0"),
                    ("origin", DOMAIN),
                    ("user-agent", BROWSER_AGENT),
                    ("accept", "*/*"),
                    ("accept-language", "en-US,en;q=0.5"),
                    ("content-type", "application/json"),
                    ("x-origin", DOMAIN),
                ]
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .into()
            }
            Auth::OAuth(token) => {
                let access = token.access()?;
                let mut h = base_headers();
                h.push(("authorization".into(), format!("{} {access}", token.token["token_type"].as_str().unwrap_or("Bearer"))));
                h.push(("X-Goog-Request-Time".into(), now().to_string()));
                h
            }
        };
        headers.push(("X-Goog-Visitor-Id".into(), visitor));
        Ok(headers)
    }

    fn send(&mut self, endpoint: &str, body: &Value, extra: &str) -> R<Value> {
        let mut body = body.clone();
        body["context"] = context();
        let key = if matches!(self.auth, Auth::Browser { .. }) { format!("&key={API_KEY}") } else { String::new() };
        let url = format!("{DOMAIN}/youtubei/v1/{endpoint}?alt=json{key}{extra}");
        let headers = self.headers()?;
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let response = net::request(&url, &refs, Body::Text(&body.to_string()), 30.0)?;
        let payload: Value = serde_json::from_slice(&response.body).map_err(|e| e.to_string())?;
        if response.status >= 400 {
            let message = text(&payload, "error/message").unwrap_or_default();
            return Err(format!("Server returned HTTP {}.\n{message}", response.status));
        }
        Ok(payload)
    }

    /// ytmusicapi's get_continuations: the classic `continuations` token, `limit` more items at most.
    fn continuations(&mut self, endpoint: &str, body: &Value, results: &Value, kind: &str, limit: i64, ctoken_path: &str, parse: &dyn Fn(&[Value]) -> Vec<Value>) -> R<Vec<Value>> {
        let mut items = Vec::new();
        let mut results = results.clone();
        while results.get("continuations").is_some() && (items.len() as i64) < limit {
            let Some(token) = text(&results, &format!("continuations/0/next{ctoken_path}ContinuationData/continuation")).map(str::to_string) else { break };
            let response = self.send(endpoint, body, &format!("&ctoken={token}&continuation={token}"))?;
            let Some(next) = at(&response, &format!("continuationContents/{kind}")) else { break };
            results = next.clone();
            let contents = ["contents", "items"].iter().find_map(|k| results.get(*k)).and_then(Value::as_array).map(|c| parse(c)).unwrap_or_default();
            if contents.is_empty() {
                break;
            }
            items.extend(contents);
        }
        Ok(items)
    }

    /// The 2025 continuations: a continuationItemRenderer closing the list, fetched by token.
    fn continuations_2025(&mut self, contents: &[Value], limit: i64, parse: &dyn Fn(&[Value]) -> Vec<Value>) -> R<Vec<Value>> {
        let token_of = |contents: &[Value]| -> Option<String> {
            let last = contents.last()?;
            if let Some(t) = text(last, "continuationItemRenderer/continuationEndpoint/continuationCommand/token") {
                return Some(t.into());
            }
            list(last, "continuationItemRenderer/continuationEndpoint/commandExecutorCommand/commands")
                .iter()
                .find(|c| text(c, "continuationCommand/request") == Some("CONTINUATION_REQUEST_TYPE_BROWSE"))
                .and_then(|c| text(c, "continuationCommand/token").map(String::from))
        };
        let mut items = Vec::new();
        let mut token = token_of(contents);
        while let Some(t) = token.filter(|_| (items.len() as i64) < limit) {
            let response = self.send("browse", &json!({"continuation": t}), "")?;
            let next = list(&response, "onResponseReceivedActions/0/appendContinuationItemsAction/continuationItems");
            let parsed = parse(next);
            if next.is_empty() || parsed.is_empty() {
                break;
            }
            items.extend(parsed);
            token = token_of(next);
        }
        Ok(items)
    }
}

impl OAuthToken {
    fn access(&mut self) -> R<String> {
        let expires_at = self.token["expires_at"].as_i64().unwrap_or(0);
        if expires_at - now() < 60 {
            let refresh = self.token["refresh_token"].as_str().unwrap_or_default().to_string();
            let fresh = oauth_post(OAUTH_TOKEN_URL, &[("client_secret", self.secret.as_str()), ("grant_type", "refresh_token"), ("refresh_token", &refresh)], &self.client_id)?;
            let Some(access) = fresh["access_token"].as_str() else { return Err(format!("token refresh failed: {fresh}")) };
            self.token["access_token"] = json!(access);
            self.token["expires_at"] = json!(now() + fresh["expires_in"].as_i64().unwrap_or(0));
            if let Some(path) = &self.cache {
                let _ = fs::write(path, serde_json::to_string_pretty(&self.token).unwrap_or_default());
            }
        }
        Ok(self.token["access_token"].as_str().unwrap_or_default().to_string())
    }
}

fn form(fields: &[(&str, &str)]) -> String {
    fields.iter().map(|(k, v)| format!("{}={}", net::quote(k, " ").replace(' ', "+"), net::quote(v, " ").replace(' ', "+"))).collect::<Vec<_>>().join("&")
}

fn oauth_post(url: &str, fields: &[(&str, &str)], client_id: &str) -> R<Value> {
    let mut fields = fields.to_vec();
    fields.push(("client_id", client_id));
    let agent = format!("{USER_AGENT} Cobalt/Version");
    let headers = [("User-Agent", agent.as_str()), ("Content-Type", "application/x-www-form-urlencoded")];
    let response = net::request(url, &headers, Body::Text(&form(&fields)), 30.0)?;
    let payload: Value = serde_json::from_slice(&response.body).map_err(|e| e.to_string())?;
    if response.status == 401 {
        return Err(match payload["error"].as_str() {
            Some("unauthorized_client") => "Token refresh error. Most likely client/token mismatch.".into(),
            Some("invalid_client") => "OAuth client failure. Most likely client_id and client_secret mismatch or YouTubeData API is not enabled.".into(),
            _ => format!("OAuth request error. status_code: 401, url: {url}, content: {payload}"),
        });
    }
    Ok(payload)
}

/// The Cookie header for YTM from the Netscape jar, and every cookie by name; None without a logged-in session.
/// A YouTube-domain value beats the google.com one of the same name, whatever the row order.
fn read_cookies(path: &Path) -> Option<(String, HashMap<String, String>)> {
    let text = fs::read_to_string(path).ok()?;
    let priority = |domain: &str| {
        let host = domain.trim_start_matches('.').to_lowercase();
        match host.as_str() {
            "music.youtube.com" => 3,
            h if h == "youtube.com" || h.ends_with(".youtube.com") => 2,
            h if h == "google.com" || h.ends_with(".google.com") => 1,
            _ => 0,
        }
    };
    let (mut order, mut cookies, mut ranks): (Vec<String>, HashMap<String, String>, HashMap<String, i32>) = Default::default();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() >= 7 && (p[0].contains("youtube.com") || p[0].contains("google.com")) {
            let rank = priority(p[0]);
            if rank >= *ranks.get(p[5]).unwrap_or(&-1) {
                if !cookies.contains_key(p[5]) {
                    order.push(p[5].to_string());
                }
                cookies.insert(p[5].into(), p[6].into());
                ranks.insert(p[5].into(), rank);
            }
        }
    }
    ["SAPISID", "__Secure-3PAPISID", "__Secure-1PAPISID"].iter().find(|k| cookies.get(**k).is_some_and(|v| !v.is_empty()))?;
    let header = order.iter().filter(|k| HEADER_COOKIES.contains(&k.as_str())).map(|k| format!("{k}={}", cookies[k])).collect::<Vec<_>>().join("; ");
    Some((header, cookies))
}

fn browser_client(path: &Path) -> Option<Client> {
    let (cookie, _) = read_cookies(path)?;
    // ytmusicapi signs every request with __Secure-3PAPISID and refuses a jar without it.
    let sapisid = cookie.split("; ").find_map(|c| c.strip_prefix("__Secure-3PAPISID="))?.replace('"', "");
    Some(Client { auth: Auth::Browser { cookie, sapisid }, visitor: None })
}

/// Cookies first (the only auth YouTube still honours for personal pages), then the OAuth tokens.
fn authenticated_client(path: &Path, allow_oauth: bool) -> Option<Client> {
    if let Some(c) = browser_client(path) {
        return Some(c);
    }
    if !allow_oauth {
        return None;
    }
    let read = |p: &Path| fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object);
    if let Some(token) = read(&itube_oauth_path()) {
        let token = OAuthToken { token, client_id: TV_CLIENT_ID.into(), secret: TV_CLIENT_KEY.into(), cache: Some(itube_oauth_path()) };
        return Some(Client { auth: Auth::OAuth(token), visitor: None });
    }
    let o = read(&oauth_path())?;
    let field = |k: &str| o[k].as_str().filter(|v| !v.is_empty()).map(String::from);
    let (id, secret, refresh) = (field("client_id")?, field("client_secret")?, field("refresh_token")?);
    let token = json!({"access_token": o.get("access_token").cloned().unwrap_or(json!("")), "refresh_token": refresh, "scope": OAUTH_SCOPE,
        "token_type": "Bearer", "expires_at": o["expires_at"].as_f64().unwrap_or(0.0) as i64, "expires_in": 3600});
    Some(Client { auth: Auth::OAuth(OAuthToken { token, client_id: id, secret, cache: None }), visitor: None })
}

/// innertube.py's _FallbackClient: a stale session empties every page, so a failed or empty authed call reruns public.
struct Yt {
    authed: Option<Client>,
    public: Option<Client>,
}

impl Yt {
    fn new() -> Yt {
        Yt { authed: authenticated_client(&cookie_path(), true), public: None }
    }

    fn call<T>(&mut self, f: impl Fn(&mut Client) -> R<T>, empty: impl Fn(&T) -> bool) -> R<T> {
        if let Some(authed) = self.authed.as_mut() {
            if let Ok(v) = f(authed).map_err(|_| ()).and_then(|v| if empty(&v) { Err(()) } else { Ok(v) }) {
                return Ok(v);
            }
        }
        f(self.public.get_or_insert_with(Client::public))
    }
}

// ---- pages

fn search_params(filter: &str) -> String {
    match filter {
        "playlists" => "Eg-KAQwIABAAGAAgACgBMABqChAEEAMQCRAFEAo%3D".into(),
        f if f.contains("playlists") => format!("EgeKAQQoA{}BagwQDhAKEAMQBBAJEAU%3D", if f == "featured_playlists" { "Dg" } else { "EA" }),
        f => {
            let p2 = match f {
                "videos" => "IQ",
                "albums" => "IY",
                "artists" => "Ig",
                _ => "II",
            };
            format!("EgWKAQ{p2}AWoMEA4QChADEAQQCRAF")
        }
    }
}

fn search_result(data: &Value, result_type: &str) -> Value {
    let mut r = json!({"resultType": result_type});
    if result_type == "artist" {
        r["artist"] = json!(item_text(data, 0, 0));
    } else {
        r["title"] = json!(item_text(data, 0, 0));
    }
    match result_type {
        "album" => {
            r["type"] = json!(item_text(data, 1, 0));
            r["playlistId"] = album_playlist_id(at(data, PLAY_NAV));
        }
        "song" => r["album"] = Value::Null,
        _ => {}
    }
    if matches!(result_type, "song" | "video") {
        r["videoId"] = json!(text(data, &format!("{PLAY_NAV}/watchEndpoint/videoId")));
    }
    if matches!(result_type, "song" | "video" | "album") {
        if let Some(flex) = flex_column(data, 1) {
            let mut runs = list(flex, "text/runs").to_vec();
            if let Some(flex2) = flex_column(data, 2) {
                runs.push(json!({"text": ""}));
                runs.extend(list(flex2, "text/runs").iter().cloned());
            }
            merge(&mut r, song_runs(&runs, true));
        }
    }
    if matches!(result_type, "artist" | "album" | "playlist") {
        r["browseId"] = json!(text(data, NAV_BROWSE_ID));
    }
    r["thumbnails"] = or_null(at(data, THUMBNAILS));
    r
}

fn search(c: &mut Client, query: &str, filter: &str) -> R<Vec<Value>> {
    let body = json!({"query": query, "params": search_params(filter)});
    let response = c.send("search", &body, "")?;
    let Some(contents) = response.get("contents") else { return Ok(Vec::new()) };
    let results = match contents.get("tabbedSearchResultsRenderer") {
        Some(t) => at(t, "tabs/0/tabRenderer/content").ok_or("no search tab")?,
        None => contents,
    };
    let sections = at(results, "sectionListRenderer/contents").and_then(Value::as_array).ok_or("no search results")?;
    let internal = if filter.contains("playlists") { "playlists" } else { filter };
    let result_type = &internal[..internal.len() - 1];
    let parse = |rows: &[Value]| rows.iter().filter_map(|r| r.get(MRLIR)).map(|d| search_result(d, result_type)).collect::<Vec<_>>();
    let mut out = Vec::new();
    for section in sections {
        let (shelf, category) = if let Some(shelf) = section.get("musicShelfRenderer") {
            (shelf, text(shelf, TITLE_TEXT))
        } else if let Some(shelf) = section.get("itemSectionRenderer") {
            (shelf, None)
        } else {
            continue;
        };
        let rows = list(shelf, "contents");
        if rows.first().is_none_or(|r| r.get(MRLIR).is_none()) {
            continue;
        }
        if category.is_some_and(|c| !c.to_lowercase().contains(result_type)) {
            continue;
        }
        out.extend(parse(rows));
        if section.get("musicShelfRenderer").is_some() {
            let more = c.continuations("search", &body, shelf, "musicShelfContinuation", 25 - out.len() as i64, "", &parse)?;
            out.extend(more);
        }
    }
    Ok(out)
}

fn subtitle_kind(data: &Value, item: &mut Value) {
    if let Some(t) = text(data, "subtitle/runs/0/text") {
        if !t.is_empty() && t.chars().all(char::is_numeric) {
            item["year"] = json!(t);
        } else {
            item["type"] = json!(t);
            if let Some(y) = text(data, "subtitle/runs/2/text").filter(|y| !y.is_empty() && y.chars().all(char::is_numeric)) {
                item["year"] = json!(y);
            }
        }
    }
}

fn parse_album(data: &Value) -> Value {
    let artists: Vec<Value> = list(data, "subtitle/runs").iter().filter(|r| r.get("navigationEndpoint").is_some()).map(|r| json!({"id": text(r, NAV_BROWSE_ID), "name": r.get("text")})).collect();
    let mut item = json!({"title": text(data, TITLE_TEXT), "artists": artists, "browseId": text(data, TITLE_BROWSE_ID), "thumbnails": or_null(at(data, THUMBNAIL_RENDERER))});
    subtitle_kind(data, &mut item);
    item
}

fn parse_single(data: &Value) -> Value {
    let mut item = json!({"title": text(data, TITLE_TEXT), "browseId": text(data, TITLE_BROWSE_ID), "thumbnails": or_null(at(data, THUMBNAIL_RENDERER))});
    subtitle_kind(data, &mut item);
    item
}

fn parse_playlist(data: &Value) -> Value {
    let id: String = text(data, TITLE_BROWSE_ID).unwrap_or_default().chars().skip(2).collect();
    let mut item = json!({"title": text(data, TITLE_TEXT), "playlistId": id, "thumbnails": or_null(at(data, THUMBNAIL_RENDERER))});
    if let Some(runs) = at(data, "subtitle/runs").and_then(Value::as_array) {
        item["description"] = json!(runs.iter().filter_map(|r| r["text"].as_str()).collect::<String>());
    }
    item
}

fn parse_related_artist(data: &Value) -> Value {
    let subscribers = text(data, "subtitle/runs/0/text").filter(|s| !s.is_empty()).map(|s| s.split(' ').next().unwrap_or_default());
    json!({"title": text(data, TITLE_TEXT), "browseId": text(data, TITLE_BROWSE_ID), "subscribers": subscribers, "thumbnails": or_null(at(data, THUMBNAIL_RENDERER))})
}

fn parse_song_flat(data: &Value) -> Value {
    let col0 = flex_column(data, 0);
    let video_id = col0.and_then(|c| text(c, "text/runs/0/navigationEndpoint/watchEndpoint/videoId")).or_else(|| text(data, &format!("{PLAY_NAV}/watchEndpoint/videoId")));
    let mut song = json!({"title": col0.and_then(|c| text(c, "text/runs/0/text")), "videoId": video_id, "thumbnails": or_null(at(data, THUMBNAILS))});
    merge(&mut song, song_runs(flex_column(data, 1).map_or(&[], |c| list(c, "text/runs")), true));
    if let Some(c2) = flex_column(data, 2).filter(|c| at(c, "text/runs/0/navigationEndpoint").is_some()) {
        song["album"] = json!({"name": text(c2, "text/runs/0/text"), "id": text(c2, "text/runs/0/navigationEndpoint/browseEndpoint/browseId")});
    }
    song
}

fn mixed_content(rows: &[Value]) -> Vec<Value> {
    let mut shelves = Vec::new();
    for row in rows {
        if let Some(shelf) = row.get("musicDescriptionShelfRenderer") {
            shelves.push(json!({"title": text(shelf, "header/runs/0/text"), "contents": text(shelf, "description/runs/0/text")}));
            continue;
        }
        let Some(results) = row.as_object().and_then(|o| o.values().next()) else { continue };
        let Some(items) = results.get("contents").and_then(Value::as_array) else { continue };
        let title = text(results, "header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text").filter(|t| !t.is_empty()).or_else(|| text(results, "header/musicCarouselShelfBasicHeaderRenderer/strapline/runs/0/text"));
        let mut contents = Vec::new();
        for result in items {
            if let Some(data) = result.get(MTRIR) {
                let content = match text(data, &format!("title/runs/0/navigationEndpoint/browseEndpoint/{PAGE_TYPE}")) {
                    None if at(data, "navigationEndpoint/watchPlaylistEndpoint/playlistId").is_some() => {
                        json!({"title": text(data, TITLE_TEXT), "playlistId": text(data, "navigationEndpoint/watchPlaylistEndpoint/playlistId"), "thumbnails": or_null(at(data, THUMBNAIL_RENDERER))})
                    }
                    None if at(data, "navigationEndpoint/watchEndpoint/videoId").is_some() => {
                        let mut song = json!({"title": text(data, TITLE_TEXT), "videoId": text(data, "navigationEndpoint/watchEndpoint/videoId"),
                            "playlistId": text(data, "navigationEndpoint/watchEndpoint/playlistId"), "thumbnails": or_null(at(data, THUMBNAIL_RENDERER))});
                        merge(&mut song, song_runs(list(data, "subtitle/runs"), true));
                        song
                    }
                    None => continue,
                    Some("MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK") => parse_album(data),
                    Some("MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL") => parse_related_artist(data),
                    Some("MUSIC_PAGE_TYPE_PLAYLIST") => parse_playlist(data),
                    Some("MUSIC_PAGE_TYPE_PODCAST_SHOW_DETAIL_PAGE") => json!({"title": text(data, TITLE_TEXT), "browseId": text(data, TITLE_BROWSE_ID), "thumbnails": or_null(at(data, THUMBNAIL_RENDERER))}),
                    Some(_) => Value::Null,
                };
                contents.push(content);
            } else if let Some(data) = result.get(MRLIR) {
                contents.push(parse_song_flat(data));
            }
        }
        shelves.push(json!({"title": title, "contents": contents}));
    }
    shelves
}

fn home(c: &mut Client, limit: i64) -> R<Vec<Value>> {
    let body = json!({"browseId": "FEmusic_home"});
    let response = c.send("browse", &body, "")?;
    let section = at(&response, &format!("{SINGLE_COLUMN_TAB}/sectionListRenderer")).ok_or("no home sections")?;
    let mut shelves = mixed_content(list(section, "contents"));
    let more = c.continuations("browse", &body, section, "sectionListContinuation", limit - shelves.len() as i64, "", &mixed_content)?;
    shelves.extend(more);
    Ok(shelves)
}

fn watch_tracks(results: &[Value]) -> Vec<Value> {
    let mut tracks = Vec::new();
    for result in results {
        let result = result.get("playlistPanelVideoWrapperRenderer").and_then(|w| w.get("primaryRenderer")).unwrap_or(result);
        let Some(data) = result.get("playlistPanelVideoRenderer") else { continue };
        if data.get("unplayableText").is_some() {
            continue;
        }
        let mut t = json!({"videoId": data.get("videoId"), "title": text(data, TITLE_TEXT), "length": text(data, "lengthText/runs/0/text"), "thumbnail": or_null(data.get("thumbnail").and_then(|t| t.get("thumbnails")))});
        merge(&mut t, song_runs(list(data, "longBylineText/runs"), false));
        tracks.push(t);
    }
    tracks
}

struct Watch {
    tracks: Vec<Value>,
    lyrics: Option<String>,
}

fn watch_playlist(c: &mut Client, video_id: &str, limit: i64) -> R<Watch> {
    let body = json!({"enablePersistentPlaylistPanel": true, "isAudioOnly": true, "tunerSettingValue": "AUTOMIX_SETTING_NORMAL", "videoId": video_id,
        "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"hasPersistentPlaylistPanel": true, "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"}},
        "playlistId": format!("RDAMVM{video_id}")});
    let response = c.send("next", &body, "")?;
    let renderer = at(&response, "contents/singleColumnMusicWatchNextResultsRenderer/tabbedRenderer/watchNextTabbedResultsRenderer").ok_or("No content returned by the server.")?;
    let lyrics = list(renderer, "tabs")
        .iter()
        .filter(|t| at(t, "tabRenderer/unselectable").is_none())
        .filter_map(|t| at(t, "tabRenderer/endpoint/browseEndpoint"))
        .find(|b| text(b, PAGE_TYPE) == Some("MUSIC_PAGE_TYPE_TRACK_LYRICS"))
        .and_then(|b| b["browseId"].as_str().map(String::from));
    let results = at(renderer, "tabs/0/tabRenderer/content/musicQueueRenderer/content/playlistPanelRenderer").ok_or("No content returned by the server.")?;
    let mut tracks = watch_tracks(list(results, "contents"));
    let more = c.continuations("next", &body, results, "playlistPanelContinuation", limit - tracks.len() as i64, "Radio", &watch_tracks)?;
    tracks.extend(more);
    Ok(Watch { tracks, lyrics })
}

/// ytmusicapi's parse_playlist_item, kept to what a track row shows.
fn playlist_item(data: &Value, is_album: bool, is_collaborative: bool) -> Option<Value> {
    let mut video_id = Value::Null;
    for item in list(data, MENU_ITEMS) {
        if let Some(edit) = at(item, "menuServiceItemRenderer/serviceEndpoint/playlistEditEndpoint") {
            video_id = or_null(at(edit, "actions/0/removedVideoId"));
        }
    }
    if let Some(v) = at(data, &format!("{PLAY_NAV}/watchEndpoint/videoId")) {
        video_id = v.clone();
    }
    let available = data["musicItemRendererDisplayPolicy"] != "MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT";
    let preset = !available || is_album;
    let (mut title_i, mut artist_i, mut duration_i) = (preset.then_some(0), preset.then_some(1), None);
    let mut album_i = if is_collaborative { Some(3) } else { preset.then_some(2) };
    let (mut user_channels, mut unrecognized) = (Vec::new(), None);
    for index in 0..list(data, "flexColumns").len() {
        let col = flex_column(data, index);
        match col.and_then(|c| at(c, "text/runs/0/navigationEndpoint")) {
            None => {
                if let Some(run) = col.and_then(|c| at(c, "text/runs/0")).filter(|r| r.get("text").is_some()) {
                    if matches!(song_run(run), Run::Duration(_)) {
                        duration_i = Some(index);
                    } else {
                        unrecognized = unrecognized.or(Some(index));
                    }
                }
            }
            Some(ep) if ep.get("watchEndpoint").is_some() => title_i = Some(index),
            Some(ep) => match text(ep, &format!("browseEndpoint/{PAGE_TYPE}")) {
                Some("MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_UNKNOWN") => artist_i = Some(index),
                Some("MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK") => album_i = Some(index),
                Some("MUSIC_PAGE_TYPE_USER_CHANNEL") => user_channels.push(index),
                Some("MUSIC_PAGE_TYPE_NON_MUSIC_AUDIO_TRACK_PAGE") => title_i = Some(index),
                _ => {}
            },
        }
    }
    let artist_i = artist_i.or(unrecognized).or(user_channels.last().copied());
    let title = title_i.and_then(|i| item_text(data, i, 0));
    if title == Some("Song deleted") {
        return None;
    }
    let artists = artist_i.map(|i| flex_column(data, i).map_or(Vec::new(), |c| artists_runs(list(c, "text/runs"))));
    let album = album_i.and_then(|i| flex_column(data, i).map(|c| json!({"name": item_text(data, i, 0), "id": text(c, "text/runs/0/navigationEndpoint/browseEndpoint/browseId")})));
    let mut duration = duration_i.filter(|&i| i != 0).and_then(|i| item_text(data, i, 0)).map(String::from);
    if data.get("fixedColumns").is_some() {
        let fixed = at(data, "fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text");
        duration = fixed.and_then(|t| t.get("simpleText").or_else(|| at(t, "runs/0/text"))).and_then(Value::as_str).map(String::from);
    }
    let mut song = json!({"videoId": video_id, "title": title, "artists": artists, "album": album, "thumbnails": or_null(at(data, THUMBNAILS))});
    if let Some(d) = duration.filter(|d| !d.is_empty()) {
        song["duration_seconds"] = json!(parse_duration(&d));
        song["duration"] = json!(d);
    }
    Some(song)
}

fn playlist_items(rows: &[Value], is_album: bool, is_collaborative: bool) -> Vec<Value> {
    rows.iter().filter_map(|r| r.get(MRLIR)).filter_map(|d| playlist_item(d, is_album, is_collaborative)).collect()
}

fn artist(c: &mut Client, browse_id: &str) -> R<Value> {
    let id = browse_id.strip_prefix("MPLA").unwrap_or(browse_id);
    let response = c.send("browse", &json!({"browseId": id}), "")?;
    let results = at(&response, &format!("{SINGLE_COLUMN_TAB}/sectionListRenderer/contents"))
        .or_else(|| at(&response, &format!("{TWO_COLUMN}/tabs/0/tabRenderer/content/sectionListRenderer/contents")))
        .and_then(Value::as_array)
        .ok_or("no artist sections")?;
    let header = at(&response, "header/musicImmersiveHeaderRenderer").ok_or("no artist header")?;
    let description = results.iter().find_map(|r| r.get("musicDescriptionShelfRenderer")).map(|shelf| match at(shelf, "description/runs").and_then(Value::as_array) {
        Some(runs) => runs.iter().filter_map(|r| r["text"].as_str()).collect::<String>(),
        None => String::new(),
    });
    let songs = match results.first().and_then(|r| r.get("musicShelfRenderer")) {
        Some(shelf) => playlist_items(list(shelf, "contents"), false, false),
        None => Vec::new(),
    };
    let carousel = |title: &str, parse: fn(&Value) -> Value| -> Vec<Value> {
        results
            .iter()
            .filter_map(|r| r.get("musicCarouselShelfRenderer"))
            .find(|s| text(s, "header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text").is_some_and(|t| t.to_lowercase() == title))
            .map_or(Vec::new(), |s| list(s, "contents").iter().filter_map(|r| r.get(MTRIR)).map(|d| card(&parse(d))).collect())
    };
    Ok(json!({"name": text(header, TITLE_TEXT), "description": description, "thumbnail": best_thumb(at(header, THUMBNAILS)),
        "songs": songs.iter().map(track).collect::<Vec<_>>(), "albums": carousel("albums", parse_album), "singles": carousel("singles & eps", parse_single)}))
}

fn to_int(s: &str) -> Option<i64> {
    s.chars().filter(char::is_ascii_digit).collect::<String>().parse().ok()
}

fn album(c: &mut Client, browse_id: &str) -> R<Value> {
    if !browse_id.starts_with("MPRE") {
        return Err("Invalid album browseId provided, must start with MPRE.".into());
    }
    let response = c.send("browse", &json!({"browseId": browse_id}), "")?;
    let header = at(&response, &format!("{TWO_COLUMN}/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer")).ok_or("no album header")?;
    let title = text(header, TITLE_TEXT);
    let mut info = song_runs(list(header, "subtitle/runs").get(2..).unwrap_or_default(), false);
    let strapline = list(header, "straplineTextOne/runs");
    let artists = if strapline.is_empty() { Value::Null } else { json!(artists_runs(strapline)) };
    info.insert("artists".into(), artists.clone());
    let second = list(header, "secondSubtitle/runs");
    let track_count = (second.len() > 1).then(|| second[0]["text"].as_str().and_then(to_int)).flatten();
    let rows = list(&response, &format!("{TWO_COLUMN}/secondaryContents/sectionListRenderer/contents/0/musicShelfRenderer/contents"));
    let tracks: Vec<Value> = playlist_items(rows, true, false)
        .into_iter()
        .map(|mut t| {
            t["album"] = json!(title);
            if !truthy(t.get("artists")) {
                t["artists"] = artists.clone();
            }
            track(&t)
        })
        .collect();
    let album = json!({"artists": artists});
    Ok(json!({"title": title, "artist": artist_names(&album), "year": info.get("year").cloned().unwrap_or(json!("")),
        "thumbnail": best_thumb(at(header, THUMBNAILS)), "trackCount": if second.len() > 1 { json!(track_count) } else { json!(tracks.len()) }, "tracks": tracks}))
}

fn playlist(c: &mut Client, playlist_id: &str, limit: i64) -> R<(Value, Vec<Value>)> {
    let browse_id = if playlist_id.starts_with("VL") { playlist_id.to_string() } else { format!("VL{playlist_id}") };
    let response = c.send("browse", &json!({"browseId": browse_id}), "")?;
    let header_data = at(&response, &format!("{TWO_COLUMN}/tabs/0/tabRenderer/content/sectionListRenderer/contents/0"));
    let shelf = at(&response, &format!("{TWO_COLUMN}/secondaryContents/sectionListRenderer/contents/0/musicPlaylistShelfRenderer")).ok_or("no playlist shelf")?;
    if (playlist_id.starts_with("OLA") || playlist_id.starts_with("VLOLA")) && header_data.is_none() {
        let parse = |rows: &[Value]| playlist_items(rows, false, false);
        let mut tracks = parse(list(shelf, "contents"));
        tracks.extend(c.continuations_2025(list(shelf, "contents"), limit - tracks.len() as i64, &parse)?);
        let title = tracks.first().map_or(Value::Null, |t| or_null(at(t, "album/name")));
        return Ok((json!({"title": title, "thumbnail": "", "trackCount": tracks.len()}), tracks));
    }
    let header_data = header_data.ok_or("no playlist header")?;
    let header = match header_data.get("musicEditablePlaylistDetailHeaderRenderer") {
        Some(editable) => at(editable, "header/musicResponsiveHeaderRenderer"),
        None => header_data.get("musicResponsiveHeaderRenderer"),
    }
    .ok_or("no playlist header")?;
    let title: String = list(header, "title/runs").iter().filter_map(|r| r["text"].as_str()).collect();
    let second = list(header, "secondSubtitle/runs");
    let track_count = if second.is_empty() {
        Value::Null
    } else {
        let has_views = if second.len() > 3 { 2 } else { 0 };
        json!(second.get(has_views).and_then(|r| r["text"].as_str()).and_then(to_int))
    };
    let collaborative = text(header, "facepile/avatarStackViewModel/rendererContext/commandContext/onTap/innertubeCommand/showEngagementPanelEndpoint/identifier/tag") == Some("PAplaylist_collaborate");
    let parse = |rows: &[Value]| playlist_items(rows, false, collaborative);
    let mut tracks = Vec::new();
    if let Some(rows) = shelf.get("contents").and_then(Value::as_array) {
        tracks = parse(rows);
        tracks.extend(c.continuations_2025(rows, limit - tracks.len() as i64, &parse)?);
    }
    Ok((json!({"title": title, "thumbnail": best_thumb(at(header, THUMBNAILS)), "trackCount": track_count}), tracks))
}

fn library_contents<'a>(response: &'a Value, renderer: &str) -> Option<&'a Value> {
    match at(response, &format!("{SINGLE_COLUMN_TAB}/sectionListRenderer/contents")).and_then(Value::as_array) {
        None => {
            let tab = if list(response, "contents/singleColumnBrowseResultsRenderer/tabs").len() < 3 { 1 } else { 2 };
            at(response, &format!("contents/singleColumnBrowseResultsRenderer/tabs/{tab}/tabRenderer/content/sectionListRenderer/contents/0/{renderer}"))
        }
        Some(section) => match section.iter().find(|s| s.get("itemSectionRenderer").is_some()) {
            None => at(response, &format!("{SINGLE_COLUMN_TAB}/sectionListRenderer/contents/0/{renderer}")),
            Some(item) => at(item, &format!("itemSectionRenderer/contents/0/{renderer}")),
        },
    }
}

fn library(c: &mut Client, kind: &str, limit: i64) -> R<Vec<Value>> {
    let (browse_id, renderer, continuation) = match kind {
        "songs" => ("FEmusic_liked_videos", "musicShelfRenderer", "musicShelfContinuation"),
        "artists" => ("FEmusic_library_corpus_track_artists", "musicShelfRenderer", "musicShelfContinuation"),
        "albums" => ("FEmusic_liked_albums", "gridRenderer", "gridContinuation"),
        _ => ("FEmusic_liked_playlists", "gridRenderer", "gridContinuation"),
    };
    let body = json!({"browseId": browse_id});
    let response = c.send("browse", &body, "")?;
    let Some(results) = library_contents(&response, renderer) else { return Ok(Vec::new()) };
    let parse: Box<dyn Fn(&[Value]) -> Vec<Value>> = match kind {
        "songs" => Box::new(|rows| playlist_items(rows, false, false).iter().map(track).collect()),
        "artists" => Box::new(|rows| {
            rows.iter()
                .filter_map(|r| r.get(MRLIR))
                .map(|d| {
                    let subscribers = item_text(d, 1, 0).filter(|s| !s.is_empty()).map(|s| s.split(' ').next().unwrap_or_default());
                    let mut a = json!({"browseId": text(d, NAV_BROWSE_ID), "artist": item_text(d, 0, 0), "thumbnails": or_null(at(d, THUMBNAILS))});
                    if let Some(s) = subscribers {
                        a["subscribers"] = json!(s);
                    }
                    card(&a)
                })
                .collect()
        }),
        "albums" => Box::new(|rows| {
            rows.iter()
                .filter_map(|r| r.get(MTRIR))
                .map(|d| {
                    let mut a = json!({"browseId": text(d, TITLE_BROWSE_ID), "title": text(d, TITLE_TEXT), "thumbnails": or_null(at(d, THUMBNAIL_RENDERER)),
                        "playlistId": text(d, &format!("{MENU_ITEMS}/0/menuNavigationItemRenderer/navigationEndpoint/watchPlaylistEndpoint/playlistId"))});
                    if let Some(runs) = at(d, "subtitle/runs").and_then(Value::as_array) {
                        a["type"] = json!(text(d, "subtitle/runs/0/text"));
                        merge(&mut a, song_runs(runs.get(2..).unwrap_or_default(), false));
                    }
                    card(&a)
                })
                .collect()
        }),
        _ => Box::new(|rows| rows.iter().filter_map(|r| r.get(MTRIR)).map(|d| card(&parse_playlist(d))).collect()),
    };
    let mut rows = list(results, if renderer == "gridRenderer" { "items" } else { "contents" });
    // songs open with a shuffle row, playlists with "New playlist"
    if (kind == "songs" && rows.len() >= 2) || (kind == "playlists" && !rows.is_empty()) {
        rows = &rows[1..];
    }
    let mut items = parse(rows);
    let more = c.continuations("browse", &body, results, continuation, limit - items.len() as i64, "", &*parse)?;
    items.extend(more);
    Ok(items)
}

fn account_info(c: &mut Client) -> R<(String, String)> {
    let response = c.send("account/account_menu", &json!({}), "")?;
    let header = at(&response, "actions/0/openPopupAction/popup/multiPageMenuRenderer/header/activeAccountHeaderRenderer").ok_or("signed out")?;
    let name = text(header, "accountName/runs/0/text").ok_or("signed out")?;
    let photo = text(header, "accountPhoto/thumbnails/0/url").ok_or("signed out")?;
    Ok((name.into(), photo.into()))
}

/// innertube.py's _account_probe: a signed-in session needs LOGIN_INFO and a real account name from YTM.
fn account_probe(path: &Path, allow_oauth: bool) -> (bool, String, String) {
    let signed_in = read_cookies(path).is_some_and(|(_, c)| c.get("LOGIN_INFO").is_some_and(|v| !v.is_empty()));
    if !signed_in && (!allow_oauth || !(itube_oauth_path().exists() || oauth_path().exists())) {
        return (false, String::new(), String::new());
    }
    match authenticated_client(path, allow_oauth).map(|mut c| account_info(&mut c)) {
        Some(Ok((name, photo))) if !name.is_empty() => (true, name, photo),
        _ => (false, String::new(), String::new()),
    }
}

// ---- the cookie helper (ytmusic_auth.py, still python)

fn run_helper(args: &[&str], output: Option<&Path>, timeout: u64) -> Option<(bool, String)> {
    let helper = std::env::var("INIR_YTMUSIC_AUTH").ok()?;
    let mut cmd = Command::new("python3");
    cmd.arg(helper).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(o) = output {
        cmd.env("INIR_YTMUSIC_COOKIE_OUTPUT", o);
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        if let Some(status) = child.try_wait().ok()? {
            return Some((status.success(), reader.join().unwrap_or_default()));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn detect() -> Value {
    run_helper(&["detect"], None, 15).and_then(|(_, out)| serde_json::from_str(&out).ok()).unwrap_or(json!({}))
}

fn candidate_path() -> PathBuf {
    let _ = fs::create_dir_all(cfg_dir());
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    cfg_dir().join(format!("yt-cookies.candidate-{}{nanos:x}", std::process::id()))
}

fn extract_to(args: &[&str], candidate: &Path) -> bool {
    if candidate.exists() && fs::remove_file(candidate).is_err() {
        return false;
    }
    run_helper(args, Some(candidate), 90).is_some_and(|(ok, out)| {
        ok && serde_json::from_str::<Value>(&out).is_ok_and(|d| d["status"] == "success") && candidate.is_file()
    })
}

fn promote(candidate: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(candidate, fs::Permissions::from_mode(0o600)).is_ok() && fs::rename(candidate, cookie_path()).is_ok()
}

/// Live cookies rotate: a few tries land on a settled set. A failed try never replaces the working jar.
fn extract_and_probe(browser: &str, attempts: u32) -> (bool, String, String) {
    let candidate = candidate_path();
    let mut result = (false, String::new(), String::new());
    for i in 0..attempts {
        if extract_to(&[browser], &candidate) {
            let (auth, name, avatar) = account_probe(&candidate, false);
            if auth && promote(&candidate) {
                result = (true, name, avatar);
                break;
            }
        }
        if i + 1 < attempts {
            std::thread::sleep(Duration::from_millis(1500));
        }
    }
    let _ = fs::remove_file(&candidate);
    result
}

// ---- lyrics

fn lrclib_synced(title: &str, artist: &str, duration: i64) -> Option<String> {
    let q = |s: &str| net::quote(s, " ").replace(' ', "+");
    let url = format!("https://lrclib.net/api/search?track_name={}&artist_name={}", q(title), q(artist));
    let response = net::get(&url, &[("User-Agent", "iNiR (https://github.com/snowarch/inir)")], 10.0).ok().filter(|r| r.status < 400)?;
    let tracks: Vec<Value> = serde_json::from_slice(&response.body).ok()?;
    let mut candidates: Vec<&Value> = tracks.iter().filter(|t| truthy(t.get("syncedLyrics"))).collect();
    if duration > 0 {
        candidates.sort_by_key(|t| (t["duration"].as_f64().unwrap_or(0.0) - duration as f64).abs() as i64 * 1000);
    }
    candidates.first()?.get("syncedLyrics")?.as_str().map(String::from)
}

fn parse_lrc(lrc: &str) -> Vec<Value> {
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[(\d+):(\d+)(?:[.:](\d+))?\]").unwrap());
    let mut out: Vec<(f64, String)> = Vec::new();
    for raw in crate::pyrt::splitlines(lrc) {
        let stamps: Vec<regex::Captures> = TAG.captures_iter(raw).collect();
        let Some(last) = stamps.last() else { continue };
        let line = raw[last.get(0).unwrap().end()..].trim().to_string();
        for m in &stamps {
            let n = |i: usize| m.get(i).map_or(0.0, |g| g.as_str().parse::<f64>().unwrap_or(0.0));
            let frac = m.get(3).map_or(0.0, |f| n(3) / if f.as_str().len() == 3 { 1000.0 } else { 100.0 });
            out.push((((n(1) * 60.0 + n(2) + frac) * 100.0).round() / 100.0, line.clone()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(t, line)| json!({"t": t, "line": line})).collect()
}

fn lyrics_of(c: &mut Client, browse_id: &str) -> R<(String, String)> {
    let response = c.send("browse", &json!({"browseId": browse_id}), "")?;
    let shelf = at(&response, "contents/sectionListRenderer/contents/0/musicDescriptionShelfRenderer").ok_or("no lyrics")?;
    let lyrics = text(shelf, "description/runs/0/text").ok_or("no lyrics")?;
    Ok((lyrics.into(), text(shelf, "runs/0/text").unwrap_or_default().into()))
}

// ---- commands

fn emit_tracks(tracks: impl Iterator<Item = Value>) {
    let mut count = 0;
    for t in tracks.filter(|t| truthy(t.get("videoId"))) {
        emit(&t);
        count += 1;
    }
    emit(&json!({"_done": true, "count": count}));
}

fn cmd_oauth_poll(device_code: &str) {
    let token = match oauth_post(OAUTH_TOKEN_URL, &[("client_secret", TV_CLIENT_KEY), ("grant_type", "http://oauth.net/grant_type/device/1.0"), ("code", device_code)], TV_CLIENT_ID) {
        Ok(t) => t,
        Err(e) => {
            let lower = e.to_lowercase();
            if lower.contains("pending") || lower.contains("slow_down") {
                return emit(&json!({"status": "pending"}));
            }
            return emit(&json!({"status": "error", "error": e.chars().take(200).collect::<String>()}));
        }
    };
    // The token endpoint answers a pending device with {"error": "authorization_pending"}, not an HTTP error.
    if let Some(err) = token.get("error").filter(|e| truthy(Some(e))) {
        let err = err.as_str().map_or_else(|| err.to_string(), String::from);
        if err == "authorization_pending" || err == "slow_down" {
            return emit(&json!({"status": "pending"}));
        }
        return emit(&json!({"status": "error", "error": err.chars().take(200).collect::<String>()}));
    }
    if !truthy(token.get("access_token")) {
        return emit(&json!({"status": "pending"}));
    }
    let mut data = token;
    for (k, v) in [("scope", OAUTH_SCOPE), ("token_type", "Bearer")] {
        if data.get(k).is_none() {
            data[k] = json!(v);
        }
    }
    use std::os::unix::fs::PermissionsExt;
    let saved = fs::create_dir_all(cfg_dir())
        .and_then(|_| fs::write(itube_oauth_path(), serde_json::to_string_pretty(&data).unwrap_or_default()))
        .and_then(|_| fs::set_permissions(itube_oauth_path(), fs::Permissions::from_mode(0o600)));
    match saved {
        Ok(()) => emit(&json!({"status": "authorized"})),
        Err(e) => fail("oauth save failed", &e.to_string()),
    }
}

fn cmd_connect(browser: &str) {
    if !browser.is_empty() && browser != "auto" {
        let (auth, name, avatar) = extract_and_probe(browser, 3);
        return emit(&if auth { json!({"authenticated": true, "account": name, "avatar": avatar, "browser": browser}) } else { json!({"authenticated": false, "error": "not_logged_in"}) });
    }
    let found = detect();
    let detected: Vec<String> = found["browsers"].as_array().into_iter().flatten().filter_map(|b| b["id"].as_str().filter(|i| !i.is_empty()).map(String::from)).collect();
    // XDG defaults outlive removed browsers: the default goes first only when a cookie store was found for it.
    let mut order: Vec<String> = found["default"].as_str().filter(|d| detected.iter().any(|b| b == d)).map(String::from).into_iter().collect();
    for b in detected {
        if !order.contains(&b) {
            order.push(b);
        }
    }
    if order.is_empty() {
        return emit(&json!({"authenticated": false, "error": "no_browser"}));
    }
    for (i, b) in order.iter().enumerate() {
        let (auth, name, avatar) = extract_and_probe(b, if i == 0 { 3 } else { 1 });
        if auth {
            return emit(&json!({"authenticated": true, "account": name, "avatar": avatar, "browser": b}));
        }
    }
    emit(&json!({"authenticated": false, "error": "not_logged_in"}))
}

fn cmd_connect_manual(path: &str) {
    let candidate = candidate_path();
    let out = if !extract_to(&["import", path], &candidate) {
        json!({"authenticated": false, "error": "import_failed"})
    } else {
        match account_probe(&candidate, false) {
            (true, name, avatar) if promote(&candidate) => json!({"authenticated": true, "account": name, "avatar": avatar, "browser": "manual"}),
            _ => json!({"authenticated": false, "error": "not_logged_in"}),
        }
    };
    let _ = fs::remove_file(&candidate);
    emit(&out)
}

fn cmd_lyrics(video_id: &str, title: &str, artist: &str, duration: &str) {
    let dur = duration.trim().parse::<f64>().ok().filter(|d| d.is_finite()).map_or(0, |d| d as i64);
    let synced = (!title.is_empty() && !artist.is_empty()).then(|| lrclib_synced(title, artist, dur)).flatten().map(|l| parse_lrc(&l));
    let mut source = if synced.is_some() { "LrcLib".to_string() } else { String::new() };
    let mut yt = Yt::new();
    let mut plain = String::new();
    if let Ok(Some(id)) = yt.call(|c| watch_playlist(c, video_id, 1).map(|w| w.lyrics), |_| false) {
        if let Ok((text, src)) = yt.call(|c| lyrics_of(c, &id), |_| false) {
            plain = text;
            if source.is_empty() {
                source = src;
            }
        }
    }
    emit(&json!({"plain": plain, "synced": synced, "source": source}))
}

pub fn run_cli(args: &[String]) -> i32 {
    let Some(action) = args.first().map(String::as_str) else { fail("Usage: inir-accel innertube <command> [args]", "") };
    let rest: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    let need = match action {
        "oauth-poll" | "connect-manual" | "search" | "radio" | "artist" | "album" | "playlist" | "library" | "lyrics" | "song" => 1,
        "rate" => 2,
        "ping" | "auth-status" | "oauth-request" | "connect" | "detect-browsers" | "disconnect" | "logout" | "home" => 0,
        _ => fail(&format!("Unknown command: {action}"), ""),
    };
    if rest.len() < need {
        fail(&format!("'{action}' needs {need} argument(s)"), "");
    }
    let arg = |i: usize| rest.get(i).copied().unwrap_or_default();
    let not_empty = |v: &Vec<Value>| v.is_empty();
    match action {
        "ping" => {
            let _ = Yt::new();
            emit(&json!({"available": true, "version": concat!("inir-accel ", env!("CARGO_PKG_VERSION"))}));
        }
        "auth-status" => {
            let (auth, name, avatar) = account_probe(&cookie_path(), true);
            emit(&json!({"authenticated": auth, "account": name, "avatar": avatar}));
        }
        "oauth-request" => match oauth_post(OAUTH_CODE_URL, &[("scope", OAUTH_SCOPE)], TV_CLIENT_ID) {
            Ok(code) => emit(&json!({"device_code": get_or(&code, "device_code", json!("")), "user_code": get_or(&code, "user_code", json!("")),
                "verification_url": get_or(&code, "verification_url", json!("")), "interval": get_or(&code, "interval", json!(5)), "expires_in": get_or(&code, "expires_in", json!(1800))})),
            Err(e) => fail("oauth request failed", &e),
        },
        "oauth-poll" => cmd_oauth_poll(arg(0)),
        "connect" => cmd_connect(arg(0)),
        "connect-manual" => cmd_connect_manual(arg(0)),
        "detect-browsers" => match run_helper(&["detect"], None, 15) {
            Some((_, out)) if !out.is_empty() => {
                print!("{out}");
            }
            Some(_) => println!(r#"{{"browsers": [], "default": null}}"#),
            None => emit(&json!({"browsers": [], "default": null, "error": "ytmusic_auth.py did not answer"})),
        },
        "disconnect" | "logout" => {
            let paths = if action == "disconnect" { vec![cookie_path(), itube_oauth_path()] } else { vec![itube_oauth_path()] };
            for p in paths {
                let _ = fs::remove_file(p);
            }
            emit(&if action == "disconnect" { json!({"connected": false}) } else { json!({"status": "ok"}) });
        }
        "search" => {
            const VALID: [&str; 7] = ["songs", "videos", "albums", "artists", "playlists", "community_playlists", "featured_playlists"];
            let filter = if VALID.contains(&arg(1)) { arg(1) } else { "songs" };
            let results = Yt::new().call(|c| search(c, arg(0), filter), not_empty).unwrap_or_else(|e| fail("search failed", &e));
            let mut count = 0;
            for card in results.iter().map(card).filter(|c| !(c["type"] == "song" && !truthy(c.get("videoId")))) {
                emit(&card);
                count += 1;
            }
            emit(&json!({"_done": true, "count": count}));
        }
        "home" => {
            let limit = arg(0).parse().unwrap_or(3);
            let shelves = Yt::new().call(|c| home(c, limit), not_empty).unwrap_or_else(|e| fail("home failed", &e));
            let shelves: Vec<Value> = shelves
                .iter()
                .map(|s| {
                    let items: Vec<Value> = s["contents"].as_array().into_iter().flatten().filter(|c| truthy(Some(c))).map(card).collect();
                    json!({"title": get_or(s, "title", json!("")), "items": items})
                })
                .collect();
            emit(&json!({"shelves": shelves}));
        }
        "radio" => {
            let watch = Yt::new().call(|c| watch_playlist(c, arg(0), 50), |_| false).unwrap_or_else(|e| fail("radio failed", &e));
            emit(&json!({"lyricsId": watch.lyrics.unwrap_or_default(), "_meta": true}));
            emit_tracks(watch.tracks.iter().map(track));
        }
        "artist" => emit(&Yt::new().call(|c| artist(c, arg(0)), |_| false).unwrap_or_else(|e| fail("artist failed", &e))),
        "album" => emit(&Yt::new().call(|c| album(c, arg(0)), |_| false).unwrap_or_else(|e| fail("album failed", &e))),
        "playlist" => {
            let (mut meta, tracks) = Yt::new().call(|c| playlist(c, arg(0), 200), |_| false).unwrap_or_else(|e| fail("playlist failed", &e));
            meta["_meta"] = json!(true);
            emit(&meta);
            emit_tracks(tracks.iter().map(track));
        }
        "library" => {
            let Some(mut c) = authenticated_client(&cookie_path(), true) else { fail("auth required", "") };
            let kind = arg(0);
            if !["songs", "artists", "albums", "playlists"].contains(&kind) {
                fail("unknown library kind", "");
            }
            let limit = Some(arg(1)).filter(|l| !l.is_empty()).map_or(50, |l| l.parse().unwrap_or(50));
            let items = library(&mut c, kind, limit).unwrap_or_else(|e| fail("library failed", &e));
            emit(&json!({"kind": kind, "items": items}));
        }
        "rate" => {
            let Some(mut c) = authenticated_client(&cookie_path(), true) else { fail("auth required", "") };
            let rating = if ["LIKE", "DISLIKE", "INDIFFERENT"].contains(&arg(1)) { arg(1) } else { "INDIFFERENT" };
            let endpoint = match rating {
                "LIKE" => "like/like",
                "DISLIKE" => "like/dislike",
                _ => "like/removelike",
            };
            c.send(endpoint, &json!({"target": {"videoId": arg(0)}}), "").unwrap_or_else(|e| fail("rate failed", &e));
            emit(&json!({"status": "ok", "videoId": arg(0), "rating": rating}));
        }
        "lyrics" => cmd_lyrics(arg(0), arg(1), arg(2), arg(3)),
        "song" => {
            let days = now().div_euclid(86400);
            let body = json!({"playbackContext": {"contentPlaybackContext": {"signatureTimestamp": days - 1}}, "video_id": arg(0)});
            let song = Yt::new().call(|c| c.send("player", &body, "").map(|r| or_null(r.get("videoDetails"))), |v| !truthy(Some(v))).unwrap_or_else(|e| fail("song failed", &e));
            let length = match &song["lengthSeconds"] {
                Value::String(s) => s.parse().unwrap_or(0),
                v => v.as_i64().unwrap_or(0),
            };
            emit(&json!({"videoId": get_or(&song, "videoId", json!(arg(0))), "title": get_or(&song, "title", json!("")), "artist": get_or(&song, "author", json!("")),
                "thumbnail": best_thumb(at(&song, "thumbnail/thumbnails")), "duration": length, "url": format!("https://music.youtube.com/watch?v={}", arg(0))}));
        }
        _ => unreachable!(),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_and_dates() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
        assert_eq!(civil(0), (1970, 1, 1));
    }

    #[test]
    fn youtube_cookie_beats_google() {
        let path = std::env::temp_dir().join(format!("inir-yt-cookies-{}", std::process::id()));
        let row = |d: &str, n: &str, v: &str| format!("{d}\tTRUE\t/\tTRUE\t0\t{n}\t{v}\n");
        let jar = [row(".youtube.com", "SID", "yt"), row(".google.com", "SID", "google"), row(".google.com", "__Secure-3PAPISID", "x"), row(".youtube.com", "NID", "n")].concat();
        fs::write(&path, format!("# Netscape\n{jar}")).unwrap();
        let (header, cookies) = read_cookies(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(header, "SID=yt; __Secure-3PAPISID=x");
        assert_eq!(cookies["NID"], "n");
        assert_eq!(civil(20754), (2026, 10, 28));
    }
}
