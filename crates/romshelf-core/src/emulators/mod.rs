//! Emulator adapters and the registry that maps systems to them.

mod dolphin;
mod retroarch;
pub mod setup;

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
    /// The folder holding `roms/`, opened read-only to the Flatpak for this run.
    pub storage_root: PathBuf,
}

impl LaunchSession {
    /// `--filesystem=<storage root>:ro`, so the sandboxed emulator can read the ROM wherever
    /// it is kept (home or an SD card).
    pub fn filesystem_arg(&self) -> String {
        format!("--filesystem={}:ro", self.storage_root.display())
    }
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

/// A BIOS file an emulator needs (or can use) for a system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BiosFile {
    pub name: String,
    pub required: bool,
}

impl BiosFile {
    pub fn required(name: &str) -> BiosFile {
        BiosFile {
            name: name.into(),
            required: true,
        }
    }

    pub fn optional(name: &str) -> BiosFile {
        BiosFile {
            name: name.into(),
            required: false,
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

    /// Installed for this user or system-wide (EmuDeck may have installed it either way), so
    /// Romshelf never adds a second copy.
    fn detect(&self, runner: &dyn PackageRunner) -> Result<bool> {
        let out = runner.flatpak(&["info", self.flatpak_id()])?;
        Ok(out.code == 0)
    }

    /// Plug-ins the emulator needs besides the Flatpak itself (RetroArch cores).
    fn required_cores(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// BIOS files per system, copied from Drive's `bios/<system>/` during setup.
    fn bios_files(&self, _system: System) -> Vec<BiosFile> {
        Vec::new()
    }

    /// Romshelf's own folder for this emulator, inside the Flatpak's data so the sandbox can
    /// reach it. The user's own emulator configuration is never edited.
    fn session_dir(&self, paths: &Paths) -> PathBuf {
        paths
            .flatpak_app_dir(self.flatpak_id())
            .join("config/romshelf")
    }

    /// Where BIOS files go for a system.
    fn bios_dir(&self, paths: &Paths, _system: System) -> PathBuf {
        self.session_dir(paths).join("system")
    }

    /// Creates the session folder and anything the emulator reads from it.
    fn prepare_session(&self, paths: &Paths) -> Result<()> {
        std::fs::create_dir_all(self.session_dir(paths))?;
        Ok(())
    }

    fn launch_command(
        &self,
        rom: &Rom,
        session: &LaunchSession,
        paths: &Paths,
    ) -> Result<CommandSpec>;
    /// The folder this game's save files are kept in on the Deck, one folder per game, as
    /// the emulator is told to use it for games Romshelf starts.
    fn save_dir(&self, rom: &Rom, paths: &Paths) -> PathBuf;
    /// The folder of this game's save states, when Romshelf can keep them apart per game.
    fn state_dir(&self, _rom: &Rom, _paths: &Paths) -> Option<PathBuf> {
        None
    }
    /// `state_dir` gives a folder for every game.
    fn keeps_states(&self) -> bool {
        false
    }
    /// The plug-in a save was made with, recorded in each backup (RetroArch's core).
    fn core(&self, _system: System) -> Option<&'static str> {
        None
    }
    /// Prepares this game's save folders before it starts. Saves an earlier Romshelf kept in a
    /// shared folder are moved into the game's own.
    fn prepare_game(&self, rom: &Rom, paths: &Paths) -> Result<()> {
        std::fs::create_dir_all(self.save_dir(rom, paths))?;
        Ok(())
    }
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

    /// The adapter for a system, or a clear "no emulator yet" error.
    pub fn require(&self, system: System) -> Result<&dyn EmulatorAdapter> {
        self.for_system(system).ok_or_else(|| {
            Error::Invalid(format!("Romshelf can't run {} games yet.", system.label()))
        })
    }

    /// The adapter for a file in a system folder, if its extension is recognized.
    pub fn for_file(&self, system: System, file_name: &str) -> Option<&dyn EmulatorAdapter> {
        if !system.recognizes(file_name) {
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
