//! Steam files seam: `shortcuts.vdf` and the grid artwork folder. Parsing arrives in stage 8.

use crate::Result;
use std::path::{Path, PathBuf};

pub trait SteamFiles: Send + Sync {
    /// Steam user ids that have a `userdata/<id>/config` folder.
    fn user_ids(&self) -> Result<Vec<String>>;
    fn read_shortcuts(&self, user: &str) -> Result<Option<Vec<u8>>>;
    /// Writes atomically after copying any existing file to `shortcuts.vdf.romshelf-bak`.
    fn write_shortcuts(&self, user: &str, bytes: &[u8]) -> Result<()>;
    fn write_grid(&self, user: &str, file_name: &str, bytes: &[u8]) -> Result<()>;
}

/// The real Steam directory, or a temp copy of it in tests.
pub struct FsSteamFiles {
    pub steam_root: PathBuf,
}

impl FsSteamFiles {
    fn config_dir(&self, user: &str) -> PathBuf {
        self.steam_root.join("userdata").join(user).join("config")
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("romshelf-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

impl SteamFiles for FsSteamFiles {
    fn user_ids(&self) -> Result<Vec<String>> {
        let dir = self.steam_root.join("userdata");
        let mut ids = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.chars().all(|c| c.is_ascii_digit()) && entry.path().join("config").is_dir() {
                ids.push(name);
            }
        }
        ids.sort();
        Ok(ids)
    }

    fn read_shortcuts(&self, user: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.config_dir(user).join("shortcuts.vdf")) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn write_shortcuts(&self, user: &str, bytes: &[u8]) -> Result<()> {
        let path = self.config_dir(user).join("shortcuts.vdf");
        if path.exists() {
            std::fs::copy(&path, path.with_extension("vdf.romshelf-bak"))?;
        }
        write_atomic(&path, bytes)
    }

    fn write_grid(&self, user: &str, file_name: &str, bytes: &[u8]) -> Result<()> {
        if file_name.contains(['/', '\\']) || file_name.starts_with('.') {
            return Err(crate::Error::Invalid(format!(
                "bad grid file name: {file_name}"
            )));
        }
        write_atomic(&self.config_dir(user).join("grid").join(file_name), bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_shortcuts_backs_up_and_touches_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        let cfg = root.path().join("userdata/123/config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("shortcuts.vdf"), b"old").unwrap();
        std::fs::write(cfg.join("localconfig.vdf"), b"keep").unwrap();
        let steam = FsSteamFiles {
            steam_root: root.path().into(),
        };
        assert_eq!(steam.user_ids().unwrap(), vec!["123".to_string()]);
        steam.write_shortcuts("123", b"new").unwrap();
        assert_eq!(steam.read_shortcuts("123").unwrap().unwrap(), b"new");
        assert_eq!(
            std::fs::read(cfg.join("shortcuts.vdf.romshelf-bak")).unwrap(),
            b"old"
        );
        assert_eq!(std::fs::read(cfg.join("localconfig.vdf")).unwrap(), b"keep");
        let mut names: Vec<_> = std::fs::read_dir(&cfg)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "localconfig.vdf",
                "shortcuts.vdf",
                "shortcuts.vdf.romshelf-bak"
            ]
        );
    }

    #[test]
    fn grid_rejects_paths() {
        let root = tempfile::tempdir().unwrap();
        let steam = FsSteamFiles {
            steam_root: root.path().into(),
        };
        assert!(steam.write_grid("1", "../x.png", b"").is_err());
        steam.write_grid("1", "123p.png", b"img").unwrap();
    }
}
