//! The `romshelf` binary: argument parsing and the Tauri shell over `romshelf-core`.

mod auth_cli;
pub mod cli;
mod gamepad;
mod library_cli;
mod state;
mod update_cli;

use clap::Parser;
use cli::{Cli, Command};
use romshelf_core::auth::open_browser;
use romshelf_core::clock::{Clock, SystemClock};
use romshelf_core::connection::{Connection, Folder, Status};
use romshelf_core::db::Database;
use romshelf_core::drive::scaffold;
use romshelf_core::library::{self, LibraryEntry};
use romshelf_core::model::RomId;
use romshelf_core::paths::Paths;
use romshelf_core::storage::{self, DiskSpace, FsDiskSpace};
use romshelf_core::transfer::{self, Downloader};
use romshelf_core::update::{self, Outcome};
use romshelf_core::Error;
use romshelf_testkit::connection::{script_login, simulate_browser};
use serde::Serialize;
use state::{AppState, SyncStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// A dropped connection is retried after 2 s, doubling to 30 s, 12 times in all (about
/// four minutes), resuming from what was already downloaded.
const DOWNLOAD_RETRY: Duration = Duration::from_secs(2);
const DOWNLOAD_ATTEMPTS: u32 = 12;

/// How long the interface waits for the browser sign-in.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Parses arguments and runs; returns the process exit code.
pub fn run() -> i32 {
    let cli = Cli::parse();
    match cli.command {
        None => run_interface(),
        Some(Command::Auth(cmd)) => {
            let conn = match Paths::for_app().and_then(|p| Connection::real(&p)) {
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
        Some(Command::Update(args)) => {
            let paths = match Paths::for_app() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Couldn't find Romshelf's folders: {e}");
                    return 1;
                }
            };
            let tokens = romshelf_core::connection::real_token_store(&paths);
            let updater = match update::for_running_appimage(&paths, &tokens) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };
            update_cli::run(
                args,
                updater.as_ref(),
                &tokens,
                &mut std::io::stdin().lock(),
                &mut std::io::stdout(),
            )
        }
        Some(Command::Sync) => {
            let result = Paths::for_app().and_then(|paths| {
                let conn = Connection::real(&paths)?;
                let db = Database::open(&paths.db_file())?;
                conn.sync(&db, &paths)
            });
            match result {
                Ok(report) => {
                    println!("{}", report.summary());
                    0
                }
                Err(e) => {
                    eprintln!("{}", UiError::from(e).message);
                    1
                }
            }
        }
        Some(Command::Download { id }) => {
            let setup = Paths::for_app().and_then(|paths| {
                let conn = Connection::real(&paths)?;
                let db = Database::open(&paths.db_file())?;
                Ok((paths, conn, db))
            });
            let (paths, conn, db) = match setup {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}", UiError::from(e).message);
                    return 1;
                }
            };
            let cancel = AtomicBool::new(false);
            let dl = Downloader {
                drive: conn.drive.as_ref(),
                paths: &paths,
                space: &FsDiskSpace,
                cancel: &cancel,
                retry_delay: DOWNLOAD_RETRY,
                max_attempts: DOWNLOAD_ATTEMPTS,
            };
            library_cli::download(
                &Mutex::new(db),
                &dl,
                &id,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            )
        }
        Some(Command::Delete(args)) => {
            let setup = Paths::for_app().and_then(|paths| {
                let conn = Connection::real(&paths)?;
                let db = Database::open(&paths.db_file())?;
                Ok((paths, conn, db))
            });
            match setup {
                Ok((paths, conn, db)) => library_cli::delete(
                    &db,
                    conn.drive.as_ref(),
                    &paths,
                    &args.id,
                    &args.scope,
                    !args.delete_save_backups,
                    &mut std::io::stdout(),
                ),
                Err(e) => {
                    eprintln!("{}", UiError::from(e).message);
                    1
                }
            }
        }
        Some(Command::Storage { set }) => {
            let setup =
                Paths::for_app().and_then(|paths| Ok((Database::open(&paths.db_file())?, paths)));
            match setup {
                Ok((db, paths)) => library_cli::storage(
                    &db,
                    &paths,
                    &FsDiskSpace,
                    set.as_deref(),
                    &mut std::io::stdout(),
                ),
                Err(e) => {
                    eprintln!("{}", UiError::from(e).message);
                    1
                }
            }
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
fn sync_status(state: tauri::State<'_, AppState>) -> SyncStatus {
    state
        .sync_status
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

/// Syncs the library with Drive. Emits `sync-status` as it starts and ends, and
/// `library-changed` when the index changed. A second call waits for the first.
#[tauri::command]
async fn sync_now(app: tauri::AppHandle) -> Result<SyncStatus, UiError> {
    tauri::async_runtime::spawn_blocking(move || run_sync(&app))
        .await
        .map_err(internal)
}

fn set_sync_status(app: &tauri::AppHandle, status: SyncStatus) {
    if let Ok(mut s) = app.state::<AppState>().sync_status.lock() {
        *s = status.clone();
    }
    let _ = app.emit("sync-status", status);
}

fn run_sync(app: &tauri::AppHandle) -> SyncStatus {
    let state = app.state::<AppState>();
    let _one_at_a_time = state.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
    let previous = state
        .sync_status
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    set_sync_status(
        app,
        SyncStatus {
            state: "syncing",
            ..previous.clone()
        },
    );
    let result = match state.db.lock() {
        Ok(db) => state.connection.sync(&db, &state.paths),
        Err(e) => Err(Error::External(e.to_string())),
    };
    let status = match result {
        Ok(report) => {
            if report.changed() || report.full_scan {
                let _ = app.emit("library-changed", ());
            }
            SyncStatus {
                state: "synced",
                last_synced_at: Some(SystemClock.now()),
                message: Some(report.summary()),
            }
        }
        Err(e) => {
            let err = UiError::from(e);
            SyncStatus {
                state: if err.kind == "offline" {
                    "offline"
                } else {
                    "error"
                },
                last_synced_at: previous.last_synced_at,
                message: Some(err.message),
            }
        }
    };
    set_sync_status(app, status.clone());
    status
}

/// The version downloaded in the background this session, if any. Runs from the next start.
#[tauri::command]
fn update_ready(state: tauri::State<'_, AppState>) -> Option<String> {
    state.update_ready.lock().ok().and_then(|v| v.clone())
}

/// Checks for a newer build once a day, in the background, and installs it in place.
/// Emits `update-ready` with the version when one is in place.
fn start_update_check(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    if state.fake {
        return;
    }
    let paths = state.paths.clone();
    std::thread::spawn(move || {
        let tokens = romshelf_core::connection::real_token_store(&paths);
        let result = update::for_running_appimage(&paths, &tokens)
            .and_then(|u| u.map(|u| u.run(false)).transpose());
        match result {
            Ok(Some(Outcome::Updated(v))) => {
                let v = v.to_string();
                if let Ok(mut slot) = app.state::<AppState>().update_ready.lock() {
                    *slot = Some(v.clone());
                }
                let _ = app.emit("update-ready", v);
            }
            Ok(_) => {}
            // Offline or GitHub unavailable: try again at the next start.
            Err(e) => eprintln!("Update check failed: {e}"),
        }
    });
}

/// Runs queued downloads on a background thread, if one isn't already. Emits `transfer`
/// events, and `library-changed` when a download ends.
fn start_downloads(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if state.worker_busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        loop {
            let dl = Downloader {
                drive: state.connection.drive.as_ref(),
                paths: &state.paths,
                space: &FsDiskSpace,
                cancel: &state.cancel,
                retry_delay: DOWNLOAD_RETRY,
                max_attempts: DOWNLOAD_ATTEMPTS,
            };
            let _ = transfer::run_pending(&state.db, &dl, &|event| {
                let finished = matches!(event, transfer::Event::Finished { .. });
                let _ = app.emit("transfer", event);
                if finished {
                    let _ = app.emit("library-changed", ());
                }
            });
            state.worker_busy.store(false, Ordering::SeqCst);
            // Something queued between the last check and clearing the flag gets a new run.
            let more = state
                .db
                .lock()
                .map(|db| {
                    transfer::pending(&db)
                        .map(|p| !p.is_empty())
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if !more || state.worker_busy.swap(true, Ordering::SeqCst) {
                return;
            }
        }
    });
}

fn parse_rom_id(id: &str) -> Result<RomId, UiError> {
    RomId::parse(id).ok_or_else(|| UiError {
        kind: "error",
        message: format!("Romshelf doesn't know the game {id}. Sync, then try again."),
    })
}

/// Queues a download (or "Download again") and starts the worker.
#[tauri::command]
fn download(app: tauri::AppHandle, rom_id: String) -> Result<(), UiError> {
    let id = parse_rom_id(&rom_id)?;
    {
        let state = app.state::<AppState>();
        let db = state.db.lock().map_err(internal)?;
        transfer::enqueue_download(&db, &id)?;
    }
    let _ = app.emit("library-changed", ());
    start_downloads(&app);
    Ok(())
}

/// Stops a running download, or takes a waiting one off the queue.
#[tauri::command]
fn cancel_download(app: tauri::AppHandle, rom_id: String) -> Result<(), UiError> {
    let id = parse_rom_id(&rom_id)?;
    let state = app.state::<AppState>();
    let db = state.db.lock().map_err(internal)?;
    match transfer::state_of(&db, &id)? {
        Some(transfer::TransferState::Running) => state.cancel.store(true, Ordering::SeqCst),
        _ => {
            transfer::dequeue(&db, &state.paths, &id)?;
            let _ = app.emit("library-changed", ());
        }
    }
    Ok(())
}

/// Deletes from this Deck, from Drive (to Drive's trash) or both.
#[tauri::command]
async fn delete_rom(
    app: tauri::AppHandle,
    rom_id: String,
    scope: transfer::DeleteScope,
    keep_save_backups: bool,
) -> Result<(), UiError> {
    let id = parse_rom_id(&rom_id)?;
    let done = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db.lock().map_err(internal)?;
        transfer::delete(
            &db,
            state.connection.drive.as_ref(),
            &state.paths,
            &id,
            scope,
            keep_save_backups,
        )
        .map_err(UiError::from)
    })
    .await
    .map_err(internal)??;
    let _ = done.emit("library-changed", ());
    Ok(())
}

#[derive(Serialize)]
struct StorageStatus {
    /// The folder holding `roms/`.
    root: String,
    available: bool,
    free: Option<u64>,
}

#[tauri::command]
fn storage_status(state: tauri::State<'_, AppState>) -> StorageStatus {
    let root = state.paths.storage_root();
    let available = storage::is_available(&state.paths);
    StorageStatus {
        free: available
            .then(|| FsDiskSpace.available(&root).ok())
            .flatten(),
        root: root.display().to_string(),
        available,
    }
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
            update_ready,
            sync_status,
            sync_now,
            download,
            cancel_download,
            delete_rom,
            storage_status,
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
            start_update_check(app.handle().clone());
            // Correct ROM locations against the disk, then resume any downloads.
            {
                let state = app.state::<AppState>();
                let result = match state.db.lock() {
                    Ok(db) => storage::reconcile(&db, &state.paths).map(|_| ()),
                    Err(_) => Ok(()),
                };
                if let Err(e) = result {
                    eprintln!("Couldn't check downloaded ROMs: {e}");
                }
            }
            start_downloads(app.handle());
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
