use serde::{Deserialize, Serialize};

/// Internal, provider-agnostic event format. Every provider's raw output
/// must be translated into one of these variants. The frontend only ever
/// sees AgentEvent — it never has to know whether it came from Claude Code,
/// Codex, or a direct API call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Started {
        session_id: String,
    },
    /// Provider's own session identifier (e.g. Claude Code's session uuid).
    /// Server uses this to persist on the conversation so the next turn can
    /// pass --resume.
    ProviderSessionId {
        session_id: String,
        provider_session_id: String,
    },
    Text {
        session_id: String,
        delta: String,
    },
    /// Extended-thinking content. Displayed as a transient strip in the
    /// client and NOT persisted to conversation history.
    Thinking {
        session_id: String,
        delta: String,
    },
    ToolCall {
        session_id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        session_id: String,
        name: String,
        output: String,
    },
    Error {
        session_id: String,
        message: String,
    },
    Finished {
        session_id: String,
        reason: String,
    },
}
