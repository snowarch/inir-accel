//! Core library for `inir-rust-plugin`: ultra-fast native algorithms for iNiR shell.
//! This code was written by: Hikari.
#![recursion_limit = "512"]

pub mod apps;
pub mod daemons;
pub mod gojson;
pub mod gojsonv;
pub mod gomath;
pub mod gotheme;
pub mod helpers;
pub mod material;
pub mod opencode;
pub mod prewarm;
pub mod region;
pub mod sensors;
pub mod themegens;
pub mod thumbgen;
pub mod vscode;
pub mod washi;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_washi_default_and_airisu() {
        let req = washi::default_request();
        let out = washi::solve_all(&req, "{}");
        assert_eq!(out.version, washi::VERSION);
        assert!(out.schemes.contains_key("dark"));
        assert!(out.schemes.contains_key("ink"));
        assert!(out.schemes.contains_key("light"));

        let mut airisu_req = washi::default_request();
        airisu_req.language = "airisu".to_string();
        airisu_req.accent = "design".to_string();
        let airisu_out = washi::solve_schemes(&airisu_req, "{}");
        assert_eq!(airisu_out.schemes["dark"].surface, "#0b0b0c");
    }

    #[test]
    fn test_washi_contrast_guarantees() {
        let req = washi::default_request();
        let out = washi::solve_schemes(&req, "");
        for (scheme_name, p) in &out.schemes {
            let surf = washi::parse_hex(&p.surface).unwrap();
            let text = washi::parse_hex(&p.text).unwrap();
            let c = washi::contrast(text, surf);
            assert!(c >= 6.98, "scheme {scheme_name} text contrast {c} < 7.0");
        }
    }

    #[test]
    fn test_sensors_and_resources() {
        let sensors = sensors::detect_sensors("/sys");
        let snap = sensors::sample_resources(&sensors);
        assert!(snap.memory_total_kb > 0);
        assert!(snap.disk_total_bytes > 0);
    }

    #[test]
    fn test_desktop_entries_and_fuzzy() {
        let entries = apps::scan_desktop_entries();
        let icon = apps::guess_icon("code", &entries);
        assert!(!icon.is_empty());
        let steam = apps::guess_icon("steam_app_730", &entries);
        assert_eq!(steam, "steam_icon_730");
    }

    #[test]
    fn test_clipboard_markup_stripping() {
        let raw = b"<meta http-equiv=\"content-type\" content=\"text/html; charset=utf-8\"><div>Hello <b>world</b>!<br>Second line</div>";
        assert!(helpers::is_browser_markup(raw));
        let cleaned = helpers::strip_browser_markup(&String::from_utf8_lossy(raw));
        assert_eq!(cleaned, "Hello world!\nSecond line");
    }

    #[test]
    fn test_jsonc_strip() {
        let jsonc = "{\n  // comment\n  \"a\": 1,\n  /* block */\n  \"b\": [2, 3,]\n}";
        let stripped = themegens::strip_jsonc_comments(jsonc);
        let parsed: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(parsed["a"], 1);
    }
}

