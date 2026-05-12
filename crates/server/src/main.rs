mod conversation;

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use agent::{AgentEvent, ProviderInfo, Registry, SessionManager};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
        )
        .init();

    let state = AppState {
        registry: Arc::new(Registry::default_providers()),
        sessions: Arc::new(SessionManager::new()),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/providers", get(list_providers))
        .route("/api/providers/:id/detect", get(detect_provider))
        .route("/api/workspace/check", get(check_workspace))
        .route("/api/conversations", get(list_conversations))
        .route("/api/conversations", post(create_conversation))
        .route("/api/conversations/:id/history", get(get_history))
        .route("/api/conversations/:id", delete(delete_conversation))
        .route("/api/chat", post(chat))
        .route("/api/cancel/:session_id", post(cancel))
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

async fn list_providers(State(state): State<AppState>) -> Json<Vec<ProviderInfo>> {
    Json(state.registry.list())
}

async fn detect_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<bool>, (StatusCode, String)> {
    let p = state
        .registry
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, format!("unknown provider: {id}")))?;
    Ok(Json(p.detect().await))
}

#[derive(Debug, Deserialize)]
struct WorkspaceQuery {
    workspace_root: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceCheck {
    ok: bool,
    message: Option<String>,
}

async fn check_workspace(Query(q): Query<WorkspaceQuery>) -> Json<WorkspaceCheck> {
    match ConversationStore::validate_workspace(&PathBuf::from(&q.workspace_root)) {
        Ok(()) => Json(WorkspaceCheck { ok: true, message: None }),
        Err(e) => Json(WorkspaceCheck { ok: false, message: Some(e) }),
    }
}

async fn list_conversations(
    Query(q): Query<WorkspaceQuery>,
) -> Result<Json<Vec<Conversation>>, (StatusCode, String)> {
    ConversationStore::list(&PathBuf::from(&q.workspace_root))
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[derive(Debug, Deserialize)]
struct CreateConvBody {
    workspace_root: String,
    provider_id: String,
    name: Option<String>,
}

async fn create_conversation(
    Json(body): Json<CreateConvBody>,
) -> Result<Json<Conversation>, (StatusCode, String)> {
    ConversationStore::create(
        &PathBuf::from(&body.workspace_root),
        &body.provider_id,
        body.name,
    )
    .map(Json)
    .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn get_history(
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    ConversationStore::read_history(&PathBuf::from(&q.workspace_root), &id)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

async fn delete_conversation(
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
) -> Result<Json<bool>, (StatusCode, String)> {
    ConversationStore::delete(&PathBuf::from(&q.workspace_root), &id)
        .map(|()| Json(true))
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    provider_id: String,
    prompt: String,
    /// Required: caller picks (or first creates) a conversation, then sends
    /// messages against it. Working dir is derived from the conversation.
    conversation_id: String,
    workspace_root: String,
    /// Provider-specific config (extra_args, etc). Server may augment with
    /// resume = conversation.provider_session_id when present.
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
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let provider = state.registry.get(&req.provider_id).ok_or((
        StatusCode::NOT_FOUND,
        format!("unknown provider: {}", req.provider_id),
    ))?;

    let workspace_root = PathBuf::from(&req.workspace_root);
    ConversationStore::validate_workspace(&workspace_root)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let conv = ConversationStore::get(&workspace_root, &req.conversation_id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("conversation not found: {}", req.conversation_id),
        ))?;

    // Inject resume token if we have one from a previous turn.
    let mut provider_config = req.provider_config.clone();
    if let Some(psid) = &conv.provider_session_id {
        if !provider_config.is_object() {
            provider_config = serde_json::json!({});
        }
        if let Some(obj) = provider_config.as_object_mut() {
            obj.entry("resume").or_insert(serde_json::Value::String(psid.clone()));
        }
    }

    let working_dir = workspace_root.join(&conv.subdir);
    let session_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel::<AgentEvent>(64);
    let cancel = CancellationToken::new();
    state.sessions.register(session_id.clone(), cancel.clone());

    // Persist the user's prompt as the first entry of this turn.
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
        // Intercept events to persist Claude's session id; otherwise pass through.
        let (provider_tx, mut provider_rx) = mpsc::channel::<AgentEvent>(64);
        let provider_handle = {
            let cancel = cancel.clone();
            tokio::spawn(async move {
                provider.run(opts, provider_tx, cancel).await
            })
        };

        while let Some(evt) = provider_rx.recv().await {
            if let AgentEvent::ProviderSessionId {
                provider_session_id, ..
            } = &evt
            {
                let _ = ConversationStore::update_provider_session_id(
                    &workspace_for_persist,
                    &conv_id_for_persist,
                    provider_session_id,
                );
            }
            // Append every event to the conversation log so replay works.
            if let Ok(v) = serde_json::to_value(&evt) {
                let _ = ConversationStore::append_history(
                    &workspace_for_persist,
                    &conv_id_for_persist,
                    &v,
                );
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

    // First SSE event: hand back the session id + conversation id.
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
    Path(session_id): Path<String>,
) -> Json<bool> {
    Json(state.sessions.cancel(&session_id))
}
