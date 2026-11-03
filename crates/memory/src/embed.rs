//! Embeddings from any OpenAI-compatible `/embeddings` endpoint (Ollama,
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

pub struct Embedder {
    client: reqwest::Client,
    base_url: String,
    pub model: String,
    key: Option<Secret<String>>,
}

impl Embedder {
    pub fn new(base_url: &str, model: &str, key: Option<Secret<String>>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("static client config"),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            key,
        }
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
            rb = rb.bearer_auth(k.expose());
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| MemoryError::Embed(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(MemoryError::Embed(format!("endpoint answered {status}")));
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
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}
