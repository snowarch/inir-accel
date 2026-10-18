// This code was written by: Hikari.
//! `inir-accel`: Multi-call ultra-fast CLI, sidecar, and transparent venv interceptor for iNiR.
//!
//! Can be invoked:
//! 1. Directly (`inir-accel <subcmd> ...`)
//! 2. Via direct script/binary symlink (`inir-washi`, `generate_colors_material.py`, `least_busy_region.py`, `thumbgen.py`, etc.)
//! 3. As `~/.local/state/quickshell/.venv/bin/python3` (transparent venv interceptor): intercepts heavy Python
//!    workloads (`generate_colors_material.py`, `least_busy_region.py`, `thumbgen.py`, `detect_sensors.py`, `scan-plugins.py`)
//!    in native Rust and transparently `exec`s `python3.real` for everything else.

use inir_core::{apps, daemons, helpers, material, prewarm, region, sensors, themegens, thumbgen, washi};
use std::env;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args
        .first()
        .and_then(|s| Path::new(s).file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // 1. Transparent Python venv interceptor mode (`python`, `python3`, `python3.12`, `python3.14`)
    if argv0 == "python" || argv0 == "python3" || argv0.starts_with("python3.") {
        handle_python_shim(&args);
    }

    // 2. Multi-call binary dispatch if invoked via script/binary symlink
    match argv0.as_str() {
        "inir-washi" => std::process::exit(cmd_washi(&args[1..])),
        "inir-vscode-themegen" => std::process::exit(themegens::run_vscode_themegen_cli(&args[1..])),
        "inir-zed-themegen" => std::process::exit(cmd_zed_themegen(&args[1..])),
        "inir-opencode-themegen" => std::process::exit(cmd_opencode_themegen(&args[1..])),
        "inir-system24-themegen" | "system24_palette.py" => {
            std::process::exit(cmd_system24_themegen(&args[1..]))
        }
        "generate_terminal_configs" | "generate_terminal_configs.py" => {
            std::process::exit(cmd_terminals(&args[1..]))
        }
        "claude_code_theme" | "claude_code_theme.py" => {
            std::process::exit(cmd_claude_code_theme(&args[1..]))
        }
        "extract_cover_colors" | "extract_cover_colors.py" => {
            std::process::exit(cmd_extract_cover_colors(&args[1..]))
        }
        "resolve_audio_source" | "resolve_audio_source.py" => {
            std::process::exit(helpers::run_resolve_audio_source_cli(&args[1..]))
        }
        "clipboard-store" | "clipboard-store.py" => {
            std::process::exit(helpers::run_clipboard_store_cli(&args[1..]))
        }
        "keyboard-lock-daemon" | "keyboard_lock_state_daemon" | "keyboard_lock_state_daemon.py" => {
            std::process::exit(cmd_keyboard_lock_daemon(&args[1..]))
        }
        "dev-hot-reload" | "dev_hot_reload" | "dev_hot_reload.py" => {
            std::process::exit(cmd_dev_hot_reload(&args[1..]))
        }
        "detect_sensors" | "detect_sensors.py" => std::process::exit(cmd_detect_sensors(&args[1..])),
        "scan-plugins" | "scan-plugins.py" => std::process::exit(cmd_scan_plugins()),
        "scan-widgets" | "scan-widgets.sh" => std::process::exit(cmd_scan_widgets(&args[1..])),
        "icon-theme-for-mode" | "icon-theme-for-mode.sh" => std::process::exit(cmd_icon_theme(&args[1..])),
        "generate_colors_material" | "generate_colors_material.py" => {
            std::process::exit(cmd_material_colors(&args[1..]))
        }
        "least_busy_region" | "least_busy_region.py" => {
            std::process::exit(cmd_region(&args[1..]))
        }
        "thumbgen" | "thumbgen.py" => {
            std::process::exit(cmd_thumbgen(&args[1..]))
        }
        _ => {}
    }

    let sub = args.get(1).map(String::as_str).unwrap_or("help");
    let rest = if args.len() > 2 { &args[2..] } else { &[] };

    let code = match sub {
        "washi" => cmd_washi(rest),
        "material-colors" | "colors" => cmd_material_colors(rest),
        "region" | "least-busy-region" => cmd_region(rest),
        "thumbgen" | "thumbnails" => cmd_thumbgen(rest),
        "terminals" | "generate-terminal-configs" => cmd_terminals(rest),
        "vscode-themegen" => themegens::run_vscode_themegen_cli(rest),
        "zed-themegen" => cmd_zed_themegen(rest),
        "opencode-themegen" => cmd_opencode_themegen(rest),
        "system24-themegen" => cmd_system24_themegen(rest),
        "claude-code-theme" => cmd_claude_code_theme(rest),
        "extract-cover-colors" => cmd_extract_cover_colors(rest),
        "resolve-audio-source" => helpers::run_resolve_audio_source_cli(rest),
        "clipboard-store" => helpers::run_clipboard_store_cli(rest),
        "keyboard-lock-daemon" => cmd_keyboard_lock_daemon(rest),
        "dev-hot-reload" => cmd_dev_hot_reload(rest),
        "ipc-probe" => cmd_ipc_probe(),
        "detect-sensors" | "sensors" => cmd_detect_sensors(rest),
        "resources" => cmd_resources(),
        "scan-plugins" => cmd_scan_plugins(),
        "scan-widgets" => cmd_scan_widgets(rest),
        "icon-theme" => cmd_icon_theme(rest),
        "fuzzy-apps" => cmd_fuzzy_apps(rest),
        "guess-icon" => cmd_guess_icon(rest),
        "prewarm" => cmd_prewarm(rest),
        "bench" => cmd_bench(),
        _ => {
            print_help();
            0
        }
    };
    std::process::exit(code);
}

fn find_real_python() -> PathBuf {
    if let Ok(p) = env::var("INIR_REAL_PYTHON") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return pb;
        }
    }
    let home = env::var("HOME").unwrap_or_default();
    let candidates = [
        format!("{home}/.local/state/quickshell/.venv/bin/python3.real"),
        format!("{home}/.local/share/uv/python/cpython-3.12-linux-x86_64-gnu/bin/python3.12"),
        "/usr/bin/python3".to_string(),
    ];
    for c in candidates {
        let p = PathBuf::from(&c);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("/usr/bin/python3")
}

fn exec_real_python(args: &[String]) -> ! {
    let real_py = find_real_python();
    let err = Command::new(&real_py).args(&args[1..]).exec();
    eprintln!("inir-accel: failed to exec {:?}: {}", real_py, err);
    std::process::exit(127);
}

fn handle_python_shim(args: &[String]) -> ! {
    // Skip simple interpreter flags (-u, -E, -B, -s, -O) to locate the script path
    let mut script_idx = None;
    for (idx, arg) in args.iter().enumerate().skip(1) {
        if matches!(arg.as_str(), "-u" | "-E" | "-B" | "-s" | "-O") {
            continue;
        }
        if arg.starts_with('-') {
            // e.g. -c, -m, --version -> delegate straight to real python
            exec_real_python(args);
        }
        script_idx = Some(idx);
        break;
    }

    let Some(idx) = script_idx else {
        exec_real_python(args);
    };

    let script_name = Path::new(&args[idx])
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let script_args = &args[idx + 1..];

    match script_name.as_str() {
        "generate_colors_material.py" => {
            let code = cmd_material_colors(script_args);
            if code == 0 {
                std::process::exit(0);
            }
            exec_real_python(args);
        }
        "least_busy_region.py" => {
            if region::run_cli(script_args).is_ok() {
                std::process::exit(0);
            }
            exec_real_python(args);
        }
        "thumbgen.py" => {
            if thumbgen::run_cli(script_args).is_ok() {
                std::process::exit(0);
            }
            exec_real_python(args);
        }
        "generate_terminal_configs.py" => {
            if themegens::run_generate_terminal_configs_cli(script_args).is_ok() {
                std::process::exit(0);
            }
            exec_real_python(args);
        }
        "claude_code_theme.py" => {
            if helpers::run_claude_code_theme_cli(script_args).is_ok() {
                std::process::exit(0);
            }
            exec_real_python(args);
        }
        "system24_palette.py" => {
            if themegens::run_system24_themegen_cli(script_args).is_ok() {
                std::process::exit(0);
            }
            exec_real_python(args);
        }
        "extract_cover_colors.py" => {
            if helpers::run_extract_cover_colors_cli(script_args).is_ok() {
                std::process::exit(0);
            }
            exec_real_python(args);
        }
        "resolve_audio_source.py" => {
            std::process::exit(helpers::run_resolve_audio_source_cli(script_args));
        }
        "clipboard-store.py" => {
            std::process::exit(helpers::run_clipboard_store_cli(script_args));
        }
        "keyboard_lock_state_daemon.py" => {
            std::process::exit(cmd_keyboard_lock_daemon(script_args));
        }
        "dev_hot_reload.py" => {
            std::process::exit(cmd_dev_hot_reload(script_args));
        }
        "detect_sensors.py" => {
            std::process::exit(cmd_detect_sensors(script_args));
        }
        "scan-plugins.py" => {
            std::process::exit(cmd_scan_plugins());
        }
        _ => {
            exec_real_python(args);
        }
    }
}

fn print_help() {
    println!(
        "inir-accel 0.2.0 — Unified Single-Binary Rust Runtime & Theme/Vision Accelerator for iNiR
Usage:
  inir-accel washi [--request '<json>' | --default] [--out <file>]
  inir-accel material-colors [--path <img> | --color <hex>] [--mode dark|light] [--scheme <name>] ...
  inir-accel region [--luma-grid <cell> | --largest-region | --color-only] [--screen-width W] [--screen-height H] <img>
  inir-accel thumbgen -d <dir> [-s normal|large|x-large|xx-large] [-w <workers>] [-i] [-r] [--machine_progress]
  inir-accel terminals [--scss <file>] [--colors <file>] [--terminal-json <file>] [--terminals ...]
  inir-accel vscode-themegen [--colors <file>] [--terminal-json <file>] [--scss <file>] [--list-forks] [--strip]
  inir-accel zed-themegen [--scss <file>] [--colors <file>] [--terminal-json <file>] [--output <file>]
  inir-accel opencode-themegen [scss] [palette] [terminal] [out-dir]
  inir-accel system24-themegen
  inir-accel claude-code-theme --terminal <file> --palette <file> --mode dark|light --out-dir <dir>
  inir-accel extract-cover-colors <image_path> [count] [output_path]
  inir-accel resolve-audio-source [--desktop-entry <id>] [--blocked-apps-json '<json>']
  inir-accel clipboard-store [--filter]
  inir-accel keyboard-lock-daemon [--once]
  inir-accel dev-hot-reload <shell_root> <shell_pid>
  inir-accel ipc-probe
  inir-accel detect-sensors [--sysfs /sys]
  inir-accel resources
  inir-accel scan-plugins
  inir-accel scan-widgets <widgets-dir> [generation]
  inir-accel icon-theme <theme> [dark|light]
  inir-accel fuzzy-apps <query> [--sloppy]
  inir-accel guess-icon <app-id>
  inir-accel prewarm [--root ~/inir] [--vfs <dir>]
  inir-accel bench"
    );
}

fn cmd_keyboard_lock_daemon(args: &[String]) -> i32 {
    let once = args.iter().any(|a| a == "--once");
    match daemons::run_keyboard_lock_daemon(once) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{{\"type\":\"error\",\"message\":{:?}}}", e.to_string());
            1
        }
    }
}

fn cmd_dev_hot_reload(args: &[String]) -> i32 {
    let Some(root_str) = args.first() else {
        eprintln!("usage: inir-accel dev-hot-reload <shell_root> <shell_pid>");
        return 1;
    };
    let Some(pid) = args.get(1).and_then(|s| s.parse::<i32>().ok()) else {
        eprintln!("usage: inir-accel dev-hot-reload <shell_root> <shell_pid>");
        return 1;
    };
    match daemons::run_dev_hot_reload(Path::new(root_str), pid) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("inir-accel dev-hot-reload: {e}");
            1
        }
    }
}

fn cmd_terminals(args: &[String]) -> i32 {
    match themegens::run_generate_terminal_configs_cli(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_zed_themegen(args: &[String]) -> i32 {
    match themegens::run_zed_themegen_cli(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_opencode_themegen(args: &[String]) -> i32 {
    match themegens::run_opencode_themegen_cli(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_system24_themegen(args: &[String]) -> i32 {
    match themegens::run_system24_themegen_cli(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_claude_code_theme(args: &[String]) -> i32 {
    match helpers::run_claude_code_theme_cli(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_extract_cover_colors(args: &[String]) -> i32 {
    match helpers::run_extract_cover_colors_cli(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_ipc_probe() -> i32 {
    match helpers::check_ipc_socket_alive() {
        Ok(sock) => {
            println!("{}", sock.display());
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn cmd_region(args: &[String]) -> i32 {
    match region::run_cli(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("inir-accel region: {e}");
            1
        }
    }
}

fn cmd_thumbgen(args: &[String]) -> i32 {
    match thumbgen::run_cli(args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("inir-accel thumbgen: {e}");
            1
        }
    }
}

fn cmd_washi(args: &[String]) -> i32 {
    let mut request_raw = String::new();
    let mut out_file = String::new();
    let mut is_default = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--request" | "-request" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    request_raw = v.clone();
                }
            }
            "--out" | "-out" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    out_file = v.clone();
                }
            }
            "--default" | "-default" => {
                is_default = true;
            }
            arg if arg.starts_with("--request=") => {
                request_raw = arg.trim_start_matches("--request=").to_string();
            }
            arg if arg.starts_with("--out=") => {
                out_file = arg.trim_start_matches("--out=").to_string();
            }
            _ => {}
        }
        i += 1;
    }

    let mut req = washi::default_request();
    if !is_default {
        if request_raw.is_empty() {
            eprintln!("inir-washi: --request or --default");
            return 2;
        }
        match serde_json::from_str::<washi::Request>(&request_raw) {
            Ok(parsed) => {
                req = parsed;
            }
            Err(e) => {
                eprintln!("inir-washi: {e}");
                return 2;
            }
        }
    } else {
        request_raw = serde_json::to_string(&req).unwrap_or_default();
    }

    let solved = washi::solve_all(&req, &request_raw);
    let Ok(data) = serde_json::to_string(&solved) else {
        return 1;
    };

    if !out_file.is_empty() {
        let out_path = Path::new(&out_file);
        if let Some(parent) = out_path.parent() {
            let fname = out_path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            let tmp = parent.join(format!(".{fname}.tmp"));
            if fs::write(&tmp, &data).is_ok() {
                let _ = fs::rename(&tmp, out_path);
            }
        }
    }

    print!("{data}");
    0
}

fn cmd_material_colors(args: &[String]) -> i32 {
    let mut cfg = material::MaterialGenConfig::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                cfg.path = args.get(i).cloned();
            }
            "--size" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    cfg.size = v;
                }
            }
            "--color" => {
                i += 1;
                cfg.color = args.get(i).cloned();
            }
            "--mode" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    cfg.mode = v.clone();
                }
            }
            "--scheme" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    cfg.scheme = v.clone();
                }
            }
            "--smart" => cfg.smart = true,
            "--transparency" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    cfg.transparency = v.clone();
                }
            }
            "--termscheme" => {
                i += 1;
                cfg.termscheme = args.get(i).cloned();
            }
            "--harmony" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    cfg.harmony = v;
                }
            }
            "--harmonize_threshold" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    cfg.harmonize_threshold = v;
                }
            }
            "--term_fg_boost" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    cfg.term_fg_boost = v;
                }
            }
            "--term_saturation" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    cfg.term_saturation = v;
                }
            }
            "--term_brightness" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    cfg.term_brightness = v;
                }
            }
            "--term_bg_brightness" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    cfg.term_bg_brightness = v;
                }
            }
            "--blend_bg_fg" => cfg.blend_bg_fg = true,
            "--cache" => {
                i += 1;
                cfg.cache = args.get(i).cloned();
            }
            "--soften" => cfg.soften = true,
            "--debug" => cfg.debug = true,
            "--json-output" => {
                i += 1;
                cfg.json_output = args.get(i).cloned();
            }
            "--palette-output" => {
                i += 1;
                cfg.palette_output = args.get(i).cloned();
            }
            "--app-palette-output" => {
                i += 1;
                cfg.app_palette_output = args.get(i).cloned();
            }
            "--terminal-output" => {
                i += 1;
                cfg.terminal_output = args.get(i).cloned();
            }
            "--meta-output" => {
                i += 1;
                cfg.meta_output = args.get(i).cloned();
            }
            "--scss-output" => {
                i += 1;
                cfg.scss_output = args.get(i).cloned();
            }
            "--surface-seed" => {
                i += 1;
                cfg.surface_seed = args.get(i).cloned();
            }
            "--accent-seed" => {
                i += 1;
                cfg.accent_seed = args.get(i).cloned();
            }
            "--washi-roles" => {
                i += 1;
                cfg.washi_roles = args.get(i).cloned();
            }
            "--render-templates" => {
                i += 1;
                cfg.render_templates = args.get(i).cloned();
            }
            "--color-strength" => {
                i += 1;
                if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                    cfg.color_strength = v;
                }
            }
            "--invert-hue" => cfg.invert_hue = true,
            _ => {}
        }
        i += 1;
    }

    match material::run_material_pipeline(cfg) {
        Ok(scss) => {
            print!("{scss}");
            0
        }
        Err(e) => {
            eprintln!("inir-accel material-colors: {e}");
            1
        }
    }
}

fn cmd_detect_sensors(args: &[String]) -> i32 {
    let sysfs = args.first().map(String::as_str).unwrap_or("/sys");
    for (k, v) in sensors::detect_sensors(sysfs) {
        println!("{k}:{v}");
    }
    0
}

fn cmd_resources() -> i32 {
    let s = sensors::detect_sensors("/sys");
    let snap = sensors::sample_resources(&s);
    println!("{}", serde_json::to_string(&snap).unwrap_or_default());
    0
}

fn cmd_scan_plugins() -> i32 {
    println!("{}", apps::scan_plugins_json());
    0
}

fn cmd_scan_widgets(args: &[String]) -> i32 {
    let Some(dir) = args.first() else {
        println!("[]");
        return 0;
    };
    let gen_str = args.get(1).map(String::as_str);
    println!("{}", apps::scan_widgets_json(dir, gen_str));
    0
}

fn cmd_icon_theme(args: &[String]) -> i32 {
    let Some(theme) = args.first() else {
        return 0;
    };
    let mode = args.get(1).map(String::as_str).unwrap_or("dark");
    let resolved = apps::icon_theme_for_mode(theme, mode);
    if !resolved.is_empty() {
        println!("{resolved}");
    }
    0
}

fn cmd_fuzzy_apps(args: &[String]) -> i32 {
    let query = args.first().map(String::as_str).unwrap_or("");
    let sloppy = args.iter().any(|a| a == "--sloppy");
    let entries = apps::scan_desktop_entries();
    let res = apps::fuzzy_search_apps(&entries, query, sloppy);
    println!("{}", serde_json::to_string(&res).unwrap_or_default());
    0
}

fn cmd_guess_icon(args: &[String]) -> i32 {
    let app_id = args.first().map(String::as_str).unwrap_or("");
    let entries = apps::scan_desktop_entries();
    println!("{}", apps::guess_icon(app_id, &entries));
    0
}

fn cmd_prewarm(args: &[String]) -> i32 {
    let home = env::var("HOME").unwrap_or_default();
    let default_root = format!("{home}/inir");
    let mut root = PathBuf::from(&default_root);
    let mut vfs: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--root" => {
                i += 1;
                if let Some(r) = args.get(i) {
                    root = PathBuf::from(r);
                }
            }
            "--vfs" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    vfs = Some(PathBuf::from(v));
                }
            }
            _ => {}
        }
        i += 1;
    }

    match prewarm::build_and_prewarm_vfs(&root, vfs.as_deref()) {
        Ok(rep) => {
            println!("{}", serde_json::to_string_pretty(&rep).unwrap_or_default());
            0
        }
        Err(e) => {
            eprintln!("inir-accel prewarm failed: {e}");
            1
        }
    }
}

fn cmd_bench() -> i32 {
    println!("=== inir-accel Rust Live Runtime Benchmark ===");

    let t0 = Instant::now();
    let req = washi::default_request();
    let _ = washi::solve_all(&req, "{}");
    let washi_us = t0.elapsed().as_micros();
    println!(
        "1. Washi full 3-scheme + 7-style palette solve  : {:>6} µs ({:.2} ms)",
        washi_us,
        washi_us as f64 / 1000.0
    );

    let t1 = Instant::now();
    let mut mcfg = material::MaterialGenConfig::default();
    mcfg.color = Some("#7AA2F7".to_string());
    let _ = material::run_material_pipeline(mcfg);
    let mat_us = t1.elapsed().as_micros();
    println!(
        "2. Material You + APCA terminal + app palette   : {:>6} µs ({:.2} ms)",
        mat_us,
        mat_us as f64 / 1000.0
    );

    let t2 = Instant::now();
    let s = sensors::detect_sensors("/sys");
    let _snap = sensors::sample_resources(&s);
    let sens_us = t2.elapsed().as_micros();
    println!(
        "3. Hardware sensor discovery + /proc telemetry  : {:>6} µs ({:.2} ms)",
        sens_us,
        sens_us as f64 / 1000.0
    );

    let t3 = Instant::now();
    let entries = apps::scan_desktop_entries();
    let _ = apps::fuzzy_search_apps(&entries, "term", false);
    let _ = apps::guess_icon("org.kde.dolphin", &entries);
    let apps_us = t3.elapsed().as_micros();
    println!(
        "4. Desktop scan ({} apps) + fuzzy + icon guess : {:>6} µs ({:.2} ms)",
        entries.len(),
        apps_us,
        apps_us as f64 / 1000.0
    );

    let sample_wall = Path::new("/home/user/inir/assets/wallpapers/default.jpg");
    if sample_wall.exists() {
        let t_reg1 = Instant::now();
        let _ = region::compute_luma_grid(sample_wall, 24, 1920, 1080, "fill");
        let reg_cold_us = t_reg1.elapsed().as_micros();
        let t_reg2 = Instant::now();
        let _ = region::compute_luma_grid(sample_wall, 24, 1920, 1080, "fill");
        let reg_warm_us = t_reg2.elapsed().as_micros();
        println!(
            "5. WallpaperLuma 24px grid (1920x1080, cold/warm): {:>6} µs / {:>4} µs ({:.2} ms / {:.2} ms)",
            reg_cold_us,
            reg_warm_us,
            reg_cold_us as f64 / 1000.0,
            reg_warm_us as f64 / 1000.0
        );
    }

    let inir_root = Path::new("/home/user/inir");
    if inir_root.exists() {
        let t4 = Instant::now();
        if let Ok(rep) = prewarm::build_and_prewarm_vfs(inir_root, None) {
            let vfs_us = t4.elapsed().as_micros();
            println!(
                "6. Parallel QML VFS scan ({} QML files)       : {:>6} µs ({:.2} ms)",
                rep.qml_files_scanned,
                vfs_us,
                vfs_us as f64 / 1000.0
            );
        }
    }

    0
}
