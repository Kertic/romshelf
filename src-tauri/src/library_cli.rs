//! `romshelf download`, `romshelf delete` and `romshelf storage`.

use romshelf_core::db::Database;
use romshelf_core::model::RomId;
use romshelf_core::paths::Paths;
use romshelf_core::settings::LocalSettings;
use romshelf_core::storage::{self, human_size, DiskSpace};
use romshelf_core::transfer::{self, Downloader, Event};
use romshelf_core::{Error, Result};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

fn finish(result: Result<()>, out: &mut dyn Write) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(out, "{e}");
            1
        }
    }
}

fn rom_id(id: &str) -> Result<RomId> {
    RomId::parse(id).ok_or_else(|| {
        Error::Invalid(format!(
            "{id} isn't a ROM id. Ids look like gba-0a1b2c3d; `romshelf sync` lists the library."
        ))
    })
}

/// Downloads one ROM now, printing progress to `progress_out`.
pub fn download(
    db: &Mutex<Database>,
    dl: &Downloader,
    id: &str,
    out: &mut dyn Write,
    progress_out: &mut dyn Write,
) -> i32 {
    let result = (|| {
        let id = rom_id(id)?;
        let rom = {
            let db = db.lock().unwrap_or_else(|e| e.into_inner());
            transfer::enqueue_download(&db, &id)?;
            db.rom(&id)?
                .ok_or_else(|| Error::NotFound(format!("ROM {id}")))?
        };
        {
            let progress = Mutex::new(&mut *progress_out);
            transfer::run_pending(db, dl, &|e| {
                if let Event::Progress { bytes, total, .. } = e {
                    let pct = (bytes * 100).checked_div(total).unwrap_or(100);
                    if let Ok(mut p) = progress.lock() {
                        let _ = write!(p, "\rDownloading {}: {pct}%", rom.display_name);
                        let _ = p.flush();
                    }
                }
            })?;
        }
        writeln!(progress_out)?;
        let path = dl.paths.rom_file(rom.system, &rom.file_name)?;
        writeln!(
            out,
            "Downloaded {} to {}.",
            rom.display_name,
            path.display()
        )?;
        Ok(())
    })();
    finish(result, out)
}

/// Removes a downloaded ROM from this Deck. Drive is never touched.
pub fn remove(db: &Database, paths: &Paths, id: &str, out: &mut dyn Write) -> i32 {
    let result = (|| {
        let id = rom_id(id)?;
        let name = db
            .rom(&id)?
            .ok_or_else(|| Error::NotFound(format!("ROM {id}")))?
            .display_name;
        transfer::remove_from_deck(db, paths, &id)?;
        writeln!(out, "Removed {name} from this Deck. It's still in Drive.")?;
        Ok(())
    })();
    finish(result, out)
}

/// Shows the storage location, or moves downloaded ROMs to `set` and saves it.
pub fn storage(
    db: &Database,
    paths: &Paths,
    space: &dyn DiskSpace,
    set: Option<&str>,
    out: &mut dyn Write,
) -> i32 {
    let result = (|| {
        let paths = match set {
            None => paths.clone(),
            Some(new_root) => {
                let new_root = Path::new(new_root);
                if !new_root.is_absolute() {
                    return Err(Error::Invalid(
                        "Give the full path of the folder, for example /run/media/deck/SD/Romshelf."
                            .into(),
                    ));
                }
                if !new_root.parent().is_some_and(Path::exists) {
                    return Err(Error::Invalid(format!(
                        "{} doesn't exist. Insert the SD card, or create the folder first.",
                        new_root.parent().unwrap_or(new_root).display()
                    )));
                }
                let report = storage::change_root(db, paths, new_root)?;
                let settings_file = paths.settings_file();
                let mut settings = LocalSettings::load(&settings_file)?;
                settings.storage_root = Some(new_root.to_path_buf());
                settings.save(&settings_file)?;
                writeln!(out, "Moved {} downloaded ROMs.", report.moved)?;
                paths.clone().with_storage_root(new_root)
            }
        };
        let root = paths.storage_root();
        if !storage::is_available(&paths) {
            writeln!(out, "{}", storage::unavailable_error(&paths))?;
            return Ok(());
        }
        let free = space.available(&root)?;
        writeln!(
            out,
            "ROMs are kept in {} ({} free).",
            paths.rom_root.display(),
            human_size(free)
        )?;
        Ok(())
    })();
    finish(result, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use romshelf_core::drive::scaffold;
    use romshelf_core::drive::{DriveClient, ROOT};
    use romshelf_core::model::LocationState;
    use romshelf_core::sync::{self, SyncContext};
    use romshelf_testkit::{FakeClock, FakeDrive, FixedSpace};
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    struct Rig {
        dir: tempfile::TempDir,
        drive: FakeDrive,
        db: Mutex<Database>,
        paths: Paths,
        rom: RomId,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let drive = FakeDrive::default();
        let layout = scaffold::setup(&drive, ROOT, &mut |_: &str| {}).unwrap();
        drive
            .create_file("Kirby.gba", &layout.rom_folders["gba"], &[7; 4096])
            .unwrap();
        let paths = Paths::under(dir.path()).with_storage_root(&dir.path().join("Romshelf"));
        let db = Database::open_in_memory().unwrap();
        sync::run(&SyncContext {
            drive: &drive,
            db: &db,
            layout: &layout,
            paths: &paths,
            clock: &FakeClock::at(0),
        })
        .unwrap();
        let rom = db.roms().unwrap()[0].id.clone();
        Rig {
            dir,
            drive,
            db: Mutex::new(db),
            paths,
            rom,
        }
    }

    #[test]
    fn download_then_remove_from_the_command_line() {
        let r = rig();
        let space = FixedSpace::new(u64::MAX / 2);
        let cancel = AtomicBool::new(false);
        let dl = Downloader {
            drive: &r.drive,
            paths: &r.paths,
            space: &space,
            cancel: &cancel,
            retry_delay: Duration::ZERO,
            max_attempts: 3,
        };
        let (mut out, mut progress) = (Vec::new(), Vec::new());
        let code = download(&r.db, &dl, r.rom.as_str(), &mut out, &mut progress);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("Downloaded Kirby to"), "{text}");
        assert!(String::from_utf8(progress).unwrap().contains("100%"));

        let mut out = Vec::new();
        let db = r.db.lock().unwrap();
        let code = remove(&db, &r.paths, r.rom.as_str(), &mut out);
        assert_eq!(code, 0);
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("It's still in Drive"));
        assert_eq!(
            db.rom(&r.rom).unwrap().unwrap().location,
            LocationState::DriveOnly
        );
    }

    #[test]
    fn a_bad_id_says_what_an_id_looks_like() {
        let r = rig();
        let mut out = Vec::new();
        let db = r.db.lock().unwrap();
        assert_eq!(remove(&db, &r.paths, "kirby", &mut out), 1);
        assert!(String::from_utf8(out).unwrap().contains("gba-0a1b2c3d"));
    }

    #[test]
    fn storage_shows_and_moves_the_root() {
        let r = rig();
        let space = FixedSpace::new(3 * 1024 * 1024 * 1024);
        let db = r.db.lock().unwrap();
        let mut out = Vec::new();
        assert_eq!(storage(&db, &r.paths, &space, None, &mut out), 0);
        assert!(String::from_utf8(out).unwrap().contains("(3.0 GB free)"));

        let sd = r.dir.path().join("sd");
        std::fs::create_dir_all(&sd).unwrap();
        let target = sd.join("Romshelf");
        let mut out = Vec::new();
        let code = storage(
            &db,
            &r.paths,
            &space,
            Some(target.to_str().unwrap()),
            &mut out,
        );
        let text = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("Moved 0 downloaded ROMs."));
        let saved = LocalSettings::load(&r.paths.settings_file()).unwrap();
        assert_eq!(saved.storage_root, Some(target));

        let mut out = Vec::new();
        let missing = r.dir.path().join("no-card/Romshelf");
        assert_eq!(
            storage(
                &db,
                &r.paths,
                &space,
                Some(missing.to_str().unwrap()),
                &mut out
            ),
            1
        );
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("Insert the SD card"));
    }
}
