//! The `romshelf` binary: argument parsing and the Tauri shell over `romshelf-core`.

mod auth_cli;
pub mod cli;
mod controls_cmd;
mod emulators_cli;
mod gamepad;
mod library_cli;
mod saves_cmd;
mod state;
mod settings_cmd;
mod steam_cli;
mod steam_cmd;
mod update_cli;

use clap::Parser;
use cli::{Cli, Command};
use romshelf_core::auth::open_browser;
use romshelf_core::clock::{Clock, SystemClock};
use romshelf_core::connection::{Connection, Folder, Status};
use romshelf_core::db::Database;
use romshelf_core::drive::scaffold;
use romshelf_core::drive::DriveClient;
use romshelf_core::emulators::setup::{self, EmulatorInfo, SetupContext, SetupEvent};
use romshelf_core::emulators::Registry;
use romshelf_core::launch::{self, LaunchContext, LaunchEvent, Step, Target};
use romshelf_core::library::{self, LibraryEntry};
use romshelf_core::model::RomId;
use romshelf_core::paths::Paths;
use romshelf_core::saves::{SaveChoice, SaveConflict};
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
        None => match open_state() {
            Ok(state) => run_interface(state, None),
            Err(code) => code,
        },
        Some(Command::Launch(args)) => {
            let target = match (args.id, args.rom) {
                (Some(id), _) => Target::Id(id),
                (None, Some(rom)) => Target::Rom(rom),
                (None, None) => unreachable!("clap requires --id or --rom"),
            };
            launch_command(target)
        }
        Some(Command::Doctor { id: Some(id) }) => doctor_launch(&id),
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
        Some(Command::Remove { id }) => {
            let setup =
                Paths::for_app().and_then(|paths| Ok((Database::open(&paths.db_file())?, paths)));
            match setup {
                Ok((db, paths)) => library_cli::remove(&db, &paths, &id, &mut std::io::stdout()),
                Err(e) => {
                    eprintln!("{}", UiError::from(e).message);
                    1
                }
            }
        }
        Some(Command::Emulators { setup }) => {
            let result = Paths::for_app().and_then(|paths| {
                let db = Database::open(&paths.db_file())?;
                let conn = Connection::real(&paths)?;
                let folder = conn.settings()?.drive;
                let http = romshelf_core::http::ReqwestTransport::new()?;
                let ctx = SetupContext {
                    runner: &romshelf_core::process::FlatpakRunner,
                    http: &http,
                    db: &db,
                    paths: &paths,
                    drive: folder
                        .as_ref()
                        .map(|f| (conn.drive.as_ref() as &dyn DriveClient, &f.layout)),
                };
                Ok(emulators_cli::run(
                    &ctx,
                    &Registry::default(),
                    setup.as_deref(),
                    &mut std::io::stdout(),
                ))
            });
            match result {
                Ok(code) => code,
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
        Some(Command::Steam(cmd)) => {
            let setup =
                Paths::for_app().and_then(|paths| Ok((Database::open(&paths.db_file())?, paths)));
            match setup {
                Ok((db, paths)) => {
                    let files = romshelf_core::steam::FsSteamFiles {
                        steam_root: paths.steam_root.clone(),
                    };
                    let ctx = romshelf_core::steam::apply::SteamContext {
                        db: &db,
                        files: &files,
                        paths: &paths,
                        now: SystemClock.now(),
                    };
                    steam_cli::run(cmd, &ctx, &mut std::io::stdout())
                }
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
    if status.state == "synced" {
        saves_cmd::flush_in_background(app);
        controls_cmd::pull_in_background(app);
        steam_cmd::after_sync(app);
    }
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
pub(crate) fn start_update_check(app: tauri::AppHandle) {
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

/// Removes the downloaded copy from this Deck. Drive is never touched.
#[tauri::command]
fn remove_from_deck(app: tauri::AppHandle, rom_id: String) -> Result<(), UiError> {
    let id = parse_rom_id(&rom_id)?;
    {
        let state = app.state::<AppState>();
        let db = state.db.lock().map_err(internal)?;
        transfer::remove_from_deck(&db, &state.paths, &id)?;
    }
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[derive(Serialize)]
struct StorageStatus {
    /// The folder holding `roms/`.
    root: String,
    available: bool,
    free: Option<u64>,
}

/// Runs `f` with a setup context on its own database connection, so a long Flatpak install
/// never holds the app's shared connection.
fn with_setup_ctx<T>(
    state: &AppState,
    f: impl FnOnce(&SetupContext) -> romshelf_core::Result<T>,
) -> romshelf_core::Result<T> {
    let db = Database::open(&state.paths.db_file())?;
    let folder = state.connection.settings()?.drive;
    let ctx = SetupContext {
        runner: state.runner.as_ref(),
        http: state.http.as_ref(),
        db: &db,
        paths: &state.paths,
        drive: folder
            .as_ref()
            .map(|f| (state.connection.drive.as_ref(), &f.layout)),
    };
    f(&ctx)
}

#[derive(Serialize)]
struct EmulatorView {
    #[serde(flatten)]
    info: EmulatorInfo,
    status_label: &'static str,
    /// Romshelf is setting it up right now.
    setting_up: bool,
}

/// Every emulator with its status, for the Emulators screen. Runs `flatpak info` per emulator.
#[tauri::command]
async fn emulators(app: tauri::AppHandle) -> Result<Vec<EmulatorView>, UiError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let busy = state.setups.lock().map(|s| s.clone()).unwrap_or_default();
        let list = with_setup_ctx(&state, |ctx| setup::list(ctx, &state.registry))?;
        Ok(list
            .into_iter()
            .map(|info| EmulatorView {
                status_label: info.status.label(),
                setting_up: busy.contains(&info.id),
                info,
            })
            .collect())
    })
    .await
    .map_err(internal)?
}

/// What the interface hears while an emulator is set up.
#[derive(Clone, Serialize)]
struct SetupUpdate {
    emulator_id: String,
    /// "step", "progress", "done" or "failed".
    kind: &'static str,
    step: Option<&'static str>,
    percent: Option<u8>,
    message: Option<String>,
}

/// Starts setting up an emulator in the background (`repair` runs every step again). Emits
/// `emulator-setup` updates, then
/// `library-changed`. Asking again while it runs does nothing.
#[tauri::command]
fn setup_emulator(app: tauri::AppHandle, emulator_id: String, repair: bool) -> Result<(), UiError> {
    let state = app.state::<AppState>();
    if state.registry.by_id(&emulator_id).is_none() {
        return Err(UiError {
            kind: "error",
            message: format!("There's no emulator called {emulator_id}."),
        });
    }
    {
        let mut busy = state.setups.lock().map_err(internal)?;
        if busy.contains(&emulator_id) {
            return Ok(());
        }
        busy.push(emulator_id.clone());
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let send = |kind, step, percent, message| {
            let _ = app.emit(
                "emulator-setup",
                SetupUpdate {
                    emulator_id: emulator_id.clone(),
                    kind,
                    step,
                    percent,
                    message,
                },
            );
        };
        let result = with_setup_ctx(&state, |ctx| {
            let adapter = state
                .registry
                .by_id(&emulator_id)
                .ok_or_else(|| Error::NotFound(emulator_id.clone()))?;
            if repair {
                setup::reset(ctx.db, adapter)?;
            }
            setup::run_setup(ctx, adapter, &mut |e| match e {
                SetupEvent::Step { step } => send("step", Some(step.label()), None, None),
                SetupEvent::Progress { percent } => send("progress", None, Some(percent), None),
            })
        });
        if let Ok(mut busy) = state.setups.lock() {
            busy.retain(|id| id != &emulator_id);
        }
        match result {
            Ok(report) => {
                let missing = report.summary();
                send("done", None, None, (!missing.is_empty()).then_some(missing));
            }
            Err(e) => send("failed", None, None, Some(UiError::from(e).message)),
        }
        let _ = app.emit("library-changed", ());
    });
    Ok(())
}

/// Brings the database's "installed" flags up to date (an emulator may have been installed or
/// removed outside Romshelf), then asks the interface to reread the library.
fn refresh_emulators(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        match with_setup_ctx(&state, |ctx| setup::list(ctx, &state.registry)) {
            Ok(_) => {
                let _ = app.emit("library-changed", ());
            }
            Err(e) => eprintln!("Couldn't check emulators: {e}"),
        }
    });
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

/// Runs `f` with a launch context built from the app state. `ask` answers a save conflict;
/// `no_answer` when there is no window to ask in.
fn with_launch_ctx<T>(
    state: &AppState,
    ask: &(dyn Fn(&SaveConflict) -> Option<SaveChoice> + Sync),
    f: impl FnOnce(&LaunchContext) -> romshelf_core::Result<T>,
) -> romshelf_core::Result<T> {
    let folder = state.connection.settings()?.drive;
    let ctx = LaunchContext {
        db: &state.db,
        registry: &state.registry,
        paths: &state.paths,
        drive: state.connection.drive.as_ref(),
        layout: folder.as_ref().map(|f| &f.layout),
        runner: state.runner.as_ref(),
        http: state.http.as_ref(),
        process: state.process.as_ref(),
        space: &FsDiskSpace,
        cancel: &state.cancel,
        clock: &SystemClock,
        retry_delay: DOWNLOAD_RETRY,
        max_attempts: DOWNLOAD_ATTEMPTS,
        device: &state.device,
        ask,
    };
    f(&ctx)
}

/// A launch with no window can't ask about a save conflict.
fn no_answer(_: &SaveConflict) -> Option<SaveChoice> {
    None
}

/// What the interface hears while a game is prepared and played.
#[derive(Clone, Default, Serialize)]
struct LaunchUpdate {
    rom_id: String,
    /// "step", "progress", "notice", "question", "finished" or "failed".
    kind: &'static str,
    step: Option<Step>,
    /// The step as the launch window shows it: "Downloading", "Setting up Dolphin".
    label: Option<String>,
    percent: Option<u8>,
    /// The emulator's exit code, when finished.
    code: Option<i32>,
    message: Option<String>,
    /// A save conflict to answer with `answer_save_question` ("question").
    conflict: Option<SaveConflict>,
}

/// Starts a game: downloads it and sets up its emulator if needed, checks its save, then runs
/// it. Returns at once; `launch-update` events follow. In the launch window, Romshelf exits
/// with the emulator's exit code when the game ends.
#[tauri::command]
fn play(app: tauri::AppHandle, rom_id: String) -> Result<(), UiError> {
    {
        let state = app.state::<AppState>();
        let mut playing = state.playing.lock().map_err(internal)?;
        if playing.is_some() {
            return Ok(());
        }
        *playing = Some(rom_id.clone());
    }
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let launching = state
            .launch_target
            .lock()
            .map(|t| t.is_some())
            .unwrap_or(false);
        let send = |u: LaunchUpdate| {
            let _ = app.emit(
                "launch-update",
                LaunchUpdate {
                    rom_id: rom_id.clone(),
                    ..u
                },
            );
        };
        // The launch waits here until the interface answers.
        let ask = |c: &SaveConflict| -> Option<SaveChoice> {
            let (tx, rx) = std::sync::mpsc::channel();
            *state.save_answer.lock().ok()? = Some(tx);
            send(LaunchUpdate {
                kind: "question",
                conflict: Some(c.clone()),
                ..LaunchUpdate::default()
            });
            rx.recv().ok()
        };
        let target = Target::Id(rom_id.clone());
        let result = with_launch_ctx(&state, &ask, |ctx| {
            let emulator = {
                let db = state
                    .db
                    .lock()
                    .map_err(|e| Error::External(e.to_string()))?;
                let rom = launch::resolve(&db, &target)?;
                state.registry.require(rom.system)?.name().to_string()
            };
            launch::run(ctx, &target, &mut |e| match e {
                LaunchEvent::Step { step } => {
                    // The game takes the screen; the launch window gets out of its way.
                    if launching && step == Step::Run {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.hide();
                        }
                    }
                    send(LaunchUpdate {
                        kind: "step",
                        step: Some(step),
                        label: Some(step.label(&emulator)),
                        ..LaunchUpdate::default()
                    })
                }
                LaunchEvent::Progress { percent } => send(LaunchUpdate {
                    kind: "progress",
                    percent: Some(percent),
                    ..LaunchUpdate::default()
                }),
                LaunchEvent::Notice { message } => send(LaunchUpdate {
                    kind: "notice",
                    message: Some(message),
                    ..LaunchUpdate::default()
                }),
            })
        });
        if let Ok(mut p) = state.playing.lock() {
            *p = None;
        }
        let _ = app.emit("library-changed", ());
        match result {
            Ok(code) => {
                send(LaunchUpdate {
                    kind: "finished",
                    code: Some(code),
                    ..LaunchUpdate::default()
                });
                if launching {
                    // Back to Steam, with the emulator's exit code.
                    app.exit(code);
                }
            }
            // Cancelled at the save question: the launch window has nothing left to do.
            Err(Error::Cancelled) if launching => app.exit(1),
            Err(e) => {
                if launching {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                    }
                }
                send(LaunchUpdate {
                    kind: "failed",
                    message: Some(UiError::from(e).message),
                    ..LaunchUpdate::default()
                });
            }
        }
    });
    Ok(())
}

/// The answer to the save question a launch is waiting on: "keep_deck", "use_drive" or
/// "cancel".
#[tauri::command]
fn answer_save_question(state: tauri::State<'_, AppState>, choice: SaveChoice) {
    if let Some(tx) = state.save_answer.lock().ok().and_then(|mut s| s.take()) {
        let _ = tx.send(choice);
    }
}

/// The game the launch window is for, if this is one.
#[tauri::command]
fn launch_target(state: tauri::State<'_, AppState>) -> Option<String> {
    state.launch_target.lock().ok().and_then(|t| t.clone())
}

/// `romshelf launch`: a game that is ready starts at once with no window; otherwise the launch
/// window shows the download or setup first, or asks about a save conflict. Exits with the
/// emulator's exit code; 2 when the game isn't known.
fn launch_command(target: Target) -> i32 {
    let state = match open_state() {
        Ok(s) => s,
        Err(code) => return code,
    };
    let failed = |e: Error| {
        let code = if matches!(e, Error::NotFound(_)) {
            2
        } else {
            1
        };
        eprintln!("{}", UiError::from(e).message);
        code
    };
    let needs_window =
        match with_launch_ctx(&state, &no_answer, |ctx| launch::needs_window(ctx, &target)) {
            Ok(n) => n,
            Err(e) => return failed(e),
        };
    // View + Menu held for a second quits the game, in any emulator.
    gamepad::watch_quit(state.process.clone(), state.paths.log_dir());
    if !needs_window {
        let result = with_launch_ctx(&state, &no_answer, |ctx| {
            launch::run(ctx, &target, &mut |_| {})
        });
        match result {
            // Only the window can ask which save to keep; nothing has started yet.
            Err(Error::NeedsAnswer) => {}
            other => return other.unwrap_or_else(failed),
        }
    }
    let rom_id = {
        let db = match state.db.lock() {
            Ok(db) => db,
            Err(e) => return failed(Error::External(e.to_string())),
        };
        match launch::resolve(&db, &target) {
            Ok(rom) => rom.id.to_string(),
            Err(e) => return failed(e),
        }
    };
    run_interface(state, Some(rom_id))
}

/// `romshelf doctor --id`: each step a launch would take, without doing any of it.
fn doctor_launch(id: &str) -> i32 {
    let state = match open_state() {
        Ok(s) => s,
        Err(code) => return code,
    };
    let target = Target::Id(id.to_string());
    match with_launch_ctx(&state, &no_answer, |ctx| launch::plan(ctx, &target)) {
        Ok(steps) => {
            println!("Launching {id} would:");
            for (i, s) in steps.iter().enumerate() {
                println!("  {}. {}", i + 1, s.detail);
            }
            0
        }
        Err(e) => {
            let code = if matches!(e, Error::NotFound(_)) {
                2
            } else {
                1
            };
            eprintln!("{}", UiError::from(e).message);
            code
        }
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

/// Opens Romshelf's data for the interface or a launch; on failure, the exit code.
fn open_state() -> Result<AppState, i32> {
    AppState::new(
        env_flag("ROMSHELF_FAKE"),
        env_flag("ROMSHELF_FAKE_FIRST_RUN"),
    )
    .map_err(|e| {
        eprintln!("Couldn't open Romshelf's data: {e}");
        1
    })
}

/// The full-screen interface, or with `launch` set, the launch window for that game.
fn run_interface(state: AppState, launch: Option<String>) -> i32 {
    #[cfg(target_os = "linux")]
    {
        // WebKitGTK's DMA-BUF renderer shows a blank window under gamescope on some drivers.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
    let launching = launch.is_some();
    // A launch from Steam already watches for View + Menu (see `launch_command`); the full
    // interface watches for games started with Play.
    let launch_quit = !launching;
    if let Ok(mut t) = state.launch_target.lock() {
        *t = launch;
    }
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
            remove_from_deck,
            storage_status,
            emulators,
            setup_emulator,
            play,
            answer_save_question,
            launch_target,
            saves_cmd::save_versions,
            saves_cmd::backup_save,
            saves_cmd::restore_save,
            saves_cmd::save_states,
            saves_cmd::delete_save_state,
            controls_cmd::controls_view,
            controls_cmd::save_controls,
            controls_cmd::reset_controls,
            controls_cmd::promote_count,
            controls_cmd::promote_controls,
            steam_cmd::steam_shortcut,
            steam_cmd::save_shortcut,
            steam_cmd::set_shortcut_artwork,
            steam_cmd::set_steam_auto_add,
            steam_cmd::steam_restart_notice,
            steam_cmd::image_files,
            settings_cmd::settings_view,
            settings_cmd::set_save_versions,
            settings_cmd::sign_out,
            settings_cmd::change_storage,
            settings_cmd::check_for_updates,
            settings_cmd::diagnostics,
            settings_cmd::transfers,
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
            if launch_quit {
                let s = app.state::<AppState>();
                gamepad::watch_quit(s.process.clone(), s.paths.log_dir());
            }
            // A launch window is only there to start one game.
            if !launching {
                start_update_check(app.handle().clone());
            }
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
            if !launching {
                refresh_emulators(app.handle());
                saves_cmd::flush_in_background(app.handle());
            }
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
