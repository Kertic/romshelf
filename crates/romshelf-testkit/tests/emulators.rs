//! Stage 4: emulator registry, detection, installation, cores, session folders, BIOS, setup.

use romshelf_core::db::Database;
use romshelf_core::drive::scaffold::{self, DriveLayout};
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::emulators::setup::{
    self, BiosFile, EmulatorStatus, SetupContext, SetupEvent, SetupStep,
};
use romshelf_core::emulators::{Dolphin, EmulatorAdapter, Registry, RetroArch};
use romshelf_core::model::System;
use romshelf_core::paths::Paths;
use romshelf_testkit::{FakeDrive, FakeHttp, FakePackageRunner};
use std::io::Write;

struct Rig {
    _dir: tempfile::TempDir,
    runner: FakePackageRunner,
    http: FakeHttp,
    db: Database,
    paths: Paths,
    drive: FakeDrive,
    layout: DriveLayout,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let drive = FakeDrive::default();
        let layout = scaffold::setup(&drive, ROOT, &mut |_: &str| {}).unwrap();
        Rig {
            paths: Paths::under(dir.path()),
            _dir: dir,
            runner: FakePackageRunner::default(),
            http: FakeHttp::default(),
            db: Database::open_in_memory().unwrap(),
            drive,
            layout,
        }
    }

    fn ctx(&self) -> SetupContext<'_> {
        SetupContext {
            runner: &self.runner,
            http: &self.http,
            db: &self.db,
            paths: &self.paths,
            drive: Some((&self.drive, &self.layout)),
        }
    }

    /// Cores RetroArch needs, already in place.
    fn with_cores(&self) {
        let dir = self.paths.retroarch_cores_dir();
        std::fs::create_dir_all(&dir).unwrap();
        for core in ["snes9x", "mgba", "mupen64plus_next"] {
            std::fs::write(dir.join(format!("{core}_libretro.so")), b"\x7fELF core").unwrap();
        }
    }

    fn flatpak_calls(&self) -> Vec<String> {
        self.runner
            .calls()
            .into_iter()
            .map(|c| c.join(" "))
            .collect()
    }
}

/// A zip holding one file, as the libretro buildbot serves cores.
fn zip_of(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap();
    }
    buf.into_inner()
}

const NOT_INSTALLED: &str =
    "error: org.DolphinEmu.dolphin-emu/*unspecified*/*unspecified* not installed";

#[test]
fn every_recognized_extension_resolves_to_one_emulator_or_says_none_yet() {
    let reg = Registry::default();
    for system in System::ALL {
        for ext in system.extensions() {
            let file = format!("game.{ext}");
            match reg.for_file(system, &file) {
                Some(a) => assert!(a.systems().contains(&system)),
                None => assert_eq!(system, System::Ps2, "{system} {ext}"),
            }
        }
    }
    let err = match reg.require(System::Ps2) {
        Err(e) => e,
        Ok(_) => panic!("PS2 has no emulator yet"),
    };
    assert_eq!(err.to_string(), "Romshelf can't run PS2 games yet.");
    assert_eq!(reg.require(System::Gamecube).unwrap().id(), "dolphin");
    assert_eq!(RetroArch::core_for(System::Gba), Some("mgba"));
    assert_eq!(RetroArch::core_for(System::N64), Some("mupen64plus_next"));
}

#[test]
fn detection_reads_installed_and_not_installed() {
    let r = Rig::new();
    r.runner.push_result(
        0,
        "Ref: app/org.libretro.RetroArch/x86_64/stable\nInstallation: user\n",
        "",
    );
    assert!(RetroArch.detect(&r.runner).unwrap());
    r.runner.push_result(1, "", NOT_INSTALLED);
    assert!(!Dolphin.detect(&r.runner).unwrap());
    // Any installation counts (EmuDeck may have installed it system-wide), so no --user here.
    assert_eq!(
        r.flatpak_calls(),
        [
            "info org.libretro.RetroArch",
            "info org.DolphinEmu.dolphin-emu"
        ]
    );
}

#[test]
fn installing_issues_the_exact_commands_once_and_reports_progress() {
    let r = Rig::new();
    r.runner.push_result(1, "", NOT_INSTALLED); // info
    r.runner.push_result(0, "", ""); // remote-add
    r.runner.push_result(
        0,
        "Installing 1/2… 12%\rInstalling 1/2… 58%\nInstalling 2/2… 100%\n",
        "",
    ); // install
    let mut events = Vec::new();
    setup::run_setup(&r.ctx(), &Dolphin, &mut |e| events.push(e)).unwrap();
    assert_eq!(
        r.flatpak_calls(),
        [
            "info org.DolphinEmu.dolphin-emu",
            "remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo",
            "install --user --noninteractive -y flathub org.DolphinEmu.dolphin-emu",
        ]
    );
    let percents: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            SetupEvent::Progress { percent } => Some(*percent),
            _ => None,
        })
        .collect();
    assert_eq!(percents, [12, 58, 100]);
    assert_eq!(
        setup::status(&r.ctx(), &Dolphin).unwrap(),
        EmulatorStatus::Installed
    );
    assert!(r.db.emulator("dolphin").unwrap().unwrap().installed);
}

#[test]
fn an_installed_emulator_is_not_installed_again() {
    let r = Rig::new();
    r.with_cores();
    r.runner.push_result(0, "Installation: system\n", ""); // info
    setup::run_setup(&r.ctx(), &RetroArch, &mut |_| {}).unwrap();
    assert_eq!(r.flatpak_calls(), ["info org.libretro.RetroArch"]);
}

#[test]
fn an_install_failure_says_why_and_a_second_attempt_works() {
    let r = Rig::new();
    r.runner.push_result(1, "", NOT_INSTALLED);
    r.runner.push_result(0, "", "");
    r.runner
        .push_result(1, "", "error: Unable to connect to dl.flathub.org");
    let err = setup::run_setup(&r.ctx(), &Dolphin, &mut |_| {}).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("Couldn't install Dolphin"), "{text}");
    assert!(
        text.contains("Unable to connect to dl.flathub.org"),
        "{text}"
    );
    r.runner.push_result(1, "", NOT_INSTALLED);
    assert_eq!(
        setup::status(&r.ctx(), &Dolphin).unwrap(),
        EmulatorStatus::NotInstalled
    );
    assert!(setup::completed(&r.db, &Dolphin).unwrap().is_empty());

    r.runner.push_result(1, "", NOT_INSTALLED);
    r.runner.push_result(0, "", "");
    r.runner.push_result(0, "100%\n", "");
    setup::run_setup(&r.ctx(), &Dolphin, &mut |_| {}).unwrap();
    assert_eq!(
        setup::status(&r.ctx(), &Dolphin).unwrap(),
        EmulatorStatus::Installed
    );
}

#[test]
fn a_missing_core_is_fetched_and_a_present_one_left_alone() {
    let r = Rig::new();
    let dir = r.paths.retroarch_cores_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("snes9x_libretro.so"), b"the user's own snes9x").unwrap();
    std::fs::write(dir.join("mupen64plus_next_libretro.so"), b"present").unwrap();
    r.http
        .push_bytes(200, zip_of("mgba_libretro.so", b"\x7fELF mgba"));

    r.runner.push_result(0, "Installation: user\n", ""); // info: RetroArch installed
    setup::run_setup(&r.ctx(), &RetroArch, &mut |_| {}).unwrap();

    let reqs = r.http.requests();
    assert_eq!(reqs.len(), 1, "only the missing core");
    assert_eq!(
        reqs[0].url,
        "https://buildbot.libretro.com/nightly/linux/x86_64/latest/mgba_libretro.so.zip"
    );
    assert_eq!(
        std::fs::read(dir.join("mgba_libretro.so")).unwrap(),
        b"\x7fELF mgba"
    );
    assert_eq!(
        std::fs::read(dir.join("snes9x_libretro.so")).unwrap(),
        b"the user's own snes9x"
    );
}

#[test]
fn a_bad_core_download_is_reported_and_nothing_is_written() {
    let r = Rig::new();
    r.http.push(404, "Not Found");
    r.runner.push_result(0, "Installation: user\n", "");
    let err = setup::run_setup(&r.ctx(), &RetroArch, &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("snes9x"), "{err}");
    let dir = r.paths.retroarch_cores_dir();
    assert!(!dir.join("snes9x_libretro.so").exists());
    assert_eq!(
        setup::status(&r.ctx(), &RetroArch).unwrap(),
        EmulatorStatus::SetupIncomplete
    );
}

#[test]
fn setup_resumes_after_the_first_step() {
    let r = Rig::new();
    r.with_cores();
    setup::record_step(&r.db, &RetroArch, SetupStep::Install).unwrap();
    r.runner.push_result(0, "Installation: user\n", "");
    let mut steps = Vec::new();
    setup::run_setup(&r.ctx(), &RetroArch, &mut |e| {
        if let SetupEvent::Step { step } = e {
            steps.push(step)
        }
    })
    .unwrap();
    assert_eq!(steps, [SetupStep::DefaultControls, SetupStep::LinkSaves]);
    assert!(
        !r.flatpak_calls().iter().any(|c| c.starts_with("install")),
        "install not repeated"
    );
    assert_eq!(
        setup::completed(&r.db, &RetroArch).unwrap(),
        SetupStep::ALL.to_vec()
    );
}

#[test]
fn status_is_not_installed_setup_incomplete_or_installed() {
    let r = Rig::new();
    r.runner.push_result(1, "", NOT_INSTALLED);
    assert_eq!(
        setup::status(&r.ctx(), &RetroArch).unwrap(),
        EmulatorStatus::NotInstalled
    );
    // Installed by EmuDeck, but Romshelf's setup hasn't run.
    r.runner.push_result(0, "Installation: user\n", "");
    assert_eq!(
        setup::status(&r.ctx(), &RetroArch).unwrap(),
        EmulatorStatus::SetupIncomplete
    );
}

#[test]
fn session_folders_are_romshelfs_own_and_the_users_config_is_untouched() {
    let r = Rig::new();
    r.with_cores();
    let users_cfg = r
        .paths
        .flatpak_app_dir("org.libretro.RetroArch")
        .join("config/retroarch/retroarch.cfg");
    std::fs::create_dir_all(users_cfg.parent().unwrap()).unwrap();
    std::fs::write(&users_cfg, b"video_fullscreen = \"true\"\n").unwrap();
    let dolphin_ini = r
        .paths
        .flatpak_app_dir("org.DolphinEmu.dolphin-emu")
        .join("config/dolphin-emu/Dolphin.ini");
    std::fs::create_dir_all(dolphin_ini.parent().unwrap()).unwrap();
    std::fs::write(&dolphin_ini, b"[General]\n").unwrap();

    r.runner.push_result(0, "Installation: user\n", "");
    setup::run_setup(&r.ctx(), &RetroArch, &mut |_| {}).unwrap();
    r.runner.push_result(0, "Installation: user\n", "");
    setup::run_setup(&r.ctx(), &Dolphin, &mut |_| {}).unwrap();

    assert_eq!(
        std::fs::read(&users_cfg).unwrap(),
        b"video_fullscreen = \"true\"\n"
    );
    assert_eq!(std::fs::read(&dolphin_ini).unwrap(), b"[General]\n");

    let session = RetroArch.session_dir(&r.paths);
    assert!(session.starts_with(r.paths.flatpak_app_dir("org.libretro.RetroArch")));
    let append = std::fs::read_to_string(session.join("romshelf.cfg")).unwrap();
    for key in [
        "savefile_directory",
        "savestate_directory",
        "system_directory",
    ] {
        assert!(append.contains(key), "{key} in {append}");
    }
    assert!(session.join("saves").is_dir());
    let dolphin_user = Dolphin.session_dir(&r.paths);
    assert!(dolphin_user.is_dir());
    assert_ne!(
        dolphin_user,
        dolphin_ini.parent().unwrap().parent().unwrap()
    );
}

/// A test adapter that needs a BIOS file, since the first adapters need none.
struct NeedsBios;

impl EmulatorAdapter for NeedsBios {
    fn id(&self) -> &'static str {
        "needs-bios"
    }
    fn name(&self) -> &'static str {
        "Needs BIOS"
    }
    fn systems(&self) -> &'static [System] {
        &[System::Ps2]
    }
    fn flatpak_id(&self) -> &'static str {
        "org.example.NeedsBios"
    }
    fn bios_files(&self, _system: System) -> Vec<BiosFile> {
        vec![
            BiosFile::required("scph39001.bin"),
            BiosFile::required("rom1.bin"),
            BiosFile::optional("erom.bin"),
        ]
    }
    fn launch_command(
        &self,
        _rom: &romshelf_core::model::Rom,
        _session: &romshelf_core::emulators::LaunchSession,
    ) -> romshelf_core::Result<romshelf_core::process::CommandSpec> {
        unreachable!()
    }
    fn save_paths(
        &self,
        _rom: &romshelf_core::model::Rom,
        _paths: &Paths,
    ) -> Vec<std::path::PathBuf> {
        Vec::new()
    }
    fn write_controls(
        &self,
        _layout: &romshelf_core::emulators::ControlLayout,
        _rom: &romshelf_core::model::Rom,
    ) -> romshelf_core::Result<()> {
        Ok(())
    }
    fn declared_actions(&self) -> Vec<romshelf_core::emulators::DeclaredAction> {
        Vec::new()
    }
}

#[test]
fn bios_files_are_copied_from_drive_and_missing_ones_named() {
    let r = Rig::new();
    let folder = &r.layout.bios_folders["ps2"];
    r.drive
        .create_file("scph39001.bin", folder, b"dummy bios")
        .unwrap();
    r.runner.push_result(0, "Installation: user\n", "");
    let report = setup::run_setup(&r.ctx(), &NeedsBios, &mut |_| {}).unwrap();
    assert_eq!(report.copied, ["scph39001.bin"]);
    assert_eq!(report.missing_required, ["rom1.bin"]);
    assert_eq!(report.missing_optional, ["erom.bin"]);
    let copied = NeedsBios
        .bios_dir(&r.paths, System::Ps2)
        .join("scph39001.bin");
    assert_eq!(std::fs::read(copied).unwrap(), b"dummy bios");
    assert!(report
        .summary()
        .contains("Missing BIOS: rom1.bin. Add it to Drive's Emulation/bios/ps2 folder."));
}

#[test]
fn retroarch_and_dolphin_declare_different_actions() {
    let gba: Vec<String> = RetroArch
        .declared_actions()
        .into_iter()
        .map(|a| a.label)
        .collect();
    let gc: Vec<String> = Dolphin
        .declared_actions()
        .into_iter()
        .map(|a| a.label)
        .collect();
    assert_ne!(gba, gc);
    assert!(gc.contains(&"Memory cards".to_string()));
    assert!(gba.contains(&"Cheats".to_string()));
}

#[test]
fn listing_reports_each_emulator_with_its_systems_and_status() {
    let r = Rig::new();
    r.runner.push_result(0, "Installation: user\n", "");
    r.runner.push_result(1, "", NOT_INSTALLED);
    let list = setup::list(&r.ctx(), &Registry::default()).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name, "RetroArch");
    assert_eq!(list[0].systems, ["SNES", "GBA", "N64"]);
    assert_eq!(list[0].status, EmulatorStatus::SetupIncomplete);
    assert_eq!(list[1].name, "Dolphin");
    assert_eq!(list[1].status, EmulatorStatus::NotInstalled);
    // The library's "Needs Dolphin" comes from the database flag, kept in step here.
    assert!(!r.db.emulator("dolphin").unwrap().unwrap().installed);
    assert!(r.db.emulator("retroarch").unwrap().unwrap().installed);
}

#[test]
fn drive_access_is_read_only_during_setup() {
    let r = Rig::new();
    r.drive
        .create_file("scph39001.bin", &r.layout.bios_folders["ps2"], b"x")
        .unwrap();
    let before = r.drive.start_page_token().unwrap();
    r.runner.push_result(0, "Installation: user\n", "");
    setup::run_setup(&r.ctx(), &NeedsBios, &mut |_| {}).unwrap();
    assert_eq!(
        r.drive.start_page_token().unwrap(),
        before,
        "nothing written to Drive"
    );
}
