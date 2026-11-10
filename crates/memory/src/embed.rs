//! Embeddings from any OpenAI-shaped `/embeddings` endpoint (Ollama,
//! llama.cpp server, a LAN box or a cloud API): the model runs elsewhere.

use std::time::Duration;

use harness_core::Secret;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{MemoryError, Result};

/// Quality presets: which embedding model to ask for. Sizes are the models'
/// published parameter counts, not measured here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// all-minilm (23M parameters, ~50 MB): fastest, fine for short facts.
    Fast,
    /// nomic-embed-text (137M, ~280 MB): the default.
    #[default]
    Balanced,
    /// mxbai-embed-large (335M, ~670 MB): best recall, slowest.
    Best,
}

impl Preset {
    /// Model name as Ollama publishes it.
    pub fn ollama_model(self) -> &'static str {
        match self {
            Preset::Fast => "all-minilm",
            Preset::Balanced => "nomic-embed-text",
            Preset::Best => "mxbai-embed-large",
        }
    }
}

/// How the key is sent: `Authorization: Bearer` (OpenAI style) or
/// `x-api-key` plus `anthropic-version` (Anthropic style). The request and
/// response bodies are the OpenAI `/embeddings` shape either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApiStyle {
    #[default]
    Openai,
    Anthropic,
}

/// Longest piece of an upstream error body shown to the admin.
const MAX_DETAIL: usize = 200;

/// Redirects only within the same origin: reqwest strips `Authorization`
/// on a cross-origin hop but not `x-api-key`, so the key would follow.
pub(crate) fn same_origin_redirects() -> reqwest::redirect::Policy {
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

pub struct Embedder {
    client: reqwest::Client,
    base_url: String,
    pub model: String,
    key: Option<Secret<String>>,
    style: ApiStyle,
}

impl Embedder {
    pub fn new(base_url: &str, model: &str, key: Option<Secret<String>>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(same_origin_redirects())
                .build()
                .expect("static client config"),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            key,
            style: ApiStyle::Openai,
        }
    }

    pub fn with_style(mut self, style: ApiStyle) -> Self {
        self.style = style;
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// One vector per input, in order.
    pub async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }
        let mut rb = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .json(&json!({ "model": self.model, "input": inputs }));
        if let Some(k) = &self.key {
            rb = match self.style {
                ApiStyle::Openai => rb.bearer_auth(k.expose()),
                ApiStyle::Anthropic => rb
                    .header("x-api-key", k.expose())
                    .header("anthropic-version", "2023-06-01"),
            };
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| MemoryError::Embed(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let mut detail = error_detail(&body);
            if let Some(k) = self.key.as_ref().filter(|k| !k.expose().is_empty()) {
                detail = detail.replace(k.expose(), "[redacted]");
            }
            return Err(MemoryError::EmbedHttp {
                status: status.as_u16(),
                detail,
            });
        }
        #[derive(Deserialize)]
        struct Item {
            embedding: Vec<f32>,
            #[serde(default)]
            index: Option<usize>,
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| MemoryError::Embed(e.to_string()))?;
        let mut items: Vec<Item> = serde_json::from_value(v["data"].clone())
            .map_err(|e| MemoryError::Embed(format!("unexpected response: {e}")))?;
        if items.len() != inputs.len() {
            return Err(MemoryError::Embed("wrong number of vectors".into()));
        }
        if items.iter().all(|i| i.index.is_some()) {
            items.sort_by_key(|i| i.index);
        }
        Ok(items.into_iter().map(|i| i.embedding).collect())
    }
}

/// A short, printable piece of an error body: the JSON error message when
/// there is one, else plain text; HTML pages are dropped.
pub(crate) fn error_detail(body: &str) -> String {
    let body = body.trim();
    let text = match serde_json::from_str::<Value>(body) {
        Ok(v) => [
            &v["error"]["message"],
            &v["error"],
            &v["message"],
            &v["detail"],
        ]
        .into_iter()
        .find_map(Value::as_str)
        .unwrap_or_default()
        .to_string(),
        Err(_) if body.starts_with('<') => String::new(),
        Err(_) => body.to_string(),
    };
    let clean: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !harness_core::text::is_hidden(*c))
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() > MAX_DETAIL {
        format!("{}…", clean.chars().take(MAX_DETAIL).collect::<String>())
    } else {
        clean
    }
}

pub(crate) fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

pub(crate) fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub(crate) fn from_blob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::error_detail;

    #[test]
    fn error_detail_is_short_and_printable() {
        assert_eq!(
            error_detail(r#"{"error":{"message":"model \"x\" not found"}}"#),
            "model \"x\" not found"
        );
        assert_eq!(error_detail(r#"{"error":"bad key"}"#), "bad key");
        assert_eq!(error_detail(r#"{"detail":"Not Found"}"#), "Not Found");
        assert_eq!(error_detail("<html><body>404</body></html>"), "");
        assert_eq!(
            error_detail("no\n\tsuch\u{202E} route\u{7}"),
            "no such route"
        );
        assert_eq!(error_detail(&"a".repeat(500)).chars().count(), 201);
    }
}
