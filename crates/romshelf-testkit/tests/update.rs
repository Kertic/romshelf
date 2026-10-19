//! Self-update: release feed, signed manifest, in-place replace, rollback.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use romshelf_core::update::{Outcome, Updater, Version, CHECK_INTERVAL};
use romshelf_core::Error;
use romshelf_testkit::{FakeClock, FakeHttp};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const NOW: i64 = 1_800_000_000;
const FEED: &str = "https://api.github.com/repos/o/r/releases/latest";
const OLD: &[u8] = b"\x7fELF old build";

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

struct Rig {
    _dir: tempfile::TempDir,
    http: Arc<FakeHttp>,
    clock: Arc<FakeClock>,
    install: PathBuf,
    updater: Updater,
}

fn rig(current: &str) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let install = dir.path().join("Applications/romshelf.AppImage");
    std::fs::create_dir_all(install.parent().unwrap()).unwrap();
    std::fs::write(&install, OLD).unwrap();
    let http = Arc::new(FakeHttp::default());
    let clock = Arc::new(FakeClock::at(NOW));
    let updater = Updater {
        http: http.clone(),
        clock: clock.clone(),
        feed_url: FEED.into(),
        public_key: key().verifying_key().to_bytes(),
        token: None,
        current: Version::parse(current).unwrap(),
        install: install.clone(),
        state_file: dir.path().join("data/update.json"),
        launch_lock: dir.path().join("data/launch.lock"),
    };
    Rig {
        _dir: dir,
        http,
        clock,
        install,
        updater,
    }
}

fn feed(tag: &str) -> String {
    let asset = |id: u32, name: &str| {
        format!(
            r#"{{"id":{id},"name":"{name}","url":"https://api.github.com/repos/o/r/releases/assets/{id}","browser_download_url":"https://github.com/o/r/releases/download/{tag}/{name}"}}"#
        )
    };
    format!(
        r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"assets":[{},{},{}]}}"#,
        asset(1, "romshelf.AppImage"),
        asset(2, "romshelf-update.json"),
        asset(3, "romshelf-update.json.sig")
    )
}

fn manifest(version: &str, file: &[u8]) -> String {
    format!(
        r#"{{"version":"{version}","sha256":"{}","size":{}}}"#,
        hex(&Sha256::digest(file)),
        file.len()
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sign(text: &str) -> String {
    STANDARD.encode(key().sign(text.as_bytes()).to_bytes())
}

/// Scripts a complete release: feed, manifest, signature, AppImage.
fn script_release(http: &FakeHttp, tag: &str, manifest_text: &str, sig: &str, file: &[u8]) {
    http.push(200, &feed(tag));
    http.push(200, manifest_text);
    http.push(200, sig);
    http.push(200, std::str::from_utf8(file).unwrap());
}

fn previous(install: &Path) -> PathBuf {
    install.with_file_name("romshelf.AppImage.previous")
}

fn untouched(r: &Rig) {
    assert_eq!(std::fs::read(&r.install).unwrap(), OLD);
    assert!(!previous(&r.install).exists());
}

#[test]
fn versions_parse_and_order() {
    assert_eq!(Version::parse("v0.2.10").unwrap(), Version(0, 2, 10));
    assert_eq!(Version::parse("1.0.0").unwrap().to_string(), "1.0.0");
    assert!(Version::parse("0.10.0").unwrap() > Version::parse("0.9.9").unwrap());
    for bad in ["", "1.2", "v1.2.x", "1.2.3.4", "latest"] {
        assert!(Version::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_newer_release_replaces_the_file_and_keeps_the_previous() {
    let r = rig("0.1.0");
    let new = b"\x7fELF new build";
    let m = manifest("0.2.0", new);
    script_release(&r.http, "v0.2.0", &m, &sign(&m), new);

    assert_eq!(
        r.updater.run(false).unwrap(),
        Outcome::Updated(Version(0, 2, 0))
    );
    assert_eq!(std::fs::read(&r.install).unwrap(), new);
    assert_eq!(std::fs::read(previous(&r.install)).unwrap(), OLD);
    assert!(!r
        .install
        .with_file_name("romshelf.AppImage.download")
        .exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&r.install).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "executable");
    }
    // Asset downloads ask GitHub for the file itself.
    let reqs = r.http.requests();
    assert_eq!(reqs[0].url, FEED);
    assert!(reqs[1..]
        .iter()
        .all(|q| q.header_value("Accept") == Some("application/octet-stream")));
    assert_eq!(
        reqs[3].url,
        "https://api.github.com/repos/o/r/releases/assets/1"
    );
}

#[test]
fn same_or_older_versions_are_ignored() {
    for tag in ["v0.2.0", "v0.1.9"] {
        let r = rig("0.2.0");
        r.http.push(200, &feed(tag));
        assert_eq!(r.updater.run(false).unwrap(), Outcome::UpToDate);
        assert_eq!(r.http.requests().len(), 1, "only the feed");
        untouched(&r);
    }
}

#[test]
fn a_bad_checksum_is_rejected_and_nothing_changes() {
    let r = rig("0.1.0");
    let m = manifest("0.2.0", b"\x7fELF what was signed");
    script_release(&r.http, "v0.2.0", &m, &sign(&m), b"\x7fELF what arrived");
    let err = r.updater.run(false).unwrap_err();
    assert!(err.to_string().contains("checksum"), "{err}");
    untouched(&r);
}

#[test]
fn a_bad_signature_is_rejected_before_downloading_the_file() {
    let r = rig("0.1.0");
    let new = b"\x7fELF new build";
    let m = manifest("0.2.0", new);
    let forged = STANDARD.encode(
        SigningKey::from_bytes(&[9u8; 32])
            .sign(m.as_bytes())
            .to_bytes(),
    );
    script_release(&r.http, "v0.2.0", &m, &forged, new);
    let err = r.updater.run(false).unwrap_err();
    assert!(err.to_string().contains("signature"), "{err}");
    assert_eq!(r.http.requests().len(), 3, "AppImage never fetched");
    untouched(&r);
}

#[test]
fn a_manifest_for_another_version_is_rejected() {
    let r = rig("0.1.0");
    let new = b"\x7fELF new build";
    // A validly signed manifest from an older release, replayed under a newer tag.
    let m = manifest("0.1.0", new);
    script_release(&r.http, "v0.2.0", &m, &sign(&m), new);
    assert!(r.updater.run(false).is_err());
    untouched(&r);
}

#[test]
fn something_that_is_not_an_appimage_is_rejected() {
    let r = rig("0.1.0");
    let html = b"<html>rate limited</html>";
    let m = manifest("0.2.0", html);
    script_release(&r.http, "v0.2.0", &m, &sign(&m), html);
    assert!(r.updater.run(false).is_err());
    untouched(&r);
}

#[test]
fn an_interrupted_download_leaves_the_current_version() {
    let r = rig("0.1.0");
    let m = manifest("0.2.0", b"\x7fELF new");
    r.http.push(200, &feed("v0.2.0"));
    r.http.push(200, &m);
    r.http.push(200, &sign(&m));
    r.http.push_network_error("connection reset by peer");
    let err = r.updater.run(false).unwrap_err();
    assert!(matches!(err, Error::Network(_)), "{err:?}");
    untouched(&r);
    // A failed check is retried at the next start, not a day later.
    r.http.push(200, &feed("v0.1.0"));
    assert_eq!(r.updater.run(false).unwrap(), Outcome::UpToDate);
}

#[test]
fn checks_at_most_once_a_day_unless_forced() {
    let r = rig("0.2.0");
    r.http.push(200, &feed("v0.2.0"));
    assert_eq!(r.updater.run(false).unwrap(), Outcome::UpToDate);
    r.clock.advance(CHECK_INTERVAL - 60);
    assert_eq!(r.updater.run(false).unwrap(), Outcome::NotDue);
    assert_eq!(r.http.requests().len(), 1);

    r.http.push(200, &feed("v0.2.0"));
    assert_eq!(r.updater.run(true).unwrap(), Outcome::UpToDate);
    r.clock.advance(CHECK_INTERVAL);
    r.http.push(200, &feed("v0.2.0"));
    assert_eq!(r.updater.run(false).unwrap(), Outcome::UpToDate);
    assert_eq!(r.http.requests().len(), 3);
}

#[test]
fn never_updates_while_a_game_is_launching() {
    let r = rig("0.1.0");
    let lock = &r.updater.launch_lock;
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    // The launch writes its own process id; this test process stands in for it.
    std::fs::write(lock, std::process::id().to_string()).unwrap();
    assert_eq!(r.updater.run(true).unwrap(), Outcome::LaunchInProgress);
    assert!(r.http.requests().is_empty());
    untouched(&r);
}

#[test]
fn rollback_restores_the_previous_build_and_skips_the_bad_version() {
    let r = rig("0.1.0");
    let new = b"\x7fELF new build";
    let m = manifest("0.2.0", new);
    script_release(&r.http, "v0.2.0", &m, &sign(&m), new);
    r.updater.run(false).unwrap();

    // The new build runs `romshelf update --rollback`.
    let bad = Updater {
        current: Version(0, 2, 0),
        ..r.updater.clone()
    };
    bad.rollback().unwrap();
    assert_eq!(std::fs::read(&r.install).unwrap(), OLD);
    assert!(!previous(&r.install).exists());

    // Back on 0.1.0, the same release is not installed again.
    r.http.push(200, &feed("v0.2.0"));
    assert_eq!(r.updater.run(true).unwrap(), Outcome::UpToDate);
    assert_eq!(std::fs::read(&r.install).unwrap(), OLD);

    // A later release is.
    let newer = b"\x7fELF newer build";
    let m = manifest("0.3.0", newer);
    script_release(&r.http, "v0.3.0", &m, &sign(&m), newer);
    assert_eq!(
        r.updater.run(true).unwrap(),
        Outcome::Updated(Version(0, 3, 0))
    );
}

#[test]
fn rollback_without_a_previous_build_says_so() {
    let r = rig("0.1.0");
    let err = r.updater.rollback().unwrap_err();
    assert!(err.to_string().contains("No previous version"), "{err}");
    untouched(&r);
}

#[test]
fn a_token_is_sent_for_private_repositories() {
    let mut r = rig("0.2.0");
    r.updater.token = Some("ghp_test".into());
    r.http.push(200, &feed("v0.2.0"));
    r.updater.run(false).unwrap();
    assert_eq!(
        r.http.requests()[0].header_value("Authorization"),
        Some("Bearer ghp_test")
    );
}

#[test]
fn a_private_repository_without_a_token_explains_what_to_do() {
    let r = rig("0.1.0");
    r.http.push(404, r#"{"message":"Not Found"}"#);
    let err = r.updater.run(false).unwrap_err();
    assert!(err.to_string().contains("update --set-token"), "{err}");
}

#[test]
fn a_release_missing_files_is_an_error() {
    let r = rig("0.1.0");
    r.http.push(
        200,
        r#"{"tag_name":"v0.2.0","assets":[{"id":1,"name":"romshelf.AppImage","url":"u"}]}"#,
    );
    let err = r.updater.run(false).unwrap_err();
    assert!(err.to_string().contains("romshelf-update.json"), "{err}");
    untouched(&r);
}

#[test]
fn updates_are_off_without_a_public_key() {
    let mut r = rig("0.1.0");
    r.updater.public_key = [0u8; 32];
    assert_eq!(r.updater.run(true).unwrap(), Outcome::NotConfigured);
    assert!(r.http.requests().is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn a_lock_left_by_a_crashed_launch_is_ignored() {
    let r = rig("0.2.0");
    let lock = &r.updater.launch_lock;
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    // Process ids wrap well below this on Linux, so no such process exists.
    std::fs::write(lock, b"999999999").unwrap();
    r.http.push(200, &feed("v0.2.0"));
    assert_eq!(r.updater.run(true).unwrap(), Outcome::UpToDate);
}

/// Made by `openssl pkeyutl -sign -rawin` with a throwaway key, exactly as the release
/// workflow signs, so CI's signatures are known to verify on the Deck.
#[test]
fn signatures_from_openssl_verify() {
    let mut r = rig("0.1.0");
    r.updater.public_key = STANDARD
        .decode("UMCN0SA4x349VhwXKJRfy6zhpongCVbFI/QfAZgaHvU=")
        .unwrap()
        .try_into()
        .unwrap();
    let m = r#"{"version":"0.2.0","sha256":"4827a9503490c74fbd34b35bd7678210de63f10969c8c2ed05acbfcfe0054970","size":14}"#;
    let sig =
        "kqcN1sly5WQskVbPifY7FIDxP6C4+oRW8jcu5dt8n4Cv9mmWiNW/mf/Xw5KeD6w7PGYRsk4XvKM8ClpUsendCw==";
    script_release(&r.http, "v0.2.0", m, sig, b"\x7fELF new build");
    assert_eq!(
        r.updater.run(false).unwrap(),
        Outcome::Updated(Version(0, 2, 0))
    );
}
