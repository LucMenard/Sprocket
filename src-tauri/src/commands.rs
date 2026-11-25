//! What the UI can ask the core for.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, Result};
use crate::engine::{ActivityView, McpServers, RunView, StartRun};
use crate::host::HostState;
use crate::{workflow, ApiCall, Ctx};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    version: &'static str,
    api_address: Option<String>,
    api_error: Option<String>,
    host: HostState,
    data_dir: String,
}

#[tauri::command]
pub fn app_status(ctx: State<'_, Arc<Ctx>>) -> AppStatus {
    let (api_address, api_error) = match ctx.api() {
        Some(Ok(addr)) => (Some(addr), None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, Some("Starting…".into())),
    };
    AppStatus {
        version: env!("CARGO_PKG_VERSION"),
        api_address,
        api_error,
        host: ctx.host.state(),
        data_dir: ctx.data_dir.display().to_string(),
    }
}

#[tauri::command]
pub async fn list_workflows(ctx: State<'_, Arc<Ctx>>) -> Result<workflow::WorkflowList> {
    Ok(workflow::load_all(&ctx.workflows_dir)?)
}

/// Shown only so it can be copied into Tracket's settings.
#[tauri::command]
pub fn api_token(ctx: State<'_, Arc<Ctx>>) -> String {
    ctx.token.clone()
}

#[tauri::command]
pub fn open_data_dir(app: tauri::AppHandle, ctx: State<'_, Arc<Ctx>>) -> Result<()> {
    app.opener()
        .open_path(ctx.data_dir.display().to_string(), None::<&str>)
        .map_err(|e| AppError::new("opener", e.to_string()))
}

#[tauri::command]
pub async fn list_runs(ctx: State<'_, Arc<Ctx>>, ticket: Option<String>) -> Result<Vec<RunView>> {
    ctx.engine.runs(ticket.as_deref())
}

#[tauri::command]
pub async fn start_run(ctx: State<'_, Arc<Ctx>>, request: StartRun) -> Result<RunView> {
    ctx.engine.start_run(request)
}

#[tauri::command]
pub async fn decide_approval(ctx: State<'_, Arc<Ctx>>, id: String, approve: bool, reason: Option<String>) -> Result<()> {
    ctx.engine.decide(&id, approve, reason, "Sprocket")
}

#[tauri::command]
pub async fn cancel_run(ctx: State<'_, Arc<Ctx>>, id: String) -> Result<()> {
    ctx.engine.cancel_run(&id, "Sprocket")
}

#[tauri::command]
pub async fn retry_job(ctx: State<'_, Arc<Ctx>>, id: String) -> Result<()> {
    ctx.engine.retry_job(&id)
}

#[tauri::command]
pub async fn job_messages(ctx: State<'_, Arc<Ctx>>, id: String) -> Result<Vec<serde_json::Value>> {
    ctx.engine.job_messages(&id)
}

#[tauri::command]
pub async fn activity(ctx: State<'_, Arc<Ctx>>, limit: Option<u32>) -> Result<Vec<ActivityView>> {
    ctx.engine.activity(limit.unwrap_or(30))
}

#[tauri::command]
pub async fn mcp_servers(ctx: State<'_, Arc<Ctx>>) -> Result<McpServers> {
    ctx.engine.mcp_servers()
}

/// The latest calls to the local API (from Tracket, or anything holding the token).
#[tauri::command]
pub fn api_calls(ctx: State<'_, Arc<Ctx>>) -> Vec<ApiCall> {
    ctx.calls()
}

/// Opens a workflow file in the default editor; only files in the workflows folder.
#[tauri::command]
pub fn open_workflow_file(app: tauri::AppHandle, ctx: State<'_, Arc<Ctx>>, file: String) -> Result<()> {
    let path = std::path::Path::new(&file);
    let inside = path
        .canonicalize()
        .ok()
        .zip(ctx.workflows_dir.canonicalize().ok())
        .is_some_and(|(p, dir)| p.starts_with(dir));
    if !inside {
        return Err(AppError::new("invalid", "Not a file of the workflows folder"));
    }
    app.opener()
        .open_path(file, None::<&str>)
        .map_err(|e| AppError::new("opener", e.to_string()))
}
