//! The local HTTP API Tracket calls, on 127.0.0.1 only, behind the bearer token.
//! The endpoints follow docs/ARCHITECTURE.md.

use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::engine::StartRun;
use crate::error::AppError;
use crate::{token, workflow, Ctx};

pub const DEFAULT_PORT: u16 = 47821;

/// `SPROCKET_PORT` overrides the port, so a development build can run next to an installed one.
pub fn address() -> String {
    let port = std::env::var("SPROCKET_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
    format!("127.0.0.1:{port}")
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// Engine errors as HTTP: what the caller did wrong is a 4xx, the rest a 500.
fn from_app(e: AppError) -> Response {
    let status = match e.kind {
        "notFound" => StatusCode::NOT_FOUND,
        "invalid" => StatusCode::BAD_REQUEST,
        "busy" | "conflict" => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "error": e.message, "kind": e.kind }))).into_response()
}

fn respond<T: serde::Serialize>(result: crate::error::Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => from_app(e),
    }
}

async fn require_token(State(ctx): State<Arc<Ctx>>, request: Request, next: Next) -> Response {
    let given = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let (method, path) = (request.method().to_string(), request.uri().path().to_string());
    let response = match given {
        Some(given) if token::matches(&ctx.token, given) => next.run(request).await,
        _ => error(StatusCode::UNAUTHORIZED, "Missing or wrong token"),
    };
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    ctx.record_call(crate::ApiCall { at, method, path, status: response.status().as_u16() });
    response
}

async fn health(State(ctx): State<Arc<Ctx>>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "host": ctx.host.state(),
    }))
}

#[derive(Deserialize)]
struct WorkflowsQuery {
    offered: Option<bool>,
}

async fn workflows(State(ctx): State<Arc<Ctx>>, Query(q): Query<WorkflowsQuery>) -> Response {
    match workflow::load_all(&ctx.workflows_dir) {
        Ok(list) => {
            let items: Vec<_> = list
                .workflows
                .into_iter()
                .filter(|w| !q.offered.unwrap_or(false) || w.offer_in_tracket)
                .map(|w| json!({ "id": w.id, "name": w.name, "description": w.description }))
                .collect();
            Json(json!({ "workflows": items })).into_response()
        }
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
struct RunsQuery {
    ticket: Option<String>,
}

async fn list_runs(State(ctx): State<Arc<Ctx>>, Query(q): Query<RunsQuery>) -> Response {
    respond(ctx.engine.runs(q.ticket.as_deref()))
}

async fn start_run(State(ctx): State<Arc<Ctx>>, Json(req): Json<StartRun>) -> Response {
    match ctx.engine.start_run(req) {
        Ok(run) => (StatusCode::CREATED, Json(run)).into_response(),
        Err(e) => from_app(e),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Decision {
    decision: String,
    reason: Option<String>,
    /// Who decided, as Tracket knows them
    by: Option<String>,
}

async fn decide(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>, Json(d): Json<Decision>) -> Response {
    let approve = match d.decision.as_str() {
        "approve" => true,
        "reject" => false,
        _ => return error(StatusCode::BAD_REQUEST, "`decision` must be approve or reject"),
    };
    let by = d.by.unwrap_or_else(|| "Tracket".into());
    respond(ctx.engine.decide(&id, approve, d.reason, &by).map(|()| json!({ "ok": true })))
}

#[derive(Deserialize)]
struct NewNote {
    body: String,
    /// A step or an agent type it is for; everyone when left out
    to: Option<String>,
    /// Who wrote it, as Tracket knows them
    by: Option<String>,
}

async fn post_note(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>, Json(n): Json<NewNote>) -> Response {
    let by = n.by.unwrap_or_else(|| "Tracket".into());
    match ctx.engine.post_note(&id, &by, &n.body, n.to.as_deref()) {
        Ok(note) => (StatusCode::CREATED, Json(note)).into_response(),
        Err(e) => from_app(e),
    }
}

#[derive(Deserialize)]
struct By {
    by: Option<String>,
}

/// No body needed: who cancelled comes as `?by=`.
async fn cancel(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>, Query(q): Query<By>) -> Response {
    let by = q.by.unwrap_or_else(|| "Tracket".into());
    respond(ctx.engine.cancel_run(&id, &by).map(|()| json!({ "ok": true })))
}

async fn retry(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    respond(ctx.engine.retry_job(&id).map(|()| json!({ "ok": true })))
}

pub fn router(ctx: Arc<Ctx>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/workflows", get(workflows))
        .route("/v1/runs", get(list_runs).post(start_run))
        .route("/v1/runs/{id}/cancel", post(cancel))
        .route("/v1/runs/{id}/notes", post(post_note))
        .route("/v1/jobs/{id}/retry", post(retry))
        .route("/v1/approvals/{id}", post(decide))
        .layer(middleware::from_fn_with_state(ctx.clone(), require_token))
        .with_state(ctx)
}

/// Binds and serves. A port already taken is reported in the UI rather than ending the app.
pub fn serve(ctx: Arc<Ctx>) {
    tauri::async_runtime::spawn(async move {
        let addr = address();
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                ctx.set_api(Ok(addr));
                if let Err(e) = axum::serve(listener, router(ctx.clone())).await {
                    ctx.set_api(Err(format!("The local API stopped: {e}")));
                }
            }
            Err(e) => ctx.set_api(Err(format!("Couldn't listen on {addr}: {e}"))),
        }
    });
}
