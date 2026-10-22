//! Stage 8: Steam shortcuts, artwork and controller layouts.

use romshelf_core::db::Database;
use romshelf_core::model::{ArtSlot, LocationState, Rom, RomId, System};
use romshelf_core::paths::Paths;
use romshelf_core::steam::apply::{self, SteamContext, AUTO_ADD_SETTING};
use romshelf_core::steam::shortcuts::{self, Desired, TAG};
use romshelf_core::steam::vdf::{self, Map, Value};
use romshelf_core::steam::{artwork, input, SteamFiles};
use romshelf_testkit::FakeSteam;
use std::path::PathBuf;

const USER: &str = "85959576";

fn fixture() -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/steam/shortcuts.vdf"))
        .unwrap()
}

/// The sanitised `shortcuts.vdf` from the Deck (67 shortcuts, names and paths replaced).
fn entries(bytes: &[u8]) -> Vec<Map> {
    let root = vdf::parse(bytes).unwrap();
    let Some(Value::Map(list)) = vdf::get(&root, "shortcuts") else {
        panic!("no shortcuts")
    };
    list.iter()
        .map(|(_, v)| match v {
            Value::Map(m) => m.clone(),
            _ => panic!("not an entry"),
        })
        .collect()
}

fn desired(rom_id: &str, name: &str) -> Desired {
    Desired {
        app_id: shortcuts::app_id_for(rom_id),
        name: name.into(),
        exe: "/home/deck/Applications/romshelf.AppImage".into(),
        start_dir: "/home/deck/Applications/".into(),
        launch_options: format!("launch --id {rom_id}"),
        icon: String::new(),
    }
}

// The file format

#[test]
fn the_decks_file_reads_and_writes_back_byte_for_byte() {
    let bytes = fixture();
    let root = vdf::parse(&bytes).unwrap();
    assert_eq!(vdf::write(&root), bytes);
    let list = entries(&bytes);
    assert_eq!(list.len(), 67);
    // Newer fields than the documented ones are kept.
    assert!(vdf::get(&list[0], "FlatpakAppID").is_some());
    assert!(vdf::get(&list[0], "sortas").is_some());
}

#[test]
fn adding_one_leaves_every_other_entry_as_it_was() {
    let before = fixture();
    let d = desired("gba-8f031392", "Kirby");
    let out = shortcuts::reconcile(Some(&before), std::slice::from_ref(&d), &[d.app_id]).unwrap();
    assert_eq!(out.added, [d.app_id]);
    let after = entries(out.bytes.as_ref().unwrap());
    assert_eq!(&after[..67], &entries(&before)[..]);
    let new = &after[67];
    assert_eq!(vdf::int_of(new, "appid"), Some(d.app_id));
    assert_eq!(vdf::str_of(new, "AppName").as_deref(), Some("Kirby"));
    assert_eq!(
        vdf::str_of(new, "Exe").as_deref(),
        Some("\"/home/deck/Applications/romshelf.AppImage\"")
    );
    assert_eq!(
        vdf::str_of(new, "StartDir").as_deref(),
        Some("\"/home/deck/Applications/\"")
    );
    assert_eq!(
        vdf::str_of(new, "LaunchOptions").as_deref(),
        Some("launch --id gba-8f031392")
    );
    // Steam's field order, as in its own entries.
    let keys = |m: &Map| m.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();
    assert_eq!(keys(new), keys(&after[0]));
    let shortcuts::Found { name, .. } = &shortcuts::list(out.bytes.as_deref()).unwrap()[0];
    assert_eq!(name, "Kirby");
}

#[test]
fn renaming_then_removing_touches_only_romshelfs_entry() {
    let original = entries(&fixture());
    let mut d = desired("gba-8f031392", "Kirby");
    let ours = [d.app_id];
    let added = shortcuts::reconcile(Some(&fixture()), std::slice::from_ref(&d), &ours).unwrap();
    d.name = "Kirby: Nightmare in Dream Land".into();
    let renamed =
        shortcuts::reconcile(added.bytes.as_deref(), std::slice::from_ref(&d), &ours).unwrap();
    assert_eq!(renamed.updated, [d.app_id]);
    let list = entries(renamed.bytes.as_ref().unwrap());
    assert_eq!(&list[..67], &original[..]);
    assert_eq!(
        vdf::str_of(&list[67], "AppName").as_deref(),
        Some(d.name.as_str())
    );
    let removed = shortcuts::reconcile(renamed.bytes.as_deref(), &[], &ours).unwrap();
    assert_eq!(removed.removed, [d.app_id]);
    assert_eq!(removed.bytes.unwrap(), fixture());
}

#[test]
fn an_entry_without_the_tag_is_never_touched() {
    // Someone else's shortcut with the same name, and even Romshelf's app ID, but no tag.
    let d = desired("gba-8f031392", "Game 1");
    let mut root = vdf::parse(&fixture()).unwrap();
    if let Some((_, Value::Map(list))) = root.iter_mut().find(|(k, _)| k == b"shortcuts") {
        if let Some((_, Value::Map(first))) = list.first_mut() {
            vdf::set(first, "appid", Value::Int(d.app_id));
        }
    }
    let theirs = vdf::write(&root);
    let out = shortcuts::reconcile(Some(&theirs), &[], &[d.app_id]).unwrap();
    assert_eq!(out.bytes, None, "nothing to remove: it isn't Romshelf's");
    let out = shortcuts::reconcile(Some(&theirs), std::slice::from_ref(&d), &[d.app_id]).unwrap();
    let list = entries(out.bytes.as_ref().unwrap());
    assert_eq!(list[0], entries(&theirs)[0], "left exactly as it was");
    assert_eq!(list.len(), 68, "Romshelf added its own beside it");
}

#[test]
fn reconciling_twice_writes_nothing_the_second_time() {
    let d = desired("gba-8f031392", "Kirby");
    let first =
        shortcuts::reconcile(Some(&fixture()), std::slice::from_ref(&d), &[d.app_id]).unwrap();
    let second = shortcuts::reconcile(
        first.bytes.as_deref(),
        std::slice::from_ref(&d),
        &[d.app_id],
    )
    .unwrap();
    assert_eq!(second, shortcuts::Reconciled::default());
}

#[test]
fn a_missing_file_starts_a_new_one() {
    let d = desired("gba-8f031392", "Kirby");
    let out = shortcuts::reconcile(None, std::slice::from_ref(&d), &[d.app_id]).unwrap();
    let list = entries(out.bytes.as_ref().unwrap());
    assert_eq!(list.len(), 1);
    assert_eq!(out.bytes.unwrap()[..11], *b"\x00shortcuts\x00");
}

#[test]
fn names_with_quotes_and_accents_keep_the_launch_options_intact() {
    let d = desired("snes-00000001", "\"Pokémon\" — Edición «Ñ»");
    let out = shortcuts::reconcile(None, std::slice::from_ref(&d), &[d.app_id]).unwrap();
    let list = entries(out.bytes.as_ref().unwrap());
    assert_eq!(
        vdf::str_of(&list[0], "AppName").as_deref(),
        Some(d.name.as_str())
    );
    assert_eq!(
        vdf::str_of(&list[0], "LaunchOptions").as_deref(),
        Some("launch --id snes-00000001")
    );
}

#[test]
fn app_ids_are_stable_and_two_games_with_one_name_differ() {
    let a = shortcuts::app_id_for("gba-00000001");
    assert_eq!(a, shortcuts::app_id_for("gba-00000001"));
    assert_ne!(a, shortcuts::app_id_for("gba-00000002"));
}

// Applying: shortcuts, artwork and layouts together

struct Rig {
    steam: FakeSteam,
    _dir: tempfile::TempDir,
    db: Database,
    paths: Paths,
}

fn template() -> String {
    "\"controller_mappings\"\r\n{\r\n\t\"localization\"\r\n\t{\r\n\t\t\"english\"\r\n\t\t{\r\n\t\t\t\"Title\" \"Gamepad\"\r\n\t\t}\r\n\t}\r\n\t\t\t\"button_back_left_upper\"\r\n\t\t\t{\r\n\t\t\t\t\"activators\"\r\n\t\t\t\t{\r\n\t\t\t\t\t\"Full_Press\"\r\n\t\t\t\t\t{\r\n\t\t\t\t\t\t\"bindings\"\r\n\t\t\t\t\t\t{\r\n\t\t\t\t\t\t}\r\n\t\t\t\t\t}\r\n\t\t\t\t}\r\n\t\t\t}\r\n}\r\n".into()
}

impl Rig {
    fn new() -> Rig {
        let seed = tempfile::tempdir().unwrap();
        let cfg = seed.path().join(format!("userdata/{USER}/config"));
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("shortcuts.vdf"), fixture()).unwrap();
        std::fs::create_dir_all(seed.path().join("controller_base/templates")).unwrap();
        std::fs::write(
            seed.path()
                .join("controller_base/templates/controller_neptune_gamepad+mouse.vdf"),
            template(),
        )
        .unwrap();
        let steam = FakeSteam::new(Some(seed.path())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        Rig {
            steam,
            paths: Paths::under(dir.path()),
            _dir: dir,
            db: Database::open_in_memory().unwrap(),
        }
    }

    fn ctx(&self, now: i64) -> SteamContext<'_> {
        SteamContext {
            db: &self.db,
            files: &self.steam.files,
            paths: &self.paths,
            now,
        }
    }

    fn game(&self, n: u8, name: &str) -> Rom {
        let r = Rom {
            id: RomId(format!("gba-0000000{n}")),
            drive_file_id: format!("f{n}"),
            system: System::Gba,
            file_name: format!("{name}.gba"),
            display_name: name.into(),
            size: 1,
            checksum: None,
            location: LocationState::OnDeck,
            recognized: true,
        };
        self.db.upsert_rom(&r).unwrap();
        r
    }

    fn grid(&self) -> Vec<String> {
        let mut g = self.steam.files.grid_files(USER).unwrap();
        g.sort();
        g
    }

    fn entries(&self) -> Vec<Map> {
        entries(&self.steam.files.read_shortcuts(USER).unwrap().unwrap())
    }
}

fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(&bytes[1..4], b"PNG");
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (w, h)
}

#[test]
fn a_shown_game_gets_a_shortcut_artwork_and_a_layout_and_a_restart_notice() {
    let r = Rig::new();
    let kirby = r.game(1, "Kirby");
    apply::set_shown(&r.db, &kirby, true).unwrap();
    let out = apply::apply(&r.ctx(1_800_000_000)).unwrap();
    assert_eq!((out.added, out.updated, out.removed), (1, 0, 0));
    assert!(out.restart_needed);
    let id = shortcuts::app_id_for("gba-00000001");
    let list = r.entries();
    assert_eq!(list.len(), 68);
    let new = &list[67];
    assert_eq!(
        vdf::str_of(new, "Exe").unwrap(),
        format!("\"{}\"", r.paths.appimage.display())
    );
    assert!(vdf::str_of(new, "icon")
        .unwrap()
        .ends_with(&format!("{id}_icon.png")));
    assert_eq!(
        r.grid(),
        [
            format!("{id}.png"),
            format!("{id}_hero.png"),
            format!("{id}_icon.png"),
            format!("{id}p.png"),
        ]
    );
    // Generated artwork at Steam's sizes.
    for (slot, size) in [
        (ArtSlot::Capsule, (600, 900)),
        (ArtSlot::WideCapsule, (920, 430)),
        (ArtSlot::Hero, (1920, 620)),
        (ArtSlot::Icon, (256, 256)),
    ] {
        let name = format!("{}.png", artwork::stem(slot, id));
        assert_eq!(
            png_size(&r.steam.files.read_grid(USER, &name).unwrap().unwrap()),
            size
        );
    }
    // The grips send keys in this shortcut, filed under its name.
    let set = r.steam.files.read_controller_set(USER).unwrap().unwrap();
    assert!(set.contains("\t\"kirby\"\n\t{\n\t\t\"autosave\"\t\t\"1\"\n\t}\n"));
    let layout = std::fs::read_to_string(r.steam.dir.path().join(format!(
        "steamapps/common/Steam Controller Configs/{USER}/config/kirby/controller_neptune.vdf"
    )))
    .unwrap();
    assert!(layout.contains("key_press F9"));
    assert!(apply::take_restart_notice(&r.db).unwrap());
    assert!(
        !apply::take_restart_notice(&r.db).unwrap(),
        "once per batch"
    );
}

#[test]
fn applying_again_changes_nothing() {
    let r = Rig::new();
    apply::set_shown(&r.db, &r.game(1, "Kirby"), true).unwrap();
    apply::apply(&r.ctx(1)).unwrap();
    let file = r.steam.files.read_shortcuts(USER).unwrap();
    apply::take_restart_notice(&r.db).unwrap();
    let again = apply::apply(&r.ctx(2)).unwrap();
    assert_eq!(again, apply::Applied::default());
    assert_eq!(r.steam.files.read_shortcuts(USER).unwrap(), file);
    assert!(!apply::take_restart_notice(&r.db).unwrap());
    // No second backup either.
    let backups = std::fs::read_dir(r.steam.dir.path().join(format!("userdata/{USER}/config")))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".bak")
        })
        .count();
    assert_eq!(backups, 1);
}

#[test]
fn chosen_artwork_replaces_the_generated_file_and_removal_clears_everything() {
    let r = Rig::new();
    let kirby = r.game(1, "Kirby");
    apply::set_shown(&r.db, &kirby, true).unwrap();
    apply::apply(&r.ctx(1)).unwrap();
    let id = shortcuts::app_id_for("gba-00000001");
    let pics = tempfile::tempdir().unwrap();
    std::fs::write(pics.path().join("cover.jpg"), b"jpeg bytes").unwrap();
    std::fs::write(pics.path().join("logo.png"), b"logo bytes").unwrap();
    apply::set_artwork(
        &r.db,
        &r.paths,
        &kirby,
        ArtSlot::Capsule,
        Some(&pics.path().join("cover.jpg")),
    )
    .unwrap();
    apply::set_artwork(
        &r.db,
        &r.paths,
        &kirby,
        ArtSlot::Logo,
        Some(&pics.path().join("logo.png")),
    )
    .unwrap();
    // The original can go; Romshelf has its own copy.
    std::fs::remove_file(pics.path().join("cover.jpg")).unwrap();
    apply::apply(&r.ctx(2)).unwrap();
    let grid = r.grid();
    assert!(grid.contains(&format!("{id}p.jpg")));
    assert!(
        !grid.contains(&format!("{id}p.png")),
        "the old one of another type is gone"
    );
    assert!(grid.contains(&format!("{id}_logo.png")));
    assert_eq!(
        r.steam
            .files
            .read_grid(USER, &format!("{id}p.jpg"))
            .unwrap()
            .unwrap(),
        b"jpeg bytes"
    );
    // Back to generated.
    apply::set_artwork(&r.db, &r.paths, &kirby, ArtSlot::Capsule, None).unwrap();
    apply::apply(&r.ctx(3)).unwrap();
    assert!(r.grid().contains(&format!("{id}p.png")));
    assert!(!r.grid().contains(&format!("{id}p.jpg")));
    // Out of Steam: entry, all five artwork files and the layout entry go.
    apply::set_shown(&r.db, &kirby, false).unwrap();
    let out = apply::apply(&r.ctx(4)).unwrap();
    assert_eq!(out.removed, 1);
    assert!(r.grid().is_empty());
    assert_eq!(r.entries().len(), 67);
    let set = r.steam.files.read_controller_set(USER).unwrap().unwrap();
    assert!(!set.contains("\"kirby\""));
}

#[test]
fn renaming_keeps_the_app_id_and_moves_the_layout() {
    let r = Rig::new();
    let kirby = r.game(1, "Kirby");
    apply::set_shown(&r.db, &kirby, true).unwrap();
    apply::apply(&r.ctx(1)).unwrap();
    apply::rename(&r.db, &kirby, "Kirby's Adventure").unwrap();
    let out = apply::apply(&r.ctx(2)).unwrap();
    assert_eq!(out.updated, 1);
    let list = r.entries();
    assert_eq!(
        vdf::int_of(&list[67], "appid"),
        Some(shortcuts::app_id_for("gba-00000001"))
    );
    let set = r.steam.files.read_controller_set(USER).unwrap().unwrap();
    assert!(set.contains("\"kirby's adventure\""));
    assert!(!set.contains("\t\"kirby\"\n"));
}

#[test]
fn a_game_gone_from_drive_loses_its_shortcut() {
    let r = Rig::new();
    let mut kirby = r.game(1, "Kirby");
    apply::set_shown(&r.db, &kirby, true).unwrap();
    apply::apply(&r.ctx(1)).unwrap();
    kirby.location = LocationState::Gone;
    r.db.upsert_rom(&kirby).unwrap();
    assert_eq!(apply::apply(&r.ctx(2)).unwrap().removed, 1);
    assert_eq!(r.entries().len(), 67);
}

#[test]
fn adding_every_new_rom_leaves_out_ones_the_user_took_out() {
    let r = Rig::new();
    let kirby = r.game(1, "Kirby");
    let other = r.game(2, "Tidewalker");
    assert!(apply::auto_add(&r.db).unwrap().is_empty(), "off by default");
    r.db.set_setting(AUTO_ADD_SETTING, "1").unwrap();
    apply::set_shown(&r.db, &kirby, false).unwrap();
    assert_eq!(
        apply::auto_add(&r.db).unwrap(),
        std::slice::from_ref(&other.id)
    );
    assert!(apply::auto_add(&r.db).unwrap().is_empty());
    assert!(r.db.shortcut(&kirby.id).unwrap().is_none());
}

#[test]
fn entries_from_another_tool_with_the_tag_but_unknown_ids_are_left_alone() {
    let r = Rig::new();
    // Tagged "Romshelf" by someone else, with an ID Romshelf never derives.
    let stranger = desired("not-a-romshelf-game", "Stranger");
    let bytes = shortcuts::reconcile(
        r.steam.files.read_shortcuts(USER).unwrap().as_deref(),
        std::slice::from_ref(&stranger),
        &[stranger.app_id],
    )
    .unwrap()
    .bytes
    .unwrap();
    r.steam.files.write_shortcuts(USER, &bytes, 0).unwrap();
    r.game(1, "Kirby");
    apply::apply(&r.ctx(1)).unwrap();
    let names: Vec<String> = r
        .entries()
        .iter()
        .filter_map(|e| vdf::str_of(e, "AppName"))
        .collect();
    assert!(names.contains(&"Stranger".to_string()));
    let _ = TAG;
}

#[test]
fn the_layout_starts_from_steams_template() {
    let out = input::layout_from_template(&template());
    assert!(out.contains("\"Title\" \"Romshelf\""));
    assert!(out.contains("key_press F9"));
}

#[test]
fn romshelf_itself_can_be_in_steam_and_a_moved_install_is_followed() {
    let mut r = Rig::new();
    r.db.set_setting(apply::SELF_SETTING, "1").unwrap();
    assert_eq!(apply::apply(&r.ctx(1)).unwrap().added, 1);
    let list = r.entries();
    let me = list
        .iter()
        .find(|e| vdf::int_of(e, "appid") == Some(apply::self_app_id()))
        .unwrap();
    assert_eq!(vdf::str_of(me, "AppName").as_deref(), Some("Romshelf"));
    assert_eq!(vdf::str_of(me, "LaunchOptions").as_deref(), Some(""));
    // If the install path ever changes, every Romshelf entry is rewritten to it.
    apply::set_shown(&r.db, &r.game(1, "Kirby"), true).unwrap();
    apply::apply(&r.ctx(2)).unwrap();
    r.paths.appimage = r.paths.appimage.with_file_name("romshelf-new.AppImage");
    let moved = apply::apply(&r.ctx(3)).unwrap();
    assert_eq!(moved.updated, 2);
    let want = format!("\"{}\"", r.paths.appimage.display());
    for e in r.entries().iter().filter(|e| {
        let id = vdf::int_of(e, "appid");
        id == Some(apply::self_app_id()) || id == Some(shortcuts::app_id_for("gba-00000001"))
    }) {
        assert_eq!(vdf::str_of(e, "Exe").unwrap(), want);
    }
    // Taken out again.
    r.db.set_setting(apply::SELF_SETTING, "0").unwrap();
    assert_eq!(apply::apply(&r.ctx(4)).unwrap().removed, 1);
}
