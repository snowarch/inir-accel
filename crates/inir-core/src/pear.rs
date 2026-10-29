//! Live theme for Pear Desktop (YouTube Music), ported from `pear-css-inject.py`: the CSS goes in as a <style> in every
//! page over the Chrome DevTools Protocol (Pear runs with --remote-debugging-port; iNiR uses 9223).
//! Prints injected/unchanged/failed; exit 0 when every reachable page has the CSS.

use crate::net;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const STYLE_ID: &str = "inir-pear-theme";
const MAX_MESSAGE: usize = 1 << 20;

#[derive(PartialEq)]
enum Outcome {
    Injected,
    Same,
    Failed,
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            out.push(if i <= chunk.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        b.iter_mut().enumerate().for_each(|(i, x)| *x = (t >> (8 * (i % 16))) as u8);
    }
    b
}

/// A client frame: final, masked, as RFC 6455 asks of clients.
fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0x80 | opcode];
    match payload.len() {
        n if n < 126 => f.push(0x80 | n as u8),
        n if n <= 0xffff => {
            f.push(0x80 | 126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            f.push(0x80 | 127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    let mask: [u8; 4] = random();
    f.extend_from_slice(&mask);
    f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

/// One text message (continuations joined), answering pings on the way.
fn read_message(s: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
    let mut message = Vec::new();
    loop {
        let mut head = [0u8; 2];
        s.read_exact(&mut head)?;
        let (fin, opcode) = (head[0] & 0x80 != 0, head[0] & 0x0f);
        let len = match head[1] & 0x7f {
            126 => {
                let mut b = [0u8; 2];
                s.read_exact(&mut b)?;
                u64::from(u16::from_be_bytes(b))
            }
            127 => {
                let mut b = [0u8; 8];
                s.read_exact(&mut b)?;
                u64::from_be_bytes(b)
            }
            n => u64::from(n),
        };
        if len as usize > MAX_MESSAGE || message.len() + len as usize > MAX_MESSAGE {
            return Err(bad("message too big"));
        }
        let mut mask = [0u8; 4];
        if head[1] & 0x80 != 0 {
            s.read_exact(&mut mask)?;
        }
        let mut payload = vec![0u8; len as usize];
        s.read_exact(&mut payload)?;
        payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= mask[i % 4]);
        match opcode {
            0x8 => return Err(bad("closed")),
            0x9 => s.write_all(&frame(0xA, &payload))?,
            0xA => {}
            _ => {
                message.extend_from_slice(&payload);
                if fin {
                    return Ok(message);
                }
            }
        }
    }
}

fn evaluate(ws_url: &str, expression: &str) -> Option<Value> {
    let rest = ws_url.strip_prefix("ws://")?;
    let (host, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    let addr = host.to_socket_addrs().ok()?.next()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    s.set_write_timeout(Some(Duration::from_secs(3))).ok()?;
    let key = base64(&random::<16>());
    let hello = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n");
    s.write_all(hello.as_bytes()).ok()?;
    let mut answer = Vec::new();
    let mut byte = [0u8; 1];
    while !answer.ends_with(b"\r\n\r\n") {
        if answer.len() > 16 * 1024 || s.read(&mut byte).ok()? == 0 {
            return None;
        }
        answer.push(byte[0]);
    }
    if !String::from_utf8_lossy(&answer).lines().next().is_some_and(|l| l.split(' ').nth(1) == Some("101")) {
        return None;
    }
    let call = json!({"id": 1, "method": "Runtime.evaluate", "params": {"expression": expression, "returnByValue": true}});
    s.write_all(&frame(0x1, call.to_string().as_bytes())).ok()?;
    let reply = serde_json::from_slice::<Value>(&read_message(&mut s).ok()?).ok()?;
    let _ = s.write_all(&frame(0x8, &1000u16.to_be_bytes()));
    Some(reply)
}

fn inject(ws_url: &str, css: &str) -> Outcome {
    let lit = serde_json::to_string(css).unwrap_or_default();
    let js = format!(
        "(function(){{var e=document.getElementById('{STYLE_ID}');if(e&&e.textContent==={lit})return 'same';if(e)e.remove();\
         var s=document.createElement('style');s.id='{STYLE_ID}';s.textContent={lit};\
         if(document.head)document.head.appendChild(s);return !!document.head;}})()"
    );
    match evaluate(ws_url, &js).as_ref().and_then(|r| r.pointer("/result/result/value")) {
        Some(Value::String(v)) if v == "same" => Outcome::Same,
        Some(Value::Bool(true)) => Outcome::Injected,
        _ => Outcome::Failed,
    }
}

/// `pear-css-inject <css-file> [--port PORT]`
pub fn run_cli(args: &[String]) -> i32 {
    let mut port = 9223u16;
    let mut file = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => match it.next().and_then(|p| p.parse().ok()) {
                Some(p) => port = p,
                None => {
                    eprintln!("usage: inir-accel pear-css-inject <css-file> [--port PORT]");
                    return 2;
                }
            },
            _ if file.is_none() => file = Some(a.clone()),
            _ => {
                eprintln!("usage: inir-accel pear-css-inject <css-file> [--port PORT]");
                return 2;
            }
        }
    }
    let Some(file) = file else {
        eprintln!("usage: inir-accel pear-css-inject <css-file> [--port PORT]");
        return 2;
    };
    let css = match std::fs::read_to_string(&file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cannot read CSS file: {e}");
            return 1;
        }
    };
    let Some(pages) = net::get(&format!("http://127.0.0.1:{port}/json"), &[], 2.0).ok().filter(|r| r.status < 400).and_then(|r| serde_json::from_slice::<Value>(&r.body).ok()) else {
        return 1;
    };
    let targets: Vec<&str> = pages
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            let url = p["url"].as_str().unwrap_or_default();
            url.contains("music.youtube.com") || url.to_lowercase().contains("youtube-music") || url.starts_with("about:blank")
        })
        .filter_map(|p| p["webSocketDebuggerUrl"].as_str().filter(|w| !w.is_empty()))
        .collect();
    if targets.is_empty() {
        return 1;
    }
    let results: Vec<Outcome> = std::thread::scope(|scope| {
        let handles: Vec<_> = targets.iter().map(|ws| scope.spawn(|| inject(ws, &css))).collect();
        handles.into_iter().map(|h| h.join().unwrap_or(Outcome::Failed)).collect()
    });
    let injected = results.contains(&Outcome::Injected);
    let same = results.contains(&Outcome::Same);
    println!("{}", if injected { "injected" } else if same { "unchanged" } else { "failed" });
    if injected || same { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_pads() {
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"the sample nonce"), "dGhlIHNhbXBsZSBub25jZQ==");
    }
}
