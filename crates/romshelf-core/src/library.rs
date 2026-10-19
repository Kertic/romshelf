//! The data layer the interface reads from.

use crate::db::Database;
use crate::emulators::Registry;
use crate::model::{LocationState, Rom, System};
use crate::Result;
use serde::Serialize;

/// One row of the Library grid, with everything the tile and drawer need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryEntry {
    pub rom: Rom,
    pub system_label: String,
    pub system_colour: String,
    pub emulator: Option<String>,
    pub emulator_installed: bool,
    pub in_steam: bool,
}

pub fn entries(db: &Database, registry: &Registry) -> Result<Vec<LibraryEntry>> {
    db.roms()?
        .into_iter()
        .filter(|r| r.location != LocationState::Gone)
        .map(|rom| {
            let adapter = registry.for_system(rom.system);
            let emulator_installed = match adapter {
                Some(a) => db.emulator(a.id())?.is_some_and(|e| e.installed),
                None => false,
            };
            Ok(LibraryEntry {
                in_steam: db.shortcut(&rom.id)?.is_some(),
                system_label: rom.system.label().into(),
                system_colour: rom.system.colour().into(),
                emulator: adapter.map(|a| a.name().to_string()),
                emulator_installed,
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
