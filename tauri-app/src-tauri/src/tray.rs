// System tray (resident) and window-close interception.
//
// The close guard lives here in Rust — not in webview JS — because the taskbar
// "close window" command must be caught even while the (minimized) WebView2
// throttles script execution; a JS-only listener silently loses the event and
// the app exits without the mount warning. Rust owns the mount state anyway.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::config::ConfigManager;
use crate::webdav::platform;
use crate::webdav::WebDavManager;

/// Stable id for the tray icon so its menu can be re-localized later.
const TRAY_ID: &str = "shareonelist-tray";

/// Tauri event telling the frontend to show the close-guard dialog. Emitted
/// only after Rust has already prevented the close and confirmed a mounted
/// drive.
pub const CLOSE_GUARD_EVENT: &str = "close-guard-requested";

/// Show and focus the main window (tray click, second instance, guard).
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Unmount every mounted drive (best-effort — one stuck mapping must not
/// trap the user) and exit the application. Goes through the platform layer
/// directly: the auto_mount flags are intentionally KEPT so the next launch
/// restores the working set (see WebDavManager::restore_auto_mounts).
pub async fn quit_application(app: AppHandle) {
    if let Some(manager) = app.try_state::<WebDavManager>() {
        let status = manager.status().await;
        for mount in status.mounts.iter().filter(|m| m.mounted) {
            let entry = &mount.entry;
            if let Ok((_, _, port)) = manager.connection_info(&entry.mount_id).await {
                let _ = platform::unmount_drive(port, &entry.mount_id, entry.drive_letter.as_deref());
            }
        }
    }
    app.exit(0);
}

/// Build the resident tray icon with a menu localized from the stored config
/// language. Called once during setup; `set_tray_menu_labels` re-localizes.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let language = app
        .try_state::<ConfigManager>()
        .map(|cm| cm.load_config().language)
        .unwrap_or_default();
    let (open_label, quit_label) = default_tray_labels(&language);
    let menu = tray_menu(app, &open_label, &quit_label)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().expect("bundle icon").clone())
        .tooltip("ShareOneList")
        .menu(&menu)
        // Left click opens the window; the menu is for the right click.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { quit_application(app).await });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn tray_menu(
    app: &AppHandle,
    open_label: &str,
    quit_label: &str,
) -> tauri::Result<Menu<Wry>> {
    let open = MenuItem::with_id(app, "open", open_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    Menu::with_items(app, &[&open, &quit])
}

/// Tray menu labels for a stored UI language; non-Chinese languages get
/// English (matching the i18n fallback).
fn default_tray_labels(language: &str) -> (&'static str, &'static str) {
    if language == "zh-CN" {
        ("打开 ShareOneList", "退出")
    } else {
        ("Open ShareOneList", "Quit")
    }
}

/// Re-localize the tray menu after the user switches UI language.
#[tauri::command]
pub fn set_tray_menu_labels(
    app: AppHandle,
    open_label: String,
    quit_label: String,
) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    let menu = tray_menu(&app, &open_label, &quit_label).map_err(|e| e.to_string())?;
    tray.set_menu(Some(menu)).map_err(|e| e.to_string())
}

/// Unmount all mounted drives and exit (tray menu and close-guard's
/// "unmount and exit" both land here).
#[tauri::command]
pub async fn quit_application_command(app: AppHandle) {
    quit_application(app).await;
}

/// Decide what a main-window close request means. Runs async after the event
/// handler has unconditionally prevented the close:
/// - close-to-tray enabled → hide to tray, no questions asked (the setting is
///   the user's standing decision; mounts stay served by the gateway);
/// - else mounted drives present → restore + focus the window and ask the
///   frontend to show the guard dialog;
/// - else → destroy the window, which exits the app.
async fn decide_close(app: AppHandle) {
    let close_to_tray = app
        .try_state::<ConfigManager>()
        .map(|cm| cm.load_config().close_to_tray)
        .unwrap_or(false);
    if close_to_tray {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
        return;
    }

    let has_mounted = match app.try_state::<WebDavManager>() {
        Some(manager) => {
            let status = manager.status().await;
            status.running && status.mounts.iter().any(|m| m.mounted)
        }
        None => false,
    };
    if has_mounted {
        show_main_window(&app);
        let _ = app.emit(CLOSE_GUARD_EVENT, ());
        return;
    }

    if let Some(window) = app.get_webview_window("main") {
        // destroy() skips CloseRequested (no recursion) and tears the app down.
        let _ = window.destroy();
    }
}

/// Window-event hook: every close path (title bar X, Alt+F4, taskbar "close
/// window") lands here. The decision itself is async (mount probing), so the
/// close is always prevented first and `decide_close` settles the outcome.
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let app = window.app_handle().clone();
        tauri::async_runtime::spawn(async move { decide_close(app).await });
    }
}

#[cfg(test)]
mod tests {
    use super::default_tray_labels;

    // Feature: webdav-mount (close guard), Property: tray labels follow the
    // stored UI language with English fallback
    #[test]
    fn tray_labels_localize_with_fallback() {
        assert_eq!(default_tray_labels("zh-CN"), ("打开 ShareOneList", "退出"));
        assert_eq!(
            default_tray_labels("en-US"),
            ("Open ShareOneList", "Quit")
        );
        // Anything unknown (incl. "system") falls back to English.
        assert_eq!(default_tray_labels("system"), ("Open ShareOneList", "Quit"));
        assert_eq!(default_tray_labels(""), ("Open ShareOneList", "Quit"));
    }
}
