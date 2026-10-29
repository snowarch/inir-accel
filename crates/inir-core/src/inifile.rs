//! `ini-set <file> <section> <key> <value> [--replace-invalid]`: one key of an INI file (kdeglobals, qt5ct/qt6ct,
//! GTK's settings.ini) set in place. Replaces IconThemeService's inline configparser, which rewrote the whole file
//! and lost its comments; every other line stays as it was, and the file is written through any link it is.
//! `--replace-invalid` is GTK's rule: a file GTK can't read (text before the first group, or no such group) is kept
//! aside as `<file>.corrupt-<stamp>.bak` and written fresh.

use std::path::Path;

fn header(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[').and_then(|r| r.strip_suffix(']'))
}

fn readable(lines: &[&str], section: &str) -> bool {
    let mut seen_group = false;
    for l in lines {
        let t = l.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        if header(l).is_some() {
            seen_group = true;
        } else if !seen_group {
            return false;
        }
    }
    lines.iter().any(|l| header(l) == Some(section))
}

/// The file with `key=value` in `[section]`: the key's first line replaced, else added after the section's last line,
/// else a new section at the end.
fn set(text: &str, section: &str, key: &str, value: &str) -> String {
    let entry = format!("{key}={value}");
    let mut lines: Vec<String> = if text.is_empty() { Vec::new() } else { text.split('\n').map(str::to_string).collect() };
    let trailing = text.ends_with('\n');
    if trailing {
        lines.pop();
    }
    if let Some(h) = lines.iter().position(|l| header(l) == Some(section)) {
        let end = lines[h + 1..].iter().position(|l| header(l).is_some()).map_or(lines.len(), |i| h + 1 + i);
        let found = lines[h + 1..end].iter().position(|l| l.split_once('=').is_some_and(|(k, _)| k.trim() == key));
        match found {
            Some(i) => lines[h + 1 + i] = entry,
            None => {
                let last = lines[h + 1..end].iter().rposition(|l| !l.trim().is_empty()).map_or(h, |i| h + 1 + i);
                lines.insert(last + 1, entry);
            }
        }
    } else {
        if lines.iter().any(|l| !l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        lines.push(entry);
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub fn run_cli(args: &[String]) -> i32 {
    let replace_invalid = args.iter().any(|a| a == "--replace-invalid");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--replace-invalid").collect();
    let [file, section, key, value] = rest[..] else {
        eprintln!("usage: inir-accel ini-set <file> <section> <key> <value> [--replace-invalid]");
        return 2;
    };
    let path = Path::new(file);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = match std::fs::read(path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            eprintln!("ini-set: {file}: {e}");
            return 1;
        }
    };
    let lines: Vec<&str> = text.lines().collect();
    let text = if replace_invalid && !readable(&lines, section) {
        if !text.is_empty() {
            let stamp = crate::pyrt::now_strftime("%Y%m%d-%H%M%S");
            if let Err(e) = std::fs::copy(path, format!("{file}.corrupt-{stamp}.bak")) {
                eprintln!("ini-set: cannot keep {file} aside: {e}");
                return 1;
            }
        }
        String::new()
    } else {
        text
    };
    match std::fs::write(path, set(&text, section, key, value)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("ini-set: {file}: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::set;

    #[test]
    fn keeps_every_other_line() {
        let kde = "# mine\n[General]\nName=x\n\n[Icons]\nTheme = Old\n\n[KDE]\nA=1\n";
        assert_eq!(set(kde, "Icons", "Theme", "Papirus"), "# mine\n[General]\nName=x\n\n[Icons]\nTheme=Papirus\n\n[KDE]\nA=1\n");
    }

    #[test]
    fn adds_the_key_or_the_section() {
        assert_eq!(set("[Appearance]\nstyle=Fusion\n\n[Fonts]\n", "Appearance", "icon_theme", "X"), "[Appearance]\nstyle=Fusion\nicon_theme=X\n\n[Fonts]\n");
        assert_eq!(set("[General]\nA=1", "Icons", "Theme", "X"), "[General]\nA=1\n\n[Icons]\nTheme=X\n");
        assert_eq!(set("", "Settings", "gtk-icon-theme-name", "X"), "[Settings]\ngtk-icon-theme-name=X\n");
    }
}
