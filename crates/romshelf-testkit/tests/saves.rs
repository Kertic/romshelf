//! Stage 6: save backup, restore, conflicts and save states.

use romshelf_core::db::Database;
use romshelf_core::drive::scaffold::{self, DriveLayout};
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::emulators::setup;
use romshelf_core::emulators::{Dolphin, EmulatorAdapter, Registry, RetroArch};
use romshelf_core::launch::{self, LaunchContext, LaunchEvent, Target};
use romshelf_core::model::{LocationState, Rom, System};
use romshelf_core::paths::Paths;
use romshelf_core::saves::{
    self, BackupOutcome, SaveCheck, SaveChoice, SaveConflict, SaveContext, KEEP_SETTING,
};
use romshelf_core::sync::{self, SyncContext};
use romshelf_core::Error;
use romshelf_testkit::{
    FakeClock, FakeDrive, FakeHttp, FakePackageRunner, FakeProcessRunner, FixedSpace,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One machine with its own folders and database. Machines in a test share one Drive.
struct Device {
    _dir: tempfile::TempDir,
    name: &'static str,
    drive: Arc<FakeDrive>,
    layout: DriveLayout,
    db: Mutex<Database>,
    paths: Paths,
    registry: Registry,
    runner: FakePackageRunner,
    http: FakeHttp,
    process: FakeProcessRunner,
    space: FixedSpace,
    cancel: AtomicBool,
    clock: FakeClock,
    /// What the launch's quick "can Google be reached?" check answers.
    online: Arc<AtomicBool>,
    reachable: Box<dyn Fn() -> bool + Sync>,
}

fn shared_drive() -> (Arc<FakeDrive>, DriveLayout) {
    let drive = Arc::new(FakeDrive::default());
    let layout = scaffold::setup(drive.as_ref(), ROOT, &mut |_: &str| {}).unwrap();
    (drive, layout)
}

fn no_answer(_: &SaveConflict) -> Option<SaveChoice> {
    None
}

impl Device {
    fn new(name: &'static str, drive: &Arc<FakeDrive>, layout: &DriveLayout) -> Device {
        let dir = tempfile::tempdir().unwrap();
        let online = Arc::new(AtomicBool::new(true));
        let paths = Paths::under(dir.path()).with_storage_root(&dir.path().join("Romshelf"));
        for core in RetroArch.required_cores() {
            let f = RetroArch::core_file(&paths, core);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, b"\x7fELF core").unwrap();
        }
        Device {
            _dir: dir,
            name,
            drive: drive.clone(),
            layout: layout.clone(),
            db: Mutex::new(Database::open_in_memory().unwrap()),
            paths,
            registry: Registry::default(),
            runner: FakePackageRunner::simulating(&[RetroArch.flatpak_id(), Dolphin.flatpak_id()]),
            http: FakeHttp::default(),
            process: FakeProcessRunner::default(),
            space: FixedSpace::new(1 << 40),
            cancel: AtomicBool::new(false),
            clock: FakeClock::at(1_800_000_000),
            online: online.clone(),
            reachable: Box::new(move || online.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    fn alone() -> Device {
        let (drive, layout) = shared_drive();
        Device::new("deck-a", &drive, &layout)
    }

    fn saves(&self) -> SaveContext<'_> {
        SaveContext {
            db: &self.db,
            drive: self.drive.as_ref(),
            layout: &self.layout,
            registry: &self.registry,
            paths: &self.paths,
            clock: &self.clock,
            device: self.name,
        }
    }

    fn launch_ctx<'a>(
        &'a self,
        ask: &'a (dyn Fn(&SaveConflict) -> Option<SaveChoice> + Sync),
    ) -> LaunchContext<'a> {
        LaunchContext {
            db: &self.db,
            registry: &self.registry,
            paths: &self.paths,
            drive: self.drive.as_ref(),
            layout: Some(&self.layout),
            runner: &self.runner,
            http: &self.http,
            process: &self.process,
            space: &self.space,
            cancel: &self.cancel,
            clock: &self.clock,
            retry_delay: Duration::ZERO,
            max_attempts: 3,
            device: self.name,
            ask,
            online: self.reachable.as_ref(),
        }
    }

    /// Syncs the library from Drive and returns the game, finding or uploading its file.
    fn game(&self, system: System, name: &str) -> Rom {
        let folder = &self.layout.rom_folders[system.id()];
        let existing = self
            .drive
            .list(folder)
            .unwrap()
            .into_iter()
            .find(|f| f.name == name);
        let f = existing.unwrap_or_else(|| {
            self.drive
                .create_file(name, folder, b"tiny dummy rom")
                .unwrap()
        });
        let db = self.db.lock().unwrap();
        sync::run(&SyncContext {
            drive: self.drive.as_ref(),
            db: &db,
            layout: &self.layout,
            paths: &self.paths,
            clock: &self.clock,
        })
        .unwrap();
        db.rom_by_drive_id(&f.id).unwrap().unwrap()
    }

    /// The game downloaded with its emulator set up, so a launch goes straight to the emulator.
    fn ready(&self, system: System, name: &str) -> Rom {
        let rom = self.game(system, name);
        let local = self.paths.rom_file(rom.system, &rom.file_name).unwrap();
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        std::fs::write(&local, b"tiny dummy rom").unwrap();
        let db = self.db.lock().unwrap();
        let mut r = db.rom(&rom.id).unwrap().unwrap();
        r.location = LocationState::OnDeck;
        db.upsert_rom(&r).unwrap();
        setup::run_setup(
            &setup::SetupContext {
                runner: &self.runner,
                http: &self.http,
                db: &db,
                paths: &self.paths,
                drive: Some((self.drive.as_ref(), &self.layout)),
            },
            self.registry.require(system).unwrap(),
            &mut |_| {},
        )
        .unwrap();
        r
    }

    fn save_dir(&self, rom: &Rom) -> PathBuf {
        self.registry
            .require(rom.system)
            .unwrap()
            .save_dir(rom, &self.paths)
    }

    /// Writes the game's save on this Deck, as the emulator would.
    fn write_save(&self, rom: &Rom, file: &str, bytes: &[u8]) {
        let dir = self.save_dir(rom);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), bytes).unwrap();
    }

    fn read_save(&self, rom: &Rom, file: &str) -> Vec<u8> {
        std::fs::read(self.save_dir(rom).join(file)).unwrap()
    }

    /// Plays the game; the "emulator" writes `save` to the save file while it runs.
    fn play(&self, rom: &Rom, file: &str, save: &[u8]) -> (Result<i32, Error>, Vec<LaunchEvent>) {
        self.play_asking(rom, file, save, &no_answer)
    }

    fn play_asking(
        &self,
        rom: &Rom,
        file: &str,
        save: &[u8],
        ask: &(dyn Fn(&SaveConflict) -> Option<SaveChoice> + Sync),
    ) -> (Result<i32, Error>, Vec<LaunchEvent>) {
        let dir = self.save_dir(rom);
        let (file, save) = (file.to_string(), save.to_vec());
        self.process.during_run(move |_| {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(&file), &save).unwrap();
        });
        let mut events = Vec::new();
        let result = launch::run(
            &self.launch_ctx(ask),
            &Target::Id(rom.id.to_string()),
            &mut |e| events.push(e),
        );
        (result, events)
    }

    fn versions(&self, rom: &Rom) -> Vec<saves::SaveVersion> {
        saves::versions(&self.saves(), rom).unwrap()
    }
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

// Backup and restore

#[test]
fn backup_then_restore_gives_back_identical_files() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    let save: Vec<u8> = (0..=255u8).cycle().take(32 * 1024).collect();
    d.write_save(&rom, "Kirby.srm", &save);
    d.write_save(&rom, "Kirby.rtc", b"\x00\x01clock");
    let outcome = saves::backup(&d.saves(), &rom).unwrap();
    let BackupOutcome::Uploaded { version } = outcome else {
        panic!("expected an upload, got {outcome:?}");
    };
    assert_eq!(version.device, "deck-a");
    assert_eq!(version.core.as_deref(), Some("mgba"));

    std::fs::remove_dir_all(d.save_dir(&rom)).unwrap();
    saves::restore(&d.saves(), &rom, &version.id).unwrap();
    assert_eq!(d.read_save(&rom, "Kirby.srm"), save);
    assert_eq!(d.read_save(&rom, "Kirby.rtc"), b"\x00\x01clock");
    assert_eq!(file_names(&d.save_dir(&rom)), ["Kirby.rtc", "Kirby.srm"]);
}

#[test]
fn backups_land_in_a_timestamped_folder_with_a_manifest() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.write_save(&rom, "Kirby.srm", b"progress");
    saves::backup(&d.saves(), &rom).unwrap();
    let system = d
        .drive
        .list(&d.layout.saves)
        .unwrap()
        .into_iter()
        .find(|f| f.name == "gba")
        .unwrap();
    let game = d.drive.list(&system.id).unwrap();
    assert_eq!(game.len(), 1);
    assert_eq!(game[0].name, rom.id.as_str());
    let versions = d.drive.list(&game[0].id).unwrap();
    assert_eq!(versions[0].name, saves::utc_stamp(1_800_000_000));
    let mut files: Vec<String> = d
        .drive
        .list(&versions[0].id)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    files.sort();
    assert_eq!(files, ["Kirby.srm", "manifest.json"]);
}

#[test]
fn an_unchanged_save_makes_no_new_version() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.write_save(&rom, "Kirby.srm", b"progress");
    saves::backup(&d.saves(), &rom).unwrap();
    d.clock.advance(60);
    assert_eq!(
        saves::backup(&d.saves(), &rom).unwrap(),
        BackupOutcome::Unchanged
    );
    assert_eq!(d.versions(&rom).len(), 1);
    d.write_save(&rom, "Kirby.srm", b"more progress");
    assert!(matches!(
        saves::backup(&d.saves(), &rom).unwrap(),
        BackupOutcome::Uploaded { .. }
    ));
    assert_eq!(d.versions(&rom).len(), 2);
}

#[test]
fn no_save_yet_uploads_nothing() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    assert_eq!(
        saves::backup(&d.saves(), &rom).unwrap(),
        BackupOutcome::NoSave
    );
    assert!(d.versions(&rom).is_empty());
}

#[test]
fn retention_keeps_exactly_the_newest_n() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.db.lock().unwrap().set_setting(KEEP_SETTING, "3").unwrap();
    for i in 0..5 {
        d.write_save(&rom, "Kirby.srm", format!("progress {i}").as_bytes());
        saves::backup(&d.saves(), &rom).unwrap();
        d.clock.advance(60);
    }
    let kept = d.versions(&rom);
    assert_eq!(kept.len(), 3);
    let times: Vec<i64> = kept.iter().map(|v| v.taken_at).collect();
    assert_eq!(
        times,
        [1_800_000_240, 1_800_000_180, 1_800_000_120],
        "newest first"
    );
    // The older two went to Drive's trash, not deleted.
    let trashed = d
        .drive
        .all_files()
        .into_iter()
        .filter(|f| f.trashed && f.is_folder())
        .count();
    assert_eq!(trashed, 2);
}

#[test]
fn ten_versions_are_kept_by_default() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    for i in 0..12 {
        d.write_save(&rom, "Kirby.srm", format!("progress {i}").as_bytes());
        saves::backup(&d.saves(), &rom).unwrap();
        d.clock.advance(60);
    }
    assert_eq!(d.versions(&rom).len(), saves::DEFAULT_KEEP);
}

#[test]
fn two_backups_in_the_same_second_both_keep() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.write_save(&rom, "Kirby.srm", b"one");
    saves::backup(&d.saves(), &rom).unwrap();
    d.write_save(&rom, "Kirby.srm", b"two");
    saves::backup(&d.saves(), &rom).unwrap();
    let kept = d.versions(&rom);
    assert_eq!(kept.len(), 2);
    // The newest is the second one.
    saves::restore(&d.saves(), &rom, &kept[1].id).unwrap();
    assert_eq!(d.read_save(&rom, "Kirby.srm"), b"one");
}

#[test]
fn restoring_backs_up_the_save_it_replaces() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.write_save(&rom, "Kirby.srm", b"old progress");
    saves::backup(&d.saves(), &rom).unwrap();
    let old = d.versions(&rom)[0].clone();
    d.clock.advance(60);
    // Played since, without a backup.
    d.write_save(&rom, "Kirby.srm", b"new progress");

    d.clock.advance(60);
    saves::restore(&d.saves(), &rom, &old.id).unwrap();
    assert_eq!(d.read_save(&rom, "Kirby.srm"), b"old progress");
    let kept = d.versions(&rom);
    assert_eq!(kept.len(), 2);
    let safety = &kept[0];
    saves::restore(&d.saves(), &rom, &safety.id).unwrap();
    assert_eq!(d.read_save(&rom, "Kirby.srm"), b"new progress");
    // Restoring back to a save Drive already has made no further copy.
    assert_eq!(d.versions(&rom).len(), 2);
}

#[test]
fn restoring_an_older_version_keeps_the_newer_one_listed() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    for text in ["first", "second"] {
        d.write_save(&rom, "Kirby.srm", text.as_bytes());
        saves::backup(&d.saves(), &rom).unwrap();
        d.clock.advance(60);
    }
    let kept = d.versions(&rom);
    saves::restore(&d.saves(), &rom, &kept[1].id).unwrap();
    assert_eq!(d.read_save(&rom, "Kirby.srm"), b"first");
    assert_eq!(d.versions(&rom), kept);
}

#[test]
fn a_restore_that_fails_leaves_the_deck_save_alone() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.write_save(&rom, "Kirby.srm", b"backed up");
    saves::backup(&d.saves(), &rom).unwrap();
    let v = d.versions(&rom)[0].clone();
    d.write_save(&rom, "Kirby.srm", b"current");
    d.drive.set_offline(true);
    assert!(saves::restore(&d.saves(), &rom, &v.id).is_err());
    assert_eq!(d.read_save(&rom, "Kirby.srm"), b"current");
    assert_eq!(file_names(&d.save_dir(&rom)), ["Kirby.srm"]);
}

#[test]
fn restoring_a_save_from_another_core_warns() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.write_save(&rom, "Kirby.srm", b"progress");
    saves::backup(&d.saves(), &rom).unwrap();
    let v = d.versions(&rom)[0].clone();
    let manifest = d
        .drive
        .list(&v.id)
        .unwrap()
        .into_iter()
        .find(|f| f.name == saves::MANIFEST)
        .unwrap();
    let mut m: saves::Manifest =
        serde_json::from_slice(&d.drive.download(&manifest.id, 0).unwrap()).unwrap();
    m.core = Some("vba_next".into());
    d.drive
        .update_content(&manifest.id, &serde_json::to_vec(&m).unwrap())
        .unwrap();
    let out = saves::restore(&d.saves(), &rom, &v.id).unwrap();
    assert_eq!(
        out.warning.as_deref(),
        Some("This save was made with retroarch (vba_next); retroarch (mgba) may not load it.")
    );
}

#[test]
fn backups_only_touch_romshelfs_saves_folder_in_drive() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    let before: Vec<_> = d
        .drive
        .all_files()
        .into_iter()
        .filter(|f| !f.is_folder())
        .collect();
    d.write_save(&rom, "Kirby.srm", b"progress");
    saves::backup(&d.saves(), &rom).unwrap();
    for f in before {
        assert_eq!(d.drive.get(&f.id).unwrap(), f, "{} changed", f.name);
    }
}

// The check before a launch

#[test]
fn the_conflict_matrix() {
    let (drive, layout) = shared_drive();
    let a = Device::new("deck-a", &drive, &layout);
    let b = Device::new("deck-b", &drive, &layout);
    let rom_a = a.ready(System::Gba, "Kirby.gba");
    let rom_b = b.ready(System::Gba, "Kirby.gba");
    assert_eq!(rom_a.id, rom_b.id);

    // Agreed: A backed up, B restored it.
    a.write_save(&rom_a, "Kirby.srm", b"v1");
    saves::backup(&a.saves(), &rom_a).unwrap();
    let SaveCheck::DriveNewer { version } = saves::check(&b.saves(), &rom_b).unwrap() else {
        panic!("B should see Drive's save");
    };
    saves::restore(&b.saves(), &rom_b, &version.id).unwrap();

    // Neither changed.
    assert_eq!(saves::check(&a.saves(), &rom_a).unwrap(), SaveCheck::InSync);
    assert_eq!(saves::check(&b.saves(), &rom_b).unwrap(), SaveCheck::InSync);

    // Only this Deck changed.
    b.write_save(&rom_b, "Kirby.srm", b"v2 on b");
    assert_eq!(
        saves::check(&b.saves(), &rom_b).unwrap(),
        SaveCheck::DeckNewer
    );

    // Only Drive changed (for A, after B backed up).
    b.clock.advance(60);
    saves::backup(&b.saves(), &rom_b).unwrap();
    let check = saves::check(&a.saves(), &rom_a).unwrap();
    let SaveCheck::DriveNewer { version } = check else {
        panic!("expected Drive newer, got {check:?}");
    };
    assert_eq!(version.device, "deck-b");

    // Both changed: A played too without seeing B's.
    a.clock.advance(120);
    a.write_save(&rom_a, "Kirby.srm", b"v2 on a");
    let check = saves::check(&a.saves(), &rom_a).unwrap();
    let SaveCheck::Conflict { conflict } = check else {
        panic!("expected a conflict, got {check:?}");
    };
    assert_eq!(conflict.deck.device, "deck-a");
    assert_eq!(conflict.drive.device, "deck-b");
    assert_eq!(conflict.drive.taken_at, 1_800_000_060);
    assert_eq!(conflict.name, rom_a.display_name);
}

#[test]
fn both_sides_reaching_the_same_save_is_no_conflict() {
    let (drive, layout) = shared_drive();
    let a = Device::new("deck-a", &drive, &layout);
    let b = Device::new("deck-b", &drive, &layout);
    let rom = a.ready(System::Gba, "Kirby.gba");
    let rom_b = b.ready(System::Gba, "Kirby.gba");
    a.write_save(&rom, "Kirby.srm", b"same");
    saves::backup(&a.saves(), &rom).unwrap();
    b.write_save(&rom_b, "Kirby.srm", b"same");
    assert_eq!(saves::check(&b.saves(), &rom_b).unwrap(), SaveCheck::InSync);
}

#[test]
fn a_save_removed_from_drive_is_backed_up_again() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.write_save(&rom, "Kirby.srm", b"progress");
    saves::backup(&d.saves(), &rom).unwrap();
    let v = d.versions(&rom)[0].clone();
    d.drive.trash(&v.id).unwrap();
    assert_eq!(
        saves::check(&d.saves(), &rom).unwrap(),
        SaveCheck::DeckNewer
    );
}

// The launch pipeline

#[test]
fn playing_backs_up_the_save_afterwards() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    let (code, events) = d.play(&rom, "Kirby.gba.srm", b"after playing");
    assert_eq!(code.unwrap(), 0);
    let kept = d.versions(&rom);
    assert_eq!(kept.len(), 1);
    assert!(events.contains(&LaunchEvent::Notice {
        message: format!("Backed up {}", rom.display_name)
    }));
    let s = d.db.lock().unwrap().save_sync(&rom.id).unwrap();
    assert!(!s.pending);
    assert_eq!(s.last_backup_at, Some(1_800_000_000));
    assert_eq!(s.last_backup_device.as_deref(), Some("deck-a"));
}

#[test]
fn the_backup_runs_after_a_non_zero_exit() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.process.push_exit(3, Duration::ZERO);
    let (code, _) = d.play(&rom, "Kirby.srm", b"before the crash");
    assert_eq!(code.unwrap(), 3);
    assert_eq!(d.versions(&rom).len(), 1);
}

#[test]
fn drive_unavailable_at_exit_queues_the_backup() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.drive.set_offline(true);
    d.process.push_exit(0, Duration::ZERO);
    let (code, events) = d.play(&rom, "Kirby.srm", b"offline progress");
    assert_eq!(code.unwrap(), 0, "the exit code is the emulator's");
    assert!(events.contains(&LaunchEvent::Notice {
        message: "Couldn't back up the save. Romshelf tries again next time.".into()
    }));
    let s = d.db.lock().unwrap().save_sync(&rom.id).unwrap();
    assert!(s.pending);
    assert!(s.last_error.is_some());

    d.drive.set_offline(false);
    let flushed = saves::flush_pending(&d.saves());
    assert_eq!(flushed.len(), 1);
    assert!(matches!(flushed[0].1, Ok(BackupOutcome::Uploaded { .. })));
    let s = d.db.lock().unwrap().save_sync(&rom.id).unwrap();
    assert!(!s.pending);
    assert_eq!(s.last_error, None);
    assert_eq!(d.versions(&rom).len(), 1);
}

#[test]
fn a_game_whose_launch_was_ended_is_backed_up_on_the_next_run() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    // Steam's "Exit game" ends Romshelf with the emulator: the backup owed at the start is
    // never done, and the save is on the Deck.
    saves::mark_pending(&d.db.lock().unwrap(), &rom.id).unwrap();
    d.write_save(&rom, "Kirby.srm", b"progress before Exit game");
    let other = d.ready(System::Snes, "Ember.sfc");
    d.play(&other, "Ember.srm", b"ember").0.unwrap();
    assert_eq!(
        d.versions(&rom).len(),
        1,
        "owed backup done at the next launch"
    );
    assert!(!d.db.lock().unwrap().save_sync(&rom.id).unwrap().pending);
}

#[test]
fn owed_backups_wait_while_a_game_is_running() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    saves::mark_pending(&d.db.lock().unwrap(), &rom.id).unwrap();
    d.write_save(&rom, "Kirby.srm", b"still playing");
    let lock = d.paths.launch_lock();
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, std::process::id().to_string()).unwrap();
    assert!(saves::flush_pending(&d.saves()).is_empty());
    assert!(d.versions(&rom).is_empty());
    std::fs::remove_file(&lock).unwrap();
    assert_eq!(saves::flush_pending(&d.saves()).len(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn a_lock_left_by_an_ended_romshelf_does_not_block_backups() {
    let d = Device::alone();
    let lock = d.paths.launch_lock();
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    // No process has this id.
    std::fs::write(&lock, "4194304").unwrap();
    assert!(!launch::game_running(&d.paths));
}

#[test]
fn launching_restores_a_newer_save_from_another_deck() {
    let (drive, layout) = shared_drive();
    let a = Device::new("deck-a", &drive, &layout);
    let b = Device::new("deck-b", &drive, &layout);
    let rom = a.ready(System::Gba, "Kirby.gba");
    a.play(&rom, "Kirby.srm", b"played on a").0.unwrap();
    let rom_b = b.ready(System::Gba, "Kirby.gba");
    // B's emulator sees A's save when it starts.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (dir, seen2) = (b.save_dir(&rom_b), seen.clone());
    b.process.during_run(move |_| {
        *seen2.lock().unwrap() = std::fs::read(dir.join("Kirby.srm")).unwrap_or_default();
    });
    let mut events = Vec::new();
    launch::run(
        &b.launch_ctx(&no_answer),
        &Target::Id(rom_b.id.to_string()),
        &mut |e| events.push(e),
    )
    .unwrap();
    assert_eq!(*seen.lock().unwrap(), b"played on a");
    assert!(events.contains(&LaunchEvent::Notice {
        message: "Restored the newer save from deck-a".into()
    }));
}

#[test]
fn two_decks_taking_turns_lose_nothing_and_are_never_asked() {
    let (drive, layout) = shared_drive();
    let a = Device::new("deck-a", &drive, &layout);
    let b = Device::new("deck-b", &drive, &layout);
    let rom_a = a.ready(System::Gba, "Kirby.gba");
    let rom_b = b.ready(System::Gba, "Kirby.gba");
    let never = |c: &SaveConflict| -> Option<SaveChoice> { panic!("asked about {c:?}") };
    for turn in 0..4 {
        let (deck, rom) = if turn % 2 == 0 {
            (&a, &rom_a)
        } else {
            (&b, &rom_b)
        };
        deck.clock.set(1_800_000_000 + turn * 600);
        let save = format!("progress after turn {turn}");
        deck.play_asking(rom, "Kirby.srm", save.as_bytes(), &never)
            .0
            .unwrap();
        assert_eq!(deck.read_save(rom, "Kirby.srm"), save.as_bytes());
    }
    let kept = a.versions(&rom_a);
    assert_eq!(kept.len(), 4, "one version per turn");
    let devices: Vec<&str> = kept.iter().map(|v| v.device.as_str()).collect();
    assert_eq!(devices, ["deck-b", "deck-a", "deck-b", "deck-a"]);
}

#[test]
fn a_conflict_with_no_one_to_ask_starts_nothing() {
    let (drive, layout) = shared_drive();
    let a = Device::new("deck-a", &drive, &layout);
    let b = Device::new("deck-b", &drive, &layout);
    let rom_a = a.ready(System::Gba, "Kirby.gba");
    let rom_b = b.ready(System::Gba, "Kirby.gba");
    a.play(&rom_a, "Kirby.srm", b"a").0.unwrap();
    b.write_save(&rom_b, "Kirby.srm", b"b, never backed up");
    let (result, _) = b.play(&rom_b, "Kirby.srm", b"unused");
    assert!(matches!(result, Err(Error::NeedsAnswer)));
    assert!(b.process.runs().is_empty());
    assert_eq!(b.read_save(&rom_b, "Kirby.srm"), b"b, never backed up");
}

fn conflicted() -> (Device, Device, Rom) {
    let (drive, layout) = shared_drive();
    let a = Device::new("deck-a", &drive, &layout);
    let b = Device::new("deck-b", &drive, &layout);
    let rom_a = a.ready(System::Gba, "Kirby.gba");
    let rom_b = b.ready(System::Gba, "Kirby.gba");
    a.play(&rom_a, "Kirby.srm", b"drive's").0.unwrap();
    b.clock.advance(60);
    b.write_save(&rom_b, "Kirby.srm", b"this deck's");
    (a, b, rom_b)
}

#[test]
fn keeping_this_decks_save_makes_it_drives_newest() {
    let (_a, b, rom) = conflicted();
    let ask = |_: &SaveConflict| Some(SaveChoice::KeepDeck);
    // The emulator leaves the save as it found it.
    let dir = b.save_dir(&rom);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    b.process.during_run(move |_| {
        *seen2.lock().unwrap() = std::fs::read(dir.join("Kirby.srm")).unwrap();
    });
    launch::run(
        &b.launch_ctx(&ask),
        &Target::Id(rom.id.to_string()),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(*seen.lock().unwrap(), b"this deck's");
    let kept = b.versions(&rom);
    assert_eq!(kept.len(), 2, "Drive's save is still there");
    assert_eq!(kept[0].device, "deck-b");
}

#[test]
fn using_drives_save_backs_up_this_decks_first() {
    let (_a, b, rom) = conflicted();
    let ask = |_: &SaveConflict| Some(SaveChoice::UseDrive);
    let (code, _) = b.play_asking(&rom, "Kirby.srm", b"played after", &ask);
    code.unwrap();
    let kept = b.versions(&rom);
    // Drive's, then this Deck's as a safety copy, then the save after playing.
    assert_eq!(kept.len(), 3);
    let safety = kept[1].clone();
    assert_eq!(safety.device, "deck-b");
    saves::restore(&b.saves(), &rom, &safety.id).unwrap();
    assert_eq!(b.read_save(&rom, "Kirby.srm"), b"this deck's");
}

#[test]
fn cancelling_a_conflict_starts_nothing() {
    let (_a, b, rom) = conflicted();
    let ask = |_: &SaveConflict| Some(SaveChoice::Cancel);
    let (result, _) = b.play_asking(&rom, "Kirby.srm", b"unused", &ask);
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(b.process.runs().is_empty());
    assert_eq!(b.read_save(&rom, "Kirby.srm"), b"this deck's");
}

#[test]
fn offline_at_the_start_plays_the_deck_save() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.write_save(&rom, "Kirby.srm", b"local");
    d.drive.set_offline(true);
    let (code, _) = d.play(&rom, "Kirby.srm", b"local, played");
    assert_eq!(code.unwrap(), 0);
    assert_eq!(d.process.runs().len(), 1);
}

// Setup

#[test]
fn setup_links_save_folders_in_drive() {
    let d = Device::alone();
    d.ready(System::Gamecube, "Twilight Princess.rvz");
    for top in [&d.layout.saves, &d.layout.states] {
        let names: Vec<String> = d
            .drive
            .list(top)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, ["gamecube"]);
    }
}

// Where each emulator keeps saves

#[test]
fn retroarch_saves_in_a_folder_per_game() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    let session = RetroArch.session_dir(&d.paths);
    assert_eq!(
        RetroArch.save_dir(&rom, &d.paths),
        session.join("saves").join(rom.id.as_str())
    );
    assert_eq!(
        RetroArch.state_dir(&rom, &d.paths),
        Some(session.join("states").join(rom.id.as_str()))
    );
    RetroArch.prepare_session(&d.paths).unwrap();
    RetroArch.prepare_game(&rom, &d.paths).unwrap();
    let cfg = std::fs::read_to_string(RetroArch::game_config(&d.paths, &rom)).unwrap();
    assert!(cfg.contains(&format!(
        "savefile_directory = \"{}\"",
        RetroArch.save_dir(&rom, &d.paths).display()
    )));
    assert!(cfg.contains(&format!(
        "savestate_directory = \"{}\"",
        RetroArch.state_dir(&rom, &d.paths).unwrap().display()
    )));
    // Whatever the user's own settings, saves go straight into the folder given.
    let base = std::fs::read_to_string(RetroArch::append_config(&d.paths)).unwrap();
    for key in [
        "savefiles_in_content_dir",
        "savestates_in_content_dir",
        "sort_savefiles_enable",
        "sort_savestates_enable",
        "sort_savefiles_by_content_enable",
        "sort_savestates_by_content_enable",
    ] {
        assert!(
            base.contains(&format!("{key} = \"false\"")),
            "{key} missing"
        );
    }
}

#[test]
fn retroarch_moves_saves_from_the_shared_folder_into_the_games() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby - Nightmare in Dream Land (USA).gba");
    let session = RetroArch.session_dir(&d.paths);
    let shared = session.join("saves");
    std::fs::write(
        shared.join("Kirby - Nightmare in Dream Land (USA).srm"),
        b"from before",
    )
    .unwrap();
    std::fs::write(shared.join("Other Game.srm"), b"not this game's").unwrap();
    std::fs::write(
        session
            .join("states")
            .join("Kirby - Nightmare in Dream Land (USA).state1"),
        b"state",
    )
    .unwrap();
    RetroArch.prepare_game(&rom, &d.paths).unwrap();
    assert_eq!(
        d.read_save(&rom, "Kirby - Nightmare in Dream Land (USA).srm"),
        b"from before"
    );
    assert!(shared.join("Other Game.srm").exists());
    let states = RetroArch.state_dir(&rom, &d.paths).unwrap();
    assert_eq!(
        file_names(&states),
        ["Kirby - Nightmare in Dream Land (USA).state1"]
    );
}

/// A tiny file with a GameCube disc header in the given container.
fn disc(path: &Path, magic: &[u8], id_at: usize, id: &[u8]) {
    let mut bytes = vec![0u8; id_at + 0x20];
    bytes[..magic.len()].copy_from_slice(magic);
    bytes[id_at..id_at + id.len()].copy_from_slice(id);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn dolphin_reads_the_game_id_from_each_format() {
    let dir = tempfile::tempdir().unwrap();
    let p = |n: &str| dir.path().join(n);
    disc(&p("a.iso"), b"", 0, b"GZ2E01");
    disc(&p("b.rvz"), b"RVZ\x01", 0x58, b"GZ2E01");
    disc(&p("c.wia"), b"WIA\x01", 0x58, b"GALE01");
    disc(&p("d.ciso"), b"CISO", 0x8000, b"GM4E01");
    disc(&p("e.gcz"), b"\x01\xc0\x0b\xb1", 0x58, b"GZ2E01");
    assert_eq!(Dolphin::game_id(&p("a.iso")).as_deref(), Some("GZ2E01"));
    assert_eq!(Dolphin::game_id(&p("b.rvz")).as_deref(), Some("GZ2E01"));
    assert_eq!(Dolphin::game_id(&p("c.wia")).as_deref(), Some("GALE01"));
    assert_eq!(Dolphin::game_id(&p("d.ciso")).as_deref(), Some("GM4E01"));
    assert_eq!(Dolphin::game_id(&p("e.gcz")), None);
    assert_eq!(Dolphin::game_id(&p("missing.iso")), None);
}

#[test]
fn dolphin_gives_each_game_its_own_memory_card_folder() {
    let d = Device::alone();
    let rom = d.ready(System::Gamecube, "Twilight Princess.rvz");
    let rom_file = d.paths.rom_file(rom.system, &rom.file_name).unwrap();
    disc(&rom_file, b"RVZ\x01", 0x58, b"GZ2E01");
    let card = Dolphin.session_dir(&d.paths).join("GC/USA/Card A");
    std::fs::create_dir_all(&card).unwrap();
    std::fs::write(card.join("01-GZ2E-gczelda2.gci"), b"link's save").unwrap();
    std::fs::write(card.join("01-GALE-SuperSmashBros0110290334.gci"), b"melee").unwrap();

    Dolphin.prepare_game(&rom, &d.paths).unwrap();
    let own = Dolphin.save_dir(&rom, &d.paths);
    assert_eq!(
        own,
        Dolphin
            .session_dir(&d.paths)
            .join("GC/Romshelf")
            .join(rom.id.as_str())
    );
    assert_eq!(file_names(&own), ["01-GZ2E-gczelda2.gci"]);
    assert_eq!(file_names(&card), ["01-GALE-SuperSmashBros0110290334.gci"]);
    assert_eq!(Dolphin.state_dir(&rom, &d.paths), None);
}

// Save states

#[test]
fn save_states_are_mirrored_listed_fetched_and_deleted() {
    let (drive, layout) = shared_drive();
    let a = Device::new("deck-a", &drive, &layout);
    let b = Device::new("deck-b", &drive, &layout);
    let rom = a.ready(System::Gba, "Kirby.gba");
    let states = RetroArch.state_dir(&rom, &a.paths).unwrap();
    std::fs::create_dir_all(&states).unwrap();
    std::fs::write(states.join("Kirby.state1"), b"slot one").unwrap();
    std::fs::write(states.join("Kirby.state1.png"), b"thumb").unwrap();
    std::fs::write(states.join("Kirby.state2"), b"slot two").unwrap();
    assert_eq!(saves::mirror_states(&a.saves(), &rom).unwrap(), 3);
    assert_eq!(saves::mirror_states(&a.saves(), &rom).unwrap(), 0);
    std::fs::write(states.join("Kirby.state2"), b"slot two, later").unwrap();
    assert_eq!(saves::mirror_states(&a.saves(), &rom).unwrap(), 1);

    let slots: Vec<String> = saves::state_slots(&a.paths, &RetroArch, &rom)
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(slots, ["Kirby.state1", "Kirby.state2"]);

    let rom_b = b.ready(System::Gba, "Kirby.gba");
    assert_eq!(saves::fetch_states(&b.saves(), &rom_b).unwrap(), 3);
    let b_states = RetroArch.state_dir(&rom_b, &b.paths).unwrap();
    assert_eq!(
        std::fs::read(b_states.join("Kirby.state2")).unwrap(),
        b"slot two, later"
    );

    saves::delete_state(&a.saves(), &rom, "Kirby.state1").unwrap();
    assert_eq!(file_names(&states), ["Kirby.state2"]);
    // Its Drive copy is in Drive's trash, not gone.
    let trashed: Vec<String> = drive
        .all_files()
        .into_iter()
        .filter(|f| f.trashed)
        .map(|f| f.name)
        .collect();
    assert_eq!(trashed.len(), 2);
    assert!(trashed.contains(&"Kirby.state1".to_string()));
    assert!(saves::delete_state(&a.saves(), &rom, "../Kirby.state2").is_err());
}

#[test]
fn a_state_made_only_on_this_deck_is_uploaded_before_it_is_deleted() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    let states = RetroArch.state_dir(&rom, &d.paths).unwrap();
    std::fs::create_dir_all(&states).unwrap();
    std::fs::write(states.join("Kirby.state3"), b"only here").unwrap();
    saves::delete_state(&d.saves(), &rom, "Kirby.state3").unwrap();
    let copy = d
        .drive
        .all_files()
        .into_iter()
        .find(|f| f.name == "Kirby.state3")
        .unwrap();
    assert!(copy.trashed);
    assert_eq!(d.drive.download(&copy.id, 0).unwrap(), b"only here");
}

#[test]
fn states_are_mirrored_when_the_game_ends() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    let states = RetroArch.state_dir(&rom, &d.paths).unwrap();
    d.process.during_run(move |_| {
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(states.join("Kirby.state"), b"quick save").unwrap();
    });
    launch::run(
        &d.launch_ctx(&no_answer),
        &Target::Id(rom.id.to_string()),
        &mut |_| {},
    )
    .unwrap();
    assert!(d
        .drive
        .all_files()
        .iter()
        .any(|f| f.name == "Kirby.state" && !f.trashed));
}

#[test]
fn retroarch_saves_follow_a_rename_in_drive() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    RetroArch.prepare_game(&rom, &d.paths).unwrap();
    d.write_save(&rom, "Kirby.srm", b"progress");
    d.write_save(&rom, "Kirby.rtc", b"clock");
    let states = RetroArch.state_dir(&rom, &d.paths).unwrap();
    std::fs::write(states.join("Kirby.state1"), b"slot").unwrap();
    std::fs::write(states.join("Kirby.state1.png"), b"thumb").unwrap();
    let mut renamed = rom.clone();
    renamed.file_name = "Kirby (USA).gba".into();
    RetroArch.prepare_game(&renamed, &d.paths).unwrap();
    assert_eq!(
        file_names(&d.save_dir(&rom)),
        ["Kirby (USA).rtc", "Kirby (USA).srm"]
    );
    assert_eq!(d.read_save(&rom, "Kirby (USA).srm"), b"progress");
    assert_eq!(
        file_names(&states),
        ["Kirby (USA).state1", "Kirby (USA).state1.png"]
    );
}

#[test]
fn a_save_under_both_names_is_left_alone() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby (USA).gba");
    d.write_save(&rom, "Kirby.srm", b"old progress");
    d.write_save(&rom, "Kirby (USA).srm", b"started again");
    RetroArch.prepare_game(&rom, &d.paths).unwrap();
    assert_eq!(
        file_names(&d.save_dir(&rom)),
        ["Kirby (USA).srm", "Kirby.srm"]
    );
    assert_eq!(d.read_save(&rom, "Kirby.srm"), b"old progress");
}

// Stage 10: a restore cut short by a crash

#[test]
fn a_restore_cut_short_is_repaired_at_the_next_start() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    let dir = d.save_dir(&rom);
    let aside = dir.with_file_name(format!("{}.replaced", rom.id));
    let incoming = dir.with_file_name(format!("{}.restoring", rom.id));

    // Ended after moving the save aside, before the restored one went in.
    d.write_save(&rom, "Kirby.srm", b"the real save");
    std::fs::rename(&dir, &aside).unwrap();
    std::fs::create_dir_all(&incoming).unwrap();
    std::fs::write(incoming.join("Kirby.srm"), b"half a download").unwrap();
    let fixed = saves::recover_interrupted(&d.db.lock().unwrap(), &d.registry, &d.paths).unwrap();
    assert_eq!(fixed, 2);
    assert_eq!(d.read_save(&rom, "Kirby.srm"), b"the real save");
    assert!(!aside.exists() && !incoming.exists());

    // Ended after the restored save went in, before the old one was cleared.
    std::fs::create_dir_all(&aside).unwrap();
    std::fs::write(aside.join("Kirby.srm"), b"old").unwrap();
    saves::recover_interrupted(&d.db.lock().unwrap(), &d.registry, &d.paths).unwrap();
    assert_eq!(d.read_save(&rom, "Kirby.srm"), b"the real save");
    assert!(!aside.exists());

    // Nothing to do: nothing changes.
    assert_eq!(
        saves::recover_interrupted(&d.db.lock().unwrap(), &d.registry, &d.paths).unwrap(),
        0
    );
}

#[test]
fn offline_a_downloaded_game_launches_with_no_network_at_any_step() {
    let d = Device::alone();
    let rom = d.ready(System::Gba, "Kirby.gba");
    d.drive.set_offline(true);
    let (code, _) = d.play(&rom, "Kirby.srm", b"played offline");
    assert_eq!(code.unwrap(), 0);
    assert_eq!(d.process.runs().len(), 1, "the emulator ran");
    assert!(
        d.db.lock().unwrap().save_sync(&rom.id).unwrap().pending,
        "the backup is queued"
    );
}

#[test]
fn offline_at_the_start_goes_straight_to_the_game_and_leaves_the_backup_owed() {
    // Found on the Deck (2026-10-22): offline, each Drive call before the game retried for about
    // sixteen seconds, so a launch seemed to do nothing until the network came back.
    let (drive, layout) = shared_drive();
    let a = Device::new("deck-a", &drive, &layout);
    let b = Device::new("deck-b", &drive, &layout);
    let rom = a.ready(System::Gba, "Kirby.gba");
    a.play(&rom, "Kirby.srm", b"played on a").0.unwrap();
    let rom_b = b.ready(System::Gba, "Kirby.gba");
    b.write_save(&rom_b, "Kirby.srm", b"b's own");
    // Google can't be reached from B. (The fake Drive itself still answers, so anything that
    // used it anyway would show: a restore of A's save, or a new backup.)
    b.online.store(false, std::sync::atomic::Ordering::SeqCst);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (dir, seen2) = (b.save_dir(&rom_b), seen.clone());
    b.process.during_run(move |_| {
        *seen2.lock().unwrap() = std::fs::read(dir.join("Kirby.srm")).unwrap_or_default();
    });
    let mut events = Vec::new();
    let code = launch::run(
        &b.launch_ctx(&no_answer),
        &Target::Id(rom_b.id.to_string()),
        &mut |e| events.push(e),
    );
    assert_eq!(code.unwrap(), 0);
    assert_eq!(
        *seen.lock().unwrap(),
        b"b's own",
        "Drive's save wasn't fetched"
    );
    assert!(events.contains(&LaunchEvent::Notice {
        message: "Offline: playing with the save on this Deck. Its backup waits for the network."
            .into()
    }));
    assert_eq!(
        a.versions(&rom).len(),
        1,
        "no backup was tried after the game"
    );
    assert!(b.db.lock().unwrap().save_sync(&rom_b.id).unwrap().pending);

    // Back online: the owed backup goes up.
    b.online.store(true, std::sync::atomic::Ordering::SeqCst);
    let flushed = saves::flush_pending(&b.saves());
    assert!(matches!(flushed[0].1, Ok(BackupOutcome::Uploaded { .. })));
}
