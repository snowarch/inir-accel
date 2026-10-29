//! Live theme for Pear Desktop (YouTube Music), ported from `pear-css-inject.py`: the CSS goes in as a <style> in every
//! page over the Chrome DevTools Protocol (Pear runs with --remote-debugging-port; iNiR uses 9223).
//! Prints injected/unchanged/failed; exit 0 when every reachable page has the CSS.

use crate::cdp;
use serde_json::Value;

const STYLE_ID: &str = "inir-pear-theme";

#[derive(PartialEq)]
enum Outcome {
    Injected,
    Same,
    Failed,
}

fn inject(ws_url: &str, css: &str) -> Outcome {
    let lit = serde_json::to_string(css).unwrap_or_default();
    let js = format!(
        "(function(){{var e=document.getElementById('{STYLE_ID}');if(e&&e.textContent==={lit})return 'same';if(e)e.remove();\
         var s=document.createElement('style');s.id='{STYLE_ID}';s.textContent={lit};\
         if(document.head)document.head.appendChild(s);return !!document.head;}})()"
    );
    match cdp::evaluate(ws_url, &js).as_ref().and_then(|r| r.pointer("/result/result/value")) {
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
    let Some(pages) = cdp::targets(port, "json") else {
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
