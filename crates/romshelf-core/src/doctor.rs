//! `romshelf doctor`: is everything Romshelf depends on in place? Each check that isn't
//! fine says what's wrong and one thing to do about it.

use crate::db::Database;
use crate::drive::scaffold::DriveLayout;
use crate::drive::DriveClient;
use crate::emulators::Registry;
use crate::model::LocationState;
use crate::paths::Paths;
use crate::process::PackageRunner;
use crate::steam::{self, shortcuts, SteamFiles};
use crate::{storage, Error};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    /// Works, but something needs attention.
    Warn,
    /// Something Romshelf needs is broken.
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
    /// One thing to do, when the check isn't fine.
    pub fix: Option<String>,
}

fn ok(name: &str, detail: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        status: Status::Ok,
        detail: detail.into(),
        fix: None,
    }
}

fn bad(name: &str, status: Status, detail: impl Into<String>, fix: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        status,
        detail: detail.into(),
        fix: Some(fix.into()),
    }
}

/// What the checks look at.
pub struct DoctorContext<'a> {
    pub db: &'a Database,
    pub paths: &'a Paths,
    pub registry: &'a Registry,
    pub runner: &'a dyn PackageRunner,
    pub steam: &'a dyn SteamFiles,
    /// The signed-in account, if any.
    pub account: Option<String>,
    /// The Drive folder's ids, once chosen.
    pub layout: Option<DriveLayout>,
    pub drive: Option<&'a dyn DriveClient>,
}

/// Every check, in a fixed order.
pub fn run(ctx: &DoctorContext) -> Vec<Check> {
    let mut out = vec![
        sign_in(ctx),
        drive_folder(ctx),
        storage_root(ctx),
        install(ctx),
    ];
    out.extend(emulators(ctx));
    out.push(steam_found(ctx));
    out.push(shortcut_targets(ctx));
    out.push(pending_backups(ctx));
    out.push(index(ctx));
    out
}

/// One line per check, then its fix; for the command line.
pub fn report(checks: &[Check]) -> String {
    let mut s = String::new();
    for c in checks {
        let mark = match c.status {
            Status::Ok => "ok  ",
            Status::Warn => "warn",
            Status::Fail => "FAIL",
        };
        s.push_str(&format!("{mark}  {}: {}\n", c.name, c.detail));
        if let Some(fix) = &c.fix {
            s.push_str(&format!("      Fix: {fix}\n"));
        }
    }
    s
}

/// 0 when nothing failed, 1 otherwise.
pub fn exit_code(checks: &[Check]) -> i32 {
    i32::from(checks.iter().any(|c| c.status == Status::Fail))
}

fn sign_in(ctx: &DoctorContext) -> Check {
    match &ctx.account {
        Some(a) => ok("Google account", format!("signed in as {a}")),
        None => bad(
            "Google account",
            Status::Fail,
            "not signed in",
            "Sign in from Romshelf, or run `romshelf auth login`.",
        ),
    }
}

fn drive_folder(ctx: &DoctorContext) -> Check {
    const NAME: &str = "Drive folder";
    let Some(layout) = &ctx.layout else {
        return bad(
            NAME,
            Status::Fail,
            "no Drive folder chosen",
            "Open Romshelf and choose where the Emulation folder goes.",
        );
    };
    let Some(drive) = ctx.drive else {
        return ok(NAME, "chosen (not checked: no connection)");
    };
    match drive.get(&layout.roms) {
        Ok(f) if !f.trashed => ok(NAME, "Emulation/roms is there"),
        Ok(_) => bad(
            NAME,
            Status::Fail,
            "Emulation/roms is in Drive's trash",
            "Restore it from Drive's trash, or choose the folder again in Settings → Drive folder.",
        ),
        Err(Error::Network(_)) => bad(
            NAME,
            Status::Warn,
            "couldn't reach Google to check it",
            "Check the Deck's internet connection, then run this again.",
        ),
        Err(_) => bad(
            NAME,
            Status::Fail,
            "Emulation/roms can't be found in Drive",
            "Choose the folder again in Settings → Drive folder.",
        ),
    }
}

fn storage_root(ctx: &DoctorContext) -> Check {
    const NAME: &str = "ROM storage";
    let root = ctx.paths.storage_root();
    if !storage::is_available(ctx.paths) {
        return bad(
            NAME,
            Status::Fail,
            format!("{} isn't there", root.display()),
            "Insert the SD card that holds your ROMs, or choose another place in Settings → Storage.",
        );
    }
    let probe = ctx.paths.rom_root.join(".romshelf-write-test");
    let writable = std::fs::create_dir_all(&ctx.paths.rom_root)
        .and_then(|_| std::fs::write(&probe, b"ok"))
        .and_then(|_| std::fs::remove_file(&probe))
        .is_ok();
    if writable {
        ok(NAME, format!("{} (writable)", root.display()))
    } else {
        bad(
            NAME,
            Status::Fail,
            format!("{} can't be written to", root.display()),
            "Choose another place in Settings → Storage, or fix the folder's permissions.",
        )
    }
}

fn install(ctx: &DoctorContext) -> Check {
    const NAME: &str = "Romshelf install";
    let file = &ctx.paths.appimage;
    if !file.is_file() {
        return bad(
            NAME,
            Status::Warn,
            format!("{} isn't there", file.display()),
            "Install with scripts/install.sh, so Steam shortcuts have a fixed program to run.",
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(file)
            .map(|m| m.permissions().mode())
            .unwrap_or(0);
        if mode & 0o111 == 0 {
            return bad(
                NAME,
                Status::Fail,
                format!("{} isn't executable", file.display()),
                format!("Run `chmod +x {}`.", file.display()),
            );
        }
    }
    ok(NAME, file.display().to_string())
}

fn emulators(ctx: &DoctorContext) -> Vec<Check> {
    let roms = ctx.db.roms().unwrap_or_default();
    ctx.registry
        .adapters()
        .map(|a| {
            let name = a.name();
            let needed = roms
                .iter()
                .any(|r| r.location != LocationState::Gone && a.systems().contains(&r.system));
            match a.detect(ctx.runner) {
                Ok(true) => ok(name, "installed"),
                Ok(false) if !needed => ok(name, "not installed (no games need it yet)"),
                Ok(false) => bad(
                    name,
                    Status::Warn,
                    "not installed, and some of your games need it",
                    format!("Set it up from Romshelf's Emulators screen, or run `romshelf emulators --setup {}`.", a.id()),
                ),
                Err(e) => bad(
                    name,
                    Status::Warn,
                    format!("couldn't ask Flatpak: {e}"),
                    "Check that Flatpak works: `flatpak list` in a terminal.",
                ),
            }
        })
        .collect()
}

fn steam_found(ctx: &DoctorContext) -> Check {
    const NAME: &str = "Steam";
    let chosen = ctx.db.setting(steam::apply::USER_SETTING).ok().flatten();
    match steam::active_user(ctx.steam, chosen.as_deref()) {
        Ok(Some(u)) => ok(NAME, format!("account {u}")),
        _ => bad(
            NAME,
            Status::Warn,
            "no Steam account found in ~/.steam/steam/userdata",
            "Start Steam and sign in once, then run this again.",
        ),
    }
}

fn shortcut_targets(ctx: &DoctorContext) -> Check {
    const NAME: &str = "Steam shortcuts";
    let chosen = ctx.db.setting(steam::apply::USER_SETTING).ok().flatten();
    let Ok(Some(user)) = steam::active_user(ctx.steam, chosen.as_deref()) else {
        return ok(NAME, "none (no Steam account)");
    };
    let bytes = match ctx.steam.read_shortcuts(&user) {
        Ok(b) => b,
        Err(e) => {
            return bad(
                NAME,
                Status::Warn,
                format!("couldn't read shortcuts.vdf: {e}"),
                "Start Steam once, then run this again.",
            )
        }
    };
    let Ok(found) = shortcuts::list(bytes.as_deref()) else {
        return bad(
            NAME,
            Status::Fail,
            "Steam's shortcuts file can't be read",
            "Restart Steam so it writes the file again.",
        );
    };
    let want = format!("\"{}\"", ctx.paths.appimage.display());
    let root = match crate::steam::vdf::parse(bytes.as_deref().unwrap_or_default()) {
        Ok(r) => r,
        Err(_) => return ok(NAME, "none yet"),
    };
    let wrong = match crate::steam::vdf::get(&root, "shortcuts") {
        Some(crate::steam::vdf::Value::Map(list)) => list
            .iter()
            .filter_map(|(_, v)| match v {
                crate::steam::vdf::Value::Map(e)
                    if found
                        .iter()
                        .any(|f| Some(f.app_id) == crate::steam::vdf::int_of(e, "appid")) =>
                {
                    Some(crate::steam::vdf::str_of(e, "Exe").unwrap_or_default())
                }
                _ => None,
            })
            .filter(|exe| *exe != want)
            .count(),
        _ => 0,
    };
    if wrong == 0 {
        ok(
            NAME,
            format!(
                "{} from Romshelf, all running {}",
                found.len(),
                ctx.paths.appimage.display()
            ),
        )
    } else {
        bad(
            NAME,
            Status::Fail,
            format!(
                "{wrong} of Romshelf's shortcuts run another program than {}",
                ctx.paths.appimage.display()
            ),
            "Run `romshelf steam apply`, then restart Steam.",
        )
    }
}

fn pending_backups(ctx: &DoctorContext) -> Check {
    const NAME: &str = "Save backups";
    match ctx.db.pending_saves() {
        Ok(p) if p.is_empty() => ok(NAME, "none waiting"),
        Ok(p) => bad(
            NAME,
            Status::Warn,
            format!(
                "{} {} waiting to reach Drive",
                p.len(),
                if p.len() == 1 {
                    "game's save is"
                } else {
                    "games' saves are"
                }
            ),
            "Open Romshelf while online; they upload at the next sync.",
        ),
        Err(e) => bad(
            NAME,
            Status::Warn,
            format!("couldn't read the index: {e}"),
            "Run this again.",
        ),
    }
}

fn index(ctx: &DoctorContext) -> Check {
    const NAME: &str = "Library index";
    if !storage::is_available(ctx.paths) {
        return ok(NAME, "not checked (storage unavailable)");
    }
    let roms = ctx.db.roms().unwrap_or_default();
    let mismatched = roms
        .iter()
        .filter(|r| match r.location {
            LocationState::OnDeck => !storage::file_matches(ctx.paths, r),
            LocationState::DriveOnly => storage::file_matches(ctx.paths, r),
            _ => false,
        })
        .count();
    if mismatched == 0 {
        ok(NAME, format!("{} games, matching the disk", roms.len()))
    } else {
        bad(
            NAME,
            Status::Warn,
            format!(
                "{mismatched} {} the disk",
                if mismatched == 1 {
                    "game doesn't match"
                } else {
                    "games don't match"
                }
            ),
            "Open Romshelf: it corrects this when it starts.",
        )
    }
}
