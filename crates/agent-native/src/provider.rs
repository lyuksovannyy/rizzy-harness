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

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMsg>,
    pub tools: Vec<ToolSpec>,
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
