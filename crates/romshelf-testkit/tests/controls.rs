//! Stage 7: the control model and merging.

use romshelf_core::controls::model::{from_pairs, validate};
use romshelf_core::controls::{difference, merge, promote, Layout, Scope, INPUTS};
use romshelf_core::model::{Binding, System};

fn bound(t: &str) -> Binding {
    Binding::Bound(t.into())
}

/// A GBA default in RetroArch, as the reference screen shows it.
fn gba_default() -> Layout {
    from_pairs(&[
        ("dpad", Some("dpad")),
        ("left_stick", Some("dpad")),
        ("right_stick", None),
        ("a", Some("a")),
        ("b", Some("b")),
        ("x", None),
        ("y", None),
        ("l1", Some("l")),
        ("r1", Some("r")),
        ("l2", None),
        ("r2", None),
        ("l3", None),
        ("r3", None),
        ("menu", Some("start")),
        ("view", Some("select")),
        ("l4", None),
        ("r4", None),
        ("l5", Some("save_state")),
        ("r5", Some("load_state")),
    ])
    .unwrap()
}

fn gba() -> Scope {
    Scope::new("retroarch", System::Gba)
}

#[test]
fn the_default_is_complete_and_valid() {
    let d = gba_default();
    assert_eq!(d.len(), INPUTS.len());
    validate(&gba(), &d).unwrap();
}

#[test]
fn merge_override_wins_cleared_stays_cleared_rest_follows() {
    let d = gba_default();
    let game = from_pairs(&[
        ("a", Some("b")),
        ("b", Some("a")),
        ("l5", None),
        ("l4", Some("fast_forward")),
    ])
    .unwrap();
    let m = merge(&d, &game);
    assert_eq!(m["a"], bound("b"));
    assert_eq!(m["b"], bound("a"));
    assert_eq!(
        m["l5"],
        Binding::Cleared,
        "explicitly unbound stays unbound"
    );
    assert_eq!(m["l4"], bound("fast_forward"));
    assert_eq!(m["menu"], bound("start"), "untouched follows the default");
    assert_eq!(m.len(), d.len());
}

#[test]
fn changing_the_default_moves_only_what_the_game_did_not_override() {
    let mut d = gba_default();
    let game = from_pairs(&[("a", Some("b"))]).unwrap();
    d.insert("a".into(), bound("select"));
    d.insert("menu".into(), bound("select"));
    let m = merge(&d, &game);
    assert_eq!(m["a"], bound("b"), "overridden: unchanged");
    assert_eq!(m["menu"], bound("select"), "not overridden: follows");
}

/// xorshift, so the "random" layouts are the same on every run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

fn random_layout(rng: &mut Rng, partial: bool) -> Layout {
    let targets = [
        "a",
        "b",
        "l",
        "r",
        "start",
        "select",
        "fast_forward",
        "rewind",
    ];
    let mut out = Layout::new();
    for input in INPUTS {
        if partial && rng.next().is_multiple_of(2) {
            continue;
        }
        let b = match rng.next() % 4 {
            0 => Binding::Cleared,
            _ => bound(rng.pick(&targets)),
        };
        out.insert(input.id.into(), b);
    }
    out
}

#[test]
fn difference_then_merge_gives_back_the_layout() {
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    for _ in 0..500 {
        let default = random_layout(&mut rng, false);
        let full = random_layout(&mut rng, false);
        let diff = difference(&full, &default);
        assert_eq!(merge(&default, &diff), full);
        // Differences hold only what actually differs.
        for (input, b) in &diff {
            assert_ne!(default.get(input), Some(b));
        }
        // And a game's differences survive the trip.
        let game = random_layout(&mut rng, true);
        let merged = merge(&default, &game);
        assert_eq!(merge(&default, &difference(&merged, &default)), merged);
    }
}

#[test]
fn promoting_leaves_the_game_with_no_differences_and_others_keep_theirs() {
    let d = gba_default();
    let this = from_pairs(&[("a", Some("b")), ("l4", Some("fast_forward"))]).unwrap();
    let other = from_pairs(&[("a", Some("select"))]).unwrap();
    let before_other = merge(&d, &other);
    let (new_default, this_after) = promote(&d, &this);
    assert!(this_after.is_empty());
    assert_eq!(merge(&new_default, &this_after), merge(&d, &this));
    let after_other = merge(&new_default, &other);
    assert_eq!(after_other["a"], bound("select"), "its own override kept");
    assert_eq!(
        after_other["l4"],
        bound("fast_forward"),
        "no override: follows"
    );
    for (input, b) in &before_other {
        if input != "l4" {
            assert_eq!(&after_other[input], b, "{input} changed");
        }
    }
}

#[test]
fn resetting_a_game_is_zero_differences() {
    let d = gba_default();
    let full = merge(&d, &Layout::new());
    assert!(difference(&full, &d).is_empty());
}

#[test]
fn two_inputs_may_share_a_target_but_one_input_cannot_have_two() {
    let shared = from_pairs(&[("dpad", Some("dpad")), ("left_stick", Some("dpad"))]).unwrap();
    validate(&gba(), &shared).unwrap();
    let err = from_pairs(&[("a", Some("a")), ("a", Some("b"))]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "A is bound twice. An input can do only one thing."
    );
}

#[test]
fn validation_names_what_is_wrong() {
    let bad = |pairs: &[(&str, Option<&str>)]| {
        validate(&gba(), &from_pairs(pairs).unwrap())
            .unwrap_err()
            .to_string()
    };
    assert_eq!(bad(&[("q", Some("a"))]), "q isn't a Deck input.");
    assert_eq!(
        bad(&[("a", Some("x"))]),
        "x isn't something retroarch can do for GBA."
    );
    assert_eq!(
        bad(&[("a", Some("dpad"))]),
        "D-pad needs the D-pad or a stick, not A."
    );
    assert_eq!(
        bad(&[("dpad", Some("a"))]),
        "A is a button; bind it to a button, not D-pad."
    );
    // Dolphin has no rewind.
    let gc = Scope::new("dolphin", System::Gamecube);
    let err = validate(&gc, &from_pairs(&[("l4", Some("rewind"))]).unwrap()).unwrap_err();
    assert!(err.to_string().contains("rewind"));
}

#[test]
fn scopes_are_stored_as_emulator_and_system() {
    assert_eq!(gba().key(), "retroarch/gba");
    assert_eq!(
        Scope::from_key("dolphin/gamecube"),
        Some(Scope::new("dolphin", System::Gamecube))
    );
    assert_eq!(Scope::from_key("retroarch"), None);
}

// Built-in defaults and storage

use romshelf_core::controls::{defaults, store};
use romshelf_core::db::Database;
use romshelf_core::drive::scaffold;
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::emulators::Registry;
use romshelf_core::model::{LocationState, Rom, RomId};
use romshelf_testkit::FakeDrive;

#[test]
fn every_built_in_default_is_complete_and_valid() {
    for adapter in Registry::default().adapters() {
        for system in adapter.systems() {
            let scope = Scope::new(adapter.id(), *system);
            let d = defaults::built_in(&scope);
            assert_eq!(d.len(), INPUTS.len(), "{}", scope.key());
            validate(&scope, &d).unwrap_or_else(|e| panic!("{}: {e}", scope.key()));
        }
    }
    // Hotkeys sit on the back grips.
    let d = defaults::built_in(&gba());
    assert_eq!(d["l4"], bound("fast_forward"));
    assert_eq!(d["l5"], bound("save_state"));
}

fn rom(db: &Database, n: u8, system: System) -> Rom {
    let r = Rom {
        id: RomId(format!("{}-0000000{n}", system.id())),
        drive_file_id: format!("file{n}"),
        system,
        file_name: format!("Game {n}.gba"),
        display_name: format!("Game {n}"),
        size: 1,
        checksum: None,
        location: LocationState::OnDeck,
        recognized: true,
    };
    db.upsert_rom(&r).unwrap();
    r
}

#[test]
fn a_game_stores_only_its_differences() {
    let db = Database::open_in_memory().unwrap();
    let g = rom(&db, 1, System::Gba);
    assert_eq!(
        store::effective(&db, &gba(), &g.id).unwrap(),
        defaults::built_in(&gba())
    );
    let swap = from_pairs(&[("a", Some("b")), ("b", Some("a"))]).unwrap();
    store::set_game(&db, &gba(), &g.id, &swap, 10).unwrap();
    assert_eq!(store::game_layout(&db, &gba(), &g.id).unwrap(), swap);
    assert_eq!(
        store::effective(&db, &gba(), &g.id).unwrap()["a"],
        bound("b")
    );
    store::reset_game(&db, &gba(), &g.id, 11).unwrap();
    assert!(store::game_layout(&db, &gba(), &g.id).unwrap().is_empty());
    assert!(store::set_game(
        &db,
        &gba(),
        &g.id,
        &from_pairs(&[("a", Some("dpad"))]).unwrap(),
        12
    )
    .is_err());
}

#[test]
fn promoting_names_the_games_that_will_change() {
    let db = Database::open_in_memory().unwrap();
    let this = rom(&db, 1, System::Gba);
    let follows = rom(&db, 2, System::Gba);
    let overrides = rom(&db, 3, System::Gba);
    let _snes = rom(&db, 4, System::Snes);
    store::set_game(
        &db,
        &gba(),
        &this.id,
        &from_pairs(&[("a", Some("b"))]).unwrap(),
        1,
    )
    .unwrap();
    store::set_game(
        &db,
        &gba(),
        &overrides.id,
        &from_pairs(&[("a", Some("select"))]).unwrap(),
        1,
    )
    .unwrap();
    let changed: Vec<RomId> = store::promote_impact(&db, &gba(), &this.id)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(changed, std::slice::from_ref(&follows.id));
    let before = store::effective(&db, &gba(), &this.id).unwrap();
    store::promote(&db, &gba(), &this.id, 2).unwrap();
    assert!(store::game_layout(&db, &gba(), &this.id)
        .unwrap()
        .is_empty());
    assert_eq!(store::effective(&db, &gba(), &this.id).unwrap(), before);
    assert_eq!(
        store::effective(&db, &gba(), &follows.id).unwrap()["a"],
        bound("b")
    );
    assert_eq!(
        store::effective(&db, &gba(), &overrides.id).unwrap()["a"],
        bound("select")
    );
}

#[test]
fn layouts_follow_the_user_to_another_deck_through_drive() {
    let drive = FakeDrive::default();
    let layout = scaffold::setup(&drive, ROOT, &mut |_: &str| {}).unwrap();
    let a = Database::open_in_memory().unwrap();
    let b = Database::open_in_memory().unwrap();
    let g = rom(&a, 1, System::Gba);
    rom(&b, 1, System::Gba);
    let mut new_default = defaults::built_in(&gba());
    new_default.insert("r4".into(), Binding::Cleared);
    store::set_default(&a, &gba(), &new_default, 100).unwrap();
    store::set_game(
        &a,
        &gba(),
        &g.id,
        &from_pairs(&[("a", Some("b"))]).unwrap(),
        100,
    )
    .unwrap();
    store::push(&drive, &layout.meta, &a, &gba()).unwrap();

    let updated = store::pull(&drive, &layout.meta, &b).unwrap();
    assert_eq!(updated, [gba()]);
    assert_eq!(
        store::effective(&b, &gba(), &g.id).unwrap(),
        store::effective(&a, &gba(), &g.id).unwrap()
    );
    // Nothing newer: pulling again changes nothing.
    assert!(store::pull(&drive, &layout.meta, &b).unwrap().is_empty());
    // A newer local change isn't overwritten by an older Drive copy.
    store::reset_game(&b, &gba(), &g.id, 200).unwrap();
    assert!(store::pull(&drive, &layout.meta, &b).unwrap().is_empty());
    assert!(store::game_layout(&b, &gba(), &g.id).unwrap().is_empty());
    // It's all in one file under .romshelf/controls/.
    let folder = drive
        .list(&layout.meta)
        .unwrap()
        .into_iter()
        .find(|f| f.name == "controls")
        .unwrap();
    let names: Vec<String> = drive
        .list(&folder.id)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, ["retroarch-gba.json"]);
}

// RetroArch

use romshelf_core::controls::retroarch;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/retroarch")
        .join(name)
}

/// Compares with a golden file; `ROMSHELF_BLESS=1` writes it instead.
fn golden(name: &str, actual: &str) {
    let path = fixture(name);
    if std::env::var("ROMSHELF_BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(actual, expected, "{name} differs");
}

/// Swapped A and B, fast forward on L4 (as the default), and R4 cleared.
fn tidewalker() -> Layout {
    let game = from_pairs(&[("a", Some("b")), ("b", Some("a")), ("r4", None)]).unwrap();
    merge(&defaults::built_in(&gba()), &game)
}

#[test]
fn retroarch_golden_gba_default() {
    golden(
        "gba-default.cfg",
        &retroarch::config(System::Gba, &defaults::built_in(&gba())),
    );
}

#[test]
fn retroarch_golden_gba_with_differences() {
    golden(
        "gba-tidewalker.cfg",
        &retroarch::config(System::Gba, &tidewalker()),
    );
}

#[test]
fn retroarch_golden_n64_default() {
    let n64 = Scope::new("retroarch", System::N64);
    golden(
        "n64-default.cfg",
        &retroarch::config(System::N64, &defaults::built_in(&n64)),
    );
}

#[test]
fn retroarch_settings_are_names_retroarch_itself_writes() {
    let known: Vec<String> = std::fs::read_to_string(fixture("input-keys.txt"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(String::from)
        .collect();
    for line in retroarch::config(System::Gba, &tidewalker())
        .lines()
        .skip(1)
    {
        let key = line.split(" = ").next().unwrap();
        assert!(
            known.iter().any(|k| k == key),
            "{key} isn't a RetroArch setting"
        );
    }
}

#[test]
fn retroarch_binds_each_deck_button_to_its_own_slot_whatever_the_layout() {
    // RetroArch's own autoconfig for the pad Steam gives games: Deck A is button 0, B 1.
    let auto = std::fs::read_to_string(fixture("steam-virtual-gamepad.cfg")).unwrap();
    assert!(auto.contains("input_b_btn = \"0\"") && auto.contains("input_a_btn = \"1\""));
    for layout in [defaults::built_in(&gba()), tidewalker()] {
        let cfg = retroarch::config(System::Gba, &layout);
        assert!(cfg.contains(
            "input_player1_b_btn = \"0\"
"
        ));
        assert!(cfg.contains(
            "input_player1_a_btn = \"1\"
"
        ));
        assert!(cfg.contains(
            "input_player1_up_btn = \"11\"
"
        ));
        assert!(cfg.contains(
            "input_player1_l_y_minus_axis = \"-1\"
"
        ));
        assert!(cfg.contains(
            "input_enable_hotkey_btn = \"nul\"
"
        ));
        assert!(cfg.contains(
            "input_fps_toggle_btn = \"nul\"
"
        ));
        assert!(cfg.contains(
            "input_autodetect_enable = \"false\"
"
        ));
    }
}

/// A remap line's value.
fn remapped(rmp: &str, key: &str) -> String {
    rmp.lines()
        .find_map(|l| l.strip_prefix(&format!("input_player1_{key} = \"")))
        .map(|v| v.trim_end_matches('"').to_string())
        .unwrap_or_else(|| panic!("{key} missing"))
}

#[test]
fn the_remap_sends_each_slot_to_its_game_button() {
    // RetroPad ids: B 0, A 8, Up 4. Deck A sits on the RetroPad's B slot.
    let rmp = retroarch::remap(System::Gba, &defaults::built_in(&gba()));
    assert_eq!(remapped(&rmp, "btn_b"), "8", "Deck A is the GBA's A");
    assert_eq!(remapped(&rmp, "btn_a"), "0", "Deck B is the GBA's B");
    assert_eq!(
        remapped(&rmp, "stk_l_y-"),
        "4",
        "the left stick moves like the D-pad"
    );
    assert_eq!(remapped(&rmp, "btn_y"), "-1", "Deck X does nothing");
    let swapped = retroarch::remap(System::Gba, &tidewalker());
    assert_eq!(remapped(&swapped, "btn_b"), "0");
    assert_eq!(remapped(&swapped, "btn_a"), "8");
    assert!(rmp.contains(
        "input_remap_port_p1 = \"0\"
"
    ));
}

#[test]
fn two_deck_buttons_can_drive_one_game_button() {
    // Your GBA default on the Deck: X also B, Y also A.
    let layout = merge(
        &defaults::built_in(&gba()),
        &from_pairs(&[("x", Some("b")), ("y", Some("a"))]).unwrap(),
    );
    let rmp = retroarch::remap(System::Gba, &layout);
    assert_eq!(remapped(&rmp, "btn_b"), "8", "Deck A: A");
    assert_eq!(remapped(&rmp, "btn_x"), "8", "Deck Y: A too");
    assert_eq!(remapped(&rmp, "btn_a"), "0", "Deck B: B");
    assert_eq!(remapped(&rmp, "btn_y"), "0", "Deck X: B too");
}

#[test]
fn a_grip_on_a_game_button_takes_a_free_slot() {
    let layout = merge(
        &defaults::built_in(&gba()),
        &from_pairs(&[("l4", Some("a"))]).unwrap(),
    );
    let cfg = retroarch::config(System::Gba, &layout);
    let rmp = retroarch::remap(System::Gba, &layout);
    // Deck X is unbound, so its slot (Y) carries L4's key, and X itself is left off it.
    assert!(cfg.contains(
        "input_player1_y = \"f9\"
"
    ));
    assert!(cfg.contains(
        "input_player1_y_btn = \"nul\"
"
    ));
    assert_eq!(remapped(&rmp, "btn_y"), "8");
    // A grip on a hotkey stays a hotkey.
    assert!(cfg.contains(
        "input_save_state = \"f11\"
"
    ));
}

#[test]
fn a_button_on_a_hotkey_drives_no_game_button() {
    let layout = merge(
        &defaults::built_in(&gba()),
        &from_pairs(&[("r3", Some("rewind"))]).unwrap(),
    );
    assert!(retroarch::config(System::Gba, &layout).contains(
        "input_rewind_btn = \"8\"
"
    ));
    assert_eq!(
        remapped(&retroarch::remap(System::Gba, &layout), "btn_r3"),
        "-1"
    );
}

#[test]
fn retroarch_golden_remaps() {
    golden(
        "gba-default.rmp",
        &retroarch::remap(System::Gba, &defaults::built_in(&gba())),
    );
    let n64 = Scope::new("retroarch", System::N64);
    golden(
        "n64-default.rmp",
        &retroarch::remap(System::N64, &defaults::built_in(&n64)),
    );
}

// Dolphin

use romshelf_core::controls::dolphin;

fn gc() -> Scope {
    Scope::new("dolphin", System::Gamecube)
}

fn dolphin_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/dolphin")
        .join(name)
}

fn dolphin_golden(name: &str, actual: &str) {
    let path = dolphin_fixture(name);
    if std::env::var("ROMSHELF_BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(actual, expected, "{name} differs");
}

#[test]
fn dolphin_default_matches_what_dolphin_wrote_for_the_deck() {
    // The settings Dolphin itself saved on the Deck (EmuDeck's layout).
    let theirs = std::fs::read_to_string(dolphin_fixture("GCPadNew.ini")).unwrap();
    let ours = dolphin::gcpad(&defaults::built_in(&gc()));
    for key in [
        "Device",
        "Buttons/A",
        "Buttons/B",
        "Buttons/X",
        "Buttons/Y",
        "Buttons/Start",
        "Main Stick/Up",
        "Main Stick/Right",
        "C-Stick/Up",
        "C-Stick/Left",
        "Triggers/L",
        "Triggers/R-Analog",
        "D-Pad/Up",
        "D-Pad/Right",
        "Main Stick/Calibration",
    ] {
        let find = |text: &str| {
            text.lines()
                .skip_while(|l| *l != "[GCPad1]")
                .find(|l| l.starts_with(&format!("{key} = ")))
                .map(|l| l.replace('`', ""))
        };
        assert_eq!(find(&ours), find(&theirs), "{key}");
    }
}

#[test]
fn dolphin_golden_default_and_hotkeys() {
    let d = defaults::built_in(&gc());
    dolphin_golden("gc-default-pad.ini", &dolphin::gcpad(&d));
    dolphin_golden("gc-default-hotkeys.ini", &dolphin::hotkeys(&d));
}

#[test]
fn dolphin_grips_are_keyboard_keys_and_other_pads_are_kept() {
    let game = from_pairs(&[("l4", Some("a")), ("r4", Some("quit"))]).unwrap();
    let layout = merge(&defaults::built_in(&gc()), &game);
    let existing = std::fs::read_to_string(dolphin_fixture("GCPadNew.ini")).unwrap();
    let file = dolphin::gcpad_file(&existing, &layout);
    assert!(file.contains("Buttons/A = `Button S`|`XInput2/0/Virtual core pointer:F9`\n"));
    assert_eq!(file.matches("[GCPad1]").count(), 1);
    assert!(file.contains("[GCPad2]\nDevice = SDL/1/Steam Virtual Gamepad\n"));
    let hotkeys = dolphin::hotkeys(&layout);
    assert!(hotkeys.contains("General/Exit = `XInput2/0/Virtual core pointer:F10`\n"));
    assert!(hotkeys
        .contains("Save State/Save to Selected Slot = `XInput2/0/Virtual core pointer:F11`\n"));
}

// Setup and launch

use romshelf_core::emulators::setup;
use romshelf_core::emulators::{Dolphin, EmulatorAdapter};
use romshelf_core::paths::Paths;
use romshelf_testkit::{FakeHttp, FakePackageRunner};

#[test]
fn setup_stores_the_built_in_default_for_each_system_and_keeps_an_existing_one() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    let db = Database::open_in_memory().unwrap();
    let snes = Scope::new("retroarch", System::Snes);
    let mut mine = defaults::built_in(&snes);
    mine.insert("l4".into(), Binding::Cleared);
    store::set_default(&db, &snes, &mine, 5).unwrap();
    let runner = FakePackageRunner::simulating(&["org.libretro.RetroArch"]);
    let http = FakeHttp::default();
    for core in romshelf_core::emulators::RetroArch.required_cores() {
        let f = romshelf_core::emulators::RetroArch::core_file(&paths, core);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, b"\x7fELF core").unwrap();
    }
    let ctx = setup::SetupContext {
        runner: &runner,
        http: &http,
        db: &db,
        paths: &paths,
        drive: None,
    };
    setup::run_setup(&ctx, &romshelf_core::emulators::RetroArch, &mut |_| {}).unwrap();
    for system in [System::Gba, System::N64] {
        let scope = Scope::new("retroarch", system);
        assert!(db.control_profile(&scope.key(), None).unwrap().is_some());
        assert_eq!(
            store::default_layout(&db, &scope).unwrap(),
            defaults::built_in(&scope)
        );
    }
    assert_eq!(
        store::default_layout(&db, &snes).unwrap(),
        mine,
        "an existing default is kept"
    );
}

#[test]
fn dolphin_writes_pad_one_and_hotkeys_into_its_own_settings() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    let config = Dolphin::config_dir(&paths);
    std::fs::create_dir_all(&config).unwrap();
    std::fs::copy(dolphin_fixture("GCPadNew.ini"), config.join("GCPadNew.ini")).unwrap();
    let r = Rom {
        id: RomId("gamecube-2260f1ec".into()),
        drive_file_id: "f".into(),
        system: System::Gamecube,
        file_name: "tp.rvz".into(),
        display_name: "Twilight Princess".into(),
        size: 1,
        checksum: None,
        location: LocationState::OnDeck,
        recognized: true,
    };
    Dolphin
        .write_controls(&defaults::built_in(&gc()), &r, &paths)
        .unwrap();
    let pads = std::fs::read_to_string(config.join("GCPadNew.ini")).unwrap();
    assert!(pads.starts_with("[GCPad1]\nDevice = SDL/0/Steam Virtual Gamepad\n"));
    assert!(pads.contains("[GCPad2]"));
    let hotkeys = std::fs::read_to_string(config.join("Hotkeys.ini")).unwrap();
    assert!(hotkeys.contains(
        "Emulation Speed/Disable Emulation Speed Limit = `XInput2/0/Virtual core pointer:F9`"
    ));
}
