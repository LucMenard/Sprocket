// Check 5: a Tauri app that hides to the tray when its window is closed, and keeps serving
// HTTP on loopback (with a bearer token) the whole time.
//   SPIKE_AUTOCLOSE=1  closes the window 2 s after start, like a user clicking X.
#![windows_subsystem = "windows"]

use std::time::Instant;

use axum::{extract::State, http::HeaderMap, http::StatusCode, routing::get, Json, Router};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};

const ADDR: &str = "127.0.0.1:47821";
const TOKEN: &str = "spike-token";

#[derive(Clone)]
struct Ctx {
    app: AppHandle,
    started: Instant,
}

async fn health(State(ctx): State<Ctx>, headers: HeaderMap) -> Result<Json<serde_json::Value>, StatusCode> {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok());
    if auth != Some(&format!("Bearer {TOKEN}")) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let visible = ctx
        .app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    Ok(Json(serde_json::json!({
        "ok": true,
        "windowVisible": visible,
        "uptimeSeconds": ctx.started.elapsed().as_secs(),
    })))
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let ctx = Ctx { app: app.handle().clone(), started: Instant::now() };
            tauri::async_runtime::spawn(async move {
                let router = Router::new().route("/v1/health", get(health)).with_state(ctx);
                let listener = tokio::net::TcpListener::bind(ADDR).await.expect("port in use");
                axum::serve(listener, router).await.expect("server failed");
            });

            let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("CT Tray Spike")
                .menu(&Menu::with_items(app, &[&show, &quit])?)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            if std::env::var("SPIKE_AUTOCLOSE").is_ok() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.close();
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app (and its runs) alive in the tray
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
