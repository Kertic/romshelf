use romshelf_core::db::Database;
use romshelf_core::emulators::Registry;
use romshelf_core::paths::Paths;
use romshelf_core::Result;
use std::sync::Mutex;

pub struct AppState {
    pub fake: bool,
    pub paths: Paths,
    pub db: Mutex<Database>,
    pub registry: Registry,
    /// Keeps fake mode's temp directory alive for the life of the app.
    _fake_root: Option<tempfile::TempDir>,
}

impl AppState {
    /// Real paths and database, or with `fake` a temp root seeded with the fixture library.
    pub fn new(fake: bool) -> Result<AppState> {
        let (paths, db, root) = if fake {
            let root = tempfile::tempdir()?;
            let paths = Paths::under(root.path());
            let db = Database::open(&paths.db_file())?;
            romshelf_testkit::fixtures::seed(&db)?;
            (paths, db, Some(root))
        } else {
            let paths = Paths::from_env()?;
            let db = Database::open(&paths.db_file())?;
            (paths, db, None)
        };
        Ok(AppState {
            fake,
            paths,
            db: Mutex::new(db),
            registry: Registry::default(),
            _fake_root: root,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use romshelf_core::library;

    #[test]
    fn fake_mode_serves_the_fixture_library() {
        let state = AppState::new(true).unwrap();
        let db = state.db.lock().unwrap();
        let entries = library::entries(&db, &state.registry).unwrap();
        assert_eq!(entries.len(), 8);
        assert!(state.paths.data_dir.exists());
    }
}
