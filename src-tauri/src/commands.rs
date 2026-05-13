use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::client::ServerClient;
use crate::config::AppConfig;
use crate::AppState;

fn client_with_token(token: Option<String>) -> Result<ServerClient, String> {
    let cfg = AppConfig::load().map_err(|e| e.to_string())?;
    let url = cfg
        .server_url
        .unwrap_or_else(|| "http://127.0.0.1:8788".into());
    Ok(ServerClient::new(&url, token))
}

fn authed_client() -> Result<ServerClient, String> {
    let cfg = AppConfig::load().map_err(|e| e.to_string())?;
    let token = cfg.auth_token.clone();
    if token.is_none() {
        return Err("未登录".into());
    }
    let url = cfg
        .server_url
        .unwrap_or_else(|| "http://127.0.0.1:8788".into());
    Ok(ServerClient::new(&url, token))
}

fn anon_client() -> Result<ServerClient, String> {
    client_with_token(None)
}

// ---------- public / login ----------

#[tauri::command]
pub async fn ping_server() -> Result<String, String> {
    anon_client()?.ping().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn login(username: String, password: String) -> Result<serde_json::Value, String> {
    let client = anon_client()?;
    let resp = client
        .login(&username, &password)
        .await
        .map_err(|e| e.to_string())?;
    let token = resp
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "服务端未返回 token".to_string())?
        .to_string();
    let mut cfg = AppConfig::load().map_err(|e| e.to_string())?;
    cfg.auth_token = Some(token);
    cfg.save().map_err(|e| e.to_string())?;
    Ok(resp)
}

#[tauri::command]
pub async fn logout() -> Result<bool, String> {
    let client = authed_client()?;
    let _ = client.logout().await; // best-effort
    let mut cfg = AppConfig::load().map_err(|e| e.to_string())?;
    cfg.auth_token = None;
    cfg.active_conversation_id = None;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn me() -> Result<serde_json::Value, String> {
    authed_client()?.me().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn change_password(
    old_password: String,
    new_password: String,
) -> Result<bool, String> {
    authed_client()?
        .change_password(&old_password, &new_password)
        .await
        .map_err(|e| e.to_string())
}

// ---------- providers / conversations (authed; workspace lives server-side) ----------

#[tauri::command]
pub async fn list_providers() -> Result<Vec<serde_json::Value>, String> {
    authed_client()?
        .list_providers()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn detect_provider(provider_id: String) -> Result<bool, String> {
    authed_client()?
        .detect(&provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_conversations() -> Result<Vec<serde_json::Value>, String> {
    authed_client()?
        .list_conversations()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_conversation(conversation_id: String) -> Result<bool, String> {
    authed_client()?
        .delete_conversation(&conversation_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_conversation_history(
    conversation_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    authed_client()?
        .get_history(&conversation_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_conversation(
    provider_id: String,
    name: Option<String>,
) -> Result<serde_json::Value, String> {
    authed_client()?
        .create_conversation(&provider_id, name.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageArgs {
    pub provider_id: String,
    pub prompt: String,
    pub conversation_id: String,
    #[serde(default)]
    pub provider_config: serde_json::Value,
}

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    args: SendMessageArgs,
) -> Result<String, String> {
    let client = authed_client()?;

    let local_id = Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    state.sessions.insert(local_id.clone(), cancel.clone());

    let body = serde_json::json!({
        "provider_id": args.provider_id,
        "prompt": args.prompt,
        "conversation_id": args.conversation_id,
        "provider_config": args.provider_config,
    });

    let sessions = state.sessions.clone();
    let app_clone = app.clone();
    let local_id_for_task = local_id.clone();
    tokio::spawn(async move {
        if let Err(e) = client.chat_stream(app_clone.clone(), body, cancel).await {
            emit_error(&app_clone, &format!("chat_stream error: {e}"));
        }
        sessions.remove(&local_id_for_task);
    });

    Ok(local_id)
}

fn emit_error(app: &AppHandle, msg: &str) {
    use tauri::Emitter;
    let _ = app.emit(
        "agent:event",
        serde_json::json!({
            "type": "error",
            "session_id": "",
            "message": msg,
        }),
    );
}

#[tauri::command]
pub async fn cancel_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<bool, String> {
    if let Some((_, token)) = state.sessions.remove(&session_id) {
        token.cancel();
        Ok(true)
    } else {
        authed_client()?
            .cancel(&session_id)
            .await
            .map_err(|e| e.to_string())
    }
}

// ---------- admin ----------

#[tauri::command]
pub async fn admin_list_users() -> Result<Vec<serde_json::Value>, String> {
    authed_client()?
        .admin_list_users()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn admin_create_user(
    username: String,
    password: String,
    workspace_root: Option<String>,
) -> Result<serde_json::Value, String> {
    authed_client()?
        .admin_create_user(&username, &password, workspace_root.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn admin_delete_user(username: String) -> Result<bool, String> {
    authed_client()?
        .admin_delete_user(&username)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn admin_set_password(username: String, password: String) -> Result<bool, String> {
    authed_client()?
        .admin_set_password(&username, &password)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn admin_set_workspace(
    username: String,
    workspace_root: Option<String>,
) -> Result<serde_json::Value, String> {
    authed_client()?
        .admin_set_workspace(&username, workspace_root.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn admin_check_workspace(workspace_root: String) -> Result<serde_json::Value, String> {
    authed_client()?
        .admin_check_workspace(&workspace_root)
        .await
        .map_err(|e| e.to_string())
}

// ---------- config ----------

#[tauri::command]
pub async fn load_config() -> Result<AppConfig, String> {
    AppConfig::load().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(config: AppConfig) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())
}
