use super::{BiosFile, ControlLayout, DeclaredAction, EmulatorAdapter, LaunchSession};
use crate::ini;
use crate::model::{Rom, System};
use crate::paths::Paths;
use crate::process::CommandSpec;
use crate::Result;
use std::path::{Path, PathBuf};

pub struct Pcsx2;

impl Pcsx2 {
    /// PCSX2's data folder when started with `-datapath <session>`: it adds `PCSX2` itself
    /// (`EmuFolders::SetDataDirectory` in PCSX2's source).
    pub fn data_root(paths: &Paths) -> PathBuf {
        Pcsx2.session_dir(paths).join("PCSX2")
    }

    /// `inis/PCSX2.ini` in the data folder.
    pub fn settings_file(paths: &Paths) -> PathBuf {
        Pcsx2::data_root(paths).join("inis/PCSX2.ini")
    }

    /// Reads the settings, changes them, writes them back.
    fn update_settings(paths: &Paths, change: impl FnOnce(String) -> String) -> Result<()> {
        let file = Pcsx2::settings_file(paths);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let existing = std::fs::read_to_string(&file).unwrap_or_default();
        std::fs::write(&file, change(existing))?;
        Ok(())
    }

    /// The BIOS PCSX2 boots: the first `.bin` in the BIOS folder, by name.
    fn bios_file(dir: &Path) -> Option<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_ascii_lowercase().ends_with(".bin"))
            .collect();
        names.sort_by_key(|n| n.to_ascii_lowercase());
        names.into_iter().next()
    }
}

impl EmulatorAdapter for Pcsx2 {
    fn id(&self) -> &'static str {
        "pcsx2"
    }

    fn name(&self) -> &'static str {
        "PCSX2"
    }

    fn systems(&self) -> &'static [System] {
        &[System::Ps2]
    }

    fn flatpak_id(&self) -> &'static str {
        "net.pcsx2.PCSX2"
    }

    /// A PS2 BIOS is required; its name depends on the console model it was dumped from.
    fn bios_files(&self, system: System) -> Vec<BiosFile> {
        match system {
            System::Ps2 => vec![BiosFile::any_of("bin", "a PS2 BIOS file (.bin)")],
            _ => Vec::new(),
        }
    }

    /// A PCSX2 data folder of Romshelf's own, passed with `-datapath`, so a PCSX2 the user
    /// already set up is never touched.
    fn session_dir(&self, paths: &Paths) -> PathBuf {
        paths
            .flatpak_app_dir(self.flatpak_id())
            .join("data/romshelf")
    }

    fn bios_dir(&self, paths: &Paths, _system: System) -> PathBuf {
        Pcsx2::data_root(paths).join("bios")
    }

    /// The settings every launch needs: the BIOS folder and file, and PCSX2's first-run wizard
    /// marked done (Confirm the key on the Deck). Other settings are kept.
    fn prepare_session(&self, paths: &Paths) -> Result<()> {
        let bios = self.bios_dir(paths, System::Ps2);
        std::fs::create_dir_all(&bios)?;
        let chosen = Pcsx2::bios_file(&bios);
        let bios_text = bios.to_string_lossy().into_owned();
        Pcsx2::update_settings(paths, |text| {
            let text = ini::set(&text, "UI", "SetupWizardIncomplete", "false");
            let text = ini::set(&text, "Folders", "Bios", &bios_text);
            match &chosen {
                Some(name) => ini::set(&text, "Filenames", "BIOS", name),
                None => text,
            }
        })
    }

    /// This game's own memory card (`Mcd001.ps2` in its own folder) and save-state folder.
    /// One game runs at a time, so the shared settings point at the game about to start.
    fn prepare_game(&self, rom: &Rom, paths: &Paths) -> Result<()> {
        let saves = self.save_dir(rom, paths);
        let states = Pcsx2::data_root(paths)
            .join("sstates")
            .join(rom.id.as_str());
        std::fs::create_dir_all(&saves)?;
        std::fs::create_dir_all(&states)?;
        let (saves, states) = (
            saves.to_string_lossy().into_owned(),
            states.to_string_lossy().into_owned(),
        );
        Pcsx2::update_settings(paths, |text| {
            let text = ini::set_all(
                &text,
                "Folders",
                &[
                    ("MemoryCards", saves.as_str()),
                    ("Savestates", states.as_str()),
                ],
            );
            ini::set_all(
                &text,
                "MemoryCards",
                &[
                    ("Slot1_Enable", "true"),
                    ("Slot1_Filename", "Mcd001.ps2"),
                    ("Slot2_Enable", "false"),
                ],
            )
        })
    }

    /// `-datapath` Romshelf's folder, `-batch` quit when the game does, `-nogui` no main window,
    /// full screen, and `--` so the path is taken whole (options from PCSX2's `QtHost.cpp`).
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
                "-datapath",
                &self.session_dir(paths).to_string_lossy(),
                "-batch",
                "-nogui",
                "-fullscreen",
                "--",
                &session.rom_path.to_string_lossy(),
            ],
        ))
    }

    fn save_dir(&self, rom: &Rom, paths: &Paths) -> PathBuf {
        Pcsx2::data_root(paths)
            .join("memcards")
            .join(rom.id.as_str())
    }

    fn state_dir(&self, rom: &Rom, paths: &Paths) -> Option<PathBuf> {
        Some(
            Pcsx2::data_root(paths)
                .join("sstates")
                .join(rom.id.as_str()),
        )
    }

    fn keeps_states(&self) -> bool {
        true
    }

    /// Pad 1 and the hotkeys in Romshelf's copy of PCSX2's settings, rewritten before each
    /// launch.
    fn write_controls(&self, layout: &ControlLayout, _rom: &Rom, paths: &Paths) -> Result<()> {
        Pcsx2::update_settings(paths, |text| {
            crate::controls::pcsx2::settings(&text, layout)
        })
    }

    fn declared_actions(&self) -> Vec<DeclaredAction> {
        vec![
            DeclaredAction::new("memory_cards", "Memory cards"),
            DeclaredAction::new("save_states", "Save states"),
        ]
    }
}
