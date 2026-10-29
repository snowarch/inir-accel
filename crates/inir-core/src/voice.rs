//! Speech to text for VoiceSearch, ported from `voiceSearch/transcribe-audio.py`: the transcription on stdout,
//! diagnostics on stderr. The key comes in INIR_VOICE_API_KEY and never reaches argv.

use crate::material::py::expanduser;
use crate::net::{self, Body};
use crate::pyrt;
use serde_json::{Value, json};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const USER_AGENT: &str = "iNiR/Voice";
const GEMINI: &str = "https://generativelanguage.googleapis.com";

// VoiceSearch records wav; python's mimetypes names it audio/x-wav.
fn mime(path: &Path) -> &'static str {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) { "audio/x-wav" } else { "audio/wav" }
}

fn json_body(response: net::Response) -> Result<Value, String> {
    if response.status >= 400 {
        return Err(format!("HTTP Error {}", response.status));
    }
    serde_json::from_slice(&response.body).map_err(|e| e.to_string())
}

fn openai_compatible(file: &Path, endpoint: &str, model: &str, key: &str, language: &str) -> Result<String, String> {
    let mut fields = vec![("model", model), ("response_format", "json")];
    if !language.is_empty() && language != "auto" {
        fields.push(("language", language));
    }
    let auth = format!("Bearer {key}");
    let headers = [("Authorization", auth.as_str()), ("Accept", "application/json"), ("User-Agent", USER_AGENT)];
    let payload = json_body(net::request(endpoint, &headers, Body::Form(&fields, ("file", file, mime(file))), 60.0)?)?;
    Ok(payload.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_string())
}

fn gemini(file: &Path, model: &str, key: &str) -> Result<String, String> {
    let mime = mime(file);
    let size = fs::metadata(file).map_err(|e| e.to_string())?.len().to_string();
    let start = net::request(
        &format!("{GEMINI}/upload/v1beta/files"),
        &[
            ("x-goog-api-key", key),
            ("X-Goog-Upload-Protocol", "resumable"),
            ("X-Goog-Upload-Command", "start"),
            ("X-Goog-Upload-Header-Content-Length", &size),
            ("X-Goog-Upload-Header-Content-Type", mime),
            ("Content-Type", "application/json"),
        ],
        Body::Text(r#"{"file": {"display_name": "iNiR voice"}}"#),
        30.0,
    )?;
    if start.status >= 400 {
        return Err(format!("HTTP Error {}", start.status));
    }
    let upload_url = start.header("x-goog-upload-url").unwrap_or_default().to_string();
    if upload_url.is_empty() {
        return Err("Gemini did not return an upload URL".into());
    }
    let upload_headers = [("x-goog-api-key", key), ("X-Goog-Upload-Offset", "0"), ("X-Goog-Upload-Command", "upload, finalize"), ("Content-Type", mime)];
    let uploaded = json_body(net::request(&upload_url, &upload_headers, Body::File(file), 60.0)?)?;
    let file_uri = uploaded.pointer("/file/uri").and_then(Value::as_str).unwrap_or_default();
    if file_uri.is_empty() {
        return Err("Gemini upload did not return a file URI".into());
    }
    let url = format!("{GEMINI}/v1beta/models/{}:generateContent?key={}", net::quote(model, "-._"), net::quote(key, "/"));
    let prompt = "Transcribe this audio exactly. Output only the transcription. If it is empty or unclear, output nothing.";
    let payload = json!({"contents": [{"parts": [{"file_data": {"mime_type": mime, "file_uri": file_uri}}, {"text": prompt}]}],
        "generationConfig": {"temperature": 0}});
    let headers = [("Content-Type", "application/json"), ("User-Agent", USER_AGENT)];
    let result = json_body(net::request(&url, &headers, Body::Text(&payload.to_string()), 60.0)?)?;
    let parts = result.pointer("/candidates/0/content/parts").and_then(Value::as_array);
    Ok(parts.into_iter().flatten().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<String>().trim().to_string())
}

fn local_models(explicit: &str) -> Vec<PathBuf> {
    let env = |k: &str, fallback: String| std::env::var(k).ok().filter(|v| !v.is_empty()).unwrap_or(fallback);
    let home = std::env::var("HOME").unwrap_or_default();
    let data = env("XDG_DATA_HOME", format!("{home}/.local/share"));
    let cache = env("XDG_CACHE_HOME", format!("{home}/.cache"));
    let mut candidates: Vec<PathBuf> = Vec::new();
    if !explicit.is_empty() {
        candidates.push(expanduser(explicit).into());
    }
    for p in [format!("{data}/inir/whisper/ggml-base.bin"), format!("{data}/whisper/ggml-base.bin"), format!("{cache}/whisper.cpp/ggml-base.bin"), format!("{cache}/whisper/ggml-base.bin")] {
        candidates.push(p.into());
    }
    candidates
}

fn probe(explicit_model: &str) -> (Option<PathBuf>, Option<PathBuf>) {
    let executable = pyrt::which("whisper-cli").or_else(|| pyrt::which("whisper-cpp"));
    (executable, local_models(explicit_model).into_iter().find(|p| p.is_file()))
}

fn local_whisper(file: &Path, model_path: &str, language: &str) -> Result<String, String> {
    let (Some(executable), Some(model)) = probe(model_path) else {
        return Err("whisper-cli or a local ggml model is missing".into());
    };
    let dir = std::env::temp_dir().join(format!("inir-voice-{}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = run_whisper(&executable, &model, file, &dir.join("transcript"), language);
    let _ = fs::remove_dir_all(&dir);
    result
}

fn run_whisper(executable: &Path, model: &Path, file: &Path, output_base: &Path, language: &str) -> Result<String, String> {
    let mut cmd = Command::new(executable);
    cmd.arg("-m").arg(model).arg("-f").arg(file).arg("-otxt").arg("-of").arg(output_base).arg("-nt");
    if !language.is_empty() && language != "auto" {
        cmd.args(["-l", language]);
    }
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let drain = |mut pipe: Box<dyn Read + Send>| std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });
    let stdout = drain(Box::new(child.stdout.take().unwrap()));
    let stderr = drain(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + Duration::from_secs(180);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("whisper-cli timed out after 180 seconds".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let (stdout, stderr) = (stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    if !status.success() {
        return Err(if stderr.trim().is_empty() { "whisper-cli failed".into() } else { stderr.trim().to_string() });
    }
    let transcript = output_base.with_extension("txt");
    if transcript.is_file() {
        return fs::read_to_string(&transcript).map(|t| t.trim().to_string()).map_err(|e| e.to_string());
    }
    Ok(stdout.trim().to_string())
}

/// `--provider probe|local|groq|openai|gemini [--audio f] [--endpoint url] [--model m] [--language l] [--local-model f]`
pub fn run_cli(args: &[String]) -> i32 {
    let mut values = std::collections::HashMap::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let name = arg.strip_prefix("--").unwrap_or_default();
        if !["provider", "audio", "endpoint", "model", "language", "local-model"].contains(&name) {
            eprintln!("transcribe-audio: unrecognized argument {arg}");
            return 2;
        }
        let Some(value) = it.next() else {
            eprintln!("transcribe-audio: --{name} expects a value");
            return 2;
        };
        values.insert(name, value.as_str());
    }
    let value = |k: &str, default: &'static str| values.get(k).copied().unwrap_or(default);
    let provider = value("provider", "");
    if !["probe", "local", "groq", "openai", "gemini"].contains(&provider) {
        eprintln!("transcribe-audio: --provider must be probe, local, groq, openai or gemini");
        return 2;
    }
    if provider == "probe" {
        let (executable, model) = probe(value("local-model", ""));
        let path = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        println!("{}", json!({"available": executable.is_some() && model.is_some(), "executable": path(&executable), "model": path(&model)}));
        return 0;
    }
    let audio = PathBuf::from(expanduser(value("audio", "")));
    if !fs::metadata(&audio).is_ok_and(|m| m.is_file() && m.len() > 0) {
        eprintln!("Audio file is missing or empty");
        return 2;
    }
    let key = std::env::var("INIR_VOICE_API_KEY").unwrap_or_default();
    let result = match provider {
        "local" => local_whisper(&audio, value("local-model", ""), value("language", "auto")),
        _ if key.is_empty() => Err(if provider == "gemini" { "Gemini key is missing" } else { "Provider key is missing" }.into()),
        "gemini" => gemini(&audio, Some(value("model", "")).filter(|m| !m.is_empty()).unwrap_or("gemini-2.5-flash"), &key),
        _ => openai_compatible(&audio, value("endpoint", ""), value("model", ""), &key, value("language", "auto")),
    };
    let _ = fs::remove_file(&audio);
    match result {
        Ok(transcript) => {
            println!("{transcript}");
            0
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}
