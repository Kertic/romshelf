//! Downloads from Drive to the Deck, and deleting from either side.
//!
//! A download writes `<file>.part`, resumes from its length with a ranged request, checks
//! size and MD5 against what Drive reports, and only then renames into place. The queue lives
//! in `transfer_queue`, so a download running when Romshelf closed resumes on the next start.

use crate::db::Database;
use crate::drive::DriveClient;
use crate::model::{LocationState, Rom, RomId};
use crate::paths::Paths;
use crate::storage::{self, human_size, DiskSpace};
use crate::{Error, Result};
use md5::{Digest, Md5};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Space left free after a download, so the Deck never fills up completely.
pub const HEADROOM: u64 = 256 * 1024 * 1024;
const KIND: &str = "download";
/// Progress events at most this often (the last one is always sent).
const EVENT_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    Queued,
    Running,
    Paused,
    Failed,
    Done,
}

impl TransferState {
    fn as_str(self) -> &'static str {
        match self {
            TransferState::Queued => "queued",
            TransferState::Running => "running",
            TransferState::Paused => "paused",
            TransferState::Failed => "failed",
            TransferState::Done => "done",
        }
    }

    fn parse(s: &str) -> Option<TransferState> {
        [
            TransferState::Queued,
            TransferState::Running,
            TransferState::Paused,
            TransferState::Failed,
            TransferState::Done,
        ]
        .into_iter()
        .find(|t| t.as_str() == s)
    }
}

/// What the interface hears about a download.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Progress {
        rom_id: RomId,
        bytes: u64,
        total: u64,
        /// Bytes per second since this download (or its resumption) started.
        rate: u64,
    },
    Finished {
        rom_id: RomId,
        ok: bool,
        /// What went wrong, for the user.
        message: Option<String>,
    },
}

pub fn part_path(local: &Path) -> PathBuf {
    let mut name = local.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    local.with_file_name(name)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn state_of(db: &Database, rom_id: &RomId) -> Result<Option<TransferState>> {
    Ok(db
        .transfer(rom_id, KIND)?
        .and_then(|(s, _)| TransferState::parse(&s)))
}

pub fn set_state(db: &Database, rom_id: &RomId, state: TransferState) -> Result<()> {
    db.set_transfer_state(rom_id, KIND, state.as_str())
}

/// Downloads waiting or under way, oldest first.
pub fn pending(db: &Database) -> Result<Vec<RomId>> {
    db.transfers_in(KIND, &["queued", "running"])
}

pub fn is_pending(db: &Database, rom_id: &RomId) -> Result<bool> {
    Ok(matches!(
        state_of(db, rom_id)?,
        Some(TransferState::Queued | TransferState::Running)
    ))
}

/// Queues a download (or "Download again") and shows the ROM as downloading. Queuing a ROM
/// that is already waiting does nothing.
pub fn enqueue_download(db: &Database, rom_id: &RomId) -> Result<()> {
    let mut rom = db
        .rom(rom_id)?
        .ok_or_else(|| Error::NotFound(format!("ROM {rom_id}")))?;
    if rom.location == LocationState::Gone {
        return Err(Error::Invalid(format!(
            "{} is no longer in Drive, so it can't be downloaded.",
            rom.display_name
        )));
    }
    if is_pending(db, rom_id)? {
        return Ok(());
    }
    db.put_transfer(rom_id, KIND, TransferState::Queued.as_str(), now())?;
    rom.location = LocationState::Downloading { percent: 0 };
    db.upsert_rom(&rom)
}

/// Takes a ROM off the queue. A running download stops through the `cancel` flag instead.
pub fn dequeue(db: &Database, paths: &Paths, rom_id: &RomId) -> Result<()> {
    db.remove_transfer(rom_id, KIND)?;
    settle_location(db, paths, rom_id)
}

/// After a download ends without a new file: on Deck if a good copy is there, else Drive only.
fn settle_location(db: &Database, paths: &Paths, rom_id: &RomId) -> Result<()> {
    if let Some(mut rom) = db.rom(rom_id)? {
        if rom.location == LocationState::Gone {
            return Ok(());
        }
        rom.location = if storage::file_matches(paths, &rom) {
            LocationState::OnDeck
        } else {
            LocationState::DriveOnly
        };
        db.upsert_rom(&rom)?;
    }
    Ok(())
}

pub struct Downloader<'a> {
    pub drive: &'a dyn DriveClient,
    pub paths: &'a Paths,
    pub space: &'a dyn DiskSpace,
    /// Set to stop the download under way.
    pub cancel: &'a AtomicBool,
    /// First wait before resuming after a dropped connection; doubles each time, up to 30 s.
    pub retry_delay: Duration,
    /// Tries per download, counting the first, before it is marked failed.
    pub max_attempts: u32,
}

/// Writes to the `.part` file, reports progress and notices cancelling.
struct PartWriter<'a> {
    file: std::fs::File,
    cancel: &'a AtomicBool,
    written: u64,
    progress: &'a mut dyn FnMut(u64),
}

impl Write for PartWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(crate::http::cancelled_write());
        }
        let n = self.file.write(buf)?;
        self.written += n as u64;
        (self.progress)(self.written);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Downloader<'_> {
    /// Downloads one ROM into place. `progress` gets the bytes held so far.
    pub fn download(&self, rom: &Rom, progress: &mut dyn FnMut(u64)) -> Result<()> {
        if !storage::is_available(self.paths) {
            return Err(storage::unavailable_error(self.paths));
        }
        let local = self.paths.rom_file(rom.system, &rom.file_name)?;
        let part = part_path(&local);
        let have = std::fs::metadata(&part)
            .map(|m| m.len())
            .unwrap_or(0)
            .min(rom.size);
        let needed = rom.size - have + HEADROOM;
        let free = self.space.available(&self.paths.storage_root())?;
        if free < needed {
            return Err(Error::External(format!(
                "Not enough space for {}: it needs {} and {} is free. Free up {} more, or choose another storage location.",
                rom.display_name,
                human_size(rom.size - have),
                human_size(free.saturating_sub(HEADROOM)),
                human_size(needed - free),
            )));
        }
        if let Some(dir) = part.parent() {
            std::fs::create_dir_all(dir)?;
        }

        let mut attempt = 0;
        loop {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&part)?;
            let mut offset = file.metadata()?.len();
            if offset > rom.size {
                // More than the whole file: something else wrote here. Start over.
                file.set_len(0)?;
                offset = 0;
            }
            if offset == rom.size {
                break;
            }
            progress(offset);
            let mut writer = PartWriter {
                file,
                cancel: self.cancel,
                written: offset,
                progress,
            };
            let result = self
                .drive
                .download_to(&rom.drive_file_id, offset, &mut writer);
            writer.file.sync_all()?;
            match result {
                Ok(_) => break,
                Err(Error::Cancelled) => {
                    let _ = std::fs::remove_file(&part);
                    return Err(Error::Cancelled);
                }
                Err(e @ Error::Network(_)) => {
                    attempt += 1;
                    if attempt >= self.max_attempts {
                        return Err(Error::Network(format!(
                            "{e}. The download of {} stopped; Romshelf resumes it when you download again.",
                            rom.display_name
                        )));
                    }
                    let delay = self
                        .retry_delay
                        .saturating_mul(1 << (attempt - 1).min(16))
                        .min(Duration::from_secs(30));
                    std::thread::sleep(delay);
                }
                Err(e) => return Err(e),
            }
        }
        if self.cancel.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(&part);
            return Err(Error::Cancelled);
        }

        let size = std::fs::metadata(&part)?.len();
        let checksum_ok = match &rom.checksum {
            Some(expected) => md5_file(&part)?.eq_ignore_ascii_case(expected),
            None => true,
        };
        if size != rom.size || !checksum_ok {
            let _ = std::fs::remove_file(&part);
            return Err(Error::External(format!(
                "The download of {} failed its checksum check, so it was discarded. Sync, then download it again.",
                rom.display_name
            )));
        }
        std::fs::rename(&part, &local)?;
        Ok(())
    }
}

fn md5_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Runs every queued download, one at a time, until none are left. Returns the last
/// failure, if any; each failure is also reported as a `Finished` event.
pub fn run_pending(db: &Mutex<Database>, dl: &Downloader, events: &dyn Fn(Event)) -> Result<()> {
    let lock = || db.lock().unwrap_or_else(|e| e.into_inner());
    let mut last_error = None;
    loop {
        let next = {
            let db = lock();
            match pending(&db)?.into_iter().next() {
                Some(id) => {
                    set_state(&db, &id, TransferState::Running)?;
                    db.rom(&id)?.map(|rom| (id, rom))
                }
                None => None,
            }
        };
        let Some((id, rom)) = next else {
            return match last_error {
                Some(e) => Err(e),
                None => Ok(()),
            };
        };
        let started = Instant::now();
        let mut start_bytes = None;
        let mut last_event: Option<Instant> = None;
        let mut last_percent = None;
        let total = rom.size;
        let mut progress = |bytes: u64| {
            let first = *start_bytes.get_or_insert(bytes);
            let done = bytes >= total;
            if !done && last_event.is_some_and(|t| t.elapsed() < EVENT_INTERVAL) {
                return;
            }
            last_event = Some(Instant::now());
            let secs = started.elapsed().as_secs_f64().max(0.001);
            events(Event::Progress {
                rom_id: id.clone(),
                bytes,
                total,
                rate: ((bytes - first) as f64 / secs) as u64,
            });
            let percent = if total == 0 {
                100
            } else {
                (bytes * 100 / total).min(100) as u8
            };
            if last_percent != Some(percent) {
                last_percent = Some(percent);
                let db = lock();
                let _ = db.set_transfer_bytes(&id, KIND, bytes);
                if let Ok(Some(mut r)) = db.rom(&id) {
                    r.location = LocationState::Downloading { percent };
                    let _ = db.upsert_rom(&r);
                }
            }
        };
        let result = dl.download(&rom, &mut progress);
        // A cancel applies to one download; the next starts clean.
        dl.cancel.store(false, Ordering::SeqCst);

        let db = lock();
        match result {
            Ok(()) => {
                set_state(&db, &id, TransferState::Done)?;
                if let Some(mut r) = db.rom(&id)? {
                    r.location = LocationState::OnDeck;
                    db.upsert_rom(&r)?;
                }
                events(Event::Finished {
                    rom_id: id,
                    ok: true,
                    message: None,
                });
            }
            Err(Error::Cancelled) => {
                dequeue(&db, dl.paths, &id)?;
                events(Event::Finished {
                    rom_id: id,
                    ok: false,
                    message: None,
                });
                last_error = Some(Error::Cancelled);
            }
            Err(e) => {
                set_state(&db, &id, TransferState::Failed)?;
                settle_location(&db, dl.paths, &id)?;
                events(Event::Finished {
                    rom_id: id,
                    ok: false,
                    message: Some(e.to_string()),
                });
                last_error = Some(e);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteScope {
    /// The local copy; Drive keeps the ROM.
    Deck,
    /// Drive's copy, moved to Drive's trash; the local copy stays.
    Drive,
    Both,
}

impl DeleteScope {
    pub fn parse(s: &str) -> Option<DeleteScope> {
        match s {
            "deck" => Some(DeleteScope::Deck),
            "drive" => Some(DeleteScope::Drive),
            "both" => Some(DeleteScope::Both),
            _ => None,
        }
    }
}

/// Deletes exactly what was chosen. Drive deletion moves the file to Drive's trash; nothing is
/// ever deleted permanently. `keep_save_backups` takes effect once save backups exist (stage 6).
pub fn delete(
    db: &Database,
    drive: &dyn DriveClient,
    paths: &Paths,
    rom_id: &RomId,
    scope: DeleteScope,
    keep_save_backups: bool,
) -> Result<()> {
    let _ = keep_save_backups;
    let mut rom = db
        .rom(rom_id)?
        .ok_or_else(|| Error::NotFound(format!("ROM {rom_id}")))?;
    let deck = matches!(scope, DeleteScope::Deck | DeleteScope::Both);
    let from_drive = matches!(scope, DeleteScope::Drive | DeleteScope::Both);
    if deck && state_of(db, rom_id)? == Some(TransferState::Running) {
        return Err(Error::Invalid(format!(
            "{} is downloading. Cancel the download, then delete it.",
            rom.display_name
        )));
    }
    // Drive first: if that fails (offline), nothing has been removed.
    if from_drive {
        match drive.trash(&rom.drive_file_id) {
            Ok(()) | Err(Error::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        rom.location = LocationState::Gone;
    }
    if deck {
        db.remove_transfer(rom_id, KIND)?;
        let local = paths.rom_file(rom.system, &rom.file_name)?;
        for path in [part_path(&local), local] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        if rom.location != LocationState::Gone {
            rom.location = LocationState::DriveOnly;
        }
    }
    db.upsert_rom(&rom)
}
