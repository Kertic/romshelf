use super::{ControlLayout, DeclaredAction, EmulatorAdapter, LaunchSession};
use crate::model::{Rom, System};
use crate::paths::Paths;
use crate::process::CommandSpec;
use crate::Result;
use std::path::PathBuf;

pub struct Dolphin;

impl EmulatorAdapter for Dolphin {
    fn id(&self) -> &'static str {
        "dolphin"
    }

    fn name(&self) -> &'static str {
        "Dolphin"
    }

    fn systems(&self) -> &'static [System] {
        &[System::Gamecube]
    }

    fn flatpak_id(&self) -> &'static str {
        "org.DolphinEmu.dolphin-emu"
    }

    fn launch_command(&self, _rom: &Rom, session: &LaunchSession) -> Result<CommandSpec> {
        // `-b` exits Dolphin when the game stops; `-e` runs the file. Confirm in stage 5.
        let rom_path = session.rom_path.to_string_lossy();
        Ok(CommandSpec::new(
            "flatpak",
            &["run", self.flatpak_id(), "-b", "-e", &rom_path],
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
            DeclaredAction::new("memory_cards", "Memory cards"),
            DeclaredAction::new("save_states", "Save states"),
            DeclaredAction::new("texture_packs", "Texture packs"),
        ]
    }
}
