//! Spicetify's live theme, from `apply-spicetify-theme.sh`'s inline Python: `spicetify-reload <port>` swaps the
//! stylesheets of the running Spotify over CDP (a page reload only when it found none), and `spicetify-block <css>
//! <kind> <block>` writes one of iNiR's generated blocks into user.css, dropping every copy a run left before.

use crate::cdp;
use serde_json::{Value, json};

// Each link keeps its own file: colors.css is a userCSS link too, and pointing it at user.css dropped the scheme.
const SWAP: &str = "(() => {
  const links = [...document.querySelectorAll('link.userCSS, link[href^=\"user.css\"]')];
  links.forEach(link => { link.href = new URL(link.href, location.href).pathname.split('/').pop() + '?inir=' + Date.now(); });
  return links.length;
})()";

/// `spicetify-reload <port>`: exit 1 when no Spotify page answers.
pub fn run_reload(args: &[String]) -> i32 {
    let Some(port) = args.first().and_then(|p| p.parse::<u16>().ok()).filter(|_| args.len() == 1) else {
        eprintln!("usage: inir-accel spicetify-reload <port>");
        return 2;
    };
    let Some(targets) = cdp::targets(port, "json/list") else { return 1 };
    let page = targets.as_array().into_iter().flatten().find(|t| {
        t["type"].as_str() == Some("page") && t["url"].as_str().unwrap_or_default().contains("spotify.com")
    });
    let Some(mut s) = page.and_then(|p| p["webSocketDebuggerUrl"].as_str()).and_then(cdp::Session::open) else {
        return 1;
    };
    // A page reload flashes the whole client and can raise its window: only when no stylesheet was swapped.
    let Some(reply) = s.call(1, "Runtime.evaluate", json!({"expression": SWAP, "returnByValue": true})) else { return 1 };
    let swapped = match reply.pointer("/result/result/value") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(t)) => !t.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    };
    if !swapped {
        let _ = s.send(2, "Page.reload", json!({"ignoreCache": true}));
    }
    0
}

struct Block {
    start: &'static str,
    end: &'static str,
}

const BRIDGE: Block = Block {
    start: "/* === iNiR CSS variable bridge - auto-generated, do not edit === */",
    end: "/* === end iNiR CSS variable bridge === */",
};
const PLAYBACK: Block = Block {
    start: "/* === iNiR playback controls fix - auto-generated === */",
    end: "/* === end iNiR playback controls fix === */",
};
const COLORS: Block = Block { start: "/* === iNiR TUI color bridge - auto-generated === */", end: "/* === end iNiR TUI color bridge === */" };
const TUI: Block = Block { start: "/* === iNiR TUI overrides - auto-generated === */", end: "/* === end iNiR TUI overrides === */" };

/// Every `start ... end` (and the newline after it) out, as `re.sub(start.*?end\n?, '', DOTALL)`.
fn strip_blocks(text: &str, b: &Block) -> String {
    let mut out = text.to_string();
    let mut from = 0;
    while let Some(s) = out[from..].find(b.start).map(|i| from + i) {
        let Some(e) = out[s + b.start.len()..].find(b.end).map(|i| s + b.start.len() + i + b.end.len()) else { break };
        let e = if out[e..].starts_with('\n') { e + 1 } else { e };
        out.replace_range(s..e, "");
        from = s;
    }
    out
}

/// The playback fix's older copies may have lost their end marker: each runs to its end, the next copy or the file's end.
fn strip_playback(text: &str) -> String {
    let mut out = text.to_string();
    let mut from = 0;
    while let Some(s) = out[from..].find(PLAYBACK.start).map(|i| from + i) {
        let after = s + PLAYBACK.start.len();
        let next = [out[after..].find(PLAYBACK.end), out[after..].find(PLAYBACK.start)].into_iter().flatten().min();
        let e = next.map_or(out.len(), |i| after + i);
        out.replace_range(s..e, "");
        from = s;
    }
    let mut res = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(i) = rest.find(PLAYBACK.end) {
        res.push_str(&rest[..i]);
        rest = &rest[i + PLAYBACK.end.len()..];
        rest = rest.strip_prefix('\n').unwrap_or(rest);
    }
    res.push_str(rest);
    res
}

/// Removing a block from the middle leaves its blank lines behind: collapse them, or every run grows the file.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = 0;
    for c in text.chars() {
        if c == '\n' {
            run += 1;
            if run > 2 {
                continue;
            }
        } else {
            run = 0;
        }
        out.push(c);
    }
    out
}

fn place(content: &str, kind: &str, block: &str) -> Option<String> {
    let out = match kind {
        // Last, so the variables win the cascade.
        "bridge" => {
            let mut c = strip_blocks(content, &BRIDGE).trim_start_matches('\n').to_string();
            if !c.is_empty() && !c.ends_with('\n') {
                c.push('\n');
            }
            format!("{c}\n{block}\n")
        }
        "playback" => format!("{block}\n{}", strip_playback(content).trim_start_matches('\n')),
        "colors" | "tui" => {
            let b = if kind == "colors" { &COLORS } else { &TUI };
            format!("{}\n\n{block}\n", strip_blocks(content, b).trim_end())
        }
        _ => return None,
    };
    Some(collapse(&out))
}

/// `spicetify-block <css-file> <bridge|playback|colors|tui> <block>`
pub fn run_block(args: &[String]) -> i32 {
    let [file, kind, block] = args else {
        eprintln!("usage: inir-accel spicetify-block <css-file> <bridge|playback|colors|tui> <block>");
        return 2;
    };
    let content = match std::fs::read(file).map(String::from_utf8) {
        Ok(Ok(c)) => c.replace("\r\n", "\n").replace('\r', "\n"),
        Ok(Err(_)) => {
            eprintln!("spicetify-block: {file} is not UTF-8");
            return 1;
        }
        Err(e) => {
            eprintln!("spicetify-block: {file}: {e}");
            return 1;
        }
    };
    let Some(out) = place(&content, kind, block) else {
        eprintln!("spicetify-block: unknown block kind {kind}");
        return 2;
    };
    match std::fs::write(file, out) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("spicetify-block: {file}: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_goes_last_once() {
        let b = format!("{}\n:root {{}}\n{}", BRIDGE.start, BRIDGE.end);
        let css = format!("a {{}}\n{b}\n\n\nb {{}}\n{b}\n");
        assert_eq!(place(&css, "bridge", &b).unwrap(), format!("a {{}}\n\nb {{}}\n\n{b}\n"));
    }

    #[test]
    fn playback_without_end_runs_to_the_next_copy() {
        let b = format!("{}\nx\n{}", PLAYBACK.start, PLAYBACK.end);
        let css = format!("{}\nold\n{}\nolder\n{}\nkeep\n", PLAYBACK.start, PLAYBACK.start, PLAYBACK.end);
        assert_eq!(place(&css, "playback", &b).unwrap(), format!("{b}\nkeep\n"));
    }

    #[test]
    fn tui_block_replaces_in_place_of_the_tail() {
        let b = format!("{}\ny\n{}", TUI.start, TUI.end);
        let css = format!("a {{}}\n{b}\n  \n");
        assert_eq!(place(&css, "tui", &b).unwrap(), format!("a {{}}\n\n{b}\n"));
    }
}
