//! Stage 4: emulator registry, detection, installation, cores, session folders, BIOS, setup.

use romshelf_core::db::Database;
use romshelf_core::drive::scaffold::{self, DriveLayout};
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::emulators::setup::{
    self, BiosFile, EmulatorStatus, SetupContext, SetupEvent, SetupStep,
};
use romshelf_core::emulators::{
    Dolphin, EmulatorAdapter, LaunchSession, Pcsx2, Registry, RetroArch,
};
use romshelf_core::ini;
use romshelf_core::model::System;
use romshelf_core::model::{LocationState, Rom, RomId};
use romshelf_core::paths::Paths;
use romshelf_core::process::PackageRunner;
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
            let a = reg
                .for_file(system, &file)
                .unwrap_or_else(|| panic!("no emulator for {system} {ext}"));
            assert!(a.systems().contains(&system));
        }
    }
    assert_eq!(reg.require(System::Ps2).unwrap().id(), "pcsx2");
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
        _paths: &Paths,
    ) -> romshelf_core::Result<romshelf_core::process::CommandSpec> {
        unreachable!()
    }
    fn save_dir(&self, _rom: &romshelf_core::model::Rom, _paths: &Paths) -> std::path::PathBuf {
        std::path::PathBuf::new()
    }
    fn write_controls(
        &self,
        _layout: &romshelf_core::emulators::ControlLayout,
        _rom: &romshelf_core::model::Rom,
        _paths: &Paths,
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
    r.runner.push_result(1, "", NOT_INSTALLED);
    let list = setup::list(&r.ctx(), &Registry::default()).unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].name, "RetroArch");
    assert_eq!(list[0].systems, ["SNES", "GBA", "N64"]);
    assert_eq!(list[0].status, EmulatorStatus::SetupIncomplete);
    assert_eq!(list[1].name, "Dolphin");
    assert_eq!(list[1].status, EmulatorStatus::NotInstalled);
    assert_eq!(list[2].name, "PCSX2");
    assert_eq!(list[2].systems, ["PS2"]);
    assert_eq!(list[2].status, EmulatorStatus::NotInstalled);
    // The library's "Needs Dolphin" comes from the database flag, kept in step here.
    assert!(!r.db.emulator("dolphin").unwrap().unwrap().installed);
    assert!(r.db.emulator("retroarch").unwrap().unwrap().installed);
}

#[test]
fn setup_reads_bios_and_only_adds_its_own_save_folders_to_drive() {
    let r = Rig::new();
    r.drive
        .create_file("scph39001.bin", &r.layout.bios_folders["ps2"], b"x")
        .unwrap();
    let before = r.drive.all_files();
    r.runner.push_result(0, "Installation: user\n", "");
    setup::run_setup(&r.ctx(), &NeedsBios, &mut |_| {}).unwrap();
    let after = r.drive.all_files();
    for f in &before {
        assert!(after.contains(f), "{} changed", f.name);
    }
    // "Link saves" adds `saves/ps2/` and `states/ps2/`, and nothing else.
    let added: Vec<_> = after.iter().filter(|f| !before.contains(f)).collect();
    assert_eq!(added.len(), 2);
    for f in added {
        assert!(f.is_folder() && f.name == "ps2");
        assert!(f.parents == [r.layout.saves.clone()] || f.parents == [r.layout.states.clone()]);
    }
}

#[test]
fn the_simulated_flatpak_installs_and_remembers() {
    let runner = FakePackageRunner::simulating(&["org.libretro.RetroArch"]);
    assert!(RetroArch.detect(&runner).unwrap());
    assert!(!Dolphin.detect(&runner).unwrap());
    let mut seen = Vec::new();
    let out = runner
        .flatpak_streaming(
            &[
                "install",
                "--user",
                "--noninteractive",
                "-y",
                "flathub",
                "org.DolphinEmu.dolphin-emu",
            ],
            &mut |p| seen.push(p.to_string()),
        )
        .unwrap();
    assert_eq!(out.code, 0);
    assert_eq!(seen.len(), 10);
    assert!(Dolphin.detect(&runner).unwrap());
}

#[test]
fn dolphin_starts_from_a_copy_of_the_users_settings_and_leaves_theirs_alone() {
    let r = Rig::new();
    let theirs = r
        .paths
        .flatpak_app_dir("org.DolphinEmu.dolphin-emu")
        .join("config/dolphin-emu");
    std::fs::create_dir_all(&theirs).unwrap();
    let pad = b"[GCPad1]\nDevice = SDL/0/Steam Virtual Gamepad\n";
    std::fs::write(theirs.join("GCPadNew.ini"), pad).unwrap();
    std::fs::write(theirs.join("Dolphin.ini"), b"[Core]\nGFXBackend = Vulkan\n").unwrap();

    Dolphin.prepare_session(&r.paths).unwrap();
    let ours = Dolphin::config_dir(&r.paths);
    assert!(ours.starts_with(Dolphin.session_dir(&r.paths)));
    assert_eq!(std::fs::read(ours.join("GCPadNew.ini")).unwrap(), pad);
    assert!(ours.join("Dolphin.ini").is_file());
    assert!(!ours.join("GFX.ini").exists(), "only files the user has");

    // Romshelf's copy is its own from now on: a change there stays, and theirs is untouched.
    std::fs::write(
        ours.join("GCPadNew.ini"),
        b"[GCPad1]\nDevice = changed by Romshelf\n",
    )
    .unwrap();
    std::fs::write(
        theirs.join("GCPadNew.ini"),
        b"[GCPad1]\nDevice = changed by the user\n",
    )
    .unwrap();
    Dolphin.prepare_session(&r.paths).unwrap();
    assert_eq!(
        std::fs::read(ours.join("GCPadNew.ini")).unwrap(),
        b"[GCPad1]\nDevice = changed by Romshelf\n"
    );
    assert_eq!(
        std::fs::read(theirs.join("GCPadNew.ini")).unwrap(),
        b"[GCPad1]\nDevice = changed by the user\n"
    );
}

// Stage 11: PS2 with PCSX2.

fn ps2_rom(n: u8, file: &str) -> Rom {
    Rom {
        id: RomId(format!("ps2-0000000{n}")),
        drive_file_id: format!("f{n}"),
        system: System::Ps2,
        file_name: file.into(),
        display_name: file.trim_end_matches(".iso").into(),
        size: 4,
        checksum: None,
        location: LocationState::OnDeck,
        recognized: true,
    }
}

fn pcsx2_settings(paths: &Paths) -> String {
    std::fs::read_to_string(Pcsx2::settings_file(paths)).unwrap()
}

#[test]
fn ps2_games_run_in_pcsx2_from_its_flatpak() {
    let reg = Registry::default();
    for file in ["Starfall Relay.iso", "Nine Orchards.chd"] {
        assert_eq!(reg.for_file(System::Ps2, file).unwrap().id(), "pcsx2");
    }
    let a = reg.for_system(System::Ps2).unwrap();
    assert_eq!(a.name(), "PCSX2");
    assert_eq!(a.flatpak_id(), "net.pcsx2.PCSX2");
}

#[test]
fn pcsx2_starts_the_game_full_screen_with_romshelfs_own_data_folder() {
    let r = Rig::new();
    let rom = ps2_rom(1, "Starfall Relay.iso");
    let session = LaunchSession {
        rom_path: "/home/deck/Romshelf/roms/ps2/Starfall Relay.iso".into(),
        storage_root: "/home/deck/Romshelf".into(),
    };
    let cmd = Pcsx2.launch_command(&rom, &session, &r.paths).unwrap();
    assert_eq!(cmd.program, "flatpak");
    let data = Pcsx2.session_dir(&r.paths).to_string_lossy().into_owned();
    assert_eq!(
        cmd.args,
        [
            "run",
            "--filesystem=/home/deck/Romshelf:ro",
            "net.pcsx2.PCSX2",
            "-datapath",
            data.as_str(),
            "-batch",
            "-nogui",
            "-fullscreen",
            "--",
            "/home/deck/Romshelf/roms/ps2/Starfall Relay.iso",
        ]
    );
    // PCSX2 keeps its data in <datapath>/PCSX2, its settings in inis/ there.
    assert!(Pcsx2::settings_file(&r.paths).ends_with("PCSX2/inis/PCSX2.ini"));
    assert!(Pcsx2::settings_file(&r.paths).starts_with(Pcsx2.session_dir(&r.paths)));
}

#[test]
fn pcsx2_settings_name_the_bios_skip_the_wizard_and_keep_what_else_is_there() {
    let r = Rig::new();
    let bios = Pcsx2.bios_dir(&r.paths, System::Ps2);
    std::fs::create_dir_all(&bios).unwrap();
    std::fs::write(bios.join("SCPH-70012.bin"), b"dummy").unwrap();
    std::fs::write(bios.join("readme.txt"), b"x").unwrap();
    let file = Pcsx2::settings_file(&r.paths);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "[EmuCore]
EnableCheats = true
",
    )
    .unwrap();
    Pcsx2.prepare_session(&r.paths).unwrap();
    let text = pcsx2_settings(&r.paths);
    assert_eq!(
        ini::get(&text, "UI", "SetupWizardIncomplete").as_deref(),
        Some("false")
    );
    assert_eq!(
        ini::get(&text, "Folders", "Bios").as_deref(),
        Some(bios.to_string_lossy().as_ref())
    );
    assert_eq!(
        ini::get(&text, "Filenames", "BIOS").as_deref(),
        Some("SCPH-70012.bin")
    );
    assert_eq!(
        ini::get(&text, "EmuCore", "EnableCheats").as_deref(),
        Some("true"),
        "kept"
    );
}

#[test]
fn each_ps2_game_gets_its_own_memory_card_and_state_folder() {
    let r = Rig::new();
    let a = ps2_rom(1, "Starfall Relay.iso");
    let b = ps2_rom(2, "Nine Orchards.iso");
    assert_ne!(Pcsx2.save_dir(&a, &r.paths), Pcsx2.save_dir(&b, &r.paths));
    assert!(Pcsx2.keeps_states());
    Pcsx2.prepare_session(&r.paths).unwrap();
    Pcsx2.prepare_game(&a, &r.paths).unwrap();
    let text = pcsx2_settings(&r.paths);
    let saves = Pcsx2.save_dir(&a, &r.paths);
    let states = Pcsx2.state_dir(&a, &r.paths).unwrap();
    assert!(saves.is_dir() && states.is_dir());
    assert_eq!(
        ini::get(&text, "Folders", "MemoryCards").as_deref(),
        Some(saves.to_string_lossy().as_ref())
    );
    assert_eq!(
        ini::get(&text, "Folders", "Savestates").as_deref(),
        Some(states.to_string_lossy().as_ref())
    );
    assert_eq!(
        ini::get(&text, "MemoryCards", "Slot1_Enable").as_deref(),
        Some("true")
    );
    assert_eq!(
        ini::get(&text, "MemoryCards", "Slot1_Filename").as_deref(),
        Some("Mcd001.ps2")
    );
    assert_eq!(
        ini::get(&text, "MemoryCards", "Slot2_Enable").as_deref(),
        Some("false")
    );
    // The next game points them at its own.
    Pcsx2.prepare_game(&b, &r.paths).unwrap();
    let text = pcsx2_settings(&r.paths);
    assert_eq!(
        ini::get(&text, "Folders", "MemoryCards").as_deref(),
        Some(Pcsx2.save_dir(&b, &r.paths).to_string_lossy().as_ref())
    );
}

#[test]
fn every_ps2_bios_file_in_drive_is_copied_whatever_its_name() {
    let r = Rig::new();
    assert_eq!(
        Pcsx2.bios_files(System::Ps2),
        [BiosFile::any_of("bin", "a PS2 BIOS file (.bin)")]
    );
    let folder = &r.layout.bios_folders["ps2"];
    r.drive
        .create_file("SCPH-70012.bin", folder, b"dummy bios")
        .unwrap();
    r.drive
        .create_file("SCPH-39001.BIN", folder, b"another")
        .unwrap();
    r.drive.create_file("notes.txt", folder, b"x").unwrap();
    r.runner.push_result(
        0,
        "Installation: user
",
        "",
    );
    let report = setup::run_setup(&r.ctx(), &Pcsx2, &mut |_| {}).unwrap();
    let mut copied = report.copied.clone();
    copied.sort();
    assert_eq!(copied, ["SCPH-39001.BIN", "SCPH-70012.bin"]);
    assert!(report.missing_required.is_empty());
    let dir = Pcsx2.bios_dir(&r.paths, System::Ps2);
    assert_eq!(
        std::fs::read(dir.join("SCPH-70012.bin")).unwrap(),
        b"dummy bios"
    );
    assert!(!dir.join("notes.txt").exists());
}

#[test]
fn no_ps2_bios_in_drive_says_what_to_add() {
    let r = Rig::new();
    r.runner.push_result(
        0,
        "Installation: user
",
        "",
    );
    let report = setup::run_setup(&r.ctx(), &Pcsx2, &mut |_| {}).unwrap();
    assert_eq!(report.missing_required, ["a PS2 BIOS file (.bin)"]);
    assert!(report.summary().contains(
        "Missing BIOS: a PS2 BIOS file (.bin). Add it to Drive's Emulation/bios/ps2 folder."
    ));
}
