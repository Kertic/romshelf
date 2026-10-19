//! Stage 1: sign-in, account and Drive folder as the CLI and interface use them.

use romshelf_core::auth::ClientConfig;
use romshelf_core::drive::{DriveClient, ROOT};
use romshelf_core::settings::LocalSettings;
use romshelf_testkit::connection::{simulate_browser, FakeConnection};
use romshelf_testkit::FAKE_ACCOUNT;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn rig() -> (tempfile::TempDir, FakeConnection) {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeConnection::new(dir.path().join("config/settings.json"));
    (dir, fake)
}

fn login(fake: &FakeConnection) {
    fake.script_login();
    fake.connection
        .login(
            &simulate_browser,
            Duration::from_secs(5),
            &AtomicBool::new(false),
        )
        .unwrap();
}

#[test]
fn first_run_from_nothing_to_ready() {
    let (dir, fake) = rig();
    let c = &fake.connection;
    let before = c.status().unwrap();
    assert!(before.client_configured);
    assert!(!before.signed_in);
    assert!(!before.ready());

    login(&fake);
    let after_login = c.status().unwrap();
    assert!(after_login.signed_in);
    assert_eq!(after_login.account.as_deref(), Some(FAKE_ACCOUNT));
    assert!(!after_login.ready(), "no folder chosen yet");

    let games = fake.drive.create_folder("Games", ROOT).unwrap();
    let mut steps = 0;
    let folder = c
        .setup_folder(&games.id, "My Drive / Games", &mut |_: &str| steps += 1)
        .unwrap();
    assert!(steps > 10);
    assert_eq!(folder.display_path(), "My Drive / Games / Emulation");

    let status = c.status().unwrap();
    assert!(status.ready());
    assert_eq!(
        status.drive_folder.as_deref(),
        Some("My Drive / Games / Emulation")
    );
    let saved = LocalSettings::load(&dir.path().join("config/settings.json")).unwrap();
    assert_eq!(saved.drive.unwrap().layout, folder.layout);
}

#[test]
fn list_folders_shows_only_folders_sorted() {
    let (_dir, fake) = rig();
    fake.drive.create_folder("zelda", ROOT).unwrap();
    fake.drive.create_folder("Archive", ROOT).unwrap();
    fake.drive.create_file("notes.txt", ROOT, b"x").unwrap();
    fake.drive.create_folder("music", ROOT).unwrap();
    let names: Vec<_> = fake
        .connection
        .list_folders(ROOT)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, ["Archive", "music", "zelda"]);
}

#[test]
fn logout_keeps_the_folder_for_the_same_account() {
    let (_dir, fake) = rig();
    login(&fake);
    fake.connection
        .setup_folder(ROOT, "My Drive", &mut |_: &str| {})
        .unwrap();
    fake.http.push(200, "");
    fake.connection.logout().unwrap();
    let out = fake.connection.status().unwrap();
    assert!(!out.signed_in);
    assert_eq!(out.account, None);

    login(&fake);
    assert!(fake.connection.status().unwrap().ready(), "no setup again");
}

#[test]
fn a_different_account_forgets_the_old_folder() {
    let (dir, fake) = rig();
    let path = dir.path().join("config/settings.json");
    login(&fake);
    fake.connection
        .setup_folder(ROOT, "My Drive", &mut |_: &str| {})
        .unwrap();
    let mut s = LocalSettings::load(&path).unwrap();
    s.account = Some("someone-else@example.com".into());
    s.save(&path).unwrap();

    login(&fake);
    let status = fake.connection.status().unwrap();
    assert_eq!(status.account.as_deref(), Some(FAKE_ACCOUNT));
    assert_eq!(status.drive_folder, None);
}

#[test]
fn a_new_client_id_signs_out() {
    let (_dir, fake) = rig();
    login(&fake);
    let same = ClientConfig {
        client_id: "fake.apps.googleusercontent.com".into(),
        client_secret: Some("added".into()),
    };
    fake.connection.set_client(&same).unwrap();
    assert!(fake.connection.status().unwrap().signed_in);

    let other = ClientConfig {
        client_id: "other.apps.googleusercontent.com".into(),
        client_secret: None,
    };
    fake.connection.set_client(&other).unwrap();
    assert!(!fake.connection.status().unwrap().signed_in);
}

#[test]
fn sync_needs_a_folder_then_indexes_it() {
    let (dir, fake) = rig();
    let db = romshelf_core::db::Database::open_in_memory().unwrap();
    let paths = romshelf_core::paths::Paths::under(dir.path());
    login(&fake);
    let err = fake.connection.sync(&db, &paths).unwrap_err();
    assert!(
        err.to_string().contains("Open Romshelf to choose one"),
        "{err}"
    );

    let folder = fake
        .connection
        .setup_folder(ROOT, "My Drive", &mut |_: &str| {})
        .unwrap();
    fake.drive
        .create_file("Kirby.gba", &folder.layout.rom_folders["gba"], b"x")
        .unwrap();
    let report = fake.connection.sync(&db, &paths).unwrap();
    assert_eq!(report.added, 1);
    assert_eq!(db.roms().unwrap()[0].display_name, "Kirby");
}
