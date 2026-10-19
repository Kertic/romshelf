//! The `romshelf` binary: argument parsing and the Tauri shell over `romshelf-core`.

mod auth_cli;
pub mod cli;
mod gamepad;
mod state;

use clap::Parser;
use cli::{Cli, Command};
use romshelf_core::auth::open_browser;
use romshelf_core::connection::{Connection, Folder, Status};
use romshelf_core::drive::scaffold;
use romshelf_core::library::{self, LibraryEntry};
use romshelf_core::paths::Paths;
use romshelf_core::Error;
use romshelf_testkit::connection::{script_login, simulate_browser};
use serde::Serialize;
use state::AppState;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// How long the interface waits for the browser sign-in.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Parses arguments and runs; returns the process exit code.
pub fn run() -> i32 {
    let cli = Cli::parse();
    match cli.command {
        None => run_interface(),
        Some(Command::Auth(cmd)) => {
            let conn = match Paths::from_env().and_then(|p| Connection::real(&p)) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Couldn't open Romshelf's settings: {e}");
                    return 1;
                }
            };
            auth_cli::run(
                cmd,
                &conn,
                &open_browser,
                &mut std::io::stdin().lock(),
                &mut std::io::stdout(),
            )
        }
        Some(cmd) => {
            eprintln!(
                "romshelf {}: not implemented yet in this build.",
                cmd.name()
            );
            2
        }
    }
}

/// An error as the interface shows it: a kind to branch on and a sentence for the user.
#[derive(Debug, Serialize)]
struct UiError {
    kind: &'static str,
    message: String,
}

impl From<Error> for UiError {
    fn from(e: Error) -> UiError {
        let kind = match e {
            Error::NoBrowser => "no_browser",
            Error::Cancelled => "cancelled",
            Error::Timeout(_) => "timeout",
            Error::Network(_) => "offline",
            Error::Auth(_) => "auth",
            Error::NeedsUpdate(_) => "needs_update",
            _ => "error",
        };
        let message = match e {
            Error::Network(_) => {
                "Couldn't reach Google. Check the Deck's internet connection and try again.".into()
            }
            other => other.to_string(),
        };
        UiError { kind, message }
    }
}

fn internal(e: impl std::fmt::Display) -> UiError {
    UiError {
        kind: "error",
        message: format!("Something went wrong inside Romshelf: {e}. Try again."),
    }
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    fake: bool,
    data_dir: String,
}

#[tauri::command]
fn app_info(state: tauri::State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        fake: state.fake,
        data_dir: state.paths.data_dir.display().to_string(),
    }
}

#[tauri::command]
fn library(state: tauri::State<'_, AppState>) -> Result<Vec<LibraryEntry>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    library::entries(&db, &state.registry).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct ConnectionStatus {
    #[serde(flatten)]
    status: Status,
    ready: bool,
}

#[tauri::command]
fn connection_status(state: tauri::State<'_, AppState>) -> Result<ConnectionStatus, UiError> {
    let status = state.connection.status()?;
    Ok(ConnectionStatus {
        ready: status.ready(),
        status,
    })
}

/// Opens the browser and waits for Google's redirect. Returns the account's address.
#[tauri::command]
async fn auth_login(state: tauri::State<'_, AppState>) -> Result<String, UiError> {
    state.login_cancel.store(false, Ordering::SeqCst);
    let conn = state.connection.clone();
    let cancel = state.login_cancel.clone();
    let fake_http = state.fake_http.clone();
    tauri::async_runtime::spawn_blocking(move || match fake_http {
        Some(http) => {
            script_login(&http);
            // Long enough to see "Waiting for sign-in…".
            let open = |url: &str| {
                std::thread::sleep(Duration::from_millis(1500));
                simulate_browser(url)
            };
            conn.login(&open, LOGIN_TIMEOUT, &cancel)
        }
        None => conn.login(&open_browser, LOGIN_TIMEOUT, &cancel),
    })
    .await
    .map_err(internal)?
    .map(|account| account.email)
    .map_err(UiError::from)
}

#[tauri::command]
fn auth_cancel(state: tauri::State<'_, AppState>) {
    state.login_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn drive_folders(
    state: tauri::State<'_, AppState>,
    parent: String,
) -> Result<Vec<Folder>, UiError> {
    let conn = state.connection.clone();
    tauri::async_runtime::spawn_blocking(move || conn.list_folders(&parent))
        .await
        .map_err(internal)?
        .map_err(UiError::from)
}

#[derive(Serialize)]
struct SetupDone {
    path: String,
    tree: Vec<String>,
}

/// Creates the folder tree under `parent`, emitting `setup-progress` with each folder's path.
#[tauri::command]
async fn drive_setup(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    parent: String,
    location: String,
) -> Result<SetupDone, UiError> {
    let conn = state.connection.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut progress = |path: &str| {
            let _ = app.emit("setup-progress", path);
        };
        conn.setup_folder(&parent, &location, &mut progress)
    })
    .await
    .map_err(internal)?
    .map(|folder| SetupDone {
        path: folder.display_path(),
        tree: scaffold::tree_lines(),
    })
    .map_err(UiError::from)
}

#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

fn run_interface() -> i32 {
    #[cfg(target_os = "linux")]
    {
        // WebKitGTK's DMA-BUF renderer shows a blank window under gamescope on some drivers.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    let state = match AppState::new(
        env_flag("ROMSHELF_FAKE"),
        env_flag("ROMSHELF_FAKE_FIRST_RUN"),
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Couldn't open Romshelf's data: {e}");
            return 1;
        }
    };
    // Full screen unless asked otherwise; debug builds default to a window for development.
    let fullscreen = if env_flag("ROMSHELF_WINDOWED") {
        false
    } else {
        !cfg!(debug_assertions) || env_flag("ROMSHELF_FULLSCREEN")
    };

    let result = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            app_info,
            library,
            connection_status,
            auth_login,
            auth_cancel,
            drive_folders,
            drive_setup,
            quit
        ])
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Romshelf")
                .inner_size(1280.0, 800.0)
                .fullscreen(fullscreen)
                .focused(true)
                .build()?;
            gamepad::start(app.handle().clone());
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Romshelf couldn't start its window: {e}");
            1
        }
    }
}
