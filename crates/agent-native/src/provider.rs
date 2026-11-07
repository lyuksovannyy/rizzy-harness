//! Provider abstraction and a provider-neutral chat model.

use harness_core::event::{ToolCallRef, Usage};
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq)]
pub enum ChatMsg {
    System(String),
    User(String),
    Assistant {
        text: String,
        tool_calls: Vec<ToolCallRef>,
    },
    Tool {
        call_id: String,
        content: String,
    },
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

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("http: {0}")]
    Http(String),
    #[error("provider returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("bad stream: {0}")]
    Stream(String),
    #[error("no provider serves model {0:?}")]
    UnknownModel(String),
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

/// Rough token count (4 characters per token). Used to decide when to
/// summarise and by the mock provider; real usage comes from the provider.
pub fn estimate_tokens(messages: &[ChatMsg]) -> u64 {
    let chars: usize = messages
        .iter()
        .map(|m| match m {
            ChatMsg::System(t) | ChatMsg::User(t) => t.len(),
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
