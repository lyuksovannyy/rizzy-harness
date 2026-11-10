//! Provider abstraction and a provider-neutral chat model.

use harness_core::event::{ToolCallRef, Usage};
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq)]
pub enum ChatMsg {
    System(String),
    User(String),
    /// A user message with images. Only the newest few carry their bytes
    /// (`IMAGE_TURNS`); older ones, and summaries, name them in the text.
    UserImages {
        text: String,
        images: Vec<ImagePart>,
    },
    Assistant {
        text: String,
        tool_calls: Vec<ToolCallRef>,
    },
    Tool {
        call_id: String,
        content: String,
    },
}

/// One image of a user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePart {
    pub sha256: String,
    pub mime: String,
    pub name: String,
    /// Base64; `None` until the host gave the bytes (`attach_images`).
    pub data: Option<std::sync::Arc<str>>,
}

impl ChatMsg {
    /// What the user wrote, for user messages with or without images.
    pub fn user_text(&self) -> Option<&str> {
        match self {
            ChatMsg::User(t) | ChatMsg::UserImages { text: t, .. } => Some(t),
            _ => None,
        }
    }
}

/// How an image the model does not get is named in the text.
pub fn image_note(name: &str) -> String {
    format!("[image: {}]", name.replace(['[', ']', '\n'], " "))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// Reasoning effort, mapped per provider (OpenAI `reasoning_effort`,
/// Anthropic thinking budget). Only sent when the model supports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    Medium,
    High,
    Max,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMsg>,
    pub tools: Vec<ToolSpec>,
    pub effort: Option<Effort>,
    /// Mark the stable prefix (system prompt + tools) and the newest message
    /// as cache breakpoints, where the provider needs explicit markers.
    pub cache: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolCallRef>,
    pub usage: Option<Usage>,
}

/// What a failed response said about trying again (its headers).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetryHint {
    /// `retry-after-ms` or `retry-after` (seconds; an HTTP date is ignored).
    pub after: Option<std::time::Duration>,
    /// `x-should-retry: false` (Anthropic, OpenAI): the provider says a
    /// retry will fail too.
    pub no_retry: bool,
}

impl RetryHint {
    /// The hint in a response's headers.
    pub fn from_headers(h: &reqwest::header::HeaderMap) -> Self {
        let get = |name: &str| h.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
        let secs = |s: &str| {
            s.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(|v| std::time::Duration::from_secs_f64(v.min(86_400.0)))
        };
        let after = get("retry-after-ms")
            .and_then(secs)
            .map(|d| d / 1000)
            .or_else(|| get("retry-after").and_then(secs));
        Self {
            after,
            no_retry: get("x-should-retry").is_some_and(|v| v.eq_ignore_ascii_case("false")),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("http: {0}")]
    Http(String),
    #[error("provider returned {status}: {body}")]
    Status {
        status: u16,
        body: String,
        hint: RetryHint,
    },
    #[error("bad stream: {0}")]
    Stream(String),
    #[error("no provider serves model {0:?}")]
    UnknownModel(String),
}

impl ProviderError {
    /// A failure of the model or its provider rather than of this request's
    /// content: the model is unknown or gone, rate-limited, the provider
    /// fails or cannot be reached. Another model may answer instead.
    pub fn is_model_level(&self) -> bool {
        match self {
            ProviderError::UnknownModel(_) | ProviderError::Http(_) => true,
            ProviderError::Status { status, body, .. } => match status {
                404 | 408 | 429 | 500..=599 => true,
                400 => body.to_ascii_lowercase().contains("model"),
                _ => false,
            },
            ProviderError::Stream(_) => false,
        }
    }

    /// Whether the same request may succeed if sent again after a wait: a
    /// rate limit (not an exhausted quota or billing limit), an overloaded
    /// or failing server, a connection that failed or a stream that broke.
    /// Never authentication, permission or a bad request. The caller makes
    /// sure no output of the failed attempt reached the user.
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::Http(_) | ProviderError::Stream(_) => true,
            ProviderError::UnknownModel(_) => false,
            ProviderError::Status { status, body, hint } => {
                !hint.no_retry
                    && match status {
                        429 => !quota_exhausted(body),
                        500 | 502 | 503 | 504 | 529 => true,
                        _ => false,
                    }
            }
        }
    }

    /// How long the provider asked to wait before trying again.
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            ProviderError::Status { hint, .. } => hint.after,
            _ => None,
        }
    }

    /// The error for a log line: the status or the kind of failure, never
    /// the provider's response body or a URL (either may carry a key or
    /// the user's text).
    pub fn log_line(&self) -> String {
        match self {
            ProviderError::Http(_) => "connection failed".into(),
            ProviderError::Stream(_) => "stream failed".into(),
            ProviderError::Status { status, .. } => format!("status {status}"),
            ProviderError::UnknownModel(_) => "unknown model".into(),
        }
    }
}

/// The `error.code` / `error.type` of a JSON error body, lowercased
/// (OpenAI and compatible servers: `{"error": {"code", "type"}}`;
/// Anthropic: `{"type": "error", "error": {"type"}}`).
fn error_codes(body: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return vec![];
    };
    ["/error/code", "/error/type", "/code"]
        .iter()
        .filter_map(|p| v.pointer(p).and_then(Value::as_str))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// A 429 that waiting a minute will not fix: the account's quota or
/// credit is used up (OpenAI `insufficient_quota`, billing errors).
fn quota_exhausted(body: &str) -> bool {
    error_codes(body).iter().any(|c| {
        matches!(
            c.as_str(),
            "insufficient_quota" | "billing_error" | "billing_hard_limit_reached"
        )
    })
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    /// Stream one completion. Text deltas go to `deltas` as they arrive; the
    /// return value is the complete message. Dropping the future aborts it.
    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError>;
}

/// What one image counts for in `estimate_tokens`, in characters (about a
/// thousand tokens, near what providers bill for a downsized photo).
const IMAGE_CHARS: usize = 4000;

/// Rough token count (4 characters per token). Used to decide when to
/// summarise and by the mock provider; real usage comes from the provider.
pub fn estimate_tokens(messages: &[ChatMsg]) -> u64 {
    let chars: usize = messages
        .iter()
        .map(|m| match m {
            ChatMsg::System(t) | ChatMsg::User(t) => t.len(),
            ChatMsg::UserImages { text, images } => text.len() + images.len() * IMAGE_CHARS,
            ChatMsg::Assistant { text, tool_calls } => {
                text.len()
                    + tool_calls
                        .iter()
                        .map(|c| c.input.to_string().len() + c.tool.len())
                        .sum::<usize>()
            }
            ChatMsg::Tool { content, .. } => content.len(),
        })
        .sum();
    (chars / 4) as u64 + 1
}
