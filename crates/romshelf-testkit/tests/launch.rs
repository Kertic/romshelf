//! Stage 5: the launch pipeline behind every Steam shortcut.

use romshelf_core::db::Database;
use romshelf_core::drive::scaffold::{self, DriveLayout};
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::emulators::setup;
use romshelf_core::emulators::{Dolphin, EmulatorAdapter, Registry, RetroArch};
use romshelf_core::launch::{self, LaunchContext, LaunchEvent, Step, Target};
use romshelf_core::model::{LocationState, Rom, System};
use romshelf_core::paths::Paths;
use romshelf_core::saves::{SaveChoice, SaveConflict};
use romshelf_core::sync::{self, SyncContext};
use romshelf_core::Error;
use romshelf_testkit::{
    FakeClock, FakeDrive, FakeHttp, FakePackageRunner, FakeProcessRunner, FixedSpace,
};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Duration;

struct Rig {
    _dir: tempfile::TempDir,
    drive: FakeDrive,
    layout: DriveLayout,
    db: Mutex<Database>,
    paths: Paths,
    registry: Registry,
    runner: FakePackageRunner,
    http: FakeHttp,
    process: FakeProcessRunner,
    space: FixedSpace,
    cancel: AtomicBool,
    clock: FakeClock,
}

impl Rig {
    /// Both emulators installed by another tool (not set up by Romshelf yet).
    fn new() -> Rig {
        Rig::with_flatpaks(&[RetroArch.flatpak_id(), Dolphin.flatpak_id()])
    }

    fn with_flatpaks(installed: &[&str]) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let drive = FakeDrive::default();
        let layout = scaffold::setup(&drive, ROOT, &mut |_: &str| {}).unwrap();
        let paths = Paths::under(dir.path()).with_storage_root(&dir.path().join("Romshelf"));
        // RetroArch's cores are already present, as on the Deck.
        for core in RetroArch.required_cores() {
            let f = RetroArch::core_file(&paths, core);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, b"\x7fELF core").unwrap();
        }
        Rig {
            _dir: dir,
            drive,
            layout,
            db: Mutex::new(Database::open_in_memory().unwrap()),
            paths,
            registry: Registry::default(),
            runner: FakePackageRunner::simulating(installed),
            http: FakeHttp::default(),
            process: FakeProcessRunner::default(),
            space: FixedSpace::new(1 << 40),
            cancel: AtomicBool::new(false),
            clock: FakeClock::at(1_800_000_000),
        }
    }

    fn ctx(&self) -> LaunchContext<'_> {
        LaunchContext {
            db: &self.db,
            registry: &self.registry,
            paths: &self.paths,
            drive: &self.drive,
            layout: Some(&self.layout),
            runner: &self.runner,
            http: &self.http,
            process: &self.process,
            space: &self.space,
            cancel: &self.cancel,
            clock: &self.clock,
            retry_delay: Duration::ZERO,
            max_attempts: 3,
            device: "test-deck",
            ask: &no_answer,
            online: &|| true,
        }
    }

    fn rom(&self, system: System, name: &str) -> Rom {
        let f = self
            .drive
            .create_file(
                name,
                &self.layout.rom_folders[system.id()],
                b"tiny dummy rom",
            )
            .unwrap();
        let db = self.db.lock().unwrap();
        sync::run(&SyncContext {
            drive: &self.drive,
            db: &db,
            layout: &self.layout,
            paths: &self.paths,
            clock: &self.clock,
        })
        .unwrap();
        db.rom_by_drive_id(&f.id).unwrap().unwrap()
    }

    /// Downloaded already, with its emulator set up: nothing to wait for.
    fn ready(&self, system: System, name: &str) -> Rom {
        let rom = self.rom(system, name);
        let local = self.paths.rom_file(rom.system, &rom.file_name).unwrap();
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        std::fs::write(&local, b"tiny dummy rom").unwrap();
        let db = self.db.lock().unwrap();
        let mut r = db.rom(&rom.id).unwrap().unwrap();
        r.location = LocationState::OnDeck;
        db.upsert_rom(&r).unwrap();
        let adapter = self.registry.require(system).unwrap();
        setup::run_setup(
            &setup::SetupContext {
                runner: &self.runner,
                http: &self.http,
                db: &db,
                paths: &self.paths,
                drive: None,
            },
            adapter,
            &mut |_| {},
        )
        .unwrap();
        r
    }

    fn launch(&self, target: Target) -> (Result<i32, Error>, Vec<LaunchEvent>) {
        let mut events = Vec::new();
        let result = launch::run(&self.ctx(), &target, &mut |e| events.push(e));
        (result, events)
    }
}

/// No one to ask: a save conflict stops the launch.
fn no_answer(_: &SaveConflict) -> Option<SaveChoice> {
    None
}

fn steps(events: &[LaunchEvent]) -> Vec<Step> {
    events
        .iter()
        .filter_map(|e| match e {
            LaunchEvent::Step { step } => Some(*step),
            _ => None,
        })
        .collect()
}

fn id(rom: &Rom) -> Target {
    Target::Id(rom.id.to_string())
}

#[test]
fn retroarch_gets_its_core_the_append_config_and_the_rom() {
    let r = Rig::new();
    let rom = r.ready(System::Gba, "Kirby.gba");
    let (code, _) = r.launch(id(&rom));
    assert_eq!(code.unwrap(), 0);
    let runs = r.process.runs();
    assert_eq!(runs.len(), 1);
    let root = r.paths.storage_root();
    let rom_path = r.paths.rom_file(System::Gba, "Kirby.gba").unwrap();
    assert_eq!(runs[0].program, "flatpak");
    assert_eq!(
        runs[0].args,
        vec![
            "run".to_string(),
            format!("--filesystem={}:ro", root.display()),
            "org.libretro.RetroArch".into(),
            "-L".into(),
            RetroArch::core_file(&r.paths, "mgba").display().to_string(),
            "--appendconfig".into(),
            format!(
                "{}|{}|{}",
                RetroArch::append_config(&r.paths).display(),
                RetroArch::game_config(&r.paths, &rom).display(),
                RetroArch::controls_config(&r.paths, &rom).display()
            ),
            "-f".into(),
            rom_path.display().to_string(),
        ]
    );
}

#[test]
fn dolphin_runs_batch_full_screen_in_romshelfs_user_folder() {
    let r = Rig::new();
    let rom = r.ready(System::Gamecube, "Twilight Princess.rvz");
    r.launch(id(&rom)).0.unwrap();
    let runs = r.process.runs();
    let root = r.paths.storage_root();
    let rom_path = r
        .paths
        .rom_file(System::Gamecube, "Twilight Princess.rvz")
        .unwrap();
    assert_eq!(
        runs[0].args,
        vec![
            "run".to_string(),
            format!("--filesystem={}:ro", root.display()),
            "org.DolphinEmu.dolphin-emu".into(),
            "-u".into(),
            Dolphin.session_dir(&r.paths).display().to_string(),
            "-b".into(),
            "-C".into(),
            "Dolphin.Display.Fullscreen=True".into(),
            "-C".into(),
            "Dolphin.Core.SlotA=8".into(),
            "-C".into(),
            format!(
                "Dolphin.Core.GCIFolderAPathOverride={}",
                Dolphin.save_dir(&rom, &r.paths).display()
            ),
            "-e".into(),
            rom_path.display().to_string(),
        ]
    );
}

#[test]
fn the_emulators_exit_code_comes_back_unchanged() {
    let r = Rig::new();
    let rom = r.ready(System::Snes, "Ember.sfc");
    r.process.push_exit(3, Duration::ZERO);
    assert_eq!(r.launch(id(&rom)).0.unwrap(), 3);
    assert_eq!(r.launch(id(&rom)).0.unwrap(), 0);
}

#[test]
fn a_ready_game_goes_straight_to_the_emulator() {
    let r = Rig::new();
    let rom = r.ready(System::Gba, "Kirby.gba");
    let (_, events) = r.launch(id(&rom));
    assert_eq!(
        steps(&events),
        [Step::SaveCheck, Step::Controls, Step::Run, Step::Backup]
    );
    assert!(!launch::needs_window(&r.ctx(), &id(&rom)).unwrap());
}

#[test]
fn a_game_in_drive_only_is_downloaded_first() {
    let r = Rig::new();
    let rom = r.rom(System::Gba, "Kirby.gba");
    {
        let db = r.db.lock().unwrap();
        setup::run_setup(
            &setup::SetupContext {
                runner: &r.runner,
                http: &r.http,
                db: &db,
                paths: &r.paths,
                drive: None,
            },
            &RetroArch,
            &mut |_| {},
        )
        .unwrap();
    }
    assert!(launch::needs_window(&r.ctx(), &id(&rom)).unwrap());
    let (code, events) = r.launch(id(&rom));
    code.unwrap();
    assert_eq!(
        steps(&events),
        [
            Step::Download,
            Step::SaveCheck,
            Step::Controls,
            Step::Run,
            Step::Backup
        ]
    );
    assert!(r
        .paths
        .rom_file(System::Gba, "Kirby.gba")
        .unwrap()
        .is_file());
    assert_eq!(
        r.db.lock().unwrap().rom(&rom.id).unwrap().unwrap().location,
        LocationState::OnDeck
    );
}

#[test]
fn a_missing_emulator_is_set_up_first() {
    let r = Rig::with_flatpaks(&[RetroArch.flatpak_id()]);
    // RetroArch is set up; Dolphin isn't installed.
    let gc = r.rom(System::Gamecube, "Twilight Princess.rvz");
    let local = r
        .paths
        .rom_file(System::Gamecube, "Twilight Princess.rvz")
        .unwrap();
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(&local, b"tiny dummy rom").unwrap();
    {
        let db = r.db.lock().unwrap();
        let mut row = db.rom(&gc.id).unwrap().unwrap();
        row.location = LocationState::OnDeck;
        db.upsert_rom(&row).unwrap();
    }
    let (code, events) = r.launch(id(&gc));
    code.unwrap();
    assert_eq!(
        steps(&events),
        [
            Step::Setup,
            Step::SaveCheck,
            Step::Controls,
            Step::Run,
            Step::Backup
        ]
    );
    assert!(r
        .runner
        .calls()
        .iter()
        .any(|c| c.first().map(String::as_str) == Some("install")));
}

#[test]
fn both_missing_downloads_then_sets_up_then_launches() {
    let r = Rig::with_flatpaks(&[]);
    let rom = r.rom(System::Gamecube, "Twilight Princess.rvz");
    let (code, events) = r.launch(id(&rom));
    code.unwrap();
    assert_eq!(
        steps(&events),
        [
            Step::Download,
            Step::Setup,
            Step::SaveCheck,
            Step::Controls,
            Step::Run,
            Step::Backup
        ]
    );
    assert_eq!(r.process.runs().len(), 1);
}

#[test]
fn id_and_rom_path_find_the_same_game() {
    let r = Rig::new();
    let rom = r.ready(System::Gba, "Kirby.gba");
    let db = r.db.lock().unwrap();
    let by_id = launch::resolve(&db, &id(&rom)).unwrap();
    let by_path = launch::resolve(&db, &Target::Rom("gba/Kirby.gba".into())).unwrap();
    assert_eq!(by_id.id, by_path.id);
    assert!(launch::resolve(&db, &Target::Rom("Kirby.gba".into())).is_err());
}

#[test]
fn an_unknown_id_is_named_and_nothing_starts() {
    let r = Rig::new();
    let (result, _) = r.launch(Target::Id("gba-deadbeef".into()));
    let err = result.unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
    assert!(err.to_string().contains("gba-deadbeef"), "{err}");
    assert!(r.process.runs().is_empty());
}

#[test]
fn a_game_gone_from_drive_is_a_clear_error() {
    let r = Rig::new();
    let rom = r.ready(System::Gba, "Kirby.gba");
    {
        let db = r.db.lock().unwrap();
        let mut row = db.rom(&rom.id).unwrap().unwrap();
        row.location = LocationState::Gone;
        db.upsert_rom(&row).unwrap();
    }
    let err = r.launch(id(&rom)).0.unwrap_err();
    assert!(err.to_string().contains("no longer in your Drive"), "{err}");
    assert!(r.process.runs().is_empty());
}

#[test]
fn a_system_without_an_emulator_or_an_unrecognized_file_is_refused() {
    let r = Rig::new();
    let ps2 = r.rom(System::Ps2, "Nine Orchards.iso");
    let err = r.launch(id(&ps2)).0.unwrap_err();
    assert_eq!(err.to_string(), "Romshelf can't run PS2 games yet.");
    let zip = r.rom(System::Gamecube, "Shadow.zip");
    let err = r.launch(id(&zip)).0.unwrap_err();
    assert!(err.to_string().contains("Dolphin can't run"), "{err}");
    assert!(r.process.runs().is_empty());
}

#[test]
fn save_backup_runs_after_a_failed_exit_too() {
    let r = Rig::new();
    let rom = r.ready(System::Gba, "Kirby.gba");
    r.process.push_exit(1, Duration::ZERO);
    let (code, events) = r.launch(id(&rom));
    assert_eq!(code.unwrap(), 1);
    let s = steps(&events);
    let run = s.iter().position(|x| *x == Step::Run).unwrap();
    assert!(s.iter().position(|x| *x == Step::SaveCheck).unwrap() < run);
    assert!(s.iter().position(|x| *x == Step::Controls).unwrap() < run);
    assert!(s.iter().position(|x| *x == Step::Backup).unwrap() > run);
}

#[test]
fn the_launch_lock_is_held_while_the_emulator_runs_and_released_after() {
    let r = Rig::new();
    let rom = r.ready(System::Gba, "Kirby.gba");
    r.process.watch(&r.paths.launch_lock());
    r.launch(id(&rom)).0.unwrap();
    assert_eq!(
        r.process.watched_existed(),
        [true],
        "lock present during the run"
    );
    assert!(!r.paths.launch_lock().exists(), "and gone after");
}

#[test]
fn every_launch_is_logged() {
    let r = Rig::new();
    let rom = r.ready(System::Gba, "Kirby.gba");
    r.process.push_exit(3, Duration::ZERO);
    r.launch(id(&rom)).0.unwrap();
    let log = std::fs::read_to_string(r.paths.log_dir().join("launch.log")).unwrap();
    let line = log.lines().last().unwrap();
    assert!(line.contains(rom.id.as_str()), "{line}");
    assert!(line.contains("retroarch"), "{line}");
    assert!(line.contains("exit 3"), "{line}");
}

#[test]
fn doctor_explains_without_writing_or_starting_anything() {
    let r = Rig::with_flatpaks(&[]);
    let rom = r.rom(System::Gamecube, "Twilight Princess.rvz");
    let db_before = r.db.lock().unwrap().total_changes().unwrap();
    let plan = launch::plan(&r.ctx(), &id(&rom)).unwrap();
    let names: Vec<Step> = plan.iter().map(|p| p.step).collect();
    assert_eq!(
        names,
        [
            Step::Download,
            Step::Setup,
            Step::SaveCheck,
            Step::Controls,
            Step::Run,
            Step::Backup
        ]
    );
    let run = plan.iter().find(|p| p.step == Step::Run).unwrap();
    assert!(run.detail.contains("flatpak run"), "{}", run.detail);
    assert!(run.detail.contains("org.DolphinEmu.dolphin-emu"));
    assert_eq!(
        r.db.lock().unwrap().total_changes().unwrap(),
        db_before,
        "no writes"
    );
    assert!(r.process.runs().is_empty(), "nothing started");
    assert!(r.runner.calls().is_empty(), "no flatpak calls");
    assert!(r.drive.download_offsets(&rom.drive_file_id).is_empty());
}

#[test]
fn the_interface_and_a_launch_can_share_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("index.sqlite");
    let ui = Database::open(&file).unwrap();
    let launcher = Database::open(&file).unwrap();
    ui.set_setting("from", "ui").unwrap();
    launcher.set_setting("from", "launch").unwrap();
    assert_eq!(ui.setting("from").unwrap().as_deref(), Some("launch"));
}

#[test]
fn every_launch_prepares_the_session_folder_again() {
    let r = Rig::new();
    let rom = r.ready(System::Gamecube, "Twilight Princess.rvz");
    let theirs = r
        .paths
        .flatpak_app_dir("org.DolphinEmu.dolphin-emu")
        .join("config/dolphin-emu");
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::write(theirs.join("GCPadNew.ini"), b"[GCPad1]\n").unwrap();
    // Set up before the copy existed, as on a Deck that set up Dolphin with stage 4.
    std::fs::remove_dir_all(Dolphin.session_dir(&r.paths)).unwrap();
    r.launch(id(&rom)).0.unwrap();
    assert!(Dolphin::config_dir(&r.paths).join("GCPadNew.ini").is_file());
}

#[test]
fn the_runner_says_a_game_is_running_only_while_it_runs() {
    use romshelf_core::process::{CommandSpec, ProcessRunner};
    let runner = std::sync::Arc::new(FakeProcessRunner::default());
    runner.push_exit(0, std::time::Duration::from_millis(300));
    assert!(!runner.is_running());
    let r = std::sync::Arc::clone(&runner);
    let game = std::thread::spawn(move || {
        r.run_and_wait(&CommandSpec {
            program: "flatpak".into(),
            args: vec![],
            env: vec![],
        })
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(runner.is_running(), "while the game runs");
    game.join().unwrap().unwrap();
    assert!(!runner.is_running(), "after it ends");
}
