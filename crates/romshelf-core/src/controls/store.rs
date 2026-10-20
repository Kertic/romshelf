//! Layouts in the database (`control_profiles`), mirrored to Drive's `.romshelf/controls/` so
//! they follow the user to another Deck.
//!
//! An emulator's default is stored once it differs from the built-in one; a game is stored only
//! while it has differences.

use super::defaults::built_in;
use super::merge::{merge, promote as promote_layouts};
use super::model::{validate, Layout, Scope, INPUTS};
use crate::db::Database;
use crate::drive::{DriveClient, DriveFile};
use crate::model::{Binding, ControlProfile, LocationState, Rom, RomId};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DRIVE_FOLDER: &str = "controls";
pub const MIRROR_SCHEMA: u32 = 1;

fn changed_key(scope: &Scope) -> String {
    format!("controls.changed.{}", scope.key())
}

/// When this scope last changed here (seconds since the epoch; 0 if never).
fn changed_at(db: &Database, scope: &Scope) -> Result<i64> {
    Ok(db
        .setting(&changed_key(scope))?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0))
}

fn touch(db: &Database, scope: &Scope, now: i64) -> Result<()> {
    db.set_setting(&changed_key(scope), &now.to_string())
}

/// The emulator's default for the scope: the user's, or Romshelf's built-in one.
pub fn default_layout(db: &Database, scope: &Scope) -> Result<Layout> {
    let mut layout = built_in(scope);
    if let Some(p) = db.control_profile(&scope.key(), None)? {
        layout.extend(p.bindings);
    }
    Ok(layout)
}

/// A game's differences from the default (empty when it has none).
pub fn game_layout(db: &Database, scope: &Scope, rom: &RomId) -> Result<Layout> {
    Ok(db
        .control_profile(&scope.key(), Some(rom))?
        .map(|p| p.bindings)
        .unwrap_or_default())
}

/// What the game plays with.
pub fn effective(db: &Database, scope: &Scope, rom: &RomId) -> Result<Layout> {
    Ok(merge(
        &default_layout(db, scope)?,
        &game_layout(db, scope, rom)?,
    ))
}

/// Replaces the emulator's default. Inputs left out are cleared.
pub fn set_default(db: &Database, scope: &Scope, layout: &Layout, now: i64) -> Result<()> {
    validate(scope, layout)?;
    let mut full: Layout = INPUTS
        .iter()
        .map(|i| (i.id.to_string(), Binding::Cleared))
        .collect();
    full.extend(layout.clone());
    db.upsert_control_profile(&ControlProfile {
        emulator_id: scope.key(),
        rom_id: None,
        bindings: full,
    })?;
    touch(db, scope, now)
}

/// Replaces a game's differences; none left removes its profile.
pub fn set_game(db: &Database, scope: &Scope, rom: &RomId, diff: &Layout, now: i64) -> Result<()> {
    validate(scope, diff)?;
    if diff.is_empty() {
        db.delete_control_profile(&scope.key(), Some(rom))?;
    } else {
        db.upsert_control_profile(&ControlProfile {
            emulator_id: scope.key(),
            rom_id: Some(rom.clone()),
            bindings: diff.clone(),
        })?;
    }
    touch(db, scope, now)
}

/// "Reset to {emulator} default".
pub fn reset_game(db: &Database, scope: &Scope, rom: &RomId, now: i64) -> Result<()> {
    set_game(db, scope, rom, &Layout::new(), now)
}

/// The other games of the scope's system whose layout "Make this the default" would change.
pub fn promote_impact(db: &Database, scope: &Scope, rom: &RomId) -> Result<Vec<Rom>> {
    promote_impact_of(db, scope, rom, &game_layout(db, scope, rom)?)
}

/// As `promote_impact`, for differences not saved yet (the editor's).
pub fn promote_impact_of(
    db: &Database,
    scope: &Scope,
    rom: &RomId,
    diff: &Layout,
) -> Result<Vec<Rom>> {
    let old = default_layout(db, scope)?;
    let (new, _) = promote_layouts(&old, diff);
    let mut out = Vec::new();
    for other in db.roms()? {
        if other.system != scope.system || other.id == *rom || other.location == LocationState::Gone
        {
            continue;
        }
        let diff = game_layout(db, scope, &other.id)?;
        if merge(&old, &diff) != merge(&new, &diff) {
            out.push(other);
        }
    }
    Ok(out)
}

/// "Make this the {emulator} default": the game's layout becomes the default and it keeps no
/// differences. Other games keep their own.
pub fn promote(db: &Database, scope: &Scope, rom: &RomId, now: i64) -> Result<()> {
    let (new, _) = promote_layouts(&default_layout(db, scope)?, &game_layout(db, scope, rom)?);
    set_default(db, scope, &new, now)?;
    reset_game(db, scope, rom, now)
}

/// One scope's layouts as kept in Drive: `.romshelf/controls/<emulator>-<system>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mirror {
    pub schema_version: u32,
    pub scope: Scope,
    pub changed_at: i64,
    /// `None` while the built-in default is used.
    pub default: Option<Layout>,
    pub games: BTreeMap<String, Layout>,
}

fn file_name(scope: &Scope) -> String {
    format!("{}-{}.json", scope.emulator_id, scope.system.id())
}

fn controls_folder(drive: &dyn DriveClient, meta: &str, create: bool) -> Result<Option<String>> {
    let mut found: Vec<DriveFile> = drive
        .list(meta)?
        .into_iter()
        .filter(|f| f.is_folder() && f.name == DRIVE_FOLDER)
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    match found.into_iter().next() {
        Some(f) => Ok(Some(f.id)),
        None if create => Ok(Some(drive.create_folder(DRIVE_FOLDER, meta)?.id)),
        None => Ok(None),
    }
}

/// Writes a scope's layouts to Drive.
pub fn push(drive: &dyn DriveClient, meta: &str, db: &Database, scope: &Scope) -> Result<()> {
    let mirror = Mirror {
        schema_version: MIRROR_SCHEMA,
        scope: scope.clone(),
        changed_at: changed_at(db, scope)?,
        default: db.control_profile(&scope.key(), None)?.map(|p| p.bindings),
        games: db
            .game_control_profiles(&scope.key())?
            .into_iter()
            .filter_map(|p| Some((p.rom_id?.to_string(), p.bindings)))
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&mirror)?;
    let folder = controls_folder(drive, meta, true)?
        .ok_or_else(|| Error::External("Drive didn't create the controls folder.".into()))?;
    let name = file_name(scope);
    match drive.list(&folder)?.into_iter().find(|f| f.name == name) {
        Some(f) => drive.update_content(&f.id, &bytes)?,
        None => drive.create_file(&name, &folder, &bytes)?,
    };
    Ok(())
}

/// Takes layouts from Drive that changed later than this Deck's. Returns the scopes updated.
pub fn pull(drive: &dyn DriveClient, meta: &str, db: &Database) -> Result<Vec<Scope>> {
    let Some(folder) = controls_folder(drive, meta, false)? else {
        return Ok(Vec::new());
    };
    let mut updated = Vec::new();
    for f in drive.list(&folder)? {
        if f.is_folder() || !f.name.ends_with(".json") {
            continue;
        }
        let mirror: Mirror = serde_json::from_slice(&drive.download(&f.id, 0)?)?;
        if mirror.schema_version > MIRROR_SCHEMA {
            return Err(Error::NeedsUpdate(format!("Drive's {}", f.name)));
        }
        let scope = mirror.scope.clone();
        if mirror.changed_at <= changed_at(db, &scope)? {
            continue;
        }
        let key = scope.key();
        db.delete_control_profile(&key, None)?;
        for p in db.game_control_profiles(&key)? {
            db.delete_control_profile(&key, p.rom_id.as_ref())?;
        }
        if let Some(default) = mirror.default {
            db.upsert_control_profile(&ControlProfile {
                emulator_id: key.clone(),
                rom_id: None,
                bindings: default,
            })?;
        }
        for (rom, bindings) in mirror.games {
            db.upsert_control_profile(&ControlProfile {
                emulator_id: key.clone(),
                rom_id: Some(RomId(rom)),
                bindings,
            })?;
        }
        touch(db, &scope, mirror.changed_at)?;
        updated.push(scope);
    }
    Ok(updated)
}
