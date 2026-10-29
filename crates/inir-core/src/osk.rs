//! On-screen keyboard labels straight from xkbcommon, ported from `osk-layout-labels.py`. The OSK types evdev
//! keycodes and the compositor's keymap turns them into text, so labelling each key with what this same keymap
//! produces is what gets typed, for any layout. xkbcommon is opened only here, as the python's ctypes did.

use regex::Regex;
use serde_json::{Map, json};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::sync::LazyLock;

const USAGE: &str = "Key labels for the on-screen keyboard, straight from xkbcommon.

Usage: inir-accel osk-layout-labels \"<layout description>\"   (as niri/Hyprland report it, e.g. \"Arabic\")";
const EVDEV_OFFSET: u32 = 8;

// Unicode's canonical combining class is non-zero in these ranges (generated from Python's unicodedata 16.0).
const COMBINING: &[(u32, u32)] = &[
    (0x300, 0x34E), (0x350, 0x36F), (0x483, 0x487), (0x591, 0x5BD), (0x5BF, 0x5BF), (0x5C1, 0x5C2), (0x5C4, 0x5C5),
    (0x5C7, 0x5C7), (0x610, 0x61A), (0x64B, 0x65F), (0x670, 0x670), (0x6D6, 0x6DC), (0x6DF, 0x6E4), (0x6E7, 0x6E8),
    (0x6EA, 0x6ED), (0x711, 0x711), (0x730, 0x74A), (0x7EB, 0x7F3), (0x7FD, 0x7FD), (0x816, 0x819), (0x81B, 0x823),
    (0x825, 0x827), (0x829, 0x82D), (0x859, 0x85B), (0x897, 0x89F), (0x8CA, 0x8E1), (0x8E3, 0x8FF), (0x93C, 0x93C),
    (0x94D, 0x94D), (0x951, 0x954), (0x9BC, 0x9BC), (0x9CD, 0x9CD), (0x9FE, 0x9FE), (0xA3C, 0xA3C), (0xA4D, 0xA4D),
    (0xABC, 0xABC), (0xACD, 0xACD), (0xB3C, 0xB3C), (0xB4D, 0xB4D), (0xBCD, 0xBCD), (0xC3C, 0xC3C), (0xC4D, 0xC4D),
    (0xC55, 0xC56), (0xCBC, 0xCBC), (0xCCD, 0xCCD), (0xD3B, 0xD3C), (0xD4D, 0xD4D), (0xDCA, 0xDCA), (0xE38, 0xE3A),
    (0xE48, 0xE4B), (0xEB8, 0xEBA), (0xEC8, 0xECB), (0xF18, 0xF19), (0xF35, 0xF35), (0xF37, 0xF37), (0xF39, 0xF39),
    (0xF71, 0xF72), (0xF74, 0xF74), (0xF7A, 0xF7D), (0xF80, 0xF80), (0xF82, 0xF84), (0xF86, 0xF87), (0xFC6, 0xFC6),
    (0x1037, 0x1037), (0x1039, 0x103A), (0x108D, 0x108D), (0x135D, 0x135F), (0x1714, 0x1715), (0x1734, 0x1734),
    (0x17D2, 0x17D2), (0x17DD, 0x17DD), (0x18A9, 0x18A9), (0x1939, 0x193B), (0x1A17, 0x1A18), (0x1A60, 0x1A60),
    (0x1A75, 0x1A7C), (0x1A7F, 0x1A7F), (0x1AB0, 0x1ABD), (0x1ABF, 0x1ACE), (0x1B34, 0x1B34), (0x1B44, 0x1B44),
    (0x1B6B, 0x1B73), (0x1BAA, 0x1BAB), (0x1BE6, 0x1BE6), (0x1BF2, 0x1BF3), (0x1C37, 0x1C37), (0x1CD0, 0x1CD2),
    (0x1CD4, 0x1CE0), (0x1CE2, 0x1CE8), (0x1CED, 0x1CED), (0x1CF4, 0x1CF4), (0x1CF8, 0x1CF9), (0x1DC0, 0x1DFF),
    (0x20D0, 0x20DC), (0x20E1, 0x20E1), (0x20E5, 0x20F0), (0x2CEF, 0x2CF1), (0x2D7F, 0x2D7F), (0x2DE0, 0x2DFF),
    (0x302A, 0x302F), (0x3099, 0x309A), (0xA66F, 0xA66F), (0xA674, 0xA67D), (0xA69E, 0xA69F), (0xA6F0, 0xA6F1),
    (0xA806, 0xA806), (0xA82C, 0xA82C), (0xA8C4, 0xA8C4), (0xA8E0, 0xA8F1), (0xA92B, 0xA92D), (0xA953, 0xA953),
    (0xA9B3, 0xA9B3), (0xA9C0, 0xA9C0), (0xAAB0, 0xAAB0), (0xAAB2, 0xAAB4), (0xAAB7, 0xAAB8), (0xAABE, 0xAABF),
    (0xAAC1, 0xAAC1), (0xAAF6, 0xAAF6), (0xABED, 0xABED), (0xFB1E, 0xFB1E), (0xFE20, 0xFE2F), (0x101FD, 0x101FD),
    (0x102E0, 0x102E0), (0x10376, 0x1037A), (0x10A0D, 0x10A0D), (0x10A0F, 0x10A0F), (0x10A38, 0x10A3A),
    (0x10A3F, 0x10A3F), (0x10AE5, 0x10AE6), (0x10D24, 0x10D27), (0x10D69, 0x10D6D), (0x10EAB, 0x10EAC),
    (0x10EFD, 0x10EFF), (0x10F46, 0x10F50), (0x10F82, 0x10F85), (0x11046, 0x11046), (0x11070, 0x11070),
    (0x1107F, 0x1107F), (0x110B9, 0x110BA), (0x11100, 0x11102), (0x11133, 0x11134), (0x11173, 0x11173),
    (0x111C0, 0x111C0), (0x111CA, 0x111CA), (0x11235, 0x11236), (0x112E9, 0x112EA), (0x1133B, 0x1133C),
    (0x1134D, 0x1134D), (0x11366, 0x1136C), (0x11370, 0x11374), (0x113CE, 0x113D0), (0x11442, 0x11442),
    (0x11446, 0x11446), (0x1145E, 0x1145E), (0x114C2, 0x114C3), (0x115BF, 0x115C0), (0x1163F, 0x1163F),
    (0x116B6, 0x116B7), (0x1172B, 0x1172B), (0x11839, 0x1183A), (0x1193D, 0x1193E), (0x11943, 0x11943),
    (0x119E0, 0x119E0), (0x11A34, 0x11A34), (0x11A47, 0x11A47), (0x11A99, 0x11A99), (0x11C3F, 0x11C3F),
    (0x11D42, 0x11D42), (0x11D44, 0x11D45), (0x11D97, 0x11D97), (0x11F41, 0x11F42), (0x1612F, 0x1612F),
    (0x16AF0, 0x16AF4), (0x16B30, 0x16B36), (0x16FF0, 0x16FF1), (0x1BC9E, 0x1BC9E), (0x1D165, 0x1D169),
    (0x1D16D, 0x1D172), (0x1D17B, 0x1D182), (0x1D185, 0x1D18B), (0x1D1AA, 0x1D1AD), (0x1D242, 0x1D244),
    (0x1E000, 0x1E006), (0x1E008, 0x1E018), (0x1E01B, 0x1E021), (0x1E023, 0x1E024), (0x1E026, 0x1E02A),
    (0x1E08F, 0x1E08F), (0x1E130, 0x1E136), (0x1E2AE, 0x1E2AE), (0x1E2EC, 0x1E2EF), (0x1E4EC, 0x1E4EF),
    (0x1E5EE, 0x1E5EF), (0x1E8D0, 0x1E8D6), (0x1E944, 0x1E94A),
];

fn combining(c: char) -> bool {
    let c = c as u32;
    COMBINING
        .binary_search_by(|&(lo, hi)| if hi < c { Ordering::Less } else if lo > c { Ordering::Greater } else { Ordering::Equal })
        .is_ok()
}

// What a dead key adds to the next letter, for surfaces that type text themselves (the lock keyboard).
fn dead_combining(name: &str) -> &'static str {
    match name {
        "grave" => "\u{300}",
        "acute" => "\u{301}",
        "circumflex" => "\u{302}",
        "tilde" => "\u{303}",
        "macron" => "\u{304}",
        "breve" => "\u{306}",
        "abovedot" => "\u{307}",
        "diaeresis" => "\u{308}",
        "abovering" => "\u{30a}",
        "doubleacute" => "\u{30b}",
        "caron" => "\u{30c}",
        "cedilla" => "\u{327}",
        "ogonek" => "\u{328}",
        "belowdot" => "\u{323}",
        "hook" => "\u{309}",
        "horn" => "\u{31b}",
        _ => "",
    }
}

fn dead_key(name: &str) -> &'static str {
    match name {
        "grave" => "`",
        "acute" => "´",
        "circumflex" => "^",
        "tilde" => "~",
        "macron" => "¯",
        "breve" => "˘",
        "abovedot" => "˙",
        "diaeresis" => "¨",
        "abovering" => "˚",
        "doubleacute" => "˝",
        "caron" => "ˇ",
        "cedilla" => "¸",
        "ogonek" => "˛",
        "iota" => "ͺ",
        "belowdot" => ".",
        "hook" => "\u{309}",
        "horn" => "\u{31b}",
        "stroke" => "/",
        "greek" => "α",
        "currency" => "¤",
        _ => "",
    }
}

#[repr(C)]
struct RuleNames {
    rules: *const c_char,
    model: *const c_char,
    layout: *const c_char,
    variant: *const c_char,
    options: *const c_char,
}

type Ptr = *mut c_void;

struct Xkb {
    context_new: unsafe extern "C" fn(c_int) -> Ptr,
    num_include_paths: unsafe extern "C" fn(Ptr) -> c_uint,
    include_path_get: unsafe extern "C" fn(Ptr, c_uint) -> *const c_char,
    keymap_new_from_names: unsafe extern "C" fn(Ptr, *const RuleNames, c_int) -> Ptr,
    keymap_mod_get_index: unsafe extern "C" fn(Ptr, *const c_char) -> u32,
    state_new: unsafe extern "C" fn(Ptr) -> Ptr,
    state_update_mask: unsafe extern "C" fn(Ptr, u32, u32, u32, u32, u32, u32) -> c_int,
    state_key_get_utf8: unsafe extern "C" fn(Ptr, u32, *mut c_char, usize) -> c_int,
    state_key_get_one_sym: unsafe extern "C" fn(Ptr, u32) -> u32,
    keysym_get_name: unsafe extern "C" fn(u32, *mut c_char, usize) -> c_int,
}

impl Xkb {
    fn load() -> Option<Xkb> {
        unsafe {
            let lib = libc::dlopen(c"libxkbcommon.so.0".as_ptr(), libc::RTLD_NOW);
            if lib.is_null() {
                return None;
            }
            macro_rules! sym {
                ($name:literal) => {{
                    let p = libc::dlsym(lib, concat!($name, "\0").as_ptr().cast());
                    if p.is_null() {
                        return None;
                    }
                    std::mem::transmute(p)
                }};
            }
            Some(Xkb {
                context_new: sym!("xkb_context_new"),
                num_include_paths: sym!("xkb_context_num_include_paths"),
                include_path_get: sym!("xkb_context_include_path_get"),
                keymap_new_from_names: sym!("xkb_keymap_new_from_names"),
                keymap_mod_get_index: sym!("xkb_keymap_mod_get_index"),
                state_new: sym!("xkb_state_new"),
                state_update_mask: sym!("xkb_state_update_mask"),
                state_key_get_utf8: sym!("xkb_state_key_get_utf8"),
                state_key_get_one_sym: sym!("xkb_state_key_get_one_sym"),
                keysym_get_name: sym!("xkb_keysym_get_name"),
            })
        }
    }

    fn sym_name(&self, state: Ptr, code: u32) -> String {
        let mut buf = [0 as c_char; 64];
        unsafe {
            (self.keysym_get_name)((self.state_key_get_one_sym)(state, code), buf.as_mut_ptr(), buf.len());
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        }
    }

    fn dead_name(&self, state: Ptr, code: u32) -> String {
        self.sym_name(state, code).strip_prefix("dead_").unwrap_or_default().to_string()
    }

    fn label(&self, state: Ptr, code: u32) -> String {
        static UNPRINTABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[[\p{C}\p{Z}]--\x20]").unwrap());
        let mut buf = [0 as c_char; 64];
        let text = unsafe {
            (self.state_key_get_utf8)(state, code, buf.as_mut_ptr(), buf.len());
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        };
        if text.is_empty() {
            return self.sym_name(state, code).strip_prefix("dead_").map_or("", dead_key).to_string();
        }
        if UNPRINTABLE.is_match(&text) || text.chars().all(|c| c == ' ') {
            return String::new();
        }
        // a lone combining mark (Arabic harakat, Hebrew points) is drawn on a dotted circle
        if text.chars().all(combining) { format!("◌{text}") } else { text }
    }

    // Layout description -> (layout, variant), read from the rules xkbcommon itself uses.
    fn resolve(&self, ctx: Ptr, description: &str) -> Option<(String, String)> {
        for i in 0..unsafe { (self.num_include_paths)(ctx) } {
            let root = unsafe { CStr::from_ptr((self.include_path_get)(ctx, i)) }.to_string_lossy().into_owned();
            let Ok(text) = std::fs::read_to_string(format!("{root}/rules/evdev.lst")) else { continue };
            let mut section = String::new();
            for line in crate::pyrt::splitlines(&text) {
                if let Some(s) = line.strip_prefix("! ") {
                    section = s.trim().to_string();
                    continue;
                }
                let line = line.trim_start();
                let Some(cut) = line.find(char::is_whitespace) else { continue };
                let (name, desc) = (&line[..cut], line[cut..].trim());
                if desc.is_empty() {
                    continue;
                }
                if section == "layout" && desc == description {
                    return Some((name.to_string(), String::new()));
                }
                if section == "variant" {
                    if let Some((layout, vdesc)) = desc.split_once(':') {
                        if vdesc.trim() == description {
                            return Some((layout.trim().to_string(), name.to_string()));
                        }
                    }
                }
            }
        }
        None
    }
}

pub fn run_cli(args: &[String]) -> i32 {
    static LETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}\p{M}]").unwrap());
    if args.len() != 1 {
        eprintln!("{USAGE}");
        return 2;
    }
    let Some(xkb) = Xkb::load() else {
        eprintln!("OSError: libxkbcommon.so.0: cannot open shared object file");
        return 1;
    };
    let ctx = unsafe { (xkb.context_new)(0) };
    let Some((layout, variant)) = xkb.resolve(ctx, &args[0]) else {
        println!("{}", json!({"error": format!("unknown layout: {}", args[0])}));
        return 1;
    };
    let (c_layout, c_variant) = (CString::new(layout.as_str()).unwrap_or_default(), CString::new(variant.as_str()).unwrap_or_default());
    let names = RuleNames {
        rules: c"evdev".as_ptr(),
        model: c"pc105".as_ptr(),
        layout: c_layout.as_ptr(),
        variant: c_variant.as_ptr(),
        options: c"".as_ptr(),
    };
    let keymap = unsafe { (xkb.keymap_new_from_names)(ctx, &names, 0) };
    if keymap.is_null() {
        println!("{}", json!({"error": format!("xkbcommon could not compile {layout}({variant})")}));
        return 1;
    }
    let (plain, shifted) = unsafe { ((xkb.state_new)(keymap), (xkb.state_new)(keymap)) };
    let shift = 1u32.checked_shl(unsafe { (xkb.keymap_mod_get_index)(keymap, c"Shift".as_ptr()) }).unwrap_or(0);
    unsafe { (xkb.state_update_mask)(shifted, shift, 0, 0, 0, 0, 0) };

    // the printable keys of a 105-key board (evdev codes); control keys keep their own labels
    let codes = (2..14).chain(16..28).chain(30..42).chain(43..54).chain([86]);
    let (mut keys, mut combine, mut letters, mut labels) = (Map::new(), Map::new(), Vec::new(), Vec::new());
    for code in codes {
        let at = code + EVDEV_OFFSET;
        let pair = [xkb.label(plain, at), xkb.label(shifted, at)];
        let dead = [dead_combining(&xkb.dead_name(plain, at)), dead_combining(&xkb.dead_name(shifted, at))];
        if dead.iter().any(|d| !d.is_empty()) {
            combine.insert(code.to_string(), json!(dead));
        }
        if LETTER.is_match(&pair[0].replace('◌', "")) {
            letters.push(code);
        }
        keys.insert(code.to_string(), json!(pair));
        labels.push((code, pair));
    }
    // the ISO key (between left Shift and Z) only matters when it types something no other key does
    let elsewhere: HashSet<&str> = labels.iter().filter(|(c, _)| *c != 86).flat_map(|(_, p)| p.iter().map(String::as_str)).collect();
    let iso = labels.iter().any(|(c, p)| *c == 86 && p.iter().any(|l| !l.is_empty() && !elsewhere.contains(l.as_str())));
    println!("{}", json!({"layout": layout, "variant": variant, "iso": iso, "keys": keys, "letters": letters, "combine": combine}));
    0
}
