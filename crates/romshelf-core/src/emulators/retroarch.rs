use super::{ControlLayout, DeclaredAction, EmulatorAdapter, LaunchSession};
use crate::model::{Rom, System};
use crate::paths::Paths;
use crate::process::CommandSpec;
use crate::Result;
use std::path::{Path, PathBuf};

pub struct RetroArch;

impl RetroArch {
    /// The config file passed with `--appendconfig` when Romshelf starts a game.
    pub fn append_config(paths: &Paths) -> PathBuf {
        RetroArch.session_dir(paths).join("romshelf.cfg")
    }

    /// This game's own config, appended after `romshelf.cfg`: its save and state folders.
    pub fn game_config(paths: &Paths, rom: &Rom) -> PathBuf {
        RetroArch
            .session_dir(paths)
            .join("games")
            .join(format!("{}.cfg", rom.id))
    }

    /// This game's controls, appended last.
    pub fn controls_config(paths: &Paths, rom: &Rom) -> PathBuf {
        RetroArch
            .session_dir(paths)
            .join("games")
            .join(format!("{}.controls.cfg", rom.id))
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

/// Writes `contents` unless the file already holds exactly that.
fn write_if_changed(file: &Path, contents: &str) -> Result<()> {
    if std::fs::read_to_string(file).ok().as_deref() != Some(contents) {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(file, contents)?;
    }
    Ok(())
}

/// Moves files named after the ROM (`Kirby.srm`, `Kirby.state1`) from a shared folder into the
/// game's own, as saves made before each game had its own folder. A file the game's folder
/// already has is left where it is.
fn move_named_files(shared: &Path, own: &Path, rom: &Rom) -> Result<()> {
    let stem = match Path::new(&rom.file_name).file_stem() {
        Some(s) => format!("{}.", s.to_string_lossy()),
        None => return Ok(()),
    };
    let Ok(entries) = std::fs::read_dir(shared) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&stem) || !entry.path().is_file() {
            continue;
        }
        let target = own.join(&name);
        if !target.exists() {
            std::fs::rename(entry.path(), target)?;
        }
    }
    Ok(())
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
    /// `--appendconfig`; the user's `retroarch.cfg` is never edited. Saves go straight into the
    /// folder given, never next to the ROM or into per-core sub-folders, whatever the user's own
    /// settings say.
    fn prepare_session(&self, paths: &Paths) -> Result<()> {
        let session = self.session_dir(paths);
        for sub in ["saves", "states", "system"] {
            std::fs::create_dir_all(session.join(sub))?;
        }
        let cfg = format!(
            "# Written by Romshelf. Appended to RetroArch's own settings for games Romshelf starts.\n\
             savefile_directory = \"{}\"\n\
             savestate_directory = \"{}\"\n\
             system_directory = \"{}\"\n\
             savefiles_in_content_dir = \"false\"\n\
             savestates_in_content_dir = \"false\"\n\
             sort_savefiles_enable = \"false\"\n\
             sort_savestates_enable = \"false\"\n\
             sort_savefiles_by_content_enable = \"false\"\n\
             sort_savestates_by_content_enable = \"false\"\n",
            session.join("saves").display(),
            session.join("states").display(),
            session.join("system").display(),
        );
        write_if_changed(&RetroArch::append_config(paths), &cfg)
    }

    /// The game's own save and state folders, and the config that points RetroArch at them.
    fn prepare_game(&self, rom: &Rom, paths: &Paths) -> Result<()> {
        let saves = self.save_dir(rom, paths);
        let states = self.state_dir(rom, paths).unwrap_or_default();
        std::fs::create_dir_all(&saves)?;
        std::fs::create_dir_all(&states)?;
        let session = self.session_dir(paths);
        move_named_files(&session.join("saves"), &saves, rom)?;
        move_named_files(&session.join("states"), &states, rom)?;
        let cfg = format!(
            "# Written by Romshelf for {}.\n\
             savefile_directory = \"{}\"\n\
             savestate_directory = \"{}\"\n",
            rom.display_name,
            saves.display(),
            states.display(),
        );
        write_if_changed(&RetroArch::game_config(paths, rom), &cfg)
    }

    /// `flatpak run --filesystem=<ROMs>:ro org.libretro.RetroArch -L <core> --appendconfig
    /// <Romshelf's cfg>|<the game's cfg>|<its controls> -f <rom>`: full screen, the game's own save folders,
    /// the user's own settings otherwise. RetroArch reads `|`-separated append configs in order.
    fn launch_command(
        &self,
        rom: &Rom,
        session: &LaunchSession,
        paths: &Paths,
    ) -> Result<CommandSpec> {
        let core = RetroArch::core_for(rom.system).ok_or_else(|| {
            crate::Error::Invalid(format!("RetroArch has no core for {}.", rom.system.label()))
        })?;
        let configs = format!(
            "{}|{}|{}",
            RetroArch::append_config(paths).display(),
            RetroArch::game_config(paths, rom).display(),
            RetroArch::controls_config(paths, rom).display()
        );
        Ok(CommandSpec::new(
            "flatpak",
            &[
                "run",
                &session.filesystem_arg(),
                self.flatpak_id(),
                "-L",
                &RetroArch::core_file(paths, core).to_string_lossy(),
                "--appendconfig",
                &configs,
                "-f",
                &session.rom_path.to_string_lossy(),
            ],
        ))
    }

    fn save_dir(&self, rom: &Rom, paths: &Paths) -> PathBuf {
        self.session_dir(paths).join("saves").join(rom.id.as_str())
    }

    fn state_dir(&self, rom: &Rom, paths: &Paths) -> Option<PathBuf> {
        Some(self.session_dir(paths).join("states").join(rom.id.as_str()))
    }

    fn core(&self, system: System) -> Option<&'static str> {
        RetroArch::core_for(system)
    }

    fn keeps_states(&self) -> bool {
        true
    }

    /// The game's controls in their own appended config, `games/<rom-id>.controls.cfg`.
    fn write_controls(&self, layout: &ControlLayout, rom: &Rom, paths: &Paths) -> Result<()> {
        write_if_changed(
            &RetroArch::controls_config(paths, rom),
            &crate::controls::retroarch::config(rom.system, layout),
        )
    }

    fn declared_actions(&self) -> Vec<DeclaredAction> {
        vec![
            DeclaredAction::new("save_states", "Save states"),
            DeclaredAction::new("cheats", "Cheats"),
            DeclaredAction::new("shaders", "Shaders"),
        ]
    }
}
