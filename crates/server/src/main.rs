mod auth;
mod conversation;

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent::{AgentEvent, ProviderInfo, Registry, SessionManager};
use auth::{AdminUser, AuthState, AuthedUser, TokenStore, UserStore, UserView};
use axum::{
    extract::{FromRef, Path, Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    routing::{delete, get, post},
    Json, Router,
};
use conversation::{Conversation, ConversationStore};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    registry: Arc<Registry>,
    sessions: Arc<SessionManager>,
    auth: AuthState,
}

impl FromRef<AppState> for AuthState {
    fn from_ref(s: &AppState) -> Self {
        s.auth.clone()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .init();

    let users = Arc::new(UserStore::new()?);
    let tokens = Arc::new(TokenStore::new(Duration::from_secs(7 * 24 * 3600)));
    let auth = AuthState { users, tokens };

    let state = AppState {
        registry: Arc::new(Registry::default_providers()),
        sessions: Arc::new(SessionManager::new()),
        auth,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // public
        .route("/api/health", get(health))
        .route("/api/login", post(login))
        // authed
        .route("/api/me", get(me))
        .route("/api/me/password", post(change_password))
        .route("/api/logout", post(logout))
        .route("/api/providers", get(list_providers))
        .route("/api/providers/:id/detect", get(detect_provider))
        .route("/api/conversations", get(list_conversations))
        .route("/api/conversations", post(create_conversation))
        .route("/api/conversations/:id/history", get(get_history))
        .route("/api/conversations/:id", delete(delete_conversation))
        .route("/api/chat", post(chat))
        .route("/api/cancel/:session_id", post(cancel))
        // admin
        .route("/api/admin/users", get(admin_list_users))
        .route("/api/admin/users", post(admin_create_user))
        .route("/api/admin/users/:username", delete(admin_delete_user))
        .route(
            "/api/admin/users/:username/password",
            post(admin_set_password),
        )
        .route(
            "/api/admin/users/:username/workspace",
            post(admin_set_workspace),
        )
        .route("/api/admin/workspace/check", get(admin_check_workspace))
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = std::env::var("AIAGENT_SERVER_ADDR").unwrap_or_else(|_| "0.0.0.0:8788".into());
    tracing::info!("listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

// ---------- auth ----------

#[derive(Debug, Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    token: String,
    user: UserView,
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<Json<LoginResponse>, (StatusCode, String)> {
    let user = state
        .auth
        .users
        .verify(&body.username, &body.password)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::UNAUTHORIZED, "用户名或密码错误".into()))?;
    let token = state.auth.tokens.mint(&user.username);
    Ok(Json(LoginResponse {
        token,
        user: (&user).into(),
    }))
}

async fn me(AuthedUser(user): AuthedUser) -> Json<UserView> {
    Json((&user).into())
}

fn bearer_from(headers: &HeaderMap) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    _user: AuthedUser,
) -> Json<bool> {
    if let Some(token) = bearer_from(&headers) {
        state.auth.tokens.revoke(&token);
    }
    Json(true)
}

#[derive(Debug, Deserialize)]
struct ChangePasswordBody {
    old_password: String,
    new_password: String,
}

async fn change_password(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<ChangePasswordBody>,
) -> Result<Json<bool>, (StatusCode, String)> {
    let _ = state
        .auth
        .users
        .verify(&user.username, &body.old_password)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::UNAUTHORIZED, "原密码不正确".into()))?;
    state
        .auth
        .users
        .set_password(&user.username, &body.new_password)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    // Don't revoke own tokens — user stays logged in after self password change.
    Ok(Json(true))
}

// ---------- providers (authed) ----------

async fn list_providers(
    State(state): State<AppState>,
    _user: AuthedUser,
) -> Json<Vec<ProviderInfo>> {
    Json(state.registry.list())
}

async fn detect_provider(
    State(state): State<AppState>,
    _user: AuthedUser,
    Path(id): Path<String>,
) -> Result<Json<bool>, (StatusCode, String)> {
    let p = state
        .registry
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, format!("未知 provider: {id}")))?;
    Ok(Json(p.detect().await))
}

// ---------- workspace + conversations (authed; workspace comes from session) ----------

/// Take the user's assigned workspace_root or 412 if none.
fn require_workspace(user: &auth::User) -> Result<PathBuf, (StatusCode, String)> {
    user.workspace_root
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .ok_or((
            StatusCode::PRECONDITION_FAILED,
            "尚未分配工作区，请联系管理员".into(),
        ))
}

async fn list_conversations(
    AuthedUser(user): AuthedUser,
) -> Result<Json<Vec<Conversation>>, (StatusCode, String)> {
    let root = require_workspace(&user)?;
    ConversationStore::list(&root)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[derive(Debug, Deserialize)]
struct CreateConvBody {
    provider_id: String,
    name: Option<String>,
}

async fn create_conversation(
    AuthedUser(user): AuthedUser,
    Json(body): Json<CreateConvBody>,
) -> Result<Json<Conversation>, (StatusCode, String)> {
    let root = require_workspace(&user)?;
    ConversationStore::create(&root, &body.provider_id, body.name)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn get_history(
    AuthedUser(user): AuthedUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let root = require_workspace(&user)?;
    ConversationStore::read_history(&root, &id)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn delete_conversation(
    AuthedUser(user): AuthedUser,
    Path(id): Path<String>,
) -> Result<Json<bool>, (StatusCode, String)> {
    let root = require_workspace(&user)?;
    ConversationStore::delete(&root, &id)
        .map(|()| Json(true))
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    provider_id: String,
    prompt: String,
    conversation_id: String,
    #[serde(default)]
    provider_config: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ChatSessionEnvelope {
    session_id: String,
    conversation_id: String,
}

async fn chat(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let workspace_root = require_workspace(&user)?;
    let provider = state.registry.get(&req.provider_id).ok_or((
        StatusCode::NOT_FOUND,
        format!("未知 provider: {}", req.provider_id),
    ))?;

    ConversationStore::validate_workspace(&workspace_root)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let conv = ConversationStore::get(&workspace_root, &req.conversation_id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("会话不存在: {}", req.conversation_id),
        ))?;
    if conv.provider_id != req.provider_id {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "会话属于 provider {}，不能用 {} 继续。请切回匹配模型或新建会话。",
                conv.provider_id, req.provider_id
            ),
        ));
    }

    let mut provider_config = req.provider_config.clone();
    if let Some(psid) = &conv.provider_session_id {
        if !provider_config.is_object() {
            provider_config = serde_json::json!({});
        }
        if let Some(obj) = provider_config.as_object_mut() {
            obj.entry("resume")
                .or_insert(serde_json::Value::String(psid.clone()));
        }
    }

    let working_dir = workspace_root.join(&conv.subdir);
    let session_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel::<AgentEvent>(64);
    let cancel = CancellationToken::new();
    state.sessions.register(session_id.clone(), cancel.clone());

    let _ = ConversationStore::append_history(
        &workspace_root,
        &conv.id,
        &serde_json::json!({
            "type": "user",
            "session_id": session_id,
            "delta": req.prompt,
        }),
    );

    let opts = agent::SendOptions {
        session_id: session_id.clone(),
        prompt: req.prompt,
        working_dir: Some(working_dir.to_string_lossy().to_string()),
        provider_config,
    };

    let sessions = state.sessions.clone();
    let sid_for_run = session_id.clone();
    let workspace_for_persist = workspace_root.clone();
    let conv_id_for_persist = conv.id.clone();
    let tx_for_run = tx.clone();

    tokio::spawn(async move {
        let (provider_tx, mut provider_rx) = mpsc::channel::<AgentEvent>(64);
        let provider_handle = {
            let cancel = cancel.clone();
            tokio::spawn(async move { provider.run(opts, provider_tx, cancel).await })
        };

        while let Some(evt) = provider_rx.recv().await {
            if let AgentEvent::ProviderSessionId {
                provider_session_id,
                ..
            } = &evt
            {
                let _ = ConversationStore::update_provider_session_id(
                    &workspace_for_persist,
                    &conv_id_for_persist,
                    provider_session_id,
                );
            }
            // Thinking is transient: shown live in the active assistant bubble
            // and dropped from JSONL so replays stay clean.
            if !matches!(evt, AgentEvent::Thinking { .. }) {
                if let Ok(v) = serde_json::to_value(&evt) {
                    let _ = ConversationStore::append_history(
                        &workspace_for_persist,
                        &conv_id_for_persist,
                        &v,
                    );
                }
            }
            if tx_for_run.send(evt).await.is_err() {
                break;
            }
        }

        if let Ok(Err(e)) = provider_handle.await {
            let _ = tx_for_run
                .send(AgentEvent::Error {
                    session_id: sid_for_run.clone(),
                    message: e.to_string(),
                })
                .await;
            let _ = tx_for_run
                .send(AgentEvent::Finished {
                    session_id: sid_for_run.clone(),
                    reason: "error".into(),
                })
                .await;
        }
        let _ = ConversationStore::touch(&workspace_for_persist, &conv_id_for_persist);
        sessions.remove(&sid_for_run);
    });

    let envelope = ChatSessionEnvelope {
        session_id: session_id.clone(),
        conversation_id: conv.id,
    };
    let envelope_event = futures::stream::once(async move {
        Ok(Event::default()
            .event("session")
            .json_data(envelope)
            .unwrap())
    });

    let agent_stream = ReceiverStream::new(rx).map(|evt| {
        Ok::<_, Infallible>(
            Event::default()
                .event("agent")
                .json_data(evt)
                .unwrap_or_else(|_| Event::default().data("{}")),
        )
    });

    let merged = envelope_event.chain(agent_stream);
    Ok(Sse::new(merged).keep_alive(KeepAlive::default()))
}

async fn cancel(
    State(state): State<AppState>,
    _user: AuthedUser,
    Path(session_id): Path<String>,
) -> Json<bool> {
    Json(state.sessions.cancel(&session_id))
}

// ---------- admin ----------

async fn admin_list_users(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Vec<UserView>>, (StatusCode, String)> {
    let list = state
        .auth
        .users
        .list()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(list.iter().map(UserView::from).collect()))
}

#[derive(Debug, Deserialize)]
struct CreateUserBody {
    username: String,
    password: String,
    workspace_root: Option<String>,
}

async fn admin_create_user(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(body): Json<CreateUserBody>,
) -> Result<Json<UserView>, (StatusCode, String)> {
    if let Some(ws) = &body.workspace_root {
        if !ws.trim().is_empty() {
            ConversationStore::validate_workspace(&PathBuf::from(ws))
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("工作区无效：{e}")))?;
        }
    }
    let user = state
        .auth
        .users
        .create(
            &body.username,
            &body.password,
            body.workspace_root.filter(|s| !s.trim().is_empty()),
        )
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json((&user).into()))
}

async fn admin_delete_user(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(username): Path<String>,
) -> Result<Json<bool>, (StatusCode, String)> {
    if admin.0.username == username {
        return Err((StatusCode::BAD_REQUEST, "不能删除自己".into()));
    }
    state
        .auth
        .users
        .delete(&username)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    state.auth.tokens.revoke_user(&username);
    Ok(Json(true))
}

#[derive(Debug, Deserialize)]
struct SetPasswordBody {
    password: String,
}

async fn admin_set_password(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(username): Path<String>,
    Json(body): Json<SetPasswordBody>,
) -> Result<Json<bool>, (StatusCode, String)> {
    state
        .auth
        .users
        .set_password(&username, &body.password)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    // Force the affected user to log in again.
    state.auth.tokens.revoke_user(&username);
    Ok(Json(true))
}

#[derive(Debug, Deserialize)]
struct SetWorkspaceBody {
    workspace_root: Option<String>,
}

async fn admin_set_workspace(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(username): Path<String>,
    Json(body): Json<SetWorkspaceBody>,
) -> Result<Json<UserView>, (StatusCode, String)> {
    let trimmed = body
        .workspace_root
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(ws) = &trimmed {
        ConversationStore::validate_workspace(&PathBuf::from(ws))
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("工作区无效：{e}")))?;
    }
    state
        .auth
        .users
        .set_workspace(&username, trimmed)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let user = state
        .auth
        .users
        .find(&username)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "用户不存在".into()))?;
    Ok(Json((&user).into()))
}

#[derive(Debug, Deserialize)]
struct WorkspaceCheckQuery {
    workspace_root: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceCheckResponse {
    ok: bool,
    message: Option<String>,
}

async fn admin_check_workspace(
    _admin: AdminUser,
    Query(q): Query<WorkspaceCheckQuery>,
) -> Json<WorkspaceCheckResponse> {
    match ConversationStore::validate_workspace(&PathBuf::from(&q.workspace_root)) {
        Ok(()) => Json(WorkspaceCheckResponse {
            ok: true,
            message: None,
        }),
        Err(e) => Json(WorkspaceCheckResponse {
            ok: false,
            message: Some(e),
        }),
    }
}
