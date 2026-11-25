//! What the UI can ask the core for.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, Result};
use crate::engine::{ActivityView, McpServers, RunView, StartRun};
use crate::host::HostState;
use crate::catalog::{self, AgentDraft, Dirs, WorkflowDraft};
use crate::{agent, workflow, ApiCall, Ctx};

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

/// A note on a run's board from the person at this computer.
#[tauri::command]
pub async fn post_note(ctx: State<'_, Arc<Ctx>>, run_id: String, body: String, to: Option<String>) -> Result<crate::engine::NoteView> {
    let me = std::env::var("USERNAME").ok().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| "You".into());
    ctx.engine.post_note(&run_id, &me, &body, to.as_deref())
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

fn dirs(ctx: &Ctx) -> Dirs {
    Dirs::new(&ctx.workflows_dir)
}

/// The shared agent types, each with the workflows that use it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentListView {
    agents: Vec<agent::SharedAgent>,
    problems: Vec<agent::Problem>,
}

#[tauri::command]
pub async fn list_agents(ctx: State<'_, Arc<Ctx>>) -> Result<AgentListView> {
    let list = agent::load_all(&dirs(&ctx).agents)?;
    Ok(AgentListView { agents: list.agents, problems: list.problems })
}

#[tauri::command]
pub async fn save_workflow(ctx: State<'_, Arc<Ctx>>, id: String, draft: WorkflowDraft) -> Result<workflow::Workflow> {
    catalog::save_workflow(&dirs(&ctx), &id, draft)
}

#[tauri::command]
pub async fn create_workflow(ctx: State<'_, Arc<Ctx>>, name: String) -> Result<workflow::Workflow> {
    catalog::create_workflow(&dirs(&ctx), &name)
}

/// Its past runs stay (they keep the workflow they ran); one still going must end first.
#[tauri::command]
pub async fn delete_workflow(ctx: State<'_, Arc<Ctx>>, id: String) -> Result<()> {
    let going = ctx
        .engine
        .runs(None)?
        .into_iter()
        .any(|r| r.workflow_id == id && matches!(r.state.as_str(), "running" | "waiting" | "interrupted"));
    if going {
        return Err(AppError::new("conflict", "A run of this workflow is still going: cancel it first"));
    }
    catalog::delete_workflow(&dirs(&ctx), &id)
}

/// `create` makes a new one and refuses to replace an existing one.
#[tauri::command]
pub async fn save_agent(ctx: State<'_, Arc<Ctx>>, id: String, draft: AgentDraft, create: bool) -> Result<agent::AgentType> {
    catalog::save_agent(&dirs(&ctx), &id, draft, create)
}

#[tauri::command]
pub async fn delete_agent(ctx: State<'_, Arc<Ctx>>, id: String) -> Result<()> {
    catalog::delete_agent(&dirs(&ctx), &id)
}

#[tauri::command]
pub async fn share_agent(ctx: State<'_, Arc<Ctx>>, workflow: String, id: String) -> Result<agent::AgentType> {
    catalog::share_agent(&dirs(&ctx), &workflow, &id)
}
