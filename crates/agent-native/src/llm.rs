//! Real providers via the `genai` crate: Anthropic Messages API and any
//! OpenAI-compatible chat-completions endpoint (OpenAI, DeepSeek, OpenRouter,
//! Ollama, vLLM...). `genai` is pinned exactly and only used here, behind our
//! own `Provider` trait, so it can be swapped without touching the loop.

use futures::StreamExt;
use genai::adapter::AdapterKind;
use genai::chat::{
    Binary, CacheControl, ChatMessage, ChatOptions, ChatRequest as GChatRequest, ChatStreamEvent,
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

/// A user message's text, then its images as binary parts (an image
/// without bytes is named instead; `images::attach` leaves none).
fn user_parts(text: &str, images: &[ImagePart]) -> Vec<ContentPart> {
    let mut parts = Vec::with_capacity(images.len() + 1);
    let mut text = text.to_string();
    for i in images.iter().filter(|i| i.data.is_none()) {
        text = format!("{text}\n\n{}", image_note(&i.name));
    }
    if !text.is_empty() {
        parts.push(ContentPart::Text(text));
    }
    parts.extend(images.iter().filter_map(|i| {
        let data = i.data.clone()?;
        Some(ContentPart::Binary(Binary::from_base64(
            i.mime.clone(),
            data,
            Some(i.name.clone()),
        )))
    }));
    parts
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
            // The loop places every note before it sends (`place_notes`,
            // T8D-3). One that arrives here was not placed: it is left out,
            // never sent as a user-role message of its own.
            ChatMsg::Note(_) => {
                tracing::warn!("a note reached a provider unplaced; it is not sent");
            }
            ChatMsg::UserImages { text, images } => msgs.push(ChatMessage::user(
                MessageContent::from_parts(user_parts(text, images)),
            )),
            ChatMsg::Assistant { text, tool_calls } => {
                let mut parts = Vec::new();
                if !text.is_empty() {
                    parts.push(ContentPart::Text(text.clone()));
                }
                for c in tool_calls {
                    parts.push(ContentPart::ToolCall(ToolCall {
                        call_id: c.call_id.clone(),
                        fn_name: c.tool.clone(),
                        // A call whose arguments did not parse (cut by the
                        // output limit) is kept as a string; Anthropic
                        // refuses a `tool_use` whose input is not an
                        // object. Its result says it did not run.
                        fn_arguments: if c.input.is_object() {
                            c.input.clone()
                        } else {
                            serde_json::Value::Object(Default::default())
                        },
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
    let status = |g: &genai::Error| {
        let hint = g.headers().map(RetryHint::from_headers).unwrap_or_default();
        match g {
            genai::Error::HttpError { status, body, .. } => Some(ProviderError::Status {
                status: status.as_u16(),
                body: body.clone(),
                hint,
            }),
            genai::Error::WebModelCall {
                webc_error: genai::webc::Error::ResponseFailedStatus { status, body, .. },
                ..
            } => Some(ProviderError::Status {
                status: status.as_u16(),
                body: body.clone(),
                hint,
            }),
            _ => g.status().map(|s| ProviderError::Status {
                status: s.as_u16(),
                body: String::new(),
                hint,
            }),
        }
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

/// Why the model stopped, from the provider's own value (the raw string;
/// genai's own normalisation is not used, so this table is the whole
/// rule). A value not listed, or none, is `Other`: never `End`.
pub fn stop_reason(kind: ApiKind, raw: Option<&str>) -> StopReason {
    let Some(raw) = raw else {
        return StopReason::Other;
    };
    match (kind, raw) {
        // Chat completions `finish_reason`; `model_length` (Mistral) and
        // `max_tokens` (gateways relaying Anthropic) are compatible
        // servers' spellings of the output limit.
        (ApiKind::Openai, "stop") => StopReason::End,
        (ApiKind::Openai, "tool_calls" | "function_call") => StopReason::ToolUse,
        (ApiKind::Openai, "length" | "model_length" | "max_tokens") => StopReason::OutputLimit,
        // Messages API `stop_reason`. We send no stop sequences, so one is
        // an ordinary end. `model_context_window_exceeded`: the answer was
        // cut because the context window filled.
        (ApiKind::Anthropic, "end_turn" | "stop_sequence") => StopReason::End,
        (ApiKind::Anthropic, "tool_use") => StopReason::ToolUse,
        (ApiKind::Anthropic, "max_tokens" | "model_context_window_exceeded") => {
            StopReason::OutputLimit
        }
        _ => StopReason::Other,
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
                    out.stop = stop_reason(
                        self.kind,
                        end.captured_stop_reason.as_ref().map(|s| s.raw()),
                    );
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

/// `("openrouter", "openai/gpt-4o")` for a prefixed id, `None` for an id
/// with no provider prefix (which names no model).
pub fn split_model(id: &str) -> Option<(&str, &str)> {
    id.split_once(MODEL_SEP)
}

/// One provider behind a [`ModelRouter`], under its prefix.
pub struct Route {
    pub prefix: String,
    pub provider: std::sync::Arc<dyn Provider>,
}

/// Sends each request to the provider named by the model's prefix, with the
/// prefix stripped.
pub struct ModelRouter {
    routes: Vec<Route>,
}

impl ModelRouter {
    pub fn new(routes: Vec<Route>) -> Self {
        Self { routes }
    }

    /// The route for `model` and the raw id to send it.
    pub fn resolve<'a>(&self, model: &'a str) -> Result<(&Route, &'a str), ProviderError> {
        split_model(model)
            .and_then(|(prefix, raw)| {
                self.routes
                    .iter()
                    .find(|r| r.prefix == prefix)
                    .map(|r| (r, raw))
            })
            .ok_or_else(|| ProviderError::UnknownModel(model.to_string()))
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

    /// Each provider's documented values, and what is not documented.
    /// OpenAI chat completions `finish_reason` (API reference, chat
    /// completion object; `function_call` is the deprecated form) and the
    /// values OpenAI-compatible servers add (`model_length`: Mistral;
    /// `max_tokens`: gateways passing Anthropic's value through).
    /// Anthropic `stop_reason` (Messages API reference, "Handling stop
    /// reasons").
    #[test]
    fn stop_reasons_map_per_provider() {
        use StopReason::*;
        let table = [
            (ApiKind::Openai, "stop", End),
            (ApiKind::Openai, "tool_calls", ToolUse),
            (ApiKind::Openai, "function_call", ToolUse),
            (ApiKind::Openai, "length", OutputLimit),
            (ApiKind::Openai, "model_length", OutputLimit),
            (ApiKind::Openai, "max_tokens", OutputLimit),
            (ApiKind::Openai, "content_filter", Other),
            (ApiKind::Openai, "insufficient_system_resource", Other),
            (ApiKind::Openai, "end_turn", Other),
            (ApiKind::Openai, "STOP", Other),
            (ApiKind::Anthropic, "end_turn", End),
            (ApiKind::Anthropic, "stop_sequence", End),
            (ApiKind::Anthropic, "tool_use", ToolUse),
            (ApiKind::Anthropic, "max_tokens", OutputLimit),
            (
                ApiKind::Anthropic,
                "model_context_window_exceeded",
                OutputLimit,
            ),
            (ApiKind::Anthropic, "pause_turn", Other),
            (ApiKind::Anthropic, "refusal", Other),
            (ApiKind::Anthropic, "stop", Other),
            (ApiKind::Anthropic, "", Other),
        ];
        for (kind, raw, want) in table {
            assert_eq!(stop_reason(kind, Some(raw)), want, "{kind:?} {raw:?}");
        }
        for kind in [ApiKind::Openai, ApiKind::Anthropic] {
            assert_eq!(stop_reason(kind, None), Other, "no value is not an end");
        }
    }

    #[test]
    fn base_url_gets_trailing_slash() {
        assert_eq!(
            normalize_base("https://api.anthropic.com/v1"),
            "https://api.anthropic.com/v1/"
        );
        assert_eq!(normalize_base("http://x/v1/"), "http://x/v1/");
    }

    /// T8D-3: a note the loop did not place never becomes a user-role
    /// message of its own.
    #[test]
    fn a_note_left_in_a_request_is_not_sent_as_a_user_message() {
        let r = to_genai(&ChatRequest {
            model: "m".into(),
            messages: vec![
                ChatMsg::System("s".into()),
                ChatMsg::User("u".into()),
                ChatMsg::Note("[harness] stray".into()),
            ],
            tools: vec![],
            effort: None,
            cache: false,
        });
        assert_eq!(r.messages.len(), 2, "the note was sent as a message");
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

    #[test]
    fn images_become_binary_parts() {
        let r = to_genai(&ChatRequest {
            model: "m".into(),
            messages: vec![ChatMsg::UserImages {
                text: "what is this?".into(),
                images: vec![
                    ImagePart {
                        sha256: "ab".into(),
                        mime: "image/png".into(),
                        name: "a.png".into(),
                        data: Some("aGk=".into()),
                    },
                    ImagePart {
                        sha256: "cd".into(),
                        mime: "image/jpeg".into(),
                        name: "gone.jpg".into(),
                        data: None,
                    },
                ],
            }],
            tools: vec![],
            effort: None,
            cache: false,
        });
        let parts = r.messages[0].content.parts();
        assert_eq!(parts.len(), 2);
        let ContentPart::Text(t) = &parts[0] else {
            panic!("{parts:?}")
        };
        assert_eq!(t, "what is this?\n\n[image: gone.jpg]");
        let ContentPart::Binary(b) = &parts[1] else {
            panic!("{parts:?}")
        };
        assert_eq!(b.content_type, "image/png");
        assert!(matches!(&b.source, genai::chat::BinarySource::Base64(d) if &**d == "aGk="));
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
    async fn router_strips_prefix() {
        let route = |prefix: &str, name: &'static str| Route {
            prefix: prefix.into(),
            provider: std::sync::Arc::new(Named(name)),
        };
        let r = ModelRouter::new(vec![route("openrouter", "a"), route("nim", "b")]);
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
        for unknown in ["gone::x", "openai/gpt-4o"] {
            assert!(matches!(
                ask(unknown).await,
                Err(ProviderError::UnknownModel(_))
            ));
        }
        assert_eq!(r.name(), "multi-provider");
    }
}
