//! Which games are in Steam, and bringing Steam's files in line with that: shortcuts, their
//! artwork and their controller layout. Changes are batched; Steam shows them after it
//! restarts.
//!
//! A row in `shortcuts` means "show this game in Steam" (with the name and any artwork the
//! user chose). Games the user took out are remembered in `steam_hidden`, so adding every new
//! ROM automatically leaves them out.

use super::artwork::{self, SLOTS};
use super::input;
use super::shortcuts::{self, Desired};
use super::SteamFiles;
use crate::db::Database;
use crate::model::{ArtSlot, LocationState, Rom, RomId, Shortcut};
use crate::paths::Paths;
use crate::{Error, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// "Add every new ROM to Steam automatically".
pub const AUTO_ADD_SETTING: &str = "steam.auto_add";
/// The Steam account to use, when there's more than one and the most recent isn't wanted.
pub const USER_SETTING: &str = "steam.user";
/// Changes Steam hasn't picked up yet (it reads its files when it starts).
pub const RESTART_SETTING: &str = "steam.restart_pending";

pub struct SteamContext<'a> {
    pub db: &'a Database,
    pub files: &'a dyn SteamFiles,
    pub paths: &'a Paths,
    pub now: i64,
}

impl SteamContext<'_> {
    fn user(&self) -> Result<String> {
        let chosen = self.db.setting(USER_SETTING)?;
        super::active_user(self.files, chosen.as_deref())?.ok_or_else(|| {
            Error::NotFound(
                "Romshelf couldn't find a Steam account on this Deck. Start Steam once, then try again."
                    .into(),
            )
        })
    }
}

/// A new shortcut row for a game: its display name, its app ID, no chosen artwork.
fn row_for(rom: &Rom) -> Shortcut {
    Shortcut {
        rom_id: rom.id.clone(),
        app_id: shortcuts::app_id_for(rom.id.as_str()),
        name: rom.display_name.clone(),
        target: String::new(),
        launch_options: Shortcut::launch_options_for(&rom.id),
        artwork: BTreeMap::new(),
    }
}

/// "Show {name} in Steam library" on or off.
pub fn set_shown(db: &Database, rom: &Rom, shown: bool) -> Result<()> {
    db.set_steam_hidden(&rom.id, !shown)?;
    if shown {
        if db.shortcut(&rom.id)?.is_none() {
            db.upsert_shortcut(&row_for(rom))?;
        }
    } else {
        db.delete_shortcut(&rom.id)?;
    }
    Ok(())
}

/// The name Steam shows. Empty means the game's display name.
pub fn rename(db: &Database, rom: &Rom, name: &str) -> Result<()> {
    let mut row = db
        .shortcut(&rom.id)?
        .ok_or_else(|| Error::Invalid(format!("{} isn't in Steam.", rom.display_name)))?;
    let name = name.trim();
    row.name = if name.is_empty() {
        rom.display_name.clone()
    } else {
        name.to_string()
    };
    db.upsert_shortcut(&row)
}

/// Where a chosen artwork file is kept: `<data>/artwork/<rom-id>/<slot>.<ext>`.
fn chosen_dir(paths: &Paths, rom: &RomId) -> PathBuf {
    paths.data_dir.join("artwork").join(rom.as_str())
}

fn slot_key(slot: ArtSlot) -> &'static str {
    match slot {
        ArtSlot::Capsule => "capsule",
        ArtSlot::WideCapsule => "wide_capsule",
        ArtSlot::Hero => "hero",
        ArtSlot::Logo => "logo",
        ArtSlot::Icon => "icon",
    }
}

/// Uses an image file for a slot. Romshelf keeps its own copy, so moving the original later
/// changes nothing. `None` goes back to the generated artwork.
pub fn set_artwork(
    db: &Database,
    paths: &Paths,
    rom: &Rom,
    slot: ArtSlot,
    file: Option<&Path>,
) -> Result<()> {
    let mut row = db
        .shortcut(&rom.id)?
        .ok_or_else(|| Error::Invalid(format!("{} isn't in Steam.", rom.display_name)))?;
    let dir = chosen_dir(paths, &rom.id);
    if let Some(old) = row.artwork.remove(&slot) {
        let _ = std::fs::remove_file(old);
    }
    if let Some(file) = file {
        let ext = file
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .filter(|e| artwork::EXTENSIONS.contains(&e.as_str()))
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "{} isn't an image Steam can show. Choose a PNG or JPEG file.",
                    file.display()
                ))
            })?;
        std::fs::create_dir_all(&dir)?;
        let copy = dir.join(format!("{}.{ext}", slot_key(slot)));
        std::fs::copy(file, &copy)?;
        row.artwork
            .insert(slot, copy.to_string_lossy().into_owned());
    }
    db.upsert_shortcut(&row)
}

/// After a sync, with "Add every new ROM" on: every playable game not in Steam, and not taken
/// out by the user, gets a shortcut. Returns those added.
pub fn auto_add(db: &Database) -> Result<Vec<RomId>> {
    if db.setting(AUTO_ADD_SETTING)?.as_deref() != Some("1") {
        return Ok(Vec::new());
    }
    let mut added = Vec::new();
    for rom in db.roms()? {
        if rom.location == LocationState::Gone || !rom.recognized {
            continue;
        }
        if db.shortcut(&rom.id)?.is_none() && !db.steam_hidden(&rom.id)? {
            db.upsert_shortcut(&row_for(&rom))?;
            added.push(rom.id);
        }
    }
    Ok(added)
}

/// One slot's image: the user's file, or generated, or none (Logo).
fn slot_image(row: &Shortcut, rom: &Rom, slot: ArtSlot) -> Result<Option<(Vec<u8>, String)>> {
    if let Some(path) = row.artwork.get(&slot) {
        if let Ok(bytes) = std::fs::read(path) {
            let ext = Path::new(path)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| "png".into());
            return Ok(Some((bytes, ext)));
        }
    }
    Ok(artwork::fallback(
        slot,
        &rom.display_name,
        rom.system.label(),
        rom.system.colour(),
    )?
    .map(|b| (b, "png".to_string())))
}

/// What applying did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Applied {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    /// Artwork files written or removed.
    pub artwork: usize,
    /// Steam must restart to show the changes.
    pub restart_needed: bool,
}

impl Applied {
    /// Shortcut changes, as the restart prompt counts them: new artwork alone counts as one.
    pub fn changes(&self) -> usize {
        let shortcuts = self.added + self.updated + self.removed;
        if shortcuts == 0 && self.artwork > 0 {
            1
        } else {
            shortcuts
        }
    }
}

/// Brings Steam's files in line with the shortcuts table. Running it again changes nothing.
pub fn apply(ctx: &SteamContext) -> Result<Applied> {
    let user = ctx.user()?;
    let roms: BTreeMap<String, Rom> = ctx
        .db
        .roms()?
        .into_iter()
        .map(|r| (r.id.to_string(), r))
        .collect();
    let rows: Vec<(Shortcut, Rom)> = ctx
        .db
        .shortcuts()?
        .into_iter()
        .filter_map(|s| {
            let rom = roms.get(s.rom_id.as_str())?.clone();
            (rom.location != LocationState::Gone).then_some((s, rom))
        })
        .collect();

    let exe = ctx.paths.appimage.to_string_lossy().into_owned();
    let start_dir = ctx
        .paths
        .appimage
        .parent()
        .map(|d| format!("{}/", d.to_string_lossy()))
        .unwrap_or_default();

    let mut out = Applied::default();
    let grid = ctx.files.grid_files(&user)?;
    let mut desired = Vec::new();
    for (row, rom) in &rows {
        // Artwork first, so the shortcut's icon names the file that's there.
        let mut icon = String::new();
        for slot in SLOTS {
            let stem = artwork::stem(slot, row.app_id);
            let image = slot_image(row, rom, slot)?;
            let wanted = image.as_ref().map(|(_, ext)| format!("{stem}.{ext}"));
            for name in grid.iter().filter(|n| is_slot_file(n, &stem)) {
                if Some(name) != wanted.as_ref() {
                    ctx.files.remove_grid(&user, name)?;
                    out.artwork += 1;
                }
            }
            if let (Some((bytes, _)), Some(name)) = (image, wanted) {
                if ctx.files.read_grid(&user, &name)?.as_deref() != Some(bytes.as_slice()) {
                    ctx.files.write_grid(&user, &name, &bytes)?;
                    out.artwork += 1;
                }
                if slot == ArtSlot::Icon {
                    icon = ctx
                        .files
                        .grid_path(&user, &name)
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        desired.push(Desired {
            app_id: row.app_id,
            name: row.name.clone(),
            exe: exe.clone(),
            start_dir: start_dir.clone(),
            launch_options: Shortcut::launch_options_for(&rom.id),
            icon,
        });
    }

    // Every ID Romshelf could have used, so its old entries are recognized.
    let mut ours: Vec<u32> = roms.keys().map(|id| shortcuts::app_id_for(id)).collect();
    ours.extend(rows.iter().map(|(s, _)| s.app_id));

    let existing = ctx.files.read_shortcuts(&user)?;
    let before = shortcuts::list(existing.as_deref())?;
    let plan = shortcuts::reconcile(existing.as_deref(), &desired, &ours)?;
    if let Some(bytes) = &plan.bytes {
        ctx.files.write_shortcuts(&user, bytes, ctx.now)?;
    }
    out.added = plan.added.len();
    out.updated = plan.updated.len();
    out.removed = plan.removed.len();

    // Artwork of removed shortcuts.
    for id in &plan.removed {
        for slot in SLOTS {
            let stem = artwork::stem(slot, *id);
            for name in grid.iter().filter(|n| is_slot_file(n, &stem)) {
                ctx.files.remove_grid(&user, name)?;
                out.artwork += 1;
            }
        }
    }

    apply_controller_layouts(ctx, &user, &before, &desired, &ours)?;

    out.restart_needed = plan.bytes.is_some() || out.artwork > 0;
    if out.restart_needed {
        ctx.db.set_setting(RESTART_SETTING, "1")?;
    }
    Ok(out)
}

/// `<stem>.<image extension>`.
fn is_slot_file(name: &str, stem: &str) -> bool {
    name.strip_prefix(stem)
        .and_then(|rest| rest.strip_prefix('.'))
        .is_some_and(|ext| artwork::EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}

/// Romshelf's Steam Input layout (grips as F9 to F12) for each shortcut, under the name Steam
/// files it by; entries for removed or renamed shortcuts are taken out. Skipped when Steam's
/// template isn't there.
fn apply_controller_layouts(
    ctx: &SteamContext,
    user: &str,
    before: &[shortcuts::Found],
    desired: &[Desired],
    ours: &[u32],
) -> Result<()> {
    let Some(template) = ctx.files.controller_template()? else {
        return Ok(());
    };
    let layout = input::layout_from_template(&template);
    let original = ctx.files.read_controller_set(user)?;
    let mut set = original.clone();
    for old in before.iter().filter(|f| ours.contains(&f.app_id)) {
        let key = input::config_key(&old.name);
        if !desired.iter().any(|d| input::config_key(&d.name) == key) {
            set = Some(input::set_entry(set.as_deref(), &key, false));
        }
    }
    for d in desired {
        let key = input::config_key(&d.name);
        ctx.files.write_controller_layout(user, &key, &layout)?;
        set = Some(input::set_entry(set.as_deref(), &key, true));
    }
    if let Some(text) = set {
        if Some(&text) != original.as_ref() {
            ctx.files.write_controller_set(user, &text)?;
        }
    }
    Ok(())
}

/// The restart prompt, once per batch: true (and cleared) when changes are waiting for Steam.
pub fn take_restart_notice(db: &Database) -> Result<bool> {
    let pending = db.setting(RESTART_SETTING)?.as_deref() == Some("1");
    if pending {
        db.set_setting(RESTART_SETTING, "0")?;
    }
    Ok(pending)
}
