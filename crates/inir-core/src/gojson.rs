//! JSON the way Go's `encoding/json` reads and writes it, for the ports of Go helpers: objects keep their key order
//! (a repeated key is decoded twice, the last one wins), field names match case-insensitively, `null` leaves a value
//! alone, and strings are written with Go's HTML-safe escapes.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// The literal as written: Go parses it only when a field asks for a number.
    Number(String),
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

pub fn parse(src: &str) -> Result<Value, String> {
    let mut p = Parser { b: src.as_bytes(), i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("invalid character {:?} after top-level value", p.b[p.i] as char));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn err<T>(&self) -> Result<T, String> {
        match self.b.get(self.i) {
            Some(c) => Err(format!("invalid character {:?} looking for beginning of value", *c as char)),
            None => Err("unexpected end of JSON input".into()),
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 10000 {
            return Err("exceeded max depth".into());
        }
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Object(out));
                }
                loop {
                    self.ws();
                    if self.b.get(self.i) != Some(&b'"') {
                        return self.err();
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return self.err();
                    }
                    self.i += 1;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    out.push((k, v));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Object(out));
                        }
                        _ => return self.err(),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::Array(out));
                }
                loop {
                    self.ws();
                    out.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Array(out));
                        }
                        _ => return self.err(),
                    }
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.lit("true", Value::Bool(true)),
            Some(b'f') => self.lit("false", Value::Bool(false)),
            Some(b'n') => self.lit("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => self.err(),
        }
    }

    fn lit(&mut self, word: &str, v: Value) -> Result<Value, String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err("invalid literal".into())
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        let digits = |p: &mut Self| {
            let s = p.i;
            while p.i < p.b.len() && p.b[p.i].is_ascii_digit() {
                p.i += 1;
            }
            p.i - s
        };
        if self.b[self.i] == b'-' {
            self.i += 1;
        }
        match self.b.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => return self.err(),
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if digits(self) == 0 {
                return self.err();
            }
        }
        if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if digits(self) == 0 {
                return self.err();
            }
        }
        Ok(Value::Number(String::from_utf8_lossy(&self.b[start..self.i]).into_owned()))
    }

    fn hex4(&mut self) -> Option<u32> {
        let s = self.b.get(self.i..self.i + 4)?;
        let v = u32::from_str_radix(std::str::from_utf8(s).ok()?, 16).ok()?;
        if s.iter().all(|c| c.is_ascii_hexdigit()) {
            self.i += 4;
            Some(v)
        } else {
            None
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&c) = self.b.get(self.i) else {
                return Err("unexpected end of JSON input".into());
            };
            match c {
                b'"' => {
                    self.i += 1;
                    // Go swaps each invalid UTF-8 byte for U+FFFD.
                    return Ok(String::from_utf8_lossy(&out).into_owned());
                }
                b'\\' => {
                    self.i += 1;
                    let Some(&e) = self.b.get(self.i) else {
                        return Err("unexpected end of JSON input".into());
                    };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let Some(hi) = self.hex4() else {
                                return Err("invalid character in \\u hexadecimal character escape".into());
                            };
                            if (0xD800..0xDC00).contains(&hi) {
                                let save = self.i;
                                if self.b.get(self.i) == Some(&b'\\') && self.b.get(self.i + 1) == Some(&b'u') {
                                    self.i += 2;
                                    match self.hex4() {
                                        Some(lo) if (0xDC00..0xE000).contains(&lo) => {
                                            char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)).unwrap_or('\u{FFFD}')
                                        }
                                        _ => {
                                            self.i = save;
                                            '\u{FFFD}'
                                        }
                                    }
                                } else {
                                    '\u{FFFD}'
                                }
                            } else {
                                char::from_u32(hi).unwrap_or('\u{FFFD}')
                            }
                        }
                        _ => return Err(format!("invalid character {:?} in string escape code", e as char)),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                0..=0x1f => return Err("invalid character in string literal".into()),
                _ => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }
}

/// The value Go decodes into a struct field named `name`: the exact key if present, else a case-insensitive one, the
/// last occurrence winning in both cases (Go assigns each key in order).
pub fn fields<'a>(obj: &'a [(String, Value)], name: &str) -> Vec<&'a Value> {
    obj.iter()
        .filter(|(k, _)| k == name || k.to_lowercase() == name.to_lowercase())
        .map(|(_, v)| v)
        .collect()
}

/// Go's float64 decode of a JSON number literal.
pub fn number(v: &Value) -> Result<Option<f64>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Number(s) => match s.parse::<f64>() {
            Ok(f) if f.is_finite() => Ok(Some(f)),
            _ => Err(format!("json: cannot unmarshal number {s} into Go value of type float64")),
        },
        other => Err(format!("json: cannot unmarshal {} into Go value of type float64", kind(other))),
    }
}

pub fn string(v: &Value) -> Result<Option<String>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Str(s) => Ok(Some(s.clone())),
        other => Err(format!("json: cannot unmarshal {} into Go value of type string", kind(other))),
    }
}

pub fn boolean(v: &Value) -> Result<Option<bool>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(*b)),
        other => Err(format!("json: cannot unmarshal {} into Go value of type bool", kind(other))),
    }
}

pub fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::Str(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// A string as Go's encoder writes it, HTML-safe: `<`, `>`, `&`, U+2028 and U+2029 escaped.
pub fn write_str(out: &mut String, s: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' | '>' | '&' | '\u{0}'..='\u{1f}' => {
                let b = ch as u32;
                out.push_str("\\u00");
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0xf) as usize] as char);
            }
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A float64 as Go's encoder writes it: the shortest round-trip digits, exponent form only below 1e-6 or from 1e21.
pub fn write_f64(out: &mut String, f: f64) {
    let a = f.abs();
    if a != 0.0 && (a < 1e-6 || a >= 1e21) {
        let s = format!("{f:e}");
        let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
        out.push_str(mant);
        out.push('e');
        if let Some(neg) = exp.strip_prefix('-') {
            out.push('-');
            out.push_str(neg);
        } else {
            out.push('+');
            if exp.len() < 2 {
                out.push('0');
            }
            out.push_str(exp);
        }
    } else {
        out.push_str(&format!("{f}"));
    }
}
