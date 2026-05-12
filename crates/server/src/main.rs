use std::convert::Infallible;
use std::sync::Arc;

use agent::{AgentEvent, ProviderInfo, Registry, SessionManager};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
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
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,tower_http=info")),
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
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<bool>, (StatusCode, String)> {
    let p = state
        .registry
        .get(&id)
        .ok_or((StatusCode::NOT_FOUND, format!("unknown provider: {id}")))?;
    Ok(Json(p.detect().await))
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    provider_id: String,
    prompt: String,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    provider_config: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ChatSessionEnvelope {
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct ChatQuery {
    /// Optional client-side hint; ignored otherwise.
    #[allow(dead_code)]
    client: Option<String>,
}

async fn chat(
    State(state): State<AppState>,
    Query(_q): Query<ChatQuery>,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let provider = state
        .registry
        .get(&req.provider_id)
        .ok_or((StatusCode::NOT_FOUND, format!("unknown provider: {}", req.provider_id)))?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel::<AgentEvent>(64);
    let cancel = CancellationToken::new();
    state.sessions.register(session_id.clone(), cancel.clone());

    let opts = agent::SendOptions {
        session_id: session_id.clone(),
        prompt: req.prompt,
        working_dir: req.working_dir,
        provider_config: req.provider_config,
    };

    let sessions = state.sessions.clone();
    let sid_for_run = session_id.clone();
    let tx_for_err = tx.clone();
    tokio::spawn(async move {
        if let Err(e) = provider.run(opts, tx_for_err.clone(), cancel).await {
            let _ = tx_for_err
                .send(AgentEvent::Error {
                    session_id: sid_for_run.clone(),
                    message: e.to_string(),
                })
                .await;
            let _ = tx_for_err
                .send(AgentEvent::Finished {
                    session_id: sid_for_run.clone(),
                    reason: "error".into(),
                })
                .await;
        }
        sessions.remove(&sid_for_run);
    });

    // First SSE event: hand back the session id so the client can cancel.
    let envelope_event = futures::stream::once(async move {
        Ok(Event::default()
            .event("session")
            .json_data(ChatSessionEnvelope {
                session_id: session_id.clone(),
            })
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
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Json<bool> {
    Json(state.sessions.cancel(&session_id))
}
