//! System tray: show the window, stop everything, quit. Left-click shows the window.

use tauri::{
    AppHandle, Manager,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::commands::AppState;

/// Tray id, unique to this app. On Linux the tray library derives the
/// indicator id and the temp icon file (`$XDG_RUNTIME_DIR/tray-icon/tray-icon-<id>-N.png`)
/// from it, so a generic id like "main" collides with other Tauri apps: they
/// overwrite each other's icon file and show up as duplicate icons.
const TRAY_ID: &str = "rouse-tray";

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    // A lingering tray (hot reload) would make libayatana warn "already exported".
    if app.tray_by_id(TRAY_ID).is_some() {
        return Ok(());
    }
    let open = MenuItemBuilder::with_id("open", "Open rouse").build(app)?;
    let stop = MenuItemBuilder::with_id("stop_all", "Stop everything").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit rouse").build(app)?;
    let menu = MenuBuilder::new(app).item(&open).item(&stop).separator().item(&quit).build()?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("rouse")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            // Stopping waits for processes; never block the menu's event loop on it.
            "stop_all" => {
                let app = app.clone();
                std::thread::spawn(move || {
                    app.state::<AppState>().procs.stop_all();
                    crate::record_live(&app);
                });
            }
            // Exit runs the ExitRequested handler, which stops everything first.
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}
