//! Save backup, restore and conflict rules, and save states.
//!
//! On the Deck, each game's saves are in a folder of its own that its emulator is pointed at
//! (`EmulatorAdapter::save_dir`). In Drive, each backup is a folder of its own:
//! `Emulation/saves/<system>/<rom-id>/<UTC time>/` holding the files and a `manifest.json`,
//! written last, so a backup without one is incomplete and ignored. Save states are mirrored
//! to `Emulation/states/<system>/<rom-id>/`.
//!
//! Romshelf only writes inside its own `saves/` and `states/` folders, and only ever moves its
//! own backups to Drive's trash (keeping the newest few). Before a save on the Deck is replaced,
//! it is backed up.

use crate::clock::Clock;
use crate::db::Database;
use crate::drive::scaffold::DriveLayout;
use crate::drive::{DriveClient, DriveFile};
use crate::emulators::{EmulatorAdapter, Registry};
use crate::model::{Rom, RomId, SaveSync};
use crate::paths::Paths;
use crate::transfer::md5_file;
use crate::{Error, Result};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub const MANIFEST: &str = "manifest.json";
pub const MANIFEST_SCHEMA: u32 = 1;
/// Backups kept per game unless the user chooses otherwise (the `save_versions` setting).
pub const DEFAULT_KEEP: usize = 10;
pub const KEEP_SETTING: &str = "save_versions";

/// One file of a save, as recorded in a backup's manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveFile {
    pub name: String,
    pub size: u64,
    pub md5: String,
}

/// `manifest.json` in each backup folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub rom_id: String,
    pub taken_at: i64,
    /// The machine the save came from ("steamdeck").
    pub device: String,
    pub emulator: String,
    /// RetroArch's core: a save from one core may not load in another.
    pub core: Option<String>,
    pub files: Vec<SaveFile>,
    /// `set_hash` of `files`.
    pub hash: String,
}

/// A backup in Drive, as the restore list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveVersion {
    /// The backup folder's Drive id.
    pub id: String,
    pub taken_at: i64,
    pub device: String,
    pub size: u64,
    pub emulator: String,
    pub core: Option<String>,
    pub hash: String,
}

impl SaveVersion {
    fn from_manifest(id: &str, m: &Manifest) -> SaveVersion {
        SaveVersion {
            id: id.into(),
            taken_at: m.taken_at,
            device: m.device.clone(),
            size: m.files.iter().map(|f| f.size).sum(),
            emulator: m.emulator.clone(),
            core: m.core.clone(),
            hash: m.hash.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackupOutcome {
    Uploaded {
        version: SaveVersion,
    },
    /// The newest backup already has this save.
    Unchanged,
    /// The game has no save on this Deck yet.
    NoSave,
}

/// One side of a conflict: when its save was made and on which machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveSide {
    pub taken_at: i64,
    pub device: String,
}

/// This Deck and Drive both changed since they last agreed (UI guide section 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveConflict {
    pub rom_id: String,
    pub name: String,
    pub deck: SaveSide,
    pub drive: SaveSide,
}

/// The save check before a launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SaveCheck {
    InSync,
    /// Only this Deck's save changed: it is backed up when the game ends.
    DeckNewer,
    /// Only Drive's changed (played on another machine): restore it.
    DriveNewer {
        version: SaveVersion,
    },
    Conflict {
        conflict: SaveConflict,
    },
}

/// The answer to a save conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveChoice {
    /// Keep this Deck's save; it becomes Drive's newest backup.
    KeepDeck,
    /// Use Drive's newest save; this Deck's is backed up first.
    UseDrive,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreOutcome {
    pub version: SaveVersion,
    /// Set when the save was made with another emulator or core.
    pub warning: Option<String>,
}

/// A save state slot on the Deck.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StateSlot {
    pub name: String,
    pub size: u64,
    /// Seconds since the epoch.
    pub modified: i64,
}

pub struct SaveContext<'a> {
    pub db: &'a Mutex<Database>,
    pub drive: &'a dyn DriveClient,
    pub layout: &'a DriveLayout,
    pub registry: &'a Registry,
    pub paths: &'a Paths,
    pub clock: &'a dyn Clock,
    /// This machine's name, recorded in each backup.
    pub device: &'a str,
}

impl SaveContext<'_> {
    fn db(&self) -> MutexGuard<'_, Database> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn adapter(&self, rom: &Rom) -> Result<&dyn EmulatorAdapter> {
        self.registry.require(rom.system)
    }

    fn keep(&self) -> usize {
        self.db()
            .setting(KEEP_SETTING)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_KEEP)
    }

    fn update_sync(&self, rom: &Rom, f: impl FnOnce(&mut SaveSync)) -> Result<()> {
        let db = self.db();
        let mut s = db.save_sync(&rom.id)?;
        f(&mut s);
        db.put_save_sync(&rom.id, &s)
    }
}

/// This machine's name for backups: `/etc/hostname` on the Deck ("steamdeck").
pub fn device_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Steam Deck".into())
}

/// The files of a save folder (not sub-folders), by name.
pub fn local_files(dir: &Path) -> Result<Vec<SaveFile>> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        out.push(SaveFile {
            name: entry.file_name().to_string_lossy().into_owned(),
            size: entry.metadata()?.len(),
            md5: md5_file(&path)?,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// One hash for a whole save: its files' names and checksums. No files hashes to `EMPTY`.
pub fn set_hash(files: &[SaveFile]) -> String {
    let mut sorted: Vec<&SaveFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut h = Md5::new();
    for f in sorted {
        h.update(f.name.as_bytes());
        h.update(b"\t");
        h.update(f.md5.as_bytes());
        h.update(b"\n");
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The hash of no save at all, which is also where a game starts before any sync.
pub fn empty_hash() -> String {
    set_hash(&[])
}

/// Backup folder names: `20261019T153500Z`, which sort in time order.
pub fn utc_stamp(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn find_folder(drive: &dyn DriveClient, parent: &str, name: &str) -> Result<Option<String>> {
    let mut found: Vec<DriveFile> = drive
        .list(parent)?
        .into_iter()
        .filter(|f| f.is_folder() && !f.trashed && f.name == name)
        .collect();
    // Duplicates are possible in Drive; pick one the same way every time.
    found.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(found.into_iter().next().map(|f| f.id))
}

fn ensure_folder(drive: &dyn DriveClient, parent: &str, name: &str) -> Result<String> {
    match find_folder(drive, parent, name)? {
        Some(id) => Ok(id),
        None => Ok(drive.create_folder(name, parent)?.id),
    }
}

/// `saves/<system>/` and `states/<system>/` for each of an emulator's systems: setup's "Link
/// saves" step.
pub fn link_folders(
    drive: &dyn DriveClient,
    layout: &DriveLayout,
    adapter: &dyn EmulatorAdapter,
) -> Result<()> {
    for system in adapter.systems() {
        ensure_folder(drive, &layout.saves, system.id())?;
        ensure_folder(drive, &layout.states, system.id())?;
    }
    Ok(())
}

/// `<top>/<system>/<rom-id>/`, created when `create` is set.
fn game_folder(ctx: &SaveContext, top: &str, rom: &Rom, create: bool) -> Result<Option<String>> {
    let lookup = |parent: &str, name: &str| -> Result<Option<String>> {
        if create {
            ensure_folder(ctx.drive, parent, name).map(Some)
        } else {
            find_folder(ctx.drive, parent, name)
        }
    };
    let Some(system) = lookup(top, rom.system.id())? else {
        return Ok(None);
    };
    lookup(&system, rom.id.as_str())
}

/// Backup folders, newest first.
fn version_folders(ctx: &SaveContext, game: &str) -> Result<Vec<DriveFile>> {
    let mut v: Vec<DriveFile> = ctx
        .drive
        .list(game)?
        .into_iter()
        .filter(|f| f.is_folder() && !f.trashed)
        .collect();
    v.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(v)
}

/// A backup's manifest; `None` when it has none (an upload that didn't finish).
fn read_manifest(ctx: &SaveContext, folder: &str) -> Result<Option<Manifest>> {
    let Some(file) = ctx
        .drive
        .list(folder)?
        .into_iter()
        .find(|f| f.name == MANIFEST && !f.is_folder())
    else {
        return Ok(None);
    };
    let bytes = ctx.drive.download(&file.id, 0)?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    if m.schema_version > MANIFEST_SCHEMA {
        return Err(Error::NeedsUpdate(format!("A save backup in {}", folder)));
    }
    Ok(Some(m))
}

/// Complete backups in Drive, newest first.
pub fn versions(ctx: &SaveContext, rom: &Rom) -> Result<Vec<SaveVersion>> {
    let Some(game) = game_folder(ctx, &ctx.layout.saves, rom, false)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for folder in version_folders(ctx, &game)? {
        if let Some(m) = read_manifest(ctx, &folder.id)? {
            out.push(SaveVersion::from_manifest(&folder.id, &m));
        }
    }
    Ok(out)
}

fn newest_version(ctx: &SaveContext, game: &str) -> Result<Option<SaveVersion>> {
    for folder in version_folders(ctx, game)? {
        if let Some(m) = read_manifest(ctx, &folder.id)? {
            return Ok(Some(SaveVersion::from_manifest(&folder.id, &m)));
        }
    }
    Ok(None)
}

/// Uploads the save folder as a new backup: the files, then the manifest.
fn upload_version(
    ctx: &SaveContext,
    rom: &Rom,
    game: &str,
    dir: &Path,
    files: &[SaveFile],
) -> Result<SaveVersion> {
    let adapter = ctx.adapter(rom)?;
    let taken_at = ctx.clock.now();
    let existing: Vec<String> = ctx.drive.list(game)?.into_iter().map(|f| f.name).collect();
    let base = utc_stamp(taken_at);
    let mut name = base.clone();
    let mut n = 2;
    while existing.contains(&name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    let folder = ctx.drive.create_folder(&name, game)?;
    for f in files {
        let bytes = std::fs::read(dir.join(&f.name))?;
        ctx.drive.create_file(&f.name, &folder.id, &bytes)?;
    }
    let manifest = Manifest {
        schema_version: MANIFEST_SCHEMA,
        rom_id: rom.id.to_string(),
        taken_at,
        device: ctx.device.into(),
        emulator: adapter.id().into(),
        core: adapter.core(rom.system).map(str::to_string),
        files: files.to_vec(),
        hash: set_hash(files),
    };
    ctx.drive
        .create_file(MANIFEST, &folder.id, &serde_json::to_vec_pretty(&manifest)?)?;
    ctx.db().add_save_backup(
        &rom.id,
        taken_at,
        &format!("saves/{}/{}/{}", rom.system.id(), rom.id, name),
    )?;
    Ok(SaveVersion::from_manifest(&folder.id, &manifest))
}

/// Keeps the newest `keep` backups and moves older ones to Drive's trash.
fn apply_retention(ctx: &SaveContext, game: &str) -> Result<()> {
    for old in version_folders(ctx, game)?.iter().skip(ctx.keep()) {
        ctx.drive.trash(&old.id)?;
    }
    Ok(())
}

/// Backs up the game's save unless the newest backup already has it. Clears the owed backup.
pub fn backup(ctx: &SaveContext, rom: &Rom) -> Result<BackupOutcome> {
    let result = backup_inner(ctx, rom);
    let error = result.as_ref().err().map(|e| e.to_string());
    ctx.update_sync(rom, |s| match &result {
        Ok(outcome) => {
            s.pending = false;
            s.last_error = None;
            if let BackupOutcome::Uploaded { version } = outcome {
                s.last_backup_at = Some(version.taken_at);
                s.last_backup_device = Some(version.device.clone());
            }
        }
        Err(_) => s.last_error = error,
    })?;
    result
}

fn backup_inner(ctx: &SaveContext, rom: &Rom) -> Result<BackupOutcome> {
    let dir = ctx.adapter(rom)?.save_dir(rom, ctx.paths);
    let files = local_files(&dir)?;
    if files.is_empty() {
        return Ok(BackupOutcome::NoSave);
    }
    let hash = set_hash(&files);
    let game = game_folder(ctx, &ctx.layout.saves, rom, true)?
        .ok_or_else(|| Error::External("Drive didn't create the save folder.".into()))?;
    let newest = newest_version(ctx, &game)?;
    let outcome = if newest.as_ref().is_some_and(|v| v.hash == hash) {
        BackupOutcome::Unchanged
    } else {
        let version = upload_version(ctx, rom, &game, &dir, &files)?;
        apply_retention(ctx, &game)?;
        BackupOutcome::Uploaded { version }
    };
    ctx.update_sync(rom, |s| s.last_synced_hash = Some(hash))?;
    Ok(outcome)
}

/// Replaces the save on the Deck with a backup. The current save is backed up first, unless a
/// kept backup already has it.
pub fn restore(ctx: &SaveContext, rom: &Rom, version_id: &str) -> Result<RestoreOutcome> {
    let adapter = ctx.adapter(rom)?;
    let manifest = read_manifest(ctx, version_id)?.ok_or_else(|| {
        Error::Invalid(
            "That save backup didn't finish uploading, so it can't be restored. Choose another."
                .into(),
        )
    })?;
    let dir = adapter.save_dir(rom, ctx.paths);
    let incoming = sibling(&dir, "restoring");
    let _ = std::fs::remove_dir_all(&incoming);
    std::fs::create_dir_all(&incoming)?;
    let fetched = fetch_version(ctx, version_id, &manifest, &incoming);
    if let Err(e) = fetched {
        let _ = std::fs::remove_dir_all(&incoming);
        return Err(e);
    }

    // The save being replaced goes to Drive first.
    let current = local_files(&dir)?;
    if !current.is_empty() {
        let hash = set_hash(&current);
        let kept = versions(ctx, rom)?;
        if !kept.iter().any(|v| v.hash == hash) {
            let game = game_folder(ctx, &ctx.layout.saves, rom, true)?
                .ok_or_else(|| Error::External("Drive didn't create the save folder.".into()))?;
            if let Err(e) = upload_version(ctx, rom, &game, &dir, &current) {
                let _ = std::fs::remove_dir_all(&incoming);
                return Err(e);
            }
            apply_retention(ctx, &game)?;
        }
    }

    let replaced = sibling(&dir, "replaced");
    let _ = std::fs::remove_dir_all(&replaced);
    if dir.exists() {
        std::fs::rename(&dir, &replaced)?;
    }
    std::fs::rename(&incoming, &dir)?;
    let _ = std::fs::remove_dir_all(&replaced);

    ctx.update_sync(rom, |s| {
        s.last_synced_hash = Some(manifest.hash.clone());
        s.pending = false;
        s.last_error = None;
    })?;
    let core = adapter.core(rom.system).map(str::to_string);
    let warning = (manifest.emulator != adapter.id() || manifest.core != core).then(|| {
        format!(
            "This save was made with {}; {} may not load it.",
            describe(&manifest.emulator, manifest.core.as_deref()),
            describe(adapter.id(), core.as_deref())
        )
    });
    Ok(RestoreOutcome {
        version: SaveVersion::from_manifest(version_id, &manifest),
        warning,
    })
}

fn describe(emulator: &str, core: Option<&str>) -> String {
    match core {
        Some(c) => format!("{emulator} ({c})"),
        None => emulator.to_string(),
    }
}

fn sibling(dir: &Path, suffix: &str) -> PathBuf {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.with_file_name(format!("{name}.{suffix}"))
}

/// Downloads a backup's files into `into`, checking each against the manifest.
fn fetch_version(ctx: &SaveContext, folder: &str, m: &Manifest, into: &Path) -> Result<()> {
    let remote = ctx.drive.list(folder)?;
    for f in &m.files {
        let file = remote
            .iter()
            .find(|r| r.name == f.name && !r.is_folder())
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "That save backup is missing {}, so it can't be restored. Choose another.",
                    f.name
                ))
            })?;
        let bytes = ctx.drive.download(&file.id, 0)?;
        let path = into.join(&f.name);
        std::fs::write(&path, &bytes)?;
        if md5_file(&path)? != f.md5 {
            return Err(Error::Invalid(format!(
                "{} didn't arrive intact from Drive. Try again.",
                f.name
            )));
        }
    }
    Ok(())
}

/// Compares this Deck's save and Drive's newest against the last time they agreed.
pub fn check(ctx: &SaveContext, rom: &Rom) -> Result<SaveCheck> {
    let adapter = ctx.adapter(rom)?;
    let dir = adapter.save_dir(rom, ctx.paths);
    let local = local_files(&dir)?;
    let local_hash = set_hash(&local);
    let newest = match game_folder(ctx, &ctx.layout.saves, rom, false)? {
        Some(game) => newest_version(ctx, &game)?,
        None => None,
    };
    let empty = empty_hash();
    let drive_hash = newest.as_ref().map_or(empty.clone(), |v| v.hash.clone());
    let last = ctx
        .db()
        .save_sync(&rom.id)?
        .last_synced_hash
        .unwrap_or(empty);

    if local_hash == drive_hash {
        if last != local_hash {
            ctx.update_sync(rom, |s| s.last_synced_hash = Some(local_hash))?;
        }
        return Ok(SaveCheck::InSync);
    }
    let deck_changed = local_hash != last;
    let drive_changed = drive_hash != last;
    Ok(match newest {
        // Drive has no backup (perhaps removed there): this Deck's save is backed up again.
        None => SaveCheck::DeckNewer,
        Some(version) if !deck_changed && drive_changed => SaveCheck::DriveNewer { version },
        Some(version) if deck_changed && drive_changed => SaveCheck::Conflict {
            conflict: SaveConflict {
                rom_id: rom.id.to_string(),
                name: rom.display_name.clone(),
                deck: SaveSide {
                    taken_at: newest_mtime(&dir, &local).unwrap_or_else(|| ctx.clock.now()),
                    device: ctx.device.into(),
                },
                drive: SaveSide {
                    taken_at: version.taken_at,
                    device: version.device,
                },
            },
        },
        Some(_) => SaveCheck::DeckNewer,
    })
}

fn newest_mtime(dir: &Path, files: &[SaveFile]) -> Option<i64> {
    files
        .iter()
        .filter_map(|f| std::fs::metadata(dir.join(&f.name)).ok()?.modified().ok())
        .max()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// Acts on a conflict answer. `Cancel` is the caller's to handle.
pub fn resolve_conflict(ctx: &SaveContext, rom: &Rom, choice: SaveChoice) -> Result<()> {
    match choice {
        SaveChoice::KeepDeck => backup(ctx, rom).map(|_| ()),
        SaveChoice::UseDrive => {
            let game = game_folder(ctx, &ctx.layout.saves, rom, false)?
                .ok_or_else(|| Error::NotFound("Drive has no save for this game.".into()))?;
            let newest = newest_version(ctx, &game)?
                .ok_or_else(|| Error::NotFound("Drive has no save for this game.".into()))?;
            restore(ctx, rom, &newest.id).map(|_| ())
        }
        SaveChoice::Cancel => Err(Error::Cancelled),
    }
}

/// Records that a backup is owed, before the game starts: if Steam ends Romshelf with the game,
/// the next Romshelf run backs it up.
pub fn mark_pending(db: &Database, rom_id: &RomId) -> Result<()> {
    let mut s = db.save_sync(rom_id)?;
    s.pending = true;
    db.put_save_sync(rom_id, &s)
}

/// Backs up every game whose backup is owed, with its save states. Does nothing while a game
/// is running, since its emulator may still be writing. Returns what happened to each.
pub fn flush_pending(ctx: &SaveContext) -> Vec<(RomId, Result<BackupOutcome>)> {
    if crate::launch::game_running(ctx.paths) {
        return Vec::new();
    }
    let pending = match ctx.db().pending_saves() {
        Ok(p) => p,
        Err(e) => return vec![(RomId(String::new()), Err(e))],
    };
    let mut out = Vec::new();
    for id in pending {
        let rom = match ctx.db().rom(&id) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(e) => {
                out.push((id, Err(e)));
                continue;
            }
        };
        let result = backup(ctx, &rom);
        if result.is_ok() {
            let _ = mirror_states(ctx, &rom);
        }
        out.push((id, result));
    }
    out
}

// Save states

fn state_files(dir: &Path) -> Result<Vec<SaveFile>> {
    local_files(dir)
}

/// Save state slots on the Deck, not counting their screenshots.
pub fn state_slots(ctx_paths: &Paths, adapter: &dyn EmulatorAdapter, rom: &Rom) -> Vec<StateSlot> {
    let Some(dir) = adapter.state_dir(rom, ctx_paths) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<StateSlot> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".png") {
                return None;
            }
            let meta = e.metadata().ok()?;
            let modified = meta
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs() as i64;
            Some(StateSlot {
                name,
                size: meta.len(),
                modified,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Uploads save states that are new or changed since the last mirror. Returns how many.
pub fn mirror_states(ctx: &SaveContext, rom: &Rom) -> Result<usize> {
    let Some(dir) = ctx.adapter(rom)?.state_dir(rom, ctx.paths) else {
        return Ok(0);
    };
    let local = state_files(&dir)?;
    if local.is_empty() {
        return Ok(0);
    }
    let game = game_folder(ctx, &ctx.layout.states, rom, true)?
        .ok_or_else(|| Error::External("Drive didn't create the states folder.".into()))?;
    let remote = ctx.drive.list(&game)?;
    let mut sent = 0;
    for f in &local {
        let there = remote.iter().find(|r| r.name == f.name && !r.is_folder());
        if there.is_some_and(|r| r.md5.as_deref() == Some(f.md5.as_str())) {
            continue;
        }
        let bytes = std::fs::read(dir.join(&f.name))?;
        match there {
            Some(r) => ctx.drive.update_content(&r.id, &bytes)?,
            None => ctx.drive.create_file(&f.name, &game, &bytes)?,
        };
        sent += 1;
    }
    Ok(sent)
}

/// Downloads save states Drive has and this Deck doesn't (made on another machine).
pub fn fetch_states(ctx: &SaveContext, rom: &Rom) -> Result<usize> {
    let Some(dir) = ctx.adapter(rom)?.state_dir(rom, ctx.paths) else {
        return Ok(0);
    };
    let Some(game) = game_folder(ctx, &ctx.layout.states, rom, false)? else {
        return Ok(0);
    };
    let mut got = 0;
    for r in ctx.drive.list(&game)? {
        let path = dir.join(&r.name);
        if r.is_folder() || path.exists() {
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        let bytes = ctx.drive.download(&r.id, 0)?;
        std::fs::write(&path, bytes)?;
        got += 1;
    }
    Ok(got)
}

/// Deletes a save state slot (and its screenshot) from the Deck. Its Drive copy goes to Drive's
/// trash, where it can be recovered; the slot is uploaded first if Drive didn't have it.
pub fn delete_state(ctx: &SaveContext, rom: &Rom, slot: &str) -> Result<()> {
    let dir = ctx
        .adapter(rom)?
        .state_dir(rom, ctx.paths)
        .ok_or_else(|| Error::Invalid("This game's save states aren't kept by Romshelf.".into()))?;
    if slot.contains(['/', '\\']) || slot.starts_with('.') {
        return Err(Error::Invalid(format!("{slot} isn't a save state slot.")));
    }
    let names = [slot.to_string(), format!("{slot}.png")];
    mirror_states(ctx, rom)?;
    if let Some(game) = game_folder(ctx, &ctx.layout.states, rom, false)? {
        for r in ctx.drive.list(&game)? {
            if names.contains(&r.name) {
                ctx.drive.trash(&r.id)?;
            }
        }
    }
    for name in &names {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_are_utc_and_sortable() {
        assert_eq!(utc_stamp(0), "19700101T000000Z");
        assert_eq!(utc_stamp(1_792_440_724), "20261019T201204Z");
        assert_eq!(utc_stamp(951_782_400), "20000229T000000Z");
        assert!(utc_stamp(1_800_000_001) > utc_stamp(1_800_000_000));
    }

    #[test]
    fn set_hash_ignores_order_but_not_names() {
        let a = SaveFile {
            name: "a.srm".into(),
            size: 1,
            md5: "x".into(),
        };
        let b = SaveFile {
            name: "b.srm".into(),
            size: 1,
            md5: "y".into(),
        };
        assert_eq!(
            set_hash(&[a.clone(), b.clone()]),
            set_hash(&[b.clone(), a.clone()])
        );
        let renamed = SaveFile {
            name: "c.srm".into(),
            ..a.clone()
        };
        assert_ne!(set_hash(&[a, b.clone()]), set_hash(&[renamed, b]));
        assert_eq!(set_hash(&[]), empty_hash());
    }
}
