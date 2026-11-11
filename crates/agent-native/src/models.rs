//! Model catalogues: `GET {base}/models` with every parameter the provider
//! reports (context, output limit, vision, tools, reasoning), plus Ollama's
//! `/api/show` for local models. Nothing is guessed: unreported is `None`.

use std::time::{Duration, Instant};

use futures::StreamExt;
use harness_core::Secret;
use serde_json::Value;

use crate::llm::{ApiKind, normalize_base};
use crate::provider::ProviderError;

/// One model as a provider describes it. `id` is the provider's raw id.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelMeta {
    pub id: String,
    pub name: Option<String>,
    pub context: Option<u64>,
    pub max_output: Option<u64>,
    pub effort: Option<bool>,
    pub vision: Option<bool>,
    pub tools: Option<bool>,
}

/// Token counts outside this range are treated as unreported.
const MAX_TOKENS: u64 = 100_000_000;
const MAX_NAME: usize = 120;
const MAX_ID: usize = 256;
/// Anthropic pages through its catalogue; all pages together get this long.
const LIST_BUDGET: Duration = Duration::from_secs(30);
const MAX_PAGES: usize = 20;
/// Ollama: at most this many `/api/show` calls, a few at a time.
const MAX_PROBES: usize = 200;
const PROBE_CONCURRENCY: usize = 4;
const PROBE_BUDGET: Duration = Duration::from_secs(20);
/// Bodies are read up to these sizes; a longer one is an error. Endpoints
/// are admin-chosen but not trusted to be small.
const MAX_LIST_BODY: usize = 8 << 20;
const MAX_SHOW_BODY: usize = 256 << 10;
const MAX_ERROR_BODY: usize = 16 << 10;
/// Models kept from one listing.
pub const MAX_MODELS: usize = 5_000;

/// Whether `base_url` looks like a local Ollama (`…:11434/v1`).
pub fn is_ollama(base_url: &str) -> bool {
    base_url.trim().trim_end_matches('/').ends_with(":11434/v1")
}

/// Redirects only within the same origin (scheme, host and port): reqwest
/// strips `Authorization` on a cross-origin hop but not `x-api-key`, so a
/// key would otherwise follow a redirect to a host it was never saved for.
pub fn same_origin_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|a| {
        let same = a.previous().last().is_some_and(|prev| {
            prev.scheme() == a.url().scheme()
                && prev.host_str() == a.url().host_str()
                && prev.port_or_known_default() == a.url().port_or_known_default()
        });
        if !same {
            a.stop()
        } else if a.previous().len() >= 10 {
            a.error("too many redirects")
        } else {
            a.follow()
        }
    })
}

/// The resolver a server-side call connects through: `None` for any
/// address, or one that refuses some (the master passes its public-only
/// resolver unless the operator listed the host, AUDIT L3).
pub type Dns = Option<std::sync::Arc<dyn reqwest::dns::Resolve>>;

fn client(timeout: Duration, dns: &Dns) -> Result<reqwest::Client, ProviderError> {
    let mut b = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .redirect(same_origin_redirects());
    if let Some(r) = dns {
        b = b.dns_resolver(r.clone());
    }
    b.build().map_err(|e| ProviderError::Http(e.to_string()))
}

/// The body, or an error once it passes `cap` bytes.
async fn read_capped(resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, ProviderError> {
    let too_large = || ProviderError::Http("response too large".into());
    if resp.content_length().is_some_and(|n| n > cap as u64) {
        return Err(too_large());
    }
    let mut out = Vec::new();
    let mut body = resp.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| ProviderError::Http(e.without_url().to_string()))?;
        if out.len() + chunk.len() > cap {
            return Err(too_large());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn get_json(
    client: &reqwest::Client,
    url: &str,
    kind: ApiKind,
    key: Option<&Secret<String>>,
) -> Result<Value, ProviderError> {
    send_json(client.get(url), kind, key, MAX_LIST_BODY).await
}

/// Send with the kind's auth headers; a non-2xx status is an error with the
/// start of its body.
async fn send_json(
    mut rb: reqwest::RequestBuilder,
    kind: ApiKind,
    key: Option<&Secret<String>>,
    cap: usize,
) -> Result<Value, ProviderError> {
    match (kind, key) {
        (ApiKind::Anthropic, Some(k)) => {
            rb = rb
                .header("x-api-key", k.expose())
                .header("anthropic-version", "2023-06-01");
        }
        (ApiKind::Anthropic, None) => rb = rb.header("anthropic-version", "2023-06-01"),
        (ApiKind::Openai, Some(k)) => rb = rb.bearer_auth(k.expose()),
        (ApiKind::Openai, None) => {}
    }
    let resp = rb
        .send()
        .await
        .map_err(|e| ProviderError::Http(e.without_url().to_string()))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let body = read_capped(resp, MAX_ERROR_BODY).await.unwrap_or_default();
        return Err(ProviderError::Status {
            status,
            body: String::from_utf8_lossy(&body).chars().take(300).collect(),
            hint: Default::default(),
        });
    }
    let body = read_capped(resp, cap).await?;
    serde_json::from_slice(&body).map_err(|e| ProviderError::Stream(e.to_string()))
}

/// List models with their metadata, sorted by id. Doubles as a key check.
/// `ollama` also asks `/api/show` per model (set it for the Ollama preset;
/// a base URL on port 11434 counts too).
pub async fn list_models(
    kind: ApiKind,
    base_url: &str,
    api_key: Option<&Secret<String>>,
    ollama: bool,
    dns: &Dns,
) -> Result<Vec<ModelMeta>, ProviderError> {
    let client = client(Duration::from_secs(20), dns)?;
    let base = normalize_base(base_url);
    let started = Instant::now();
    let mut out: Vec<ModelMeta> = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let url = match (kind, &after) {
            (ApiKind::Anthropic, None) => format!("{base}models?limit=1000"),
            (ApiKind::Anthropic, Some(a)) => format!("{base}models?limit=1000&after_id={a}"),
            (ApiKind::Openai, _) => format!("{base}models"),
        };
        let v = get_json(&client, &url, kind, api_key).await?;
        let items = v["data"]
            .as_array()
            .or_else(|| v["models"].as_array())
            .cloned()
            .unwrap_or_default();
        out.extend(items.iter().filter_map(parse_model));
        if out.len() >= MAX_MODELS {
            out.truncate(MAX_MODELS);
            break;
        }
        // Only Anthropic pages; ids in `last_id` are opaque, so they are
        // checked like any id before going into a URL.
        after = (kind == ApiKind::Anthropic && v["has_more"].as_bool() == Some(true))
            .then(|| v["last_id"].as_str().map(str::to_string))
            .flatten()
            .filter(|id| {
                id.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
            });
        if after.is_none() || started.elapsed() > LIST_BUDGET {
            break;
        }
    }
    if kind == ApiKind::Anthropic {
        // Extended thinking with tool calls needs signed thinking blocks
        // replayed, which the history does not keep yet (PLAN.md R8): never
        // offer effort for Anthropic models, whatever the API says.
        for m in &mut out {
            m.effort = Some(false);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    if ollama || is_ollama(base_url) {
        probe_ollama(&base, &mut out, dns).await;
    }
    Ok(out)
}

fn clean_id(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s.len() <= MAX_ID && !s.chars().any(char::is_control)).then(|| s.to_string())
}

/// Display names: no control or bidi characters, length-capped.
fn clean_name(s: &str) -> Option<String> {
    let s: String = s
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'))
        .take(MAX_NAME)
        .collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn tokens(v: &Value) -> Option<u64> {
    let n = match v {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64))?,
        Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    (1..=MAX_TOKENS).contains(&n).then_some(n)
}

fn first_tokens(v: &Value, pointers: &[&str]) -> Option<u64> {
    pointers.iter().find_map(|p| v.pointer(p).and_then(tokens))
}

/// Whether a string array (if present) contains any of `wanted`.
fn lists(v: &Value, pointer: &str, wanted: &[&str]) -> Option<bool> {
    let a = v.pointer(pointer)?.as_array()?;
    Some(
        a.iter()
            .any(|x| x.as_str().is_some_and(|s| wanted.contains(&s))),
    )
}

/// A bool, or Anthropic's `{"supported": bool}`.
fn flag(v: &Value, pointer: &str) -> Option<bool> {
    let x = v.pointer(pointer)?;
    x.as_bool().or_else(|| x["supported"].as_bool())
}

/// Read whatever a `/models` entry reports. Field names differ by provider:
/// OpenRouter (`context_length`, `top_provider`, `architecture`,
/// `supported_parameters`), Anthropic (`display_name`, `max_input_tokens`,
/// `max_tokens`, `capabilities`), Mistral (`max_context_length`,
/// `capabilities`), Groq (`context_window`, `max_completion_tokens`),
/// Together (`context_length`), Gemini (`inputTokenLimit`, …), vLLM
/// (`max_model_len`).
pub fn parse_model(v: &Value) -> Option<ModelMeta> {
    let id = clean_id(v["id"].as_str().or_else(|| v["name"].as_str())?)?;
    let name = ["display_name", "displayName", "name"]
        .iter()
        .find_map(|k| v[*k].as_str().and_then(clean_name))
        .filter(|n| *n != id);
    let context = first_tokens(
        v,
        &[
            "/context_length",
            "/context_window",
            "/max_context_length",
            "/max_input_tokens",
            "/inputTokenLimit",
            "/max_model_len",
            "/top_provider/context_length",
        ],
    );
    let max_output = first_tokens(
        v,
        &[
            "/top_provider/max_completion_tokens",
            "/max_completion_tokens",
            "/max_output_tokens",
            "/outputTokenLimit",
            "/max_tokens",
        ],
    );
    let vision = lists(v, "/architecture/input_modalities", &["image"])
        .or_else(|| flag(v, "/capabilities/vision"))
        .or_else(|| flag(v, "/capabilities/image_input"));
    let tools = lists(v, "/supported_parameters", &["tools"])
        .or_else(|| flag(v, "/capabilities/function_calling"))
        .or_else(|| flag(v, "/capabilities/tools"));
    let effort = lists(
        v,
        "/supported_parameters",
        &["reasoning", "reasoning_effort"],
    )
    .or_else(|| flag(v, "/capabilities/reasoning"))
    .or_else(|| flag(v, "/capabilities/thinking"));
    Some(ModelMeta {
        id,
        name,
        context,
        max_output,
        effort,
        vision,
        tools,
    })
}

/// What `/api/show` adds: context length from `model_info` and the
/// `capabilities` list (vision, tools, thinking).
pub fn parse_ollama_show(v: &Value, m: &mut ModelMeta) {
    if let Some(info) = v["model_info"].as_object() {
        let ctx = info
            .iter()
            .find(|(k, _)| k.ends_with(".context_length"))
            .and_then(|(_, n)| tokens(n));
        m.context = ctx.or(m.context);
    }
    if v["capabilities"].is_array() {
        m.vision = lists(v, "/capabilities", &["vision"]);
        m.tools = lists(v, "/capabilities", &["tools"]);
        m.effort = lists(v, "/capabilities", &["thinking"]);
    }
}

async fn probe_ollama(base: &str, models: &mut [ModelMeta], dns: &Dns) {
    let host = base.trim_end_matches('/').trim_end_matches("/v1");
    let url = format!("{host}/api/show");
    let Ok(client) = client(Duration::from_secs(5), dns) else {
        return;
    };
    let ids: Vec<(usize, String)> = models
        .iter()
        .enumerate()
        .take(MAX_PROBES)
        .map(|(i, m)| (i, m.id.clone()))
        .collect();
    let mut shows = futures::stream::iter(ids)
        .map(|(i, id)| {
            let (client, url) = (client.clone(), url.clone());
            async move {
                let resp = client
                    .post(&url)
                    .json(&serde_json::json!({ "model": id }))
                    .send()
                    .await
                    .ok()?;
                if !resp.status().is_success() {
                    return None;
                }
                let body = read_capped(resp, MAX_SHOW_BODY).await.ok()?;
                Some((i, serde_json::from_slice::<Value>(&body).ok()?))
            }
        })
        .buffer_unordered(PROBE_CONCURRENCY);
    let deadline = tokio::time::Instant::now() + PROBE_BUDGET;
    while let Ok(Some(next)) = tokio::time::timeout_at(deadline, shows.next()).await {
        if let Some((i, v)) = next {
            parse_ollama_show(&v, &mut models[i]);
        }
    }
}

/// The model test's prompt, and how much of an answer it allows.
pub const TEST_PROMPT: &str = "Reply with OK.";
const TEST_MAX_TOKENS: u64 = 16;
const TEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_CHAT_BODY: usize = 1 << 20;
/// Characters of the test answer kept.
pub const MAX_REPLY: usize = 200;

/// Ask `model` (raw id) [`TEST_PROMPT`] once, without streaming, and return
/// the start of its answer (cleaned like a display name, possibly empty when
/// a reasoning model spends the few tokens thinking). Fails the way a turn
/// would: unknown model, bad key, rate limit, unreachable endpoint.
pub async fn test_chat(
    kind: ApiKind,
    base_url: &str,
    api_key: Option<&Secret<String>>,
    model: &str,
    dns: &Dns,
) -> Result<String, ProviderError> {
    let client = client(TEST_TIMEOUT, dns)?;
    let base = normalize_base(base_url);
    let messages = serde_json::json!([{ "role": "user", "content": TEST_PROMPT }]);
    let v = match kind {
        ApiKind::Anthropic => {
            let body = serde_json::json!({
                "model": model, "max_tokens": TEST_MAX_TOKENS, "messages": messages,
            });
            let rb = client.post(format!("{base}messages")).json(&body);
            send_json(rb, kind, api_key, MAX_CHAT_BODY).await?
        }
        ApiKind::Openai => {
            let url = format!("{base}chat/completions");
            let body = |limit: &str| {
                let mut b = serde_json::json!({
                    "model": model, "messages": messages, "stream": false,
                });
                b[limit] = TEST_MAX_TOKENS.into();
                b
            };
            let first = client.post(&url).json(&body("max_tokens"));
            match send_json(first, kind, api_key, MAX_CHAT_BODY).await {
                // Newer OpenAI reasoning models refuse `max_tokens`.
                Err(ProviderError::Status {
                    status: 400,
                    body: b,
                    ..
                }) if b.contains("max_completion_tokens") => {
                    let again = client.post(&url).json(&body("max_completion_tokens"));
                    send_json(again, kind, api_key, MAX_CHAT_BODY).await?
                }
                r => r?,
            }
        }
    };
    Ok(reply_text(kind, &v))
}

/// The answer's text: OpenAI `choices[0].message.content` (a string or
/// parts), Anthropic's `text` blocks.
fn reply_text(kind: ApiKind, v: &Value) -> String {
    let parts = |a: &Vec<Value>| {
        a.iter()
            .filter(|p| p["type"].as_str().is_none_or(|t| t == "text"))
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("")
    };
    let raw = match kind {
        ApiKind::Anthropic => v["content"].as_array().map(parts),
        ApiKind::Openai => {
            let c = v.pointer("/choices/0/message/content");
            c.and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| c.and_then(Value::as_array).map(parts))
        }
    };
    sanitize_reply(&raw.unwrap_or_default())
}

/// At most [`MAX_REPLY`] characters on one line, without control or bidi
/// characters.
pub fn sanitize_reply(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'))
        .take(MAX_REPLY)
        .collect();
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openrouter_fields() {
        let m = parse_model(&json!({
            "id": "openai/gpt-4o", "name": "OpenAI: GPT-4o", "context_length": 128000,
            "architecture": {"input_modalities": ["text", "image"]},
            "top_provider": {"max_completion_tokens": 16384},
            "supported_parameters": ["tools", "temperature"]
        }))
        .unwrap();
        assert_eq!(m.name.as_deref(), Some("OpenAI: GPT-4o"));
        assert_eq!((m.context, m.max_output), (Some(128000), Some(16384)));
        assert_eq!(
            (m.vision, m.tools, m.effort),
            (Some(true), Some(true), Some(false))
        );
    }

    #[test]
    fn mistral_groq_and_bare() {
        let m = parse_model(&json!({
            "id": "pixtral", "max_context_length": 131072,
            "capabilities": {"vision": true, "function_calling": false}
        }))
        .unwrap();
        assert_eq!(
            (m.context, m.vision, m.tools),
            (Some(131072), Some(true), Some(false))
        );
        let g = parse_model(
            &json!({"id": "llama", "context_window": 8192, "max_completion_tokens": 1024}),
        )
        .unwrap();
        assert_eq!((g.context, g.max_output), (Some(8192), Some(1024)));
        let b = parse_model(&json!({"id": "x", "object": "model"})).unwrap();
        assert_eq!(
            b,
            ModelMeta {
                id: "x".into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn junk_is_dropped_or_clamped() {
        assert!(parse_model(&json!({"id": "bad\u{7}id"})).is_none());
        assert!(parse_model(&json!({"id": ""})).is_none());
        let m = parse_model(&json!({
            "id": "m", "display_name": "Nice\u{202E} name\n", "context_length": 0,
            "max_tokens": 999_999_999_999u64
        }))
        .unwrap();
        assert_eq!(m.name.as_deref(), Some("Nice name"));
        assert_eq!((m.context, m.max_output), (None, None));
        let long = parse_model(&json!({"id": "m", "name": "n".repeat(500)})).unwrap();
        assert_eq!(long.name.unwrap().len(), MAX_NAME);
    }

    #[test]
    fn test_replies() {
        let openai = json!({"choices": [{"message": {"content": "  OK\n\tthere\u{7}"}}]});
        assert_eq!(reply_text(ApiKind::Openai, &openai), "OK  there");
        let parts = json!({"choices": [{"message": {"content": [{"type": "text", "text": "O"}, {"text": "K"}]}}]});
        assert_eq!(reply_text(ApiKind::Openai, &parts), "OK");
        let anthropic = json!({"content": [
            {"type": "thinking", "thinking": "hm"}, {"type": "text", "text": "OK."}
        ]});
        assert_eq!(reply_text(ApiKind::Anthropic, &anthropic), "OK.");
        assert_eq!(reply_text(ApiKind::Openai, &json!({"choices": []})), "");
        assert_eq!(sanitize_reply(&"x".repeat(500)).len(), MAX_REPLY);
    }

    #[test]
    fn ollama_show() {
        let mut m = ModelMeta {
            id: "qwen3".into(),
            ..Default::default()
        };
        parse_ollama_show(
            &json!({
                "model_info": {"general.architecture": "qwen3", "qwen3.context_length": 40960},
                "capabilities": ["completion", "tools", "thinking"]
            }),
            &mut m,
        );
        assert_eq!(m.context, Some(40960));
        assert_eq!(
            (m.tools, m.effort, m.vision),
            (Some(true), Some(true), Some(false))
        );
        assert!(is_ollama("http://localhost:11434/v1/"));
        assert!(!is_ollama("https://api.openai.com/v1"));
    }
}
