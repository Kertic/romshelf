//! Every filesystem location Romshelf uses. Nothing else builds a path from `~`.

use crate::model::System;
use crate::{Error, Result};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// `~/.local/share/romshelf`: index database and logs.
    pub data_dir: PathBuf,
    /// `~/.config/romshelf`: settings and the client config.
    pub config_dir: PathBuf,
    /// Where downloaded ROMs live (internal storage or SD card).
    pub rom_root: PathBuf,
    /// `~/.steam/steam`: the Steam installation holding `userdata/`.
    pub steam_root: PathBuf,
}

impl Paths {
    /// Build from the environment, honouring XDG variables and `ROMSHELF_ROM_ROOT`.
    pub fn from_env() -> Result<Paths> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .ok_or_else(|| Error::Invalid("HOME is not set".into()))?;
        let xdg = |var: &str, default: &str| {
            std::env::var_os(var)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(default))
        };
        let data_dir = xdg("XDG_DATA_HOME", ".local/share").join("romshelf");
        let config_dir = xdg("XDG_CONFIG_HOME", ".config").join("romshelf");
        let rom_root = std::env::var_os("ROMSHELF_ROM_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("Emulation/roms"));
        let steam_root = home.join(".steam/steam");
        Ok(Paths {
            data_dir,
            config_dir,
            rom_root,
            steam_root,
        })
    }

    /// Every location under one root. Used by tests and fake mode.
    pub fn under(root: &Path) -> Paths {
        Paths {
            data_dir: root.join("data"),
            config_dir: root.join("config"),
            rom_root: root.join("roms"),
            steam_root: root.join("steam"),
        }
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("index.sqlite")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn token_file(&self) -> PathBuf {
        self.config_dir.join("token.json")
    }

    pub fn saves_dir(&self) -> PathBuf {
        self.data_dir.join("saves")
    }

    pub fn states_dir(&self) -> PathBuf {
        self.data_dir.join("states")
    }

    /// Local path of a ROM file. Rejects names that would leave the system folder.
    pub fn rom_file(&self, system: System, file_name: &str) -> Result<PathBuf> {
        let rel = Path::new(file_name);
        let mut parts = rel.components();
        let simple = matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none();
        if !simple {
            return Err(Error::Invalid(format!("unsafe file name: {file_name}")));
        }
        Ok(self.rom_root.join(system.id()).join(rel))
    }

    /// All fixed locations, for checks such as "nothing escapes the root".
    pub fn all(&self) -> Vec<PathBuf> {
        vec![
            self.data_dir.clone(),
            self.config_dir.clone(),
            self.rom_root.clone(),
            self.steam_root.clone(),
            self.db_file(),
            self.log_dir(),
            self.settings_file(),
            self.token_file(),
            self.saves_dir(),
            self.states_dir(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_escapes_temp_root() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::under(root.path());
        for p in paths.all() {
            assert!(p.starts_with(root.path()), "{p:?} escapes root");
        }
        for sys in System::ALL {
            let file = paths.rom_file(sys, "game.bin").unwrap();
            assert!(file.starts_with(root.path()));
        }
    }

    #[test]
    fn rom_file_rejects_traversal() {
        let paths = Paths::under(Path::new("/r"));
        for bad in ["../x.gba", "a/b.gba", "/etc/passwd", "..", "", "."] {
            assert!(
                paths.rom_file(System::Gba, bad).is_err(),
                "{bad:?} accepted"
            );
        }
    }
}
