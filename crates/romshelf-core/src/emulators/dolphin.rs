use super::{ControlLayout, DeclaredAction, EmulatorAdapter, LaunchSession};
use crate::model::{Rom, System};
use crate::paths::Paths;
use crate::process::CommandSpec;
use crate::Result;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub struct Dolphin;

/// The user's Dolphin settings files copied into Romshelf's user folder the first time:
/// general and graphics settings, GameCube controllers and keyboards, hotkeys.
const SEEDED_CONFIG: &[&str] = &[
    "Dolphin.ini",
    "GFX.ini",
    "GCPadNew.ini",
    "GCKeyNew.ini",
    "Hotkeys.ini",
];

impl Dolphin {
    /// The six-character game id (`GZ2E01`) from the disc header: at the start of an ISO or
    /// GCM, after the 32 KB header of a CISO, and in the copy of the disc header that RVZ and
    /// WIA files keep at 0x58. `None` for other formats (GCZ is compressed from the start).
    pub fn game_id(rom_file: &Path) -> Option<String> {
        let mut f = std::fs::File::open(rom_file).ok()?;
        let mut magic = [0u8; 4];
        f.read_exact(&mut magic).ok()?;
        let offset = match &magic {
            b"RVZ\x01" | b"WIA\x01" => 0x58,
            b"CISO" => 0x8000,
            _ => 0,
        };
        f.seek(SeekFrom::Start(offset)).ok()?;
        let mut id = [0u8; 6];
        f.read_exact(&mut id).ok()?;
        id.iter()
            .all(u8::is_ascii_alphanumeric)
            .then(|| String::from_utf8_lossy(&id).into_owned())
    }

    /// `Config/` inside Romshelf's user folder, where Dolphin reads settings when run with `-u`.
    pub fn config_dir(paths: &Paths) -> PathBuf {
        Dolphin.session_dir(paths).join("Config")
    }
}

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

    /// Starts Romshelf's user folder from a copy of the user's own Dolphin settings (controls,
    /// graphics, hotkeys), so games play the way Dolphin is already set up. The user's files are
    /// only read; a file Romshelf already has is never replaced.
    fn prepare_session(&self, paths: &Paths) -> Result<()> {
        let ours = Dolphin::config_dir(paths);
        std::fs::create_dir_all(&ours)?;
        let theirs = paths
            .flatpak_app_dir(self.flatpak_id())
            .join("config/dolphin-emu");
        for name in SEEDED_CONFIG {
            let target = ours.join(name);
            if target.exists() {
                continue;
            }
            if let Ok(bytes) = std::fs::read(theirs.join(name)) {
                std::fs::write(target, bytes)?;
            }
        }
        Ok(())
    }

    /// The game's own memory-card folder. Saves made before each game had one are in Dolphin's
    /// shared `GC/<region>/Card A/`; those for this game (named `<maker>-<game code>-….gci`)
    /// move into it.
    fn prepare_game(&self, rom: &Rom, paths: &Paths) -> Result<()> {
        let own = self.save_dir(rom, paths);
        std::fs::create_dir_all(&own)?;
        let Some(id) = paths
            .rom_file(rom.system, &rom.file_name)
            .ok()
            .and_then(|f| Dolphin::game_id(&f))
        else {
            return Ok(());
        };
        let prefix = format!("{}-{}-", &id[4..6], &id[..4]);
        let gc = self.session_dir(paths).join("GC");
        for region in ["USA", "EUR", "JAP"] {
            let Ok(entries) = std::fs::read_dir(gc.join(region).join("Card A")) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let target = own.join(&name);
                if name.starts_with(&prefix) && name.ends_with(".gci") && !target.exists() {
                    std::fs::rename(entry.path(), target)?;
                }
            }
        }
        Ok(())
    }

    /// `-u` Romshelf's user folder, `-b` no Dolphin window (quits with the game), full screen,
    /// memory card A as a folder of saves (GCI folder, `SlotA = 8`) that is this game's own,
    /// `-e` the game. Options and setting names checked against Dolphin 2407 on the Deck.
    fn launch_command(
        &self,
        rom: &Rom,
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
                "-C",
                "Dolphin.Core.SlotA=8",
                "-C",
                &format!(
                    "Dolphin.Core.GCIFolderAPathOverride={}",
                    self.save_dir(rom, paths).display()
                ),
                "-e",
                &session.rom_path.to_string_lossy(),
            ],
        ))
    }

    /// Under `GC/Romshelf/`, beside Dolphin's own region folders.
    fn save_dir(&self, rom: &Rom, paths: &Paths) -> PathBuf {
        self.session_dir(paths)
            .join("GC/Romshelf")
            .join(rom.id.as_str())
    }

    /// Pad 1 and the hotkeys in Romshelf's copy of Dolphin's settings, rewritten before each
    /// launch (one game runs at a time). Other pads are left as they were.
    fn write_controls(&self, layout: &ControlLayout, _rom: &Rom, paths: &Paths) -> Result<()> {
        let dir = Dolphin::config_dir(paths);
        std::fs::create_dir_all(&dir)?;
        let pads = dir.join("GCPadNew.ini");
        let existing = std::fs::read_to_string(&pads).unwrap_or_default();
        std::fs::write(
            &pads,
            crate::controls::dolphin::gcpad_file(&existing, layout),
        )?;
        std::fs::write(
            dir.join("Hotkeys.ini"),
            crate::controls::dolphin::hotkeys(layout),
        )?;
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
