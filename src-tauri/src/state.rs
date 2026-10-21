use romshelf_core::connection::Connection;
use romshelf_core::db::Database;
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::emulators::setup::{self, SetupStep};
use romshelf_core::emulators::{EmulatorAdapter, Registry, RetroArch};
use romshelf_core::http::{HttpTransport, ReqwestTransport};
use romshelf_core::paths::Paths;
use romshelf_core::process::CommandSpec;
use romshelf_core::process::{FlatpakRunner, PackageRunner, ProcessRunner, SystemProcessRunner};
use romshelf_core::saves::{self, SaveChoice};
use romshelf_core::Result;
use romshelf_testkit::connection::{simulate_browser, FakeConnection};
use romshelf_testkit::{FakeHttp, FakePackageRunner, FakeProcessRunner};
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The last sync as the interface shows it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    /// "idle" before the first sync, then "syncing", "synced", "offline" or "error".
    pub state: &'static str,
    /// Seconds since the epoch of the last successful sync.
    pub last_synced_at: Option<i64>,
    /// The summary line, or what went wrong.
    pub message: Option<String>,
}

pub struct AppState {
    pub fake: bool,
    pub paths: Paths,
    pub db: Mutex<Database>,
    pub registry: Registry,
    pub connection: Arc<Connection>,
    /// Set to stop a sign-in that is waiting for the browser.
    pub login_cancel: Arc<AtomicBool>,
    /// Fake mode only: where the fake sign-in's Google answers are scripted.
    pub fake_http: Option<Arc<FakeHttp>>,
    /// A newer version installed in the background this session.
    pub update_ready: Mutex<Option<String>>,
    pub sync_status: Mutex<SyncStatus>,
    /// Held while a sync runs, so two never overlap.
    pub sync_lock: Mutex<()>,
    /// Stops the download under way.
    pub cancel: Arc<AtomicBool>,
    /// A download worker thread is running.
    pub worker_busy: AtomicBool,
    /// Runs `flatpak` to detect and install emulators.
    pub runner: Arc<dyn PackageRunner>,
    /// Starts emulators.
    pub process: Arc<dyn ProcessRunner>,
    /// A game is running (or being prepared) from this app; Play waits for it.
    pub playing: Mutex<Option<String>>,
    /// The game `romshelf launch` opened the launch window for.
    pub launch_target: Mutex<Option<String>>,
    /// Fetches RetroArch cores.
    pub http: Arc<dyn HttpTransport>,
    /// Emulator ids whose setup is running.
    pub setups: Mutex<Vec<String>>,
    /// This machine's name in save backups.
    pub device: String,
    /// Where the answer to a save conflict goes while a launch waits for it.
    pub save_answer: Mutex<Option<std::sync::mpsc::Sender<SaveChoice>>>,
    /// Keeps fake mode's temp directory alive for the life of the app.
    _fake_root: Option<tempfile::TempDir>,
}

impl AppState {
    /// Real paths, database and Google connection, or with `fake` a temp root seeded with the
    /// fixture library and a fake Drive. `fake_first_run` starts fake mode signed out.
    pub fn new(fake: bool, fake_first_run: bool) -> Result<AppState> {
        if !fake {
            let paths = Paths::for_app()?;
            let db = Database::open(&paths.db_file())?;
            let connection = Connection::real(&paths)?;
            return Ok(AppState {
                fake,
                db: Mutex::new(db),
                registry: Registry::default(),
                connection: Arc::new(connection),
                login_cancel: Arc::new(AtomicBool::new(false)),
                update_ready: Mutex::new(None),
                sync_status: Mutex::new(SyncStatus {
                    state: "idle",
                    ..SyncStatus::default()
                }),
                sync_lock: Mutex::new(()),
                cancel: Arc::new(AtomicBool::new(false)),
                worker_busy: AtomicBool::new(false),
                runner: Arc::new(FlatpakRunner),
                process: Arc::new(SystemProcessRunner::default()),
                playing: Mutex::new(None),
                launch_target: Mutex::new(None),
                http: Arc::new(ReqwestTransport::new()?),
                setups: Mutex::new(Vec::new()),
                device: saves::device_name(),
                save_answer: Mutex::new(None),
                fake_http: None,
                paths,
                _fake_root: None,
            });
        }

        let root = tempfile::tempdir()?;
        let paths = Paths::under(root.path());
        // A Steam account to write shortcuts to.
        std::fs::create_dir_all(paths.steam_root.join("userdata/12345/config"))?;
        let db = Database::open(&paths.db_file())?;
        romshelf_testkit::fixtures::seed(&db)?;
        let fake_conn = FakeConnection::new(paths.settings_file());
        // A few folders so the location picker has something to browse.
        let games = fake_conn.drive.create_folder("Games", ROOT)?;
        fake_conn.drive.create_folder("Retro", &games.id)?;
        fake_conn.drive.create_folder("Photos", ROOT)?;
        fake_conn.drive.create_folder("Work", ROOT)?;
        if !fake_first_run {
            fake_conn.script_login();
            fake_conn.connection.login(
                &simulate_browser,
                Duration::from_secs(5),
                &AtomicBool::new(false),
            )?;
            let folder = fake_conn
                .connection
                .setup_folder(ROOT, "My Drive", &mut |_: &str| {})?;
            // The fixture ROMs as files in the fake Drive, so syncing finds what is indexed.
            for rom in romshelf_testkit::fixtures::roms() {
                fake_conn.drive.create_file_with_id(
                    &rom.drive_file_id,
                    &rom.file_name,
                    &folder.layout.rom_folders[rom.system.id()],
                    &vec![0; rom.size as usize],
                )?;
            }
        }
        // RetroArch is installed and set up (cores in place); Dolphin is not, so its setup
        // can be tried. The simulated Flatpak installs with visible progress.
        let mut runner = FakePackageRunner::simulating(&[RetroArch.flatpak_id()]);
        // A "game" that runs for five seconds and exits normally.
        let fake_process = FakeProcessRunner::default();
        for _ in 0..100 {
            fake_process.push_exit(0, Duration::from_secs(5));
        }
        // ...and writes a save, so backups can be seen.
        fake_process.during_run(write_fake_save);
        runner.step_delay = Duration::from_millis(300);
        for core in RetroArch.required_cores() {
            let file = RetroArch::core_file(&paths, core);
            std::fs::create_dir_all(file.parent().unwrap_or(&paths.flatpak_root))?;
            std::fs::write(file, b"ELF fake core")?;
        }
        for step in SetupStep::ALL {
            setup::record_step(&db, &RetroArch, step)?;
        }
        let FakeConnection {
            connection, http, ..
        } = fake_conn;
        Ok(AppState {
            fake,
            paths,
            db: Mutex::new(db),
            registry: Registry::default(),
            connection: Arc::new(connection),
            login_cancel: Arc::new(AtomicBool::new(false)),
            update_ready: Mutex::new(None),
            sync_status: Mutex::new(SyncStatus {
                state: "idle",
                ..SyncStatus::default()
            }),
            sync_lock: Mutex::new(()),
            cancel: Arc::new(AtomicBool::new(false)),
            worker_busy: AtomicBool::new(false),
            runner: Arc::new(runner),
            process: Arc::new(fake_process),
            playing: Mutex::new(None),
            launch_target: Mutex::new(None),
            http: Arc::new(FakeHttp::default()),
            setups: Mutex::new(Vec::new()),
            device: "fake-deck".into(),
            save_answer: Mutex::new(None),
            fake_http: Some(http),
            _fake_root: Some(root),
        })
    }
}

/// Fake mode's "emulator" saves where Romshelf told it to: RetroArch's game config names the
/// folder, Dolphin's memory-card folder is on its command line.
fn write_fake_save(cmd: &CommandSpec) {
    let arg_after = |flag: &str| {
        cmd.args
            .iter()
            .position(|a| a == flag)
            .and_then(|i| cmd.args.get(i + 1))
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let target = if let Some(configs) = arg_after("--appendconfig") {
        configs.split('|').nth(1).and_then(|cfg| {
            let text = std::fs::read_to_string(cfg).ok()?;
            let line = text.lines().find(|l| l.starts_with("savefile_directory"))?;
            let dir = line.split('"').nth(1)?;
            Some(std::path::Path::new(dir).join("game.srm"))
        })
    } else {
        cmd.args
            .iter()
            .find_map(|a| a.strip_prefix("Dolphin.Core.GCIFolderAPathOverride="))
            .map(|dir| std::path::Path::new(dir).join("01-FAKE-save.gci"))
    };
    if let Some(file) = target {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, format!("fake progress at {stamp}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use romshelf_core::library;

    #[test]
    fn fake_mode_serves_the_fixture_library() {
        let state = AppState::new(true, false).unwrap();
        let db = state.db.lock().unwrap();
        let entries = library::entries(&db, &state.registry).unwrap();
        assert_eq!(entries.len(), 8);
        assert!(state.paths.data_dir.exists());
        assert!(state.connection.status().unwrap().ready());
    }

    #[test]
    fn fake_mode_syncs_its_fixture_drive_without_changes() {
        let state = AppState::new(true, false).unwrap();
        let db = state.db.lock().unwrap();
        let report = state.connection.sync(&db, &state.paths).unwrap();
        assert_eq!(report.total, 8);
        assert_eq!(report.added + report.updated + report.gone, 0, "{report:?}");
        let entries = library::entries(&db, &state.registry).unwrap();
        assert_eq!(entries.len(), 8);
        assert_eq!(entries.iter().filter(|e| e.in_steam).count(), 3);
    }

    #[test]
    fn fake_first_run_starts_signed_out_with_folders_to_browse() {
        let state = AppState::new(true, true).unwrap();
        let status = state.connection.status().unwrap();
        assert!(status.client_configured);
        assert!(!status.signed_in);
        let names: Vec<_> = state
            .connection
            .list_folders(ROOT)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, ["Games", "Photos", "Work"]);
    }
}
