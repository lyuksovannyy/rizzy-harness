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
use harness_core::event::{Thinking, ThinkingBlock, ToolCallRef, Usage};
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

/// The Messages API version genai sends (`anthropic-version`); a key-less
/// Anthropic-style endpoint gets it too.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// What a key-less endpoint (Ollama, llama.cpp, vLLM, LM Studio, a local
/// router) is sent: the URL genai would build and no key header at all.
/// genai's OpenAI and Anthropic adapters always send one (`Bearer <key>`,
/// `x-api-key`) and refuse a request with no key, and a router that rejects
/// unknown keys answers 401 to a placeholder such as `no-key` (live test
/// 2026-11-15). genai's `RequestOverride` replaces the URL and the headers
/// of a chat request (`exec_chat` and `exec_chat_stream`) and nothing
/// else; this provider sends no extra headers that it would drop.
fn keyless(kind: ApiKind, endpoint: &str) -> AuthData {
    let (url, headers) = match kind {
        ApiKind::Openai => (openai_chat_url(endpoint), genai::Headers::default()),
        ApiKind::Anthropic => (
            format!("{endpoint}messages"),
            genai::Headers::from((
                "anthropic-version".to_string(),
                ANTHROPIC_VERSION.to_string(),
            )),
        ),
    };
    AuthData::RequestOverride { url, headers }
}

/// `chat/completions` under the base, its query kept, as genai joins it.
fn openai_chat_url(endpoint: &str) -> String {
    match reqwest::Url::parse(endpoint) {
        Ok(base) => {
            let query = base.query().map(str::to_string);
            match base.join("chat/completions") {
                Ok(mut u) => {
                    u.set_query(query.as_deref());
                    u.to_string()
                }
                Err(_) => format!("{endpoint}chat/completions"),
            }
        }
        Err(_) => format!("{endpoint}chat/completions"),
    }
}

impl LlmProvider {
    /// `api_key` empty or blank is no key: nothing is sent for it.
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
        let api_key = api_key.filter(|k| !k.is_blank());
        let resolver = ServiceTargetResolver::from_resolver_fn(
            move |t: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
                Ok(ServiceTarget {
                    endpoint: Endpoint::from_owned(endpoint.clone()),
                    auth: match &api_key {
                        Some(k) => AuthData::from_single(k.expose().clone()),
                        None => keyless(kind, &endpoint),
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

/// A user message's text, then its images as binary parts (an image
/// without bytes is named instead; `images::attach` leaves none).
fn user_parts(text: &str, images: &[ImagePart]) -> Vec<ContentPart> {
    let mut parts = Vec::with_capacity(images.len() + 1);
    let missing: Vec<ImagePart> = images
        .iter()
        .filter(|i| i.data.is_none())
        .cloned()
        .collect();
    let text = if missing.is_empty() {
        text.to_string()
    } else {
        crate::images::named(text, &missing)
    };
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

/// An image a `read` returned, as the provider is sent it: the harness's
/// line naming the file and the call (each made one line, quotation marks
/// read as `'`), then the image; one whose bytes the store lost, the
/// harness's line saying so. A withheld one adds nothing here: its run's
/// names go into one line ([`crate::images::tool_withheld_line`]).
fn tool_image_parts(m: &ChatMsg) -> Vec<ContentPart> {
    let ChatMsg::ToolImage {
        call_id,
        image,
        state,
        ..
    } = m
    else {
        return vec![];
    };
    match state {
        ImageState::Withheld => vec![],
        ImageState::Missing => vec![ContentPart::Text(crate::images::tool_missing_line(
            &image.name,
        ))],
        ImageState::Shown => match image.data.clone() {
            Some(data) => vec![
                ContentPart::Text(crate::images::tool_image_line(&image.name, call_id)),
                ContentPart::Binary(Binary::from_base64(
                    image.mime.clone(),
                    data,
                    Some(image.name.clone()),
                )),
            ],
            None => vec![ContentPart::Text(crate::images::tool_missing_line(
                &image.name,
            ))],
        },
    }
}

/// Add `parts` to the step's image message (the last one, when the message
/// before was a step's image), or start it.
fn push_image_parts(msgs: &mut Vec<ChatMessage>, open: bool, parts: Vec<ContentPart>) {
    if open && let Some(last) = msgs.last_mut() {
        let mut all = last.content.parts().to_vec();
        all.extend(parts);
        last.content = MessageContent::from_parts(all);
    } else {
        msgs.push(ChatMessage::user(MessageContent::from_parts(parts)));
    }
}

#[cfg(test)]
pub(crate) fn to_genai(req: &ChatRequest) -> GChatRequest {
    convert(req, None).request
}

/// A request as genai is given it, with what the thinking rule decided.
pub(crate) struct Converted {
    pub request: GChatRequest,
    /// [`Prefix`] of everything the request sends: what a signed block the
    /// answer brings records as its `prefix`.
    pub prefix: String,
    /// Whether any earlier step's thinking is sent back.
    pub replayed: bool,
}

/// The running SHA-256 of what a request sends, in order: the tools (name,
/// description, schema), then each message's role and content as genai is
/// given them. Cache markers are left out: they move every request and are
/// not content the model reads (Anthropic's prefix check leaves them out
/// too). A signed block is sent back only where this is what it was when
/// the block was made (X.25, T73).
struct Prefix {
    hash: sha2::Sha256,
    fed: usize,
}

impl Prefix {
    fn new(tools: &[ToolSpec]) -> Self {
        use sha2::Digest;
        let mut hash = sha2::Sha256::new();
        for t in tools {
            let one = serde_json::json!([t.name, t.description, t.schema]).to_string();
            hash.update((one.len() as u64).to_le_bytes());
            hash.update(one.as_bytes());
        }
        hash.update(b"messages");
        Self { hash, fed: 0 }
    }

    /// Take in the messages of `msgs` not yet taken. Called only where
    /// none of them changes any more.
    fn feed(&mut self, msgs: &[ChatMessage]) {
        use sha2::Digest;
        for m in &msgs[self.fed..] {
            let one = serde_json::to_string(&(&m.role, &m.content)).unwrap_or_default();
            self.hash.update((one.len() as u64).to_le_bytes());
            self.hash.update(one.as_bytes());
        }
        self.fed = msgs.len();
    }

    fn hex(&self) -> String {
        use sha2::Digest;
        self.hash
            .clone()
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// The parts a step's recorded thinking adds in front of its message, by
/// the rule of the API the request goes to (X.25, T73), or none:
/// - Anthropic: every `thinking` and `redacted_thinking` block, exactly as
///   it came, in its order, only while everything before the step is what
///   the request that made them sent (`prefix`). The provider requires
///   them inside a tool loop and recommends them across turns; it strips
///   what a model does not keep, and refuses a changed block or one whose
///   prefix changed. Leaving them out is allowed ("omit prior turns'
///   thinking, but not a block between two that you keep"): a block after
///   a left-out one has a changed prefix here, so it is left out too.
/// - OpenAI-compatible: the plain reasoning text as `reasoning_content`
///   (DeepSeek: required back in every later request once tools are in
///   the request, ignored without them).
/// - No request ever sends back a step's thinking of another model: the
///   router passes only the request's own model's ([`ModelRouter`]).
fn thinking_parts(
    t: &harness_core::event::Thinking,
    kind: ApiKind,
    prefix: &Prefix,
) -> Vec<ContentPart> {
    use harness_core::event::ThinkingBlock;
    if t.unsent {
        return vec![];
    }
    match kind {
        ApiKind::Anthropic => {
            if t.prefix.is_empty() || t.prefix != prefix.hex() {
                return vec![];
            }
            let mut parts = Vec::new();
            for b in &t.blocks {
                match b {
                    ThinkingBlock::Signed { text, signature } => {
                        parts.push(ContentPart::ThoughtSignature(signature.clone()));
                        parts.push(ContentPart::ReasoningContent(text.clone()));
                    }
                    ThinkingBlock::Redacted { data } => {
                        parts.push(ContentPart::Custom(genai::chat::CustomPart {
                            model_iden: None,
                            data: serde_json::json!({"type": "redacted_thinking", "data": data}),
                        }));
                    }
                    // Not Anthropic's: never sent there.
                    ThinkingBlock::Text { .. } => return vec![],
                }
            }
            parts
        }
        ApiKind::Openai => {
            let mut text = Vec::new();
            for b in &t.blocks {
                match b {
                    ThinkingBlock::Text { text: x } => text.push(x.as_str()),
                    _ => return vec![],
                }
            }
            if text.is_empty() {
                return vec![];
            }
            vec![ContentPart::ReasoningContent(text.join("\n"))]
        }
    }
}

/// [`to_genai`] for a provider of `kind`: earlier steps' thinking is sent
/// back by that API's rule ([`thinking_parts`]); `None` sends none.
pub(crate) fn convert(req: &ChatRequest, kind: Option<ApiKind>) -> Converted {
    // The hash is needed only where a signed block may be sent back.
    let hash = kind == Some(ApiKind::Anthropic) && signed_thinking(req);
    convert_with(req, kind, hash)
}

/// [`Prefix`] of everything `req` sends to Anthropic: what a signed block
/// its answer brings records. Computed only when one came back.
pub(crate) fn request_prefix(req: &ChatRequest) -> String {
    convert_with(req, Some(ApiKind::Anthropic), true).prefix
}

/// Whether `req` holds a signed or redacted block that may be sent.
fn signed_thinking(req: &ChatRequest) -> bool {
    use harness_core::event::ThinkingBlock;
    req.messages.iter().any(|m| {
        matches!(m, ChatMsg::Assistant { thinking: Some(t), .. }
            if t.blocks.iter().any(|b| !matches!(b, ThinkingBlock::Text { .. })))
    })
}

fn convert_with(req: &ChatRequest, kind: Option<ApiKind>, hash: bool) -> Converted {
    let mut prefix = Prefix::new(&req.tools);
    // A step's blocks were never sent (`unsent`): no signed block after it
    // is (the gap rule, [`crate::provider::settle_thinking`], kept here for
    // a conversation not settled). A block whose prefix changed needs no
    // gap: every block after it that it was sent with has a changed
    // prefix too, and one made after it was left out is valid.
    let mut gap = false;
    let mut replayed = false;
    let mut msgs = Vec::new();
    let mut system_seen = false;
    // Where each user message (the person's, or a callback turn's
    // opening) went in `msgs`.
    let mut users = Vec::new();
    // The message before was a step's image: the next one joins it.
    let mut images_open = false;
    // The files of a step's withheld images, named once its run ends.
    let mut withheld: Vec<String> = Vec::new();
    for m in &req.messages {
        if !withheld.is_empty() && !matches!(m, ChatMsg::ToolImage { .. }) {
            let line = crate::images::tool_withheld_line(&std::mem::take(&mut withheld));
            push_image_parts(&mut msgs, images_open, vec![ContentPart::Text(line)]);
            images_open = true;
        }
        if matches!(
            m,
            ChatMsg::User(_) | ChatMsg::UserImages { .. } | ChatMsg::Opening(_)
        ) {
            users.push(msgs.len());
        }
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
            ChatMsg::Note(_) | ChatMsg::CallNote(_) => {
                tracing::warn!("a note reached a provider unplaced; it is not sent");
            }
            // `place_notes` sends it as the user message it is; the same
            // here (a request must end in a user or tool message).
            ChatMsg::Opening(t) | ChatMsg::Summary(t) | ChatMsg::StepContext(t) => {
                msgs.push(ChatMessage::user(t.clone()))
            }
            ChatMsg::UserImages { text, images } => msgs.push(ChatMessage::user(
                MessageContent::from_parts(user_parts(text, images)),
            )),
            ChatMsg::Assistant {
                text,
                tool_calls,
                thinking,
            } => {
                let mut parts = Vec::new();
                if let (Some(t), Some(kind)) = (thinking, kind) {
                    let signed = t
                        .blocks
                        .iter()
                        .any(|b| !matches!(b, harness_core::event::ThinkingBlock::Text { .. }));
                    let lead = if kind == ApiKind::Anthropic && signed && gap {
                        vec![]
                    } else {
                        // Everything before this message is final here.
                        if hash {
                            prefix.feed(&msgs);
                        }
                        thinking_parts(t, kind, &prefix)
                    };
                    gap |= t.unsent;
                    replayed |= !lead.is_empty();
                    parts.extend(lead);
                }
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
            // A step's images (`place_notes` put them after its results):
            // one user-role message, each under the harness's line.
            ChatMsg::ToolImage {
                image,
                state: ImageState::Withheld,
                ..
            } => {
                withheld.push(image.name.clone());
                continue;
            }
            ChatMsg::ToolImage { .. } => {
                push_image_parts(&mut msgs, images_open, tool_image_parts(m));
            }
        }
        images_open = matches!(m, ChatMsg::ToolImage { .. });
    }
    if !withheld.is_empty() {
        let line = crate::images::tool_withheld_line(&withheld);
        push_image_parts(&mut msgs, images_open, vec![ContentPart::Text(line)]);
    }
    if req.cache && msgs.len() > 1 {
        // Every message rebuilds byte for byte from the log, this turn's
        // own included (its context block is recorded, `context_sent`,
        // X.1), so breakpoint 4 of a turn's last request is read by the
        // next turn's first, two blocks later.
        // Breakpoint 2: the end of the history before this turn's message.
        // Breakpoint 3: the end of the history before the previous user
        // message: where the previous turn put breakpoint 2. Both are read
        // where they end however long the turn was, for a step whose many
        // parallel results put the last entry beyond the 20 blocks a
        // breakpoint looks back (before X.1 they were the only entries a
        // later turn could read: review C43-1).
        // Breakpoint 4: the newest message, so the next step of this turn,
        // and the next turn, read everything up to here.
        // A user message first in the request has no history before it.
        // Places that coincide (the history end is the system prompt in a
        // second turn) are marked once, so with breakpoint 1 there are
        // never more than 4: Anthropic refuses a request with more.
        let at = users
            .iter()
            .rev()
            .take(2)
            .filter(|&&u| u > 0)
            .map(|u| u - 1)
            .chain([msgs.len() - 1]);
        for i in at {
            let m = &mut msgs[i];
            if m.options.as_ref().is_none_or(|o| o.cache_control.is_none()) {
                *m = m.clone().with_options(CacheControl::Ephemeral);
            }
        }
    }
    if hash {
        prefix.feed(&msgs);
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
    Converted {
        request: r,
        prefix: prefix.hex(),
        replayed,
    }
}

/// Where a converted request has cache markers (tests).
#[cfg(test)]
pub(crate) fn marked(r: &GChatRequest) -> Vec<usize> {
    r.messages
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.options
                .as_ref()
                .is_some_and(|o| o.cache_control.is_some())
        })
        .map(|(i, _)| i)
        .collect()
}

/// Each marked prefix of a converted request, as the provider is sent it
/// with the markers left out (moving a marker does not change what a
/// cache entry holds). Tests compare these across turns.
#[cfg(test)]
pub(crate) fn marked_prefixes(r: &GChatRequest) -> Vec<String> {
    marked(r)
        .into_iter()
        .map(|i| {
            let msgs: Vec<_> = r.messages[..=i]
                .iter()
                .map(|m| (&m.role, &m.content))
                .collect();
            serde_json::to_string(&msgs).expect("messages serialise")
        })
        .collect()
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

/// What a call that went silent reads as (both keys named, so the person
/// knows which one to raise).
fn silent(what: &str, after: std::time::Duration) -> ProviderError {
    ProviderError::Silent(format!(
        "the provider {what} for {} s (`[limits] model_first_byte_timeout_s` and \
         `model_idle_timeout_s` in node.toml)",
        after.as_secs()
    ))
}

/// genai's own header timeout, when it fired.
fn header_timeout(e: &genai::Error, limits: Option<StreamLimits>) -> Option<ProviderError> {
    let genai::Error::WebStream { error, .. } = e else {
        return None;
    };
    match error.downcast_ref::<genai::webc::StreamTimeout>()? {
        genai::webc::StreamTimeout::Headers => Some(silent(
            "did not answer",
            limits.map_or_else(Default::default, |l| l.first_byte),
        )),
        genai::webc::StreamTimeout::Read => None,
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

/// Whether every thinking block of an Anthropic answer came before its
/// text and tool calls (`order`: the blocks' types as the stream opened
/// them). genai sends a message's thinking blocks first, so only such an
/// answer is sent back as it came; one with a block after the text or a
/// tool call (a progress update before a second parallel call) is not
/// (X.25).
pub(crate) fn leads(order: &[String]) -> bool {
    let thinking = |t: &str| matches!(t, "thinking" | "redacted_thinking");
    match order.iter().position(|t| !thinking(t)) {
        Some(first) => !order[first..].iter().any(|t| thinking(t)),
        None => true,
    }
}

#[async_trait::async_trait]
impl Provider for LlmProvider {
    fn replays_thinking(&self, req: &ChatRequest) -> bool {
        req.messages.iter().any(|m| {
            matches!(
                m,
                ChatMsg::Assistant {
                    thinking: Some(_),
                    ..
                }
            )
        }) && convert(req, Some(self.kind)).replayed
    }

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
        // The thinking the answer brings is captured to be recorded with
        // its step (X.25); capturing changes nothing genai sends here (it
        // adds Anthropic's `display` only with an effort, which Anthropic
        // models are not sent: PLAN.md R8).
        let mut opts = ChatOptions::default()
            .with_capture_content(true)
            .with_capture_tool_calls(true)
            .with_capture_usage(true)
            .with_capture_reasoning_content(true);
        // The type of each content block, in the order the stream opens
        // them: genai's capture keeps the thinking blocks' order but not
        // where they stood among the text and the tool calls.
        let order: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        if self.kind == ApiKind::Anthropic {
            let seen = order.clone();
            opts = opts.with_raw_frame_fn(move |_, frame| {
                if frame.event == Some("content_block_start")
                    && let Some(v) = frame.json()
                    && let Some(t) = v.pointer("/content_block/type").and_then(|t| t.as_str())
                {
                    let mut seen = seen.lock().unwrap_or_else(|e| e.into_inner());
                    // Bounded: a hostile stream opening blocks forever
                    // grows nothing past this.
                    if seen.len() < 4096 {
                        seen.push(t.to_string());
                    }
                }
            });
        }
        if let Some(e) = req.effort {
            opts = opts.with_reasoning_effort(effort(e));
        }
        if let Some(n) = req.max_tokens {
            opts = opts.with_max_tokens(n);
        }
        // No answer without tool calls is asked of a model the loop would
        // refuse one from (the tools stay listed: Anthropic needs them once
        // the history holds tool use).
        if req.no_tool_calls && !req.tools.is_empty() {
            opts = opts.with_tool_choice(genai::chat::ToolChoice::None);
        }
        // Neither genai nor reqwest has a timeout here by default. genai
        // times the wait for the response headers; the wait for the first
        // event after them, and between events (keep-alive pings count),
        // is timed below.
        if let Some(l) = req.limits {
            opts = opts.with_stream_header_timeout(l.first_byte);
        }
        let limits = req.limits;
        let fail =
            |e: genai::Error| header_timeout(&e, limits).unwrap_or_else(|| provider_error(e));
        let converted = convert(req, Some(self.kind));
        let mut stream = self
            .client
            .exec_chat_stream(&req.model, converted.request, Some(&opts))
            .await
            .map_err(fail)?
            .stream;
        let mut text = String::new();
        let mut out = Completion::default();
        // `Start` comes with the headers: the first answer is the event
        // after it.
        let mut answered = false;
        loop {
            let next = match limits {
                None => stream.next().await,
                Some(l) => {
                    let (wait, what) = if answered {
                        (l.idle, "sent nothing more")
                    } else {
                        (l.first_byte, "sent nothing")
                    };
                    match tokio::time::timeout(wait, stream.next()).await {
                        Ok(n) => n,
                        Err(_) => return Err(silent(what, wait)),
                    }
                }
            };
            let Some(ev) = next else { break };
            let ev = ev.map_err(fail)?;
            answered |= !matches!(ev, ChatStreamEvent::Start);
            match ev {
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
                            ..Default::default()
                        });
                    }
                    let mut blocks = Vec::new();
                    if let Some(content) = end.captured_content {
                        let mut parts = content.into_parts().into_iter().peekable();
                        while let Some(part) = parts.next() {
                            match part {
                                ContentPart::ToolCall(tc) => out.tool_calls.push(ToolCallRef {
                                    call_id: tc.call_id,
                                    tool: tc.fn_name,
                                    input: tc.fn_arguments,
                                }),
                                // genai's capture: a signature, then its text.
                                ContentPart::ThoughtSignature(signature)
                                    if self.kind == ApiKind::Anthropic =>
                                {
                                    let text = match parts.peek() {
                                        Some(ContentPart::ReasoningContent(_)) => {
                                            match parts.next() {
                                                Some(ContentPart::ReasoningContent(t)) => t,
                                                _ => String::new(),
                                            }
                                        }
                                        _ => String::new(),
                                    };
                                    blocks.push(ThinkingBlock::Signed { text, signature });
                                }
                                ContentPart::Custom(c)
                                    if self.kind == ApiKind::Anthropic
                                        && c.data.get("type").and_then(|t| t.as_str())
                                            == Some("redacted_thinking") =>
                                {
                                    let data = c
                                        .data
                                        .get("data")
                                        .and_then(|d| d.as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    blocks.push(ThinkingBlock::Redacted { data });
                                }
                                _ => {}
                            }
                        }
                    }
                    if self.kind == ApiKind::Openai
                        && let Some(r) = end.captured_reasoning_content
                        && !r.is_empty()
                    {
                        blocks.push(ThinkingBlock::Text { text: r });
                    }
                    if !blocks.is_empty() {
                        let order = order.lock().unwrap_or_else(|e| e.into_inner());
                        out.thinking = Some(Thinking {
                            model: req.model.clone(),
                            prefix: if self.kind == ApiKind::Anthropic {
                                request_prefix(req)
                            } else {
                                String::new()
                            },
                            unsent: !leads(&order),
                            blocks,
                        });
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

    fn replays_thinking(&self, req: &ChatRequest) -> bool {
        self.current().replays_thinking(req)
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

/// `req` for the route's provider: the raw model id, and only the
/// request's own model's thinking (X.25, T73): thinking from another
/// model, or another provider, is never sent; a signed block is valid only
/// for the model that made it, whatever the provider would accept.
fn own_thinking(req: &ChatRequest, raw: &str) -> ChatRequest {
    let mut sent = req.clone();
    sent.model = raw.to_string();
    for m in &mut sent.messages {
        if let ChatMsg::Assistant { thinking, .. } = m
            && thinking.as_ref().is_some_and(|t| t.model != req.model)
        {
            *thinking = None;
        }
    }
    sent
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

    fn replays_thinking(&self, req: &ChatRequest) -> bool {
        match self.resolve(&req.model) {
            Ok((route, raw)) => route.provider.replays_thinking(&own_thinking(req, raw)),
            Err(_) => false,
        }
    }

    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let (route, raw) = self.resolve(&req.model)?;
        let sent = own_thinking(req, raw);
        let mut c = route.provider.complete(&sent, deltas).await?;
        // The thinking is the prefixed model's, as usage is.
        if let Some(t) = c.thinking.as_mut() {
            t.model = req.model.clone();
        }
        // Who used these tokens: the prefixed model asked for, whatever
        // the provider claims. Every model call of a turn, a fallback, a
        // summary, a sub-agent or a hand-off note passes here, so the
        // turn's `Usage::add` keeps a per-model breakdown to price.
        if let Some(u) = c.usage.as_mut() {
            u.stamp(&req.model);
        }
        Ok(c)
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
            limits: None,
            no_tool_calls: false,
            max_tokens: None,
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
                    thinking: None,
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
            limits: None,
            no_tool_calls: false,
            max_tokens: None,
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
            limits: None,
            no_tool_calls: false,
            max_tokens: None,
        });
        let parts = r.messages[0].content.parts();
        assert_eq!(parts.len(), 2);
        let ContentPart::Text(t) = &parts[0] else {
            panic!("{parts:?}")
        };
        assert_eq!(t, &format!("what is this?\n\n{}", image_note("gone.jpg")));
        let ContentPart::Binary(b) = &parts[1] else {
            panic!("{parts:?}")
        };
        assert_eq!(b.content_type, "image/png");
        assert!(matches!(&b.source, genai::chat::BinarySource::Base64(d) if &**d == "aGk="));
    }

    /// A step's images go as one user-role message after its results: a
    /// shown one under the harness's line saying it is a file from the
    /// folder, a lost one as the line that it could not be loaded, and the
    /// withheld ones of the run as one line naming each file (T71).
    #[test]
    fn tool_images_shown_lost_and_withheld() {
        let img = |c: &str, data: Option<&str>, state: ImageState| ChatMsg::ToolImage {
            call_id: c.into(),
            image: ImagePart {
                sha256: c.into(),
                mime: "image/png".into(),
                name: format!("{c}.png"),
                data: data.map(Into::into),
            },
            bytes: 3,
            state,
        };
        let tool = |c: &str| ChatMsg::Tool {
            call_id: c.into(),
            content: "r".into(),
        };
        let r = to_genai(&ChatRequest {
            model: "m".into(),
            messages: vec![
                user("look"),
                ChatMsg::Assistant {
                    text: String::new(),
                    tool_calls: ["a", "b", "c", "d"]
                        .iter()
                        .map(|c| ToolCallRef {
                            call_id: c.to_string(),
                            tool: "read".into(),
                            input: serde_json::json!({}),
                        })
                        .collect(),
                    thinking: None,
                },
                tool("a"),
                tool("b"),
                tool("c"),
                tool("d"),
                img("a", Some("aGk="), ImageState::Shown),
                img("b", None, ImageState::Withheld),
                img("c", None, ImageState::Missing),
                img("d", None, ImageState::Withheld),
            ],
            tools: vec![],
            effort: None,
            cache: false,
            limits: None,
            no_tool_calls: false,
            max_tokens: None,
        });
        assert_eq!(r.messages.len(), 7, "one message for the step's images");
        let parts = r.messages[6].content.parts();
        let texts: Vec<&str> = parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(parts.len(), 4, "{parts:?}");
        assert!(matches!(&parts[1], ContentPart::Binary(_)));
        assert!(texts[0].contains("\"a.png\"") && texts[0].contains("not the person's words"));
        assert_eq!(texts[1], crate::images::tool_missing_line("c.png"));
        assert_eq!(
            texts[2],
            crate::images::tool_withheld_line(&["b.png".into(), "d.png".into()])
        );
        assert!(texts[2].contains("\"b.png\", \"d.png\"") && texts[2].contains("alone"));
        // Withheld alone: still one message, the line only.
        let r = to_genai(&ChatRequest {
            model: "m".into(),
            messages: vec![
                tool("b"),
                img("b", None, ImageState::Withheld),
                user("next"),
            ],
            tools: vec![],
            effort: None,
            cache: false,
            limits: None,
            no_tool_calls: false,
            max_tokens: None,
        });
        assert_eq!(r.messages.len(), 3);
        assert!(
            matches!(&r.messages[1].content.parts()[..], [ContentPart::Text(t)] if t.contains("\"b.png\""))
        );
    }

    fn user(t: &str) -> ChatMsg {
        ChatMsg::User(t.into())
    }
    fn said(t: &str) -> ChatMsg {
        ChatMsg::Assistant {
            text: t.into(),
            tool_calls: vec![],
            thinking: None,
        }
    }
    fn called(id: &str) -> [ChatMsg; 2] {
        [
            ChatMsg::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCallRef {
                    call_id: id.into(),
                    tool: "bash".into(),
                    input: json!({"command": "ls"}),
                }],
                thinking: None,
            },
            ChatMsg::Tool {
                call_id: id.into(),
                content: "out".into(),
            },
        ]
    }
    fn cached(messages: Vec<ChatMsg>) -> Vec<usize> {
        marked(&to_genai(&ChatRequest {
            model: "m".into(),
            messages,
            tools: vec![],
            effort: None,
            cache: true,
            limits: None,
            no_tool_calls: false,
            max_tokens: None,
        }))
    }

    /// The message before this turn's ends the history, and the message
    /// before the previous user message is where the previous turn put
    /// that marker (C43-1; since X.1 the newest marker is read by the next
    /// turn too, and these two are for a step past the 20-block lookback).
    #[test]
    fn the_history_before_this_turn_and_the_last_are_marked() {
        let mut m = vec![ChatMsg::System("s".into()), user("u1"), said("a1")];
        m.push(user("u2"));
        m.extend(called("c1"));
        m.push(said("a2"));
        m.push(user("context then u3"));
        // system, a1 (before u2), a2 (before u3), u3 (the newest)
        assert_eq!(cached(m.clone()), vec![0, 2, 6, 7]);
        // A later step of the same turn: the history markers stay, the
        // newest moves to the tool result.
        m.extend(called("c2"));
        assert_eq!(cached(m), vec![0, 2, 6, 9]);
    }

    /// A first turn has no history: the system prompt and the message.
    /// A second turn's previous history end is the system prompt. A
    /// callback turn's opening is a user message too.
    #[test]
    fn short_requests_mark_each_place_once() {
        let sys = || ChatMsg::System("s".into());
        assert_eq!(cached(vec![sys(), user("u1")]), vec![0, 1]);
        assert_eq!(
            cached(vec![sys(), user("u1"), said("a1"), user("u2")]),
            vec![0, 2, 3]
        );
        assert_eq!(
            cached(vec![
                sys(),
                user("u1"),
                said("a1"),
                ChatMsg::Opening("ended".into())
            ]),
            vec![0, 2, 3]
        );
        assert_eq!(cached(vec![user("only")]), Vec::<usize>::new());
        let off = to_genai(&ChatRequest {
            model: "m".into(),
            messages: vec![sys(), user("u1"), said("a1"), user("u2")],
            tools: vec![],
            effort: None,
            cache: false,
            limits: None,
            no_tool_calls: false,
            max_tokens: None,
        });
        assert!(marked(&off).is_empty());
    }

    /// Anthropic refuses a request with more than 4 breakpoints. Every
    /// prefix of a conversation that mixes text, tool loops, images and
    /// several system messages stays within it, and keeps the system
    /// prompt and the newest message marked.
    #[test]
    fn no_request_has_more_than_four_breakpoints() {
        let mut all = vec![ChatMsg::System("s".into()), ChatMsg::System("s2".into())];
        for t in 0..6 {
            all.push(ChatMsg::UserImages {
                text: format!("u{t}"),
                images: vec![],
            });
            for c in 0..t {
                all.extend(called(&format!("c{t}-{c}")));
            }
            all.push(said(&format!("a{t}")));
            all.push(user(&format!("again {t}")));
        }
        for n in 2..=all.len() {
            let r = to_genai(&ChatRequest {
                model: "m".into(),
                messages: all[..n].to_vec(),
                tools: vec![],
                effort: None,
                cache: true,
                limits: None,
                no_tool_calls: false,
                max_tokens: None,
            });
            let at = marked(&r);
            assert!(at.len() <= 4, "{n}: {at:?}");
            assert_eq!(at.first(), Some(&0), "{n}: {at:?}");
            assert_eq!(at.last(), Some(&(r.messages.len() - 1)), "{n}: {at:?}");
        }
    }

    use harness_core::event::{Thinking, ThinkingBlock};
    use std::sync::Arc;

    fn req_of(messages: Vec<ChatMsg>) -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages,
            tools: vec![ToolSpec {
                name: "bash".into(),
                description: "d".into(),
                schema: json!({"type": "object"}),
            }],
            effort: None,
            cache: true,
            limits: None,
            no_tool_calls: false,
            max_tokens: None,
        }
    }

    fn signed(text: &str, sig: &str) -> ThinkingBlock {
        ThinkingBlock::Signed {
            text: text.into(),
            signature: sig.into(),
        }
    }

    /// A step that called `bash` as `id`, with `thinking`, then its result.
    fn step(id: &str, thinking: Option<Thinking>) -> [ChatMsg; 2] {
        [
            ChatMsg::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCallRef {
                    call_id: id.into(),
                    tool: "bash".into(),
                    input: json!({"command": "ls"}),
                }],
                thinking: thinking.map(Arc::new),
            },
            ChatMsg::Tool {
                call_id: id.into(),
                content: "out".into(),
            },
        ]
    }

    fn kinds(m: &ChatMessage) -> Vec<String> {
        m.content
            .parts()
            .iter()
            .map(|p| match p {
                ContentPart::ThoughtSignature(s) => format!("sig:{s}"),
                ContentPart::ReasoningContent(t) => format!("text:{t}"),
                ContentPart::Custom(c) => format!("custom:{}", c.data),
                ContentPart::ToolCall(c) => format!("call:{}", c.call_id),
                ContentPart::Text(t) => format!("said:{t}"),
                other => format!("{other:?}"),
            })
            .collect()
    }

    /// X.25, Anthropic: a step's blocks go back exactly as they came, in
    /// their order (a redacted one in its place), in front of its tool
    /// call, while everything before the step is what the request that
    /// made them sent; a later step's too, its prefix holding the first
    /// step's blocks. A block whose prefix changed is not sent, and then
    /// neither is any block after it (never one between two that are).
    #[test]
    fn anthropic_sends_signed_blocks_back_while_their_prefix_holds() {
        let base = vec![ChatMsg::System("s".into()), user("look")];
        let p1 = request_prefix(&req_of(base.clone()));
        let t1 = Thinking {
            model: "m".into(),
            blocks: vec![
                signed("first", "S1"),
                ThinkingBlock::Redacted { data: "R".into() },
                signed("", "S2"),
            ],
            prefix: p1,
            unsent: false,
        };
        let mut two = base.clone();
        two.extend(step("c1", Some(t1.clone())));
        let c = convert(&req_of(two.clone()), Some(ApiKind::Anthropic));
        assert!(c.replayed);
        assert_eq!(
            kinds(&c.request.messages[2]),
            vec![
                "sig:S1",
                "text:first",
                r#"custom:{"type":"redacted_thinking","data":"R"}"#,
                "sig:S2",
                "text:",
                "call:c1"
            ]
        );
        let t2 = Thinking {
            model: "m".into(),
            blocks: vec![signed("second", "S3")],
            prefix: c.prefix,
            unsent: false,
        };
        let mut three = two.clone();
        three.extend(step("c2", Some(t2.clone())));
        three.push(user("next"));
        let c = convert(&req_of(three.clone()), Some(ApiKind::Anthropic));
        assert_eq!(kinds(&c.request.messages[2])[0], "sig:S1");
        assert_eq!(
            kinds(&c.request.messages[4])[..2],
            ["sig:S3", "text:second"]
        );
        // The first result changed (cleared): the first step's blocks
        // stand before it and stay; the second's prefix changed.
        let mut cleared = three.clone();
        cleared[3] = ChatMsg::Tool {
            call_id: "c1".into(),
            content: "[cleared]".into(),
        };
        let c = convert(&req_of(cleared), Some(ApiKind::Anthropic));
        assert_eq!(kinds(&c.request.messages[2])[0], "sig:S1");
        assert_eq!(kinds(&c.request.messages[4]), vec!["call:c2"]);
        // The person's first message changed: no block is sent at all,
        // the second step's not either (its prefix held the first's).
        let mut changed = three.clone();
        changed[1] = user("look again");
        let c = convert(&req_of(changed), Some(ApiKind::Anthropic));
        assert!(!c.replayed);
        assert_eq!(kinds(&c.request.messages[2]), vec!["call:c1"]);
        assert_eq!(kinds(&c.request.messages[4]), vec!["call:c2"]);
        // The tools changed: none either.
        let mut r = req_of(three.clone());
        r.tools[0].description = "other".into();
        assert!(!convert(&r, Some(ApiKind::Anthropic)).replayed);
        // A cache marker that moved is not a change.
        let mut r = req_of(three);
        r.cache = false;
        assert!(convert(&r, Some(ApiKind::Anthropic)).replayed);
    }

    /// Review of PR 137: a step's thinking left out later (in-turn
    /// shortening) starts no gap: a block made after it was left out is
    /// valid and sent; one recorded `unsent` (never sent) starts one.
    #[test]
    fn only_a_never_sent_step_starts_a_gap() {
        let base = vec![ChatMsg::System("s".into()), user("look")];
        let t1 = Thinking {
            model: "m".into(),
            blocks: vec![signed("first", "S1")],
            prefix: request_prefix(&req_of(base.clone())),
            unsent: false,
        };
        let dropped = Thinking {
            model: "m".into(),
            blocks: vec![],
            prefix: String::new(),
            unsent: false,
        };
        let mut two = base.clone();
        two.extend(step("c1", Some(dropped)));
        let t2 = Thinking {
            model: "m".into(),
            blocks: vec![signed("second", "S2")],
            prefix: request_prefix(&req_of(two.clone())),
            unsent: false,
        };
        let mut three = two.clone();
        three.extend(step("c2", Some(t2.clone())));
        three.push(user("next"));
        let c = convert(&req_of(three), Some(ApiKind::Anthropic));
        assert_eq!(kinds(&c.request.messages[2]), vec!["call:c1"]);
        assert_eq!(
            kinds(&c.request.messages[4])[..2],
            ["sig:S2", "text:second"]
        );
        // The same with the first step never sent: no block after it.
        let never = Thinking { unsent: true, ..t1 };
        let mut two = base.clone();
        two.extend(step("c1", Some(never)));
        let t2 = Thinking {
            prefix: request_prefix(&req_of(two.clone())),
            ..t2
        };
        let mut three = two;
        three.extend(step("c2", Some(t2)));
        let c = convert(&req_of(three), Some(ApiKind::Anthropic));
        assert!(!c.replayed, "a block after a never-sent step was sent");
    }

    /// X.25: an `unsent` step's thinking never goes; plain reasoning goes
    /// to an OpenAI-compatible API as `reasoning_content` whatever changed
    /// before it, and never to Anthropic; signed blocks never to OpenAI.
    #[test]
    fn each_api_gets_only_its_own_kind_of_thinking() {
        let base = vec![ChatMsg::System("s".into()), user("look")];
        let p = request_prefix(&req_of(base.clone()));
        let with = |t: Thinking| {
            let mut m = base.clone();
            m.extend(step("c1", Some(t)));
            req_of(m)
        };
        let mut t = Thinking {
            model: "m".into(),
            blocks: vec![signed("a", "S")],
            prefix: p,
            unsent: true,
        };
        assert!(!convert(&with(t.clone()), Some(ApiKind::Anthropic)).replayed);
        t.unsent = false;
        assert!(convert(&with(t.clone()), Some(ApiKind::Anthropic)).replayed);
        assert!(!convert(&with(t.clone()), Some(ApiKind::Openai)).replayed);
        assert!(!convert(&with(t), None).replayed, "to_genai sends none");
        let plain = Thinking {
            model: "m".into(),
            blocks: vec![ThinkingBlock::Text { text: "why".into() }],
            prefix: String::new(),
            unsent: false,
        };
        let c = convert(&with(plain.clone()), Some(ApiKind::Openai));
        assert!(c.replayed);
        assert_eq!(kinds(&c.request.messages[2]), vec!["text:why", "call:c1"]);
        assert!(!convert(&with(plain), Some(ApiKind::Anthropic)).replayed);
    }

    /// A request without thinking converts exactly as before the thinking
    /// rule existed, for either API (the wire form is pinned in
    /// `tests/thinking_http.rs`).
    #[test]
    fn a_request_without_thinking_is_unchanged() {
        let mut m = vec![ChatMsg::System("s".into()), user("look")];
        m.extend(step("c1", None));
        m.push(said("done"));
        let r = req_of(m);
        let plain = serde_json::to_string(&to_genai(&r)).unwrap();
        for k in [ApiKind::Anthropic, ApiKind::Openai] {
            let c = convert(&r, Some(k));
            assert!(!c.replayed);
            assert_eq!(serde_json::to_string(&c.request).unwrap(), plain);
        }
    }

    /// genai sends a message's thinking blocks first: an answer with one
    /// after its text or a tool call cannot be sent back as it came.
    #[test]
    fn thinking_after_text_or_a_call_is_out_of_order() {
        let o = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(leads(&o(&[])));
        assert!(leads(&o(&[
            "thinking",
            "redacted_thinking",
            "text",
            "tool_use"
        ])));
        assert!(leads(&o(&["text", "tool_use", "tool_use"])));
        assert!(!leads(&o(&[
            "thinking", "tool_use", "thinking", "tool_use"
        ])));
        assert!(!leads(&o(&["text", "thinking"])));
    }

    /// Answers with the thinking of the request it got (as a trace), and
    /// thinking of its own under the raw model id.
    struct Echo(Arc<std::sync::Mutex<Vec<Vec<String>>>>);

    #[async_trait::async_trait]
    impl Provider for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }
        async fn complete(
            &self,
            req: &ChatRequest,
            _deltas: mpsc::UnboundedSender<String>,
        ) -> Result<Completion, ProviderError> {
            self.0.lock().unwrap().push(
                req.messages
                    .iter()
                    .filter_map(|m| match m {
                        ChatMsg::Assistant {
                            thinking: Some(t), ..
                        } => Some(t.model.clone()),
                        _ => None,
                    })
                    .collect(),
            );
            Ok(Completion {
                thinking: Some(Thinking {
                    model: req.model.clone(),
                    blocks: vec![ThinkingBlock::Text { text: "t".into() }],
                    prefix: String::new(),
                    unsent: false,
                }),
                ..Default::default()
            })
        }
        fn replays_thinking(&self, req: &ChatRequest) -> bool {
            req.messages.iter().any(|m| {
                matches!(
                    m,
                    ChatMsg::Assistant {
                        thinking: Some(_),
                        ..
                    }
                )
            })
        }
    }

    /// X.25, T73: thinking from another model, the same provider's or
    /// another's, never reaches a request; the answer's thinking carries
    /// the prefixed model.
    #[tokio::test]
    async fn the_router_sends_only_the_models_own_thinking() {
        let seen: Arc<std::sync::Mutex<Vec<Vec<String>>>> = Default::default();
        let r = ModelRouter::new(vec![
            Route {
                prefix: "a".into(),
                provider: Arc::new(Echo(seen.clone())),
            },
            Route {
                prefix: "b".into(),
                provider: Arc::new(Echo(seen.clone())),
            },
        ]);
        let t = |model: &str| Thinking {
            model: model.into(),
            blocks: vec![ThinkingBlock::Text { text: "x".into() }],
            prefix: String::new(),
            unsent: false,
        };
        let mut m = vec![user("u")];
        m.extend(step("c1", Some(t("a::one"))));
        m.extend(step("c2", Some(t("a::two"))));
        m.extend(step("c3", Some(t("b::one"))));
        let mut req = req_of(m);
        req.model = "a::one".into();
        let (tx, _rx) = mpsc::unbounded_channel();
        let c = r.complete(&req, tx.clone()).await.unwrap();
        assert_eq!(c.thinking.unwrap().model, "a::one");
        assert!(r.replays_thinking(&req));
        req.model = "b::two".into();
        assert!(!r.replays_thinking(&req), "b::two wrote none of it");
        r.complete(&req, tx).await.unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            vec![vec!["a::one".to_string()], vec![]]
        );
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
                limits: None,
                no_tool_calls: false,
                max_tokens: None,
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

    /// Reports usage under whatever (wrong) model a provider might claim.
    struct Counting;

    #[async_trait::async_trait]
    impl Provider for Counting {
        fn name(&self) -> &'static str {
            "counting"
        }
        async fn complete(
            &self,
            _req: &ChatRequest,
            _deltas: mpsc::UnboundedSender<String>,
        ) -> Result<Completion, ProviderError> {
            Ok(Completion {
                usage: Some(Usage {
                    input_tokens: 1000,
                    output_tokens: 20,
                    cache_read_tokens: 700,
                    cache_write_tokens: 100,
                    model: Some("not-me".into()),
                    ..Default::default()
                }),
                ..Default::default()
            })
        }
    }

    /// Every call's usage leaves the router under the prefixed model it
    /// was asked for, so a turn's sums can be priced per model.
    #[tokio::test]
    async fn router_stamps_each_calls_usage_with_its_model() {
        let r = ModelRouter::new(vec![
            Route {
                prefix: "a".into(),
                provider: std::sync::Arc::new(Counting),
            },
            Route {
                prefix: "b".into(),
                provider: std::sync::Arc::new(Named("b")),
            },
        ]);
        let req = |model: &str| ChatRequest {
            model: model.into(),
            messages: vec![],
            tools: vec![],
            effort: None,
            cache: false,
            limits: None,
            no_tool_calls: false,
            max_tokens: None,
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let u = r
            .complete(&req("a::m-1"), tx.clone())
            .await
            .unwrap()
            .usage
            .unwrap();
        let by: Vec<_> = u.by_model.iter().collect();
        assert_eq!(by.len(), 1, "{by:?}");
        assert_eq!(by[0].0, "a::m-1");
        assert_eq!(
            (
                by[0].1.input_tokens,
                by[0].1.output_tokens,
                by[0].1.cache_read_tokens,
                by[0].1.cache_write_tokens
            ),
            (1000, 20, 700, 100)
        );
        // No usage reported: nothing to stamp, none made up.
        let c = r.complete(&req("b::x"), tx).await.unwrap();
        assert!(c.usage.is_none());
    }
}
