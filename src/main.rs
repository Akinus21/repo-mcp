mod exec_tools;
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

#[derive(Clone, serde::Deserialize)]
pub struct GitCredential {
    pub host: String,
    pub username: String,
    pub token: String,
}

#[derive(Clone)]
pub struct AppState {
    pub base_dir: PathBuf,
    pub git_author_name: String,
    pub git_author_email: String,
    pub git_credentials: Vec<GitCredential>,
    pub sessions: Arc<Mutex<std::collections::HashSet<String>>>,
    pub exec_enabled: bool,
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

    let mut git_credentials: Vec<GitCredential> = Vec::new();

    // Preferred: a JSON array of { host, username, token } objects, one per
    // git remote host — lets a single container talk to Forgejo, GitHub,
    // etc. all at once.
    if let Ok(raw) = std::env::var("GIT_CREDENTIALS_JSON") {
        match serde_json::from_str::<Vec<GitCredential>>(&raw) {
            Ok(mut parsed) => git_credentials.append(&mut parsed),
            Err(e) => tracing::error!("failed to parse GIT_CREDENTIALS_JSON: {e}"),
        }
    }

    // Back-compat: a single host via FORGEJO_HOST/USERNAME/TOKEN. Defensively
    // strip any accidental scheme prefix (a real mistake made once already)
    // since the value must be a bare hostname to match remote URLs.
    if let (Ok(host), Ok(username), Ok(token)) = (
        std::env::var("FORGEJO_HOST"),
        std::env::var("FORGEJO_USERNAME"),
        std::env::var("FORGEJO_TOKEN"),
    ) {
        let host = host
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        git_credentials.push(GitCredential {
            host,
            username,
            token,
        });
    }

    for cred in &git_credentials {
        tracing::info!("configured git credential for host: {}", cred.host);
    }

    // exec_command runs arbitrary shell in the sandboxed base_dir — a wider
    // surface than the fs/git tools, so it's opt-in rather than on by
    // default. Set REPO_MCP_EXEC_ENABLED=true to allow it.
    let exec_enabled = std::env::var("REPO_MCP_EXEC_ENABLED")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if exec_enabled {
        tracing::info!("exec_command is ENABLED — arbitrary shell commands will run in base_dir");
    } else {
        tracing::info!("exec_command is disabled (set REPO_MCP_EXEC_ENABLED=true to enable)");
    }

    std::fs::create_dir_all(&base_dir).expect("failed to create base_dir");

    let state = AppState {
        base_dir: PathBuf::from(&base_dir),
        git_author_name,
        git_author_email,
        git_credentials,
        sessions: Arc::new(Mutex::new(std::collections::HashSet::new())),
        exec_enabled,
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
