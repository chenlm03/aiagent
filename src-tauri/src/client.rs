use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEnvelope {
    pub session_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
}

pub struct ServerClient {
    base: String,
    http: reqwest::Client,
}

impl ServerClient {
    pub fn new(base_url: &str) -> Self {
        // No overall request timeout — chat_stream is long-lived. Short ops
        // are guarded by connect_timeout only.
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
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
        Ok(self
            .http
            .get(format!("{}/api/providers", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn detect(&self, provider_id: &str) -> anyhow::Result<bool> {
        Ok(self
            .http
            .get(format!("{}/api/providers/{}/detect", self.base, provider_id))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn check_workspace(&self, workspace_root: &str) -> anyhow::Result<serde_json::Value> {
        Ok(self
            .http
            .get(format!("{}/api/workspace/check", self.base))
            .query(&[("workspace_root", workspace_root)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn list_conversations(
        &self,
        workspace_root: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        Ok(self
            .http
            .get(format!("{}/api/conversations", self.base))
            .query(&[("workspace_root", workspace_root)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn create_conversation(
        &self,
        workspace_root: &str,
        provider_id: &str,
        name: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let mut body = serde_json::json!({
            "workspace_root": workspace_root,
            "provider_id": provider_id,
        });
        if let Some(n) = name {
            body["name"] = serde_json::Value::String(n.into());
        }
        Ok(self
            .http
            .post(format!("{}/api/conversations", self.base))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn get_history(
        &self,
        conversation_id: &str,
        workspace_root: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        Ok(self
            .http
            .get(format!(
                "{}/api/conversations/{}/history",
                self.base, conversation_id
            ))
            .query(&[("workspace_root", workspace_root)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn cancel(&self, session_id: &str) -> anyhow::Result<bool> {
        Ok(self
            .http
            .post(format!("{}/api/cancel/{}", self.base, session_id))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

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
        let mut server_session_id = String::new();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    if !server_session_id.is_empty() {
                        let _ = self.cancel(&server_session_id).await;
                    }
                    break;
                }
                evt = stream.next() => {
                    match evt {
                        Some(Ok(msg)) => {
                            match msg.event.as_str() {
                                "session" => {
                                    if let Ok(env) = serde_json::from_str::<AgentEnvelope>(&msg.data) {
                                        server_session_id = env.session_id;
                                    }
                                }
                                "agent" | "" => {
                                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                                        let _ = app.emit("agent:event", payload);
                                    }
                                }
                                _ => {}
                            }
                        }
                        Some(Err(e)) => {
                            let _ = app.emit("agent:event", serde_json::json!({
                                "type": "error",
                                "session_id": server_session_id,
                                "message": format!("stream error: {e}"),
                            }));
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        Ok(server_session_id)
    }
}
