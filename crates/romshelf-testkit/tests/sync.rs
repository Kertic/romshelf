//! Stage 2: the library index built from Drive, kept current from the change feed.

use romshelf_core::db::Database;
use romshelf_core::drive::scaffold::{self, DriveLayout};
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::model::{LocationState, Rom, RomId, System};
use romshelf_core::paths::Paths;
use romshelf_core::sync::{self, SyncContext, INDEX_FILE, TOKEN_KEY};
use romshelf_core::Error;
use romshelf_testkit::{FakeClock, FakeDrive};

struct Rig {
    _dir: tempfile::TempDir,
    drive: FakeDrive,
    db: Database,
    layout: DriveLayout,
    paths: Paths,
    clock: FakeClock,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let drive = FakeDrive::default();
        let layout = scaffold::setup(&drive, ROOT, &mut |_: &str| {}).unwrap();
        Rig {
            paths: Paths::under(dir.path()),
            _dir: dir,
            drive,
            db: Database::open_in_memory().unwrap(),
            layout,
            clock: FakeClock::at(1_800_000_000),
        }
    }

    /// Another device on the same Drive, with its own empty database.
    fn second_device(&self) -> Database {
        Database::open_in_memory().unwrap()
    }

    fn ctx<'a>(&'a self, db: &'a Database) -> SyncContext<'a> {
        SyncContext {
            drive: &self.drive,
            db,
            layout: &self.layout,
            paths: &self.paths,
            clock: &self.clock,
        }
    }

    fn sync(&self) -> sync::SyncReport {
        sync::run(&self.ctx(&self.db)).unwrap()
    }

    fn folder(&self, system: System) -> String {
        self.layout.rom_folders[system.id()].clone()
    }

    fn upload(&self, system: System, name: &str) -> String {
        self.drive
            .create_file(name, &self.folder(system), b"tiny dummy")
            .unwrap()
            .id
    }

    fn by_file(&self, file_id: &str) -> Rom {
        self.db
            .roms()
            .unwrap()
            .into_iter()
            .find(|r| r.drive_file_id == file_id)
            .expect("row for file")
    }

    fn active(&self) -> Vec<Rom> {
        self.db
            .roms()
            .unwrap()
            .into_iter()
            .filter(|r| r.location != LocationState::Gone)
            .collect()
    }
}

#[test]
fn every_listed_extension_maps_to_its_system() {
    let table: &[(System, &[&str])] = &[
        (System::Snes, &["sfc", "smc", "zip", "7z"]),
        (System::Gba, &["gba", "zip", "7z"]),
        (System::N64, &["z64", "n64", "v64", "zip", "7z"]),
        (System::Gamecube, &["iso", "rvz", "gcz", "ciso"]),
        (System::Ps2, &["iso", "chd"]),
    ];
    for (system, exts) in table {
        for ext in *exts {
            assert!(system.recognizes(&format!("Game.{ext}")), "{system} {ext}");
            assert!(
                system.recognizes(&format!("Game.{}", ext.to_uppercase())),
                "{system} {ext} upper case"
            );
        }
    }
    assert!(!System::Gba.recognizes("notes.txt"));
    assert!(!System::Gba.recognizes("noextension"));
    assert!(!System::Gamecube.recognizes("Shadow.zip"));
}

#[test]
fn display_names_follow_the_rules() {
    for (file, expected) in [
        ("Tidewalker.gba", "Tidewalker"),
        ("Kite_Rally (USA) [!].z64", "Kite Rally"),
        (
            "Kirby - Nightmare in Dream Land (USA).gba",
            "Kirby Nightmare in Dream Land",
        ),
        ("super_mario_world.sfc", "Super Mario World"),
        ("pokemon-emerald-version.gba", "Pokemon Emerald Version"),
        (
            "The Legend of Zelda (Europe) (En,Fr,De) (Rev 1).z64",
            "The Legend of Zelda",
        ),
        ("Metroid Prime [GM8E01].iso", "Metroid Prime"),
        ("F-Zero (Japan).sfc", "F Zero"),
        ("ffx.chd", "Ffx"),
        ("Shadow the Hedgehog (USA).zip", "Shadow the Hedgehog"),
        ("[BIOS] (World).gba", "[BIOS] (World)"),
        ("  spaced   out  .gba", "Spaced Out"),
        ("Mixed_case-Name.n64", "Mixed case Name"),
        ("noext", "Noext"),
    ] {
        assert_eq!(sync::display_name(file), expected, "{file}");
    }
}

#[test]
fn full_scan_indexes_every_file_in_one_pass() {
    let r = Rig::new();
    let exts = [
        (System::Snes, "sfc"),
        (System::Gba, "gba"),
        (System::N64, "z64"),
        (System::Gamecube, "rvz"),
        (System::Ps2, "chd"),
    ];
    for i in 0..500 {
        let (system, ext) = exts[i % exts.len()];
        r.upload(system, &format!("Game {i}.{ext}"));
    }
    let report = r.sync();
    assert!(report.full_scan);
    assert_eq!(report.added, 500);
    assert_eq!(r.db.roms().unwrap().len(), 500);
    let gba = r
        .active()
        .into_iter()
        .filter(|x| x.system == System::Gba)
        .count();
    assert_eq!(gba, 100);
    for rom in r.db.roms().unwrap() {
        assert_eq!(rom.id, RomId::derive(rom.system, &rom.drive_file_id));
        assert_eq!(rom.location, LocationState::DriveOnly);
        assert!(rom.recognized);
    }
}

#[test]
fn a_sync_with_nothing_new_writes_nothing() {
    let r = Rig::new();
    r.upload(System::Gba, "Tidewalker.gba");
    r.upload(System::Gba, "notes.txt");
    r.sync();
    let db_before = r.db.total_changes().unwrap();
    let drive_before = r.drive.start_page_token().unwrap();
    let report = r.sync();
    assert!(!report.full_scan, "incremental the second time");
    assert_eq!(
        r.db.total_changes().unwrap(),
        db_before,
        "no database writes"
    );
    assert_eq!(
        r.drive.start_page_token().unwrap(),
        drive_before,
        "no Drive writes"
    );
    assert!(!report.changed());
}

#[test]
fn a_new_upload_appears_on_the_next_sync() {
    let r = Rig::new();
    r.sync();
    let id = r.upload(System::Snes, "Ember Circuit.sfc");
    let report = r.sync();
    assert!(!report.full_scan);
    assert_eq!(report.added, 1);
    let rom = r.by_file(&id);
    assert_eq!(rom.system, System::Snes);
    assert_eq!(rom.display_name, "Ember Circuit");
}

#[test]
fn unrecognized_files_are_recorded_but_marked() {
    let r = Rig::new();
    let txt = r.upload(System::Gba, "notes.txt");
    let zip = r.upload(System::Gamecube, "Shadow the Hedgehog (USA).zip");
    let report = r.sync();
    assert_eq!(report.unrecognized, 2);
    assert!(!r.by_file(&txt).recognized);
    assert!(!r.by_file(&zip).recognized);
}

#[test]
fn only_real_files_directly_in_a_system_folder_count() {
    let r = Rig::new();
    r.drive
        .create_file("loose.gba", &r.layout.roms, b"x")
        .unwrap();
    r.drive.create_file("elsewhere.gba", ROOT, b"x").unwrap();
    let sub = r
        .drive
        .create_folder("Hacks", &r.folder(System::Gba))
        .unwrap();
    r.drive.create_file("inside.gba", &sub.id, b"x").unwrap();
    r.drive
        .create_file_with_mime(
            "Notes",
            &r.folder(System::Gba),
            "application/vnd.google-apps.document",
        )
        .unwrap();
    r.drive
        .create_file_with_mime(
            "Link",
            &r.folder(System::Gba),
            "application/vnd.google-apps.shortcut",
        )
        .unwrap();
    r.upload(System::Gba, "real.gba");
    r.sync();
    let names: Vec<_> = r.active().into_iter().map(|x| x.file_name).collect();
    assert_eq!(names, ["real.gba"]);
}

#[test]
fn rename_keeps_the_id_and_updates_the_name() {
    let r = Rig::new();
    let id = r.upload(System::Gba, "tidewalker_beta.gba");
    r.sync();
    let before = r.by_file(&id);
    r.drive.rename(&id, "Tidewalker (USA).gba").unwrap();
    let report = r.sync();
    assert_eq!(report.updated, 1);
    let after = r.by_file(&id);
    assert_eq!(after.id, before.id);
    assert_eq!(after.file_name, "Tidewalker (USA).gba");
    assert_eq!(after.display_name, "Tidewalker");
    assert_eq!(r.db.roms().unwrap().len(), 1, "no duplicate");
}

#[test]
fn a_name_override_survives_a_rename() {
    let r = Rig::new();
    let id = r.upload(System::Gba, "pk.gba");
    r.sync();
    let rom = r.by_file(&id);
    sync::set_display_name(&r.ctx(&r.db), &rom.id, Some("Pocket Kingdoms")).unwrap();
    assert_eq!(r.by_file(&id).display_name, "Pocket Kingdoms");
    r.drive.rename(&id, "pk_v2.gba").unwrap();
    r.sync();
    let after = r.by_file(&id);
    assert_eq!(after.file_name, "pk_v2.gba");
    assert_eq!(after.display_name, "Pocket Kingdoms");

    sync::set_display_name(&r.ctx(&r.db), &rom.id, None).unwrap();
    assert_eq!(r.by_file(&id).display_name, "Pk V2");
}

#[test]
fn moving_between_system_folders_keeps_the_id() {
    let r = Rig::new();
    let id = r.upload(System::Snes, "Pocket Kingdoms.gba");
    r.sync();
    let before = r.by_file(&id);
    assert_eq!(before.system, System::Snes);
    assert!(!before.recognized, ".gba isn't a SNES file");
    r.drive.move_to(&id, &r.folder(System::Gba)).unwrap();
    r.sync();
    let after = r.by_file(&id);
    assert_eq!(after.id, before.id);
    assert_eq!(after.system, System::Gba);
    assert!(after.recognized);
}

#[test]
fn trash_marks_gone_keeps_saves_and_restore_brings_it_back() {
    let r = Rig::new();
    let id = r.upload(System::Gba, "Tidewalker.gba");
    r.sync();
    let rom = r.by_file(&id);
    r.db.add_save_backup(&rom.id, 1_799_000_000, "saves/gba/tidewalker/1.srm")
        .unwrap();

    r.drive.trash(&id).unwrap();
    let report = r.sync();
    assert_eq!(report.gone, 1);
    assert_eq!(r.by_file(&id).location, LocationState::Gone);
    assert_eq!(r.db.save_backup_count(&rom.id).unwrap(), 1);
    assert!(r.active().is_empty());

    r.drive.untrash(&id).unwrap();
    let report = r.sync();
    assert_eq!(report.restored, 1);
    let back = r.by_file(&id);
    assert_eq!(back.id, rom.id);
    assert_eq!(back.location, LocationState::DriveOnly);
}

#[test]
fn a_restored_rom_that_is_on_the_deck_shows_on_deck() {
    let r = Rig::new();
    let id = r.upload(System::Gba, "Tidewalker.gba");
    r.sync();
    r.drive.trash(&id).unwrap();
    r.sync();
    let local = r.paths.rom_file(System::Gba, "Tidewalker.gba").unwrap();
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(&local, b"tiny dummy").unwrap();
    r.drive.untrash(&id).unwrap();
    r.sync();
    assert_eq!(r.by_file(&id).location, LocationState::OnDeck);
}

#[test]
fn permanent_delete_and_moving_out_mark_gone() {
    let r = Rig::new();
    let a = r.upload(System::Gba, "A.gba");
    let b = r.upload(System::Gba, "B.gba");
    r.sync();
    r.drive.remove_permanently(&a);
    r.drive.move_to(&b, ROOT).unwrap();
    let report = r.sync();
    assert_eq!(report.gone, 2);
    assert_eq!(r.by_file(&a).location, LocationState::Gone);
    assert_eq!(r.by_file(&b).location, LocationState::Gone);
}

#[test]
fn a_full_scan_marks_missing_files_gone() {
    let r = Rig::new();
    let a = r.upload(System::Gba, "A.gba");
    r.sync();
    r.drive.remove_permanently(&a);
    r.db.set_setting(TOKEN_KEY, "").unwrap();
    let report = r.sync();
    assert!(report.full_scan);
    assert_eq!(r.by_file(&a).location, LocationState::Gone);
}

#[test]
fn a_second_device_adopts_the_first_devices_ids() {
    let r = Rig::new();
    let moved = r.upload(System::Snes, "Pocket Kingdoms.gba");
    let other = r.upload(System::N64, "kite_rally.z64");
    r.sync();
    r.drive.move_to(&moved, &r.folder(System::Gba)).unwrap();
    r.sync();
    let rom = r.by_file(&other);
    sync::set_display_name(&r.ctx(&r.db), &rom.id, Some("Kite Rally!")).unwrap();
    let a_ids: Vec<_> = r.db.roms().unwrap().into_iter().map(|x| x.id).collect();
    assert!(a_ids[1].as_str().starts_with("snes-") || a_ids[0].as_str().starts_with("snes-"));

    let b = r.second_device();
    sync::run(&r.ctx(&b)).unwrap();
    let b_rows = b.roms().unwrap();
    let mut b_ids: Vec<_> = b_rows.iter().map(|x| x.id.clone()).collect();
    let mut a_sorted = a_ids.clone();
    a_sorted.sort();
    b_ids.sort();
    assert_eq!(b_ids, a_sorted, "same ids, including the moved file's");
    let kite = b_rows.iter().find(|x| x.drive_file_id == other).unwrap();
    assert_eq!(kite.display_name, "Kite Rally!", "override adopted");
}

#[test]
fn the_index_file_is_written_only_when_something_changed() {
    let r = Rig::new();
    r.upload(System::Gba, "A.gba");
    let report = r.sync();
    assert!(report.index_written);
    let index = r
        .drive
        .list(&r.layout.meta)
        .unwrap()
        .into_iter()
        .find(|f| f.name == INDEX_FILE)
        .expect("index.json in .romshelf");
    let text = String::from_utf8(r.drive.download(&index.id, 0).unwrap()).unwrap();
    assert!(text.contains("\"schema_version\""));
    let report = r.sync();
    assert!(!report.index_written);
}

#[test]
fn a_rejected_change_token_falls_back_to_a_full_scan() {
    let r = Rig::new();
    let a = r.upload(System::Gba, "A.gba");
    r.sync();
    let expected = r.db.roms().unwrap();
    r.db.set_setting(TOKEN_KEY, "not-a-token").unwrap();
    let report = r.sync();
    assert!(report.full_scan);
    assert_eq!(r.db.roms().unwrap(), expected);
    assert_eq!(r.by_file(&a).location, LocationState::DriveOnly);
    assert_ne!(
        r.db.setting(TOKEN_KEY).unwrap().as_deref(),
        Some("not-a-token")
    );
}

#[test]
fn choosing_another_drive_folder_starts_over_with_a_full_scan() {
    let mut r = Rig::new();
    r.upload(System::Gba, "A.gba");
    r.sync();
    let games = r.drive.create_folder("Games", ROOT).unwrap();
    r.layout = scaffold::setup(&r.drive, &games.id, &mut |_: &str| {}).unwrap();
    let report = r.sync();
    assert!(report.full_scan);
    assert!(r.active().is_empty(), "the old folder's ROMs are gone");
}

#[test]
fn an_id_collision_gets_a_different_id() {
    let r = Rig::new();
    // An existing row already holds the id this file would derive.
    let taken = RomId::derive(System::Gba, "clash");
    r.db.upsert_rom(&Rom {
        id: taken.clone(),
        drive_file_id: "someone-else".into(),
        system: System::Gba,
        file_name: "Other.gba".into(),
        display_name: "Other".into(),
        size: 1,
        checksum: None,
        location: LocationState::Gone,
        recognized: true,
    })
    .unwrap();
    r.drive
        .create_file_with_id("clash", "Clash.gba", &r.folder(System::Gba), b"x")
        .unwrap();
    r.sync();
    let rom = r.by_file("clash");
    assert_ne!(rom.id, taken);
    assert!(rom.id.as_str().starts_with("gba-"));
    assert!(RomId::parse(rom.id.as_str()).is_some());
}

#[test]
fn an_index_from_a_newer_romshelf_stops_sync() {
    let r = Rig::new();
    r.drive
        .create_file(
            INDEX_FILE,
            &r.layout.meta,
            br#"{"schema_version": 99, "roms": {}}"#,
        )
        .unwrap();
    r.upload(System::Gba, "A.gba");
    let err = sync::run(&r.ctx(&r.db)).unwrap_err();
    assert!(matches!(err, Error::NeedsUpdate(_)), "{err:?}");
    assert!(r.db.roms().unwrap().is_empty());
}

#[test]
fn the_summary_line_reads_well() {
    let r = Rig::new();
    r.upload(System::Gba, "A.gba");
    r.upload(System::Gba, "B.gba");
    r.upload(System::Gba, "notes.txt");
    assert_eq!(
        r.sync().summary(),
        "Synced 3 files: 3 new, 1 not recognized."
    );
    assert_eq!(r.sync().summary(), "Synced 3 files. Nothing changed.");
}
