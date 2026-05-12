use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::client::ServerClient;
use crate::config::AppConfig;
use crate::AppState;

fn client_from_config() -> Result<ServerClient, String> {
    let cfg = AppConfig::load().map_err(|e| e.to_string())?;
    let url = cfg
        .server_url
        .unwrap_or_else(|| "http://127.0.0.1:8788".into());
    Ok(ServerClient::new(&url))
}

#[tauri::command]
pub async fn ping_server() -> Result<String, String> {
    client_from_config()?
        .ping()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_providers() -> Result<Vec<serde_json::Value>, String> {
    client_from_config()?
        .list_providers()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn detect_provider(provider_id: String) -> Result<bool, String> {
    client_from_config()?
        .detect(&provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_workspace(workspace_root: String) -> Result<serde_json::Value, String> {
    client_from_config()?
        .check_workspace(&workspace_root)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_conversations(
    workspace_root: String,
) -> Result<Vec<serde_json::Value>, String> {
    client_from_config()?
        .list_conversations(&workspace_root)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_conversation_history(
    conversation_id: String,
    workspace_root: String,
) -> Result<Vec<serde_json::Value>, String> {
    client_from_config()?
        .get_history(&conversation_id, &workspace_root)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_conversation(
    workspace_root: String,
    provider_id: String,
    name: Option<String>,
) -> Result<serde_json::Value, String> {
    client_from_config()?
        .create_conversation(&workspace_root, &provider_id, name.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageArgs {
    pub provider_id: String,
    pub prompt: String,
    pub conversation_id: String,
    pub workspace_root: String,
    #[serde(default)]
    pub provider_config: serde_json::Value,
}

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    args: SendMessageArgs,
) -> Result<String, String> {
    let client = client_from_config()?;

    let local_id = Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    state.sessions.insert(local_id.clone(), cancel.clone());

    let body = serde_json::json!({
        "provider_id": args.provider_id,
        "prompt": args.prompt,
        "conversation_id": args.conversation_id,
        "workspace_root": args.workspace_root,
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
        let client = client_from_config()?;
        client.cancel(&session_id).await.map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn load_config() -> Result<AppConfig, String> {
    AppConfig::load().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(config: AppConfig) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())
}
