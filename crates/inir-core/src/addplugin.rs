//! A web app into iNiR's plugin folder, ported from `add-plugin.py`: the page's title and favicon, then the
//! manifest.json the ii family's web apps tab lists. Progress goes to stderr, the manifest to stdout.

use crate::material::py::{self, Json};
use crate::net;
use std::fs;

const TIMEOUT: f64 = 8.0;
const UA: (&str, &str) = ("User-Agent", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");

/// `urlparse(url)`: (scheme, netloc, path, query)
fn split(url: &str) -> (&str, &str, &str, &str) {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let rest = rest.split('#').next().unwrap_or(rest);
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (netloc, tail) = rest.split_at(end);
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    (scheme, netloc, path, query)
}

/// `urlparse(url).hostname`: lowercase, no credentials, no port.
fn hostname(netloc: &str) -> String {
    let host = netloc.rsplit_once('@').map_or(netloc, |(_, h)| h);
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(v6),
        None => host.split(':').next().unwrap_or(host),
    };
    host.to_lowercase()
}

/// github.com -> github, www.youtube.com -> youtube, localhost -> localhost
fn id_from_url(url: &str) -> String {
    let (_, netloc, _, _) = split(url);
    let host = Some(hostname(netloc)).filter(|h| !h.is_empty()).or_else(|| Some(netloc.to_string()).filter(|n| !n.is_empty())).unwrap_or_else(|| "plugin".into());
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let label = if host.contains('.') { host.split('.').next().unwrap_or(host) } else { host };
    let label: String = label.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    if label.is_empty() { "plugin".into() } else { label }
}

/// `urljoin(base, href)` for the hrefs pages put on icons (RFC 3986 resolution).
fn urljoin(base: &str, href: &str) -> String {
    if href.contains("://") || href.starts_with("data:") {
        return href.to_string();
    }
    let (scheme, netloc, path, _) = split(base);
    if let Some(rest) = href.strip_prefix("//") {
        return format!("{scheme}://{rest}");
    }
    if href.is_empty() {
        return base.to_string();
    }
    let href = href.split('#').next().unwrap_or(href);
    if let Some(q) = href.strip_prefix('?') {
        return format!("{scheme}://{netloc}{path}?{q}");
    }
    let (rel, q) = href.split_once('?').map_or((href, None), |(p, q)| (p, Some(q)));
    let merged = if rel.starts_with('/') { rel.to_string() } else { format!("{}{rel}", &path[..path.rfind('/').map_or(0, |i| i + 1)]) };
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = merged.split('/').collect();
    for (i, seg) in segments.iter().enumerate() {
        match *seg {
            "." => {}
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
            }
            s => out.push(s),
        }
        if matches!(*seg, "." | "..") && i == segments.len() - 1 {
            out.push("");
        }
    }
    let mut path = out.join("/");
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    match q {
        Some(q) => format!("{scheme}://{netloc}{path}?{q}"),
        None => format!("{scheme}://{netloc}{path}"),
    }
}

fn fetch(url: &str) -> Option<net::Response> {
    net::get(url, &[UA], TIMEOUT).ok().filter(|r| r.status > 0 && r.status < 400)
}

/// The body as `resp.headers.get_content_charset()` decodes it, replacing what does not decode.
fn text(r: &net::Response) -> String {
    let charset = r.header("content-type").and_then(|ct| ct.split(';').find_map(|p| p.trim().strip_prefix("charset="))).map(|c| c.trim_matches('"').to_lowercase());
    match charset.as_deref() {
        Some("iso-8859-1" | "latin-1" | "latin1" | "l1") => r.body.iter().map(|&b| b as char).collect(),
        _ => String::from_utf8_lossy(&r.body).into_owned(),
    }
}

/// `html.unescape` for what titles and hrefs carry: numeric references and the common names.
fn unescape(s: &str) -> String {
    const NAMED: [(&str, &str); 16] = [
        ("amp", "&"), ("lt", "<"), ("gt", ">"), ("quot", "\""), ("apos", "'"), ("nbsp", "\u{a0}"), ("middot", "·"), ("mdash", "—"),
        ("ndash", "–"), ("hellip", "…"), ("copy", "©"), ("reg", "®"), ("trade", "™"), ("laquo", "«"), ("raquo", "»"), ("bull", "•"),
    ];
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).map_or(rest.len(), |e| e + 1);
        let name = &rest[1..end];
        let decoded = if let Some(num) = name.strip_prefix('#') {
            let n = num.strip_prefix(['x', 'X']).map_or_else(|| num.parse().ok(), |h| u32::from_str_radix(h, 16).ok());
            n.and_then(char::from_u32).map(String::from)
        } else {
            NAMED.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
        };
        match decoded {
            Some(d) if end > 1 => {
                out.push_str(&d);
                rest = &rest[end..];
                rest = rest.strip_prefix(';').unwrap_or(rest);
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

/// What html.parser hands a parser: (start tag name, attributes) and the text inside <title>.
struct Page {
    title: String,
    icons: Vec<String>,
}

fn parse(html: &str) -> Page {
    let mut page = Page { title: String::new(), icons: Vec::new() };
    let (mut i, mut in_title) = (0, false);
    let lower = html.to_ascii_lowercase();
    while i < html.len() {
        let Some(lt) = html[i..].find('<').map(|p| i + p) else {
            if in_title {
                page.title += &unescape(&html[i..]);
            }
            break;
        };
        if in_title {
            page.title += &unescape(&html[i..lt]);
        }
        let rest = &lower[lt..];
        if rest.starts_with("<!--") {
            i = lower[lt + 4..].find("-->").map_or(html.len(), |e| lt + 4 + e + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            i = lower[lt..].find('>').map_or(html.len(), |e| lt + e + 1);
            continue;
        }
        let closing = rest.starts_with("</");
        let name_start = lt + if closing { 2 } else { 1 };
        let name_len = lower[name_start..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':')).unwrap_or(lower.len() - name_start);
        if name_len == 0 || !lower[name_start..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            if in_title {
                page.title.push('<');
            }
            i = lt + 1;
            continue;
        }
        let name = &lower[name_start..name_start + name_len];
        let (attrs, after) = attributes(html, name_start + name_len);
        i = after;
        match (closing, name) {
            (false, "title") => in_title = true,
            (true, "title") => in_title = false,
            (false, "link") => {
                let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
                if get("rel").unwrap_or_default().to_lowercase().contains("icon") {
                    let href = get("href").unwrap_or_default().trim().to_string();
                    if !href.is_empty() {
                        page.icons.push(href);
                    }
                }
            }
            (false, "script" | "style") => {
                let close = format!("</{name}");
                i = lower[i..].find(&close).map_or(html.len(), |e| i + e);
            }
            _ => {}
        }
    }
    page
}

/// A tag's attributes from `from` to its `>`, values unescaped; returns them and where the tag ends.
fn attributes(html: &str, from: usize) -> (Vec<(String, String)>, usize) {
    let b = html.as_bytes();
    let mut i = from;
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() {
            return (attrs, i);
        }
        if b[i] == b'>' {
            return (attrs, i + 1);
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let name = html[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let end = b[i + 1..].iter().position(|&c| c == q).map_or(b.len(), |e| i + 1 + e);
                value = unescape(&html[i + 1..end]);
                i = (end + 1).min(b.len());
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = unescape(&html[vs..i]);
            }
        }
        if name.is_empty() {
            i += 1;
        } else {
            attrs.push((name, value));
        }
    }
}

fn favicon(url: &str) -> Option<Vec<u8>> {
    let (scheme, netloc, _, _) = split(url);
    if let Some(r) = fetch(&format!("{scheme}://{netloc}/favicon.ico")).filter(|r| r.body.len() > 16) {
        return Some(r.body);
    }
    let page = fetch(url)?;
    let base = if page.url.is_empty() { url.to_string() } else { page.url.clone() };
    parse(&text(&page)).icons.iter().find_map(|href| fetch(&urljoin(&base, href)).filter(|r| r.body.len() > 16).map(|r| r.body))
}

/// PNG when the image decodes (ICO, JPEG, GIF...), the bytes as they came otherwise (SVG).
fn save_icon(data: &[u8], dest: &std::path::Path) -> bool {
    if let Ok(img) = image::load_from_memory(data) {
        if img.save_with_format(dest, image::ImageFormat::Png).is_ok() {
            return true;
        }
    }
    fs::write(dest, data).is_ok()
}

/// `add-plugin --url URL [--name NAME] [--icon ICON]`
pub fn run_cli(args: &[String]) -> i32 {
    let (mut url, mut name, mut icon) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let slot = match a.as_str() {
            "--url" => &mut url,
            "--name" => &mut name,
            "--icon" => &mut icon,
            _ => {
                eprintln!("usage: inir-accel add-plugin --url URL [--name NAME] [--icon ICON]");
                return 2;
            }
        };
        match it.next() {
            Some(v) => *slot = Some(v.clone()),
            None => {
                eprintln!("usage: inir-accel add-plugin --url URL [--name NAME] [--icon ICON]");
                return 2;
            }
        }
    }
    let Some(mut url) = url else {
        eprintln!("usage: inir-accel add-plugin --url URL [--name NAME] [--icon ICON]");
        return 2;
    };
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("https://{url}");
    }
    let id = id_from_url(&url);
    let name = match name.filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => {
            eprintln!("[add-plugin] Fetching title from {url} ...");
            let title = fetch(&url).map(|r| py::strip(&parse(&text(&r)).title).to_string()).filter(|t| !t.is_empty());
            match title {
                Some(t) => {
                    eprintln!("[add-plugin] Title: {t}");
                    t
                }
                None => {
                    let mut c = id.chars();
                    let n = c.next().map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()).unwrap_or_default();
                    eprintln!("[add-plugin] Could not fetch title, using '{n}'");
                    n
                }
            }
        }
    };
    let dir = crate::apps::shell_config_dir().join("plugins").join(&id);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("[add-plugin] Cannot create {}: {e}", dir.display());
        return 1;
    }
    eprintln!("[add-plugin] Fetching favicon ...");
    let mut icon_path = None;
    match favicon(&url) {
        Some(data) => {
            let dest = dir.join("icon.png");
            if save_icon(&data, &dest) {
                icon_path = Some("icon.png");
                eprintln!("[add-plugin] Favicon saved to {}", dest.display());
            } else {
                eprintln!("[add-plugin] Could not save favicon, skipping");
            }
        }
        None => eprintln!("[add-plugin] No favicon found, using symbol fallback"),
    }
    let s = |v: &str| Json::Str(v.to_string());
    let mut manifest = vec![
        ("id".to_string(), s(&id)),
        ("name".to_string(), s(&name)),
        ("url".to_string(), s(&url)),
        ("icon".to_string(), s(icon.as_deref().filter(|i| !i.is_empty()).unwrap_or("language"))),
        ("version".to_string(), s("1.0")),
        ("display".to_string(), s("tab")),
    ];
    if let Some(p) = icon_path {
        manifest.push(("iconPath".to_string(), s(p)));
    }
    let text = py::json_dump(&Json::Obj(manifest));
    if let Err(e) = fs::write(dir.join("manifest.json"), format!("{text}\n")) {
        eprintln!("[add-plugin] Cannot write the manifest: {e}");
        return 1;
    }
    println!("{text}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_joins() {
        assert_eq!(id_from_url("https://www.youtube.com/watch"), "youtube");
        assert_eq!(id_from_url("https://user@Sub.Example.co:8080/x"), "sub");
        assert_eq!(id_from_url("http://localhost:3000"), "localhost");
        assert_eq!(urljoin("https://a.com/b/c.html", "icon.png"), "https://a.com/b/icon.png");
        assert_eq!(urljoin("https://a.com/b/c.html", "../i/x.png?v=2"), "https://a.com/i/x.png?v=2");
        assert_eq!(urljoin("https://a.com/b/", "/fav.ico"), "https://a.com/fav.ico");
        assert_eq!(urljoin("https://a.com/b", "//cdn.x.com/f.png"), "https://cdn.x.com/f.png");
        assert_eq!(unescape("A &amp; B &#8211; &#x41;&nbsp;&bogus;"), "A & B – A\u{a0}&bogus;");
        let p = parse("<html><head><TITLE>Hi &amp; you</TITLE><script>var t='<title>no</title>'</script><link rel=\"Shortcut Icon\" href=\" /f.png \"><!-- <link rel=icon href=x> --></head>");
        assert_eq!(p.title, "Hi & you");
        assert_eq!(p.icons, vec!["/f.png"]);
    }
}
