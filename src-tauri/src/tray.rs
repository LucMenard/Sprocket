//! The tray icon. Closing the window only hides it: runs go on, and Quit here really quits.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager};

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Quit from the tray: the agents are stopped first, then the app exits. If the event loop hasn't
/// ended two seconds later, the process ends anyway, so Quit always quits.
pub fn quit(app: &AppHandle) {
    log::info!("Quit from the tray");
    if let Some(ctx) = app.try_state::<std::sync::Arc<crate::Ctx>>() {
        ctx.host.shutdown();
    }
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(2));
        log::warn!("The app didn't exit by itself: ending the process");
        std::process::exit(0);
    });
    app.exit(0);
}

pub fn create(app: &App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open Sprocket", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit (stops running agents)", true, None::<&str>)?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("bundle icon"))
        .tooltip("Sprocket")
        .menu(&Menu::with_items(app, &[&show, &quit])?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => self::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
