//! The launch pipeline: what `romshelf launch --id <rom-id>` (every Steam shortcut), the Play
//! button and `romshelf doctor --id` share.
//!
//! Find the game; download it if it isn't on the Deck; set up its emulator if needed; check its
//! save against Drive; write its controls; start the emulator and wait; back up the save.
//! The emulator's exit code is returned unchanged, whatever happens to the backup.

use crate::clock::Clock;
use crate::controls;
use crate::db::Database;
use crate::drive::scaffold::DriveLayout;
use crate::drive::DriveClient;
use crate::emulators::setup::{self, SetupContext, SetupEvent, SetupStep};
use crate::emulators::{EmulatorAdapter, LaunchSession, Registry};
use crate::http::HttpTransport;
use crate::model::{LocationState, Rom, RomId};
use crate::paths::Paths;
use crate::process::{PackageRunner, ProcessRunner};
use crate::saves::{self, SaveCheck, SaveChoice, SaveConflict, SaveContext};
use crate::storage::{self, DiskSpace};
use crate::transfer::{self, Downloader};
use crate::{Error, Result};
use serde::Serialize;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Which game to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `--id gba-0a1b2c3d`
    Id(String),
    /// `--rom gba/Kirby.gba`: the system folder and file name.
    Rom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Download,
    Setup,
    SaveCheck,
    Controls,
    Run,
    Backup,
}

impl Step {
    /// The launch window's step names (UI guide section 8).
    pub fn label(self, emulator: &str) -> String {
        match self {
            Step::Download => "Downloading".into(),
            Step::Setup => format!("Setting up {emulator}"),
            Step::SaveCheck => "Checking save".into(),
            Step::Controls => "Applying controls".into(),
            Step::Run => format!("Playing in {emulator}"),
            Step::Backup => "Backing up save".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchEvent {
    Step {
        step: Step,
    },
    /// Progress of the current step (download or setup), 0 to 100.
    Progress {
        percent: u8,
    },
    /// Something worth a line in the interface ("Restored your save from Drive").
    Notice {
        message: String,
    },
}

/// Everything a launch needs from outside. The same for the interface and the command line.
pub struct LaunchContext<'a> {
    pub db: &'a Mutex<Database>,
    pub registry: &'a Registry,
    pub paths: &'a Paths,
    pub drive: &'a dyn DriveClient,
    /// For BIOS files during setup; `None` before a Drive folder is chosen.
    pub layout: Option<&'a DriveLayout>,
    pub runner: &'a dyn PackageRunner,
    pub http: &'a dyn HttpTransport,
    pub process: &'a dyn ProcessRunner,
    pub space: &'a dyn DiskSpace,
    pub cancel: &'a AtomicBool,
    pub clock: &'a dyn Clock,
    pub retry_delay: Duration,
    pub max_attempts: u32,
    /// This machine's name, recorded in save backups.
    pub device: &'a str,
    /// Asks the user about a save conflict. `None` means no one can be asked now (a launch
    /// with no window): the launch stops with `Error::NeedsAnswer` before anything starts.
    pub ask: &'a (dyn Fn(&SaveConflict) -> Option<SaveChoice> + Sync),
}

impl LaunchContext<'_> {
    fn db(&self) -> MutexGuard<'_, Database> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Save backups need a Drive folder; `None` before one is chosen.
    pub fn saves(&self) -> Option<SaveContext<'_>> {
        self.layout.map(|layout| SaveContext {
            db: self.db,
            drive: self.drive,
            layout,
            registry: self.registry,
            paths: self.paths,
            clock: self.clock,
            device: self.device,
        })
    }
}

/// Finds the game. An unknown id or path is `NotFound` naming it; a game gone from Drive is an
/// error too, since its file can't be fetched.
pub fn resolve(db: &Database, target: &Target) -> Result<Rom> {
    let rom = match target {
        Target::Id(id) => RomId::parse(id)
            .and_then(|rid| db.rom(&rid).transpose())
            .transpose()?,
        Target::Rom(path) => {
            let (system, file) = path.split_once('/').ok_or_else(|| {
                Error::Invalid(format!(
                    "{path} isn't a ROM path. Give the system folder and file name, such as gba/Kirby.gba."
                ))
            })?;
            db.roms()?
                .into_iter()
                .find(|r| r.system.id() == system && r.file_name == file)
        }
    };
    let what = match target {
        Target::Id(id) => id.as_str(),
        Target::Rom(path) => path.as_str(),
    };
    let rom = rom.ok_or_else(|| {
        Error::NotFound(format!(
            "Romshelf doesn't know a game called {what}. Sync, then try again."
        ))
    })?;
    if rom.location == LocationState::Gone {
        return Err(Error::Invalid(format!(
            "{} is no longer in your Drive, so it can't be played. Put it back in Drive's {} folder and sync.",
            rom.display_name,
            rom.system.id()
        )));
    }
    Ok(rom)
}

/// The emulator for the game, refusing systems without one and files it can't run.
fn adapter_for<'r>(registry: &'r Registry, rom: &Rom) -> Result<&'r dyn EmulatorAdapter> {
    let adapter = registry.require(rom.system)?;
    if !rom.recognized {
        return Err(Error::Invalid(format!(
            "{} can't run {}. Convert it to one of: {}.",
            adapter.name(),
            rom.file_name,
            rom.system.extensions().join(", ")
        )));
    }
    Ok(adapter)
}

fn on_deck(paths: &Paths, rom: &Rom) -> bool {
    rom.location == LocationState::OnDeck && storage::file_matches(paths, rom)
}

/// Romshelf's setup has run for the emulator. Read from the database only (no `flatpak` call),
/// so `doctor` and the "is a window needed?" check change nothing.
fn emulator_ready(db: &Database, adapter: &dyn EmulatorAdapter) -> Result<bool> {
    let installed = db.emulator(adapter.id())?.is_some_and(|e| e.installed);
    Ok(installed && setup::completed(db, adapter)?.len() == SetupStep::ALL.len())
}

fn session(paths: &Paths, rom: &Rom) -> Result<LaunchSession> {
    Ok(LaunchSession {
        rom_path: paths.rom_file(rom.system, &rom.file_name)?,
        storage_root: paths.storage_root(),
    })
}

/// True when the game must be downloaded or its emulator set up first: then `romshelf launch`
/// shows the launch window. A ready game starts with no window at all.
pub fn needs_window(ctx: &LaunchContext, target: &Target) -> Result<bool> {
    let db = ctx.db();
    let rom = resolve(&db, target)?;
    let adapter = adapter_for(ctx.registry, &rom)?;
    Ok(!on_deck(ctx.paths, &rom) || !emulator_ready(&db, adapter)?)
}

/// One step of `romshelf doctor --id`: whether it would run and what it would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedStep {
    pub step: Step,
    pub detail: String,
}

/// What a launch would do, without doing any of it: no writes, downloads or programs started.
pub fn plan(ctx: &LaunchContext, target: &Target) -> Result<Vec<PlannedStep>> {
    let db = ctx.db();
    let rom = resolve(&db, target)?;
    let adapter = adapter_for(ctx.registry, &rom)?;
    let session = session(ctx.paths, &rom)?;
    let mut out = Vec::new();
    if !on_deck(ctx.paths, &rom) {
        out.push(PlannedStep {
            step: Step::Download,
            detail: format!(
                "Download {} ({}) to {}",
                rom.file_name,
                storage::human_size(rom.size),
                session.rom_path.display()
            ),
        });
    }
    if !emulator_ready(&db, adapter)? {
        out.push(PlannedStep {
            step: Step::Setup,
            detail: format!(
                "Install and set up {} ({})",
                adapter.name(),
                adapter.flatpak_id()
            ),
        });
    }
    out.push(PlannedStep {
        step: Step::SaveCheck,
        detail: "Compare the save with Drive's newest backup; restore it if only Drive's changed, ask if both did".into(),
    });
    out.push(PlannedStep {
        step: Step::Controls,
        detail: "Write this game's controls: the emulator's default with its differences".into(),
    });
    let cmd = adapter.launch_command(&rom, &session, ctx.paths)?;
    out.push(PlannedStep {
        step: Step::Run,
        detail: format!("{} {}", cmd.program, cmd.args.join(" ")),
    });
    out.push(PlannedStep {
        step: Step::Backup,
        detail: "Back up the save to Drive when the game ends".into(),
    });
    Ok(out)
}

/// Runs the whole pipeline and returns the emulator's exit code.
pub fn run(
    ctx: &LaunchContext,
    target: &Target,
    events: &mut dyn FnMut(LaunchEvent),
) -> Result<i32> {
    let rom = resolve(&ctx.db(), target)?;
    let adapter = adapter_for(ctx.registry, &rom)?;

    if !on_deck(ctx.paths, &rom) {
        events(LaunchEvent::Step {
            step: Step::Download,
        });
        transfer::enqueue_download(&ctx.db(), &rom.id)?;
        let dl = Downloader {
            drive: ctx.drive,
            paths: ctx.paths,
            space: ctx.space,
            cancel: ctx.cancel,
            retry_delay: ctx.retry_delay,
            max_attempts: ctx.max_attempts,
        };
        let progress = Mutex::new(&mut *events);
        transfer::run_pending(ctx.db, &dl, &|e| {
            if let transfer::Event::Progress { bytes, total, .. } = e {
                let percent = (bytes * 100).checked_div(total).unwrap_or(100).min(100) as u8;
                if let Ok(mut ev) = progress.lock() {
                    ev(LaunchEvent::Progress { percent });
                }
            }
        })?;
    }

    if !emulator_ready(&ctx.db(), adapter)? {
        events(LaunchEvent::Step { step: Step::Setup });
        let db = ctx.db();
        let report = setup::run_setup(
            &SetupContext {
                runner: ctx.runner,
                http: ctx.http,
                db: &db,
                paths: ctx.paths,
                drive: ctx.layout.map(|l| (ctx.drive, l)),
            },
            adapter,
            &mut |e| {
                if let SetupEvent::Progress { percent } = e {
                    events(LaunchEvent::Progress { percent });
                }
            },
        )?;
        let missing = report.summary();
        if !missing.is_empty() {
            return Err(Error::Invalid(missing));
        }
    }

    // A restore that Romshelf was ended in the middle of is put right first.
    let _ = saves::recover_interrupted(&ctx.db(), ctx.registry, ctx.paths);

    // Romshelf's session folder is checked before every launch, so an emulator set up by an
    // earlier version gets anything added since (Dolphin's copied settings).
    adapter.prepare_session(ctx.paths)?;
    adapter.prepare_game(&rom, ctx.paths)?;

    events(LaunchEvent::Step {
        step: Step::SaveCheck,
    });
    if let Some(sc) = ctx.saves() {
        // Backups owed from earlier games (Steam ended Romshelf, or Drive was unreachable).
        let _ = saves::flush_pending(&sc);
        check_save(ctx, &sc, &rom, events)?;
    }
    // Owed until the backup after the game succeeds, even if Romshelf is ended with the game.
    saves::mark_pending(&ctx.db(), &rom.id)?;

    // The game's layout: the emulator's default with this game's differences.
    events(LaunchEvent::Step {
        step: Step::Controls,
    });
    let layout = controls::store::effective(
        &ctx.db(),
        &controls::Scope::new(adapter.id(), rom.system),
        &rom.id,
    )?;
    adapter.write_controls(&layout, &rom, ctx.paths)?;

    events(LaunchEvent::Step { step: Step::Run });
    let rom = resolve(&ctx.db(), &Target::Id(rom.id.to_string()))?;
    let cmd = adapter.launch_command(&rom, &session(ctx.paths, &rom)?, ctx.paths)?;
    let started = Instant::now();
    let code = {
        let _lock = LaunchLock::take(ctx.paths)?;
        ctx.process.run_and_wait(&cmd)
    };

    // Whatever the exit code was. The emulator has exited, so its save is complete. A failed
    // backup stays owed for the next run and never changes the exit code.
    events(LaunchEvent::Step { step: Step::Backup });
    if let Some(sc) = ctx.saves() {
        match saves::backup(&sc, &rom) {
            Ok(saves::BackupOutcome::Uploaded { .. }) => events(LaunchEvent::Notice {
                message: format!("Backed up {}", rom.display_name),
            }),
            Ok(_) => {}
            Err(_) => events(LaunchEvent::Notice {
                message: "Couldn't back up the save. Romshelf tries again next time.".into(),
            }),
        }
        let _ = saves::mirror_states(&sc, &rom);
    }

    let code = code?;
    log_launch(ctx, &rom, adapter, started.elapsed(), code);
    Ok(code)
}

/// The save check: a newer save in Drive (only) is restored; a conflict is asked about. When
/// Drive can't be reached, the game starts with the save on the Deck.
fn check_save(
    ctx: &LaunchContext,
    sc: &SaveContext,
    rom: &Rom,
    events: &mut dyn FnMut(LaunchEvent),
) -> Result<()> {
    let _ = saves::fetch_states(sc, rom);
    let check = match saves::check(sc, rom) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    match check {
        SaveCheck::InSync | SaveCheck::DeckNewer => Ok(()),
        SaveCheck::DriveNewer { version } => {
            saves::restore(sc, rom, &version.id)?;
            events(LaunchEvent::Notice {
                message: format!("Restored the newer save from {}", version.device),
            });
            Ok(())
        }
        SaveCheck::Conflict { conflict } => match (ctx.ask)(&conflict) {
            None => Err(Error::NeedsAnswer),
            Some(choice) => saves::resolve_conflict(sc, rom, choice),
        },
    }
}

/// A game is running: `launch.lock` names a process that is still alive. A lock left behind
/// when Steam ended Romshelf names a process that is gone.
pub fn game_running(paths: &Paths) -> bool {
    let Ok(text) = std::fs::read_to_string(paths.launch_lock()) else {
        return false;
    };
    match text.trim().parse::<u32>() {
        Ok(pid) => process_alive(pid),
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Elsewhere (development only) a lock is trusted while it exists.
#[cfg(not(target_os = "linux"))]
fn process_alive(_pid: u32) -> bool {
    true
}

/// `launch.lock` holds this process's id while the emulator runs, so the self-updater waits.
struct LaunchLock(std::path::PathBuf);

impl LaunchLock {
    fn take(paths: &Paths) -> Result<LaunchLock> {
        let path = paths.launch_lock();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, std::process::id().to_string())?;
        Ok(LaunchLock(path))
    }
}

impl Drop for LaunchLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// One line per launch in `<data>/logs/launch.log`. A failure to log never stops a game.
fn log_launch(
    ctx: &LaunchContext,
    rom: &Rom,
    adapter: &dyn EmulatorAdapter,
    took: Duration,
    code: i32,
) {
    let dir = ctx.paths.log_dir();
    let mut line = format!(
        "{} {} {} {}s exit {}\n",
        ctx.clock.now(),
        rom.id,
        adapter.id(),
        took.as_secs(),
        code
    );
    // When the emulator failed, what it last said explains why.
    if code != 0 {
        for out in ctx.process.last_output() {
            line.push_str(&format!("    {out}\n"));
        }
    }
    // Past 1 MB the log moves to launch.log.1, keeping three.
    let _ = crate::diagnostics::rotate(
        &dir.join("launch.log"),
        crate::diagnostics::LOG_MAX_BYTES,
        crate::diagnostics::LOGS_KEPT,
    );
    let _ = std::fs::create_dir_all(&dir).and_then(|_| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("launch.log"))
            .and_then(|mut f| f.write_all(line.as_bytes()))
    });
}

/// How long View and Menu are held together to quit a game.
pub const QUIT_HOLD: Duration = Duration::from_secs(1);

/// Romshelf's own quit combination: View and Menu held together for `QUIT_HOLD`. It works the
/// same in every emulator and every state, whatever the game's layout, because Romshelf reads
/// the controller itself and stops the emulator cleanly (saves are written first).
#[derive(Debug, Default)]
pub struct QuitCombo {
    since: Option<Instant>,
    fired: bool,
}

impl QuitCombo {
    /// Feeds whether both buttons are held now; true once per hold, when it reaches a second.
    pub fn update(&mut self, held: bool, now: Instant) -> bool {
        if !held {
            self.since = None;
            self.fired = false;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if !self.fired && now.duration_since(since) >= QUIT_HOLD {
            self.fired = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod quit_tests {
    use super::*;

    #[test]
    fn quits_once_after_a_second_held() {
        let t0 = Instant::now();
        let mut q = QuitCombo::default();
        assert!(!q.update(true, t0));
        assert!(!q.update(true, t0 + Duration::from_millis(900)));
        assert!(q.update(true, t0 + QUIT_HOLD));
        assert!(
            !q.update(true, t0 + Duration::from_secs(3)),
            "once per hold"
        );
    }

    #[test]
    fn letting_go_starts_again() {
        let t0 = Instant::now();
        let mut q = QuitCombo::default();
        q.update(true, t0);
        assert!(!q.update(false, t0 + Duration::from_millis(800)));
        assert!(!q.update(true, t0 + Duration::from_millis(900)));
        assert!(!q.update(true, t0 + Duration::from_millis(1800)));
        assert!(q.update(true, t0 + Duration::from_millis(1900)));
    }
}
