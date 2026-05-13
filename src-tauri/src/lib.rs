mod client;
mod commands;
mod config;

use std::sync::Arc;

use dashmap::DashMap;
use tokio_util::sync::CancellationToken;

pub struct AppState {
    /// Tracks in-flight session cancellation tokens (so the UI can stop a stream).
    pub sessions: Arc<DashMap<String, CancellationToken>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState {
        sessions: Arc::new(DashMap::new()),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::detect_provider,
            commands::send_message,
            commands::cancel_session,
            commands::load_config,
            commands::save_config,
            commands::ping_server,
            commands::list_conversations,
            commands::create_conversation,
            commands::delete_conversation,
            commands::get_conversation_history,
            commands::login,
            commands::logout,
            commands::me,
            commands::change_password,
            commands::admin_list_users,
            commands::admin_create_user,
            commands::admin_delete_user,
            commands::admin_set_password,
            commands::admin_set_workspace,
            commands::admin_check_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
