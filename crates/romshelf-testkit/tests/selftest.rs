//! The self-test: what Romshelf checks on the Deck by itself at the press of a button.
//! Each check is shown passing, then made to fail once.

use romshelf_core::db::Database;
use romshelf_core::doctor::DoctorContext;
use romshelf_core::drive::scaffold::{self, DriveLayout};
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::emulators::{Dolphin, EmulatorAdapter, Registry, RetroArch};
use romshelf_core::model::{LocationState, Rom, RomId, System};
use romshelf_core::paths::Paths;
use romshelf_core::selftest::{self, Check, Outcome};
use romshelf_core::steam::shortcuts::{self, Desired};
use romshelf_core::steam::{artwork, input, SteamFiles};
use romshelf_testkit::{FakeDrive, FakePackageRunner, FakeSteam, FixedSpace};

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
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path()).with_storage_root(&dir.path().join("Romshelf"));
        std::fs::create_dir_all(paths.storage_root()).unwrap();
        std::fs::create_dir_all(paths.appimage.parent().unwrap()).unwrap();
        std::fs::write(&paths.appimage, b"\x7fELF romshelf").unwrap();
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
        }
    }

    fn doctor(&self) -> DoctorContext<'_> {
        DoctorContext {
            db: &self.db,
            paths: &self.paths,
            registry: &self.registry,
            runner: &self.runner,
            steam: &self.steam.files,
            account: Some("player@example.com".into()),
            layout: Some(self.layout.clone()),
            drive: Some(&self.drive),
        }
    }

    fn game(&self, id: &str, name: &str, system: System, location: LocationState) -> Rom {
        let r = Rom {
            id: RomId(id.into()),
            drive_file_id: format!("f-{id}"),
            system,
            file_name: format!("{name}.bin"),
            display_name: name.into(),
            size: 4,
            checksum: None,
            location,
            recognized: true,
        };
        self.db.upsert_rom(&r).unwrap();
        r
    }

    /// The game's shortcut as Romshelf writes it: tagged, launching it, with generated artwork
    /// and Romshelf's Steam Input layout.
    fn publish(&self, rom: &Rom, artwork_too: bool, layout_too: bool) {
        let id = shortcuts::app_id_for(rom.id.as_str());
        let existing = self.steam.files.read_shortcuts(USER).unwrap();
        let mut desired: Vec<Desired> = shortcuts::list(existing.as_deref())
            .unwrap()
            .into_iter()
            .map(|f| Desired {
                app_id: f.app_id,
                name: f.name,
                exe: self.paths.appimage.to_string_lossy().into_owned(),
                start_dir: "x/".into(),
                launch_options: f.launch_options,
                icon: String::new(),
            })
            .collect();
        desired.push(Desired {
            app_id: id,
            name: rom.display_name.clone(),
            exe: self.paths.appimage.to_string_lossy().into_owned(),
            start_dir: "x/".into(),
            launch_options: format!("launch --id {}", rom.id),
            icon: String::new(),
        });
        let ours: Vec<u32> = desired.iter().map(|d| d.app_id).collect();
        let bytes = shortcuts::reconcile(existing.as_deref(), &desired, &ours)
            .unwrap()
            .bytes
            .unwrap();
        self.steam.files.write_shortcuts(USER, &bytes, 1).unwrap();
        if artwork_too {
            for slot in artwork::SLOTS {
                if artwork::size(slot).is_some() {
                    let name = format!("{}.png", artwork::stem(slot, id));
                    self.steam.files.write_grid(USER, &name, b"png").unwrap();
                }
            }
        }
        if layout_too {
            self.steam
                .files
                .write_controller_layout(
                    USER,
                    &input::config_key(&rom.display_name),
                    "\"controller_mappings\" {}",
                )
                .unwrap();
        }
    }

    fn retroarch_config(&self, text: &str) {
        let path = RetroArch::append_config(&self.paths);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn find<'a>(checks: &'a [Check], name: &str) -> &'a Check {
    checks
        .iter()
        .find(|c| c.name.contains(name))
        .unwrap_or_else(|| panic!("no check named like {name:?} in {checks:#?}"))
}

#[test]
fn a_published_game_with_artwork_and_layout_passes() {
    let r = Rig::new();
    let kirby = r.game("gba-00000001", "Kirby", System::Gba, LocationState::OnDeck);
    r.publish(&kirby, true, true);
    let checks = selftest::steam(&r.doctor());
    for c in checks.iter().filter(|c| c.name != "Romshelf in Steam") {
        assert_eq!(c.outcome, Outcome::Pass, "{c:?}");
    }
    assert!(find(&checks, "Kirby").detail.contains("artwork"));
    // Romshelf's own shortcut isn't in this Steam: say how to add it.
    let own = find(&checks, "Romshelf in Steam");
    assert_eq!(own.outcome, Outcome::Warn);
    assert!(own
        .fix
        .as_deref()
        .unwrap()
        .contains("romshelf steam add-self"));
}

#[test]
fn a_shortcut_without_romshelfs_layout_is_named() {
    let r = Rig::new();
    let kirby = r.game("gba-00000001", "Kirby", System::Gba, LocationState::OnDeck);
    r.publish(&kirby, true, false);
    let c = find(&selftest::steam(&r.doctor()), "Kirby").clone();
    assert_eq!(c.outcome, Outcome::Warn, "{c:?}");
    assert!(c.detail.contains("no Steam Input layout"), "{c:?}");
    assert!(c.fix.unwrap().contains("romshelf steam apply"));
}

#[test]
fn missing_artwork_is_named_by_slot() {
    let r = Rig::new();
    let kirby = r.game("gba-00000001", "Kirby", System::Gba, LocationState::OnDeck);
    r.publish(&kirby, false, true);
    let c = find(&selftest::steam(&r.doctor()), "Kirby").clone();
    assert_eq!(c.outcome, Outcome::Warn, "{c:?}");
    assert!(c.detail.contains("Capsule"), "{c:?}");
}

#[cfg(unix)]
#[test]
fn a_shortcuts_file_that_lost_its_executable_bit_fails() {
    use std::os::unix::fs::PermissionsExt;
    let r = Rig::new();
    let kirby = r.game("gba-00000001", "Kirby", System::Gba, LocationState::OnDeck);
    r.publish(&kirby, true, true);
    let file = r
        .steam
        .dir
        .path()
        .join(format!("userdata/{USER}/config/shortcuts.vdf"));
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let c = find(&selftest::steam(&r.doctor()), "shortcuts file").clone();
    assert_eq!(c.outcome, Outcome::Fail, "{c:?}");
    assert!(c.detail.contains("isn't executable"));
}

#[test]
fn retroarch_config_must_turn_remaps_on() {
    let r = Rig::new();
    r.game("gba-00000001", "Kirby", System::Gba, LocationState::OnDeck);
    r.retroarch_config("input_remap_binds_enable = \"true\"\nremap_save_on_exit = \"false\"\n");
    assert_eq!(
        find(&selftest::emulator_files(&r.doctor()), "RetroArch").outcome,
        Outcome::Pass
    );
    r.retroarch_config("remap_save_on_exit = \"false\"\n");
    let c = find(&selftest::emulator_files(&r.doctor()), "RetroArch").clone();
    assert_eq!(c.outcome, Outcome::Fail, "{c:?}");
    assert!(c.detail.contains("input_remap_binds_enable"));
}

#[test]
fn a_game_not_played_yet_has_no_controls_files_and_thats_fine() {
    let r = Rig::new();
    r.game("gba-00000001", "Kirby", System::Gba, LocationState::OnDeck);
    let checks = selftest::emulator_files(&r.doctor());
    let c = find(&checks, "Kirby");
    assert_eq!(c.outcome, Outcome::Pass, "{c:?}");
    assert!(c.detail.contains("next time it's played"), "{c:?}");
}

#[test]
fn storage_is_written_read_and_cleaned_up() {
    let r = Rig::new();
    let c = selftest::storage(&r.paths, &FixedSpace::new(50 << 30));
    assert_eq!(c.outcome, Outcome::Pass, "{c:?}");
    assert!(c.detail.contains("50.0 GB free"), "{c:?}");
    assert_eq!(
        std::fs::read_dir(r.paths.storage_root()).unwrap().count(),
        0,
        "nothing left behind"
    );
    let gone = r
        .paths
        .clone()
        .with_storage_root(std::path::Path::new("/run/media/deck/NOPE/Romshelf"));
    assert_eq!(
        selftest::storage(&gone, &FixedSpace::new(1)).outcome,
        Outcome::Fail
    );
}

#[test]
fn drive_round_trip_uploads_downloads_with_a_drop_and_cleans_up() {
    let r = Rig::new();
    let c = selftest::drive_round_trip(
        &r.drive,
        &r.layout,
        &r.paths,
        &FixedSpace::new(50 << 30),
        256 * 1024,
    );
    assert_eq!(c.outcome, Outcome::Pass, "{c:?}");
    assert!(
        c.detail.contains("resumed after a dropped connection"),
        "{c:?}"
    );
    // Only Romshelf's own folder was used, and the test file went to the trash.
    let test_folder = r
        .drive
        .list(&r.layout.meta)
        .unwrap()
        .into_iter()
        .find(|f| f.name == "selftest")
        .expect("the test folder");
    assert!(r.drive.list(&test_folder.id).unwrap().is_empty());
    let sent: Vec<_> = r
        .drive
        .all_files()
        .into_iter()
        .filter(|f| f.parents.contains(&test_folder.id))
        .collect();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].trashed);
    // It really resumed: the second request started halfway, not from the beginning.
    assert_eq!(r.drive.download_offsets(&sent[0].id), vec![0, 128 * 1024]);
    // Nothing else in Drive was touched: only the scaffold, the folder and the test file.
    assert!(r.drive.all_files().iter().filter(|f| f.trashed).count() == 1);
    assert!(!r.paths.storage_root().join(".romshelf-selftest").exists());
}

#[test]
fn drive_round_trip_offline_says_so() {
    let r = Rig::new();
    r.drive.set_offline(true);
    let c = selftest::drive_round_trip(
        &r.drive,
        &r.layout,
        &r.paths,
        &FixedSpace::new(50 << 30),
        1024,
    );
    assert_eq!(c.outcome, Outcome::Fail, "{c:?}");
    assert!(c.detail.contains("couldn't reach Google"), "{c:?}");
}

#[test]
fn recent_launches_are_summarised_and_a_failure_is_named() {
    let r = Rig::new();
    assert_eq!(selftest::launches(&r.db, &r.paths).outcome, Outcome::Skip);
    r.game("gba-00000001", "Kirby", System::Gba, LocationState::OnDeck);
    std::fs::create_dir_all(r.paths.log_dir()).unwrap();
    std::fs::write(
        r.paths.log_dir().join("launch.log"),
        "1760000000 gba-00000001 retroarch 120s exit 0\n1760000500 gba-00000001 retroarch 2s exit 1\n    core failed\n",
    )
    .unwrap();
    let c = selftest::launches(&r.db, &r.paths);
    assert_eq!(c.outcome, Outcome::Warn, "{c:?}");
    assert!(c.detail.contains("2 launches"), "{c:?}");
    assert!(c.detail.contains("Kirby ended with exit 1"), "{c:?}");
    // When, so an old failure isn't mistaken for a new one.
    assert!(c.detail.contains("(2025-10-09 09:01 UTC)"), "{c:?}");
}

#[test]
fn run_reports_each_check_as_it_finishes_and_plans_every_game_on_the_deck() {
    let r = Rig::new();
    let kirby = r.game("gba-00000001", "Kirby", System::Gba, LocationState::OnDeck);
    r.game(
        "gc-00000002",
        "Twilight",
        System::Gamecube,
        LocationState::OnDeck,
    );
    r.game(
        "snes-00000003",
        "Drive only",
        System::Snes,
        LocationState::DriveOnly,
    );
    r.publish(&kirby, true, true);
    let plan = |rom: &Rom| -> romshelf_core::Result<Vec<String>> {
        if rom.system == System::Gamecube {
            Err(romshelf_core::Error::NotFound(
                "Dolphin isn't installed".into(),
            ))
        } else {
            Ok(vec!["Check the save".into(), "Start RetroArch".into()])
        }
    };
    let ctx = selftest::Context {
        doctor: r.doctor(),
        space: &FixedSpace::new(50 << 30),
        plan: &plan,
        drive_test_bytes: 1024,
    };
    let mut seen = Vec::new();
    let checks = selftest::run(&ctx, &mut |c: &Check| seen.push(c.name.clone()));
    assert_eq!(seen.len(), checks.len());
    let groups: Vec<&str> = checks.iter().map(|c| c.group.as_str()).collect();
    for g in [
        "Setup",
        "Steam",
        "Games",
        "Emulators",
        "Storage",
        "Drive",
        "Launches",
    ] {
        assert!(groups.contains(&g), "no {g} in {groups:?}");
    }
    let kirby_plan = checks
        .iter()
        .find(|c| c.group == "Games" && c.name == "Kirby")
        .unwrap();
    assert_eq!(kirby_plan.outcome, Outcome::Pass);
    assert!(kirby_plan.detail.contains("Start RetroArch"));
    let twilight = checks
        .iter()
        .find(|c| c.group == "Games" && c.name == "Twilight")
        .unwrap();
    assert_eq!(twilight.outcome, Outcome::Fail);
    assert!(twilight.detail.contains("Dolphin isn't installed"));
    assert!(!checks
        .iter()
        .any(|c| c.group == "Games" && c.name == "Drive only"));
    let text = selftest::report(&checks);
    assert!(text.contains("FAIL  Games / Twilight"));
}
