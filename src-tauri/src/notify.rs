//! Native Windows toast notifications for messages raised by the page.
//!
//! The page's `Notification` API is replaced by inject.js, which forwards each
//! notification here with a numeric id. When the user clicks the toast we bring the
//! window forward and tell the page which notification was clicked, so the page's own
//! click handler opens the right conversation.

use tauri::{AppHandle, Runtime};

/// A plain informational toast (used for update status).
pub fn info<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    show(app, 0, title, body);
}

#[cfg(windows)]
pub fn show<R: Runtime>(app: &AppHandle<R>, id: u32, title: &str, body: &str) {
    use crate::{show_main_window, MAIN_WINDOW};
    use tauri::Manager;
    use tauri_winrt_notification::Toast;

    // Installed builds have a Start menu shortcut registered with our identifier as its
    // AppUserModelID; dev builds don't, so borrow PowerShell's like tauri does.
    let app_id = if cfg!(debug_assertions) {
        Toast::POWERSHELL_APP_ID.to_string()
    } else {
        app.config().identifier.clone()
    };

    let click_app = app.clone();
    let title = truncate(title, 120);
    let body = truncate(body, 400);

    let result = Toast::new(&app_id)
        .title(&title)
        .text1(&body)
        .on_activated(move |_action| {
            show_main_window(&click_app);
            if let Some(window) = click_app.get_webview_window(MAIN_WINDOW) {
                let _ = window.eval(format!(
                    "window.__gmdNotificationClicked && window.__gmdNotificationClicked({id})"
                ));
            }
            Ok(())
        })
        .show();

    if let Err(err) = result {
        eprintln!("failed to show notification: {err}");
    }

    // Flash the taskbar button if the window isn't in front.
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        if !window.is_focused().unwrap_or(false) {
            let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
        }
    }
}

#[cfg(not(windows))]
pub fn show<R: Runtime>(_app: &AppHandle<R>, _id: u32, title: &str, body: &str) {
    // Only Windows is supported; elsewhere just log so dev builds still run.
    eprintln!("notification: {} - {}", truncate(title, 120), truncate(body, 400));
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}
