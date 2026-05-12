use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::AgentEvent;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Wraps an external CLI binary (claude / codex / etc.) via subprocess.
    Subprocess,
    /// Talks to an HTTP API directly.
    Api,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendOptions {
    pub session_id: String,
    pub prompt: String,
    pub working_dir: Option<String>,
    /// Provider-specific JSON config (model name, API key override,
    /// extra CLI args, etc.). Each provider documents its own schema.
    #[serde(default)]
    pub provider_config: serde_json::Value,
}

#[async_trait]
pub trait AgentProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;

    /// Returns true if this provider is usable on the current machine
    /// (CLI on PATH, API key present, etc.).
    async fn detect(&self) -> bool;

    /// Run one turn. Emit events through `tx`; respect `cancel` by
    /// stopping the underlying work promptly (kill child, abort request).
    async fn run(
        &self,
        opts: SendOptions,
        tx: mpsc::Sender<AgentEvent>,
        cancel: CancellationToken,
    ) -> anyhow::Result<()>;
}
