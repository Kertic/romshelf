//! The library index: builds the ROM table from the Drive folder and keeps it current from
//! Drive's change feed. Metadata only; nothing is downloaded here.
//!
//! `.romshelf/index.json` maps Drive file ids to rom-ids and display-name overrides, so a
//! second device adopts the same ids (a file moved between system folders keeps its first id).

use crate::clock::Clock;
use crate::db::Database;
use crate::drive::scaffold::DriveLayout;
use crate::drive::{DriveClient, DriveFile};
use crate::model::{LocationState, Rom, RomId, System};
use crate::paths::Paths;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const INDEX_FILE: &str = "index.json";
pub const INDEX_SCHEMA: u32 = 1;
/// Database settings keys.
pub const TOKEN_KEY: &str = "sync.change_token";
pub const FOLDER_KEY: &str = "sync.emulation_folder";

pub struct SyncContext<'a> {
    pub drive: &'a dyn DriveClient,
    pub db: &'a Database,
    pub layout: &'a DriveLayout,
    pub paths: &'a Paths,
    pub clock: &'a dyn Clock,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    /// Every system folder was listed (first sync, or the change token was rejected).
    pub full_scan: bool,
    pub added: usize,
    pub updated: usize,
    pub gone: usize,
    pub restored: usize,
    /// Files in the library now, recognized or not.
    pub total: usize,
    /// Of `total`, files whose extension the system doesn't recognize.
    pub unrecognized: usize,
    pub index_written: bool,
}

impl SyncReport {
    pub fn changed(&self) -> bool {
        self.added + self.updated + self.gone + self.restored > 0
    }

    /// One line for `romshelf sync` and the interface.
    pub fn summary(&self) -> String {
        let files = if self.total == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", self.total)
        };
        if !self.changed() {
            return format!("Synced {files}. Nothing changed.");
        }
        let mut parts = Vec::new();
        for (n, label) in [
            (self.added, "new"),
            (self.updated, "changed"),
            (self.gone, "gone from Drive"),
            (self.restored, "back in Drive"),
            (self.unrecognized, "not recognized"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {label}"));
            }
        }
        format!("Synced {files}: {}.", parts.join(", "))
    }
}

/// Contents of `.romshelf/index.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexFile {
    pub schema_version: u32,
    /// Drive file id → entry.
    #[serde(default)]
    pub roms: BTreeMap<String, IndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub rom_id: RomId,
    /// A display name the user chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Display name from a file name: the stem with bracketed tags removed, underscores and
/// hyphens as spaces, title-cased only when the stem was all lower case.
pub fn display_name(file_name: &str) -> String {
    let stem = match file_name.rsplit_once('.') {
        Some((s, ext)) if !s.trim().is_empty() && !ext.contains(' ') => s,
        _ => file_name,
    };
    let mut cleaned = String::new();
    let mut depth = 0u32;
    for c in stem.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '_' | '-' if depth == 0 => cleaned.push(' '),
            _ if depth == 0 => cleaned.push(c),
            _ => {}
        }
    }
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.is_empty() {
        // Nothing but tags: keep them rather than show an empty name.
        return stem.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    let all_lower = !stem.chars().any(|c| c.is_uppercase());
    if !all_lower {
        return words.join(" ");
    }
    words
        .iter()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// A file Romshelf may index: not a folder, Google Doc or Drive shortcut.
fn is_candidate(f: &DriveFile) -> bool {
    !f.is_folder() && !f.mime_type.starts_with("application/vnd.google-apps.")
}

/// The system whose `roms/<system>/` folder directly holds `f`.
fn system_of(layout: &DriveLayout, f: &DriveFile) -> Option<System> {
    f.parents.iter().find_map(|p| {
        layout
            .rom_folders
            .iter()
            .find(|(_, id)| *id == p)
            .and_then(|(system, _)| System::from_id(system))
    })
}

struct Index {
    file_id: Option<String>,
    content: IndexFile,
}

fn read_index(ctx: &SyncContext) -> Result<Index> {
    let mut files: Vec<DriveFile> = ctx
        .drive
        .list(&ctx.layout.meta)?
        .into_iter()
        .filter(|f| f.name == INDEX_FILE && !f.is_folder())
        .collect();
    files.sort_by(|a, b| a.id.cmp(&b.id));
    let Some(file) = files.into_iter().next() else {
        return Ok(Index {
            file_id: None,
            content: IndexFile {
                schema_version: INDEX_SCHEMA,
                roms: BTreeMap::new(),
            },
        });
    };
    let bytes = ctx.drive.download(&file.id, 0)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if version > u64::from(INDEX_SCHEMA) {
        return Err(Error::NeedsUpdate(
            "The library index in your Drive's Emulation folder".into(),
        ));
    }
    // A damaged index is rebuilt from this device's database.
    let mut content: IndexFile = serde_json::from_value(value).unwrap_or_default();
    content.schema_version = INDEX_SCHEMA;
    Ok(Index {
        file_id: Some(file.id),
        content,
    })
}

fn write_index(ctx: &SyncContext, index: &mut Index) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(&index.content)?;
    match &index.file_id {
        Some(id) => {
            ctx.drive.update_content(id, &bytes)?;
        }
        None => {
            index.file_id = Some(
                ctx.drive
                    .create_file(INDEX_FILE, &ctx.layout.meta, &bytes)?
                    .id,
            );
        }
    }
    Ok(())
}

/// Brings the database up to date with Drive, by the change feed when possible.
pub fn run(ctx: &SyncContext) -> Result<SyncReport> {
    let mut index = read_index(ctx)?;
    let original = index.content.clone();
    let mut report = SyncReport::default();

    let stored_token = ctx.db.setting(TOKEN_KEY)?.filter(|t| !t.is_empty());
    let same_folder = ctx.db.setting(FOLDER_KEY)?.as_deref() == Some(ctx.layout.emulation.as_str());
    let mut token = None;
    if let (Some(t), true) = (&stored_token, same_folder) {
        match ctx.drive.changes(t) {
            Ok(page) => {
                apply_changes(ctx, &mut index, &page.changes, &mut report)?;
                token = Some(page.new_token);
            }
            // Expired or unknown token: start again from a full scan.
            Err(Error::Invalid(_) | Error::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    let mut token = match token {
        Some(t) => t,
        None => {
            report.full_scan = true;
            full_scan(ctx, &mut index, &mut report)?
        }
    };

    // Record this device's ids and names for other devices.
    for rom in ctx.db.roms()? {
        let entry = IndexEntry {
            name: ctx.db.name_override(&rom.id)?,
            rom_id: rom.id,
        };
        index.content.roms.insert(rom.drive_file_id, entry);
    }
    if index.content != original {
        write_index(ctx, &mut index)?;
        report.index_written = true;
        // Step past our own write so the next sync starts clean.
        let page = ctx.drive.changes(&token)?;
        apply_changes(ctx, &mut index, &page.changes, &mut report)?;
        token = page.new_token;
    }

    if stored_token.as_deref() != Some(token.as_str()) {
        ctx.db.set_setting(TOKEN_KEY, &token)?;
    }
    if !same_folder {
        ctx.db.set_setting(FOLDER_KEY, &ctx.layout.emulation)?;
    }
    let active: Vec<Rom> = ctx
        .db
        .roms()?
        .into_iter()
        .filter(|r| r.location != LocationState::Gone)
        .collect();
    report.total = active.len();
    report.unrecognized = active.iter().filter(|r| !r.recognized).count();
    Ok(report)
}

/// Lists every system folder. Returns the change token taken before listing, so changes made
/// during the scan are picked up next time.
fn full_scan(ctx: &SyncContext, index: &mut Index, report: &mut SyncReport) -> Result<String> {
    let token = ctx.drive.start_page_token()?;
    let mut seen = HashSet::new();
    for system in System::ALL {
        let Some(folder) = ctx.layout.rom_folders.get(system.id()) else {
            continue;
        };
        for f in ctx.drive.list(folder)? {
            if is_candidate(&f) && !f.trashed {
                seen.insert(f.id.clone());
                upsert_file(ctx, index, &f, system, report)?;
            }
        }
    }
    for rom in ctx.db.roms()? {
        if rom.location != LocationState::Gone && !seen.contains(&rom.drive_file_id) {
            mark_gone(ctx, rom, report)?;
        }
    }
    Ok(token)
}

fn apply_changes(
    ctx: &SyncContext,
    index: &mut Index,
    changes: &[crate::drive::Change],
    report: &mut SyncReport,
) -> Result<()> {
    for change in changes {
        if index.file_id.as_deref() == Some(change.file_id.as_str()) {
            continue;
        }
        let in_library = change
            .file
            .as_ref()
            .filter(|f| !f.trashed && is_candidate(f))
            .and_then(|f| system_of(ctx.layout, f).map(|s| (f, s)));
        match in_library {
            Some((f, system)) => upsert_file(ctx, index, f, system, report)?,
            // Trashed, deleted, or moved out of the system folders.
            None => {
                if let Some(rom) = ctx.db.rom_by_drive_id(&change.file_id)? {
                    if rom.location != LocationState::Gone {
                        mark_gone(ctx, rom, report)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn upsert_file(
    ctx: &SyncContext,
    index: &Index,
    f: &DriveFile,
    system: System,
    report: &mut SyncReport,
) -> Result<()> {
    let existing = ctx.db.rom_by_drive_id(&f.id)?;
    let indexed = index.content.roms.get(&f.id);
    let id = match (&existing, indexed) {
        (Some(r), _) => r.id.clone(),
        (None, Some(e)) => e.rom_id.clone(),
        (None, None) => new_id(ctx, index, system, &f.id)?,
    };
    // The index is shared between devices, so its name wins when it has an entry.
    let name_override = match indexed {
        Some(e) => e.name.clone(),
        None => ctx.db.name_override(&id)?,
    };
    if ctx.db.name_override(&id)? != name_override {
        ctx.db.set_name_override(&id, name_override.as_deref())?;
    }
    let location = match &existing {
        Some(r) if r.location != LocationState::Gone => r.location.clone(),
        _ => {
            let local = ctx.paths.rom_file(system, &f.name).ok();
            if local.is_some_and(|p| p.is_file()) {
                LocationState::OnDeck
            } else {
                LocationState::DriveOnly
            }
        }
    };
    let rom = Rom {
        id,
        drive_file_id: f.id.clone(),
        system,
        file_name: f.name.clone(),
        display_name: name_override.unwrap_or_else(|| display_name(&f.name)),
        size: f.size,
        checksum: f.md5.clone(),
        location,
        recognized: system.recognizes(&f.name),
    };
    match existing {
        None => {
            ctx.db.upsert_rom(&rom)?;
            report.added += 1;
        }
        Some(old) if old == rom => {}
        Some(old) => {
            ctx.db.upsert_rom(&rom)?;
            if old.location == LocationState::Gone {
                report.restored += 1;
            } else {
                report.updated += 1;
            }
        }
    }
    Ok(())
}

/// The derived id, or a salted one when another file already holds it.
fn new_id(ctx: &SyncContext, index: &Index, system: System, file_id: &str) -> Result<RomId> {
    for salt in 0.. {
        let id = RomId::derive_salted(system, file_id, salt);
        let in_db = ctx.db.rom(&id)?.is_some_and(|r| r.drive_file_id != file_id);
        let in_index = index
            .content
            .roms
            .iter()
            .any(|(fid, e)| e.rom_id == id && fid != file_id);
        if !in_db && !in_index {
            return Ok(id);
        }
    }
    unreachable!("an unbounded range always yields")
}

/// Marked, not deleted: saves, controls and the shortcut record stay.
fn mark_gone(ctx: &SyncContext, mut rom: Rom, report: &mut SyncReport) -> Result<()> {
    rom.location = LocationState::Gone;
    ctx.db.upsert_rom(&rom)?;
    report.gone += 1;
    Ok(())
}

/// Sets or clears (`None`) the name the user chose, here and in the shared index.
pub fn set_display_name(ctx: &SyncContext, id: &RomId, name: Option<&str>) -> Result<()> {
    let mut rom = ctx
        .db
        .rom(id)?
        .ok_or_else(|| Error::NotFound(format!("ROM {id}")))?;
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    ctx.db.set_name_override(id, name)?;
    rom.display_name = name
        .map(str::to_string)
        .unwrap_or_else(|| display_name(&rom.file_name));
    ctx.db.upsert_rom(&rom)?;
    let mut index = read_index(ctx)?;
    index.content.roms.insert(
        rom.drive_file_id.clone(),
        IndexEntry {
            rom_id: rom.id.clone(),
            name: name.map(str::to_string),
        },
    );
    write_index(ctx, &mut index)
}
