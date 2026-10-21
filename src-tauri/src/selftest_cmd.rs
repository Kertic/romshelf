//! The self-test (Settings → Run the self-test, and `romshelf doctor --selftest`): the checks in
//! `romshelf_core::selftest`, a probe the guided steps poll, and the report written where it can
//! be read over SSH: `~/.local/share/romshelf/logs/selftest/latest.txt`.

use super::{internal, no_answer, with_launch_ctx, UiError};
use crate::state::AppState;
use romshelf_core::launch::{self, Target};
use romshelf_core::model::{LocationState, Rom, RomId};
use romshelf_core::paths::Paths;
use romshelf_core::selftest::{self, Check};
use romshelf_core::storage::{self, FsDiskSpace};
use romshelf_core::Error;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{Emitter, Manager};

/// What's sent through Drive and back: under Drive's 5 MB simple-upload limit.
const DRIVE_TEST_BYTES: usize = 4 * 1024 * 1024;

/// Every check, in order; `on_check` hears each as it finishes.
pub(crate) fn run(
    state: &AppState,
    on_check: &mut dyn FnMut(&Check),
) -> romshelf_core::Result<Vec<Check>> {
    // Plan each game's launch first: planning takes the database lock the checks then hold.
    let roms: Vec<Rom> = state
        .db
        .lock()
        .map_err(|e| Error::External(e.to_string()))?
        .roms()?
        .into_iter()
        .filter(|r| r.recognized && r.location == LocationState::OnDeck)
        .collect();
    let mut plans: HashMap<RomId, Result<Vec<String>, String>> = HashMap::new();
    for rom in roms {
        let planned = with_launch_ctx(state, &no_answer, |ctx| {
            launch::plan(ctx, &Target::Id(rom.id.0.clone()))
        });
        plans.insert(
            rom.id.clone(),
            planned
                .map(|steps| steps.into_iter().map(|s| s.detail).collect())
                .map_err(|e| UiError::from(e).message),
        );
    }
    let plan = |rom: &Rom| -> romshelf_core::Result<Vec<String>> {
        match plans.get(&rom.id) {
            Some(Ok(steps)) => Ok(steps.clone()),
            Some(Err(message)) => Err(Error::External(message.clone())),
            None => Err(Error::NotFound(format!(
                "{} wasn't planned",
                rom.display_name
            ))),
        }
    };

    let settings = state.connection.settings()?;
    let signed_in = state
        .connection
        .status()
        .map(|s| s.signed_in)
        .unwrap_or(false);
    let steam = romshelf_core::steam::FsSteamFiles {
        steam_root: state.paths.steam_root.clone(),
    };
    let db = state
        .db
        .lock()
        .map_err(|e| Error::External(e.to_string()))?;
    let ctx = selftest::Context {
        doctor: romshelf_core::doctor::DoctorContext {
            db: &db,
            paths: &state.paths,
            registry: &state.registry,
            runner: state.runner.as_ref(),
            steam: &steam,
            account: settings.account.clone().filter(|_| signed_in),
            layout: settings.drive.as_ref().map(|d| d.layout.clone()),
            drive: signed_in.then_some(state.connection.drive.as_ref()),
        },
        space: &FsDiskSpace,
        plan: &plan,
        drive_test_bytes: DRIVE_TEST_BYTES,
    };
    Ok(selftest::run(&ctx, on_check))
}

/// Writes the report as `logs/selftest/<UTC stamp>.txt`, `latest.txt` and `latest.json`.
pub(crate) fn save(
    paths: &Paths,
    text: &str,
    json: &serde_json::Value,
) -> romshelf_core::Result<PathBuf> {
    let dir = paths.log_dir().join("selftest");
    std::fs::create_dir_all(&dir)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    std::fs::write(
        dir.join(format!("{}.txt", romshelf_core::saves::utc_stamp(now))),
        text,
    )?;
    std::fs::write(dir.join("latest.txt"), text)?;
    std::fs::write(dir.join("latest.json"), serde_json::to_vec_pretty(json)?)?;
    Ok(dir.join("latest.txt"))
}

/// `romshelf doctor --selftest`: the checks Romshelf can run without its window, printed and
/// saved. Exits 1 when any failed.
pub(crate) fn headless(state: &AppState) -> i32 {
    match run(state, &mut |c| {
        eprintln!("{:?}  {} / {}", c.outcome, c.group, c.name);
    }) {
        Ok(checks) => {
            let text = format!(
                "Romshelf {} self-test (without the interface)\n\n{}",
                env!("CARGO_PKG_VERSION"),
                selftest::report(&checks)
            );
            print!("{text}");
            let json =
                serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "checks": checks });
            if let Ok(path) = save(&state.paths, &text, &json) {
                println!("Saved to {}", path.display());
            }
            i32::from(checks.iter().any(|c| c.outcome == selftest::Outcome::Fail))
        }
        Err(e) => {
            eprintln!("{}", UiError::from(e).message);
            1
        }
    }
}

/// Runs the checks, emitting `selftest-check` with each as it finishes.
#[tauri::command]
pub(crate) async fn selftest_run(app: tauri::AppHandle) -> Result<Vec<Check>, UiError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let emitter = app.clone();
        Ok(run(&state, &mut |c| {
            let _ = emitter.emit("selftest-check", c);
        })?)
    })
    .await
    .map_err(internal)?
}

#[derive(Serialize)]
pub(crate) struct Probe {
    /// Google answered just now.
    online: bool,
    /// Games whose save backup is owed.
    pending_backups: usize,
    storage_available: bool,
    /// The ROM folder is on an SD card or other removable drive.
    storage_removable: bool,
    storage_root: String,
    /// Lines in `launch.log`, to see a new launch arrive.
    launches: usize,
}

/// What the guided steps watch for: the network, owed backups, the SD card, new launches.
#[tauri::command]
pub(crate) async fn selftest_probe(app: tauri::AppHandle) -> Result<Probe, UiError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let online = state.connection.drive.about().is_ok();
        let pending_backups = state.db.lock().map_err(internal)?.pending_saves()?.len();
        let root = state.paths.storage_root();
        let launches = std::fs::read_to_string(state.paths.log_dir().join("launch.log"))
            .map(|t| t.lines().filter(|l| !l.starts_with(' ')).count())
            .unwrap_or(0);
        Ok(Probe {
            online,
            pending_backups,
            storage_available: storage::is_available(&state.paths),
            storage_removable: root.starts_with("/run/media"),
            storage_root: root.to_string_lossy().into_owned(),
            launches,
        })
    })
    .await
    .map_err(internal)?
}

/// Backs up owed saves now (the airplane-mode step, once the network is back).
#[tauri::command]
pub(crate) fn selftest_flush(app: tauri::AppHandle) {
    super::saves_cmd::flush_in_background(&app);
}

/// Saves the whole report (these checks, the interface's and the guided steps'); its path.
#[tauri::command]
pub(crate) fn selftest_save(
    state: tauri::State<'_, AppState>,
    text: String,
    json: serde_json::Value,
) -> Result<String, UiError> {
    Ok(save(&state.paths, &text, &json)?
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_is_saved_as_latest_and_by_time() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let path = save(
            &paths,
            "pass  Setup / x: y\n",
            &serde_json::json!({"checks": []}),
        )
        .unwrap();
        assert_eq!(path, paths.log_dir().join("selftest/latest.txt"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "pass  Setup / x: y\n"
        );
        let files: Vec<_> = std::fs::read_dir(paths.log_dir().join("selftest"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files.len(), 3, "{files:?}");
        assert!(files.contains(&"latest.json".to_string()));
    }
}
