use super::{ControlLayout, DeclaredAction, EmulatorAdapter, LaunchSession};
use crate::model::{Rom, System};
use crate::paths::Paths;
use crate::process::CommandSpec;
use crate::Result;
use std::path::PathBuf;

pub struct RetroArch;

impl RetroArch {
    /// Libretro core per system. Confirm in stage 4: core names and their install location.
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
