//! The Settings screen's commands (UI guide section 6: Settings).

use super::{internal, UiError};
use crate::state::AppState;
use romshelf_core::saves::{DEFAULT_KEEP, KEEP_SETTING};
use romshelf_core::settings::LocalSettings;
use romshelf_core::steam::apply::AUTO_ADD_SETTING;
use romshelf_core::storage::{self, DiskSpace, FsDiskSpace};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Serialize)]
pub(crate) struct StorageChoice {
    path: String,
    /// "This Deck" or "SD card (FF8S5)".
    label: String,
    free: Option<u64>,
    current: bool,
}

#[derive(Serialize)]
pub(crate) struct SettingsView {
    account: Option<String>,
    drive_folder: Option<String>,
    storage_root: String,
    storage_available: bool,
    storage_free: Option<u64>,
    storage_choices: Vec<StorageChoice>,
    save_versions: usize,
    auto_add: bool,
    version: &'static str,
    update_ready: Option<String>,
}

/// Where ROMs can go: `~/Romshelf`, and a `Romshelf` folder on each SD card or drive mounted
/// under `/run/media/<user>/`.
fn storage_choices(current: &Path) -> Vec<StorageChoice> {
    let mut out = Vec::new();
    let mut add = |path: PathBuf, label: String| {
        let free = path.parent().and_then(|p| FsDiskSpace.available(p).ok());
        out.push(StorageChoice {
            current: path == current,
            path: path.to_string_lossy().into_owned(),
            label,
            free,
        });
    };
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        add(
            home.join(romshelf_core::paths::DEFAULT_STORAGE_DIR),
            "This Deck".into(),
        );
    }
    if let Ok(users) = std::fs::read_dir("/run/media") {
        for user in users.flatten() {
            if let Ok(mounts) = std::fs::read_dir(user.path()) {
                for m in mounts.flatten().filter(|m| m.path().is_dir()) {
                    let name = m.file_name().to_string_lossy().into_owned();
                    add(m.path().join("Romshelf"), format!("SD card ({name})"));
                }
            }
        }
    }
    if !out.iter().any(|c| c.current) {
        out.push(StorageChoice {
            path: current.to_string_lossy().into_owned(),
            label: "Current folder".into(),
            free: FsDiskSpace.available(current).ok(),
            current: true,
        });
    }
    out
}

#[tauri::command]
pub(crate) fn settings_view(state: tauri::State<'_, AppState>) -> Result<SettingsView, UiError> {
    let local = state.connection.settings()?;
    let root = state.paths.storage_root();
    let available = storage::is_available(&state.paths);
    let db = state.db.lock().map_err(internal)?;
    Ok(SettingsView {
        account: local.account.clone(),
        drive_folder: local.drive.as_ref().map(|d| d.display_path()),
        storage_free: available
            .then(|| FsDiskSpace.available(&root).ok())
            .flatten(),
        storage_available: available,
        storage_choices: storage_choices(&root),
        storage_root: root.to_string_lossy().into_owned(),
        save_versions: db
            .setting(KEEP_SETTING)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_KEEP),
        auto_add: db.setting(AUTO_ADD_SETTING)?.as_deref() == Some("1"),
        version: env!("CARGO_PKG_VERSION"),
        update_ready: state.update_ready.lock().ok().and_then(|v| v.clone()),
    })
}

/// Backups kept per game, 1 to 50.
#[tauri::command]
pub(crate) fn set_save_versions(
    state: tauri::State<'_, AppState>,
    count: usize,
) -> Result<(), UiError> {
    if !(1..=50).contains(&count) {
        return Err(UiError {
            kind: "error",
            message: "Keep between 1 and 50 backups of each save.".into(),
        });
    }
    let db = state.db.lock().map_err(internal)?;
    db.set_setting(KEEP_SETTING, &count.to_string())?;
    Ok(())
}

/// "Sign out": forgets the Google sign-in on this Deck. Drive and the library are untouched.
#[tauri::command]
pub(crate) fn sign_out(state: tauri::State<'_, AppState>) -> Result<(), UiError> {
    state.connection.logout()?;
    Ok(())
}

/// Moves downloaded ROMs to another storage root, saves it, then restarts Romshelf so every
/// path follows.
#[tauri::command]
pub(crate) async fn change_storage(app: tauri::AppHandle, path: String) -> Result<(), UiError> {
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<usize, UiError> {
        let state = worker.state::<AppState>();
        let new_root = PathBuf::from(&path);
        if !new_root.parent().is_some_and(Path::exists) {
            return Err(UiError {
                kind: "error",
                message: format!(
                    "{} isn't there. Insert the SD card, then try again.",
                    new_root.parent().unwrap_or(&new_root).display()
                ),
            });
        }
        let report = {
            let db = state.db.lock().map_err(internal)?;
            storage::change_root(&db, &state.paths, &new_root)?
        };
        let file = state.paths.settings_file();
        let mut settings = LocalSettings::load(&file)?;
        settings.storage_root = Some(new_root);
        settings.save(&file)?;
        Ok(report.moved)
    })
    .await
    .map_err(internal)??;
    app.restart()
}

/// "Check for updates": looks now, in the background. `update-ready` follows when one installs.
#[tauri::command]
pub(crate) fn check_for_updates(app: tauri::AppHandle) {
    super::start_update_check(app);
}

/// "Copy diagnostics": what someone helping would need, with no tokens or secrets.
#[tauri::command]
pub(crate) fn diagnostics(state: tauri::State<'_, AppState>) -> Result<String, UiError> {
    let mut out = format!(
        "Romshelf {}\nData: {}\nStorage: {} ({})\n",
        env!("CARGO_PKG_VERSION"),
        state.paths.data_dir.display(),
        state.paths.storage_root().display(),
        if storage::is_available(&state.paths) {
            "available"
        } else {
            "unavailable"
        }
    );
    let local = state.connection.settings()?;
    out.push_str(&format!(
        "Signed in: {}\nDrive folder: {}\n",
        if local.account.is_some() { "yes" } else { "no" },
        local
            .drive
            .as_ref()
            .map(|d| d.display_path())
            .unwrap_or_else(|| "not chosen".into())
    ));
    {
        let db = state.db.lock().map_err(internal)?;
        let roms = db.roms()?;
        out.push_str(&format!(
            "Games: {} ({} on this Deck)\nSteam shortcuts: {}\n",
            roms.len(),
            roms.iter()
                .filter(|r| r.location == romshelf_core::model::LocationState::OnDeck)
                .count(),
            db.shortcuts()?.len()
        ));
        for a in state.registry.adapters() {
            let ready = db.emulator(a.id())?.is_some_and(|e| e.installed);
            out.push_str(&format!(
                "{}: {}\n",
                a.name(),
                if ready { "installed" } else { "not installed" }
            ));
        }
    }
    let log = std::fs::read_to_string(state.paths.log_dir().join("launch.log")).unwrap_or_default();
    let tail: Vec<&str> = log.lines().rev().take(20).collect();
    out.push_str("Last launches:\n");
    for line in tail.into_iter().rev() {
        out.push_str(&format!("  {line}\n"));
    }
    Ok(out)
}

#[derive(Serialize)]
pub(crate) struct TransferRow {
    rom_id: String,
    name: String,
    /// "download" or "backup".
    kind: &'static str,
    /// "queued", "running", or for a backup "waiting" (owed, or failed last time).
    state: &'static str,
    size: u64,
    /// Why the last backup failed.
    message: Option<String>,
}

/// The Transfers panel (UI guide section 6): downloads under way or queued, and save backups
/// still owed to Drive.
#[tauri::command]
pub(crate) fn transfers(state: tauri::State<'_, AppState>) -> Result<Vec<TransferRow>, UiError> {
    let db = state.db.lock().map_err(internal)?;
    let mut out = Vec::new();
    for (wanted, label) in [("running", "running"), ("queued", "queued")] {
        for id in db.transfers_in("download", &[wanted])? {
            if let Some(rom) = db.rom(&id)? {
                out.push(TransferRow {
                    rom_id: id.to_string(),
                    name: rom.display_name,
                    kind: "download",
                    state: label,
                    size: rom.size,
                    message: None,
                });
            }
        }
    }
    for id in db.pending_saves()? {
        if romshelf_core::launch::game_running(&state.paths) {
            break;
        }
        if let Some(rom) = db.rom(&id)? {
            let sync = db.save_sync(&id)?;
            out.push(TransferRow {
                rom_id: id.to_string(),
                name: rom.display_name,
                kind: "backup",
                state: "waiting",
                size: 0,
                message: sync.last_error,
            });
        }
    }
    Ok(out)
}

/// Asks Steam to show its on-screen keyboard (`steam://open/keyboard`), for a text field.
/// Works when Romshelf runs under Steam; elsewhere nothing happens, and the in-app keyboard is
/// there instead. Confirm under Game Mode on the Deck.
#[tauri::command]
pub(crate) fn open_steam_keyboard(state: tauri::State<'_, AppState>) {
    if state.fake {
        return;
    }
    let _ = romshelf_core::process::host_command("xdg-open")
        .arg("steam://open/keyboard")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
