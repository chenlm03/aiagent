use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::provider::{AgentProvider, ProviderInfo, ProviderKind, SendOptions};
use crate::AgentEvent;

pub struct AnthropicApi;

impl AnthropicApi {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentProvider for AnthropicApi {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "anthropic-api".into(),
            name: "Anthropic API (direct)".into(),
            kind: ProviderKind::Api,
            description:
                "Calls https://api.anthropic.com/v1/messages directly. Requires ANTHROPIC_API_KEY."
                    .into(),
        }
    }

    async fn detect(&self) -> bool {
        std::env::var("ANTHROPIC_API_KEY").is_ok()
            || std::env::var("CLAUDE_API_KEY").is_ok()
    }

    async fn run(
        &self,
        opts: SendOptions,
        tx: mpsc::Sender<AgentEvent>,
        _cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        let session_id = opts.session_id.clone();
        let _ = tx
            .send(AgentEvent::Started {
                session_id: session_id.clone(),
            })
            .await;

        // TODO: implement Messages API streaming.
        //   1. POST https://api.anthropic.com/v1/messages with stream=true
        //   2. Parse SSE; for each `content_block_delta` emit AgentEvent::Text { delta }
        //   3. For tool_use blocks emit AgentEvent::ToolCall
        //   4. Honor `cancel` by aborting the HTTP request
        let _ = tx
            .send(AgentEvent::Error {
                session_id: session_id.clone(),
                message: "anthropic-api provider not implemented yet".into(),
            })
            .await;
        let _ = tx
            .send(AgentEvent::Finished {
                session_id,
                reason: "stub".into(),
            })
            .await;
        Ok(())
    }
}
