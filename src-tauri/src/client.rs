use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
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
    token: Option<String>,
}

impl ServerClient {
    pub fn new(base_url: &str, token: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest client");
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            http,
            token,
        }
    }

    fn auth_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(t) = &self.token {
            if let Ok(v) = HeaderValue::from_str(&format!("Bearer {t}")) {
                h.insert(AUTHORIZATION, v);
            }
        }
        h
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let res = self
            .http
            .get(self.url(path))
            .headers(self.auth_headers())
            .send()
            .await?;
        let res = ensure_ok(res).await?;
        Ok(res.json().await?)
    }

    async fn post_json<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> anyhow::Result<T> {
        let res = self
            .http
            .post(self.url(path))
            .headers(self.auth_headers())
            .json(body)
            .send()
            .await?;
        let res = ensure_ok(res).await?;
        Ok(res.json().await?)
    }

    pub async fn ping(&self) -> anyhow::Result<String> {
        // Health check does NOT require auth; allow this to work pre-login too.
        let res = self.http.get(self.url("/api/health")).send().await?;
        Ok(res.text().await?)
    }

    pub async fn login(
        &self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let body = serde_json::json!({ "username": username, "password": password });
        let res = self
            .http
            .post(self.url("/api/login"))
            .json(&body)
            .send()
            .await?;
        let res = ensure_ok(res).await?;
        Ok(res.json().await?)
    }

    pub async fn logout(&self) -> anyhow::Result<bool> {
        self.post_json("/api/logout", &serde_json::json!({})).await
    }

    pub async fn me(&self) -> anyhow::Result<serde_json::Value> {
        self.get_json("/api/me").await
    }

    pub async fn change_password(
        &self,
        old_password: &str,
        new_password: &str,
    ) -> anyhow::Result<bool> {
        self.post_json(
            "/api/me/password",
            &serde_json::json!({
                "old_password": old_password,
                "new_password": new_password,
            }),
        )
        .await
    }

    pub async fn list_providers(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        self.get_json("/api/providers").await
    }

    pub async fn detect(&self, provider_id: &str) -> anyhow::Result<bool> {
        self.get_json(&format!("/api/providers/{provider_id}/detect")).await
    }

    pub async fn list_conversations(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        self.get_json("/api/conversations").await
    }

    pub async fn create_conversation(
        &self,
        provider_id: &str,
        name: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let mut body = serde_json::json!({ "provider_id": provider_id });
        if let Some(n) = name {
            body["name"] = serde_json::Value::String(n.into());
        }
        self.post_json("/api/conversations", &body).await
    }

    pub async fn get_history(
        &self,
        conversation_id: &str,
    ) -> anyhow::Result<Vec<serde_json::Value>> {
        self.get_json(&format!("/api/conversations/{conversation_id}/history"))
            .await
    }

    pub async fn delete_conversation(&self, conversation_id: &str) -> anyhow::Result<bool> {
        let res = self
            .http
            .delete(self.url(&format!("/api/conversations/{conversation_id}")))
            .headers(self.auth_headers())
            .send()
            .await?;
        let res = ensure_ok(res).await?;
        Ok(res.json().await?)
    }

    pub async fn cancel(&self, session_id: &str) -> anyhow::Result<bool> {
        self.post_json(&format!("/api/cancel/{session_id}"), &serde_json::json!({}))
            .await
    }

    pub async fn admin_list_users(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        self.get_json("/api/admin/users").await
    }

    pub async fn admin_create_user(
        &self,
        username: &str,
        password: &str,
        workspace_root: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let body = serde_json::json!({
            "username": username,
            "password": password,
            "workspace_root": workspace_root,
        });
        self.post_json("/api/admin/users", &body).await
    }

    pub async fn admin_delete_user(&self, username: &str) -> anyhow::Result<bool> {
        let res = self
            .http
            .delete(self.url(&format!("/api/admin/users/{username}")))
            .headers(self.auth_headers())
            .send()
            .await?;
        let res = ensure_ok(res).await?;
        Ok(res.json().await?)
    }

    pub async fn admin_set_password(
        &self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<bool> {
        self.post_json(
            &format!("/api/admin/users/{username}/password"),
            &serde_json::json!({ "password": password }),
        )
        .await
    }

    pub async fn admin_set_workspace(
        &self,
        username: &str,
        workspace_root: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        self.post_json(
            &format!("/api/admin/users/{username}/workspace"),
            &serde_json::json!({ "workspace_root": workspace_root }),
        )
        .await
    }

    pub async fn admin_check_workspace(
        &self,
        workspace_root: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let res = self
            .http
            .get(self.url("/api/admin/workspace/check"))
            .headers(self.auth_headers())
            .query(&[("workspace_root", workspace_root)])
            .send()
            .await?;
        let res = ensure_ok(res).await?;
        Ok(res.json().await?)
    }

    pub async fn chat_stream(
        &self,
        app: AppHandle,
        body: serde_json::Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<String> {
        let response = self
            .http
            .post(self.url("/api/chat"))
            .headers(self.auth_headers())
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

async fn ensure_ok(res: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    if res.status().is_success() {
        return Ok(res);
    }
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    if body.is_empty() {
        anyhow::bail!("HTTP {status}")
    } else {
        anyhow::bail!("{body}")
    }
}
