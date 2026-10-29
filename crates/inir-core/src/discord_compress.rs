//! A Discord-sized copy of a recording through two-pass x264, ported from `videos/compress-discord.py`.

use crate::material::py::expanduser;
use crate::pyrt;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;

const PRESETS: [&str; 9] = ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];

struct Args {
    input: Option<String>,
    output: Option<String>,
    target_mb: f64,
    safety_margin_mb: f64,
    audio_kbps: i64,
    preset: String,
    max_dimension: i64,
    retries: i64,
    nice: i64,
    force: bool,
    quiet: bool,
    json: bool,
}

struct Info {
    duration: f64,
    size_bytes: u64,
    width: i64,
    height: i64,
    has_audio: bool,
}

struct Plan {
    video_kbps: i64,
    audio_kbps: i64,
    width: i64,
    height: i64,
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

fn usage(message: impl std::fmt::Display) -> ! {
    eprintln!("usage: inir-accel compress-discord --input <video> [--output <mp4>] [--target-mb N] [--json] ...\n{message}");
    std::process::exit(2)
}

fn require_binary(name: &str) -> PathBuf {
    pyrt::which(name).unwrap_or_else(|| fail(format!("{name} is required but was not found in PATH")))
}

fn tail(text: &[u8], n: usize) -> String {
    let text = String::from_utf8_lossy(text);
    let lines = pyrt::splitlines(&text);
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn probe_media(ffprobe: &Path, input: &Path) -> Info {
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration:stream=codec_type,width,height", "-of", "json"])
        .arg(input)
        .output()
        .unwrap_or_else(|e| fail(e));
    if !out.status.success() {
        let detail = tail(&out.stderr, 8);
        fail(if detail.is_empty() { format!("{} failed", ffprobe.display()) } else { detail });
    }
    let payload: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| fail("ffprobe returned invalid JSON"));
    let number = |v: Option<&Value>| match v {
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        Some(v) => v.as_f64().unwrap_or(0.0),
        None => 0.0,
    };
    let duration = number(payload.pointer("/format/duration"));
    let (mut width, mut height, mut has_audio) = (0, 0, false);
    for stream in payload["streams"].as_array().into_iter().flatten() {
        match stream["codec_type"].as_str() {
            Some("video") if width <= 0 => {
                width = number(stream.get("width")) as i64;
                height = number(stream.get("height")) as i64;
            }
            Some("audio") => has_audio = true,
            _ => {}
        }
    }
    if duration <= 0.0 {
        fail("Could not detect video duration");
    }
    if width <= 0 || height <= 0 {
        fail("Could not detect video dimensions");
    }
    let size_bytes = fs::metadata(input).map(|m| m.len()).unwrap_or(0);
    Info { duration, size_bytes, width, height, has_audio }
}

fn even(value: f64) -> i64 {
    (value as i64 / 2 * 2).max(2)
}

fn scaled_size(info: &Info, video_kbps: i64, requested_max_dimension: i64) -> (i64, i64) {
    let mut max_dimension = requested_max_dimension.max(240);
    for (below, cap) in [(180, 540), (300, 720), (600, 960), (1100, 1280)] {
        if video_kbps < below {
            max_dimension = max_dimension.min(cap);
            break;
        }
    }
    let long_edge = info.width.max(info.height);
    if long_edge <= max_dimension {
        return (even(info.width as f64), even(info.height as f64));
    }
    let ratio = max_dimension as f64 / long_edge as f64;
    (even(info.width as f64 * ratio), even(info.height as f64 * ratio))
}

fn build_plan(info: &Info, budget_bytes: i64, audio_kbps: i64, max_dimension: i64) -> Plan {
    let total_bps = (budget_bytes as f64 * 8.0 / info.duration).max(1000.0);
    let mut audio_bps: f64 = 0.0;
    if info.has_audio && audio_kbps > 0 {
        audio_bps = ((audio_kbps * 1000) as f64).min(total_bps * 0.25);
        audio_bps = if total_bps >= 140_000.0 {
            audio_bps.max(32_000.0)
        } else if total_bps >= 70_000.0 {
            audio_bps.max(20_000.0)
        } else {
            (total_bps * 0.16).max(0.0)
        };
        audio_bps = audio_bps.min(total_bps * 0.45);
    }
    let video_bps = (total_bps - audio_bps).max(1000.0);
    let video_kbps = ((video_bps / 1000.0).floor() as i64).max(1);
    let (width, height) = scaled_size(info, video_kbps, max_dimension);
    Plan { video_kbps, audio_kbps: ((audio_bps / 1000.0).floor() as i64).max(0), width, height }
}

fn run_ffmpeg(command: &[String], quiet: bool) -> Result<(), String> {
    let mut cmd = Command::new(&command[0]);
    cmd.args(&command[1..]);
    let (status, stderr) = if quiet {
        let out = cmd.stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| e.to_string())?;
        (out.status, out.stderr)
    } else {
        (cmd.status().map_err(|e| e.to_string())?, Vec::new())
    };
    if status.success() {
        return Ok(());
    }
    let detail = tail(&stderr, 12);
    Err(if detail.is_empty() { format!("{} failed with exit code {}", command[0], status.code().unwrap_or(-1)) } else { detail })
}

fn video_args(plan: &Plan, preset: &str, info: &Info) -> Vec<String> {
    let kbps = format!("{}k", plan.video_kbps);
    let bufsize = format!("{}k", (plan.video_kbps * 2).max(2));
    let mut args: Vec<String> = ["-map", "0:v:0", "-sn", "-c:v", "libx264", "-preset", preset, "-b:v", &kbps, "-maxrate", &kbps]
        .into_iter()
        .chain(["-bufsize", &bufsize, "-pix_fmt", "yuv420p"])
        .map(String::from)
        .collect();
    if plan.width != even(info.width as f64) || plan.height != even(info.height as f64) {
        args.extend(["-vf".into(), format!("scale={}:{}:flags=lanczos", plan.width, plan.height)]);
    }
    args
}

struct Job<'a> {
    ffmpeg: &'a Path,
    input: &'a Path,
    output: &'a Path,
    temp_output: &'a Path,
    info: &'a Info,
    preset: &'a str,
    quiet: bool,
}

fn encode_attempt(job: &Job, plan: &Plan, passlog: &Path) -> Result<(), String> {
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let common: Vec<String> = vec![s(job.ffmpeg), "-hide_banner".into(), "-nostdin".into(), "-y".into(), "-i".into(), s(job.input)];
    let pass = |n: &str| [common.clone(), video_args(plan, job.preset, job.info), vec!["-pass".into(), n.into(), "-passlogfile".into(), s(passlog)]].concat();
    let mut first = pass("1");
    first.extend(["-an", "-f", "null", "/dev/null"].map(String::from));
    let mut second = pass("2");
    if job.info.has_audio && plan.audio_kbps > 0 {
        second.extend(["-map".into(), "0:a:0?".into(), "-c:a".into(), "aac".into(), "-b:a".into(), format!("{}k", plan.audio_kbps), "-ac".into(), "2".into()]);
    } else {
        second.push("-an".into());
    }
    second.extend(["-map_metadata".into(), "0".into(), "-movflags".into(), "+faststart".into(), s(job.temp_output)]);

    let _ = fs::remove_file(job.temp_output);
    run_ffmpeg(&first, job.quiet)?;
    run_ffmpeg(&second, job.quiet)?;
    if !fs::metadata(job.temp_output).is_ok_and(|m| m.len() > 0) {
        return Err("ffmpeg did not create an output file".into());
    }
    fs::rename(job.temp_output, job.output).map_err(|e| e.to_string())
}

/// Path.expanduser().resolve(): symlinks of the part that exists resolved, the rest kept.
fn resolve(path: &str) -> PathBuf {
    let path = PathBuf::from(expanduser(path));
    let path = if path.is_absolute() { path } else { std::env::current_dir().unwrap_or_default().join(path) };
    if let Ok(real) = fs::canonicalize(&path) {
        return real;
    }
    match (path.parent().and_then(|dir| fs::canonicalize(dir).ok()), path.file_name()) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => path,
    }
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn compress(args: &Args) -> Value {
    unsafe { libc::nice(args.nice.max(0) as libc::c_int) };
    let ffmpeg = require_binary("ffmpeg");
    let ffprobe = require_binary("ffprobe");
    let input = resolve(args.input.as_deref().unwrap_or_default());
    let output = match args.output.as_deref().filter(|o| !o.is_empty()) {
        Some(o) => resolve(o),
        None => input.with_file_name(format!("{}.discord.mp4", stem(&input))),
    };
    if !input.is_file() {
        fail(format!("Input file does not exist: {}", input.display()));
    }
    if input == output {
        fail("Output path must be different from input path");
    }

    let target_bytes = ((args.target_mb * 1_000_000.0) as i64).max(1);
    let budget_mb = (args.target_mb - args.safety_margin_mb.max(0.0)).max(0.1);
    let budget_bytes = target_bytes.min(((budget_mb * 1_000_000.0) as i64).max(1));
    if let Some(dir) = output.parent() {
        let _ = fs::create_dir_all(dir);
    }

    let info = probe_media(&ffprobe, &input);
    let path = |p: &Path| p.to_string_lossy().into_owned();
    if !args.force && info.size_bytes as i64 <= target_bytes {
        return json!({"status": "skipped", "reason": "already_under_target", "input": path(&input), "output": path(&input),
            "sizeBytes": info.size_bytes, "targetBytes": target_bytes});
    }

    let suffix = output.extension().map_or(".mp4".to_string(), |e| format!(".{}", e.to_string_lossy()));
    let temp_output = output.with_file_name(format!(".{}.tmp-{}{suffix}", stem(&output), std::process::id()));
    let temp_dir = std::env::temp_dir().join(format!("inir-discord-compress-{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);
    let job = Job { ffmpeg: &ffmpeg, input: &input, output: &output, temp_output: &temp_output, info: &info, preset: &args.preset, quiet: args.quiet };
    let mut last_error = String::from("compression failed");
    let mut result = None;
    for attempt in 0..=args.retries.max(0) {
        let attempt_budget = ((budget_bytes as f64 * 0.88f64.powi(attempt as i32)) as i64).max(1);
        let plan = build_plan(&info, attempt_budget, args.audio_kbps, args.max_dimension);
        match encode_attempt(&job, &plan, &temp_dir.join(format!("pass-{attempt}"))) {
            Ok(()) => {
                let size_bytes = fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
                if size_bytes as i64 <= target_bytes {
                    result = Some(json!({"status": "compressed", "input": path(&input), "output": path(&output), "sizeBytes": size_bytes,
                        "targetBytes": target_bytes, "videoKbps": plan.video_kbps, "audioKbps": plan.audio_kbps, "width": plan.width,
                        "height": plan.height, "attempt": attempt + 1}));
                    break;
                }
                last_error = format!("output exceeded target ({size_bytes} > {target_bytes})");
                let _ = fs::remove_file(&output);
            }
            Err(error) => {
                last_error = error;
                let _ = fs::remove_file(&temp_output);
            }
        }
    }
    let _ = fs::remove_dir_all(&temp_dir);
    result.unwrap_or_else(|| fail(last_error))
}

fn parse(args: &[String]) -> Args {
    let mut a = Args {
        input: None,
        output: None,
        target_mb: 10.0,
        safety_margin_mb: 0.5,
        audio_kbps: 96,
        preset: "slow".into(),
        max_dimension: 1280,
        retries: 3,
        nice: 5,
        force: false,
        quiet: false,
        json: false,
    };
    fn number<T: FromStr>(flag: &str, value: &str) -> T {
        value.trim().parse().unwrap_or_else(|_| usage(format!("argument {flag}: invalid value: '{value}'")))
    }
    let mut it = args.iter();
    while let Some(raw) = it.next() {
        let (flag, inline) = match raw.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (raw.as_str(), None),
        };
        let mut value = || inline.clone().or_else(|| it.next().cloned()).unwrap_or_else(|| usage(format!("argument {flag}: expected one argument")));
        match flag {
            "--input" => a.input = Some(value()),
            "--output" => a.output = Some(value()),
            "--target-mb" => a.target_mb = number(flag, &value()),
            "--safety-margin-mb" => a.safety_margin_mb = number(flag, &value()),
            "--audio-kbps" => a.audio_kbps = number(flag, &value()),
            "--max-dimension" => a.max_dimension = number(flag, &value()),
            "--retries" => a.retries = number(flag, &value()),
            "--nice" => a.nice = number(flag, &value()),
            "--preset" => {
                let preset = value();
                if !PRESETS.contains(&preset.as_str()) {
                    usage(format!("argument --preset: invalid choice: '{preset}'"));
                }
                a.preset = preset;
            }
            "--force" => a.force = true,
            "--quiet" => a.quiet = true,
            "--json" => a.json = true,
            other => usage(format!("unrecognized arguments: {other}")),
        }
    }
    if a.input.is_none() {
        usage("the following arguments are required: --input");
    }
    a
}

pub fn run_cli(args: &[String]) -> i32 {
    let args = parse(args);
    let result = compress(&args);
    if args.json {
        println!("{result}");
    } else {
        println!("{}", result["output"].as_str().unwrap_or_default());
    }
    0
}
