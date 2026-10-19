//! Stage 1: the Drive folder tree and `.romshelf/settings.json`, against the fake Drive.

use romshelf_core::drive::scaffold::{self, DriveLayout, DriveSettings, SETTINGS_FILE};
use romshelf_core::drive::{DriveClient, DriveFile, ROOT};
use romshelf_core::model::System;
use romshelf_core::settings::{DriveFolder, LocalSettings};
use romshelf_core::Error;
use romshelf_testkit::FakeDrive;
use std::collections::BTreeMap;

/// Every folder as "parent path/name" → count, built by walking the fake Drive.
fn folder_counts(drive: &FakeDrive) -> BTreeMap<String, usize> {
    let files = drive.all_files();
    let by_id: BTreeMap<&str, &DriveFile> = files.iter().map(|f| (f.id.as_str(), f)).collect();
    let path = |f: &DriveFile| {
        let mut parts = vec![f.name.clone()];
        let mut parent = f.parents.first().cloned();
        while let Some(p) = parent {
            match by_id.get(p.as_str()) {
                Some(pf) => {
                    parts.push(pf.name.clone());
                    parent = pf.parents.first().cloned();
                }
                None => break,
            }
        }
        parts.reverse();
        parts.join("/")
    };
    let mut counts = BTreeMap::new();
    for f in files.iter().filter(|f| f.is_folder() && !f.trashed) {
        *counts.entry(path(f)).or_insert(0) += 1;
    }
    counts
}

fn expected_folders(prefix: &str) -> Vec<String> {
    let mut out: Vec<String> = ["", "/roms", "/saves", "/states", "/bios", "/.romshelf"]
        .iter()
        .map(|s| format!("{prefix}Emulation{s}"))
        .collect();
    for sys in System::ALL {
        out.push(format!("{prefix}Emulation/roms/{}", sys.id()));
        out.push(format!("{prefix}Emulation/bios/{}", sys.id()));
    }
    out.sort();
    out
}

fn no_progress(_: &str) {}

#[test]
fn empty_drive_gets_every_folder_exactly_once() {
    let drive = FakeDrive::default();
    let layout = scaffold::scaffold(&drive, ROOT, &mut no_progress).unwrap();
    let counts = folder_counts(&drive);
    assert_eq!(
        counts.keys().cloned().collect::<Vec<_>>(),
        expected_folders("")
    );
    assert!(counts.values().all(|&n| n == 1), "{counts:?}");
    assert_eq!(layout.rom_folders.len(), System::ALL.len());
    assert_eq!(layout.bios_folders.len(), System::ALL.len());
    assert_eq!(drive.get(&layout.emulation).unwrap().parents, vec![ROOT]);
    assert_eq!(
        drive.get(&layout.rom_folders["gba"]).unwrap().parents,
        vec![layout.roms.clone()]
    );
}

#[test]
fn scaffold_twice_creates_nothing_new() {
    let drive = FakeDrive::default();
    let first = scaffold::scaffold(&drive, ROOT, &mut no_progress).unwrap();
    let before = drive.all_files().len();
    let second = scaffold::scaffold(&drive, ROOT, &mut no_progress).unwrap();
    assert_eq!(first, second);
    assert_eq!(drive.all_files().len(), before);
}

#[test]
fn partial_tree_is_completed_not_duplicated() {
    let drive = FakeDrive::default();
    let emu = drive.create_folder("Emulation", ROOT).unwrap();
    let roms = drive.create_folder("roms", &emu.id).unwrap();
    let snes = drive.create_folder("snes", &roms.id).unwrap();
    let layout = scaffold::scaffold(&drive, ROOT, &mut no_progress).unwrap();
    assert_eq!(layout.emulation, emu.id);
    assert_eq!(layout.roms, roms.id);
    assert_eq!(layout.rom_folders["snes"], snes.id);
    let counts = folder_counts(&drive);
    assert_eq!(
        counts.keys().cloned().collect::<Vec<_>>(),
        expected_folders("")
    );
    assert!(counts.values().all(|&n| n == 1), "{counts:?}");
}

#[test]
fn same_named_folders_elsewhere_are_ignored() {
    let drive = FakeDrive::default();
    let other = drive.create_folder("Other", ROOT).unwrap();
    let stray = drive.create_folder("Emulation", &other.id).unwrap();
    drive.create_folder("roms", ROOT).unwrap();
    drive.create_folder("roms", &stray.id).unwrap();
    let layout = scaffold::scaffold(&drive, ROOT, &mut no_progress).unwrap();
    assert_ne!(layout.emulation, stray.id);
    assert_eq!(drive.get(&layout.emulation).unwrap().parents, vec![ROOT]);
    assert!(
        drive.list(&stray.id).unwrap().len() == 1,
        "stray tree untouched"
    );
}

#[test]
fn a_file_with_a_folder_name_is_not_mistaken_for_the_folder() {
    let drive = FakeDrive::default();
    let file = drive.create_file("Emulation", ROOT, b"x").unwrap();
    let layout = scaffold::scaffold(&drive, ROOT, &mut no_progress).unwrap();
    assert_ne!(layout.emulation, file.id);
    assert!(drive.get(&layout.emulation).unwrap().is_folder());
}

#[test]
fn scaffold_under_a_chosen_folder() {
    let drive = FakeDrive::default();
    let games = drive.create_folder("Games", ROOT).unwrap();
    let layout = scaffold::scaffold(&drive, &games.id, &mut no_progress).unwrap();
    assert_eq!(
        drive.get(&layout.emulation).unwrap().parents,
        vec![games.id]
    );
    assert_eq!(folder_counts(&drive).keys().cloned().collect::<Vec<_>>(), {
        let mut v = expected_folders("Games/");
        v.push("Games".into());
        v.sort();
        v
    });
}

#[test]
fn progress_names_every_folder() {
    let drive = FakeDrive::default();
    let mut seen = Vec::new();
    scaffold::scaffold(&drive, ROOT, &mut |p: &str| seen.push(p.to_string())).unwrap();
    seen.sort();
    assert_eq!(seen, expected_folders(""));
}

#[test]
fn setup_writes_settings_that_round_trip() {
    let drive = FakeDrive::default();
    let layout = scaffold::setup(&drive, ROOT, &mut no_progress).unwrap();
    let read = scaffold::read_settings(&drive, &layout.meta)
        .unwrap()
        .expect("settings.json written");
    assert_eq!(read.schema_version, scaffold::SETTINGS_SCHEMA);
    assert_eq!(read.folders, layout);

    // Running setup again updates the same file rather than adding a second one.
    scaffold::setup(&drive, ROOT, &mut no_progress).unwrap();
    let settings_files: Vec<_> = drive
        .list(&layout.meta)
        .unwrap()
        .into_iter()
        .filter(|f| f.name == SETTINGS_FILE)
        .collect();
    assert_eq!(settings_files.len(), 1);
}

#[test]
fn newer_drive_settings_ask_for_an_update_and_are_not_overwritten() {
    let drive = FakeDrive::default();
    let layout = scaffold::scaffold(&drive, ROOT, &mut no_progress).unwrap();
    let newer = br#"{"schema_version": 99, "folders": {}, "something_new": true}"#;
    let file = drive
        .create_file(SETTINGS_FILE, &layout.meta, newer)
        .unwrap();

    let err = scaffold::read_settings(&drive, &layout.meta).unwrap_err();
    assert!(matches!(err, Error::NeedsUpdate(_)), "{err:?}");
    assert!(err.to_string().contains("Please update Romshelf"));

    assert!(scaffold::setup(&drive, ROOT, &mut no_progress).is_err());
    assert_eq!(drive.download(&file.id, 0).unwrap(), newer);
}

#[test]
fn damaged_settings_are_an_error_not_a_crash() {
    assert!(scaffold::parse_settings(b"{not json").is_err());
    assert!(scaffold::parse_settings(b"{}").is_err());
    let ok = DriveSettings {
        schema_version: scaffold::SETTINGS_SCHEMA,
        folders: DriveLayout::default(),
    };
    let bytes = serde_json::to_vec(&ok).unwrap();
    assert_eq!(scaffold::parse_settings(&bytes).unwrap(), ok);
}

#[test]
fn local_settings_round_trip_and_reject_newer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config/settings.json");
    assert_eq!(
        LocalSettings::load(&path).unwrap(),
        LocalSettings::default()
    );

    let drive = FakeDrive::default();
    let layout = scaffold::scaffold(&drive, ROOT, &mut no_progress).unwrap();
    let settings = LocalSettings {
        account: Some("player@example.com".into()),
        drive: Some(DriveFolder {
            location: "My Drive".into(),
            layout,
        }),
        ..LocalSettings::default()
    };
    settings.save(&path).unwrap();
    assert_eq!(LocalSettings::load(&path).unwrap(), settings);

    std::fs::write(&path, br#"{"schema_version": 42}"#).unwrap();
    assert!(matches!(
        LocalSettings::load(&path).unwrap_err(),
        Error::NeedsUpdate(_)
    ));
}
