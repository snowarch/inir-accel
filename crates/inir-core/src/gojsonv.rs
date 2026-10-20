//! `encoding/json` v1 the way Go 1.27 runs it, on top of `jsontext`: the syntax check with the messages it prints
//! (`v2_scanner.go` rewrites jsontext's wording back to v1's), and `Unmarshal` into the two shapes the VSCode
//! themegen reads, `map[string]any` and `[]map[string]any`.
//! This code was written by: Hikari.
use crate::gotheme::J;
use std::collections::BTreeMap;

const MAX_DEPTH: usize = 10000;

enum Err {
    Eof,
    Msg(String),
}

/// `checkValid`: nil, or the `SyntaxError` text.
pub fn check_valid(b: &[u8]) -> Result<(), String> {
    let run = || -> Result<(), Err> {
        let pos = ws(b, 0);
        if pos >= b.len() {
            return Err(Err::Eof);
        }
        let pos = ws(b, value(b, pos, 1)?);
        if pos < b.len() {
            return Err(invalid_char(b, pos, "after top-level value"));
        }
        Ok(())
    };
    run().map_err(|e| match e {
        Err::Eof => "unexpected end of JSON input".to_string(),
        Err::Msg(m) => legacy(&m),
    })
}

/// `transformSyntacticError`: v1's names for jsontext's places.
fn legacy(msg: &str) -> String {
    let mut m = msg.to_string();
    if let Some(i) = m.find(" (expecting") {
        if !m.contains(" in literal") {
            m.truncate(i);
        }
    }
    for (from, to) in [
        ("object name", "object key"),
        ("at start of value", "looking for beginning of value"),
        ("at start of string", "looking for beginning of object key string"),
        ("after object value", "after object key:value pair"),
        ("in number", "in numeric literal"),
    ] {
        m = m.replace(from, to);
    }
    m
}

fn ws(b: &[u8], mut pos: usize) -> usize {
    while pos < b.len() && matches!(b[pos], b' ' | b'\t' | b'\r' | b'\n') {
        pos += 1;
    }
    pos
}

/// `utf8.DecodeRune`: the rune and its width, `None` with width 1 for anything invalid.
fn decode_rune(b: &[u8]) -> (Option<char>, usize) {
    let Some(&x) = b.first() else { return (None, 0) };
    if x < 0x80 {
        return (Some(x as char), 1);
    }
    let (need, lo, hi) = match x {
        0xc2..=0xdf => (2, 0x80, 0xbf),
        0xe0 => (3, 0xa0, 0xbf),
        0xed => (3, 0x80, 0x9f),
        0xe1..=0xef => (3, 0x80, 0xbf),
        0xf0 => (4, 0x90, 0xbf),
        0xf4 => (4, 0x80, 0x8f),
        0xf1..=0xf3 => (4, 0x80, 0xbf),
        _ => return (None, 1),
    };
    if b.len() < need || !(lo..=hi).contains(&b[1]) || b[2..need].iter().any(|c| !(0x80..=0xbf).contains(c)) {
        return (None, 1);
    }
    (std::str::from_utf8(&b[..need]).ok().and_then(|s| s.chars().next()), need)
}

/// `utf8.FullRune`: false only for a valid encoding cut short by the end of the input.
fn full_rune(b: &[u8]) -> bool {
    let Some(&x) = b.first() else { return false };
    let (need, lo, hi) = match x {
        0xc2..=0xdf => (2, 0x80, 0xbf),
        0xe0 => (3, 0xa0, 0xbf),
        0xed => (3, 0x80, 0x9f),
        0xe1..=0xef => (3, 0x80, 0xbf),
        0xf0 => (4, 0x90, 0xbf),
        0xf4 => (4, 0x80, 0x8f),
        0xf1..=0xf3 => (4, 0x80, 0xbf),
        _ => return true,
    };
    if b.len() >= need {
        return true;
    }
    if b.len() > 1 && !(lo..=hi).contains(&b[1]) {
        return true;
    }
    if b.len() > 2 && !(0x80..=0xbf).contains(&b[2]) {
        return true;
    }
    false
}

/// `unicode.IsPrint` for what a settings file can hold: every assigned graphic rune, minus the spaces, controls and
/// format characters. Unassigned code points count as printable here (Go's tables say otherwise; no editor writes them).
fn is_print(r: char) -> bool {
    let c = r as u32;
    if c < 0x80 {
        return (0x20..0x7f).contains(&c);
    }
    !matches!(c,
        0x80..=0xa0 | 0xad | 0x600..=0x605 | 0x61c | 0x6dd | 0x70f | 0x890..=0x891 | 0x8e2 | 0x1680 | 0x180e
        | 0x2000..=0x200f | 0x2028..=0x202f | 0x205f..=0x2064 | 0x2066..=0x206f | 0x3000 | 0xd800..=0xf8ff | 0xfeff
        | 0xfff9..=0xfffb | 0x110bd | 0x110cd | 0x13430..=0x1343f | 0x1bca0..=0x1bca3 | 0x1d173..=0x1d17a | 0xe0001
        | 0xe0020..=0xe007f | 0xf0000..)
}

fn is_space(r: char) -> bool {
    matches!(r as u32, 0x09..=0x0d | 0x20 | 0x85 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000)
}

/// The body of `strconv.Quote`/`QuoteRune` for one rune.
fn quote_into(out: &mut String, r: char, quote: char) {
    match r {
        '\u{7}' => out.push_str("\\a"),
        '\u{8}' => out.push_str("\\b"),
        '\u{c}' => out.push_str("\\f"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        '\u{b}' => out.push_str("\\v"),
        c if c == quote || c == '\\' => {
            out.push('\\');
            out.push(c);
        }
        c if is_print(c) => out.push(c),
        c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
        c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
        c => out.push_str(&format!("\\U{:08x}", c as u32)),
    }
}

/// `strconv.Quote` of a valid UTF-8 string.
pub fn go_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        quote_into(&mut out, c, '"');
    }
    out.push('"');
    out
}

/// `jsonwire.QuoteRune`.
fn quote_rune(b: &[u8]) -> String {
    match decode_rune(b) {
        (None, _) => format!("'\\x{:x}'", b[0]),
        (Some(r), _) => {
            let mut s = String::from("'");
            quote_into(&mut s, r, '\'');
            s.push('\'');
            s
        }
    }
}

/// `InvalidTextError.Error`.
fn invalid_text(label: &str, what: &[u8], place: &str) -> Err {
    let mut runes = Vec::new();
    let mut i = 0;
    while i < what.len() {
        let (r, n) = decode_rune(&what[i..]);
        runes.push((r, &what[i..i + n]));
        i += n;
    }
    let shown = if runes.len() == 1 {
        quote_rune(what)
    } else if runes.iter().any(|(r, _)| r.is_none_or(|c| c == '`' || c == '\u{fffd}' || is_space(c) || !is_print(c))) {
        let mut s = String::from("\"");
        for (r, raw) in &runes {
            match r {
                Some(c) => quote_into(&mut s, *c, '"'),
                None => s.push_str(&format!("\\x{:02x}", raw[0])),
            }
        }
        s.push('"');
        s
    } else {
        format!("`{}`", String::from_utf8_lossy(what))
    };
    Err::Msg(format!("invalid {label} {shown} {place}").trim_end().to_string())
}

fn invalid_char(b: &[u8], pos: usize, place: &str) -> Err {
    let (_, n) = decode_rune(&b[pos..]);
    invalid_text("character", &b[pos..pos + n], place)
}

fn value(b: &[u8], pos: usize, depth: usize) -> Result<usize, Err> {
    match b[pos] {
        b'n' => literal(b, pos, "null"),
        b'f' => literal(b, pos, "false"),
        b't' => literal(b, pos, "true"),
        b'"' => string(b, pos),
        b'-' | b'0'..=b'9' => number(b, pos),
        b'{' => object(b, pos, depth),
        b'[' => array(b, pos, depth),
        _ => Err(invalid_char(b, pos, "at start of value")),
    }
}

fn literal(b: &[u8], pos: usize, lit: &str) -> Result<usize, Err> {
    let rest = &b[pos..];
    for (i, want) in lit.bytes().enumerate() {
        match rest.get(i) {
            None => return Err(Err::Eof),
            Some(&c) if c != want => {
                return Err(invalid_char(b, pos + i, &format!("in literal {lit} (expecting '{}')", want as char)));
            }
            _ => {}
        }
    }
    Ok(pos + lit.len())
}

/// `\uXXXX` cut short by the end of the input (`hasEscapedUTF16Prefix` without the low-half check).
fn escape_prefix(b: &[u8]) -> bool {
    b.iter().enumerate().all(|(i, &c)| match i {
        0 => c == b'\\',
        1 => c == b'u',
        2..=5 => c.is_ascii_hexdigit(),
        _ => true,
    })
}

fn string(b: &[u8], pos: usize) -> Result<usize, Err> {
    if b[pos] != b'"' {
        return Err(invalid_char(b, pos, "at start of string (expecting '\"')"));
    }
    let mut n = pos + 1;
    loop {
        while n < b.len() && b[n] < 0x80 && b[n] >= b' ' && b[n] != b'\\' && b[n] != b'"' {
            n += 1;
        }
        if n >= b.len() {
            return Err(Err::Eof);
        }
        if b[n] == b'"' {
            return Ok(n + 1);
        }
        match decode_rune(&b[n..]) {
            (_, w) if w > 1 => n += w,
            (Some('\\'), _) => {
                if b.len() < n + 2 {
                    return Err(Err::Eof);
                }
                match b[n + 1] {
                    b'/' | b'"' | b'\\' | b'b' | b'f' | b'n' | b'r' | b't' => n += 2,
                    b'u' => {
                        if b.len() < n + 6 {
                            if escape_prefix(&b[n..]) {
                                return Err(Err::Eof);
                            }
                            return Err(invalid_text("escape sequence", &b[n..], "in string"));
                        }
                        if !b[n + 2..n + 6].iter().all(u8::is_ascii_hexdigit) {
                            return Err(invalid_text("escape sequence", &b[n..n + 6], "in string"));
                        }
                        n += 6;
                    }
                    _ => return Err(invalid_text("escape sequence", &b[n..n + 2], "in string")),
                }
            }
            (None, _) => {
                if !full_rune(&b[n..]) {
                    return Err(Err::Eof);
                }
                n += 1;
            }
            _ => return Err(invalid_char(b, n, "in string (expecting non-control character)")),
        }
    }
}

fn number(b: &[u8], pos: usize) -> Result<usize, Err> {
    let mut n = pos;
    let digits = |n: &mut usize| {
        while *n < b.len() && b[*n].is_ascii_digit() {
            *n += 1;
        }
    };
    if b[n] == b'-' {
        n += 1;
    }
    match b.get(n) {
        None => return Err(Err::Eof),
        Some(b'0') => n += 1,
        Some(b'1'..=b'9') => digits(&mut n),
        _ => return Err(invalid_char(b, n, "in number (expecting digit)")),
    }
    if b.get(n) == Some(&b'.') {
        n += 1;
        match b.get(n) {
            None => return Err(Err::Eof),
            Some(c) if c.is_ascii_digit() => digits(&mut n),
            _ => return Err(invalid_char(b, n, "in number (expecting digit)")),
        }
    }
    if matches!(b.get(n), Some(b'e' | b'E')) {
        n += 1;
        if matches!(b.get(n), Some(b'-' | b'+')) {
            n += 1;
        }
        match b.get(n) {
            None => return Err(Err::Eof),
            Some(c) if c.is_ascii_digit() => digits(&mut n),
            _ => return Err(invalid_char(b, n, "in number (expecting digit)")),
        }
    }
    Ok(n)
}

fn at(b: &[u8], pos: usize) -> Result<u8, Err> {
    b.get(pos).copied().ok_or(Err::Eof)
}

fn object(b: &[u8], pos: usize, depth: usize) -> Result<usize, Err> {
    if depth > MAX_DEPTH {
        return Err(Err::Msg("exceeded max depth".into()));
    }
    let mut pos = ws(b, pos + 1);
    if at(b, pos)? == b'}' {
        return Ok(pos + 1);
    }
    loop {
        pos = ws(b, pos);
        at(b, pos)?;
        pos = ws(b, string(b, pos)?);
        if at(b, pos)? != b':' {
            return Err(invalid_char(b, pos, "after object name (expecting ':')"));
        }
        pos = ws(b, pos + 1);
        at(b, pos)?;
        pos = ws(b, value(b, pos, depth + 1)?);
        match at(b, pos)? {
            b',' => pos += 1,
            b'}' => return Ok(pos + 1),
            _ => return Err(invalid_char(b, pos, "after object value (expecting ',' or '}')")),
        }
    }
}

fn array(b: &[u8], pos: usize, depth: usize) -> Result<usize, Err> {
    if depth > MAX_DEPTH {
        return Err(Err::Msg("exceeded max depth".into()));
    }
    let mut pos = ws(b, pos + 1);
    if at(b, pos)? == b']' {
        return Ok(pos + 1);
    }
    loop {
        pos = ws(b, pos);
        at(b, pos)?;
        pos = ws(b, value(b, pos, depth + 1)?);
        match at(b, pos)? {
            b',' => pos += 1,
            b']' => return Ok(pos + 1),
            _ => return Err(invalid_char(b, pos, "after array element (expecting ',' or ']')")),
        }
    }
}

// ---------------------------------------------------------------- decoding (input already checked)

enum Seg {
    Name(String),
    Index(usize),
}

struct Dec<'a> {
    b: &'a [u8],
    i: usize,
    path: Vec<Seg>,
    /// The first error, as `Unmarshal` returns it after decoding everything it can.
    err: Option<String>,
}

impl Dec<'_> {
    fn ws(&mut self) {
        self.i = ws(self.b, self.i);
    }

    fn save(&mut self, e: String) {
        self.err.get_or_insert(e);
    }

    /// `UnmarshalTypeError.Error`'s "into ...": the path joined by dots, `Go struct field ` in front unless its last
    /// dot-separated piece is all digits (v2_decode.go's guess that it indexes an array), `Go value` for no path.
    fn place(&self) -> String {
        let field = self
            .path
            .iter()
            .map(|s| match s {
                // JSON Pointer escapes, as jsontext keeps the path.
                Seg::Name(n) => n.replace('~', "~0").replace('/', "~1"),
                Seg::Index(i) => i.to_string(),
            })
            .collect::<Vec<_>>()
            .join(".");
        if field.is_empty() {
            return "Go value".into();
        }
        let last = &field[field.rfind('.').map_or(0, |i| i + 1)..];
        if !last.is_empty() && last.bytes().all(|c| c.is_ascii_digit()) {
            format!(".{field}")
        } else {
            format!("Go struct field .{field}")
        }
    }

    fn kind(&self) -> &'static str {
        match self.b[self.i] {
            b'{' => "object",
            b'[' => "array",
            b'"' => "string",
            b't' | b'f' => "bool",
            b'n' => "null",
            _ => "number",
        }
    }

    fn skip(&mut self) {
        self.i = value(self.b, self.i, 1).unwrap_or(self.b.len());
    }

    /// A value decoded into `any`.
    fn any(&mut self) -> J {
        self.ws();
        match self.b[self.i] {
            b'{' => J::Map(self.map()),
            b'[' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b[self.i] == b']' {
                    self.i += 1;
                    return J::Arr(out);
                }
                loop {
                    self.path.push(Seg::Index(out.len()));
                    let v = self.any();
                    self.path.pop();
                    out.push(v);
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    if c == b']' {
                        return J::Arr(out);
                    }
                }
            }
            b'"' => J::Str(self.string()),
            b't' => {
                self.i += 4;
                J::Bool(true)
            }
            b'f' => {
                self.i += 5;
                J::Bool(false)
            }
            b'n' => {
                self.i += 4;
                J::Null
            }
            _ => {
                let start = self.i;
                self.i = number(self.b, start).unwrap_or(self.b.len());
                let lit = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
                let f: f64 = lit.parse().unwrap_or(f64::NAN);
                if f.is_infinite() {
                    let e = format!("json: cannot unmarshal number {lit} into {} of type float64", self.place());
                    self.save(e);
                }
                J::Num(f)
            }
        }
    }

    /// An object into `map[string]any`, the last of a repeated name winning.
    fn map(&mut self) -> BTreeMap<String, J> {
        let mut m = BTreeMap::new();
        self.i += 1;
        loop {
            self.ws();
            match self.b[self.i] {
                b'}' => {
                    self.i += 1;
                    return m;
                }
                b',' => {
                    self.i += 1;
                    continue;
                }
                _ => {}
            }
            let k = self.string();
            self.ws();
            self.i += 1; // ':'
            self.path.push(Seg::Name(k.clone()));
            let v = self.any();
            self.path.pop();
            m.insert(k, v);
        }
    }

    /// `AppendUnquote` with invalid UTF-8 allowed: each bad byte and each unpaired surrogate becomes U+FFFD.
    fn string(&mut self) -> String {
        let b = self.b;
        let end = string(b, self.i).unwrap_or(b.len());
        let raw = &b[self.i + 1..end - 1];
        self.i = end;
        let mut out = String::with_capacity(raw.len());
        let mut i = 0;
        let hex = |s: &[u8]| u32::from_str_radix(std::str::from_utf8(s).unwrap_or("x"), 16).ok();
        while i < raw.len() {
            if raw[i] == b'\\' {
                let e = raw[i + 1];
                i += 2;
                match e {
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hi = hex(&raw[i..i + 4]).unwrap_or(0xfffd);
                        i += 4;
                        if (0xd800..0xe000).contains(&hi) {
                            if hi < 0xdc00 && raw.get(i) == Some(&b'\\') && raw.get(i + 1) == Some(&b'u') && raw.len() >= i + 6 {
                                if let Some(lo) = hex(&raw[i + 2..i + 6]).filter(|lo| (0xdc00..0xe000).contains(lo)) {
                                    out.push(char::from_u32(0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00)).unwrap());
                                    i += 6;
                                    continue;
                                }
                            }
                            out.push('\u{fffd}');
                        } else {
                            out.push(char::from_u32(hi).unwrap_or('\u{fffd}'));
                        }
                    }
                    c => out.push(c as char),
                }
                continue;
            }
            match decode_rune(&raw[i..]) {
                (Some(c), n) => {
                    out.push(c);
                    i += n;
                }
                (None, _) => {
                    out.push('\u{fffd}');
                    i += 1;
                }
            }
        }
        out
    }
}

/// What `json.Unmarshal(data, &m)` leaves in a `map[string]any` (Some(None) is the nil map `null` sets), with its error.
pub fn unmarshal_map(b: &[u8]) -> (Option<BTreeMap<String, J>>, Option<String>) {
    if let Err(e) = check_valid(b) {
        return (Some(BTreeMap::new()), Some(e));
    }
    let mut d = Dec { b, i: 0, path: Vec::new(), err: None };
    d.ws();
    let m = match d.b[d.i] {
        b'{' => Some(d.map()),
        b'n' => None,
        _ => {
            let e = format!("json: cannot unmarshal {} into Go value of type map[string]interface {{}}", d.kind());
            d.save(e);
            Some(BTreeMap::new())
        }
    };
    (m, d.err)
}

/// What `json.Unmarshal(data, &entries)` leaves in a nil `[]map[string]any`: each element a map or `null`.
pub fn unmarshal_map_slice(b: &[u8]) -> (Option<Vec<J>>, Option<String>) {
    if let Err(e) = check_valid(b) {
        return (None, Some(e));
    }
    let mut d = Dec { b, i: 0, path: Vec::new(), err: None };
    d.ws();
    let mut out = Vec::new();
    match d.b[d.i] {
        b'[' => {
            d.i += 1;
            loop {
                d.ws();
                match d.b[d.i] {
                    b']' => break,
                    b',' => {
                        d.i += 1;
                        continue;
                    }
                    b'{' => {
                        d.path.push(Seg::Index(out.len()));
                        let m = d.map();
                        d.path.pop();
                        out.push(J::Map(m));
                    }
                    b'n' => {
                        d.i += 4;
                        out.push(J::Null);
                    }
                    _ => {
                        d.path.push(Seg::Index(out.len()));
                        let e = format!("json: cannot unmarshal {} into {} of type map[string]interface {{}}", d.kind(), d.place());
                        d.path.pop();
                        d.save(e);
                        d.skip();
                        out.push(J::Null);
                    }
                }
            }
        }
        b'n' => return (None, d.err),
        _ => {
            let e = format!("json: cannot unmarshal {} into Go value of type []map[string]interface {{}}", d.kind());
            d.save(e);
            return (None, d.err);
        }
    }
    (Some(out), d.err)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `GOJSON_CORPUS=<file>`: one case per line as hex; writes `<file>.rust` with the error and the compact
    /// re-encoding for each, to diff against the same cases through Go (test/gojson_corpus.go).
    #[test]
    #[ignore]
    fn corpus() {
        let Ok(path) = std::env::var("GOJSON_CORPUS") else { return };
        let text = std::fs::read_to_string(&path).unwrap();
        let mut out = String::new();
        for line in text.lines() {
            let b: Vec<u8> = (0..line.len() / 2).map(|i| u8::from_str_radix(&line[2 * i..2 * i + 2], 16).unwrap()).collect();
            let (m, e) = unmarshal_map(&b);
            let enc = match &m {
                Some(m) => crate::gotheme::marshal(&J::Map(m.clone())).unwrap_or_else(|e| e),
                None => "null".into(),
            };
            let (s, e2) = unmarshal_map_slice(&b);
            let enc2 = match s {
                Some(s) => crate::gotheme::marshal(&J::Arr(s)).unwrap_or_else(|e| e),
                None => "null".into(),
            };
            out.push_str(&format!("{} | {}\n{} | {}\n", e.unwrap_or("<nil>".into()), enc, e2.unwrap_or("<nil>".into()), enc2));
        }
        std::fs::write(format!("{path}.rust"), out).unwrap();
    }

    #[test]
    fn messages() {
        assert_eq!(check_valid(b"{ broken").unwrap_err(), "invalid character 'b' looking for beginning of object key string");
        assert_eq!(check_valid(b"").unwrap_err(), "unexpected end of JSON input");
        assert_eq!(check_valid(b"{\"a\":tru}").unwrap_err(), "invalid character '}' in literal true (expecting 'e')");
        assert_eq!(check_valid(b"{\"a\":\"\\u12\"}").unwrap_err(), "invalid escape sequence `\\u12\"}` in string");
        assert_eq!(check_valid(b"{\"a\":\"\\ \"}").unwrap_err(), "invalid escape sequence \"\\\\ \" in string");
    }
}
