//! A Chrome DevTools Protocol client over a bare websocket, for the live themes of Electron apps (Pear, Spotify):
//! the page list over HTTP, then one websocket per page with masked client frames.

use crate::net;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const MAX_MESSAGE: usize = 1 << 20;

/// `http://127.0.0.1:{port}/{path}` as JSON (`json` or `json/list`), None when nothing answers.
pub(crate) fn targets(port: u16, path: &str) -> Option<Value> {
    let r = net::get(&format!("http://127.0.0.1:{port}/{path}"), &[], 2.0).ok().filter(|r| r.status < 400)?;
    serde_json::from_slice(&r.body).ok()
}

pub(crate) fn base64(bytes: &[u8]) -> String {
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

pub(crate) struct Session {
    s: TcpStream,
}

impl Session {
    pub(crate) fn open(ws_url: &str) -> Option<Session> {
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
        Some(Session { s })
    }

    pub(crate) fn send(&mut self, id: u64, method: &str, params: Value) -> Option<()> {
        let call = json!({"id": id, "method": method, "params": params});
        self.s.write_all(&frame(0x1, call.to_string().as_bytes())).ok()
    }

    /// The reply to `id`; events and other replies on the way are skipped.
    pub(crate) fn call(&mut self, id: u64, method: &str, params: Value) -> Option<Value> {
        self.send(id, method, params)?;
        loop {
            let message = read_message(&mut self.s).ok()?;
            if let Ok(v) = serde_json::from_slice::<Value>(&message)
                && v.get("id").and_then(Value::as_u64) == Some(id)
            {
                return Some(v);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.s.write_all(&frame(0x8, &1000u16.to_be_bytes()));
    }
}

pub(crate) fn evaluate(ws_url: &str, expression: &str) -> Option<Value> {
    Session::open(ws_url)?.call(1, "Runtime.evaluate", json!({"expression": expression, "returnByValue": true}))
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
