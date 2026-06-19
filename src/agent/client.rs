//! The chat seam the agentic loop drives.
//!
//! A deliberately tiny tool-calling client trait, separate from the broader
//! `OllamaClient`. Keeping it here means the whole agentic feature is
//! self-contained in `src/agent/` — the live impl (on `LiveOllamaClient`) and a
//! scripted test mock both satisfy this one method, and none of the existing
//! `OllamaClient` impls have to change.

use async_trait::async_trait;
use serde::Serialize;

use super::AgentError;
use super::tools::{ToolCall, ToolDefinition};

/// One message in the agent conversation.
///
/// Serializes straight into the `messages` array of an Ollama `/api/chat`
/// request. `tool_calls` is only populated on the assistant turn we echo back;
/// it is omitted from the wire when empty.
#[derive(Debug, Clone, Serialize)]
pub struct AgentMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

impl AgentMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::plain("system", content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::plain("user", content)
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self::plain("tool", content)
    }

    pub fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls,
        }
    }

    fn plain(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            content: content.into(),
            tool_calls: Vec::new(),
        }
    }
}

/// The assistant's reply to one turn: free-text content and/or tool calls.
#[derive(Debug, Clone)]
pub struct AgentTurn {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

/// A chat client that can take tool definitions and return tool calls.
///
/// The agentic loop ([`super::run::run_agentic_review`]) drives this; the live
/// implementation is on `LiveOllamaClient` (src/runtime.rs), and tests use a
/// scripted mock.
#[async_trait]
pub trait AgentChatClient {
    /// Send one chat turn with the available tools; return what the model said
    /// and any tool calls it requested.
    ///
    /// `think` mirrors the review path: `Some(false)` disables a thinking
    /// model's reasoning pass (which otherwise consumes the turn and returns an
    /// empty message with no tool call), `None` omits the field for models that
    /// don't support thinking.
    async fn chat_turn(
        &self,
        model: &str,
        messages: &[AgentMessage],
        tools: &[ToolDefinition],
        think: Option<bool>,
    ) -> Result<AgentTurn, AgentError>;
}
