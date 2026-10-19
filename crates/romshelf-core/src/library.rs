//! The data layer the interface reads from.

use crate::db::Database;
use crate::emulators::setup::{self, SetupStep};
use crate::emulators::{DeclaredAction, Registry, RetroArch};
use crate::model::{LocationState, Rom, System};
use crate::Result;
use serde::Serialize;

/// One row of the Library grid, with everything the tile and drawer need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryEntry {
    pub rom: Rom,
    pub system_label: String,
    pub system_colour: String,
    /// The emulator's name, or `None` when Romshelf can't run this system yet.
    pub emulator: Option<String>,
    pub emulator_id: Option<String>,
    /// RetroArch's core for this system ("mgba").
    pub core: Option<String>,
    /// The emulator's Flatpak is installed (here or by another tool).
    pub emulator_installed: bool,
    /// Installed and Romshelf's setup has finished: the game can be played once downloaded.
    pub emulator_ready: bool,
    /// The emulator's own functions for the ROM page.
    pub actions: Vec<DeclaredAction>,
    pub in_steam: bool,
}

pub fn entries(db: &Database, registry: &Registry) -> Result<Vec<LibraryEntry>> {
    db.roms()?
        .into_iter()
        .filter(|r| r.location != LocationState::Gone)
        .map(|rom| {
            let adapter = registry.for_system(rom.system);
            let (emulator_installed, emulator_ready) = match adapter {
                Some(a) => {
                    let installed = db.emulator(a.id())?.is_some_and(|e| e.installed);
                    let steps = setup::completed(db, a)?.len() == SetupStep::ALL.len();
                    (installed, installed && steps)
                }
                None => (false, false),
            };
            Ok(LibraryEntry {
                in_steam: db.shortcut(&rom.id)?.is_some(),
                system_label: rom.system.label().into(),
                system_colour: rom.system.colour().into(),
                emulator: adapter.map(|a| a.name().to_string()),
                emulator_id: adapter.map(|a| a.id().to_string()),
                core: RetroArch::core_for(rom.system)
                    .filter(|_| adapter.is_some_and(|a| a.id() == "retroarch"))
                    .map(str::to_string),
                emulator_installed,
                emulator_ready,
                actions: adapter.map(|a| a.declared_actions()).unwrap_or_default(),
                rom,
            })
        })
        .collect()
}
pub fn systems_with_roms(entries: &[LibraryEntry]) -> Vec<System> {
    let mut v: Vec<System> = entries.iter().map(|e| e.rom.system).collect();
    v.sort();
    v.dedup();
    v
}
