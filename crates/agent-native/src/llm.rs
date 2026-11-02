//! Real providers via the `genai` crate: Anthropic Messages API and any
//! OpenAI-compatible chat-completions endpoint (OpenAI, DeepSeek, OpenRouter,
//! Ollama, vLLM...). `genai` is pinned exactly and only used here, behind our
//! own `Provider` trait, so it can be swapped without touching the loop.

use futures::StreamExt;
use genai::adapter::AdapterKind;
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest as GChatRequest, ChatStreamEvent, ContentPart,
    MessageContent, ReasoningEffort, Tool, ToolCall, ToolResponse,
};
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{Client, ModelIden, ServiceTarget};
use harness_core::Secret;
use harness_core::event::{ToolCallRef, Usage};
use tokio::sync::mpsc;

use crate::provider::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKind {
    /// OpenAI chat-completions wire format.
    Openai,
    /// Anthropic Messages API (API key only; no subscription OAuth).
    Anthropic,
}

pub struct LlmProvider {
    client: Client,
    kind: ApiKind,
}

/// `genai` appends paths like `messages` to the base URL, so it must end in `/`.
fn normalize_base(url: &str) -> String {
    let mut u = url.trim().to_string();
    if !u.ends_with('/') {
        u.push('/');
    }
    u
}

impl LlmProvider {
    pub fn new(
        kind: ApiKind,
        base_url: &str,
        api_key: Option<Secret<String>>,
    ) -> Result<Self, ProviderError> {
        let endpoint = normalize_base(base_url);
        let adapter = match kind {
            ApiKind::Openai => AdapterKind::OpenAI,
            ApiKind::Anthropic => AdapterKind::Anthropic,
        };
        let resolver = ServiceTargetResolver::from_resolver_fn(
            move |t: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
                Ok(ServiceTarget {
                    endpoint: Endpoint::from_owned(endpoint.clone()),
                    auth: match &api_key {
                        Some(k) => AuthData::from_single(k.expose().clone()),
                        None => AuthData::None,
                    },
                    model: ModelIden::new(adapter, t.model.model_name),
                })
            },
        );
        let client = Client::builder()
            .with_service_target_resolver(resolver)
            .build()
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        Ok(Self { client, kind })
    }
}

pub(crate) fn to_genai(req: &ChatRequest) -> GChatRequest {
    let mut msgs = Vec::new();
    for m in &req.messages {
        match m {
            ChatMsg::System(t) => msgs.push(ChatMessage::system(t.clone())),
            ChatMsg::User(t) => msgs.push(ChatMessage::user(t.clone())),
            ChatMsg::Assistant { text, tool_calls } => {
                let mut parts = Vec::new();
                if !text.is_empty() {
                    parts.push(ContentPart::Text(text.clone()));
                }
                for c in tool_calls {
                    parts.push(ContentPart::ToolCall(ToolCall {
                        call_id: c.call_id.clone(),
                        fn_name: c.tool.clone(),
                        fn_arguments: c.input.clone(),
                        thought_signatures: None,
                    }));
                }
                if !parts.is_empty() {
                    msgs.push(ChatMessage::assistant(MessageContent::from_parts(parts)));
                }
            }
            ChatMsg::Tool { call_id, content } => msgs.push(ChatMessage::from(ToolResponse::new(
                call_id.clone(),
                content.clone(),
            ))),
        }
    }
    let mut r = GChatRequest::new(msgs);
    if !req.tools.is_empty() {
        r = r.with_tools(
            req.tools
                .iter()
                .map(|t| {
                    Tool::new(t.name.clone())
                        .with_description(t.description.clone())
                        .with_schema(t.schema.clone())
                })
                .collect::<Vec<_>>(),
        );
    }
    r
}

fn effort(e: Effort) -> ReasoningEffort {
    match e {
        Effort::Low => ReasoningEffort::Low,
        Effort::Medium => ReasoningEffort::Medium,
        Effort::High => ReasoningEffort::High,
        Effort::Max => ReasoningEffort::Max,
    }
}

#[async_trait::async_trait]
impl Provider for LlmProvider {
    fn name(&self) -> &'static str {
        match self.kind {
            ApiKind::Openai => "openai-compatible",
            ApiKind::Anthropic => "anthropic",
        }
    }

    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let mut opts = ChatOptions::default()
            .with_capture_content(true)
            .with_capture_tool_calls(true)
            .with_capture_usage(true);
        if let Some(e) = req.effort {
            opts = opts.with_reasoning_effort(effort(e));
        }
        let mut stream = self
            .client
            .exec_chat_stream(&req.model, to_genai(req), Some(&opts))
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?
            .stream;
        let mut text = String::new();
        let mut out = Completion::default();
        while let Some(ev) = stream.next().await {
            match ev.map_err(|e| ProviderError::Stream(e.to_string()))? {
                ChatStreamEvent::Chunk(c) => {
                    if !c.content.is_empty() {
                        text.push_str(&c.content);
                        let _ = deltas.send(c.content);
                    }
                }
                ChatStreamEvent::End(end) => {
                    if let Some(u) = end.captured_usage {
                        out.usage = Some(Usage {
                            input_tokens: u.prompt_tokens.unwrap_or(0).max(0) as u64,
                            output_tokens: u.completion_tokens.unwrap_or(0).max(0) as u64,
                        });
                    }
                    if let Some(content) = end.captured_content {
                        for part in content.into_parts() {
                            if let ContentPart::ToolCall(tc) = part {
                                out.tool_calls.push(ToolCallRef {
                                    call_id: tc.call_id,
                                    tool: tc.fn_name,
                                    input: tc.fn_arguments,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        out.text = text;
        Ok(out)
    }
}

/// A provider that can be replaced while sessions are running (onboarding /
/// settings change the provider without a restart). A turn keeps the provider
/// it started with.
pub struct SwappableProvider {
    inner: std::sync::RwLock<std::sync::Arc<dyn Provider>>,
}

impl SwappableProvider {
    pub fn new(p: std::sync::Arc<dyn Provider>) -> Self {
        Self {
            inner: std::sync::RwLock::new(p),
        }
    }

    pub fn set(&self, p: std::sync::Arc<dyn Provider>) {
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = p;
    }

    pub fn current(&self) -> std::sync::Arc<dyn Provider> {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[async_trait::async_trait]
impl Provider for SwappableProvider {
    fn name(&self) -> &'static str {
        self.current().name()
    }

    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let p = self.current();
        p.complete(req, deltas).await
    }
}

/// List model ids from `GET {base}/models` (OpenAI-compatible and Anthropic
/// both answer `{"data": [{"id": ...}]}`). Doubles as a key check.
pub async fn list_models(
    kind: ApiKind,
    base_url: &str,
    api_key: Option<&Secret<String>>,
) -> Result<Vec<String>, ProviderError> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| ProviderError::Http(e.to_string()))?;
    let url = format!("{}models", normalize_base(base_url));
    let mut rb = client.get(&url);
    match (kind, api_key) {
        (ApiKind::Anthropic, Some(k)) => {
            rb = rb
                .header("x-api-key", k.expose())
                .header("anthropic-version", "2023-06-01");
        }
        (ApiKind::Anthropic, None) => {
            rb = rb.header("anthropic-version", "2023-06-01");
        }
        (ApiKind::Openai, Some(k)) => rb = rb.bearer_auth(k.expose()),
        (ApiKind::Openai, None) => {}
    }
    let resp = rb
        .send()
        .await
        .map_err(|e| ProviderError::Http(e.without_url().to_string()))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(ProviderError::Status {
            status,
            body: body.chars().take(300).collect(),
        });
    }
    let v: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| ProviderError::Stream(e.to_string()))?;
    let mut ids: Vec<String> = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn base_url_gets_trailing_slash() {
        assert_eq!(
            normalize_base("https://api.anthropic.com/v1"),
            "https://api.anthropic.com/v1/"
        );
        assert_eq!(normalize_base("http://x/v1/"), "http://x/v1/");
    }

    #[test]
    fn history_converts_with_tool_calls() {
        let r = to_genai(&ChatRequest {
            model: "m".into(),
            messages: vec![
                ChatMsg::System("s".into()),
                ChatMsg::User("u".into()),
                ChatMsg::Assistant {
                    text: "".into(),
                    tool_calls: vec![ToolCallRef {
                        call_id: "c1".into(),
                        tool: "bash".into(),
                        input: json!({"command": "ls"}),
                    }],
                },
                ChatMsg::Tool {
                    call_id: "c1".into(),
                    content: "out".into(),
                },
            ],
            tools: vec![ToolSpec {
                name: "bash".into(),
                description: "d".into(),
                schema: json!({"type": "object"}),
            }],
            effort: None,
        });
        assert_eq!(r.messages.len(), 4);
        assert_eq!(r.tools.as_ref().map(|t| t.len()), Some(1));
    }
}
