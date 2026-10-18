//! Local SQLite index: schema, migrations and queries.

use crate::model::{ControlProfile, Emulator, LocationState, Rom, RomId, Shortcut, System};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;

/// Each entry moves the schema up one version. Never edit a released entry; append a new one.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE systems (
    id      TEXT PRIMARY KEY,
    label   TEXT NOT NULL,
    colour  TEXT NOT NULL
);
CREATE TABLE roms (
    id            TEXT PRIMARY KEY,
    drive_file_id TEXT NOT NULL UNIQUE,
    system        TEXT NOT NULL REFERENCES systems(id),
    file_name     TEXT NOT NULL,
    display_name  TEXT NOT NULL,
    size          INTEGER NOT NULL,
    checksum      TEXT,
    location      TEXT NOT NULL,
    recognized    INTEGER NOT NULL
);
CREATE TABLE emulators (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    flatpak_id  TEXT NOT NULL,
    systems     TEXT NOT NULL,
    installed   INTEGER NOT NULL
);
CREATE TABLE save_backups (
    id          INTEGER PRIMARY KEY,
    rom_id      TEXT NOT NULL,
    taken_at    INTEGER NOT NULL,
    drive_path  TEXT NOT NULL,
    checksum    TEXT
);
CREATE TABLE control_profiles (
    emulator_id TEXT NOT NULL,
    rom_id      TEXT NOT NULL DEFAULT '',
    bindings    TEXT NOT NULL,
    PRIMARY KEY (emulator_id, rom_id)
);
CREATE TABLE shortcuts (
    rom_id          TEXT PRIMARY KEY,
    app_id          INTEGER NOT NULL,
    name            TEXT NOT NULL,
    target          TEXT NOT NULL,
    launch_options  TEXT NOT NULL,
    artwork         TEXT NOT NULL
);
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE transfer_queue (
    id          INTEGER PRIMARY KEY,
    rom_id      TEXT NOT NULL,
    kind        TEXT NOT NULL,
    bytes_done  INTEGER NOT NULL DEFAULT 0,
    state       TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);
"#];

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Database> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Database::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Database> {
        Database::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Database> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let mut db = Database { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    pub fn latest_version() -> u32 {
        MIGRATIONS.len() as u32
    }

    /// Applies pending migrations. Running it again is a no-op.
    pub fn migrate(&mut self) -> Result<()> {
        let current = self.schema_version()? as usize;
        if current > MIGRATIONS.len() {
            return Err(Error::Invalid(format!(
                "database schema {current} is newer than this Romshelf"
            )));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as u32)?;
            tx.commit()?;
        }
        self.seed_systems()
    }

    fn seed_systems(&self) -> Result<()> {
        for s in System::ALL {
            self.conn.execute(
                "INSERT INTO systems (id, label, colour) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET label = excluded.label, colour = excluded.colour",
                params![s.id(), s.label(), s.colour()],
            )?;
        }
        Ok(())
    }

    pub fn systems(&self) -> Result<Vec<System>> {
        let mut stmt = self.conn.prepare("SELECT id FROM systems ORDER BY rowid")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter()
            .map(|id| System::from_id(id).ok_or_else(|| Error::Invalid(format!("system {id}"))))
            .collect()
    }

    // ROMs

    pub fn upsert_rom(&self, rom: &Rom) -> Result<()> {
        self.conn.execute(
            "INSERT INTO roms (id, drive_file_id, system, file_name, display_name, size, checksum, location, recognized)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
               drive_file_id = excluded.drive_file_id, system = excluded.system,
               file_name = excluded.file_name, display_name = excluded.display_name,
               size = excluded.size, checksum = excluded.checksum,
               location = excluded.location, recognized = excluded.recognized",
            params![
                rom.id.as_str(),
                rom.drive_file_id,
                rom.system.id(),
                rom.file_name,
                rom.display_name,
                rom.size as i64,
                rom.checksum,
                serde_json::to_string(&rom.location)?,
                rom.recognized,
            ],
        )?;
        Ok(())
    }

    pub fn rom(&self, id: &RomId) -> Result<Option<Rom>> {
        self.conn
            .query_row(
                "SELECT id, drive_file_id, system, file_name, display_name, size, checksum, location, recognized
                 FROM roms WHERE id = ?1",
                [id.as_str()],
                rom_from_row,
            )
            .optional()?
            .transpose()
    }

    pub fn roms(&self) -> Result<Vec<Rom>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, drive_file_id, system, file_name, display_name, size, checksum, location, recognized
             FROM roms ORDER BY display_name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], rom_from_row)?;
        rows.map(|r| r?).collect()
    }

    // Emulators

    pub fn upsert_emulator(&self, e: &Emulator) -> Result<()> {
        self.conn.execute(
            "INSERT INTO emulators (id, name, flatpak_id, systems, installed) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, flatpak_id = excluded.flatpak_id,
               systems = excluded.systems, installed = excluded.installed",
            params![e.id, e.name, e.flatpak_id, serde_json::to_string(&e.systems)?, e.installed],
        )?;
        Ok(())
    }

    pub fn emulator(&self, id: &str) -> Result<Option<Emulator>> {
        self.conn
            .query_row(
                "SELECT id, name, flatpak_id, systems, installed FROM emulators WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, bool>(4)?,
                    ))
                },
            )
            .optional()?
            .map(|(id, name, flatpak_id, systems, installed)| {
                Ok(Emulator {
                    id,
                    name,
                    flatpak_id,
                    systems: serde_json::from_str(&systems)?,
                    installed,
                })
            })
            .transpose()
    }

    // Control profiles

    pub fn upsert_control_profile(&self, p: &ControlProfile) -> Result<()> {
        self.conn.execute(
            "INSERT INTO control_profiles (emulator_id, rom_id, bindings) VALUES (?1, ?2, ?3)
             ON CONFLICT(emulator_id, rom_id) DO UPDATE SET bindings = excluded.bindings",
            params![
                p.emulator_id,
                p.rom_id.as_ref().map(RomId::as_str).unwrap_or(""),
                serde_json::to_string(&p.bindings)?
            ],
        )?;
        Ok(())
    }

    pub fn control_profile(
        &self,
        emulator_id: &str,
        rom_id: Option<&RomId>,
    ) -> Result<Option<ControlProfile>> {
        let key = rom_id.map(RomId::as_str).unwrap_or("");
        self.conn
            .query_row(
                "SELECT bindings FROM control_profiles WHERE emulator_id = ?1 AND rom_id = ?2",
                [emulator_id, key],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|b| {
                Ok(ControlProfile {
                    emulator_id: emulator_id.to_string(),
                    rom_id: rom_id.cloned(),
                    bindings: serde_json::from_str(&b)?,
                })
            })
            .transpose()
    }

    // Shortcuts

    pub fn upsert_shortcut(&self, s: &Shortcut) -> Result<()> {
        self.conn.execute(
            "INSERT INTO shortcuts (rom_id, app_id, name, target, launch_options, artwork)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(rom_id) DO UPDATE SET app_id = excluded.app_id, name = excluded.name,
               target = excluded.target, launch_options = excluded.launch_options, artwork = excluded.artwork",
            params![
                s.rom_id.as_str(),
                s.app_id,
                s.name,
                s.target,
                s.launch_options,
                serde_json::to_string(&s.artwork)?
            ],
        )?;
        Ok(())
    }

    pub fn shortcut(&self, rom_id: &RomId) -> Result<Option<Shortcut>> {
        self.conn
            .query_row(
                "SELECT app_id, name, target, launch_options, artwork FROM shortcuts WHERE rom_id = ?1",
                [rom_id.as_str()],
                |r| {
                    Ok((
                        r.get::<_, u32>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .map(|(app_id, name, target, launch_options, artwork)| {
                Ok(Shortcut {
                    rom_id: rom_id.clone(),
                    app_id,
                    name,
                    target,
                    launch_options,
                    artwork: serde_json::from_str(&artwork)?,
                })
            })
            .transpose()
    }

    // Settings

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }
}

fn rom_from_row(r: &Row<'_>) -> rusqlite::Result<Result<Rom>> {
    let id: String = r.get(0)?;
    let system: String = r.get(2)?;
    let location: String = r.get(7)?;
    let size: i64 = r.get(5)?;
    let build = |drive_file_id, file_name, display_name, checksum, recognized| -> Result<Rom> {
        Ok(Rom {
            id: RomId(id.clone()),
            drive_file_id,
            system: System::from_id(&system)
                .ok_or_else(|| Error::Invalid(format!("system {system}")))?,
            file_name,
            display_name,
            size: size as u64,
            checksum,
            location: serde_json::from_str::<LocationState>(&location)?,
            recognized,
        })
    };
    Ok(build(r.get(1)?, r.get(3)?, r.get(4)?, r.get(6)?, r.get(8)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArtSlot, Binding};
    use std::collections::BTreeMap;

    fn sample_rom() -> Rom {
        Rom {
            id: RomId::derive(System::Gba, "drive-abc"),
            drive_file_id: "drive-abc".into(),
            system: System::Gba,
            file_name: "Tidewalker.gba".into(),
            display_name: "Tidewalker".into(),
            size: 16 * 1024 * 1024,
            checksum: Some("d41d8cd98f00b204e9800998ecf8427e".into()),
            location: LocationState::Downloading { percent: 64 },
            recognized: true,
        }
    }

    #[test]
    fn migrations_apply_to_empty_file_and_record_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/index.sqlite");
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), Database::latest_version());
        drop(db);
        // Reopening applies nothing new and keeps data.
        let db = Database::open(&path).unwrap();
        db.set_setting("k", "v").unwrap();
        drop(db);
        let mut db = Database::open(&path).unwrap();
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), Database::latest_version());
        assert_eq!(db.setting("k").unwrap().as_deref(), Some("v"));
        assert_eq!(db.systems().unwrap(), System::ALL.to_vec());
    }

    #[test]
    fn every_table_exists() {
        let db = Database::open_in_memory().unwrap();
        for t in [
            "roms",
            "systems",
            "emulators",
            "save_backups",
            "control_profiles",
            "shortcuts",
            "settings",
            "transfer_queue",
        ] {
            let n: i64 = db
                .conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [t],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "table {t}");
        }
    }

    #[test]
    fn rejects_newer_schema() {
        let db = Database::open_in_memory().unwrap();
        db.conn.pragma_update(None, "user_version", 999).unwrap();
        let mut db = db;
        assert!(db.migrate().is_err());
    }

    #[test]
    fn rom_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let mut rom = sample_rom();
        db.upsert_rom(&rom).unwrap();
        assert_eq!(db.rom(&rom.id).unwrap().unwrap(), rom);
        rom.location = LocationState::OnDeck;
        rom.checksum = None;
        db.upsert_rom(&rom).unwrap();
        assert_eq!(db.roms().unwrap(), vec![rom.clone()]);
        assert!(db.rom(&RomId("gba-00000000".into())).unwrap().is_none());
    }

    #[test]
    fn emulator_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let e = Emulator {
            id: "retroarch".into(),
            name: "RetroArch".into(),
            flatpak_id: "org.libretro.RetroArch".into(),
            systems: vec![System::Snes, System::Gba, System::N64],
            installed: true,
        };
        db.upsert_emulator(&e).unwrap();
        assert_eq!(db.emulator("retroarch").unwrap().unwrap(), e);
    }

    #[test]
    fn control_profile_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let mut bindings = BTreeMap::new();
        bindings.insert("L4".into(), Binding::Bound("fast_forward".into()));
        bindings.insert("R5".into(), Binding::Cleared);
        let default = ControlProfile {
            emulator_id: "retroarch".into(),
            rom_id: None,
            bindings: bindings.clone(),
        };
        let game = ControlProfile {
            rom_id: Some(RomId::derive(System::Gba, "x")),
            ..default.clone()
        };
        db.upsert_control_profile(&default).unwrap();
        db.upsert_control_profile(&game).unwrap();
        assert_eq!(
            db.control_profile("retroarch", None).unwrap().unwrap(),
            default
        );
        assert_eq!(
            db.control_profile("retroarch", game.rom_id.as_ref())
                .unwrap()
                .unwrap(),
            game
        );
    }

    #[test]
    fn shortcut_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let rom_id = RomId::derive(System::Snes, "y");
        let mut artwork = BTreeMap::new();
        artwork.insert(ArtSlot::Capsule, "123p.png".to_string());
        let s = Shortcut {
            launch_options: Shortcut::launch_options_for(&rom_id),
            rom_id,
            app_id: 3_000_000_123,
            name: "Ember Circuit".into(),
            target: "/home/deck/Applications/romshelf.AppImage".into(),
            artwork,
        };
        db.upsert_shortcut(&s).unwrap();
        assert_eq!(db.shortcut(&s.rom_id).unwrap().unwrap(), s);
    }
}
