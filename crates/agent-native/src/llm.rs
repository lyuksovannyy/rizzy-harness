//! Real providers via the `genai` crate: Anthropic Messages API and any
//! OpenAI-compatible chat-completions endpoint (OpenAI, DeepSeek, OpenRouter,
//! Ollama, vLLM...). `genai` is pinned exactly and only used here, behind our
//! own `Provider` trait, so it can be swapped without touching the loop.

use futures::StreamExt;
use genai::adapter::AdapterKind;
use genai::chat::{
    CacheControl, ChatMessage, ChatOptions, ChatRequest as GChatRequest, ChatStreamEvent,
    ContentPart, MessageContent, ReasoningEffort, Tool, ToolCall, ToolResponse,
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
pub(crate) fn normalize_base(url: &str) -> String {
    let mut u = url.trim().to_string();
    if !u.ends_with('/') {
        u.push('/');
    }
    u
}

/// The bearer sent to endpoints that take no key.
pub const NO_KEY: &str = "no-key";

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
                    // genai refuses a request with no key at all, so a
                    // key-less endpoint (Ollama, llama.cpp, vLLM, LM Studio)
                    // gets the placeholder those servers ignore.
                    auth: AuthData::from_single(match &api_key {
                        Some(k) => k.expose().clone(),
                        None => NO_KEY.to_string(),
                    }),
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
    let mut system_seen = false;
    for m in &req.messages {
        match m {
            ChatMsg::System(t) => {
                let mut m = ChatMessage::system(t.clone());
                // Breakpoint 1: tools + system prompt (Anthropic caches the
                // prefix up to and including the marked block).
                if req.cache && !system_seen {
                    m = m.with_options(CacheControl::Ephemeral);
                }
                system_seen = true;
                msgs.push(m)
            }
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
    // Breakpoint 2: the newest message, so the next call (same history plus
    // one more step) reads everything up to here from the cache.
    if req.cache
        && msgs.len() > 1
        && let Some(last) = msgs.last_mut()
    {
        *last = last.clone().with_options(CacheControl::Ephemeral);
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

/// A genai error as ours. An HTTP status (which a stream reports as its
/// first event) becomes `Status`, a failed connection or a timeout `Http`,
/// so the loop can tell a model-level failure from a broken stream.
fn provider_error(e: genai::Error) -> ProviderError {
    let inner = match &e {
        genai::Error::WebStream { error, .. } => Some(error.as_ref()),
        _ => None,
    };
    let status = |g: &genai::Error| match g {
        genai::Error::HttpError { status, body, .. } => Some(ProviderError::Status {
            status: status.as_u16(),
            body: body.clone(),
        }),
        genai::Error::WebModelCall {
            webc_error: genai::webc::Error::ResponseFailedStatus { status, body, .. },
            ..
        } => Some(ProviderError::Status {
            status: status.as_u16(),
            body: body.clone(),
        }),
        _ => g.status().map(|s| ProviderError::Status {
            status: s.as_u16(),
            body: String::new(),
        }),
    };
    if let Some(p) = status(&e).or_else(|| inner?.downcast_ref::<genai::Error>().and_then(status)) {
        return p;
    }
    match inner.and_then(|i| i.downcast_ref::<reqwest::Error>()) {
        Some(r) if r.is_connect() || r.is_timeout() => ProviderError::Http(r.to_string()),
        _ if inner.is_some() => ProviderError::Stream(e.to_string()),
        _ => ProviderError::Http(e.to_string()),
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
            .map_err(provider_error)?
            .stream;
        let mut text = String::new();
        let mut out = Completion::default();
        while let Some(ev) = stream.next().await {
            match ev.map_err(provider_error)? {
                ChatStreamEvent::Chunk(c) => {
                    if !c.content.is_empty() {
                        text.push_str(&c.content);
                        let _ = deltas.send(c.content);
                    }
                }
                ChatStreamEvent::End(end) => {
                    if let Some(u) = end.captured_usage {
                        let d = u.prompt_tokens_details.as_ref();
                        let n = |v: Option<i32>| v.unwrap_or(0).max(0) as u64;
                        out.usage = Some(Usage {
                            input_tokens: n(u.prompt_tokens),
                            output_tokens: n(u.completion_tokens),
                            context_tokens: 0,
                            cache_read_tokens: n(d.and_then(|d| d.cached_tokens)),
                            cache_write_tokens: n(d.and_then(|d| d.cache_creation_tokens)),
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

/// Separates a provider prefix from the provider's own model id
/// (`openrouter::openai/gpt-4o`); raw ids may contain `/` but not `::`.
pub const MODEL_SEP: &str = "::";

/// `("openrouter", "openai/gpt-4o")` for a prefixed id, `None` for a legacy
/// unprefixed one.
pub fn split_model(id: &str) -> Option<(&str, &str)> {
    id.split_once(MODEL_SEP)
}

/// One provider behind a [`ModelRouter`]: its prefix and the raw ids it lists.
pub struct Route {
    pub prefix: String,
    pub provider: std::sync::Arc<dyn Provider>,
    pub models: Vec<String>,
}

/// Sends each request to the provider named by the model's prefix, with the
/// prefix stripped. A legacy unprefixed id goes to the first provider that
/// lists it, else to the fallback (the default model's provider).
pub struct ModelRouter {
    routes: Vec<Route>,
    fallback: Option<usize>,
}

impl ModelRouter {
    pub fn new(routes: Vec<Route>, fallback: Option<&str>) -> Self {
        let fallback = fallback
            .and_then(|f| routes.iter().position(|r| r.prefix == f))
            .or((!routes.is_empty()).then_some(0));
        Self { routes, fallback }
    }

    /// The route for `model` and the raw id to send it.
    pub fn resolve<'a>(&self, model: &'a str) -> Result<(&Route, &'a str), ProviderError> {
        let unknown = || ProviderError::UnknownModel(model.to_string());
        match split_model(model) {
            Some((prefix, raw)) => self
                .routes
                .iter()
                .find(|r| r.prefix == prefix)
                .map(|r| (r, raw))
                .ok_or_else(unknown),
            None => self
                .routes
                .iter()
                .find(|r| r.models.iter().any(|m| m == model))
                .or_else(|| self.fallback.map(|i| &self.routes[i]))
                .map(|r| (r, model))
                .ok_or_else(unknown),
        }
    }
}

#[async_trait::async_trait]
impl Provider for ModelRouter {
    fn name(&self) -> &'static str {
        match self.routes.as_slice() {
            [one] => one.provider.name(),
            _ => "multi-provider",
        }
    }

    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let (route, raw) = self.resolve(&req.model)?;
        let mut req = req.clone();
        req.model = raw.to_string();
        route.provider.complete(&req, deltas).await
    }
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
            cache: false,
        });
        assert_eq!(r.messages.len(), 4);
        assert_eq!(r.tools.as_ref().map(|t| t.len()), Some(1));
    }

    /// Answers with its own name and the model id it was sent.
    struct Named(&'static str);

    #[async_trait::async_trait]
    impl Provider for Named {
        fn name(&self) -> &'static str {
            self.0
        }
        async fn complete(
            &self,
            req: &ChatRequest,
            _deltas: mpsc::UnboundedSender<String>,
        ) -> Result<Completion, ProviderError> {
            Ok(Completion {
                text: format!("{} {}", self.0, req.model),
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn router_strips_prefix_and_resolves_legacy_ids() {
        let route = |prefix: &str, name: &'static str, models: &[&str]| Route {
            prefix: prefix.into(),
            provider: std::sync::Arc::new(Named(name)),
            models: models.iter().map(|m| m.to_string()).collect(),
        };
        let r = ModelRouter::new(
            vec![
                route("openrouter", "a", &["openai/gpt-4o", "shared"]),
                route("nim", "b", &["meta/llama-3.1-70b-instruct", "shared"]),
            ],
            Some("nim"),
        );
        let ask = |model: &str| {
            let req = ChatRequest {
                model: model.into(),
                messages: vec![],
                tools: vec![],
                effort: None,
                cache: false,
            };
            let r = &r;
            async move {
                let (tx, _rx) = mpsc::unbounded_channel();
                r.complete(&req, tx).await.map(|c| c.text)
            }
        };
        assert_eq!(
            ask("openrouter::openai/gpt-4o").await.unwrap(),
            "a openai/gpt-4o"
        );
        assert_eq!(
            ask("nim::meta/llama-3.1-70b-instruct").await.unwrap(),
            "b meta/llama-3.1-70b-instruct"
        );
        // Legacy: the first provider listing it, else the fallback.
        assert_eq!(ask("shared").await.unwrap(), "a shared");
        assert_eq!(ask("old-model").await.unwrap(), "b old-model");
        assert!(matches!(
            ask("gone::x").await,
            Err(ProviderError::UnknownModel(_))
        ));
        assert_eq!(r.name(), "multi-provider");
    }
}
