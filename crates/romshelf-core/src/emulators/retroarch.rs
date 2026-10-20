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

    /// This game's own remap folder (`input_remapping_directory` for its launches).
    pub fn remap_dir(paths: &Paths, rom: &Rom) -> PathBuf {
        RetroArch
            .session_dir(paths)
            .join("remaps")
            .join(rom.id.as_str())
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

/// What follows the ROM's name in a save or state file: `.srm`, `.rtc`, `.state1`,
/// `.state.auto`, `.state1.png`. `None` for anything else.
fn save_suffix(name: &str) -> Option<&str> {
    if let Some(at) = name.rfind(".state") {
        return Some(&name[at..]);
    }
    [".srm", ".rtc", ".sav", ".ldci"]
        .iter()
        .find(|ext| name.ends_with(*ext))
        .map(|ext| &name[name.len() - ext.len()..])
}

/// RetroArch names saves after the ROM file, so a ROM renamed in Drive would start with no
/// save. The game's own folder holds only its files: any under an old name move to the
/// current one, unless a file of that kind already has the current name (then nothing is
/// guessed and both stay).
fn follow_rom_name(own: &Path, rom: &Rom) -> Result<()> {
    let Some(stem) = Path::new(&rom.file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
    else {
        return Ok(());
    };
    let Ok(entries) = std::fs::read_dir(own) else {
        return Ok(());
    };
    let names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    for name in &names {
        let Some(suffix) = save_suffix(name) else {
            continue;
        };
        let current = format!("{stem}{suffix}");
        if *name == current || names.contains(&current) {
            continue;
        }
        // Another old name with the same kind of file: unclear which is right, so none moves.
        let rivals = names
            .iter()
            .filter(|n| save_suffix(n) == Some(suffix))
            .count();
        if rivals == 1 {
            std::fs::rename(own.join(name), own.join(&current))?;
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
             sort_savestates_by_content_enable = \"false\"\n\
             input_remap_binds_enable = \"true\"\n\
             remap_save_on_exit = \"false\"\n",
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
        follow_rom_name(&saves, rom)?;
        follow_rom_name(&states, rom)?;
        let cfg = format!(
            "# Written by Romshelf for {}.\n\
             savefile_directory = \"{}\"\n\
             savestate_directory = \"{}\"\n\
             input_remapping_directory = \"{}\"\n",
            rom.display_name,
            saves.display(),
            states.display(),
            RetroArch::remap_dir(paths, rom).display(),
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
        if let Some(core) =
            RetroArch::core_for(rom.system).and_then(crate::controls::retroarch::core_name)
        {
            write_if_changed(
                &RetroArch::remap_dir(paths, rom)
                    .join(core)
                    .join(format!("{core}.rmp")),
                &crate::controls::retroarch::remap(rom.system, layout),
            )?;
        }
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
