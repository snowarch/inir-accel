//! The USB devices plugged in right now, with a readable name and a kind, ported from `devices/usb-snapshot.py`.
//! Read from sysfs, so it needs no daemon. Hubs, root hubs and mass storage are left out: storage is announced
//! as a drive, with its label and size.

use crate::pyrt;
use regex::Regex;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

const ROOT: &str = "/sys/bus/usb/devices";
static GENERIC: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"(?i)^(usb|gaming|wireless|2\.4g|generic|hid)\b|receiver|device|composite"));
static JUNK: LazyLock<Regex> =
    LazyLock::new(|| pyrt::re(r"(?i)demo|^uac\d*\b|^usb(\s*(device|audio|composite device))?$|^[0-9a-f]{4}$"));
static PAD: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"(?i)pad|controller|joystick|dualsense|dualshock|xbox|8bitdo|joy-?con"));
static WEARABLE: LazyLock<Regex> = LazyLock::new(|| pyrt::re(r"(?i)head(set|phone)|buds|earphone|airpods"));
static VENDOR_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    pyrt::re(r"(?i)[,.]?\s*(co\.?|ltd\.?|inc\.?|corp(oration)?\.?|llc|gmbh|technology|technologies|electronics?|semiconductor|international|computer|limited)\b")
});

fn read(dir: &Path, name: &str) -> String {
    fs::read(dir.join(name)).map(|b| String::from_utf8_lossy(&b).trim().to_string()).unwrap_or_default()
}

fn sorted_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}

// a product string that is only a model code: letters and digits, both, 4 to 14 of them
fn model_code(s: &str) -> bool {
    (4..=14).contains(&s.chars().count())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().any(|c| c.is_ascii_alphabetic())
}

fn short_vendor(vendor: &str) -> String {
    let mut cleaned = vendor.to_string();
    for _ in 0..3 {
        cleaned = VENDOR_NOISE.replace_all(&cleaned, "").trim_matches(&[' ', ',', '.', '-'][..]).to_string();
    }
    if cleaned.is_empty() { vendor.to_string() } else { cleaned }
}

fn kind_of(interfaces: &[String], device_class: &str, name: &str) -> Option<&'static str> {
    let head = |i: &String, n: usize| i.chars().take(n).collect::<String>();
    let classes: Vec<String> =
        if interfaces.is_empty() { vec![device_class.to_string()] } else { interfaces.iter().map(|i| head(i, 2)).collect() };
    let has = |c: &str| classes.iter().any(|x| x == c);
    if has("09") && classes.iter().all(|x| x == "09") {
        return None;
    }
    for i in interfaces {
        if i.starts_with("03") && i.ends_with("02") {
            return Some("mouse");
        }
        if i.starts_with("03") && i.ends_with("01") {
            return Some("keyboard");
        }
    }
    if interfaces.iter().any(|i| matches!(head(i, 4).as_str(), "ff5d" | "ff47" | "ff58")) || has("03") && PAD.is_match(name) {
        return Some("controller");
    }
    Some(if has("08") {
        return None;
    } else if has("0e") {
        "webcam"
    } else if has("01") {
        if WEARABLE.is_match(name) { "headset" } else { "audio" }
    } else if has("06") || interfaces.iter().any(|i| i.starts_with("ff42")) || has("e0") && (has("0a") || has("02")) {
        "phone"
    } else if has("e0") {
        "bluetooth"
    } else if has("07") {
        "printer"
    } else if has("0b") {
        "smartcard"
    } else if has("02") || has("0a") {
        "serial"
    } else if has("03") {
        if PAD.is_match(name) { "controller" } else { "input" }
    } else {
        "device"
    })
}

pub fn usb_snapshot() -> Value {
    let mut devices = Vec::new();
    for entry in sorted_names(Path::new(ROOT)) {
        if entry.contains(':') || entry.starts_with("usb") {
            continue;
        }
        let path = Path::new(ROOT).join(&entry);
        let (vendor_id, product_id) = (read(&path, "idVendor"), read(&path, "idProduct"));
        if vendor_id.is_empty() {
            continue;
        }
        let prefix = format!("{entry}:");
        let interfaces: Vec<String> = sorted_names(&path)
            .iter()
            .filter(|sub| sub.starts_with(&prefix))
            .map(|sub| {
                let sub = path.join(sub);
                format!("{}{}{}", read(&sub, "bInterfaceClass"), read(&sub, "bInterfaceSubClass"), read(&sub, "bInterfaceProtocol"))
            })
            .filter(|i| i.chars().count() == 6)
            .map(|i| i.to_lowercase())
            .collect();
        let (manufacturer, product) = (read(&path, "manufacturer"), read(&path, "product"));
        let Some(kind) = kind_of(&interfaces, &read(&path, "bDeviceClass").to_lowercase(), &product) else { continue };
        let vendor = short_vendor(&manufacturer);
        let name = if product.is_empty() || !vendor.is_empty() && JUNK.is_match(&product) {
            if vendor.is_empty() { product.clone() } else { vendor }
        } else if !vendor.is_empty()
            && (GENERIC.is_match(&product) || model_code(&product))
            && !product.to_lowercase().contains(&vendor.to_lowercase())
        {
            format!("{vendor} {product}")
        } else {
            product
        };
        devices.push(json!({
            "id": format!("{entry}/{vendor_id}:{product_id}"),
            "name": name.split_whitespace().collect::<Vec<_>>().join(" "),
            "kind": kind,
        }));
    }
    json!({"devices": devices})
}
