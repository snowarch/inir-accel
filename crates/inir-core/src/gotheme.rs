//! `scripts/colors/themegencommon` from the shell's Go, line for line, plus the slice of `encoding/json` the
//! themegens print through: `MarshalIndent` of maps (keys sorted), slices and Go's HTML-safe strings.
//! This code was written by: Hikari.
use crate::gojson;
use std::collections::BTreeMap;
use std::fs;

pub type Colors = BTreeMap<String, String>;

/// `ReadStringMapJSON`: the top-level string values of a JSON object; anything else is an error.
pub fn read_string_map_json(path: &str) -> Result<Colors, String> {
    let data = fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&data);
    match gojson::parse(&text)? {
        gojson::Value::Object(fields) => {
            let mut out = Colors::new();
            // Go decodes into a map, so a repeated key keeps its last value.
            for (k, v) in fields {
                match v {
                    gojson::Value::Str(s) => {
                        out.insert(k, s);
                    }
                    _ => {
                        out.remove(&k);
                    }
                }
            }
            Ok(out)
        }
        gojson::Value::Null => Ok(Colors::new()),
        _ => Err("json: cannot unmarshal into map".into()),
    }
}

/// `ParseSCSS`: `$name: #rrggbb;` and `$name: True|False;` lines, the regex unanchored like Go's.
pub fn parse_scss(path: &str) -> Result<Colors, String> {
    let data = fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&data);
    let mut out = Colors::new();
    for line in text.split('\n') {
        if let Some((k, v)) = scss_match(go_trim_space(line)) {
            out.insert(k.to_string(), v.to_string());
        }
    }
    Ok(out)
}

/// `strings.TrimSpace`: Unicode white space at both ends.
pub fn go_trim_space(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace())
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The leftmost match of `\$(\w+):\s*(#[A-Fa-f0-9]{6}|True|False);`.
fn scss_match(line: &str) -> Option<(&str, &str)> {
    let b = line.as_bytes();
    for start in 0..b.len() {
        if b[start] != b'$' {
            continue;
        }
        let mut i = start + 1;
        while i < b.len() && is_word(b[i]) {
            i += 1;
        }
        let name_end = i;
        if name_end == start + 1 {
            continue;
        }
        // \w+ is greedy but may backtrack; ':' is not a word byte, so the only way on is right after the run.
        if i >= b.len() || b[i] != b':' {
            continue;
        }
        i += 1;
        while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\x0c' | b'\r') {
            i += 1;
        }
        let rest = &line[i..];
        let value_len = if rest.len() >= 7 && rest.as_bytes()[0] == b'#' && rest.as_bytes()[1..7].iter().all(u8::is_ascii_hexdigit) {
            7
        } else if rest.starts_with("True") {
            4
        } else if rest.starts_with("False") {
            5
        } else {
            continue;
        };
        if rest.as_bytes().get(value_len) != Some(&b';') {
            continue;
        }
        return Some((&line[start + 1..name_end], &rest[..value_len]));
    }
    None
}

pub fn merge(base: &Colors, overlays: &[&Colors]) -> Colors {
    let mut out = base.clone();
    for o in overlays {
        for (k, v) in o.iter() {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// `Pick`: the value when present and not empty.
pub fn pick(m: &Colors, key: &str, fallback: &str) -> String {
    match m.get(key) {
        Some(v) if !v.is_empty() => v.clone(),
        _ => fallback.to_string(),
    }
}

/// `fmt.Sscanf(s, "%x", &v)`: leading spaces skipped (a newline stops the scan), an optional sign, then hex
/// digits; nothing scanned leaves 0.
pub fn hex_byte(s: &str) -> i64 {
    hex_byte_b(s.as_bytes())
}

pub fn hex_byte_b(b: &[u8]) -> i64 {
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    let mut any = false;
    while i < b.len() && b[i].is_ascii_hexdigit() {
        v = v * 16 + (b[i] as char).to_digit(16).unwrap() as i64;
        any = true;
        i += 1;
    }
    if !any {
        return 0;
    }
    if neg {
        -v
    } else {
        v
    }
}

pub fn clamp_int(v: i64, lo: i64, hi: i64) -> i64 {
    v.clamp(lo, hi)
}

/// `HexToRGB`: `#` trimmed once, then three two-byte slices (by bytes, like Go).
pub fn hex_to_rgb(color: &str) -> (i64, i64, i64) {
    let c = color.strip_prefix('#').unwrap_or(color).as_bytes();
    if c.len() < 6 {
        return (0, 0, 0);
    }
    let part = |a: usize| hex_byte(&String::from_utf8_lossy(&c[a..a + 2]));
    (part(0), part(2), part(4))
}

pub fn rgb_to_hex(r: i64, g: i64, b: i64) -> String {
    format!("#{:02x}{:02x}{:02x}", clamp_int(r, 0, 255), clamp_int(g, 0, 255), clamp_int(b, 0, 255))
}

/// Go's `int(f)`: toward zero (the themegens never feed it NaN or out-of-range values).
pub fn go_int(f: f64) -> i64 {
    f.trunc() as i64
}

pub fn blend(c1: &str, c2: &str, factor: f64) -> String {
    let (r1, g1, b1) = hex_to_rgb(c1);
    let (r2, g2, b2) = hex_to_rgb(c2);
    let mix = |a: i64, b: i64| go_int(a as f64 + ((b - a) as f64 * factor));
    rgb_to_hex(mix(r1, r2), mix(g1, g2), mix(b1, b2))
}

pub fn adjust_lightness_scale(color: &str, factor: f64) -> String {
    let (r, g, b) = hex_to_rgb(color);
    let f = |c: i64| {
        if factor > 1.0 {
            255.min(go_int(c as f64 + (255 - c) as f64 * (factor - 1.0)))
        } else {
            0.max(go_int(c as f64 * factor))
        }
    };
    rgb_to_hex(f(r), f(g), f(b))
}

/// An `os` error's text as Go prints it: the bare errno text, lower case.
pub fn go_err(e: &std::io::Error) -> String {
    let s = e.to_string();
    // Go prints the bare errno text ("permission denied"); Rust appends " (os error N)" and capitalises.
    let s = s.split(" (os error").next().unwrap_or(&s).to_string();
    let mut c = s.chars();
    c.next().map(|f| f.to_lowercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The JSON values the themegens build: Go maps (sorted on output), slices and scalars.
#[derive(Clone, Debug)]
pub enum J {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Arr(Vec<J>),
    Map(BTreeMap<String, J>),
    /// A Go struct: its fields in declaration order.
    Obj(Vec<(String, J)>),
}

impl From<&str> for J {
    fn from(s: &str) -> J {
        J::Str(s.to_string())
    }
}
impl From<String> for J {
    fn from(s: String) -> J {
        J::Str(s)
    }
}
impl From<&String> for J {
    fn from(s: &String) -> J {
        J::Str(s.clone())
    }
}
impl From<f64> for J {
    fn from(f: f64) -> J {
        J::Num(f)
    }
}
impl From<bool> for J {
    fn from(b: bool) -> J {
        J::Bool(b)
    }
}

/// A Go `map[string]any` literal.
#[macro_export]
macro_rules! jmap {
    ($($k:expr => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m = std::collections::BTreeMap::<String, $crate::gotheme::J>::new();
        $( m.insert(String::from($k), $crate::gotheme::J::from($v)); )*
        $crate::gotheme::J::Map(m)
    }};
}

/// `json.Marshal(v)`: compact, failing like Go on a value JSON cannot hold.
pub fn marshal(v: &J) -> Result<String, String> {
    check_finite(v)?;
    let mut out = String::new();
    write_compact(&mut out, v);
    Ok(out)
}

fn check_finite(v: &J) -> Result<(), String> {
    match v {
        J::Num(f) if !f.is_finite() => Err(format!("json: unsupported value: {}", if f.is_nan() { "NaN" } else if *f > 0.0 { "+Inf" } else { "-Inf" })),
        J::Arr(items) => items.iter().try_for_each(check_finite),
        J::Map(m) => m.values().try_for_each(check_finite),
        J::Obj(fields) => fields.iter().try_for_each(|(_, v)| check_finite(v)),
        _ => Ok(()),
    }
}

fn write_compact(out: &mut String, v: &J) {
    let pairs = |out: &mut String, it: &mut dyn Iterator<Item = (&String, &J)>| {
        out.push('{');
        for (i, (k, item)) in it.enumerate() {
            if i > 0 {
                out.push(',');
            }
            gojson::write_str(out, k);
            out.push(':');
            write_compact(out, item);
        }
        out.push('}');
    };
    match v {
        J::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(out, item);
            }
            out.push(']');
        }
        J::Map(m) => pairs(out, &mut m.iter()),
        J::Obj(f) => pairs(out, &mut f.iter().map(|(k, v)| (k, v))),
        scalar => write(out, scalar, "", "", 0),
    }
}

/// `json.MarshalIndent(v, prefix, indent)`.
pub fn marshal_indent(v: &J, prefix: &str, indent: &str) -> String {
    let mut out = String::new();
    write(&mut out, v, prefix, indent, 0);
    out
}

fn newline(out: &mut String, prefix: &str, indent: &str, depth: usize) {
    out.push('\n');
    out.push_str(prefix);
    for _ in 0..depth {
        out.push_str(indent);
    }
}

fn write(out: &mut String, v: &J, prefix: &str, indent: &str, depth: usize) {
    match v {
        J::Str(s) => gojson::write_str(out, s),
        J::Num(f) => gojson::write_f64(out, *f),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Null => out.push_str("null"),
        J::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, prefix, indent, depth + 1);
                write(out, item, prefix, indent, depth + 1);
            }
            newline(out, prefix, indent, depth);
            out.push(']');
        }
        J::Map(_) | J::Obj(_) => {
            let fields: Vec<(&String, &J)> = match v {
                J::Map(m) => m.iter().collect(),
                J::Obj(f) => f.iter().map(|(k, v)| (k, v)).collect(),
                _ => unreachable!(),
            };
            if fields.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, item)) in fields.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, prefix, indent, depth + 1);
                gojson::write_str(out, k);
                out.push_str(": ");
                write(out, item, prefix, indent, depth + 1);
            }
            newline(out, prefix, indent, depth);
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sscanf_hex_like_go() {
        assert_eq!(hex_byte("ff"), 255);
        assert_eq!(hex_byte("1g"), 1);
        assert_eq!(hex_byte("zz"), 0);
        assert_eq!(hex_byte("-1"), -1);
        assert_eq!(blend("#000000", "#ffffff", 0.5), "#7f7f7f");
    }

    #[test]
    fn scss_regex() {
        assert_eq!(scss_match("$primary: #A1b2C3;"), Some(("primary", "#A1b2C3")));
        assert_eq!(scss_match("x $darkmode:True; y"), Some(("darkmode", "True")));
        assert_eq!(scss_match("$a: #12345;"), None);
        assert_eq!(scss_match("$a: #1234567;"), None);
        assert_eq!(scss_match("$-x: #123456; $b: #abcdef;"), Some(("b", "#abcdef")));
    }
}
