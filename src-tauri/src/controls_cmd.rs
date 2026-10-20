//! The Controls editor's commands: read a layout, save it, reset a game, make a game's layout
//! the default. Every change is mirrored to Drive in the background.

use super::{internal, parse_rom_id, UiError};
use crate::state::AppState;
use romshelf_core::clock::{Clock, SystemClock};
use romshelf_core::controls::model::{game_targets, hotkey_targets};
use romshelf_core::controls::{store, Layout, Scope, TargetKind, INPUTS};
use romshelf_core::model::{RomId, System};
use serde::Serialize;
use tauri::{Emitter, Manager};

#[derive(Serialize)]
pub(crate) struct InputView {
    id: &'static str,
    label: &'static str,
    directional: bool,
    back_grip: bool,
}

#[derive(Serialize)]
pub(crate) struct TargetView {
    id: &'static str,
    label: &'static str,
    kind: TargetKind,
    directional: bool,
}

#[derive(Serialize)]
pub(crate) struct GameView {
    id: String,
    name: String,
}

/// Everything the editor needs for one scope, and for one game when given.
#[derive(Serialize)]
pub(crate) struct ControlsView {
    emulator_id: String,
    emulator: String,
    system: System,
    system_label: String,
    game: Option<GameView>,
    inputs: Vec<InputView>,
    targets: Vec<TargetView>,
    default: Layout,
    /// The game's differences (empty for none, or with no game).
    game_diff: Layout,
}

fn scope(state: &AppState, emulator_id: &str, system: &str) -> Result<Scope, UiError> {
    let system = System::from_id(system).ok_or_else(|| UiError {
        kind: "error",
        message: format!("There's no system called {system}."),
    })?;
    let adapter = state
        .registry
        .by_id(emulator_id)
        .filter(|a| a.systems().contains(&system))
        .ok_or_else(|| UiError {
            kind: "error",
            message: format!("{emulator_id} doesn't run {} games.", system.label()),
        })?;
    Ok(Scope::new(adapter.id(), system))
}

fn game(rom_id: &Option<String>) -> Result<Option<RomId>, UiError> {
    rom_id.as_deref().map(parse_rom_id).transpose()
}

/// Writes the scope's layouts to Drive's `.romshelf/controls/` in the background.
fn mirror(app: &tauri::AppHandle, scope: Scope) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let Ok(Some(folder)) = state.connection.settings().map(|s| s.drive) else {
            return;
        };
        let Ok(db) = state.db.lock() else { return };
        if let Err(e) = store::push(
            state.connection.drive.as_ref(),
            &folder.layout.meta,
            &db,
            &scope,
        ) {
            eprintln!("Couldn't copy controls to Drive: {e}");
        }
    });
}

#[tauri::command]
pub(crate) fn controls_view(
    state: tauri::State<'_, AppState>,
    emulator_id: String,
    system: String,
    rom_id: Option<String>,
) -> Result<ControlsView, UiError> {
    let scope = scope(&state, &emulator_id, &system)?;
    let db = state.db.lock().map_err(internal)?;
    let game = match game(&rom_id)? {
        Some(id) => Some(db.rom(&id)?.ok_or_else(|| UiError {
            kind: "error",
            message: "Romshelf doesn't know that game. Sync, then try again.".into(),
        })?),
        None => None,
    };
    Ok(ControlsView {
        emulator: state
            .registry
            .by_id(&scope.emulator_id)
            .map(|a| a.name().to_string())
            .unwrap_or_default(),
        system_label: scope.system.label().into(),
        inputs: INPUTS
            .iter()
            .map(|i| InputView {
                id: i.id,
                label: i.label,
                directional: i.directional,
                back_grip: i.back_grip,
            })
            .collect(),
        targets: game_targets(scope.system)
            .iter()
            .chain(hotkey_targets(&scope.emulator_id))
            .map(|t| TargetView {
                id: t.id,
                label: t.label,
                kind: t.kind,
                directional: t.directional,
            })
            .collect(),
        default: store::default_layout(&db, &scope)?,
        game_diff: match &game {
            Some(g) => store::game_layout(&db, &scope, &g.id)?,
            None => Layout::new(),
        },
        game: game.map(|g| GameView {
            id: g.id.to_string(),
            name: g.display_name,
        }),
        emulator_id: scope.emulator_id.clone(),
        system: scope.system,
    })
}

/// Saves the default (`rom_id` absent: the full layout) or a game's differences.
#[tauri::command]
pub(crate) fn save_controls(
    app: tauri::AppHandle,
    emulator_id: String,
    system: String,
    rom_id: Option<String>,
    layout: Layout,
) -> Result<(), UiError> {
    let state = app.state::<AppState>();
    let scope = scope(&state, &emulator_id, &system)?;
    {
        let db = state.db.lock().map_err(internal)?;
        match game(&rom_id)? {
            Some(id) => store::set_game(&db, &scope, &id, &layout, SystemClock.now())?,
            None => store::set_default(&db, &scope, &layout, SystemClock.now())?,
        }
    }
    mirror(&app, scope);
    let _ = app.emit("library-changed", ());
    Ok(())
}

/// "Reset to {emulator} default".
#[tauri::command]
pub(crate) fn reset_controls(
    app: tauri::AppHandle,
    emulator_id: String,
    system: String,
    rom_id: String,
) -> Result<(), UiError> {
    let state = app.state::<AppState>();
    let scope = scope(&state, &emulator_id, &system)?;
    {
        let db = state.db.lock().map_err(internal)?;
        store::reset_game(&db, &scope, &parse_rom_id(&rom_id)?, SystemClock.now())?;
    }
    mirror(&app, scope);
    let _ = app.emit("library-changed", ());
    Ok(())
}

/// How many other games "Make this the default" would change, for its confirmation.
#[tauri::command]
pub(crate) fn promote_count(
    state: tauri::State<'_, AppState>,
    emulator_id: String,
    system: String,
    rom_id: String,
    layout: Layout,
) -> Result<usize, UiError> {
    let scope = scope(&state, &emulator_id, &system)?;
    let db = state.db.lock().map_err(internal)?;
    Ok(store::promote_impact_of(&db, &scope, &parse_rom_id(&rom_id)?, &layout)?.len())
}

/// "Make this the {emulator} default": saves the game's differences, then promotes them.
#[tauri::command]
pub(crate) fn promote_controls(
    app: tauri::AppHandle,
    emulator_id: String,
    system: String,
    rom_id: String,
    layout: Layout,
) -> Result<(), UiError> {
    let state = app.state::<AppState>();
    let scope = scope(&state, &emulator_id, &system)?;
    {
        let db = state.db.lock().map_err(internal)?;
        let id = parse_rom_id(&rom_id)?;
        let now = SystemClock.now();
        store::set_game(&db, &scope, &id, &layout, now)?;
        store::promote(&db, &scope, &id, now)?;
    }
    mirror(&app, scope);
    let _ = app.emit("library-changed", ());
    Ok(())
}

/// Takes layouts changed on another Deck from Drive, after a sync.
pub(crate) fn pull_in_background(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let Ok(Some(folder)) = state.connection.settings().map(|s| s.drive) else {
            return;
        };
        let Ok(db) = state.db.lock() else { return };
        match store::pull(state.connection.drive.as_ref(), &folder.layout.meta, &db) {
            Ok(updated) if !updated.is_empty() => {
                let _ = app.emit("library-changed", ());
            }
            Ok(_) => {}
            Err(e) => eprintln!("Couldn't read controls from Drive: {e}"),
        }
    });
}
