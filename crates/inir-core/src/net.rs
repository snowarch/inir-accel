//! HTTP for the ports of inir's python helpers, through `curl` (already an inir dependency) so inir-accel carries
//! no TLS stack. The URL and headers reach curl on stdin, never argv: they can carry API keys.

use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct Response {
    pub status: u16,
    /// curl's `%{header_json}`: lowercase names, each with its values.
    pub headers: Value,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name.to_ascii_lowercase())?.as_array()?.last()?.as_str()
    }
}

/// A body makes the request a POST. Text and form fields go in the config on stdin, so a secret in them stays out of argv.
pub enum Body<'a> {
    None,
    Text(&'a str),
    File(&'a Path),
    /// multipart/form-data: text fields, then one file as (field, path, mime).
    Form(&'a [(&'a str, &'a str)], (&'a str, &'a Path, &'a str)),
}

/// urllib.parse.quote: everything but letters, digits, `_.-~` and `safe` percent-encoded as UTF-8.
pub fn quote(s: &str, safe: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"_.-~".contains(&b) || safe.as_bytes().contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace(['\r', '\n'], "").replace('\\', "\\\\").replace('"', "\\\""))
}

fn curl(url: &str, headers: &[(&str, &str)], body: &Body, timeout: f64, extra: &[&str]) -> Result<Vec<u8>, String> {
    let mut config = format!("url = {}\n", quoted(url));
    for (k, v) in headers {
        config += &format!("header = {}\n", quoted(&format!("{k}: {v}")));
    }
    let path = |p: &Path| p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    match body {
        Body::None => {}
        Body::Text(text) => config += &format!("data-binary = {}\n", quoted(text)),
        Body::File(file) => config += &format!("data-binary = {}\n", quoted(&format!("@{}", file.to_string_lossy()))),
        Body::Form(fields, (name, file, mime)) => {
            for (k, v) in *fields {
                config += &format!("form-string = {}\n", quoted(&format!("{k}={v}")));
            }
            config += &format!("form = {}\n", quoted(&format!("{name}=@\"{}\";type={mime}", path(file))));
        }
    }
    // Like a socket timeout: gives up when connecting or the transfer stalls that long, not on a slow big body.
    let stall = (timeout.ceil() as u64).max(1).to_string();
    let mut child = Command::new("curl")
        .args(["-sS", "-L", "--connect-timeout", &timeout.to_string(), "--speed-time", &stall, "--speed-limit", "1", "-K", "-"])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    child.stdin.take().unwrap().write_all(config.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("curl: ").to_string())
    }
}

const TRAILER: &[u8] = b"\n<inir-accel-net>";

/// A request that follows redirects; `Err` is curl's own failure (DNS, TLS, timeout), an HTTP error comes back as a status.
pub fn request(url: &str, headers: &[(&str, &str)], body: Body, timeout: f64) -> Result<Response, String> {
    let mut out = curl(url, headers, &body, timeout, &["-w", "\n<inir-accel-net>%{http_code} %{header_json}"])?;
    let cut = out.windows(TRAILER.len()).rposition(|w| w == TRAILER).unwrap_or(out.len());
    let trailer = String::from_utf8_lossy(&out[(cut + TRAILER.len()).min(out.len())..]).into_owned();
    out.truncate(cut);
    let (status, headers) = trailer.split_once(' ').unwrap_or((trailer.as_str(), ""));
    Ok(Response { status: status.trim().parse().unwrap_or(0), headers: serde_json::from_str(headers).unwrap_or(Value::Null), body: out })
}

pub fn get(url: &str, headers: &[(&str, &str)], timeout: f64) -> Result<Response, String> {
    request(url, headers, Body::None, timeout)
}

/// A download straight to `dest`; an HTTP error fails it.
pub fn download(url: &str, headers: &[(&str, &str)], dest: &Path, timeout: f64) -> Result<(), String> {
    let dest = dest.to_str().ok_or("the download path is not UTF-8")?;
    curl(url, headers, &Body::None, timeout, &["-f", "-o", dest]).map(|_| ())
}
