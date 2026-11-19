//! Messages Desktop: a small, maintained desktop shell for Google Messages for Web.
//!
//! The page itself runs in the system WebView2 (Edge) runtime, which Windows keeps
//! patched independently of this app. Everything here is the native glue around it:
//! tray, notifications, unread badge, start-up, single instance and self-update.

mod notify;
mod updates;

use std::sync::atomic::{AtomicU32, Ordering};

use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    webview::NewWindowResponse,
    AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_opener::OpenerExt as _;

pub const MAIN_WINDOW: &str = "main";
const START_URL: &str = "https://messages.google.com/web/conversations";
const TRAY_ID: &str = "main-tray";
const MINIMIZED_ARG: &str = "--minimized";

const INJECT_JS: &str = include_str!("inject.js");
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");
const TRAY_ICON_UNREAD: &[u8] = include_bytes!("../icons/tray-unread.png");
const BADGE_ICON: &[u8] = include_bytes!("../icons/badge.png");

/// Last unread count we applied, so we only touch the taskbar when it changes.
static UNREAD: AtomicU32 = AtomicU32::new(u32::MAX);

/// Hosts the main window may navigate to. Everything else opens in the default browser.
fn is_internal_host(url: &Url) -> bool {
    match url.scheme() {
        "about" | "data" | "blob" => return true,
        "https" => {}
        _ => return false,
    }
    let Some(host) = url.host_str() else { return false };
    host == "messages.google.com"
        || host == "accounts.google.com"
        || host.ends_with(".google.com") && host.starts_with("accounts.")
        || host == "accounts.youtube.com"
        || host.ends_with(".gstatic.com")
        || host == "ssl.gstatic.com"
}

fn open_in_browser<R: Runtime>(app: &AppHandle<R>, url: &str) {
    if let Ok(parsed) = Url::parse(url) {
        if matches!(parsed.scheme(), "http" | "https" | "mailto" | "tel") {
            let _ = app.opener().open_url(parsed.as_str(), None::<&str>);
        }
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let visible = window.is_visible().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);
        if visible && !minimized {
            let _ = window.hide();
        } else {
            show_main_window(app);
        }
    }
}

// ---------------------------------------------------------------------------
// Commands called from the injected page script (see inject.js and capabilities/).
// ---------------------------------------------------------------------------

#[tauri::command]
fn notify(app: AppHandle, id: u32, title: String, body: String) {
    notify::show(&app, id, &title, &body);
}

#[tauri::command]
fn set_unread(app: AppHandle, count: u32) {
    apply_unread(&app, count);
}

#[tauri::command]
fn open_external(app: AppHandle, url: String) {
    open_in_browser(&app, &url);
}

fn apply_unread<R: Runtime>(app: &AppHandle<R>, count: u32) {
    if UNREAD.swap(count, Ordering::SeqCst) == count {
        return;
    }
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let overlay = if count > 0 { Image::from_bytes(BADGE_ICON).ok() } else { None };
        let _ = window.set_overlay_icon(overlay);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let (icon, tip) = if count > 0 {
            let noun = if count == 1 { "unread conversation" } else { "unread conversations" };
            (TRAY_ICON_UNREAD, format!("Messages ({count} {noun})"))
        } else {
            (TRAY_ICON, "Messages".to_string())
        };
        if let Ok(img) = Image::from_bytes(icon) {
            let _ = tray.set_icon(Some(img));
        }
        let _ = tray.set_tooltip(Some(tip));
    }
}

/// Google Messages puts the unread count in the tab title, e.g. "(3) Messages".
fn unread_from_title(title: &str) -> Option<u32> {
    let start = title.find('(')?;
    let rest = &title[start + 1..];
    let end = rest.find(')')?;
    rest[..end].trim().parse().ok()
}

// ---------------------------------------------------------------------------
// Window and tray
// ---------------------------------------------------------------------------

fn build_main_window<R: Runtime>(app: &AppHandle<R>, start_hidden: bool) -> tauri::Result<WebviewWindow<R>> {
    let nav_app = app.clone();
    let popup_app = app.clone();
    let url = Url::parse(START_URL).expect("valid start url");

    WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::External(url))
        .title("Messages")
        .inner_size(1100.0, 780.0)
        .min_inner_size(420.0, 520.0)
        .center()
        .visible(!start_hidden)
        .zoom_hotkeys_enabled(true)
        .initialization_script(INJECT_JS)
        .on_navigation(move |url| {
            if is_internal_host(url) {
                true
            } else {
                open_in_browser(&nav_app, url.as_str());
                false
            }
        })
        .on_new_window(move |url, _features| {
            // Links that try to open a new window go to the default browser.
            open_in_browser(&popup_app, url.as_str());
            NewWindowResponse::Deny
        })
        .on_document_title_changed(|window, title| {
            let count = unread_from_title(&title).unwrap_or(0);
            apply_unread(window.app_handle(), count);
        })
        .build()
}

fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayIcon<R>> {
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);

    let open = MenuItem::with_id(app, "open", "Open Messages", true, None::<&str>)?;
    let reload = MenuItem::with_id(app, "reload", "Reload", true, None::<&str>)?;
    let autostart =
        CheckMenuItem::with_id(app, "autostart", "Start with Windows", true, autostart_on, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?;
    let version = MenuItem::with_id(
        app,
        "version",
        format!("Version {}", app.package_info().version),
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[&open, &reload, &sep1, &autostart, &update, &version, &sep2, &quit],
    )?;

    let autostart_item = autostart.clone();
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(TRAY_ICON)?)
        .tooltip("Messages")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "reload" => {
                if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
                    let _ = w.reload();
                }
            }
            "autostart" => {
                let manager = app.autolaunch();
                let enabled = manager.is_enabled().unwrap_or(false);
                let _ = if enabled { manager.disable() } else { manager.enable() };
                let _ = autostart_item.set_checked(manager.is_enabled().unwrap_or(false));
            }
            "update" => updates::check_now(app.clone(), true),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        })
        .build(app)
}

pub fn run() {
    let start_hidden = std::env::args().any(|a| a == MINIMIZED_ARG);

    tauri::Builder::default()
        // Must be first: a second launch just brings the existing window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_ARG]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![notify, set_unread, open_external])
        .setup(move |app| {
            let handle = app.handle();
            let window = build_main_window(handle, start_hidden)?;
            if start_hidden {
                let _ = window.hide();
            }
            build_tray(handle)?;
            updates::start_background_checks(handle.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app running in the tray so messages still arrive.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == MAIN_WINDOW {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Messages");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unread_title() {
        assert_eq!(unread_from_title("(3) Messages"), Some(3));
        assert_eq!(unread_from_title("Messages (12)"), Some(12));
        assert_eq!(unread_from_title("Messages"), None);
        assert_eq!(unread_from_title("(x) Messages"), None);
    }

    #[test]
    fn internal_hosts() {
        let ok = |s: &str| is_internal_host(&Url::parse(s).unwrap());
        assert!(ok("https://messages.google.com/web/conversations"));
        assert!(ok("https://accounts.google.com/signin"));
        assert!(ok("https://www.gstatic.com/x.js"));
        assert!(!ok("https://example.com/"));
        assert!(!ok("http://messages.google.com/"));
        assert!(!ok("https://support.google.com/messages"));
        assert!(!ok("https://evilgoogle.com/"));
    }
}
