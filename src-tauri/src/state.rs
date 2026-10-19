use romshelf_core::connection::Connection;
use romshelf_core::db::Database;
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::emulators::Registry;
use romshelf_core::paths::Paths;
use romshelf_core::Result;
use romshelf_testkit::connection::{simulate_browser, FakeConnection};
use romshelf_testkit::FakeHttp;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    /// Keeps fake mode's temp directory alive for the life of the app.
    _fake_root: Option<tempfile::TempDir>,
}

impl AppState {
    /// Real paths, database and Google connection, or with `fake` a temp root seeded with the
    /// fixture library and a fake Drive. `fake_first_run` starts fake mode signed out.
    pub fn new(fake: bool, fake_first_run: bool) -> Result<AppState> {
        if !fake {
            let paths = Paths::from_env()?;
            let db = Database::open(&paths.db_file())?;
            let connection = Connection::real(&paths)?;
            return Ok(AppState {
                fake,
                db: Mutex::new(db),
                registry: Registry::default(),
                connection: Arc::new(connection),
                login_cancel: Arc::new(AtomicBool::new(false)),
                fake_http: None,
                paths,
                _fake_root: None,
            });
        }

        let root = tempfile::tempdir()?;
        let paths = Paths::under(root.path());
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
            fake_conn
                .connection
                .setup_folder(ROOT, "My Drive", &mut |_: &str| {})?;
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
            fake_http: Some(http),
            _fake_root: Some(root),
        })
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
