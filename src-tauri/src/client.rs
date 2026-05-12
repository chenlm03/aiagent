use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

/// Same shape as `agent::AgentEvent` on the server. We deserialize into raw
/// JSON and forward to the frontend so the UI doesn't need a Rust dep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEnvelope {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub description: String,
}

pub struct ServerClient {
    base: String,
    http: reqwest::Client,
}

impl ServerClient {
    pub fn new(base_url: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(0)) // streaming: no overall timeout
            .build()
            .expect("reqwest client");
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    pub async fn ping(&self) -> anyhow::Result<String> {
        let res = self.http.get(format!("{}/api/health", self.base)).send().await?;
        Ok(res.text().await?)
    }

    pub async fn list_providers(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        let res = self
            .http
            .get(format!("{}/api/providers", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(res)
    }

    pub async fn detect(&self, provider_id: &str) -> anyhow::Result<bool> {
        let res = self
            .http
            .get(format!("{}/api/providers/{}/detect", self.base, provider_id))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(res)
    }

    pub async fn cancel(&self, session_id: &str) -> anyhow::Result<bool> {
        let res = self
            .http
            .post(format!("{}/api/cancel/{}", self.base, session_id))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(res)
    }

    /// Opens an SSE stream against POST /api/chat. The server first sends a
    /// `session` event with the session id, then a series of `agent` events
    /// each carrying one AgentEvent JSON payload. We re-emit each one to the
    /// frontend via the Tauri event bus.
    pub async fn chat_stream(
        &self,
        app: AppHandle,
        body: serde_json::Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<String> {
        let response = self
            .http
            .post(format!("{}/api/chat", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let mut stream = response.bytes_stream().eventsource();
        let mut session_id = String::new();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    if !session_id.is_empty() {
                        let _ = self.cancel(&session_id).await;
                    }
                    break;
                }
                evt = stream.next() => {
                    match evt {
                        Some(Ok(msg)) => {
                            match msg.event.as_str() {
                                "session" => {
                                    if let Ok(env) = serde_json::from_str::<AgentEnvelope>(&msg.data) {
                                        session_id = env.session_id;
                                    }
                                }
                                "agent" | "" => {
                                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                                        let _ = app.emit("agent:event", payload);
                                    }
                                }
                                _ => { /* ignore unknown event types */ }
                            }
                        }
                        Some(Err(e)) => {
                            let _ = app.emit("agent:event", serde_json::json!({
                                "type": "error",
                                "session_id": session_id,
                                "message": format!("stream error: {e}"),
                            }));
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        Ok(session_id)
    }
}
