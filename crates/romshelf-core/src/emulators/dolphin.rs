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

    /// A Dolphin user folder of Romshelf's own, passed with `-u` (Confirm in stage 5), so the
    /// user's own Dolphin settings and memory cards are never touched.
    fn session_dir(&self, paths: &Paths) -> PathBuf {
        paths
            .flatpak_app_dir(self.flatpak_id())
            .join("data/romshelf-user")
    }

    /// `-u` Romshelf's user folder, `-b` no Dolphin window (quits with the game), full screen,
    /// `-e` the game. Options checked against Dolphin 2407's `--help` on the Deck.
    fn launch_command(
        &self,
        _rom: &Rom,
        session: &LaunchSession,
        paths: &Paths,
    ) -> Result<CommandSpec> {
        Ok(CommandSpec::new(
            "flatpak",
            &[
                "run",
                &session.filesystem_arg(),
                self.flatpak_id(),
                "-u",
                &self.session_dir(paths).to_string_lossy(),
                "-b",
                "-C",
                "Dolphin.Display.Fullscreen=True",
                "-e",
                &session.rom_path.to_string_lossy(),
            ],
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
