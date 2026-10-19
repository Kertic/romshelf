//! Creates Romshelf's folder tree in Drive and keeps `.romshelf/settings.json`.
//!
//! Folder names are not unique in Drive, so every lookup is by parent id plus name, and the
//! resulting ids are stored.

use super::{DriveClient, DriveFile};
use crate::model::System;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const EMULATION: &str = "Emulation";
pub const META: &str = ".romshelf";
pub const SETTINGS_FILE: &str = "settings.json";
pub const SETTINGS_SCHEMA: u32 = 1;

/// Ids of every folder Romshelf manages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveLayout {
    pub emulation: String,
    pub roms: String,
    pub saves: String,
    pub states: String,
    pub bios: String,
    /// `.romshelf/`
    pub meta: String,
    /// System id → `roms/<system>/` folder id.
    pub rom_folders: BTreeMap<String, String>,
    /// System id → `bios/<system>/` folder id.
    pub bios_folders: BTreeMap<String, String>,
}

/// Contents of `.romshelf/settings.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveSettings {
    pub schema_version: u32,
    pub folders: DriveLayout,
}

/// The tree as shown to the user after setup, one line per folder.
pub fn tree_lines() -> Vec<String> {
    let systems = System::ALL
        .iter()
        .map(|s| s.id())
        .collect::<Vec<_>>()
        .join(", ");
    vec![
        format!("{EMULATION}/"),
        format!("  roms/      {systems}"),
        "  saves/".into(),
        "  states/".into(),
        format!("  bios/      {systems}"),
        format!("  {META}/"),
    ]
}

/// Finds or creates each of `names` as a folder under `parent`, listing the parent once.
/// `progress` receives `path_prefix` plus each name.
fn ensure_folders(
    drive: &dyn DriveClient,
    parent: &str,
    names: &[&str],
    path_prefix: &str,
    progress: &mut dyn FnMut(&str),
) -> Result<Vec<String>> {
    let mut existing: Vec<DriveFile> = drive
        .list(parent)?
        .into_iter()
        .filter(|f| f.is_folder() && !f.trashed)
        .collect();
    // Duplicates are possible in Drive; pick one the same way every time.
    existing.sort_by(|a, b| a.id.cmp(&b.id));
    let mut ids = Vec::with_capacity(names.len());
    for name in names {
        let id = match existing.iter().find(|f| f.name == *name) {
            Some(f) => f.id.clone(),
            None => drive.create_folder(name, parent)?.id,
        };
        progress(&format!("{path_prefix}{name}"));
        ids.push(id);
    }
    Ok(ids)
}

/// Creates `Emulation/` and its sub-folders under `parent`. Running it again creates nothing new;
/// a partial tree is completed.
pub fn scaffold(
    drive: &dyn DriveClient,
    parent: &str,
    progress: &mut dyn FnMut(&str),
) -> Result<DriveLayout> {
    let emulation = ensure_folders(drive, parent, &[EMULATION], "", progress)?.remove(0);
    let prefix = format!("{EMULATION}/");
    let top = ensure_folders(
        drive,
        &emulation,
        &["roms", "saves", "states", "bios", META],
        &prefix,
        progress,
    )?;
    let system_ids: Vec<&str> = System::ALL.iter().map(|s| s.id()).collect();
    let roms = ensure_folders(
        drive,
        &top[0],
        &system_ids,
        &format!("{prefix}roms/"),
        progress,
    )?;
    let bios = ensure_folders(
        drive,
        &top[3],
        &system_ids,
        &format!("{prefix}bios/"),
        progress,
    )?;
    let zip = |ids: Vec<String>| {
        system_ids
            .iter()
            .map(|s| s.to_string())
            .zip(ids)
            .collect::<BTreeMap<_, _>>()
    };
    Ok(DriveLayout {
        emulation,
        roms: top[0].clone(),
        saves: top[1].clone(),
        states: top[2].clone(),
        bios: top[3].clone(),
        meta: top[4].clone(),
        rom_folders: zip(roms),
        bios_folders: zip(bios),
    })
}

/// Parses `settings.json`, refusing a schema newer than this build.
pub fn parse_settings(bytes: &[u8]) -> Result<DriveSettings> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| Error::Invalid("Drive settings have no schema_version".into()))?;
    if version > u64::from(SETTINGS_SCHEMA) {
        return Err(Error::NeedsUpdate(
            "The settings file in your Drive's Emulation folder".into(),
        ));
    }
    Ok(serde_json::from_value(value)?)
}

fn settings_file(drive: &dyn DriveClient, meta: &str) -> Result<Option<DriveFile>> {
    let mut files: Vec<DriveFile> = drive
        .list(meta)?
        .into_iter()
        .filter(|f| f.name == SETTINGS_FILE && !f.is_folder())
        .collect();
    files.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(files.into_iter().next())
}

pub fn read_settings(drive: &dyn DriveClient, meta: &str) -> Result<Option<DriveSettings>> {
    match settings_file(drive, meta)? {
        Some(f) => parse_settings(&drive.download(&f.id, 0)?).map(Some),
        None => Ok(None),
    }
}

/// Scaffolds under `parent` and writes `.romshelf/settings.json`. Settings from a newer
/// Romshelf are left untouched and reported.
pub fn setup(
    drive: &dyn DriveClient,
    parent: &str,
    progress: &mut dyn FnMut(&str),
) -> Result<DriveLayout> {
    let layout = scaffold(drive, parent, progress)?;
    let existing = settings_file(drive, &layout.meta)?;
    if let Some(f) = &existing {
        // Fails with NeedsUpdate for a newer schema; a damaged file is simply replaced.
        if let Err(e @ Error::NeedsUpdate(_)) = parse_settings(&drive.download(&f.id, 0)?) {
            return Err(e);
        }
    }
    let bytes = serde_json::to_vec_pretty(&DriveSettings {
        schema_version: SETTINGS_SCHEMA,
        folders: layout.clone(),
    })?;
    match existing {
        Some(f) => drive.update_content(&f.id, &bytes)?,
        None => drive.create_file(SETTINGS_FILE, &layout.meta, &bytes)?,
    };
    Ok(layout)
}
