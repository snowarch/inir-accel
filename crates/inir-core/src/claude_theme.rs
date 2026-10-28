//! Port of `scripts/colors/claude_code_theme.py`, line for line: the four Claude Code themes from terminal.json and
//! palette.json, on the same HCT (`material::hct`, byte for byte with materialyoucolor). `test/parity.py claude`.

use crate::material::hct::Hct;
use crate::material::py::{self, Json};
use crate::pyrt::{self, exc, R};
use std::path::Path;

/// `obj[key]` on a loaded JSON value.
fn item<'a>(v: &'a Json, key: &str) -> R<&'a Json> {
    match v {
        Json::Obj(_) => v.get(key).ok_or_else(|| exc("KeyError", pyrt::repr_str(key))),
        Json::Arr(_) => Err(exc("TypeError", "list indices must be integers or slices, not str")),
        Json::Str(_) => Err(exc("TypeError", "string indices must be integers, not 'str'")),
        other => Err(exc("TypeError", format!("'{}' object is not subscriptable", pyrt::type_name(other)))),
    }
}

/// `Hct.from_int(int("FF" + hex.lstrip("#"), 16))`; Python's int is unbounded, the colour reads its low 32 bits.
fn to_hct(v: &Json) -> R<Hct> {
    let digits = format!("FF{}", pyrt::as_str(v, "lstrip")?.trim_start_matches('#'));
    let bad = || exc("ValueError", format!("invalid literal for int() with base 16: {}", pyrt::repr_str(&digits)));
    let t = py::strip(&digits);
    if t.starts_with('_') || t.ends_with('_') || t.contains("__") {
        return Err(bad());
    }
    let mut n: u64 = 0;
    for c in t.chars().filter(|&c| c != '_') {
        let d = c.to_digit(16).filter(|_| c.is_ascii()).ok_or_else(bad)?;
        n = n.wrapping_mul(16).wrapping_add(d as u64);
    }
    Ok(Hct::from_int(n as u32))
}

fn hct_hex(hue: f64, chroma: f64, tone: f64) -> String {
    let c = if chroma > 0.0 { chroma } else { 0.0 };
    let t = if tone < 100.0 { tone } else { 100.0 };
    let t = if t > 0.0 { t } else { 0.0 };
    format!("#{:06X}", Hct::from_hct(hue, c, t).argb & 0xFFFFFF)
}

fn retone(hex: &str, tone: f64, scale: f64) -> R<String> {
    let h = to_hct(&Json::Str(hex.to_string()))?;
    let c = h.chroma * scale;
    Ok(hct_hex(h.hue, if c > 0.0 { c } else { 0.0 }, tone))
}

const ANSI: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];

fn ansi(slot: usize) -> String {
    format!("ansi:{}", if slot < 8 { ANSI[slot].to_string() } else { format!("{}Bright", ANSI[slot - 8]) })
}

const SLOT_DARK: [(&str, usize); 12] = [("success", 10), ("error", 9), ("warning", 11), ("permission", 12), ("suggestion", 12),
    ("remember", 12), ("planMode", 14), ("autoAccept", 13), ("skill", 13), ("bashBorder", 13), ("merged", 13), ("ide", 4)];
const SLOT_LIGHT: [(&str, usize); 12] = [("success", 2), ("error", 1), ("warning", 3), ("permission", 4), ("suggestion", 4),
    ("remember", 4), ("planMode", 6), ("autoAccept", 5), ("skill", 5), ("bashBorder", 5), ("merged", 5), ("ide", 4)];

#[derive(Default)]
struct Obj(Vec<(String, Json)>);

impl Obj {
    fn set(&mut self, k: &str, v: Json) {
        match self.0.iter_mut().find(|(key, _)| key == k) {
            Some(slot) => slot.1 = v,
            None => self.0.push((k.to_string(), v)),
        }
    }
    fn s(&mut self, k: &str, v: String) {
        self.set(k, Json::Str(v));
    }
    fn get_str(&self, k: &str) -> String {
        self.0.iter().find(|(key, _)| key == k).map(|(_, v)| pyrt::str_of(v)).unwrap_or_default()
    }
}

fn dynamic(name: &str, soft: bool, term: &Json, palette: &Json, mode: &str) -> R<Json> {
    let dark = mode != "light";
    let bg = to_hct(item(term, "term0")?)?;
    let t0 = bg.tone;
    let sign = if dark { 1.0 } else { -1.0 };
    let primary = to_hct(item(palette, "primary")?)?;
    let ph = primary.hue;
    let accent = hct_hex(ph, if soft { 30.0 } else { 36.0f64.max(primary.chroma.min(60.0)) },
        if soft { if dark { 70.0 } else { 46.0 } } else if dark { 74.0 } else { 42.0 });
    let accent_hi = retone(&accent, if dark { 86.0 } else { 55.0 }, 1.0)?;
    let slot: &[(&str, usize); 12] = if dark { &SLOT_DARK } else { &SLOT_LIGHT };
    let ide = slot.iter().find(|(r, _)| *r == "ide").unwrap().1;
    let mut o = Obj::default();
    o.set("text", item(term, "term15")?.clone());
    o.set("inverseText", item(term, "term0")?.clone());
    o.s("claude", accent.clone());
    o.s("claudeShimmer", accent_hi);
    o.s("clawd_body", accent.clone());
    o.s("rate_limit_fill", accent);
    o.s("rate_limit_empty", hct_hex(bg.hue, bg.chroma.min(14.0), t0 + sign * 12.0));
    o.s("promptBorder", hct_hex(ph, if soft { 10.0 } else { 16.0 }, if dark { 48.0 } else { 56.0 }));
    o.s("promptBorderShimmer", hct_hex(ph, if soft { 14.0 } else { 22.0 }, if dark { 62.0 } else { 44.0 }));
    o.s("subtle", hct_hex(bg.hue, bg.chroma.min(8.0), if dark { 40.0 } else { 72.0 }));
    o.s("inactive", "ansi:blackBright".into());
    o.s("inactiveShimmer", if dark { "ansi:white" } else { "ansi:black" }.into());
    o.s("professionalBlue", ansi(ide));
    o.s("userMessageBackground", hct_hex(ph, if soft { 10.0 } else { 14.0 }, t0 + sign * 11.0));
    o.s("userMessageBackgroundHover", hct_hex(ph, if soft { 12.0 } else { 16.0 }, t0 + sign * 15.0));
    o.s("composerSidebarBackground", hct_hex(bg.hue, bg.chroma.min(12.0), t0 + sign * 5.0));
    let t5 = to_hct(item(term, "term5")?)?.hue;
    o.s("bashMessageBackgroundColor", hct_hex(t5, if dark { 14.0 } else { 10.0 }, t0 + sign * 10.0));
    let t6 = to_hct(item(term, "term6")?)?.hue;
    o.s("memoryBackgroundColor", hct_hex(t6, if dark { 12.0 } else { 9.0 }, t0 + sign * 10.0));
    o.s("selectionBg", hct_hex(ph, if dark { 24.0 } else { 18.0 }, if dark { 34.0 } else { 84.0 }));
    let add_hue = to_hct(item(term, "term2")?)?.hue;
    let del_hue = to_hct(item(term, "term1")?)?.hue;
    for (key, hue, strength) in [("Added", add_hue, 1.0), ("Removed", del_hue, 0.7)] {
        o.s(&format!("diff{key}"), hct_hex(hue, if dark { 18.0 } else { 14.0 } * strength, t0 + sign * 8.0));
        o.s(&format!("diff{key}Word"), hct_hex(hue, if dark { 30.0 } else { 24.0 } * strength, t0 + sign * 16.0));
        o.s(&format!("diff{key}Dimmed"), hct_hex(hue, 8.0 * strength, t0 + sign * 5.0));
    }
    if soft {
        let fg = to_hct(item(term, "term15")?)?;
        o.s("text", hct_hex(fg.hue, fg.chroma, if dark { fg.tone - 4.0 } else { fg.tone + 4.0 }));
        for (role, s) in slot {
            let n = if *s >= 9 { s - 8 } else { *s };
            let v = item(term, &format!("term{n}"))?;
            let hex = pyrt::as_str(v, "lstrip").map(String::from);
            let tone = to_hct(v)?.tone + if dark { 0.0 } else { 3.0 };
            o.s(role, retone(&hex?, tone, 0.8)?);
        }
        let ide = o.get_str("ide");
        o.s("professionalBlue", ide);
        for (shim, role) in [("warningShimmer", "warning"), ("permissionShimmer", "permission")] {
            let v = retone(&o.get_str(role), if dark { 84.0 } else { 52.0 }, 1.0)?;
            o.s(shim, v);
        }
    }
    Ok(theme(name, if dark { "dark-ansi" } else { "light-ansi" }, o))
}

fn theme(name: &str, base: &str, o: Obj) -> Json {
    Json::Obj(vec![("name".into(), Json::Str(name.into())), ("base".into(), Json::Str(base.into())), ("overrides".into(), Json::Obj(o.0))])
}

type Pal = Vec<(&'static str, String)>;

fn pal(pairs: &[(&'static str, &str)]) -> Pal {
    pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
}

/// `dict(base, **over)`
fn with(base: &Pal, over: &[(&'static str, &str)]) -> Pal {
    let mut p = base.clone();
    for (k, v) in over {
        match p.iter_mut().find(|(key, _)| key == k) {
            Some(slot) => slot.1 = v.to_string(),
            None => p.push((k, v.to_string())),
        }
    }
    p
}

fn vivid() -> Pal {
    pal(&[("text", "#F8F8F2"), ("inverse", "#1C1A17"), ("amber", "#FFA23A"), ("amber_hi", "#FFC57A"), ("cyan", "#66D9EF"),
        ("cyan_hi", "#A6EAF6"), ("green", "#A6E22E"), ("green_hi", "#C8F26B"), ("red", "#FF5C73"), ("red_hi", "#FF9AA8"),
        ("yellow", "#FFD04A"), ("yellow_hi", "#FFE58F"), ("purple", "#AE81FF"), ("purple_hi", "#CDB4FF"), ("pink", "#F92672"),
        ("pink_hi", "#FF73A6"), ("teal", "#5FD7AF"), ("blue", "#82A0FF"), ("violet", "#D78AFF"), ("inactive", "#A39B86"),
        ("inactive_hi", "#C9C1AA"), ("subtle", "#6E685A"), ("border", "#8C8570"), ("border_hi", "#B5AD94"), ("umsg", "#3D3631"),
        ("umsg_hover", "#4A423B"), ("side", "#2C2723"), ("bash", "#442B38"), ("memory", "#2B3D40"), ("select", "#5A4B3A"),
        ("add_bg", "#1F5A2B"), ("add_word", "#27682F"), ("add_dim", "#2A3A2D"), ("del_bg", "#6A2230"), ("del_word", "#8E2F45"),
        ("del_dim", "#3E2A2E"), ("rl_empty", "#4F4333"), ("fast", "#FF7A1A"), ("fast_hi", "#FFAA55")])
}

fn monokai_soft() -> Pal {
    with(&vivid(), &[("text", "#DDD6C6"), ("amber", "#E8A060"), ("amber_hi", "#F2C494"), ("cyan", "#7FC8D8"), ("cyan_hi", "#B1DCE6"),
        ("green", "#A7CC6B"), ("green_hi", "#C4DD9A"), ("red", "#E5707F"), ("red_hi", "#EFA2AB"), ("yellow", "#E6C675"),
        ("yellow_hi", "#F0D9A0"), ("purple", "#A890DC"), ("purple_hi", "#C6B6E8"), ("pink", "#DB6F98"), ("pink_hi", "#E8A1BC"),
        ("teal", "#74C2A4"), ("blue", "#91A6E6"), ("violet", "#C595E0"), ("inactive", "#968F7C"), ("inactive_hi", "#B8B19D"),
        ("subtle", "#645F53"), ("border", "#7C7662"), ("border_hi", "#A39C86"), ("umsg", "#37312D"), ("umsg_hover", "#423B36"),
        ("side", "#29251F"), ("bash", "#3C2A34"), ("memory", "#2A383A"), ("select", "#52463A"), ("add_bg", "#23472B"),
        ("add_word", "#337043"), ("add_dim", "#2A352C"), ("del_bg", "#552A33"), ("del_word", "#8F4252"), ("del_dim", "#392C2F"),
        ("rl_empty", "#473E31"), ("fast", "#E58A3A"), ("fast_hi", "#EFAE72")])
}

fn paper() -> Pal {
    pal(&[("text", "#2A241E"), ("inverse", "#F6F1E7"), ("amber", "#A8480A"), ("amber_hi", "#C8661E"), ("cyan", "#0B6A80"),
        ("cyan_hi", "#1A88A0"), ("green", "#4A720C"), ("green_hi", "#5E8C1A"), ("red", "#B02640"), ("red_hi", "#CC4A60"),
        ("yellow", "#7F5A00"), ("yellow_hi", "#9C7210"), ("purple", "#6440B8"), ("purple_hi", "#7E5CD0"), ("pink", "#AE1758"),
        ("pink_hi", "#C83E78"), ("teal", "#0C7058"), ("blue", "#2C50B0"), ("violet", "#8438AE"), ("inactive", "#6E6656"),
        ("inactive_hi", "#4F483B"), ("subtle", "#8E8673"), ("border", "#9A927D"), ("border_hi", "#6E6656"), ("fast", "#B8520C"),
        ("fast_hi", "#D2701E")])
}

fn paper_soft() -> Pal {
    with(&paper(), &[("text", "#3A332B"), ("amber", "#9A5524"), ("cyan", "#2A6B78"), ("green", "#557236"), ("red", "#A0414F"),
        ("yellow", "#77602A"), ("purple", "#6A5598"), ("pink", "#9C3C64"), ("teal", "#2E6A58"), ("blue", "#40589C"), ("violet", "#7C4C98")])
}

fn paper_backgrounds(p: &Pal, term: &Json) -> R<Pal> {
    let bg = to_hct(item(term, "term0")?)?;
    let t0 = bg.tone;
    let neutral = |dt: f64, c: f64| hct_hex(bg.hue, bg.chroma.min(c), t0 + dt);
    let add = to_hct(item(term, "term2")?)?.hue;
    let rem = to_hct(item(term, "term1")?)?.hue;
    let get = |k: &str| p.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone()).unwrap();
    let hue_of = |k: &str| to_hct(&Json::Str(get(k))).map(|h| h.hue);
    let vals = [
        ("umsg", neutral(-7.0, 10.0)), ("umsg_hover", neutral(-11.0, 10.0)), ("side", neutral(-4.0, 10.0)),
        ("select", hct_hex(hue_of("amber")?, 20.0, t0 - 12.0)), ("bash", hct_hex(hue_of("pink")?, 10.0, t0 - 8.0)),
        ("memory", hct_hex(hue_of("teal")?, 10.0, t0 - 8.0)), ("add_bg", hct_hex(add, 18.0, t0 - 8.0)),
        ("add_word", hct_hex(add, 28.0, t0 - 16.0)), ("add_dim", hct_hex(add, 8.0, t0 - 4.0)), ("del_bg", hct_hex(rem, 14.0, t0 - 8.0)),
        ("del_word", hct_hex(rem, 22.0, t0 - 16.0)), ("del_dim", hct_hex(rem, 6.0, t0 - 4.0)), ("rl_empty", neutral(-12.0, 14.0)),
    ];
    let over: Vec<(&'static str, &str)> = vals.iter().map(|(k, v)| (*k, v.as_str())).collect();
    Ok(with(p, &over))
}

fn monokai(name: &str, p: &Pal, base: &str) -> Json {
    let g = |k: &str| p.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone()).unwrap();
    let keys: [(&str, &str); 66] = [
        ("autoAccept", "purple"), ("skill", "purple"), ("bashBorder", "pink"), ("claude", "amber"), ("claudeShimmer", "amber_hi"),
        ("claudeBlue_FOR_SYSTEM_SPINNER", "cyan"), ("claudeBlueShimmer_FOR_SYSTEM_SPINNER", "cyan_hi"), ("permission", "cyan"),
        ("permissionShimmer", "cyan_hi"), ("planMode", "teal"), ("ide", "blue"), ("promptBorder", "border"),
        ("promptBorderShimmer", "border_hi"), ("text", "text"), ("inverseText", "inverse"), ("inactive", "inactive"),
        ("inactiveShimmer", "inactive_hi"), ("subtle", "subtle"), ("suggestion", "cyan"), ("remember", "cyan"), ("background", "teal"),
        ("success", "green"), ("error", "red"), ("warning", "yellow"), ("merged", "purple"), ("warningShimmer", "yellow_hi"),
        ("diffAdded", "add_bg"), ("diffRemoved", "del_bg"), ("diffAddedDimmed", "add_dim"), ("diffRemovedDimmed", "del_dim"),
        ("diffAddedWord", "add_word"), ("diffRemovedWord", "del_word"), ("red_FOR_SUBAGENTS_ONLY", "red"),
        ("blue_FOR_SUBAGENTS_ONLY", "blue"), ("green_FOR_SUBAGENTS_ONLY", "green"), ("yellow_FOR_SUBAGENTS_ONLY", "yellow"),
        ("purple_FOR_SUBAGENTS_ONLY", "purple"), ("orange_FOR_SUBAGENTS_ONLY", "amber"), ("pink_FOR_SUBAGENTS_ONLY", "pink_hi"),
        ("cyan_FOR_SUBAGENTS_ONLY", "cyan"), ("professionalBlue", "blue"), ("chromeYellow", "yellow"),
        ("userMessageBackground", "umsg"), ("userMessageBackgroundHover", "umsg_hover"), ("composerSidebarBackground", "side"),
        ("selectionBg", "select"), ("bashMessageBackgroundColor", "bash"), ("memoryBackgroundColor", "memory"),
        ("rate_limit_fill", "amber"), ("rate_limit_empty", "rl_empty"), ("fastMode", "fast"), ("fastModeShimmer", "fast_hi"),
        ("effortUltra", "purple"), ("briefLabelYou", "cyan"), ("briefLabelClaude", "amber"), ("rainbow_red", "red"),
        ("rainbow_orange", "amber"), ("rainbow_yellow", "yellow"), ("rainbow_green", "green"), ("rainbow_blue", "cyan"),
        ("rainbow_indigo", "blue"), ("rainbow_violet", "violet"), ("rainbow_red_shimmer", "red_hi"),
        ("rainbow_orange_shimmer", "amber_hi"), ("rainbow_yellow_shimmer", "yellow_hi"), ("rainbow_green_shimmer", "green_hi"),
    ];
    let mut o = Obj::default();
    for (k, src) in keys {
        o.s(k, g(src));
    }
    o.s("rainbow_blue_shimmer", g("cyan_hi"));
    o.s("rainbow_indigo_shimmer", "#B4C4FF".into());
    o.s("rainbow_violet_shimmer", g("purple_hi"));
    theme(name, base, o)
}

struct Args {
    terminal: String,
    palette: String,
    mode: String,
    out_dir: String,
    print: bool,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let (mut terminal, mut palette, mut out_dir, mut mode, mut print) = (None, None, None, "dark".to_string(), false);
    let mut i = 0;
    while i < args.len() {
        let (flag, inline) = match args[i].split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (args[i].clone(), None),
        };
        let value = |i: &mut usize| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            *i += 1;
            args.get(*i).filter(|v| !v.starts_with('-') || v.as_str() == "-").cloned().ok_or(format!("argument {flag}: expected one argument"))
        };
        match flag.as_str() {
            "--terminal" => terminal = Some(value(&mut i)?),
            "--palette" => palette = Some(value(&mut i)?),
            "--out-dir" => out_dir = Some(value(&mut i)?),
            "--mode" => {
                mode = value(&mut i)?;
                if mode != "dark" && mode != "light" {
                    return Err(format!("argument --mode: invalid choice: {} (choose from 'dark', 'light')", pyrt::repr_str(&mode)));
                }
            }
            "--print" => print = true,
            other => return Err(format!("unrecognized arguments: {other}")),
        }
        i += 1;
    }
    let missing: Vec<&str> = [("--terminal", &terminal), ("--palette", &palette), ("--out-dir", &out_dir)]
        .iter().filter(|(_, v)| v.is_none()).map(|(k, _)| *k).collect();
    if !missing.is_empty() {
        return Err(format!("the following arguments are required: {}", missing.join(", ")));
    }
    Ok(Args { terminal: terminal.unwrap(), palette: palette.unwrap(), mode, out_dir: out_dir.unwrap(), print })
}

fn run(a: &Args) -> R<()> {
    let term = pyrt::json_load(Path::new(&a.terminal))?;
    let palette = pyrt::json_load(Path::new(&a.palette))?;
    let light = a.mode == "light";
    let files = [
        ("inir.json", dynamic("iNiR", false, &term, &palette, &a.mode)?),
        ("inir-soft.json", dynamic("iNiR Soft", true, &term, &palette, &a.mode)?),
        ("inir-monokai.json", if light { monokai("iNiR Monokai", &paper_backgrounds(&paper(), &term)?, "light") } else { monokai("iNiR Monokai", &vivid(), "dark") }),
        ("inir-monokai-soft.json", if light { monokai("iNiR Monokai Soft", &paper_backgrounds(&paper_soft(), &term)?, "light") } else { monokai("iNiR Monokai Soft", &monokai_soft(), "dark") }),
    ];
    let out = Path::new(&a.out_dir);
    let mut changed = vec![];
    for (name, theme) in &files {
        let text = format!("{}\n", py::json_dump(theme));
        if a.print {
            println!("{name} {text}");
            continue;
        }
        pyrt::makedirs(out)?;
        let path = out.join(name);
        if pyrt::exists(&path) && pyrt::read_text(&path)? == text {
            continue;
        }
        pyrt::write_text(&path, &text)?;
        changed.push(*name);
    }
    if changed.is_empty() {
        println!("{}", files.iter().map(|(n, _)| *n).collect::<Vec<_>>().join("\n"));
    } else {
        println!("changed: {}", changed.join(" "));
    }
    Ok(())
}

/// `inir-accel claude-code-theme ...`: the exit code.
pub fn run_cli(args: &[String]) -> i32 {
    match parse_args(args) {
        Ok(a) => match run(&a) {
            Ok(()) => 0,
            Err(e) => pyrt::die(e),
        },
        Err(e) => {
            eprintln!("inir-accel claude-code-theme: error: {e}");
            2
        }
    }
}
