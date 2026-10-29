//! Fast Desktop Entry scanner, Fuzzy search (fuzzysort + Levenshtein), and Window-to-Icon resolver.
//! Replaces the JS-heavy `AppSearch.qml`, `scan-plugins.py`, `scan-widgets.sh`, and `icon-theme-for-mode.sh`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopEntryInfo {
    pub id: String,
    pub name: String,
    pub generic_name: String,
    pub comment: String,
    pub icon: String,
    pub exec: String,
    pub startup_class: String,
    pub run_in_terminal: bool,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredApp {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub comment: String,
    pub score: f64,
}

/// Scan standard XDG application directories (`$XDG_DATA_HOME/applications`, `$XDG_DATA_DIRS/applications`)
/// and parse `.desktop` entries in ~1.5ms.
pub fn scan_desktop_entries() -> Vec<DesktopEntryInfo> {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg_data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
    let xdg_data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share:/var/lib/flatpak/exports/share".to_string());

    let mut dirs = vec![
        PathBuf::from(&xdg_data_home).join("applications"),
        PathBuf::from(&xdg_data_home).join("flatpak/exports/share/applications"),
    ];
    for d in xdg_data_dirs.split(':').filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(d).join("applications"));
    }

    let mut seen_ids = std::collections::HashSet::new();
    let mut entries = Vec::new();

    for dir in dirs {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "desktop"))
            .collect();
        files.sort();
        for file in files {
            let Some(fname) = file.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
            if !seen_ids.insert(fname.clone()) {
                continue;
            }
            if let Some(info) = parse_desktop_file(&file, &fname) {
                entries.push(info);
            }
        }
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn parse_desktop_file(path: &Path, id: &str) -> Option<DesktopEntryInfo> {
    let text = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut name = String::new();
    let mut generic_name = String::new();
    let mut comment = String::new();
    let mut icon = String::new();
    let mut exec = String::new();
    let mut startup_class = String::new();
    let mut no_display = false;
    let mut hidden = false;
    let mut run_in_terminal = false;
    let mut entry_type = String::new();
    let mut categories = Vec::new();
    let mut keywords = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let k = k.trim();
        let v = v.trim();
        match k {
            "Type" => entry_type = v.to_string(),
            "Name" if name.is_empty() => name = v.to_string(),
            "GenericName" if generic_name.is_empty() => generic_name = v.to_string(),
            "Comment" if comment.is_empty() => comment = v.to_string(),
            "Icon" => icon = v.to_string(),
            "Exec" => exec = v.to_string(),
            "StartupWMClass" => startup_class = v.to_string(),
            "NoDisplay" => no_display = v.eq_ignore_ascii_case("true"),
            "Hidden" => hidden = v.eq_ignore_ascii_case("true"),
            "Terminal" => run_in_terminal = v.eq_ignore_ascii_case("true"),
            "Categories" => {
                categories = v.split(';').filter(|s| !s.is_empty()).map(String::from).collect();
            }
            "Keywords" => {
                keywords = v.split(';').filter(|s| !s.is_empty()).map(String::from).collect();
            }
            _ => {}
        }
    }

    if no_display || hidden || (entry_type != "Application" && !entry_type.is_empty()) || name.is_empty() {
        return None;
    }

    Some(DesktopEntryInfo {
        id: id.to_string(),
        name,
        generic_name,
        comment,
        icon,
        exec,
        startup_class,
        run_in_terminal,
        categories,
        keywords,
    })
}

/// Fast fuzzy query matching `AppSearch.fuzzyQuery`: subsequence / Levenshtein + prefix/word-boundary boosts.
pub fn fuzzy_search_apps(entries: &[DesktopEntryInfo], query: &str, sloppy: bool) -> Vec<ScoredApp> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    for e in entries {
        let name_lower = e.name.to_lowercase();
        let score = if sloppy {
            let mut s = levenshtein_similarity(&name_lower, &q);
            if name_lower.starts_with(&q) {
                s += 0.3;
            } else if name_lower.contains(&format!(" {q}")) || name_lower.contains(&format!("-{q}")) {
                s += 0.15;
            } else if name_lower.contains(&q) {
                s += 0.1;
            }
            if s > 0.2 { Some(s.min(1.0)) } else { None }
        } else {
            subsequence_score(&name_lower, &q).map(|mut s| {
                if name_lower.starts_with(&q) {
                    s += 50000.0;
                } else if name_lower.contains(&format!(" {q}")) || name_lower.contains(&format!("-{q}")) {
                    s += 20000.0;
                } else if name_lower.contains(&q) {
                    s += 10000.0;
                }
                s
            })
        };

        if let Some(sc) = score {
            results.push(ScoredApp {
                id: e.id.clone(),
                name: e.name.clone(),
                icon: e.icon.clone(),
                comment: e.comment.clone(),
                score: sc,
            });
        }
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results
}

fn subsequence_score(target: &str, query: &str) -> Option<f64> {
    let mut t_chars = target.char_indices().peekable();
    let mut score = 0.0;
    let mut last_idx: Option<usize> = None;

    for qc in query.chars() {
        let mut found = None;
        while let Some(&(idx, tc)) = t_chars.peek() {
            t_chars.next();
            if tc == qc {
                found = Some(idx);
                break;
            }
        }
        let idx = found?;
        if let Some(prev) = last_idx {
            if idx == prev + 1 {
                score += 15.0; // consecutive bonus
            } else {
                score -= (idx - prev) as f64;
            }
        } else if idx == 0 {
            score += 25.0;
        } else {
            score -= idx as f64 * 0.5;
        }
        last_idx = Some(idx);
    }
    Some(score - (target.len() as f64 * 0.1))
}

fn levenshtein_similarity(a: &str, b: &str) -> f64 {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    if ac.is_empty() || bc.is_empty() {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=bc.len()).collect();
    let mut curr = vec![0; bc.len() + 1];
    for (i, &ca) in ac.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in bc.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    let dist = prev[bc.len()];
    let max_len = ac.len().max(bc.len()) as f64;
    (1.0 - (dist as f64 / max_len)).max(0.0)
}

/// Guess icon name for a window `appId` using all of `AppSearch.guessIcon`'s rules & substitutions.
pub fn guess_icon(app_id: &str, entries: &[DesktopEntryInfo]) -> String {
    let s = app_id.trim();
    if s.is_empty() {
        return "image-missing".to_string();
    }
    let lower = s.to_lowercase();
    let kebab = lower.replace(' ', "-");

    // 1. Desktop entries lookup (StartupWMClass, Exec basename, ID stem)
    for e in entries {
        let stem = e.id.strip_suffix(".desktop").unwrap_or(&e.id).to_lowercase();
        if stem == lower || stem == kebab {
            if !e.icon.is_empty() {
                return e.icon.clone();
            }
        }
        if !e.startup_class.is_empty() && e.startup_class.to_lowercase() == lower {
            if !e.icon.is_empty() {
                return e.icon.clone();
            }
        }
        if let Some(first_cmd) = e.exec.split_whitespace().next() {
            let base = first_cmd.rsplit('/').next().unwrap_or(first_cmd).to_lowercase();
            if !matches!(base.as_str(), "bash" | "sh" | "env" | "python" | "python3" | "java" | "electron" | "node")
                && (base == lower || base == kebab)
                && !e.icon.is_empty()
            {
                return e.icon.clone();
            }
        }
    }

    // 2. Direct substitutions table from AppSearch.qml
    let subs = [
        ("code-url-handler", "visual-studio-code"),
        ("code", "visual-studio-code"),
        ("code-oss", "visual-studio-code"),
        ("vscodium", "vscodium"),
        ("zed", "dev.zed.Zed"),
        ("firefox-esr", "firefox"),
        ("firefox-developer-edition", "firefox-developer-edition"),
        ("firefoxdeveloperedition", "firefox-developer-edition"),
        ("chromium-browser", "chromium"),
        ("google-chrome-stable", "google-chrome"),
        ("brave-browser", "brave"),
        ("microsoft-edge", "microsoft-edge"),
        ("microsoft-edge-stable", "microsoft-edge"),
        ("footclient", "foot"),
        ("foot-server", "foot"),
        ("alacritty", "Alacritty"),
        ("wezterm-gui", "org.wezfurlong.wezterm"),
        ("spotify-launcher", "spotify"),
        ("io.mpv.mpv", "mpv"),
        ("webcord", "webcord"),
        ("telegram-desktop", "telegram"),
        ("telegramdesktop", "telegram"),
        ("org.telegram.desktop", "telegram"),
        ("teams-for-linux", "teams"),
        ("zoom", "Zoom"),
        ("nautilus", "org.gnome.Nautilus"),
        ("dolphin", "system-file-manager"),
        ("org.kde.dolphin", "system-file-manager"),
        ("thunar", "org.xfce.thunar"),
        ("pcmanfm", "system-file-manager"),
        ("gnome-tweaks", "org.gnome.tweaks"),
        ("pavucontrol-qt", "pavucontrol"),
        ("pavucontrol", "multimedia-volume-control"),
        ("gnome-control-center", "org.gnome.Settings"),
        ("wps", "wps-office2019-kprometheus"),
        ("wpsoffice", "wps-office2019-kprometheus"),
        ("heroic", "com.heroicgameslauncher.hgl"),
        ("jetbrains-idea", "intellij-idea"),
        ("jetbrains-idea-ce", "idea"),
        ("figma-linux", "figma"),
        ("gnome-terminal", "utilities-terminal"),
        ("org.gnome.terminal", "utilities-terminal"),
        ("xdg-desktop-portal-gtk", "preferences-desktop"),
        ("polkit-gnome-authentication-agent-1", "dialog-password"),
        ("quickshell", "org.quickshell"),
        ("com.github.th_ch.youtube_music", "youtube-music"),
    ];
    for (k, v) in subs {
        if lower == k {
            return v.to_string();
        }
    }

    // 3. Regex-equivalent prefix/pattern substitutions
    if let Some(num) = lower.strip_prefix("steam_app_") {
        if num.chars().all(|c| c.is_ascii_digit()) {
            return format!("steam_icon_{num}");
        }
    }
    if lower.starts_with("steam_proton") || lower.starts_with("proton-") {
        return "steam".to_string();
    }
    if lower.starts_with("wine-") {
        return "wine".to_string();
    }
    if s.starts_with("Minecraft") {
        return "minecraft".to_string();
    }
    if lower.starts_with("net.lutris.") {
        return "lutris".to_string();
    }
    if let Some(jb) = lower.strip_prefix("jetbrains-") {
        return jb.to_string();
    }
    for prefix in ["discord", "slack", "teams", "zoom"] {
        if lower.starts_with(prefix) {
            return prefix.to_string();
        }
    }
    if lower.starts_with("soffice.") {
        return "libreoffice-startcenter".to_string();
    }
    if lower.contains("polkit") || lower.contains("gcr.prompter") {
        return "system-lock-screen".to_string();
    }

    s.to_string()
}

/// Resolve installed icon theme sibling for light/dark mode (`scripts/colors/icon-theme-for-mode.sh`).
pub fn icon_theme_for_mode(theme: &str, mode: &str) -> String {
    let theme = theme.trim();
    if theme.is_empty() {
        return String::new();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg_data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
    let xdg_data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());

    let mut icon_dirs = vec![
        PathBuf::from(xdg_data_home).join("icons"),
        PathBuf::from(&home).join(".icons"),
    ];
    for d in xdg_data_dirs.split(':').filter(|s| !s.is_empty()) {
        icon_dirs.push(PathBuf::from(d).join("icons"));
    }

    let installed = |cand: &str| -> bool {
        icon_dirs.iter().any(|d| d.join(cand).join("index.theme").is_file())
    };

    let mut base = theme;
    let mut suffix = "";
    for s in ["-dark", "-Dark", "-light", "-Light"] {
        if let Some(stripped) = theme.strip_suffix(s) {
            base = stripped;
            suffix = if s.eq_ignore_ascii_case("-dark") { "-dark" } else { "-light" };
            break;
        }
    }

    let candidates: Vec<String> = if mode == "dark" {
        if suffix == "-dark" {
            return theme.to_string();
        }
        let mut c = vec![format!("{base}-dark"), format!("{base}-Dark")];
        if suffix == "-light" {
            c.push(base.to_string());
        }
        c
    } else {
        if suffix != "-dark" {
            return theme.to_string();
        }
        vec![base.to_string(), format!("{base}-light"), format!("{base}-Light")]
    };

    for cand in candidates {
        if installed(&cand) {
            return cand;
        }
    }
    theme.to_string()
}

/// Scan custom widgets directory and output JSON manifest list (`scripts/scan-widgets.sh`).
pub fn scan_widgets_json(dir: &str, generation: Option<&str>) -> String {
    let root_dir = Path::new(dir);
    if !root_dir.is_dir() {
        return "[]".to_string();
    }

    let mut loads_dir: Option<PathBuf> = None;
    if let Some(gen_str) = generation.filter(|g| !g.is_empty()) {
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        let base = PathBuf::from(runtime).join("inir/widget-loads");
        let gdir = base.join(format!("g{gen_str}"));
        if fs::create_dir_all(&gdir).is_ok() {
            if let Ok(gen_num) = gen_str.parse::<i64>() {
                let keep_prev = format!("g{}", gen_num - 1);
                let keep_cur = format!("g{gen_str}");
                if let Ok(rd) = fs::read_dir(&base) {
                    for e in rd.flatten() {
                        let fname = e.file_name().to_string_lossy().into_owned();
                        if fname.starts_with('g') && fname != keep_cur && fname != keep_prev {
                            let _ = fs::remove_dir_all(e.path());
                        }
                    }
                }
            }
            loads_dir = Some(gdir);
        }
    }

    let Ok(rd) = fs::read_dir(root_dir) else {
        return "[]".to_string();
    };
    let mut subdirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    subdirs.sort();

    let mut items = Vec::new();
    for wdir in subdirs {
        let manifest_path = wdir.join("widget.json");
        if !manifest_path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(&manifest_path) else { continue };
        let Ok(manifest_val) = serde_json::from_str::<serde_json::Value>(&content) else { continue };
        let wid = wdir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let wdir_str = wdir.to_string_lossy().into_owned();
        let mut load_str = wdir_str.clone();

        if let Some(ref ldir) = loads_dir {
            let link_path = ldir.join(&wid);
            let _ = fs::remove_file(&link_path);
            if std::os::unix::fs::symlink(&wdir, &link_path).is_ok() {
                load_str = link_path.to_string_lossy().into_owned();
            }
        }

        items.push(serde_json::json!({
            "id": wid,
            "dir": wdir_str,
            "load": load_str,
            "manifest": manifest_val,
        }));
    }

    serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
}

/// Scan plugins directory (`scripts/scan-plugins.py`).
fn copy_tree(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

// The base's built-in web apps (defaults/plugins) land once, when the person has none yet.
fn bootstrap_default_plugins(defaults: &Path, plugins_dir: &Path) {
    let Ok(rd) = fs::read_dir(defaults) else { return };
    for src in rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let Some(name) = src.file_name() else { continue };
        if copy_tree(&src, &plugins_dir.join(name)).is_ok() {
            eprintln!("[Plugins] Installed default plugin: {}", name.to_string_lossy());
        }
    }
}

pub fn scan_plugins_json(defaults: Option<&Path>) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg_config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
    let inir_dir = PathBuf::from(&xdg_config).join("inir");
    let legacy_dir = PathBuf::from(&xdg_config).join("illogical-impulse");
    // same order as scripts/lib/config-path.sh: a real legacy dir still wins
    let config_dir = if legacy_dir.is_symlink() && inir_dir.is_dir() {
        inir_dir
    } else if legacy_dir.is_dir() {
        legacy_dir
    } else {
        inir_dir
    };

    scan_plugins_dir(&config_dir.join("plugins"), defaults)
}

fn scan_plugins_dir(plugins_dir: &Path, defaults: Option<&Path>) -> String {
    let empty = fs::read_dir(&plugins_dir).map_or(true, |mut rd| rd.next().is_none());
    if let (true, Some(defaults)) = (empty, defaults) {
        bootstrap_default_plugins(defaults, &plugins_dir);
    }
    if !plugins_dir.is_dir() {
        return "[]".to_string();
    }

    let Ok(rd) = fs::read_dir(&plugins_dir) else {
        return "[]".to_string();
    };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    entries.sort();

    let mut out = Vec::new();
    for pdir in entries {
        let manifest_path = pdir.join("manifest.json");
        let Ok(raw) = fs::read_to_string(&manifest_path) else { continue };
        let Ok(mut data) = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(&raw) else { continue };
        let Some(id_val) = data.get("id").and_then(|v| v.as_str()).map(String::from) else { continue };
        if !data.contains_key("url") {
            continue;
        }
        data.entry("name".to_string()).or_insert_with(|| serde_json::Value::String(id_val));
        data.entry("icon".to_string()).or_insert_with(|| serde_json::Value::String("language".into()));
        data.entry("display".to_string()).or_insert_with(|| serde_json::Value::String("tab".into()));
        data.entry("version".to_string()).or_insert_with(|| serde_json::Value::String("1.0".into()));

        if let Some(icon_rel) = data.get("iconPath").and_then(|v| v.as_str()) {
            let full = pdir.join(icon_rel);
            if full.is_file() {
                data.insert("faviconPath".to_string(), serde_json::Value::String(full.to_string_lossy().into_owned()));
            }
        }

        if let Some(scripts) = data.get("userscripts").and_then(|v| v.as_array()).cloned() {
            let mut resolved = Vec::new();
            let mut sources = Vec::new();
            for s in scripts {
                if let Some(s_str) = s.as_str() {
                    let full = pdir.join(s_str);
                    if full.is_file() {
                        resolved.push(serde_json::Value::String(full.to_string_lossy().into_owned()));
                        let src = fs::read_to_string(&full).unwrap_or_default();
                        sources.push(serde_json::Value::String(src));
                    }
                }
            }
            data.insert("userscriptPaths".to_string(), serde_json::Value::Array(resolved));
            data.insert("userscriptSources".to_string(), serde_json::Value::Array(sources));
        }

        out.push(data);
    }

    serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_plugins_bootstraps_defaults_once() {
        let root = std::env::temp_dir().join(format!("inir-scan-plugins-{}", std::process::id()));
        let defaults = root.join("defaults");
        fs::create_dir_all(defaults.join("music/scripts")).unwrap();
        fs::write(defaults.join("music/manifest.json"), r#"{"id":"music","url":"https://music.youtube.com"}"#).unwrap();
        fs::write(defaults.join("music/scripts/a.js"), "x").unwrap();
        let plugins = root.join("plugins");

        let out: serde_json::Value = serde_json::from_str(&scan_plugins_dir(&plugins, Some(&defaults))).unwrap();
        assert_eq!(out[0]["id"], "music");
        assert_eq!(out[0]["name"], "music");
        assert!(plugins.join("music/scripts/a.js").is_file());

        fs::remove_dir_all(plugins.join("music")).unwrap();
        fs::create_dir_all(plugins.join("other")).unwrap();
        assert_eq!(scan_plugins_dir(&plugins, Some(&defaults)), "[]");
        fs::remove_dir_all(&root).unwrap();
    }
}
