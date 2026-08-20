mod fs_tools;
mod git_tools;
mod tools;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{sse::Event, sse::KeepAlive, IntoResponse, Response, Sse},
    routing::{get, head, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{convert::Infallible, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tokio_stream::{wrappers::IntervalStream, StreamExt as _};
use uuid::Uuid;

const SESSION_HEADER: &str = "Mcp-Session-Id";

#[derive(Clone)]
pub struct AppState {
    pub base_dir: PathBuf,
    pub git_author_name: String,
    pub git_author_email: String,
    pub sessions: Arc<Mutex<std::collections::HashSet<String>>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let base_dir = std::env::var("REPO_MCP_BASE_DIR").unwrap_or_else(|_| "/repos".to_string());
    let port: u16 = std::env::var("REPO_MCP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let git_author_name =
        std::env::var("GIT_AUTHOR_NAME").unwrap_or_else(|_| "repo-mcp".to_string());
    let git_author_email =
        std::env::var("GIT_AUTHOR_EMAIL").unwrap_or_else(|_| "repo-mcp@localhost".to_string());

    std::fs::create_dir_all(&base_dir).expect("failed to create base_dir");

    let state = AppState {
        base_dir: PathBuf::from(&base_dir),
        git_author_name,
        git_author_email,
        sessions: Arc::new(Mutex::new(std::collections::HashSet::new())),
    };

    let app = Router::new()
        .route("/mcp", post(handle_post))
        .route("/mcp", get(handle_get))
        .route("/mcp", head(|| async { StatusCode::OK }))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state);

    tracing::info!("repo-mcp listening on 0.0.0.0:{port}, base_dir={base_dir}");
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind failed");
    axum::serve(listener, app).await.expect("server error");
}

async fn handle_get(
    _headers: HeaderMap,
    State(_state): State<AppState>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let stream = IntervalStream::new(tokio::time::interval(std::time::Duration::from_secs(20)))
        .map(|_| Ok(Event::default().comment("keep-alive")));

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn handle_post(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = body.get("id").cloned().unwrap_or(Value::Null);

    let incoming_session = headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if method == "initialize" {
        let session_id = Uuid::new_v4().to_string();
        state.sessions.lock().await.insert(session_id.clone());

        let result = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "repo-mcp", "version": env!("CARGO_PKG_VERSION") }
            }
        });

        let mut resp = Json(result).into_response();
        resp.headers_mut()
            .insert(SESSION_HEADER, HeaderValue::from_str(&session_id).unwrap());
        return resp;
    }

    if let Some(sid) = &incoming_session {
        state.sessions.lock().await.insert(sid.clone());
    }

    let result = match method {
        "notifications/initialized" => return StatusCode::ACCEPTED.into_response(),
        "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tools::tool_list() }
        }),
        "tools/call" => {
            let name = body
                .pointer("/params/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = body
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            match tools::dispatch(&state, name, &args).await {
                Ok(text) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": text }],
                        "isError": false
                    }
                }),
                Err(err) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": err }],
                        "isError": true
                    }
                }),
            }
        }
        other => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("method not found: {other}") }
        }),
    };

    let mut resp = Json(result).into_response();
    if let Some(sid) = incoming_session {
        resp.headers_mut()
            .insert(SESSION_HEADER, HeaderValue::from_str(&sid).unwrap());
    }
    resp
}
