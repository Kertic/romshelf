//! The Google connection as the CLI and interface see it: client, sign-in, account and the
//! chosen Drive folder, with the local settings kept in step.

use crate::auth::{ClientConfig, GoogleAuth};
use crate::clock::SystemClock;
use crate::db::Database;
use crate::drive::google::GoogleDrive;
use crate::drive::scaffold;
use crate::drive::{Account, DriveClient};
use crate::http::ReqwestTransport;
use crate::paths::Paths;
use crate::settings::{DriveFolder, LocalSettings};
use crate::sync::{self, SyncContext, SyncReport};
use crate::tokens::{FallbackTokenStore, FileTokenStore, KeyringTokenStore};
use crate::{Error, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

/// Keyring service name.
pub const KEYRING_SERVICE: &str = "romshelf";

/// The keyring, falling back silently to the private token file.
pub fn real_token_store(paths: &Paths) -> FallbackTokenStore<KeyringTokenStore, FileTokenStore> {
    FallbackTokenStore {
        primary: KeyringTokenStore {
            service: KEYRING_SERVICE.into(),
        },
        fallback: FileTokenStore {
            path: paths.token_file(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub client_configured: bool,
    pub signed_in: bool,
    pub account: Option<String>,
    /// "My Drive / Games / Emulation"
    pub drive_folder: Option<String>,
}

impl Status {
    /// Signed in with a Drive folder chosen: first run is done.
    pub fn ready(&self) -> bool {
        self.signed_in && self.drive_folder.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
}

pub struct Connection {
    pub auth: Arc<GoogleAuth>,
    pub drive: Arc<dyn DriveClient>,
    settings_path: PathBuf,
}

impl Connection {
    pub fn new(auth: Arc<GoogleAuth>, drive: Arc<dyn DriveClient>, settings_path: PathBuf) -> Self {
        Connection {
            auth,
            drive,
            settings_path,
        }
    }

    /// Google over HTTPS, tokens in the keyring or the private token file.
    pub fn real(paths: &Paths) -> Result<Connection> {
        let http = Arc::new(ReqwestTransport::new()?);
        let tokens = Arc::new(real_token_store(paths));
        let auth = Arc::new(GoogleAuth::new(http.clone(), tokens, Arc::new(SystemClock)));
        let drive = Arc::new(GoogleDrive::new(auth.clone(), http));
        Ok(Connection::new(auth, drive, paths.settings_file()))
    }

    pub fn settings(&self) -> Result<LocalSettings> {
        LocalSettings::load(&self.settings_path)
    }

    pub fn status(&self) -> Result<Status> {
        let settings = self.settings()?;
        let signed_in = self.auth.signed_in()?;
        Ok(Status {
            client_configured: self.auth.has_client()?,
            signed_in,
            account: settings.account.filter(|_| signed_in),
            drive_folder: settings.drive.map(|d| d.display_path()),
        })
    }

    pub fn set_client(&self, client: &ClientConfig) -> Result<()> {
        self.auth.set_client(client)
    }

    /// Browser sign-in, then records the account. Signing in as a different account forgets
    /// the previous account's Drive folder.
    pub fn login(
        &self,
        open: &dyn Fn(&str) -> Result<()>,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<Account> {
        self.auth.login(open, timeout, cancel)?;
        let account = self.drive.about()?;
        let mut settings = self.settings()?;
        if settings.account.as_deref() != Some(account.email.as_str()) {
            settings.drive = None;
        }
        settings.account = Some(account.email.clone());
        settings.save(&self.settings_path)?;
        Ok(account)
    }

    /// Signs out. The client and the Drive folder stay, so signing in again as the same
    /// account needs no new setup.
    pub fn logout(&self) -> Result<()> {
        self.auth.logout()
    }

    /// Sub-folders of `parent` (`root` for My Drive), sorted by name.
    pub fn list_folders(&self, parent: &str) -> Result<Vec<Folder>> {
        let mut folders: Vec<Folder> = self
            .drive
            .list(parent)?
            .into_iter()
            .filter(|f| f.is_folder())
            .map(|f| Folder {
                id: f.id,
                name: f.name,
            })
            .collect();
        folders.sort_by_key(|f| (f.name.to_lowercase(), f.id.clone()));
        Ok(folders)
    }

    /// Brings the library index up to date with the chosen Drive folder.
    pub fn sync(&self, db: &Database, paths: &Paths) -> Result<SyncReport> {
        let Some(folder) = self.settings()?.drive else {
            return Err(Error::NotFound(
                "No Drive folder is chosen yet. Open Romshelf to choose one.".into(),
            ));
        };
        sync::run(&SyncContext {
            drive: self.drive.as_ref(),
            db,
            layout: &folder.layout,
            paths,
            clock: &SystemClock,
        })
    }

    /// Files (not folders) in each `roms/<system>/` folder, by system id. Empty before setup.
    pub fn rom_file_counts(&self) -> Result<Vec<(String, usize)>> {
        let Some(folder) = self.settings()?.drive else {
            return Ok(Vec::new());
        };
        folder
            .layout
            .rom_folders
            .iter()
            .map(|(system, id)| {
                let n = self
                    .drive
                    .list(id)?
                    .iter()
                    .filter(|f| !f.is_folder())
                    .count();
                Ok((system.clone(), n))
            })
            .collect()
    }

    /// Creates or completes `Emulation/` under `parent`, writes the Drive settings file and
    /// mirrors the folder ids locally. `location` is the parent's display path.
    pub fn setup_folder(
        &self,
        parent: &str,
        location: &str,
        progress: &mut dyn FnMut(&str),
    ) -> Result<DriveFolder> {
        let layout = scaffold::setup(self.drive.as_ref(), parent, progress)?;
        let folder = DriveFolder {
            location: location.into(),
            layout,
        };
        let mut settings = self.settings()?;
        settings.drive = Some(folder.clone());
        settings.save(&self.settings_path)?;
        Ok(folder)
    }
}
