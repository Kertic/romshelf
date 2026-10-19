use super::{ControlLayout, DeclaredAction, EmulatorAdapter, LaunchSession};
use crate::model::{Rom, System};
use crate::paths::Paths;
use crate::process::CommandSpec;
use crate::Result;
use std::path::PathBuf;

pub struct RetroArch;

impl RetroArch {
    /// The config file passed with `--appendconfig` when Romshelf starts a game.
    pub fn append_config(paths: &Paths) -> PathBuf {
        RetroArch.session_dir(paths).join("romshelf.cfg")
    }

    /// A core's file in the RetroArch Flatpak's cores folder.
    pub fn core_file(paths: &Paths, core: &str) -> PathBuf {
        paths
            .retroarch_cores_dir()
            .join(format!("{core}_libretro.so"))
    }

    /// Libretro core per system (stage 4's table).
    pub fn core_for(system: System) -> Option<&'static str> {
        match system {
            System::Snes => Some("snes9x"),
            System::Gba => Some("mgba"),
            System::N64 => Some("mupen64plus_next"),
            _ => None,
        }
    }
}

impl EmulatorAdapter for RetroArch {
    fn id(&self) -> &'static str {
        "retroarch"
    }

    fn name(&self) -> &'static str {
        "RetroArch"
    }

    fn systems(&self) -> &'static [System] {
        &[System::Snes, System::Gba, System::N64]
    }

    fn flatpak_id(&self) -> &'static str {
        "org.libretro.RetroArch"
    }

    fn required_cores(&self) -> Vec<&'static str> {
        self.systems()
            .iter()
            .filter_map(|s| RetroArch::core_for(*s))
            .collect()
    }

    /// Saves, states and BIOS in Romshelf's own folders, through a config file appended with
    /// `--appendconfig`; the user's `retroarch.cfg` is never edited.
    fn prepare_session(&self, paths: &Paths) -> Result<()> {
        let session = self.session_dir(paths);
        for sub in ["saves", "states", "system"] {
            std::fs::create_dir_all(session.join(sub))?;
        }
        let cfg = format!(
            "# Written by Romshelf. Appended to RetroArch's own settings for games Romshelf starts.\n\
             savefile_directory = \"{}\"\n\
             savestate_directory = \"{}\"\n\
             system_directory = \"{}\"\n",
            session.join("saves").display(),
            session.join("states").display(),
            session.join("system").display(),
        );
        let file = RetroArch::append_config(paths);
        if std::fs::read_to_string(&file).ok().as_deref() != Some(cfg.as_str()) {
            std::fs::write(file, cfg)?;
        }
        Ok(())
    }

    fn launch_command(&self, rom: &Rom, session: &LaunchSession) -> Result<CommandSpec> {
        let core = RetroArch::core_for(rom.system).ok_or_else(|| {
            crate::Error::Invalid(format!("RetroArch has no core for {}", rom.system))
        })?;
        let rom_path = session.rom_path.to_string_lossy();
        Ok(CommandSpec::new(
            "flatpak",
            &["run", self.flatpak_id(), "-L", core, &rom_path],
        ))
    }

    fn save_paths(&self, rom: &Rom, paths: &Paths) -> Vec<PathBuf> {
        vec![paths
            .saves_dir()
            .join(rom.system.id())
            .join(rom.id.as_str())]
    }

    fn write_controls(&self, _layout: &ControlLayout, _rom: &Rom) -> Result<()> {
        Ok(())
    }

    fn declared_actions(&self) -> Vec<DeclaredAction> {
        vec![
            DeclaredAction::new("save_states", "Save states"),
            DeclaredAction::new("cheats", "Cheats"),
            DeclaredAction::new("shaders", "Shaders"),
        ]
    }
}
