//! Each AI provider's model list, normalized for AiProviderCatalog; ported from `ai/discover-provider-models.py`.
//! The key comes in INIR_AI_PROVIDER_KEY and reaches curl on stdin, never argv.

use crate::net;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

const UNKNOWN: &str = "unknown";
const SUPPORTED: &str = "supported";
const UNSUPPORTED: &str = "unsupported";

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        other => other.to_string(),
    }
}

/// `str(row.get(key) or fallback)`
fn text(row: &Value, key: &str, fallback: &str) -> String {
    row.get(key).filter(|v| truthy(v)).map_or_else(|| fallback.to_string(), py_str)
}

/// `int(row.get(key) or 0)`
fn int(row: &Value, key: &str) -> i64 {
    match row.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        Some(Value::Bool(b)) => *b as i64,
        _ => 0,
    }
}

fn caps(overrides: &[(&str, &str)]) -> Value {
    let mut c = json!({"chat": SUPPORTED, "vision": UNKNOWN, "audioInput": UNKNOWN, "toolCalling": UNKNOWN,
        "webSearch": UNKNOWN, "structuredOutput": UNKNOWN, "reasoning": UNKNOWN});
    for (k, v) in overrides {
        c[*k] = json!(v);
    }
    c
}

fn pick(yes: bool, otherwise: &'static str) -> &'static str {
    if yes { SUPPORTED } else { otherwise }
}

fn any_in(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

fn modalities(raw: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in raw.as_array().into_iter().flatten() {
        let value = py_str(item).trim().to_lowercase();
        let value = match value.as_str() {
            "image_url" | "images" => "image".into(),
            "audio_url" => "audio".into(),
            _ => value,
        };
        if !value.is_empty() && !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

fn rows<'a>(payload: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    payload.get(key).and_then(Value::as_array).into_iter().flatten()
}

struct Ctx {
    provider_id: String,
    chat_endpoint: String,
    api_format: String,
    requires_key: bool,
    key_id: String,
    local: bool,
}

impl Ctx {
    fn base(&self, remote_id: &str, display_name: &str, description: &str) -> Value {
        json!({"providerId": self.provider_id, "remoteId": remote_id,
            "displayName": if display_name.is_empty() { remote_id } else { display_name }, "description": description,
            "endpoint": self.chat_endpoint, "apiFormat": self.api_format, "authScheme": "strategy",
            "requiresKey": self.requires_key, "keyId": self.key_id, "local": self.local, "free": self.local,
            "inputModalities": ["text"], "outputModalities": ["text"], "capabilities": caps(&[]), "contextTokens": 0,
            "maxOutputTokens": 0, "pricing": {}, "expiresAt": "", "status": "available", "source": "live"})
    }
}

fn openrouter(payload: &Value, c: &Ctx) -> Vec<Value> {
    let mut models = Vec::new();
    for row in rows(payload, "data").filter(|r| r.is_object()) {
        let remote_id = row.get("id").map(py_str).unwrap_or_default().trim().to_string();
        if remote_id.is_empty() {
            continue;
        }
        let mut item = c.base(&remote_id, &text(row, "name", &remote_id), &text(row, "description", ""));
        let arch = row.get("architecture").cloned().unwrap_or(Value::Null);
        let input = match arch.get("input_modalities").filter(|v| truthy(v)) {
            Some(list) => modalities(list),
            None => {
                let modality = arch.get("modality").and_then(Value::as_str).unwrap_or_default();
                modalities(&json!([modality.split("->").next().unwrap_or_default()]))
            }
        };
        let output = modalities(arch.get("output_modalities").unwrap_or(&Value::Null));
        if !input.is_empty() {
            item["inputModalities"] = json!(input);
        }
        if !output.is_empty() {
            item["outputModalities"] = json!(output);
        }
        let params: HashSet<String> = row.get("supported_parameters").and_then(Value::as_array).into_iter().flatten().map(|v| py_str(v).to_lowercase()).collect();
        let has = |names: &[&str]| names.iter().any(|n| params.contains(*n));
        let takes = |kind: &str| item["inputModalities"].as_array().is_some_and(|m| m.iter().any(|v| v == kind));
        let capabilities = caps(&[
            ("vision", pick(takes("image"), UNSUPPORTED)),
            ("audioInput", pick(takes("audio"), UNSUPPORTED)),
            ("toolCalling", pick(has(&["tools", "tool_choice"]), UNKNOWN)),
            ("webSearch", pick(has(&["web_search", "web_search_options"]), UNKNOWN)),
            ("structuredOutput", pick(has(&["response_format", "structured_outputs"]), UNKNOWN)),
            ("reasoning", pick(has(&["reasoning", "include_reasoning"]), UNKNOWN)),
        ]);
        item["capabilities"] = capabilities;
        item["contextTokens"] = json!(int(row, "context_length"));
        item["maxOutputTokens"] = json!(int(row.get("top_provider").unwrap_or(&Value::Null), "max_completion_tokens"));
        let pricing = row.get("pricing").filter(|v| truthy(v)).cloned().unwrap_or(json!({}));
        let zero = |k: &str| pricing.get(k).map(py_str).unwrap_or_default().trim() == "0";
        item["free"] = json!(zero("prompt") && zero("completion"));
        item["pricing"] = pricing;
        item["expiresAt"] = json!(text(row, "expiration_date", ""));
        models.push(item);
    }
    models
}

fn ollama(payload: &Value, c: &Ctx) -> Vec<Value> {
    let mut models = Vec::new();
    for row in rows(payload, "models").filter(|r| r.is_object()) {
        let remote_id = text(row, "model", &text(row, "name", "")).trim().to_string();
        if remote_id.is_empty() {
            continue;
        }
        let details = row.get("details").unwrap_or(&Value::Null);
        let (size, quant) = (text(details, "parameter_size", ""), text(details, "quantization_level", ""));
        let description = [size.as_str(), quant.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        let mut item = c.base(&remote_id, &remote_id, &description);
        let lowered = remote_id.to_lowercase();
        item["capabilities"] = caps(&[
            ("vision", pick(any_in(&lowered, &["vision", "-vl", ":vl"]), UNKNOWN)),
            ("reasoning", pick(any_in(&lowered, &["reason", "thinking", "deepseek-r1"]), UNKNOWN)),
        ]);
        item["free"] = json!(true);
        item["sizeBytes"] = json!(int(row, "size"));
        item["parameterSize"] = json!(size);
        item["quantization"] = json!(quant);
        models.push(item);
    }
    models
}

fn gemini(payload: &Value, c: &Ctx) -> Vec<Value> {
    let mut models = Vec::new();
    for row in rows(payload, "models").filter(|r| r.is_object()) {
        let full_name = text(row, "name", "");
        let remote_id = full_name.strip_prefix("models/").unwrap_or(&full_name).trim().to_string();
        let generates = row.get("supportedGenerationMethods").and_then(Value::as_array).into_iter().flatten()
            .any(|m| matches!(m.as_str(), Some("generateContent" | "streamGenerateContent")));
        if remote_id.is_empty() || !generates {
            continue;
        }
        let mut item = c.base(&remote_id, &text(row, "displayName", &remote_id), &text(row, "description", ""));
        item["endpoint"] = json!(c.chat_endpoint.replace("{model}", &remote_id));
        item["contextTokens"] = json!(int(row, "inputTokenLimit"));
        item["maxOutputTokens"] = json!(int(row, "outputTokenLimit"));
        item["capabilities"] = caps(&[
            ("toolCalling", SUPPORTED),
            ("webSearch", SUPPORTED),
            ("structuredOutput", SUPPORTED),
            ("reasoning", pick(any_in(&remote_id.to_lowercase(), &["2.5", "3", "thinking"]), UNKNOWN)),
        ]);
        models.push(item);
    }
    models
}

fn friendly_model_name(remote_id: &str) -> String {
    const ACRONYMS: [&str; 7] = ["gpt", "glm", "kimi", "qwen", "mimo", "hy", "ai"];
    let capitalize = |w: &str| {
        let mut chars = w.chars();
        chars.next().map_or(String::new(), |f| f.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect())
    };
    remote_id
        .replace(['/', '-'], " ")
        .split_whitespace()
        .map(|w| if ACRONYMS.contains(&w.to_lowercase().as_str()) { w.to_uppercase() } else { capitalize(w) })
        .collect::<Vec<_>>()
        .join(" ")
}

fn opencode(payload: &Value, c: &Ctx) -> Vec<Value> {
    let is_go = c.provider_id == "opencode-go";
    let base = if is_go { "https://opencode.ai/zen/go/v1" } else { "https://opencode.ai/zen/v1" };
    let mut models = Vec::new();
    for row in rows(payload, "data") {
        let remote_id = match row {
            Value::Object(_) => row.get("id").filter(|v| !v.is_null()).map(py_str).unwrap_or_default(),
            other if truthy(other) => py_str(other),
            _ => String::new(),
        };
        let remote_id = remote_id.trim();
        if remote_id.is_empty() {
            continue;
        }
        let lowered = remote_id.to_lowercase();
        let starts = |prefixes: &[&str]| prefixes.iter().any(|p| lowered.starts_with(p));
        // OpenCode Go speaks Messages for MiniMax and Qwen, OpenAI chat for the rest; Zen routes by model family.
        let (endpoint, format) = if is_go {
            if starts(&["minimax-", "qwen"]) { (format!("{base}/messages"), "anthropic") } else { (format!("{base}/chat/completions"), "openai") }
        } else if starts(&["gpt-"]) {
            (format!("{base}/responses"), "openai-response")
        } else if starts(&["claude-", "qwen"]) {
            (format!("{base}/messages"), "anthropic")
        } else if starts(&["gemini-"]) {
            (format!("{base}/models/{remote_id}:streamGenerateContent"), "gemini")
        } else {
            (format!("{base}/chat/completions"), "openai")
        };
        let mut item = c.base(remote_id, &friendly_model_name(remote_id), "");
        item["endpoint"] = json!(endpoint);
        item["apiFormat"] = json!(format);
        item["authScheme"] = json!(match format {
            "anthropic" => "anthropic",
            "gemini" => "gemini-header",
            _ => "bearer",
        });
        item["free"] = json!(!is_go && lowered.ends_with("-free"));
        item["capabilities"] = caps(&[
            ("toolCalling", SUPPORTED),
            ("reasoning", pick(any_in(&lowered, &["gpt-5", "claude-", "deepseek", "glm-5", "kimi-"]), UNKNOWN)),
            ("structuredOutput", pick(matches!(format, "openai" | "openai-response"), UNKNOWN)),
        ]);
        models.push(item);
    }
    models
}

fn generic(payload: &Value, c: &Ctx) -> Vec<Value> {
    let rows: Vec<&Value> = match payload {
        Value::Object(_) => ["data", "models"].iter().find_map(|k| payload.get(*k).filter(|v| truthy(v))).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default(),
        Value::Array(a) => a.iter().collect(),
        _ => Vec::new(),
    };
    let mut models = Vec::new();
    for row in rows {
        let (remote_id, display_name, description) = match row {
            Value::String(s) => (s.clone(), s.clone(), String::new()),
            Value::Object(_) => {
                let id = text(row, "id", &text(row, "name", &text(row, "model", "")));
                let display = text(row, "display_name", &text(row, "name", &id));
                (id, display, text(row, "description", ""))
            }
            _ => continue,
        };
        let remote_id = remote_id.trim();
        if remote_id.is_empty() {
            continue;
        }
        let mut item = c.base(remote_id, &display_name, &description);
        if row.is_object() {
            item["contextTokens"] = json!(match int(row, "context_length") {
                0 => int(row, "max_context_length"),
                n => n,
            });
            if let Some(cap) = row.get("capabilities").filter(|v| v.is_object()) {
                let on = |k: &str| cap.get(k).is_some_and(truthy);
                item["capabilities"] = caps(&[
                    ("vision", pick(on("vision"), UNKNOWN)),
                    ("toolCalling", pick(on("function_calling"), UNKNOWN)),
                    ("structuredOutput", pick(on("json_schema"), UNKNOWN)),
                ]);
            }
        }
        models.push(item);
    }
    models
}

fn headers(kind: &str, key: &str) -> Vec<(&'static str, String)> {
    let mut headers = vec![("Accept", "application/json".to_string()), ("User-Agent", "iNiR/AI-Catalog".to_string())];
    if key.is_empty() {
        return headers;
    }
    match kind {
        "anthropic" => headers.extend([("x-api-key", key.to_string()), ("anthropic-version", "2023-06-01".to_string())]),
        "gemini" => headers.push(("x-goog-api-key", key.to_string())),
        _ => headers.push(("Authorization", format!("Bearer {key}"))),
    }
    headers
}

/// `--provider-id <id> --kind <kind> --models-url <url> --chat-endpoint <url> [--api-format f] [--key-id k]
/// [--requires-key] [--public-catalog] [--auth-scheme s] [--local] [--timeout secs]`
pub fn run_cli(args: &[String]) -> i32 {
    let mut values: HashMap<&str, String> = HashMap::new();
    let mut flags: HashSet<&str> = HashSet::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.strip_prefix("--").unwrap_or_default() {
            name @ ("requires-key" | "public-catalog" | "local") => {
                flags.insert(name);
            }
            name @ ("provider-id" | "kind" | "models-url" | "chat-endpoint" | "api-format" | "key-id" | "auth-scheme" | "timeout") => {
                let Some(value) = it.next() else {
                    eprintln!("discover-provider-models: --{name} expects a value");
                    return 2;
                };
                values.insert(name, value.clone());
            }
            _ => {
                eprintln!("discover-provider-models: unrecognized argument {arg}");
                return 2;
            }
        }
    }
    for required in ["provider-id", "kind", "models-url", "chat-endpoint"] {
        if !values.contains_key(required) {
            eprintln!("discover-provider-models: --{required} is required");
            return 2;
        }
    }
    let value = |k: &str, default: &str| values.get(k).cloned().unwrap_or_else(|| default.to_string());
    let ctx = Ctx {
        provider_id: value("provider-id", ""),
        chat_endpoint: value("chat-endpoint", ""),
        api_format: value("api-format", "openai"),
        requires_key: flags.contains("requires-key"),
        key_id: value("key-id", ""),
        local: flags.contains("local"),
    };
    let kind = value("kind", "");
    let key = std::env::var("INIR_AI_PROVIDER_KEY").unwrap_or_default();
    let fetched_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut result = json!({"providerId": ctx.provider_id, "ok": false, "models": [], "error": "", "fetchedAt": fetched_at});

    // Public catalogs are browseable before a key exists; generation stays locked until then.
    if ctx.requires_key && key.is_empty() && !flags.contains("public-catalog") {
        result["error"] = json!("missing-key");
        println!("{result}");
        return 0;
    }
    let mut url = value("models-url", "");
    if kind == "gemini" && !key.is_empty() && !url.contains("key=") {
        url = format!("{url}{}key={}", if url.contains('?') { "&" } else { "?" }, net::quote(&key));
    }
    let owned = headers(&kind, &key);
    let headers: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let timeout = values.get("timeout").and_then(|t| t.parse().ok()).unwrap_or(8.0);
    match net::get(&url, &headers, timeout) {
        Err(e) => result["error"] = json!(format!("network-{e}")),
        Ok(r) if r.status >= 400 => result["error"] = json!(format!("http-{}", r.status)),
        Ok(r) => match serde_json::from_slice::<Value>(&r.body) {
            Err(e) => result["error"] = json!(format!("invalid-response-{e}")),
            Ok(payload) => {
                let mut models = match kind.as_str() {
                    "openrouter" => openrouter(&payload, &ctx),
                    "ollama" => ollama(&payload, &ctx),
                    "gemini" => gemini(&payload, &ctx),
                    "opencode-zen" | "opencode-go" => opencode(&payload, &ctx),
                    _ => generic(&payload, &ctx),
                };
                let auth_scheme = value("auth-scheme", "strategy");
                for model in &mut models {
                    if matches!(model["authScheme"].as_str(), None | Some("" | "strategy")) {
                        model["authScheme"] = json!(auth_scheme);
                    }
                }
                result["ok"] = json!(true);
                result["models"] = json!(models);
            }
        },
    }
    println!("{result}");
    0
}
