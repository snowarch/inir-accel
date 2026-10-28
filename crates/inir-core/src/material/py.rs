//! Python's semantics wherever the original's output depends on them: float `%`, `repr`, `json.dump`/`json.load`,
//! what `float()` and `int()` accept, `str.isspace`, and `os.path`'s normalisation.

/// `a % b` on Python floats: the result takes the divisor's sign.
pub fn fmod(a: f64, b: f64) -> f64 {
    let m = a % b;
    if m != 0.0 {
        if (b < 0.0) != (m < 0.0) { m + b } else { m }
    } else {
        0.0f64.copysign(b)
    }
}

/// `str.isspace` for one character.
pub fn is_space(c: char) -> bool {
    matches!(c, '\t'..='\r' | '\x1c'..='\x1f' | ' ' | '\u{85}' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
        | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

/// `str.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `repr(float)`: the shortest digits that round-trip, fixed between 1e-4 and 1e16, `1e-05` style outside.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let sci = format!("{x:e}");
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let (neg, mant) = match mant.strip_prefix('-') {
        Some(m) => (true, m),
        None => (false, mant),
    };
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1;
    let mut out = String::with_capacity(digits.len() + 8);
    if neg {
        out.push('-');
    }
    if -4 < decpt && decpt <= 16 {
        if decpt <= 0 {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-decpt) as usize));
            out.push_str(&digits);
        } else if decpt as usize >= digits.len() {
            out.push_str(&digits);
            out.extend(std::iter::repeat_n('0', decpt as usize - digits.len()));
            out.push_str(".0");
        } else {
            out.push_str(&digits[..decpt as usize]);
            out.push('.');
            out.push_str(&digits[decpt as usize..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push(if exp < 0 { '-' } else { '+' });
        out.insert(out.len() - 1, 'e');
        out.push_str(&format!("{:02}", exp.abs()));
    }
    out
}

/// Digits with Python's underscores (one, between two digits) removed, or `None` where Python refuses them.
fn without_underscores(s: &str, is_digit: impl Fn(u8) -> bool) -> Option<String> {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in b.iter().enumerate() {
        if c == b'_' {
            let ok = i > 0 && is_digit(b[i - 1]) && i + 1 < b.len() && is_digit(b[i + 1]);
            if !ok {
                return None;
            }
        } else {
            out.push(c as char);
        }
    }
    Some(out)
}

/// `float(s)`.
pub fn parse_float(s: &str) -> Option<f64> {
    let t = strip(s);
    if !t.is_ascii() {
        return None;
    }
    let t = without_underscores(t, |c| c.is_ascii_digit())?;
    let body = t.trim_start_matches(['+', '-']);
    if t.len() - body.len() > 1 {
        return None;
    }
    let lower = body.to_ascii_lowercase();
    if matches!(lower.as_str(), "inf" | "infinity" | "nan") {
        let v = if lower == "nan" { f64::NAN } else { f64::INFINITY };
        return Some(if t.starts_with('-') { -v } else { v });
    }
    // Rust's grammar is Python's past the words above, but it also takes "infinity"-like spellings Python refuses.
    if !body.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return None;
    }
    t.parse::<f64>().ok()
}

/// `int(s)` in base 10, within i64.
pub fn parse_int(s: &str) -> Option<i64> {
    let t = strip(s);
    if !t.is_ascii() {
        return None;
    }
    let (neg, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if body.is_empty() || !body.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    let digits = without_underscores(body, |c| c.is_ascii_digit())?;
    if !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: i64 = digits.parse().ok()?;
    Some(if neg { -v } else { v })
}

/// `int(s, 16) & 0xff`: Python's ints are unbounded and negative ones are two's complement all the way up.
pub fn parse_hex_byte(s: &str) -> Option<u8> {
    let t = strip(s);
    if !t.is_ascii() {
        return None;
    }
    let (neg, mut body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut lead_underscore_ok = false;
    if body.len() >= 2 && (body.starts_with("0x") || body.starts_with("0X")) {
        body = &body[2..];
        lead_underscore_ok = true;
    }
    if lead_underscore_ok && body.starts_with('_') {
        body = &body[1..];
    }
    if body.is_empty() || !body.as_bytes()[0].is_ascii_hexdigit() {
        return None;
    }
    let digits = without_underscores(body, |c| c.is_ascii_hexdigit())?;
    if !digits.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let low = u8::from_str_radix(&digits[digits.len().saturating_sub(2)..], 16).ok()?;
    Some(if neg { low.wrapping_neg() } else { low })
}

// ---------------------------------------------------------------- json

/// A JSON value as Python's `json` sees it: objects keep insertion order, a repeated key keeps its first place.
#[derive(Clone, Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(b) => *b,
            Json::Int(i) => *i != 0,
            Json::Float(f) => *f != 0.0,
            Json::Str(s) => !s.is_empty(),
            Json::Arr(a) => !a.is_empty(),
            Json::Obj(o) => !o.is_empty(),
        }
    }
}

/// `json.loads`: NaN and Infinity allowed, raw control characters in strings refused (strict mode).
pub fn json_loads(text: &str) -> Option<Json> {
    let mut p = Parser { s: text.as_bytes(), i: 0, text };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return None;
    }
    Some(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    text: &'a str,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn eat(&mut self, lit: &str) -> bool {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }
    fn value(&mut self) -> Option<Json> {
        match *self.s.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut obj: Vec<(String, Json)> = Vec::new();
                self.ws();
                if self.eat("}") {
                    return Some(Json::Obj(obj));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return None;
                    }
                    let k = self.string()?;
                    self.ws();
                    if !self.eat(":") {
                        return None;
                    }
                    self.ws();
                    let v = self.value()?;
                    match obj.iter_mut().find(|(kk, _)| *kk == k) {
                        Some(slot) => slot.1 = v,
                        None => obj.push((k, v)),
                    }
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        return Some(Json::Obj(obj));
                    }
                    return None;
                }
            }
            b'[' => {
                self.i += 1;
                let mut arr = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Some(Json::Arr(arr));
                }
                loop {
                    self.ws();
                    arr.push(self.value()?);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("]") {
                        return Some(Json::Arr(arr));
                    }
                    return None;
                }
            }
            b'"' => self.string().map(Json::Str),
            b'n' if self.eat("null") => Some(Json::Null),
            b't' if self.eat("true") => Some(Json::Bool(true)),
            b'f' if self.eat("false") => Some(Json::Bool(false)),
            b'N' if self.eat("NaN") => Some(Json::Float(f64::NAN)),
            b'I' if self.eat("Infinity") => Some(Json::Float(f64::INFINITY)),
            b'-' if self.s[self.i..].starts_with(b"-Infinity") => {
                self.i += 9;
                Some(Json::Float(f64::NEG_INFINITY))
            }
            _ => self.number(),
        }
    }
    fn number(&mut self) -> Option<Json> {
        let start = self.i;
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        match self.s.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(c) if c.is_ascii_digit() => {
                while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                    self.i += 1;
                }
            }
            _ => return None,
        }
        let mut float = false;
        if self.s.get(self.i) == Some(&b'.') && self.s.get(self.i + 1).is_some_and(u8::is_ascii_digit) {
            float = true;
            self.i += 1;
            while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
            }
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            let save = self.i;
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                float = true;
                while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                    self.i += 1;
                }
            } else {
                self.i = save;
            }
        }
        let lit = &self.text[start..self.i];
        if float {
            lit.parse().ok().map(Json::Float)
        } else {
            Some(lit.parse::<i128>().map(Json::Int).unwrap_or_else(|_| Json::Float(lit.parse().unwrap_or(f64::INFINITY))))
        }
    }
    fn string(&mut self) -> Option<String> {
        self.i += 1;
        let mut out: Vec<u16> = Vec::new();
        let mut text = String::new();
        let flush = |out: &mut Vec<u16>, text: &mut String| {
            if !out.is_empty() {
                text.push_str(&String::from_utf16_lossy(out));
                out.clear();
            }
        };
        loop {
            let c = *self.s.get(self.i)?;
            match c {
                b'"' => {
                    self.i += 1;
                    flush(&mut out, &mut text);
                    return Some(text);
                }
                b'\\' => {
                    let e = *self.s.get(self.i + 1)?;
                    self.i += 2;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\x08',
                        b'f' => '\x0c',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hex = self.text.get(self.i..self.i + 4)?;
                            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                                return None;
                            }
                            out.push(u16::from_str_radix(hex, 16).ok()?);
                            self.i += 4;
                            continue;
                        }
                        _ => return None,
                    };
                    flush(&mut out, &mut text);
                    text.push(ch);
                }
                0..=0x1f => return None,
                _ => {
                    flush(&mut out, &mut text);
                    let rest = &self.text[self.i..];
                    let ch = rest.chars().next()?;
                    text.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
    }
}

/// `json.dump(value, f, indent=2)` with `ensure_ascii`.
pub fn json_dump(v: &Json) -> String {
    let mut out = String::new();
    dump_into(v, 0, &mut out);
    out
}

fn dump_into(v: &Json, depth: usize, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::Float(f) => out.push_str(&if f.is_nan() {
            "NaN".to_string()
        } else if f.is_infinite() {
            if *f > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
        } else {
            float_repr(*f)
        }),
        Json::Str(s) => json_str(s, out),
        Json::Arr(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                out.extend(std::iter::repeat_n(' ', 2 * (depth + 1)));
                dump_into(x, depth + 1, out);
            }
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', 2 * depth));
            out.push(']');
        }
        Json::Obj(o) => {
            if o.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                out.extend(std::iter::repeat_n(' ', 2 * (depth + 1)));
                json_str(k, out);
                out.push_str(": ");
                dump_into(x, depth + 1, out);
            }
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', 2 * depth));
            out.push('}');
        }
    }
}

/// `json.dumps(dict_of_str)` with the default separators, as `iris-seeds.json` is written.
pub fn json_dumps_flat(pairs: &[(&str, String)]) -> String {
    let mut out = String::from("{");
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        json_str(k, &mut out);
        out.push_str(": ");
        json_str(v, &mut out);
    }
    out.push('}');
    out
}

fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------- os.path

/// `os.path.expanduser` for `~` and `~/...`.
pub fn expanduser(p: &str) -> String {
    if p == "~" || p.starts_with("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        let home = if home.len() > 1 { home.trim_end_matches('/').to_string() } else { home };
        return format!("{home}{}", &p[1..]);
    }
    p.to_string()
}

/// `os.path.normpath` (posix).
pub fn normpath(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let mut initial = if p.starts_with('/') { 1 } else { 0 };
    if initial == 1 && p.starts_with("//") && !p.starts_with("///") {
        initial = 2;
    }
    let mut parts: Vec<&str> = Vec::new();
    for comp in p.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp != ".." || (initial == 0 && parts.is_empty()) || parts.last() == Some(&"..") {
            parts.push(comp);
        } else if !parts.is_empty() {
            parts.pop();
        }
    }
    let joined = parts.join("/");
    let out = format!("{}{}", "/".repeat(initial), joined);
    if out.is_empty() { ".".into() } else { out }
}

/// `os.path.abspath`.
pub fn abspath(p: &str) -> String {
    if p.starts_with('/') {
        normpath(p)
    } else {
        let cwd = std::env::current_dir().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
        normpath(&join(&cwd, p))
    }
}

/// `os.path.join(a, b)`.
pub fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `os.path.dirname`.
pub fn dirname(p: &str) -> String {
    let i = p.rfind('/').map(|i| i + 1).unwrap_or(0);
    let head = &p[..i];
    if !head.is_empty() && head.bytes().any(|b| b != b'/') {
        head.trim_end_matches('/').to_string()
    } else {
        head.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_matches_python() {
        for (x, want) in [(0.0, "0.0"), (-0.0, "-0.0"), (0.4, "0.4"), (100.0, "100.0"), (1e16, "1e+16"),
            (1e15, "1000000000000000.0"), (1e-5, "1e-05"), (0.0001, "0.0001"), (1.5e-7, "1.5e-07"),
            (123456789.125, "123456789.125"), (2.5e20, "2.5e+20"), (0.1 + 0.2, "0.30000000000000004")] {
            assert_eq!(float_repr(x), want, "{x}");
        }
    }

    #[test]
    fn numbers_like_python() {
        assert_eq!(parse_float(" 1_0.5 "), Some(10.5));
        assert_eq!(parse_float("1__0"), None);
        assert_eq!(parse_float("-INF"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_float("infinit"), None);
        assert_eq!(parse_int("+6_4"), Some(64));
        assert_eq!(parse_int("6.4"), None);
        assert_eq!(parse_hex_byte("a5ff"), Some(0xff));
        assert_eq!(parse_hex_byte("-1"), Some(0xff));
        assert_eq!(parse_hex_byte("0x"), None);
        assert_eq!(parse_hex_byte(""), None);
    }

    #[test]
    fn paths_like_python() {
        assert_eq!(normpath("/a//b/./c/../d/"), "/a/b/d");
        assert_eq!(normpath("//a"), "//a");
        assert_eq!(normpath("../x/.."), "..");
        assert_eq!(dirname("/a/b/c.txt"), "/a/b");
        assert_eq!(dirname("/c"), "/");
    }
}
