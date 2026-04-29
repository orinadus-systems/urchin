use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use urchin_core::{
    config::Config,
    event::Event,
    identity::Identity,
    journal::Journal,
};

#[derive(Clone)]
pub struct AppState {
    pub journal: Arc<Journal>,
    pub journal_path: PathBuf,
    pub identity: Arc<Identity>,
}

impl AppState {
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            journal: Arc::new(Journal::new(cfg.journal_path.clone())),
            journal_path: cfg.journal_path.clone(),
            identity: Arc::new(Identity::resolve()),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ingest", post(ingest))
        .with_state(state)
}

/// Start the intake server on 127.0.0.1:<cfg.intake_port>.
/// Blocks until the process is killed or the listener dies.
pub async fn serve(cfg: &Config) -> Result<()> {
    let state = AppState::from_config(cfg);
    let addr = format!("127.0.0.1:{}", cfg.intake_port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("intake listening on {}", addr);
    axum::serve(listener, router(state)).await?;
    Ok(())
}

/// Start the intake server with an external shutdown signal.
/// The server stops cleanly once `shutdown` resolves.
pub async fn serve_with_shutdown(
    cfg: &Config,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let state = AppState::from_config(cfg);
    let addr = format!("127.0.0.1:{}", cfg.intake_port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("intake listening on {}", addr);
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let count = match state.journal.stats() {
        Ok(s) => s.event_count,
        Err(_) => 0,
    };
    Json(json!({
        "status":  "ok",
        "events":  count,
        "journal": state.journal_path.display().to_string(),
    }))
}

/// Accept a fully-formed Event from the SDK or any caller.
/// The event's id and timestamp are preserved exactly — the SDK owns creation identity.
async fn ingest(
    State(state): State<AppState>,
    Json(event): Json<Event>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let id = event.id;

    if let Err(e) = state.journal.append(&event) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ));
    }

    Ok(Json(json!({
        "id":     id,
        "status": "ok",
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tempfile::NamedTempFile;
    use tower::ServiceExt;

    fn test_state(path: PathBuf) -> AppState {
        AppState {
            journal:      Arc::new(Journal::new(path.clone())),
            journal_path: path,
            identity:     Arc::new(Identity {
                account: "test".into(),
                device:  "test".into(),
            }),
        }
    }

    async fn json_body(resp: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // Minimal valid Event JSON — id and timestamp are required by the Event struct.
    const EVENT_JSON: &str = r#"{
        "id": "56816532-adb7-4000-8a0f-1dda8408aab5",
        "timestamp": "2026-04-28T12:00:00Z",
        "source": "test",
        "kind": "conversation",
        "content": "hello from test"
    }"#;

    #[tokio::test]
    async fn health_reflects_ingested_events() {
        let tmp = NamedTempFile::new().unwrap();
        let state = test_state(tmp.path().to_path_buf());
        let app = router(state);

        let resp = app.clone().oneshot(
            Request::builder().uri("/health").body(Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let before = json_body(resp).await;
        assert_eq!(before["events"], 0);

        let resp = app.clone().oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest")
                .header("content-type", "application/json")
                .body(Body::from(EVENT_JSON))
                .unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let posted = json_body(resp).await;
        assert_eq!(posted["status"], "ok");
        assert_eq!(posted["id"], "56816532-adb7-4000-8a0f-1dda8408aab5");

        let resp = app.oneshot(
            Request::builder().uri("/health").body(Body::empty()).unwrap()
        ).await.unwrap();
        let after = json_body(resp).await;
        assert_eq!(after["events"], 1);
    }

    #[tokio::test]
    async fn ingest_rejects_missing_required_fields() {
        let tmp = NamedTempFile::new().unwrap();
        let state = test_state(tmp.path().to_path_buf());
        let app = router(state);

        // Missing id, timestamp — not a valid Event
        let body = r#"{"source":"test","content":"oops"}"#;
        let resp = app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap()
        ).await.unwrap();

        assert!(resp.status().is_client_error());
    }
}
