//! What a line-for-line port of one of inir's Python helpers needs from Python itself: an uncaught exception ends
//! the run the way the interpreter does (traceback on stderr, exit 1, stdout already written), `int(x, 16)`,
//! `str()`/`repr()` of JSON values, text-mode reads (strict UTF-8, universal newlines), OSError texts, and `re`
//! (`\s` with Python's extra separators, `re.sub` replacement templates).

use crate::material::py::{self, Json};
use regex::{Captures, Regex};
use std::ffi::CStr;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// A Python exception: its type as the traceback's last line names it, and its message.
#[derive(Debug)]
pub struct Exc {
    pub ty: String,
    pub msg: String,
}

pub type R<T> = Result<T, Exc>;

pub fn exc(ty: &str, msg: impl Into<String>) -> Exc {
    Exc { ty: ty.to_string(), msg: msg.into() }
}

/// The end of a run that raised: Python flushes stdout, prints the traceback and exits 1.
pub fn die(e: Exc) -> i32 {
    let _ = io::stdout().flush();
    let mut err = io::stderr();
    let _ = writeln!(err, "Traceback (most recent call last):\n  (inir-accel)");
    if e.msg.is_empty() {
        let _ = writeln!(err, "{}", e.ty);
    } else {
        let _ = writeln!(err, "{}: {}", e.ty, e.msg);
    }
    1
}

pub fn type_name(v: &Json) -> &'static str {
    match v {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::Arr(_) => "list",
        Json::Obj(_) => "dict",
    }
}

/// `v.<attr>` on a value that has to be a str.
pub fn as_str<'a>(v: &'a Json, attr: &str) -> R<&'a str> {
    match v {
        Json::Str(s) => Ok(s),
        other => Err(exc("AttributeError", format!("'{}' object has no attribute '{attr}'", type_name(other)))),
    }
}

fn printable(c: char) -> bool {
    let u = c as u32;
    !(u < 0x20
        || (0x7f..=0xa0).contains(&u)
        || u == 0xad
        || u == 0x1680
        || (0x2000..=0x200f).contains(&u)
        || (0x2028..=0x202f).contains(&u)
        || (0x205f..=0x2064).contains(&u)
        || u == 0x3000
        || (0xd800..=0xf8ff).contains(&u)
        || u == 0xfeff
        || (0xfff9..=0xfffb).contains(&u)
        || u >= 0xf0000)
}

/// `repr(str)`.
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if printable(c) => out.push(c),
            c if (c as u32) < 0x100 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push_str(&format!("\\U{:08x}", c as u32)),
        }
    }
    out.push(quote);
    out
}

fn float_str(f: f64) -> String {
    if f.is_nan() {
        "nan".into()
    } else if f.is_infinite() {
        if f > 0.0 { "inf".into() } else { "-inf".into() }
    } else {
        py::float_repr(f)
    }
}

/// `repr(v)`.
pub fn repr(v: &Json) -> String {
    match v {
        Json::Str(s) => repr_str(s),
        other => str_of(other),
    }
}

/// `str(v)`, as an f-string prints it.
pub fn str_of(v: &Json) -> String {
    match v {
        Json::Null => "None".into(),
        Json::Bool(b) => if *b { "True" } else { "False" }.into(),
        Json::Int(i) => i.to_string(),
        Json::Float(f) => float_str(*f),
        Json::Str(s) => s.clone(),
        Json::Arr(a) => format!("[{}]", a.iter().map(repr).collect::<Vec<_>>().join(", ")),
        Json::Obj(o) => format!("{{{}}}", o.iter().map(|(k, v)| format!("{}: {}", repr_str(k), repr(v))).collect::<Vec<_>>().join(", ")),
    }
}

/// `int(s, 16)`: whitespace around, a sign, `0x`, single underscores between digits.
pub fn int16(s: &str) -> R<i64> {
    let bad = || exc("ValueError", format!("invalid literal for int() with base 16: {}", repr_str(s)));
    let t = py::strip(s);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let (t, after_prefix) = if t.len() >= 2 && (t.starts_with("0x") || t.starts_with("0X")) { (&t[2..], true) } else { (t, false) };
    let t = if after_prefix { t.strip_prefix('_').unwrap_or(t) } else { t };
    if t.is_empty() || t.starts_with('_') || t.ends_with('_') || t.contains("__") {
        return Err(bad());
    }
    let mut v: i64 = 0;
    for c in t.chars().filter(|&c| c != '_') {
        let d = c.to_digit(16).filter(|_| c.is_ascii()).ok_or_else(bad)?;
        v = v.checked_mul(16).and_then(|v| v.checked_add(d as i64)).ok_or_else(bad)?;
    }
    Ok(if neg { -v } else { v })
}

/// `s[a:b]` by code points (Python slices never fail).
pub fn slice(s: &str, a: usize, b: Option<usize>) -> String {
    let n = s.chars().count();
    let b = b.unwrap_or(n).min(n);
    if a >= b {
        return String::new();
    }
    s.chars().skip(a).take(b - a).collect()
}

// ---------------------------------------------------------------- files

fn strerror(code: i32) -> String {
    unsafe { CStr::from_ptr(libc::strerror(code)) }.to_string_lossy().into_owned()
}

/// An OSError the way Python raises it for a call on `path`.
pub fn os_exc(e: &io::Error, path: &Path) -> Exc {
    let Some(code) = e.raw_os_error() else {
        return exc("OSError", e.to_string());
    };
    let ty = match code {
        libc::ENOENT => "FileNotFoundError",
        libc::EACCES | libc::EPERM => "PermissionError",
        libc::EISDIR => "IsADirectoryError",
        libc::ENOTDIR => "NotADirectoryError",
        libc::EEXIST => "FileExistsError",
        _ => "OSError",
    };
    exc(ty, format!("[Errno {code}] {}: {}", strerror(code), repr_str(&path.to_string_lossy())))
}

/// `open(path).read()` in text mode: strict UTF-8, `\r\n` and `\r` read as `\n`.
pub fn read_text(path: &Path) -> R<String> {
    let bytes = fs::read(path).map_err(|e| os_exc(&e, path))?;
    let text = String::from_utf8(bytes).map_err(|e| {
        let at = e.utf8_error().valid_up_to();
        let byte = e.as_bytes()[at];
        exc("UnicodeDecodeError", format!("'utf-8' codec can't decode byte 0x{byte:02x} in position {at}: invalid start byte"))
    })?;
    Ok(if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text })
}

pub fn write_text(path: &Path, text: &str) -> R<()> {
    fs::write(path, text).map_err(|e| os_exc(&e, path))
}

pub fn append_text(path: &Path, text: &str) -> R<()> {
    let mut f = fs::OpenOptions::new().append(true).create(true).open(path).map_err(|e| os_exc(&e, path))?;
    f.write_all(text.as_bytes()).map_err(|e| os_exc(&e, path))
}

/// `os.makedirs(path, exist_ok=True)`.
pub fn makedirs(path: &Path) -> R<()> {
    if path.as_os_str().is_empty() || path.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(path).map_err(|e| {
        if path.exists() { exc("FileExistsError", format!("[Errno 17] File exists: {}", repr_str(&path.to_string_lossy()))) } else { os_exc(&e, path) }
    })
}

/// `os.path.exists` / `Path.exists`: follows links, any error is false.
pub fn exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

/// `json.load(open(path))`.
pub fn json_load(path: &Path) -> R<Json> {
    let text = read_text(path)?;
    py::json_loads(&text).ok_or_else(|| exc("json.decoder.JSONDecodeError", "Expecting value"))
}

/// `datetime.now().strftime(fmt)` for the `%Y %m %d %H %M %S` it is used with.
pub fn now_strftime(fmt: &str) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    unsafe { libc::localtime_r(&t, &mut tm) };
    fmt.replace("%Y", &format!("{:04}", tm.tm_year + 1900))
        .replace("%m", &format!("{:02}", tm.tm_mon + 1))
        .replace("%d", &format!("{:02}", tm.tm_mday))
        .replace("%H", &format!("{:02}", tm.tm_hour))
        .replace("%M", &format!("{:02}", tm.tm_min))
        .replace("%S", &format!("{:02}", tm.tm_sec))
}

// ---------------------------------------------------------------- re

/// A Python pattern: `\s` also takes the separators `str.isspace` counts (0x1c-0x1f).
pub fn re(pat: &str) -> Regex {
    Regex::new(&pat.replace(r"\s", r"[\s\x1c-\x1f]")).expect("pattern")
}

enum Piece {
    Lit(String),
    Group(usize),
}

/// `re.sub`'s replacement template, checked against the pattern's groups before anything matches.
fn template(repl: &str, groups: usize) -> R<Vec<Piece>> {
    let chars: Vec<char> = repl.chars().collect();
    let mut out = vec![];
    let mut lit = String::new();
    let mut i = 0;
    let bad = |m: String| exc("re.error", m);
    while i < chars.len() {
        let c = chars[i];
        if c != '\\' {
            lit.push(c);
            i += 1;
            continue;
        }
        let at = i;
        let Some(&n) = chars.get(i + 1) else {
            return Err(bad(format!("bad escape (end of pattern) at position {at}")));
        };
        i += 2;
        let group = |g: usize, out: &mut Vec<Piece>, lit: &mut String| -> R<()> {
            if g > groups {
                return Err(bad(format!("invalid group reference {g} at position {}", at + 1)));
            }
            if !lit.is_empty() {
                out.push(Piece::Lit(std::mem::take(lit)));
            }
            out.push(Piece::Group(g));
            Ok(())
        };
        match n {
            'g' => {
                let close = chars[i..].iter().position(|&c| c == '>');
                if chars.get(i) != Some(&'<') || close.is_none() {
                    return Err(bad(format!("missing < at position {}", at + 2)));
                }
                let name: String = chars[i + 1..i + close.unwrap()].iter().collect();
                i += close.unwrap() + 1;
                let g = name.parse::<usize>().map_err(|_| exc("IndexError", format!("unknown group name {}", repr_str(&name))))?;
                group(g, &mut out, &mut lit)?;
            }
            '0' => {
                let mut v = 0u32;
                let mut k = 0;
                while k < 2 && chars.get(i).is_some_and(|c| ('0'..='7').contains(c)) {
                    v = v * 8 + chars[i].to_digit(8).unwrap();
                    i += 1;
                    k += 1;
                }
                lit.push(char::from_u32(v).unwrap());
            }
            '1'..='9' => {
                let d1 = chars.get(i).copied();
                let d2 = chars.get(i + 1).copied();
                let oct = |c: Option<char>| c.is_some_and(|c| ('0'..='7').contains(&c));
                if ('1'..='7').contains(&n) && oct(d1) && oct(d2) {
                    let v = n.to_digit(8).unwrap() * 64 + d1.unwrap().to_digit(8).unwrap() * 8 + d2.unwrap().to_digit(8).unwrap();
                    if v > 0o377 {
                        return Err(bad(format!("octal escape value \\{n}{}{} outside of range 0-0o377 at position {at}", d1.unwrap(), d2.unwrap())));
                    }
                    lit.push(char::from_u32(v).unwrap());
                    i += 2;
                } else {
                    let mut g = n.to_digit(10).unwrap() as usize;
                    if d1.is_some_and(|c| c.is_ascii_digit()) {
                        g = g * 10 + d1.unwrap().to_digit(10).unwrap() as usize;
                        i += 1;
                    }
                    group(g, &mut out, &mut lit)?;
                }
            }
            'a' => lit.push('\x07'),
            'b' => lit.push('\x08'),
            'f' => lit.push('\x0c'),
            'n' => lit.push('\n'),
            'r' => lit.push('\r'),
            't' => lit.push('\t'),
            'v' => lit.push('\x0b'),
            '\\' => lit.push('\\'),
            c if c.is_ascii_alphabetic() => return Err(bad(format!("bad escape \\{c} at position {at}"))),
            c => {
                lit.push('\\');
                lit.push(c);
            }
        }
    }
    if !lit.is_empty() {
        out.push(Piece::Lit(lit));
    }
    Ok(out)
}

/// `re.sub(pattern, repl, text, count)` (count 0: every match).
pub fn sub(rx: &Regex, repl: &str, text: &str, count: usize) -> R<String> {
    let pieces = template(repl, rx.captures_len() - 1)?;
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (n, caps) in rx.captures_iter(text).enumerate() {
        if count > 0 && n == count {
            break;
        }
        let m = caps.get(0).unwrap();
        out.push_str(&text[last..m.start()]);
        expand(&pieces, &caps, &mut out);
        last = m.end();
    }
    out.push_str(&text[last..]);
    Ok(out)
}

fn expand(pieces: &[Piece], caps: &Captures, out: &mut String) {
    for p in pieces {
        match p {
            Piece::Lit(s) => out.push_str(s),
            Piece::Group(g) => out.push_str(caps.get(*g).map_or("", |m| m.as_str())),
        }
    }
}

/// A replacement used as plain text after Python checked it (a lookahead pattern done by hand).
pub fn template_text(repl: &str) -> R<String> {
    let mut out = String::new();
    for p in template(repl, 0)? {
        if let Piece::Lit(s) = p {
            out.push_str(&s);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int16_like_python() {
        assert_eq!(int16("ff").unwrap(), 255);
        assert_eq!(int16("-1").unwrap(), -1);
        assert_eq!(int16(" f").unwrap(), 15);
        assert_eq!(int16("0x1f").unwrap(), 31);
        assert!(int16("").is_err());
        assert!(int16("0x").is_err());
        assert!(int16("_f").is_err());
        assert!(int16("zz").is_err());
        assert_eq!(int16("zz").unwrap_err().msg, "invalid literal for int() with base 16: 'zz'");
    }

    #[test]
    fn sub_templates() {
        let rx = re(r"(?m)^(gui:.*)");
        assert_eq!(sub(&rx, r"\1\nX", "a\ngui: 1\n", 1).unwrap(), "a\ngui: 1\nX\n");
        assert!(sub(&re("a"), r"\q", "a", 0).is_err());
        assert!(sub(&re("a"), r"\1", "a", 0).is_err());
        assert_eq!(sub(&re("a"), r"\-", "a", 0).unwrap(), r"\-");
    }

    #[test]
    fn str_and_repr() {
        assert_eq!(str_of(&Json::Arr(vec![Json::Str("#fff".into())])), "['#fff']");
        assert_eq!(str_of(&Json::Null), "None");
        assert_eq!(repr_str("it's"), "\"it's\"");
    }
}
