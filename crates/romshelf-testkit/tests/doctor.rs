//! Stage 10: `romshelf doctor` for the whole setup. Each check is made to fail once.

use romshelf_core::db::Database;
use romshelf_core::doctor::{self, Check, DoctorContext, Status};
use romshelf_core::drive::scaffold::{self, DriveLayout};
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::emulators::{Dolphin, EmulatorAdapter, Registry, RetroArch};
use romshelf_core::model::{LocationState, Rom, RomId, System};
use romshelf_core::paths::Paths;
use romshelf_core::saves;
use romshelf_core::steam::shortcuts::{self, Desired};
use romshelf_core::steam::SteamFiles;
use romshelf_testkit::{FakeDrive, FakePackageRunner, FakeSteam};

const USER: &str = "85959576";

struct Rig {
    _dir: tempfile::TempDir,
    paths: Paths,
    db: Database,
    registry: Registry,
    runner: FakePackageRunner,
    steam: FakeSteam,
    drive: FakeDrive,
    layout: DriveLayout,
    account: Option<String>,
}

impl Rig {
    /// A healthy install: signed in, folder chosen, storage writable, the AppImage in place,
    /// both emulators installed, a Steam account.
    fn healthy() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).with_storage_root(&dir.path().join("Romshelf"));
        std::fs::create_dir_all(paths.appimage.parent().unwrap()).unwrap();
        std::fs::write(&paths.appimage, b"\x7fELF romshelf").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&paths.appimage, std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let seed = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(seed.path().join(format!("userdata/{USER}/config"))).unwrap();
        let drive = FakeDrive::default();
        let layout = scaffold::setup(&drive, ROOT, &mut |_: &str| {}).unwrap();
        Rig {
            paths,
            _dir: dir,
            db: Database::open_in_memory().unwrap(),
            registry: Registry::default(),
            runner: FakePackageRunner::simulating(&[RetroArch.flatpak_id(), Dolphin.flatpak_id()]),
            steam: FakeSteam::new(Some(seed.path())).unwrap(),
            drive,
            layout,
            account: Some("player@example.com".into()),
        }
    }

    fn run(&self) -> Vec<Check> {
        doctor::run(&DoctorContext {
            db: &self.db,
            paths: &self.paths,
            registry: &self.registry,
            runner: &self.runner,
            steam: &self.steam.files,
            account: self.account.clone(),
            layout: Some(self.layout.clone()),
            drive: Some(&self.drive),
        })
    }

    fn check(&self, name: &str) -> Check {
        self.run()
            .into_iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no check {name}"))
    }

    fn game(&self, system: System, location: LocationState) -> Rom {
        let r = Rom {
            id: RomId(format!("{}-00000001", system.id())),
            drive_file_id: "f1".into(),
            system,
            file_name: "Game.iso".into(),
            display_name: "Game".into(),
            size: 4,
            checksum: None,
            location,
            recognized: true,
        };
        self.db.upsert_rom(&r).unwrap();
        r
    }
}

fn assert_fix(c: &Check, status: Status, detail: &str, fix: &str) {
    assert_eq!(c.status, status, "{c:?}");
    assert!(c.detail.contains(detail), "{c:?}");
    assert!(c.fix.as_deref().unwrap_or_default().contains(fix), "{c:?}");
}

#[test]
fn a_healthy_install_is_all_green() {
    let r = Rig::healthy();
    let checks = r.run();
    for c in &checks {
        assert_eq!(c.status, Status::Ok, "{c:?}");
        assert!(c.fix.is_none());
    }
    assert_eq!(doctor::exit_code(&checks), 0);
    let names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Google account",
            "Drive folder",
            "ROM storage",
            "Romshelf install",
            "RetroArch",
            "Dolphin",
            "PCSX2",
            "Steam",
            "Steam shortcuts",
            "Save backups",
            "Library index"
        ]
    );
}

#[test]
fn not_signed_in() {
    let mut r = Rig::healthy();
    r.account = None;
    assert_fix(
        &r.check("Google account"),
        Status::Fail,
        "not signed in",
        "romshelf auth login",
    );
    assert_eq!(doctor::exit_code(&r.run()), 1);
}

#[test]
fn no_drive_folder_chosen() {
    let r = Rig::healthy();
    let c = doctor::run(&DoctorContext {
        db: &r.db,
        paths: &r.paths,
        registry: &r.registry,
        runner: &r.runner,
        steam: &r.steam.files,
        account: r.account.clone(),
        layout: None,
        drive: Some(&r.drive),
    })
    .into_iter()
    .find(|c| c.name == "Drive folder")
    .unwrap();
    assert_fix(
        &c,
        Status::Fail,
        "no Drive folder chosen",
        "choose where the Emulation folder goes",
    );
}

#[test]
fn the_roms_folder_in_drives_trash() {
    let r = Rig::healthy();
    r.drive.trash(&r.layout.roms).unwrap();
    assert_fix(
        &r.check("Drive folder"),
        Status::Fail,
        "trash",
        "Restore it",
    );
}

#[test]
fn offline_the_drive_folder_is_a_warning() {
    let r = Rig::healthy();
    r.drive.set_offline(true);
    assert_fix(
        &r.check("Drive folder"),
        Status::Warn,
        "couldn't reach Google",
        "internet connection",
    );
}

#[test]
fn storage_missing() {
    let mut r = Rig::healthy();
    r.paths = r
        .paths
        .clone()
        .with_storage_root(std::path::Path::new("/run/media/deck/NOPE/Romshelf"));
    assert_fix(
        &r.check("ROM storage"),
        Status::Fail,
        "isn't there",
        "Insert the SD card",
    );
}

#[test]
fn the_appimage_missing() {
    let r = Rig::healthy();
    std::fs::remove_file(&r.paths.appimage).unwrap();
    assert_fix(
        &r.check("Romshelf install"),
        Status::Warn,
        "isn't there",
        "install.sh",
    );
}

#[cfg(unix)]
#[test]
fn the_appimage_not_executable() {
    use std::os::unix::fs::PermissionsExt;
    let r = Rig::healthy();
    std::fs::set_permissions(&r.paths.appimage, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_fix(
        &r.check("Romshelf install"),
        Status::Fail,
        "isn't executable",
        "chmod +x",
    );
}

#[test]
fn an_emulator_missing_matters_only_with_games_for_it() {
    let mut r = Rig::healthy();
    r.runner = FakePackageRunner::simulating(&[RetroArch.flatpak_id()]);
    assert_eq!(
        r.check("Dolphin").status,
        Status::Ok,
        "no GameCube games yet"
    );
    r.game(System::Gamecube, LocationState::DriveOnly);
    assert_fix(
        &r.check("Dolphin"),
        Status::Warn,
        "some of your games need it",
        "--setup dolphin",
    );
}

#[test]
fn no_steam_account() {
    let mut r = Rig::healthy();
    r.steam = FakeSteam::new(None).unwrap();
    assert_fix(
        &r.check("Steam"),
        Status::Warn,
        "no Steam account",
        "Start Steam",
    );
}

#[test]
fn shortcuts_running_another_program() {
    let r = Rig::healthy();
    let id = shortcuts::app_id_for("gba-00000001");
    let old = Desired {
        app_id: id,
        name: "Game".into(),
        exe: "/home/deck/Applications/Romshelf/romshelf.AppImage".into(),
        start_dir: "/home/deck/Applications/Romshelf/".into(),
        launch_options: "launch --id gba-00000001".into(),
        icon: String::new(),
    };
    let bytes = shortcuts::reconcile(None, &[old], &[id])
        .unwrap()
        .bytes
        .unwrap();
    r.steam.files.write_shortcuts(USER, &bytes, 1).unwrap();
    assert_fix(
        &r.check("Steam shortcuts"),
        Status::Fail,
        "1 of Romshelf's shortcuts",
        "romshelf steam apply",
    );
}

#[test]
fn shortcuts_running_this_install_are_fine() {
    let r = Rig::healthy();
    let id = shortcuts::app_id_for("gba-00000001");
    let good = Desired {
        app_id: id,
        name: "Game".into(),
        exe: r.paths.appimage.to_string_lossy().into_owned(),
        start_dir: "x/".into(),
        launch_options: "launch --id gba-00000001".into(),
        icon: String::new(),
    };
    let bytes = shortcuts::reconcile(None, &[good], &[id])
        .unwrap()
        .bytes
        .unwrap();
    r.steam.files.write_shortcuts(USER, &bytes, 1).unwrap();
    let c = r.check("Steam shortcuts");
    assert_eq!(c.status, Status::Ok, "{c:?}");
    assert!(c.detail.starts_with("1 from Romshelf"));
}

#[test]
fn saves_waiting_to_back_up() {
    let r = Rig::healthy();
    let g = r.game(System::Gba, LocationState::DriveOnly);
    saves::mark_pending(&r.db, &g.id).unwrap();
    assert_fix(
        &r.check("Save backups"),
        Status::Warn,
        "1 game's save is waiting",
        "next sync",
    );
}

#[test]
fn the_index_not_matching_the_disk() {
    let r = Rig::healthy();
    r.game(System::Gba, LocationState::OnDeck);
    assert_fix(
        &r.check("Library index"),
        Status::Warn,
        "1 game doesn't match",
        "Open Romshelf",
    );
}

#[test]
fn the_report_gives_each_problem_one_fix() {
    let mut r = Rig::healthy();
    r.account = None;
    let text = doctor::report(&r.run());
    assert!(text.contains("FAIL  Google account: not signed in\n      Fix: Sign in from Romshelf"));
    assert!(text.contains("ok    ROM storage:"));
    assert_eq!(text.matches("Fix:").count(), 1);
}
