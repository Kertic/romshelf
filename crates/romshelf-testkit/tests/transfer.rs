//! Stage 3: downloads, deletes and the storage root.

use romshelf_core::db::Database;
use romshelf_core::drive::scaffold::{self, DriveLayout};
use romshelf_core::drive::{DriveClient, DriveFile, ROOT};
use romshelf_core::model::{LocationState, Rom, System};
use romshelf_core::paths::Paths;
use romshelf_core::storage;
use romshelf_core::sync::{self, SyncContext};
use romshelf_core::transfer::{self, Downloader, Event, TransferState};
use romshelf_core::Error;
use romshelf_testkit::{FakeClock, FakeDrive, FixedSpace};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Duration;

const MB: u64 = 1024 * 1024;

struct Rig {
    dir: tempfile::TempDir,
    drive: FakeDrive,
    db: Mutex<Database>,
    layout: DriveLayout,
    paths: Paths,
    space: FixedSpace,
    cancel: AtomicBool,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let drive = FakeDrive::default();
        let layout = scaffold::setup(&drive, ROOT, &mut |_: &str| {}).unwrap();
        let paths = Paths::under(dir.path());
        std::fs::create_dir_all(&paths.rom_root).unwrap();
        Rig {
            dir,
            drive,
            db: Mutex::new(Database::open_in_memory().unwrap()),
            layout,
            paths,
            space: FixedSpace::new(64 * 1024 * MB),
            cancel: AtomicBool::new(false),
        }
    }

    /// ROMs kept on a (fake) SD card mounted at `<temp>/sdcard`.
    fn on_card() -> Rig {
        let mut r = Rig::new();
        r.paths = r
            .paths
            .clone()
            .with_storage_root(&r.dir.path().join("sdcard/Romshelf"));
        std::fs::create_dir_all(&r.paths.rom_root).unwrap();
        r
    }

    /// A first download creates the default root when its parent (home) exists.
    fn fresh_root() -> Rig {
        let mut r = Rig::new();
        r.paths = r
            .paths
            .clone()
            .with_storage_root(&r.dir.path().join("Romshelf"));
        r
    }

    /// A file in Drive with a recognisable byte pattern, synced into the index.
    fn rom(&self, system: System, name: &str, len: usize) -> Rom {
        let bytes: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
        let f = self
            .drive
            .create_file(name, &self.layout.rom_folders[system.id()], &bytes)
            .unwrap();
        self.sync();
        self.db
            .lock()
            .unwrap()
            .rom_by_drive_id(&f.id)
            .unwrap()
            .unwrap()
    }

    fn sync(&self) {
        let db = self.db.lock().unwrap();
        sync::run(&SyncContext {
            drive: &self.drive,
            db: &db,
            layout: &self.layout,
            paths: &self.paths,
            clock: &FakeClock::at(1_800_000_000),
        })
        .unwrap();
    }

    fn downloader(&self) -> Downloader<'_> {
        Downloader {
            drive: &self.drive,
            paths: &self.paths,
            space: &self.space,
            cancel: &self.cancel,
            retry_delay: Duration::ZERO,
            max_attempts: 5,
        }
    }

    fn reload(&self, rom: &Rom) -> Rom {
        self.db.lock().unwrap().rom(&rom.id).unwrap().unwrap()
    }

    fn local(&self, rom: &Rom) -> PathBuf {
        self.paths.rom_file(rom.system, &rom.file_name).unwrap()
    }

    fn content(&self, rom: &Rom) -> Vec<u8> {
        self.drive.download(&rom.drive_file_id, 0).unwrap()
    }

    fn download(&self, rom: &Rom) -> Result<(), Error> {
        transfer::enqueue_download(&self.db.lock().unwrap(), &rom.id).unwrap();
        transfer::run_pending(&self.db, &self.downloader(), &|_: Event| {})
    }

    /// Every file under the temp root and every Drive file, for "nothing else changed" checks.
    fn snapshot(&self) -> (BTreeMap<PathBuf, Vec<u8>>, Vec<DriveFile>) {
        let mut files = BTreeMap::new();
        walk(self.dir.path(), &mut files);
        (files, self.drive.all_files())
    }
}

fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_none_or(|e| e != "sqlite") {
            out.insert(p.clone(), std::fs::read(&p).unwrap());
        }
    }
}

#[test]
fn a_download_arrives_verified_and_on_deck() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 300_000);
    r.download(&rom).unwrap();
    let local = r.local(&rom);
    assert_eq!(std::fs::read(&local).unwrap(), r.content(&rom));
    assert!(!local.with_file_name("Kirby.gba.part").exists());
    assert_eq!(r.reload(&rom).location, LocationState::OnDeck);
    let db = r.db.lock().unwrap();
    assert_eq!(
        transfer::state_of(&db, &rom.id).unwrap(),
        Some(TransferState::Done)
    );
}

#[test]
fn an_interrupted_download_resumes_from_where_it_stopped() {
    let r = Rig::new();
    let rom = r.rom(System::N64, "Kite Rally.z64", 1_000_000);
    r.drive.fail_next_download_after(400_000);
    r.download(&rom).unwrap();
    assert_eq!(
        r.drive.download_offsets(&rom.drive_file_id),
        vec![0, 400_000]
    );
    assert_eq!(std::fs::read(r.local(&rom)).unwrap(), r.content(&rom));
    assert_eq!(r.reload(&rom).location, LocationState::OnDeck);
}

#[test]
fn a_restart_with_a_running_download_resumes_it() {
    let r = Rig::new();
    let rom = r.rom(System::Snes, "Ember.sfc", 500_000);
    {
        let db = r.db.lock().unwrap();
        transfer::enqueue_download(&db, &rom.id).unwrap();
        transfer::set_state(&db, &rom.id, TransferState::Running).unwrap();
    }
    // The previous run got 200 000 bytes into the .part file before the app closed.
    let part = transfer::part_path(&r.local(&rom));
    std::fs::create_dir_all(part.parent().unwrap()).unwrap();
    std::fs::write(&part, &r.content(&rom)[..200_000]).unwrap();

    transfer::run_pending(&r.db, &r.downloader(), &|_: Event| {}).unwrap();
    assert_eq!(r.drive.download_offsets(&rom.drive_file_id), vec![200_000]);
    assert_eq!(std::fs::read(r.local(&rom)).unwrap(), r.content(&rom));
}

#[test]
fn a_checksum_mismatch_discards_the_file() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 50_000);
    r.drive
        .set_md5(&rom.drive_file_id, "00000000000000000000000000000000");
    r.sync();
    let err = r.download(&rom).unwrap_err();
    assert!(err.to_string().contains("checksum"), "{err}");
    assert!(!r.local(&rom).exists());
    assert!(!transfer::part_path(&r.local(&rom)).exists());
    assert_eq!(r.reload(&rom).location, LocationState::DriveOnly);
    let db = r.db.lock().unwrap();
    assert_eq!(
        transfer::state_of(&db, &rom.id).unwrap(),
        Some(TransferState::Failed)
    );
}

#[test]
fn not_enough_space_writes_nothing_and_names_the_shortfall() {
    let mut r = Rig::new();
    let rom = r.rom(System::Gamecube, "Big.rvz", 10_000);
    r.space = FixedSpace::new(5_000);
    let before = r.snapshot();
    let err = r.download(&rom).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("Not enough space"), "{text}");
    assert!(text.contains("more"), "names the shortfall: {text}");
    assert!(r.drive.download_offsets(&rom.drive_file_id).is_empty());
    assert_eq!(r.snapshot().0, before.0, "nothing written");
    assert_eq!(r.reload(&rom).location, LocationState::DriveOnly);
}

#[test]
fn space_is_checked_with_headroom() {
    let mut r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 1_000);
    // Room for the file but not the headroom.
    r.space = FixedSpace::new(1_000 + transfer::HEADROOM - 1);
    assert!(r.download(&rom).is_err());
    r.space = FixedSpace::new(1_000 + transfer::HEADROOM);
    r.download(&rom).unwrap();
}

#[test]
fn cancelling_removes_the_partial_file() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 200_000);
    r.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    let err = r.download(&rom).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert!(!transfer::part_path(&r.local(&rom)).exists());
    assert!(!r.local(&rom).exists());
    assert_eq!(r.reload(&rom).location, LocationState::DriveOnly);
    let db = r.db.lock().unwrap();
    assert_eq!(transfer::state_of(&db, &rom.id).unwrap(), None);
}

#[test]
fn queueing_marks_the_rom_downloading_once() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 1_000);
    let db = r.db.lock().unwrap();
    transfer::enqueue_download(&db, &rom.id).unwrap();
    transfer::enqueue_download(&db, &rom.id).unwrap();
    assert_eq!(transfer::pending(&db).unwrap().len(), 1);
    assert_eq!(
        db.rom(&rom.id).unwrap().unwrap().location,
        LocationState::Downloading { percent: 0 }
    );
}

#[test]
fn progress_events_end_at_the_total() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 3 * MB as usize);
    transfer::enqueue_download(&r.db.lock().unwrap(), &rom.id).unwrap();
    let events = Mutex::new(Vec::new());
    transfer::run_pending(&r.db, &r.downloader(), &|e: Event| {
        events.lock().unwrap().push(e)
    })
    .unwrap();
    let events = events.into_inner().unwrap();
    let progress: Vec<(u64, u64)> = events
        .iter()
        .filter_map(|e| match e {
            Event::Progress { bytes, total, .. } => Some((*bytes, *total)),
            _ => None,
        })
        .collect();
    assert!(!progress.is_empty());
    assert!(
        progress.windows(2).all(|w| w[0].0 <= w[1].0),
        "never goes back"
    );
    assert_eq!(*progress.last().unwrap(), (3 * MB, 3 * MB));
    assert!(matches!(
        events.last(),
        Some(Event::Finished { ok: true, .. })
    ));
}

#[test]
fn removing_from_the_deck_leaves_drive_alone() {
    let r = Rig::new();
    let keep = r.rom(System::Gba, "Keep.gba", 1_000);
    let rom = r.rom(System::Gba, "Kirby.gba", 1_000);
    r.download(&keep).unwrap();
    r.download(&rom).unwrap();
    let (files, drive) = r.snapshot();

    transfer::remove_from_deck(&r.db.lock().unwrap(), &r.paths, &rom.id).unwrap();
    assert!(!r.local(&rom).exists());
    assert_eq!(r.reload(&rom).location, LocationState::DriveOnly);
    assert_eq!(r.drive.all_files(), drive, "Drive untouched");
    let mut expected = files;
    expected.remove(&r.local(&rom));
    assert_eq!(r.snapshot().0, expected, "only that file removed");
}

#[test]
fn downloading_and_removing_never_change_rom_files_in_drive() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 50_000);
    let before: Vec<DriveFile> = r
        .drive
        .all_files()
        .into_iter()
        .filter(|f| f.parents.contains(&r.layout.rom_folders["gba"]))
        .collect();
    r.download(&rom).unwrap();
    transfer::remove_from_deck(&r.db.lock().unwrap(), &r.paths, &rom.id).unwrap();
    r.download(&rom).unwrap();
    r.sync();
    let after: Vec<DriveFile> = r
        .drive
        .all_files()
        .into_iter()
        .filter(|f| f.parents.contains(&r.layout.rom_folders["gba"]))
        .collect();
    assert_eq!(after, before);
}

#[test]
fn removing_cancels_a_queued_download() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 1_000);
    let db = r.db.lock().unwrap();
    transfer::enqueue_download(&db, &rom.id).unwrap();
    transfer::remove_from_deck(&db, &r.paths, &rom.id).unwrap();
    assert!(transfer::pending(&db).unwrap().is_empty());
    assert_eq!(
        db.rom(&rom.id).unwrap().unwrap().location,
        LocationState::DriveOnly
    );
}

#[test]
fn reconcile_matches_rows_to_files_on_disk() {
    let r = Rig::new();
    let gone_local = r.rom(System::Gba, "A.gba", 1_000);
    let copied_in = r.rom(System::Gba, "B.gba", 1_000);
    let wrong_size = r.rom(System::Gba, "C.gba", 1_000);
    r.download(&gone_local).unwrap();
    r.download(&wrong_size).unwrap();
    std::fs::remove_file(r.local(&gone_local)).unwrap();
    std::fs::write(r.local(&copied_in), r.content(&copied_in)).unwrap();
    std::fs::write(r.local(&wrong_size), b"short").unwrap();

    let report = storage::reconcile(&r.db.lock().unwrap(), &r.paths).unwrap();
    assert!(!report.unavailable);
    assert_eq!(report.fixed, 3);
    assert_eq!(r.reload(&gone_local).location, LocationState::DriveOnly);
    assert_eq!(r.reload(&copied_in).location, LocationState::OnDeck);
    assert_eq!(r.reload(&wrong_size).location, LocationState::DriveOnly);
}

#[test]
fn a_missing_storage_root_is_unavailable_not_deleted() {
    let r = Rig::on_card();
    let rom = r.rom(System::Gba, "Kirby.gba", 1_000);
    r.download(&rom).unwrap();
    // The SD card is removed: its mount point disappears with it.
    std::fs::remove_dir_all(r.dir.path().join("sdcard")).unwrap();
    let report = storage::reconcile(&r.db.lock().unwrap(), &r.paths).unwrap();
    assert!(report.unavailable);
    assert_eq!(report.fixed, 0);
    assert_eq!(r.reload(&rom).location, LocationState::OnDeck);
}

#[test]
fn a_download_refuses_while_storage_is_unavailable() {
    let r = Rig::on_card();
    let rom = r.rom(System::Gba, "Kirby.gba", 1_000);
    std::fs::remove_dir_all(r.dir.path().join("sdcard")).unwrap();
    let err = r.download(&rom).unwrap_err();
    assert!(err.to_string().contains("isn't available"), "{err}");
    assert!(!r.paths.storage_root().exists(), "not recreated");
}

#[test]
fn moving_the_storage_root_keeps_downloaded_roms() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 20_000);
    r.download(&rom).unwrap();
    let new_root = r.dir.path().join("sdcard/Romshelf");
    std::fs::create_dir_all(new_root.parent().unwrap()).unwrap();
    let moved = storage::change_root(&r.db.lock().unwrap(), &r.paths, &new_root).unwrap();
    assert_eq!(moved.moved, 1);
    let new_paths = r.paths.clone().with_storage_root(&new_root);
    let new_local = new_paths.rom_file(rom.system, &rom.file_name).unwrap();
    assert_eq!(std::fs::read(&new_local).unwrap(), r.content(&rom));
    assert!(!r.local(&rom).exists());
    assert_eq!(r.reload(&rom).location, LocationState::OnDeck);
    assert_eq!(
        r.drive.download_offsets(&rom.drive_file_id),
        vec![0],
        "not downloaded again"
    );
}

#[test]
fn moving_to_a_root_that_holds_a_different_file_does_not_overwrite_it() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba", 20_000);
    r.download(&rom).unwrap();
    let new_root = r.dir.path().join("other");
    let theirs = new_root.join("roms/gba/Kirby.gba");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, b"someone else's file").unwrap();
    let err = storage::change_root(&r.db.lock().unwrap(), &r.paths, &new_root).unwrap_err();
    assert!(err.to_string().contains("already"), "{err}");
    assert_eq!(std::fs::read(&theirs).unwrap(), b"someone else's file");
    assert!(r.local(&rom).exists(), "nothing moved");
}

#[test]
fn the_first_download_creates_the_default_root() {
    let r = Rig::fresh_root();
    assert!(!r.paths.storage_root().exists());
    let rom = r.rom(System::Gba, "Kirby.gba", 1_000);
    let report = storage::reconcile(&r.db.lock().unwrap(), &r.paths).unwrap();
    assert!(
        !report.unavailable,
        "a root not made yet is not a missing card"
    );
    r.download(&rom).unwrap();
    assert!(r
        .local(&rom)
        .starts_with(r.dir.path().join("Romshelf/roms/gba")));
}
