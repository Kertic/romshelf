//! The interface's save commands: back up, the list of backups, restore, and save states.

use super::{internal, parse_rom_id, UiError};
use crate::state::AppState;
use romshelf_core::clock::SystemClock;
use romshelf_core::launch;
use romshelf_core::model::Rom;
use romshelf_core::saves::{
    self, BackupOutcome, RestoreOutcome, SaveContext, SaveVersion, StateSlot,
};
use romshelf_core::Error;
use tauri::{Emitter, Manager};

/// Runs `f` with a save context, or says Drive isn't set up yet.
pub(crate) fn with_save_ctx<T>(
    state: &AppState,
    f: impl FnOnce(&SaveContext) -> romshelf_core::Result<T>,
) -> romshelf_core::Result<T> {
    let folder = state.connection.settings()?.drive.ok_or_else(|| {
        Error::Invalid("Choose a Drive folder first, so saves have somewhere to go.".into())
    })?;
    let ctx = SaveContext {
        db: &state.db,
        drive: state.connection.drive.as_ref(),
        layout: &folder.layout,
        registry: &state.registry,
        paths: &state.paths,
        clock: &SystemClock,
        device: &state.device,
    };
    f(&ctx)
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

/// Saves are only changed while no game is running, since the emulator may still be writing.
fn not_while_playing(state: &AppState) -> Result<(), UiError> {
    let here = state.playing.lock().map(|p| p.is_some()).unwrap_or(false);
    if here || launch::game_running(&state.paths) {
        return Err(UiError {
            kind: "error",
            message: "A game is running. Quit it, then try again.".into(),
        });
    }
    Ok(())
}

/// Runs a blocking save action off the interface's thread.
async fn blocking<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&AppState) -> Result<T, UiError> + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .map_err(internal)?
}

/// Backups in Drive, newest first, for the restore list.
#[tauri::command]
pub(crate) async fn save_versions(
    app: tauri::AppHandle,
    rom_id: String,
) -> Result<Vec<SaveVersion>, UiError> {
    blocking(app, move |state| {
        let rom = rom(state, &rom_id)?;
        Ok(with_save_ctx(state, |ctx| saves::versions(ctx, &rom))?)
    })
    .await
}

/// "Back up save data".
#[tauri::command]
pub(crate) async fn backup_save(
    app: tauri::AppHandle,
    rom_id: String,
) -> Result<BackupOutcome, UiError> {
    let emitter = app.clone();
    let out = blocking(app, move |state| {
        not_while_playing(state)?;
        let rom = rom(state, &rom_id)?;
        Ok(with_save_ctx(state, |ctx| saves::backup(ctx, &rom))?)
    })
    .await;
    let _ = emitter.emit("library-changed", ());
    out
}

/// "Restore save from Drive": the chosen backup replaces the save on the Deck, which is backed
/// up first.
#[tauri::command]
pub(crate) async fn restore_save(
    app: tauri::AppHandle,
    rom_id: String,
    version_id: String,
) -> Result<RestoreOutcome, UiError> {
    let emitter = app.clone();
    let out = blocking(app, move |state| {
        not_while_playing(state)?;
        let rom = rom(state, &rom_id)?;
        Ok(with_save_ctx(state, |ctx| {
            saves::restore(ctx, &rom, &version_id)
        })?)
    })
    .await;
    let _ = emitter.emit("library-changed", ());
    out
}

/// Save state slots on the Deck.
#[tauri::command]
pub(crate) fn save_states(
    state: tauri::State<'_, AppState>,
    rom_id: String,
) -> Result<Vec<StateSlot>, UiError> {
    let rom = rom(&state, &rom_id)?;
    let adapter = state.registry.require(rom.system)?;
    Ok(saves::state_slots(&state.paths, adapter, &rom))
}

/// Deletes a save state slot from the Deck; its Drive copy goes to Drive's trash.
#[tauri::command]
pub(crate) async fn delete_save_state(
    app: tauri::AppHandle,
    rom_id: String,
    slot: String,
) -> Result<(), UiError> {
    blocking(app, move |state| {
        not_while_playing(state)?;
        let rom = rom(state, &rom_id)?;
        Ok(with_save_ctx(state, |ctx| {
            saves::delete_state(ctx, &rom, &slot)
        })?)
    })
    .await
}

/// Backs up any game whose backup is owed (Steam ended Romshelf with the game, or Drive was
/// unreachable), in the background. Emits `library-changed` and a `save-backed-up` toast per
/// game uploaded.
pub(crate) fn flush_in_background(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let Ok(results) = with_save_ctx(&state, |ctx| Ok(saves::flush_pending(ctx))) else {
            return;
        };
        let mut changed = false;
        for (id, result) in results {
            changed = true;
            if let Ok(BackupOutcome::Uploaded { .. }) = result {
                let name = state
                    .db
                    .lock()
                    .ok()
                    .and_then(|db| db.rom(&id).ok().flatten())
                    .map(|r| r.display_name);
                if let Some(name) = name {
                    let _ = app.emit("save-backed-up", format!("Backed up {name}"));
                }
            }
        }
        if changed {
            let _ = app.emit("library-changed", ());
        }
    });
}
