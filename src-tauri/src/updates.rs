//! Self-update from GitHub Releases (signed with the key in tauri.conf.json).
//!
//! On Windows, installing an update runs the new installer in passive mode, which
//! closes and relaunches the app. To avoid yanking the window out from under you,
//! background checks only install while the window is hidden or not focused.

use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

use crate::{notify, MAIN_WINDOW};

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(15);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const BUSY_RETRY: Duration = Duration::from_secs(20 * 60);

fn user_is_busy<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(MAIN_WINDOW)
        .map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
        .unwrap_or(false)
}

pub fn start_background_checks<R: Runtime>(app: AppHandle<R>) {
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            let wait = match tauri::async_runtime::block_on(check_and_install(&app, false, false)) {
                Outcome::Deferred => BUSY_RETRY,
                _ => CHECK_INTERVAL,
            };
            std::thread::sleep(wait);
        }
    });
}

/// Run a check right away (from the tray menu). `manual` reports the result even when
/// there is nothing to install, and installs even if the window is focused.
pub fn check_now<R: Runtime>(app: AppHandle<R>, manual: bool) {
    tauri::async_runtime::spawn(async move {
        check_and_install(&app, manual, manual).await;
    });
}

enum Outcome {
    UpToDate,
    Deferred,
    Failed,
}

async fn check_and_install<R: Runtime>(app: &AppHandle<R>, manual: bool, force: bool) -> Outcome {
    let updater = match app.updater() {
        Ok(u) => u,
        Err(err) => return failed(app, manual, &err.to_string()),
    };
    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => {
            if manual {
                notify::info(
                    app,
                    "Messages is up to date",
                    &format!("You have the latest version ({}).", app.package_info().version),
                );
            }
            return Outcome::UpToDate;
        }
        Err(err) => return failed(app, manual, &err.to_string()),
    };

    if !force && user_is_busy(app) {
        return Outcome::Deferred;
    }

    notify::info(
        app,
        "Updating Messages",
        &format!("Installing version {}. Messages will restart in a moment.", update.version),
    );
    match update.download_and_install(|_, _| {}, || {}).await {
        // On Windows the installer takes over and this process exits.
        Ok(()) => {
            app.restart();
        }
        Err(err) => failed(app, manual, &err.to_string()),
    }
}

fn failed<R: Runtime>(app: &AppHandle<R>, manual: bool, err: &str) -> Outcome {
    eprintln!("update check failed: {err}");
    if manual {
        notify::info(app, "Couldn't check for updates", err);
    }
    Outcome::Failed
}
