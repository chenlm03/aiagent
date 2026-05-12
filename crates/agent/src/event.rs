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
    Text {
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
