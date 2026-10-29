//! Keeps the ii-pixel SDDM theme in step with iNiR (appearance, colours, picture, avatar), ported from
//! `sync-pixel-sddm.py`. The colour pipeline runs it (scripts/colors/modules/60-sddm.sh); install-pixel-sddm.sh gives
//! the theme directory to the person, so it needs no sudo. Nothing is written when theme.conf already matches.

use crate::material::py::{self, Json};
use crate::pyrt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const THEME_DIR: &str = "/usr/share/sddm/themes/ii-pixel";
const TEMPLATE: &str = "[SddmTheme]\nName=ii-pixel\nDescription=iNiR SDDM login screen — Material You dynamic colors\nType=sddm-theme\n\
Author=iNiR project\nVersion=1.0\nWebsite=https://github.com/snowarch/iNiR\nScreenshot=\nMainScript=Main.qml\nConfigFile=theme.conf\n\n\
[General]\nbackground=assets/background.png\ndefaultBackground=assets/background.png\nblurRadius=50\n\n\
# iNiR: updated automatically by inir-accel sddm-sync";
const RETIRED: [&str; 22] = [
    "irisBlur", "irisSaturation", "irisDim", "irisVignette", "irisScrim", "irisScrimStrength", "irisFit", "irisClockColour", "irisClockSize",
    "irisClockWeight", "irisClockTracking", "irisClockSeconds", "irisClock", "irisClockZone", "irisClockStyle", "irisClockDate", "irisClockFx",
    "irisClockFy", "irisSessionZone", "irisSessionFx", "irisSessionFy", "irisSessionWidth",
];
const VIDEO: [&str; 7] = [".mp4", ".mkv", ".webm", ".avi", ".mov", ".gif", ".webp"];

/// Under sudo the files are still the person's.
fn sudo_user() -> String {
    std::env::var("SUDO_USER").unwrap_or_default()
}

fn real_home() -> PathBuf {
    let user = sudo_user();
    if !user.is_empty() {
        if let Ok(name) = std::ffi::CString::new(user) {
            // SAFETY: getpwnam returns a pointer into static storage or null; it is read at once.
            let pw = unsafe { libc::getpwnam(name.as_ptr()) };
            if !pw.is_null() {
                let dir = unsafe { std::ffi::CStr::from_ptr((*pw).pw_dir) };
                return PathBuf::from(dir.to_string_lossy().into_owned());
            }
        }
    }
    PathBuf::from(py::expanduser("~"))
}

fn xdg(var: &str, home: &Path, fallback: &str) -> PathBuf {
    std::env::var(var).ok().filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(fallback))
}

fn load(path: &Path) -> Option<Json> {
    py::json_loads(&fs::read_to_string(path).ok()?)
}

/// `dig(data, *keys, default=None)`: the default past a missing key, a null or a non-object.
fn dig<'a>(data: &'a Json, keys: &[&str]) -> Option<&'a Json> {
    keys.iter().try_fold(data, |d, k| d.get(k).filter(|v| !matches!(v, Json::Null)))
}

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

/// Python's `a or b`
fn or(a: Option<&Json>, b: Json) -> Json {
    a.filter(|v| v.truthy()).cloned().unwrap_or(b)
}

fn read_colors(generated: &Path) -> Option<Vec<(&'static str, Json)>> {
    let app = generated.join("app-palette.json");
    let Some(source) = [app.clone(), generated.join("palette.json"), generated.join("colors.json")].into_iter().find(|p| p.is_file()) else {
        println!("[sddm-pixel] generated palette not found: {}", app.display());
        return None;
    };
    let data = load(&source).filter(Json::truthy).unwrap_or(Json::Obj(vec![]));
    let mut dark = data.get("colors").and_then(|c| c.get("dark")).cloned().unwrap_or(Json::Obj(vec![]));
    if !dark.truthy() {
        if data.get("primary").is_some() || data.get("on_surface").is_some() {
            dark = data;
        } else {
            println!("[sddm-pixel] No dark colors in generated palette");
            return None;
        }
    }
    let pick = |a: &str, b: &str, d: &str| or(dark.get(a), dark.get(b).cloned().unwrap_or_else(|| s(d)));
    Some(vec![
        ("primaryColor", pick("app_accent", "primary", "#cba6f7")),
        ("onPrimaryColor", pick("app_on_accent", "on_primary", "#1e1e2e")),
        ("surfaceColor", pick("app_background", "surface", "#1e1e2e")),
        ("surfaceContainerColor", pick("app_surface", "surface_container", "#181825")),
        ("onSurfaceColor", pick("app_foreground", "on_surface", "#cdd6f4")),
        ("onSurfaceVariantColor", pick("app_subtext", "on_surface_variant", "#9399b2")),
        ("backgroundColor", pick("app_background", "background", "#1e1e2e")),
        ("errorColor", dark.get("error").cloned().unwrap_or_else(|| s("#f38ba8"))),
    ])
}

fn read_appearance(config: &Json) -> String {
    let chosen = dig(config, &["lock", "loginScreen"]).map_or("auto".to_string(), pyrt::str_of);
    if chosen == "classic" || chosen == "iris" {
        return chosen;
    }
    if matches!(config.get("panelFamily"), Some(Json::Str(f)) if f == "iris") { "iris" } else { "classic" }.to_string()
}

/// What the iRiS login styles read, resolved for SDDM's own Qt: the iRiS palette and type, and which of the lock's
/// choices a login keeps (the picture, who you are, the clock and date formats).
fn read_iris(config: &Json, palette: Option<&[(&str, Json)]>, generated: &Path) -> Vec<(&'static str, Json)> {
    let obj = |v: Option<&Json>| v.filter(|v| v.truthy()).cloned().unwrap_or(Json::Obj(vec![]));
    let lock = obj(dig(config, &["iris", "lock"]));
    let appearance = obj(dig(config, &["iris", "appearance"]));
    let scene = obj(lock.get("scene"));
    let kind = obj(lock.get("type"));
    let session = obj(dig(&lock, &["blocks", "session"]));
    let washi_file = load(&generated.join("iris-washi.json")).filter(Json::truthy).unwrap_or(Json::Obj(vec![]));
    let washi = obj(dig(&washi_file, &["schemes", "dark"]));
    let from_palette = |k: &str| palette.and_then(|p| p.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone()));
    let accent = or(washi.get("accent"), or(from_palette("primaryColor").as_ref(), s("#a8c7fa")));
    let highlight = or(washi.get("highlight"), s("#ff9f0a"));
    let follow = appearance.get("followTheme").map_or(true, Json::truthy);
    let material = if follow { "theme".to_string() } else { dig(&appearance, &["theme", "surface"]).map_or("black".to_string(), pyrt::str_of) };
    let surface = or(dig(&washi, &["materials", &material]), or(from_palette("backgroundColor").as_ref(), s("#000000")));
    let main_font = or(appearance.get("fontFamily"), s("Inter"));
    let clock_font = match kind.get("clockFont").map_or("numbers".to_string(), pyrt::str_of).as_str() {
        "main" => main_font.clone(),
        "title" => or(appearance.get("titleFontFamily"), s("Inter Display")),
        _ => or(appearance.get("numbersFontFamily"), s("Rubik")),
    };
    let flag = |v: Option<&Json>| s(if v.map_or(true, Json::truthy) { "true" } else { "false" });
    let style = dig(config, &["lock", "loginStyle"]).map_or("lens".to_string(), pyrt::str_of);
    let get = |o: &Json, k: &str, d: Json| o.get(k).cloned().unwrap_or(d);
    vec![
        ("irisLoginStyle", s(if ["cover", "frame", "lens"].contains(&style.as_str()) { &style } else { "lens" })),
        ("irisSceneSource", get(&scene, "source", s("desktop"))),
        ("irisSurface", surface),
        ("irisDanger", or(washi.get("danger"), s("#ff6961"))),
        ("irisAccent", accent),
        ("irisHighlight", highlight),
        ("irisFontMain", main_font),
        ("irisFontClock", clock_font),
        ("irisTypeScale", get(&kind, "scale", Json::Int(100))),
        ("irisClockFormat", get(&kind, "clockFormat", s("auto"))),
        ("irisDateFormat", get(&kind, "dateFormat", s("long"))),
        ("irisAvatar", flag(session.get("avatar"))),
        ("irisName", flag(session.get("name"))),
        ("irisHint", flag(session.get("hint"))),
    ]
}

/// Sets `values` in theme.conf's [General] section, restoring the template when its structure is gone.
fn update_theme_conf(conf: &Path, values: &[(&str, String)]) -> bool {
    let Ok(before) = fs::read_to_string(conf) else {
        println!("[sddm-pixel] theme.conf not found: {}", conf.display());
        return false;
    };
    let mut lines: Vec<String> = before.split('\n').map(String::from).collect();
    let has_general = lines.iter().any(|l| l.contains("[General]"));
    let has_background = lines.iter().any(|l| py::strip(l).starts_with("background="));
    if !has_general || !has_background {
        println!("[sddm-pixel] theme.conf missing structural elements — restoring template");
        lines = TEMPLATE.split('\n').map(String::from).collect();
    }
    let mut remaining: Vec<(&str, String)> = values.to_vec();
    let mut out = Vec::new();
    for line in lines {
        let key = line.split_once('=').map(|(k, _)| py::strip(k).to_string());
        if key.as_deref().is_some_and(|k| RETIRED.contains(&k)) {
            continue;
        }
        match key.and_then(|k| remaining.iter().position(|(r, _)| *r == k)) {
            Some(i) => {
                let (k, v) = remaining.remove(i);
                out.push(format!("{k}={v}"));
            }
            None => out.push(line),
        }
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.extend(remaining.iter().map(|(k, v)| format!("{k}={v}")));
    let content = out.join("\n") + "\n";
    if content == before {
        return true;
    }
    match fs::write(conf, content) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            println!("[sddm-pixel] Permission denied writing {}.", conf.display());
            println!("[sddm-pixel] Re-run install-pixel-sddm.sh to fix ownership.");
            false
        }
        Err(e) => {
            println!("[sddm-pixel] Error writing theme.conf: {}", e);
            false
        }
    }
}

fn mtime_secs(p: &Path) -> Option<i64> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(p).ok().map(|m| m.mtime())
}

/// Copies keep the source's size and mtime, so a match means nothing to copy.
fn same_file(src: &Path, dst: &Path) -> bool {
    match (fs::metadata(src), fs::metadata(dst)) {
        (Ok(a), Ok(b)) => a.len() == b.len() && mtime_secs(src) == mtime_secs(dst),
        _ => false,
    }
}

/// `shutil.copy2` then chmod 644.
fn copy2(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(src)?;
    fs::copy(src, dst)?;
    let times = fs::FileTimes::new().set_accessed(meta.accessed()?).set_modified(meta.modified()?);
    fs::OpenOptions::new().write(true).open(dst)?.set_times(times)?;
    fs::set_permissions(dst, fs::Permissions::from_mode(0o644))
}

fn copy_asset(assets: &Path, src: &Path, name: &str) -> bool {
    if !assets.is_dir() {
        if let Err(e) = fs::create_dir_all(assets) {
            println!("[sddm-pixel] Cannot create {}: {}", assets.display(), e);
            return false;
        }
    }
    let dst = assets.join(name);
    if same_file(src, &dst) {
        return true;
    }
    match copy2(src, &dst) {
        Ok(()) => true,
        Err(e) => {
            println!("[sddm-pixel] Copy to {} failed: {}", dst.display(), e);
            false
        }
    }
}

/// The first frame of a video as a PNG in its own temp dir; None without ffmpeg.
fn video_frame(video: &Path) -> Option<PathBuf> {
    if pyrt::which("ffmpeg").is_none() {
        println!("[sddm-pixel] ffmpeg not found — cannot extract video frame");
        return None;
    }
    // a fresh 700 dir nobody could have planted: this can run as root
    let dir = (0..16).find_map(|_| {
        let mut b = [0u8; 8];
        fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut b)).ok()?;
        let dir = std::env::temp_dir().join(format!("sddm-pixel-{:016x}", u64::from_le_bytes(b)));
        std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700).create(&dir).ok().map(|()| dir)
    })?;
    let out = dir.join("frame.png");
    let mut child = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(video)
        .args(["-vframes", "1", "-update", "1", "-f", "image2"])
        .arg(&out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if start.elapsed() < Duration::from_secs(15) => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                println!("[sddm-pixel] ffmpeg error: timed out after 15 seconds");
                break false;
            }
        }
    };
    let stamped = ok && out.is_file() && fs::metadata(video).and_then(|m| Ok(fs::FileTimes::new().set_accessed(m.accessed()?).set_modified(m.modified()?))).and_then(|t| fs::OpenOptions::new().write(true).open(&out)?.set_times(t)).is_ok();
    if stamped {
        Some(out)
    } else {
        let _ = fs::remove_dir_all(&dir);
        None
    }
}

/// A wallpaper (or its first video frame) into the theme as `name`.
fn update_picture(assets: &Path, path: &str, name: &str) -> bool {
    let p = Path::new(path);
    if path.is_empty() || !p.is_file() {
        return false;
    }
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
    if !VIDEO.contains(&ext.as_str()) {
        return copy_asset(assets, p, name);
    }
    let dst = assets.join(name);
    if dst.is_file() && mtime_secs(&dst).is_some() && mtime_secs(&dst) == mtime_secs(p) {
        return true;
    }
    let Some(frame) = video_frame(p) else {
        println!("[sddm-pixel] Keeping the existing picture (video, no frame)");
        return false;
    };
    let done = copy_asset(assets, &frame, name);
    if let Some(dir) = frame.parent() {
        let _ = fs::remove_dir_all(dir);
    }
    done
}

/// Pillow's `Image.resize` on an "L" image with its default BICUBIC filter (Resample.c, 8 bits per channel).
fn pillow_bicubic(src: &[u8], w: usize, h: usize, ow: usize, oh: usize) -> Vec<u8> {
    const PRECISION_BITS: u32 = 32 - 8 - 2;
    fn filter(x: f64) -> f64 {
        let (a, x) = (-0.5, x.abs());
        if x < 1.0 {
            ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
        } else if x < 2.0 {
            (((x - 5.0) * x + 8.0) * x - 4.0) * a
        } else {
            0.0
        }
    }
    // (first input index, taps) per output index, taps in Pillow's fixed point
    fn coeffs(in_size: usize, out_size: usize) -> Vec<(usize, Vec<i64>)> {
        let scale = in_size as f64 / out_size as f64;
        let filterscale = scale.max(1.0);
        let support = 2.0 * filterscale;
        (0..out_size)
            .map(|xx| {
                let center = (xx as f64 + 0.5) * scale;
                let ss = 1.0 / filterscale;
                let xmin = ((center - support + 0.5) as i64).max(0) as usize;
                let xmax = ((center + support + 0.5) as i64).min(in_size as i64) as usize - xmin;
                let k: Vec<f64> = (0..xmax).map(|x| filter((x as f64 + xmin as f64 - center + 0.5) * ss)).collect();
                let ww: f64 = k.iter().sum();
                let k = k.into_iter().map(|v| if ww != 0.0 { v / ww } else { v });
                let fixed = k.map(|v| if v < 0.0 { (-0.5 + v * f64::from(1u32 << PRECISION_BITS)) as i64 } else { (0.5 + v * f64::from(1u32 << PRECISION_BITS)) as i64 }).collect();
                (xmin, fixed)
            })
            .collect()
    }
    if ow == 0 || oh == 0 || w == 0 || h == 0 {
        return Vec::new();
    }
    let clip8 = |v: i64| -> u8 { (v >> PRECISION_BITS).clamp(0, 255) as u8 };
    let start = 1i64 << (PRECISION_BITS - 1);
    let (horiz, vert) = (coeffs(w, ow), coeffs(h, oh));
    let need_h = ow != w;
    let need_v = oh != h;
    let (ybox_first, ybox_last) = (vert[0].0, vert[oh - 1].0 + vert[oh - 1].1.len());
    let (mut img, mut iw, mut vshift) = (src.to_vec(), w, 0);
    if need_h {
        let rows = ybox_last - ybox_first;
        let mut tmp = vec![0u8; ow * rows];
        for y in 0..rows {
            let row = &img[(y + ybox_first) * w..(y + ybox_first + 1) * w];
            for (xx, (xmin, k)) in horiz.iter().enumerate() {
                tmp[y * ow + xx] = clip8(k.iter().enumerate().fold(start, |ss, (x, c)| ss + i64::from(row[xmin + x]) * c));
            }
        }
        (img, iw, vshift) = (tmp, ow, ybox_first);
    }
    if need_v {
        let mut out = vec![0u8; iw * oh];
        for (yy, (ymin, k)) in vert.iter().enumerate() {
            let ymin = ymin - vshift;
            for x in 0..iw {
                out[yy * iw + x] = clip8(k.iter().enumerate().fold(start, |ss, (y, c)| ss + i64::from(img[(ymin + y) * iw + x]) * c));
            }
        }
        img = out;
    }
    img
}

/// A veil for a light picture: white text needs ~4.5:1 over the blurred mean, so it dims by what it lacks.
/// Pillow read the file unrotated; imread turns EXIF-rotated photos, which can move the mean a hair.
fn picture_lift(path: &Path) -> Json {
    let crate::region::decode::Read::Ok(m) = crate::region::decode::imread(&path.to_string_lossy(), false, None) else { return Json::Int(0) };
    let luma: Vec<u8> = match m.cn {
        1 => m.data.clone(),
        _ => m.data.chunks(m.cn).map(|p| ((u32::from(p[2]) * 19595 + u32::from(p[1]) * 38470 + u32::from(p[0]) * 7471 + 0x8000) >> 16) as u8).collect(),
    };
    if m.w == 0 || m.h == 0 {
        return Json::Int(0);
    }
    let small = pillow_bicubic(&luma, m.w, m.h, 48, 27);
    if small.is_empty() {
        return Json::Int(0);
    }
    let lum = small.iter().map(|&v| f64::from(v)).sum::<f64>() / small.len() as f64 / 255.0;
    let v = ((lum - 0.42) * 0.9).min(0.45).max(0.0);
    Json::Float(format!("{v:.2}").parse().unwrap_or(0.0))
}

/// The desktop wallpaper the active family shows.
fn read_wallpaper(config: &Json) -> Option<String> {
    let text = |v: Option<&Json>| match v {
        Some(Json::Str(s)) => s.clone(),
        _ => String::new(),
    };
    let main = text(config.get("background").and_then(|b| b.get("wallpaperPath")));
    let waffle = dig(config, &["waffles", "background"]);
    let family = config.get("panelFamily").map_or("ii".to_string(), pyrt::str_of);
    let use_main = waffle.and_then(|w| w.get("useMainWallpaper")).map_or(true, Json::truthy);
    let mut path = if family == "waffle" && !use_main {
        Some(text(waffle.and_then(|w| w.get("wallpaperPath")))).filter(|p| !p.is_empty()).unwrap_or(main)
    } else {
        main
    };
    if let Some(p) = path.strip_prefix("file://") {
        path = p.to_string();
    }
    Some(path).filter(|p| !p.is_empty() && Path::new(p).is_file())
}

pub fn run_cli(_args: &[String]) -> i32 {
    let theme = Path::new(THEME_DIR);
    let assets = theme.join("assets");
    if !theme.is_dir() {
        println!("[sddm-pixel] Theme not installed at {THEME_DIR}. Run install-pixel-sddm.sh first.");
        return 0;
    }
    let home = real_home();
    let generated = xdg("XDG_STATE_HOME", &home, ".local/state").join("quickshell/user/generated");
    // the shell's own config dir (Directories.shellConfig), a link to ~/.config/inir once that exists
    let config = load(&xdg("XDG_CONFIG_HOME", &home, ".config").join("illogical-impulse/config.json")).filter(|c| matches!(c, Json::Obj(_))).unwrap_or(Json::Obj(vec![]));
    let colors = read_colors(&generated);
    let appearance = read_appearance(&config);
    let mut values: Vec<(&str, Json)> = vec![("appearance", s(&appearance))];
    values.extend(colors.iter().flatten().cloned());
    let shape = dig(&config, &["lock", "materialShapeChars"]).is_some_and(Json::truthy);
    values.push(("materialShapeChars", s(if shape { "true" } else { "false" })));
    let iris = read_iris(&config, colors.as_deref(), &generated);
    let custom = matches!(iris.iter().find(|(k, _)| *k == "irisSceneSource"), Some((_, Json::Str(v))) if v == "custom");
    values.extend(iris);
    let own = dig(&config, &["iris", "lock", "scene", "path"]).filter(|v| v.truthy()).map_or(String::new(), pyrt::str_of);
    let own = own.strip_prefix("file://").unwrap_or(&own).to_string();
    let picture = if custom && update_picture(&assets, &own, "lock-picture.png") { "assets/lock-picture.png" } else { "" };
    values.push(("irisPicture", s(picture)));
    let shown = if appearance == "iris" && !picture.is_empty() { theme.join(picture) } else { assets.join("background.png") };
    if !read_wallpaper(&config).is_some_and(|w| update_picture(&assets, &w, "background.png")) {
        println!("[sddm-pixel] No wallpaper path found, keeping existing background");
    }
    values.push(("irisLift", if shown.is_file() { picture_lift(&shown) } else { Json::Int(0) }));
    let values: Vec<(&str, String)> = values.iter().map(|(k, v)| (*k, pyrt::str_of(v))).collect();
    if update_theme_conf(&theme.join("theme.conf"), &values) {
        println!("[sddm-pixel] Synced ({appearance})");
    } else {
        println!("[sddm-pixel] Sync failed");
    }
    let user = Some(sudo_user()).filter(|u| !u.is_empty()).or_else(|| std::env::var("USER").ok()).unwrap_or_default();
    let mut faces = vec![home.join(".face"), home.join(".face.icon")];
    if !user.is_empty() {
        faces.push(PathBuf::from(format!("/var/lib/AccountsService/icons/{user}")));
    }
    if let Some(face) = faces.iter().find(|p| p.is_file()) {
        copy_asset(&assets, face, "user-face.png");
    }
    0
}
