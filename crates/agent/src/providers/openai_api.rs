use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::provider::{AgentProvider, ProviderInfo, ProviderKind, SendOptions};
use crate::AgentEvent;

pub struct OpenAiApi;

impl OpenAiApi {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentProvider for OpenAiApi {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "openai-api".into(),
            name: "OpenAI API (direct)".into(),
            kind: ProviderKind::Api,
            description:
                "Calls the OpenAI Responses / Chat Completions API directly. Requires OPENAI_API_KEY."
                    .into(),
        }
    }

    async fn detect(&self) -> bool {
        std::env::var("OPENAI_API_KEY").is_ok()
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

        // TODO: implement OpenAI streaming.
        //   1. POST https://api.openai.com/v1/responses with stream=true
        //   2. Parse SSE; for each `response.output_text.delta` emit AgentEvent::Text { delta }
        //   3. For tool calls emit AgentEvent::ToolCall and feed back results in subsequent turns
        //   4. Honor `cancel` by aborting the HTTP request
        let _ = tx
            .send(AgentEvent::Error {
                session_id: session_id.clone(),
                message: "openai-api provider not implemented yet".into(),
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
