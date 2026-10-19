//! The eight-ROM fixture library shown in the approved screenshots.

use romshelf_core::db::Database;
use romshelf_core::emulators::Registry;
use romshelf_core::model::{Emulator, LocationState, Rom, RomId, Shortcut, System};
use romshelf_core::Result;
use std::collections::BTreeMap;

struct Fixture {
    name: &'static str,
    file: &'static str,
    system: System,
    location: LocationState,
    in_steam: bool,
}

fn fixtures() -> Vec<Fixture> {
    use LocationState::*;
    use System::*;
    let f = |name, file, system, location, in_steam| Fixture {
        name,
        file,
        system,
        location,
        in_steam,
    };
    vec![
        f("Ember Circuit", "Ember Circuit.sfc", Snes, OnDeck, true),
        f("Tidewalker", "Tidewalker.gba", Gba, OnDeck, true),
        f(
            "Starfall Relay",
            "Starfall Relay.chd",
            Ps2,
            DriveOnly,
            false,
        ),
        f(
            "Moss & Lantern",
            "Moss & Lantern.rvz",
            Gamecube,
            OnDeck,
            false,
        ),
        f("Kite Rally", "Kite Rally.z64", N64, OnDeck, true),
        f(
            "Pocket Kingdoms",
            "Pocket Kingdoms.gba",
            Gba,
            Downloading { percent: 64 },
            false,
        ),
        f("Hollow Vane", "Hollow Vane.sfc", Snes, DriveOnly, false),
        f("Nine Orchards", "Nine Orchards.iso", Ps2, DriveOnly, false),
    ]
}

/// The fixture ROMs, in screenshot order.
pub fn roms() -> Vec<Rom> {
    fixtures()
        .into_iter()
        .enumerate()
        .map(|(i, f)| {
            let drive_file_id = format!("fixture-{i}");
            Rom {
                id: RomId::derive(f.system, &drive_file_id),
                drive_file_id,
                system: f.system,
                file_name: f.file.into(),
                display_name: f.name.into(),
                size: 1024 * (i as u64 + 1),
                checksum: None,
                location: f.location,
                recognized: true,
            }
        })
        .collect()
}

/// Fills a database with the fixture library: RetroArch installed, Dolphin missing,
/// three games in Steam.
pub fn seed(db: &Database) -> Result<()> {
    let registry = Registry::default();
    for a in registry.adapters() {
        db.upsert_emulator(&Emulator {
            id: a.id().into(),
            name: a.name().into(),
            flatpak_id: a.flatpak_id().into(),
            systems: a.systems().to_vec(),
            installed: a.id() == "retroarch",
        })?;
    }
    for (rom, f) in roms().into_iter().zip(fixtures()) {
        db.upsert_rom(&rom)?;
        if f.in_steam {
            db.upsert_shortcut(&Shortcut {
                launch_options: Shortcut::launch_options_for(&rom.id),
                app_id: 0,
                name: rom.display_name.clone(),
                target: "romshelf".into(),
                artwork: BTreeMap::new(),
                rom_id: rom.id,
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use romshelf_core::library;

    #[test]
    fn fake_data_layer_returns_the_eight_fixture_roms() {
        let db = Database::open_in_memory().unwrap();
        seed(&db).unwrap();
        let entries = library::entries(&db, &Registry::default()).unwrap();
        let mut names: Vec<_> = entries
            .iter()
            .map(|e| e.rom.display_name.as_str())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "Ember Circuit",
                "Hollow Vane",
                "Kite Rally",
                "Moss & Lantern",
                "Nine Orchards",
                "Pocket Kingdoms",
                "Starfall Relay",
                "Tidewalker"
            ]
        );
        let moss = entries
            .iter()
            .find(|e| e.rom.display_name == "Moss & Lantern")
            .unwrap();
        assert_eq!(moss.emulator.as_deref(), Some("Dolphin"));
        assert!(!moss.emulator_installed);
        assert_eq!(entries.iter().filter(|e| e.in_steam).count(), 3);
    }

    #[test]
    fn seeding_twice_changes_nothing() {
        let db = Database::open_in_memory().unwrap();
        seed(&db).unwrap();
        let first = db.roms().unwrap();
        seed(&db).unwrap();
        assert_eq!(db.roms().unwrap(), first);
    }
}
