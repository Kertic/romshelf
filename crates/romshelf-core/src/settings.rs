//! Local settings in `~/.config/romshelf/settings.json`, including a mirror of the Drive folder ids.

use crate::drive::scaffold::DriveLayout;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const LOCAL_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalSettings {
    pub schema_version: u32,
    /// Email of the Google account the Drive folder belongs to.
    pub account: Option<String>,
    pub drive: Option<DriveFolder>,
    /// Where ROMs are kept (`<root>/roms`); `None` is `~/Romshelf`.
    pub storage_root: Option<std::path::PathBuf>,
}

impl Default for LocalSettings {
    fn default() -> LocalSettings {
        LocalSettings {
            schema_version: LOCAL_SCHEMA,
            account: None,
            drive: None,
            storage_root: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveFolder {
    /// Where `Emulation/` was created, for display: "My Drive / Games".
    pub location: String,
    pub layout: DriveLayout,
}

impl DriveFolder {
    /// "My Drive / Games / Emulation"
    pub fn display_path(&self) -> String {
        format!("{} / {}", self.location, crate::drive::scaffold::EMULATION)
    }
}

impl LocalSettings {
    /// Defaults when the file does not exist yet.
    pub fn load(path: &Path) -> Result<LocalSettings> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LocalSettings::default())
            }
            Err(e) => return Err(e.into()),
        };
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let version = value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if version > u64::from(LOCAL_SCHEMA) {
            return Err(Error::NeedsUpdate(format!(
                "The settings file {}",
                path.display()
            )));
        }
        Ok(serde_json::from_value(value)?)
    }

    /// Writes atomically: a temp file renamed over the old one.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}
