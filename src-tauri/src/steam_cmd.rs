//! The Steam shortcut screen's commands, and applying shortcut changes in the background.

use super::{internal, parse_rom_id, UiError};
use crate::state::AppState;
use base64::Engine;
use romshelf_core::clock::{Clock, SystemClock};
use romshelf_core::model::{ArtSlot, Rom, Shortcut};
use romshelf_core::steam::apply::{self, Applied, SteamContext, AUTO_ADD_SETTING};
use romshelf_core::steam::{artwork, FsSteamFiles};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{Emitter, Manager};

#[derive(Serialize)]
pub(crate) struct SlotView {
    slot: ArtSlot,
    label: &'static str,
    /// "chosen", "generated" or "missing" (Logo only).
    source: &'static str,
    /// The image as a data URL, for the preview.
    preview: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct ShortcutView {
    rom_id: String,
    shown: bool,
    /// The name in Steam (the game's name until changed).
    name: String,
    default_name: String,
    target: String,
    launch_options: String,
    slots: Vec<SlotView>,
    auto_add: bool,
}

fn label(slot: ArtSlot) -> &'static str {
    match slot {
        ArtSlot::Capsule => "Capsule",
        ArtSlot::WideCapsule => "Wide capsule",
        ArtSlot::Hero => "Hero",
        ArtSlot::Logo => "Logo",
        ArtSlot::Icon => "Icon",
    }
}

fn data_url(bytes: &[u8], ext: &str) -> String {
    let mime = match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        _ => "image/png",
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn rom(state: &AppState, rom_id: &str) -> Result<Rom, UiError> {
    let id = parse_rom_id(rom_id)?;
    state
        .db
        .lock()
        .map_err(internal)?
        .rom(&id)?
        .ok_or_else(|| UiError {
            kind: "error",
            message: "Romshelf doesn't know that game. Sync, then try again.".into(),
        })
}

fn view(state: &AppState, rom: &Rom) -> Result<ShortcutView, UiError> {
    let db = state.db.lock().map_err(internal)?;
    let row: Option<Shortcut> = db.shortcut(&rom.id)?;
    let mut slots = Vec::new();
    for slot in artwork::SLOTS {
        let chosen = row
            .as_ref()
            .and_then(|r| r.artwork.get(&slot))
            .and_then(|p| {
                let bytes = std::fs::read(p).ok()?;
                let ext = PathBuf::from(p)
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                Some(data_url(&bytes, &ext))
            });
        let (source, preview) = match chosen {
            Some(url) => ("chosen", Some(url)),
            None => match artwork::fallback(
                slot,
                &rom.display_name,
                rom.system.label(),
                rom.system.colour(),
            )? {
                Some(bytes) => ("generated", Some(data_url(&bytes, "png"))),
                None => ("missing", None),
            },
        };
        slots.push(SlotView {
            slot,
            label: label(slot),
            source,
            preview,
        });
    }
    Ok(ShortcutView {
        rom_id: rom.id.to_string(),
        shown: row.is_some(),
        name: row
            .as_ref()
            .map(|r| r.name.clone())
            .unwrap_or_else(|| rom.display_name.clone()),
        default_name: rom.display_name.clone(),
        target: state.paths.appimage.to_string_lossy().into_owned(),
        launch_options: Shortcut::launch_options_for(&rom.id),
        slots,
        auto_add: db.setting(AUTO_ADD_SETTING)?.as_deref() == Some("1"),
    })
}

/// Applies pending changes to Steam's files now. Emits `steam-changed` with what changed.
pub(crate) fn apply_now(app: &tauri::AppHandle) -> Result<Applied, UiError> {
    let state = app.state::<AppState>();
    let files = FsSteamFiles {
        steam_root: state.paths.steam_root.clone(),
    };
    let db = state.db.lock().map_err(internal)?;
    let done = apply::apply(&SteamContext {
        db: &db,
        files: &files,
        paths: &state.paths,
        now: SystemClock.now(),
    })?;
    drop(db);
    let _ = app.emit("library-changed", ());
    Ok(done)
}

#[tauri::command]
pub(crate) fn steam_shortcut(
    state: tauri::State<'_, AppState>,
    rom_id: String,
) -> Result<ShortcutView, UiError> {
    let rom = rom(&state, &rom_id)?;
    view(&state, &rom)
}

/// Saves the shortcut screen: shown or not, the name, then applies. Returns what changed.
#[tauri::command]
pub(crate) fn save_shortcut(
    app: tauri::AppHandle,
    rom_id: String,
    shown: bool,
    name: String,
) -> Result<Applied, UiError> {
    {
        let state = app.state::<AppState>();
        let rom = rom(&state, &rom_id)?;
        let db = state.db.lock().map_err(internal)?;
        apply::set_shown(&db, &rom, shown)?;
        if shown {
            apply::rename(&db, &rom, &name)?;
        }
    }
    apply_now(&app)
}

/// Uses an image file for a slot (`path` empty: back to generated). The game must be in Steam.
#[tauri::command]
pub(crate) fn set_shortcut_artwork(
    app: tauri::AppHandle,
    rom_id: String,
    slot: ArtSlot,
    path: Option<String>,
) -> Result<ShortcutView, UiError> {
    let state = app.state::<AppState>();
    let rom = rom(&state, &rom_id)?;
    {
        let db = state.db.lock().map_err(internal)?;
        if db.shortcut(&rom.id)?.is_none() {
            apply::set_shown(&db, &rom, true)?;
        }
        apply::set_artwork(
            &db,
            &state.paths,
            &rom,
            slot,
            path.as_deref().map(std::path::Path::new),
        )?;
    }
    view(&state, &rom)
}

/// "Add every new ROM to Steam automatically".
#[tauri::command]
pub(crate) fn set_steam_auto_add(app: tauri::AppHandle, on: bool) -> Result<Applied, UiError> {
    {
        let state = app.state::<AppState>();
        let db = state.db.lock().map_err(internal)?;
        db.set_setting(AUTO_ADD_SETTING, if on { "1" } else { "0" })?;
        apply::auto_add(&db)?;
    }
    apply_now(&app)
}

/// True once per batch of changes Steam hasn't seen: the restart prompt.
#[tauri::command]
pub(crate) fn steam_restart_notice(state: tauri::State<'_, AppState>) -> Result<bool, UiError> {
    let db = state.db.lock().map_err(internal)?;
    Ok(apply::take_restart_notice(&db)?)
}

#[derive(Serialize)]
pub(crate) struct ImageFile {
    path: String,
    name: String,
    folder: String,
}

/// Images to choose artwork from: Pictures, Downloads and Desktop (and their first level of
/// folders), newest first.
#[tauri::command]
pub(crate) fn image_files() -> Vec<ImageFile> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, ImageFile)> = Vec::new();
    let mut visit = |dir: &std::path::Path, folder: &str| {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            let ext = p
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if p.is_file() && ["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) {
                let modified = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                found.push((
                    modified,
                    ImageFile {
                        path: p.to_string_lossy().into_owned(),
                        name: e.file_name().to_string_lossy().into_owned(),
                        folder: folder.to_string(),
                    },
                ));
            }
        }
    };
    for top in ["Pictures", "Downloads", "Desktop"] {
        let dir = home.join(top);
        visit(&dir, top);
        if let Ok(subs) = std::fs::read_dir(&dir) {
            for s in subs.flatten().filter(|s| s.path().is_dir()) {
                visit(
                    &s.path(),
                    &format!("{top}/{}", s.file_name().to_string_lossy()),
                );
            }
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().take(200).map(|(_, f)| f).collect()
}

/// After a sync: new games go into Steam when that's turned on, and any pending change is
/// written. In the background; emits `steam-changed` when something changed.
pub(crate) fn after_sync(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        {
            let state = app.state::<AppState>();
            let Ok(db) = state.db.lock() else { return };
            if let Err(e) = apply::auto_add(&db) {
                eprintln!("Couldn't add new games to Steam: {e}");
            }
        }
        match apply_now(&app) {
            Ok(done) if done.restart_needed => {
                let _ = app.emit("steam-changed", done.changes());
            }
            Ok(_) => {}
            Err(e) => eprintln!("Couldn't update Steam's shortcuts: {}", e.message),
        }
    });
}
