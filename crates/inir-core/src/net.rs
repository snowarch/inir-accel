//! HTTP for the ports of inir's python helpers, through `curl` (already an inir dependency) so inir-accel carries
//! no TLS stack. The URL and headers reach curl on stdin, never argv: they can carry API keys.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct Response {
    pub status: u16,
    pub retry_after: Option<f64>,
    pub body: Vec<u8>,
}

/// urllib.parse.quote: everything but letters, digits, `_.-~` and `/` percent-encoded as UTF-8.
pub fn quote(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn curl(url: &str, headers: &[(&str, &str)], timeout: f64, extra: &[&str]) -> Result<Vec<u8>, String> {
    let quoted = |s: &str| format!("\"{}\"", s.replace(['\r', '\n'], "").replace('\\', "\\\\").replace('"', "\\\""));
    let mut config = format!("url = {}\n", quoted(url));
    for (k, v) in headers {
        config += &format!("header = {}\n", quoted(&format!("{k}: {v}")));
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

/// A GET that follows redirects; `Err` is curl's own failure (DNS, TLS, timeout), an HTTP error comes back as a status.
pub fn get(url: &str, headers: &[(&str, &str)], timeout: f64) -> Result<Response, String> {
    let mut body = curl(url, headers, timeout, &["-w", "\n%{http_code} %header{retry-after}"])?;
    let cut = body.iter().rposition(|&b| b == b'\n').unwrap_or(0);
    let trailer = String::from_utf8_lossy(&body[cut..]).trim().to_string();
    body.truncate(cut);
    let mut parts = trailer.split_whitespace();
    let status = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let retry_after = parts.next().and_then(|s| s.parse().ok());
    Ok(Response { status, retry_after, body })
}

/// A download straight to `dest`; an HTTP error fails it.
pub fn download(url: &str, headers: &[(&str, &str)], dest: &Path, timeout: f64) -> Result<(), String> {
    let dest = dest.to_str().ok_or("the download path is not UTF-8")?;
    curl(url, headers, timeout, &["-f", "-o", dest]).map(|_| ())
}
