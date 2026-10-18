//! Emulator adapters and the registry that maps systems to them.

mod dolphin;
mod retroarch;

pub use dolphin::Dolphin;
pub use retroarch::RetroArch;

use crate::model::{Binding, Rom, System};
use crate::paths::Paths;
use crate::process::{CommandSpec, PackageRunner};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Everything an adapter needs to build a launch command.
#[derive(Debug, Clone)]
pub struct LaunchSession {
    pub rom_path: PathBuf,
    pub saves_dir: PathBuf,
    pub states_dir: PathBuf,
}

/// An emulator-specific function shown on the ROM page (cheats, memory cards, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredAction {
    pub id: String,
    pub label: String,
}

impl DeclaredAction {
    fn new(id: &str, label: &str) -> DeclaredAction {
        DeclaredAction {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Merged control layout: Deck input name to binding.
pub type ControlLayout = BTreeMap<String, Binding>;

pub trait EmulatorAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn systems(&self) -> &'static [System];
    fn flatpak_id(&self) -> &'static str;

    fn detect(&self, runner: &dyn PackageRunner) -> Result<bool> {
        let out = runner.flatpak(&["info", "--user", self.flatpak_id()])?;
        Ok(out.code == 0)
    }

    fn install(&self, runner: &dyn PackageRunner) -> Result<()> {
        let out = runner.flatpak(&[
            "install",
            "--user",
            "--noninteractive",
            "-y",
            "flathub",
            self.flatpak_id(),
        ])?;
        if out.code == 0 {
            Ok(())
        } else {
            Err(Error::External(format!(
                "Couldn't install {}: {}",
                self.name(),
                out.stderr.trim()
            )))
        }
    }

    fn launch_command(&self, rom: &Rom, session: &LaunchSession) -> Result<CommandSpec>;
    fn save_paths(&self, rom: &Rom, paths: &Paths) -> Vec<PathBuf>;
    /// Render the merged layout into the emulator's own configuration. Implemented in stage 7.
    fn write_controls(&self, layout: &ControlLayout, rom: &Rom) -> Result<()>;
    fn declared_actions(&self) -> Vec<DeclaredAction>;
}

pub struct Registry {
    adapters: Vec<Box<dyn EmulatorAdapter>>,
}

impl Default for Registry {
    fn default() -> Registry {
        Registry {
            adapters: vec![Box::new(RetroArch), Box::new(Dolphin)],
        }
    }
}

impl Registry {
    pub fn adapters(&self) -> impl Iterator<Item = &dyn EmulatorAdapter> {
        self.adapters.iter().map(|a| a.as_ref())
    }

    pub fn by_id(&self, id: &str) -> Option<&dyn EmulatorAdapter> {
        self.adapters().find(|a| a.id() == id)
    }

    pub fn for_system(&self, system: System) -> Option<&dyn EmulatorAdapter> {
        self.adapters().find(|a| a.systems().contains(&system))
    }

    /// The adapter for a file in a system folder, if its extension is recognized.
    pub fn for_file(&self, system: System, file_name: &str) -> Option<&dyn EmulatorAdapter> {
        let ext = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
        if !system.extensions().contains(&ext.as_str()) {
            return None;
        }
        self.for_system(system)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_extension_has_exactly_one_adapter() {
        let reg = Registry::default();
        for sys in System::ALL {
            let owners: Vec<_> = reg
                .adapters()
                .filter(|a| a.systems().contains(&sys))
                .collect();
            assert!(owners.len() <= 1, "{sys} has {} adapters", owners.len());
            if owners.is_empty() {
                continue;
            }
            for ext in sys.extensions() {
                let a = reg.for_file(sys, &format!("game.{ext}")).unwrap();
                assert_eq!(a.id(), owners[0].id());
                let upper = reg.for_file(sys, &format!("game.{}", ext.to_uppercase()));
                assert_eq!(upper.map(|a| a.id()), Some(owners[0].id()));
            }
        }
    }

    #[test]
    fn unknown_extension_has_no_adapter() {
        let reg = Registry::default();
        assert!(reg.for_file(System::Gba, "notes.txt").is_none());
        assert!(reg.for_file(System::Gba, "noext").is_none());
    }

    #[test]
    fn first_adapters_cover_the_brief() {
        let reg = Registry::default();
        assert_eq!(reg.for_system(System::Snes).unwrap().id(), "retroarch");
        assert_eq!(reg.for_system(System::Gba).unwrap().id(), "retroarch");
        assert_eq!(reg.for_system(System::N64).unwrap().id(), "retroarch");
        assert_eq!(reg.for_system(System::Gamecube).unwrap().id(), "dolphin");
    }
}
