//! The self-test: everything Romshelf can check on the Deck by itself, at the press of a button
//! in Settings (or `romshelf selftest`). `doctor`'s checks, then what a game needs beyond them:
//! its Steam shortcut, artwork and Steam Input layout, its launch planned step by step, the
//! emulator files Romshelf writes, ROM storage, a real round trip through Drive (with a dropped
//! connection on the way down) and how recent launches ended.
//!
//! Nothing here touches the player's games or saves. Drive is written only inside Romshelf's own
//! `.romshelf/selftest/`, and the test file goes to the trash afterwards.

use crate::controls::retroarch::core_name;
use crate::db::Database;
use crate::doctor::{self, DoctorContext};
use crate::drive::scaffold::DriveLayout;
use crate::drive::{DriveClient, DriveFile};
use crate::emulators::{Dolphin, EmulatorAdapter, RetroArch};
use crate::model::{ArtSlot, LocationState, Rom, RomId, System};
use crate::paths::Paths;
use crate::steam::{self, apply, artwork, input, shortcuts};
use crate::storage::{self, human_size, DiskSpace};
use crate::transfer::Downloader;
use crate::{Error, Result};
use md5::{Digest, Md5};
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    /// Works, but something needs attention.
    Warn,
    Fail,
    /// Couldn't be checked here (not signed in, nothing to check yet).
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// "Setup", "Steam", "Games", "Emulators", "Storage", "Drive", "Launches".
    pub group: String,
    pub name: String,
    pub outcome: Outcome,
    pub detail: String,
    /// One thing to do, when the check isn't fine.
    pub fix: Option<String>,
}

fn check(group: &str, name: &str, outcome: Outcome, detail: impl Into<String>) -> Check {
    Check {
        group: group.into(),
        name: name.into(),
        outcome,
        detail: detail.into(),
        fix: None,
    }
}

fn with_fix(mut c: Check, fix: impl Into<String>) -> Check {
    c.fix = Some(fix.into());
    c
}

pub struct Context<'a> {
    pub doctor: DoctorContext<'a>,
    pub space: &'a dyn DiskSpace,
    /// A launch planned without doing any of it (`romshelf doctor --id`): its steps.
    pub plan: &'a dyn Fn(&Rom) -> Result<Vec<String>>,
    /// Size of the file sent through Drive and back (Drive's simple upload takes up to 5 MB).
    pub drive_test_bytes: usize,
}

/// Every check in order; `on_check` hears each as it finishes, for a live list.
pub fn run(ctx: &Context, on_check: &mut dyn FnMut(&Check)) -> Vec<Check> {
    let mut out = Vec::new();
    let mut add = |checks: Vec<Check>, out: &mut Vec<Check>| {
        for c in checks {
            on_check(&c);
            out.push(c);
        }
    };
    let setup = doctor::run(&ctx.doctor)
        .into_iter()
        .map(|c| Check {
            group: "Setup".into(),
            name: c.name,
            outcome: match c.status {
                doctor::Status::Ok => Outcome::Pass,
                doctor::Status::Warn => Outcome::Warn,
                doctor::Status::Fail => Outcome::Fail,
            },
            detail: c.detail,
            fix: c.fix,
        })
        .collect();
    add(setup, &mut out);
    add(steam(&ctx.doctor), &mut out);
    add(games(ctx), &mut out);
    add(emulator_files(&ctx.doctor), &mut out);
    add(vec![storage(ctx.doctor.paths, ctx.space)], &mut out);
    let drive = match (&ctx.doctor.drive, &ctx.doctor.layout, &ctx.doctor.account) {
        (Some(drive), Some(layout), Some(_)) => drive_round_trip(
            *drive,
            layout,
            ctx.doctor.paths,
            ctx.space,
            ctx.drive_test_bytes,
        ),
        _ => check(
            "Drive",
            "Round trip",
            Outcome::Skip,
            "Not signed in, or no Drive folder chosen.",
        ),
    };
    add(vec![drive], &mut out);
    add(vec![launches(ctx.doctor.db, ctx.doctor.paths)], &mut out);
    out
}

/// Games on the Deck, by name.
fn on_deck(db: &Database) -> Vec<Rom> {
    let mut roms: Vec<Rom> = db
        .roms()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.recognized && r.location == LocationState::OnDeck)
        .collect();
    roms.sort_by_key(|r| r.display_name.to_lowercase());
    roms
}

/// Each game on the Deck, its launch planned step by step.
fn games(ctx: &Context) -> Vec<Check> {
    let roms = on_deck(ctx.doctor.db);
    if roms.is_empty() {
        return vec![check(
            "Games",
            "Games on this Deck",
            Outcome::Skip,
            "No games downloaded yet.",
        )];
    }
    roms.iter()
        .map(|rom| match (ctx.plan)(rom) {
            Ok(steps) => check(
                "Games",
                &rom.display_name,
                Outcome::Pass,
                format!("Launch would: {}", steps.join(" → ")),
            ),
            Err(e) => with_fix(
                check("Games", &rom.display_name, Outcome::Fail, e.to_string()),
                "Open the game in Romshelf: its page says what it needs.",
            ),
        })
        .collect()
}

/// Steam's shortcuts file, and each of Romshelf's shortcuts: what it launches, its artwork and
/// Romshelf's Steam Input layout (which gives the back grips their keys).
pub fn steam(ctx: &DoctorContext) -> Vec<Check> {
    const G: &str = "Steam";
    let chosen = ctx.db.setting(apply::USER_SETTING).ok().flatten();
    let user = match steam::active_user(ctx.steam, chosen.as_deref()) {
        Ok(Some(u)) => u,
        _ => {
            return vec![check(
                G,
                "Steam account",
                Outcome::Skip,
                "No Steam account found.",
            )]
        }
    };
    let mut out = Vec::new();
    let found = match ctx
        .steam
        .read_shortcuts(&user)
        .and_then(|b| shortcuts::list(b.as_deref()))
    {
        Ok(f) => f,
        Err(e) => {
            return vec![with_fix(
                check(
                    G,
                    "Steam's shortcuts file",
                    Outcome::Fail,
                    format!("It can't be read: {e}"),
                ),
                "Restart Steam; if it stays, send this report.",
            )]
        }
    };
    let games: Vec<_> = found
        .iter()
        .filter(|f| f.app_id != apply::self_app_id())
        .collect();
    out.push(match ctx.steam.shortcuts_executable(&user) {
        Ok(None) => check(
            G,
            "Steam's shortcuts file",
            Outcome::Skip,
            "There's no shortcuts file yet.",
        ),
        Ok(Some(true)) => check(
            G,
            "Steam's shortcuts file",
            Outcome::Pass,
            format!(
                "Readable and executable, with {} of Romshelf's games.",
                games.len()
            ),
        ),
        Ok(Some(false)) => with_fix(
            check(
                G,
                "Steam's shortcuts file",
                Outcome::Fail,
                "It isn't executable, so NonSteamLaunchers' scanner may empty it.",
            ),
            format!("Run: chmod +x ~/.steam/steam/userdata/{user}/config/shortcuts.vdf"),
        ),
        Err(e) => check(
            G,
            "Steam's shortcuts file",
            Outcome::Warn,
            format!("Its permissions couldn't be read: {e}"),
        ),
    });
    out.push(if found.iter().any(|f| f.app_id == apply::self_app_id()) {
        check(
            G,
            "Romshelf in Steam",
            Outcome::Pass,
            "Romshelf's own shortcut is there.",
        )
    } else {
        with_fix(
            check(
                G,
                "Romshelf in Steam",
                Outcome::Warn,
                "Romshelf itself has no shortcut made by Romshelf.",
            ),
            "Run: romshelf steam add-self, then restart Steam.",
        )
    });
    let grid = ctx.steam.grid_files(&user).unwrap_or_default();
    for f in games {
        let id = f
            .launch_options
            .strip_prefix("launch --id ")
            .unwrap_or("")
            .trim();
        let rom = ctx.db.rom(&RomId(id.to_string())).ok().flatten();
        if rom.is_none() {
            out.push(with_fix(
                check(
                    G,
                    &f.name,
                    Outcome::Warn,
                    format!(
                        "It launches \"{}\", which isn't in Romshelf's library.",
                        f.launch_options
                    ),
                ),
                "Run: romshelf steam apply",
            ));
            continue;
        }
        let missing_art: Vec<&str> = artwork::SLOTS
            .iter()
            .filter(|s| artwork::size(**s).is_some())
            .filter(|s| {
                let stem = artwork::stem(**s, f.app_id);
                !grid
                    .iter()
                    .any(|g| g.rsplit_once('.').map(|(b, _)| b) == Some(stem.as_str()))
            })
            .map(|s| slot_label(*s))
            .collect();
        let layout = ctx
            .steam
            .read_controller_layout(&user, &input::config_key(&f.name))
            .ok()
            .flatten();
        let mut problems = Vec::new();
        if !missing_art.is_empty() {
            problems.push(format!("missing artwork: {}", missing_art.join(", ")));
        }
        if layout.is_none() {
            problems.push("no Steam Input layout, so the back grips send nothing".into());
        }
        out.push(if problems.is_empty() {
            check(
                G,
                &f.name,
                Outcome::Pass,
                format!("Launches {id}; artwork and Steam Input layout in place."),
            )
        } else {
            with_fix(
                check(
                    G,
                    &f.name,
                    Outcome::Warn,
                    format!("Launches {id}, but {}.", problems.join("; ")),
                ),
                "Run: romshelf steam apply, then restart Steam.",
            )
        });
    }
    out
}

fn slot_label(slot: ArtSlot) -> &'static str {
    match slot {
        ArtSlot::Capsule => "Capsule",
        ArtSlot::WideCapsule => "Wide capsule",
        ArtSlot::Hero => "Hero",
        ArtSlot::Logo => "Logo",
        ArtSlot::Icon => "Icon",
    }
}

/// The files Romshelf writes for each emulator: RetroArch's shared config, and each game's
/// controls (written at launch, so a game not played yet has none).
pub fn emulator_files(ctx: &DoctorContext) -> Vec<Check> {
    const G: &str = "Emulators";
    let roms = on_deck(ctx.db);
    let mut out = Vec::new();
    let cfg = RetroArch::append_config(ctx.paths);
    let retro_games = roms.iter().any(|r| RetroArch.systems().contains(&r.system));
    match std::fs::read_to_string(&cfg) {
        Ok(text) => {
            let missing: Vec<&str> = [
                "input_remap_binds_enable = \"true\"",
                "remap_save_on_exit = \"false\"",
            ]
            .into_iter()
            .filter(|line| !text.contains(line))
            .collect();
            out.push(if missing.is_empty() {
                check(
                    G,
                    "RetroArch config",
                    Outcome::Pass,
                    "Remaps on, and never saved over by RetroArch.",
                )
            } else {
                with_fix(
                    check(
                        G,
                        "RetroArch config",
                        Outcome::Fail,
                        format!("{} is missing {}.", cfg.display(), missing.join(" and ")),
                    ),
                    "Play any RetroArch game from Romshelf once: it writes the file again.",
                )
            });
        }
        Err(_) if retro_games => out.push(check(
            G,
            "RetroArch config",
            Outcome::Pass,
            "Not written yet: it's written the next time a RetroArch game is played.",
        )),
        Err(_) => {}
    }
    for rom in &roms {
        let files: Vec<std::path::PathBuf> = if RetroArch.systems().contains(&rom.system) {
            let mut f = vec![RetroArch::controls_config(ctx.paths, rom)];
            if let Some(core) = RetroArch::core_for(rom.system).and_then(core_name) {
                f.push(
                    RetroArch::remap_dir(ctx.paths, rom)
                        .join(core)
                        .join(format!("{core}.rmp")),
                );
            }
            f
        } else if Dolphin.systems().contains(&rom.system) {
            vec![Dolphin::config_dir(ctx.paths).join("GCPadNew.ini")]
        } else {
            continue;
        };
        let present = files.iter().filter(|f| f.exists()).count();
        out.push(if present == files.len() {
            check(
                G,
                &rom.display_name,
                Outcome::Pass,
                "Controls written for its emulator.",
            )
        } else if present == 0 {
            check(
                G,
                &rom.display_name,
                Outcome::Pass,
                "Controls are written the next time it's played.",
            )
        } else {
            let missing: Vec<String> = files
                .iter()
                .filter(|f| !f.exists())
                .map(|f| f.display().to_string())
                .collect();
            with_fix(
                check(
                    G,
                    &rom.display_name,
                    Outcome::Warn,
                    format!("Missing {}.", missing.join(", ")),
                ),
                "Play it once from Romshelf: its controls are written again.",
            )
        });
    }
    out
}

/// ROM storage: there, writable (a file written, read back and removed) and its free space.
pub fn storage(paths: &Paths, space: &dyn DiskSpace) -> Check {
    const G: &str = "Storage";
    let root = paths.storage_root();
    if !storage::is_available(paths) {
        return with_fix(
            check(
                G,
                "ROM storage",
                Outcome::Fail,
                format!("{} isn't there.", root.display()),
            ),
            "Insert the SD card that holds it, or choose another location in Settings.",
        );
    }
    let probe = root.join(".romshelf-selftest-probe");
    let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 251) as u8).collect();
    let written = std::fs::write(&probe, &data).and_then(|_| std::fs::read(&probe));
    let _ = std::fs::remove_file(&probe);
    match written {
        Ok(back) if back == data => {}
        Ok(_) => {
            return with_fix(
                check(
                    G,
                    "ROM storage",
                    Outcome::Fail,
                    "A file written there read back different.",
                ),
                "Check the SD card in another device; it may be failing.",
            )
        }
        Err(e) => {
            return with_fix(
                check(
                    G,
                    "ROM storage",
                    Outcome::Fail,
                    format!("It can't be written: {e}"),
                ),
                "Choose another location in Settings.",
            )
        }
    }
    let free = space
        .available(&root)
        .map(|b| format!("{} free", human_size(b)))
        .unwrap_or_else(|_| "free space unknown".into());
    let card = if root.starts_with("/run/media") {
        " (SD card)"
    } else {
        ""
    };
    check(
        G,
        "ROM storage",
        Outcome::Pass,
        format!("{}{card}: writable, {free}.", root.display()),
    )
}

/// Drops the connection once, partway through the first download, to prove resuming works.
struct DropOnce<'a> {
    inner: &'a dyn DriveClient,
    after: u64,
    dropped: AtomicBool,
}

struct Cut<'a> {
    out: &'a mut dyn std::io::Write,
    left: u64,
}

impl std::io::Write for Cut<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "dropped on purpose",
            ));
        }
        let n = buf.len().min(self.left as usize);
        let n = self.out.write(&buf[..n])?;
        self.left -= n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

impl DriveClient for DropOnce<'_> {
    fn about(&self) -> Result<crate::drive::Account> {
        self.inner.about()
    }
    fn create_folder(&self, name: &str, parent: &str) -> Result<DriveFile> {
        self.inner.create_folder(name, parent)
    }
    fn create_file(&self, name: &str, parent: &str, bytes: &[u8]) -> Result<DriveFile> {
        self.inner.create_file(name, parent, bytes)
    }
    fn update_content(&self, id: &str, bytes: &[u8]) -> Result<DriveFile> {
        self.inner.update_content(id, bytes)
    }
    fn list(&self, parent: &str) -> Result<Vec<DriveFile>> {
        self.inner.list(parent)
    }
    fn get(&self, id: &str) -> Result<DriveFile> {
        self.inner.get(id)
    }
    fn rename(&self, id: &str, new_name: &str) -> Result<DriveFile> {
        self.inner.rename(id, new_name)
    }
    fn move_to(&self, id: &str, new_parent: &str) -> Result<DriveFile> {
        self.inner.move_to(id, new_parent)
    }
    fn trash(&self, id: &str) -> Result<()> {
        self.inner.trash(id)
    }
    fn download(&self, id: &str, offset: u64) -> Result<Vec<u8>> {
        self.inner.download(id, offset)
    }
    fn download_to(&self, id: &str, offset: u64, out: &mut dyn std::io::Write) -> Result<u64> {
        if self.dropped.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return self.inner.download_to(id, offset, out);
        }
        let mut cut = Cut {
            out,
            left: self.after.saturating_sub(offset),
        };
        match self.inner.download_to(id, offset, &mut cut) {
            Err(Error::Network(_)) | Err(Error::Io(_)) | Err(Error::External(_)) => Err(
                Error::Network("the connection dropped (on purpose, by the self-test)".into()),
            ),
            other => other,
        }
    }
    fn start_page_token(&self) -> Result<String> {
        self.inner.start_page_token()
    }
    fn changes(&self, token: &str) -> Result<crate::drive::ChangePage> {
        self.inner.changes(token)
    }
}

fn test_bytes(n: usize) -> Vec<u8> {
    let mut x: u32 = 0x9E37_79B9;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A file sent to Drive (inside `.romshelf/selftest/`), checked there, downloaded the way a ROM
/// is with the connection dropped halfway, checked again, then put in Drive's trash.
pub fn drive_round_trip(
    drive: &dyn DriveClient,
    layout: &DriveLayout,
    paths: &Paths,
    space: &dyn DiskSpace,
    bytes: usize,
) -> Check {
    const G: &str = "Drive";
    const N: &str = "Round trip";
    let offline = |e: &Error| matches!(e, Error::Network(_));
    let fail = |e: Error| {
        if offline(&e) {
            with_fix(
                check(
                    G,
                    N,
                    Outcome::Fail,
                    format!("Romshelf couldn't reach Google: {e}"),
                ),
                "Check the Deck's internet connection, then run the self-test again.",
            )
        } else {
            with_fix(
                check(G, N, Outcome::Fail, e.to_string()),
                "Send this report.",
            )
        }
    };
    let data = test_bytes(bytes);
    let want = md5_hex(&data);
    let folder = match drive.list(&layout.meta).and_then(|files| {
        match files
            .into_iter()
            .find(|f| f.name == "selftest" && f.is_folder() && !f.trashed)
        {
            Some(f) => Ok(f),
            None => drive.create_folder("selftest", &layout.meta),
        }
    }) {
        Ok(f) => f,
        Err(e) => return fail(e),
    };
    let up_start = Instant::now();
    let file = match drive.create_file("selftest.bin", &folder.id, &data) {
        Ok(f) => f,
        Err(e) => return fail(e),
    };
    let up = up_start.elapsed();
    let result = (|| -> Result<(Duration, bool)> {
        let there = drive.get(&file.id)?;
        if there.size != data.len() as u64 || there.md5.as_deref() != Some(want.as_str()) {
            return Err(Error::External(format!(
                "Drive has {} with checksum {:?}; Romshelf sent {} with {want}.",
                human_size(there.size),
                there.md5,
                human_size(data.len() as u64),
            )));
        }
        let scratch = paths.storage_root().join(".romshelf-selftest");
        std::fs::create_dir_all(&scratch)?;
        let scratch_paths = paths.clone().with_storage_root(&scratch);
        let rom = Rom {
            id: RomId("gba-5e1f7e57".into()),
            drive_file_id: file.id.clone(),
            system: System::Gba,
            file_name: "selftest.gba".into(),
            display_name: "Self-test file".into(),
            size: data.len() as u64,
            checksum: Some(want.clone()),
            location: LocationState::DriveOnly,
            recognized: true,
        };
        let flaky = DropOnce {
            inner: drive,
            after: (data.len() / 2) as u64,
            dropped: AtomicBool::new(false),
        };
        let cancel = AtomicBool::new(false);
        let dl = Downloader {
            drive: &flaky,
            paths: &scratch_paths,
            space,
            cancel: &cancel,
            retry_delay: Duration::from_millis(200),
            max_attempts: 4,
        };
        let down_start = Instant::now();
        let got = dl.download(&rom, &mut |_| {});
        let down = down_start.elapsed();
        let local = scratch_paths.rom_file(System::Gba, "selftest.gba");
        let same = local
            .as_ref()
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| b == data)
            .unwrap_or(false);
        let _ = std::fs::remove_dir_all(&scratch);
        got?;
        Ok((down, same))
    })();
    let trashed = drive.trash(&file.id);
    let (down, same) = match result {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    if !same {
        return with_fix(
            check(
                G,
                N,
                Outcome::Fail,
                "The downloaded file doesn't match what was sent.",
            ),
            "Send this report.",
        );
    }
    let rate = |d: Duration| {
        let secs = d.as_secs_f64().max(0.001);
        format!("{:.1} MB/s", data.len() as f64 / secs / 1_000_000.0)
    };
    let mut c = check(
        G,
        N,
        Outcome::Pass,
        format!(
            "Sent {} up ({}) and back ({}); resumed after a dropped connection; checksums matched.",
            human_size(data.len() as u64),
            rate(up),
            rate(down),
        ),
    );
    if let Err(e) = trashed {
        c.outcome = Outcome::Warn;
        c.detail
            .push_str(&format!(" The test file couldn't be put in the trash: {e}"));
        c.fix = Some("Delete .romshelf/selftest/selftest.bin from Drive.".into());
    }
    c
}

/// How recent launches ended, from `launch.log`.
pub fn launches(db: &Database, paths: &Paths) -> Check {
    const G: &str = "Launches";
    const N: &str = "Recent launches";
    let text = std::fs::read_to_string(paths.log_dir().join("launch.log")).unwrap_or_default();
    let mut count = 0;
    let mut last_failure: Option<(String, String, String, Vec<String>)> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("    ") {
            if let Some((_, _, _, out)) = last_failure.as_mut() {
                out.push(rest.to_string());
            }
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let [when, id, _, _, "exit", code] = parts.as_slice() else {
            continue;
        };
        count += 1;
        if *code != "0" {
            let name = db
                .rom(&RomId((*id).to_string()))
                .ok()
                .flatten()
                .map(|r| r.display_name)
                .unwrap_or_else(|| (*id).to_string());
            let when = when
                .parse::<i64>()
                .map(|t| {
                    // 20251009T090140Z → 2025-10-09 09:01
                    let st = crate::saves::utc_stamp(t);
                    format!(
                        " ({}-{}-{} {}:{} UTC)",
                        &st[0..4],
                        &st[4..6],
                        &st[6..8],
                        &st[9..11],
                        &st[11..13]
                    )
                })
                .unwrap_or_default();
            last_failure = Some((name, (*code).to_string(), when, Vec::new()));
        }
    }
    if count == 0 {
        return check(G, N, Outcome::Skip, "No games launched yet.");
    }
    match last_failure {
        None => check(
            G,
            N,
            Outcome::Pass,
            format!("{count} launches, all ended normally."),
        ),
        Some((name, code, when, out)) => {
            let said = if out.is_empty() {
                String::new()
            } else {
                format!(" It said: {}", out.join(" / "))
            };
            check(
                G,
                N,
                Outcome::Warn,
                format!(
                    "{count} launches; the last that failed: {name} ended with exit {code}{when}.{said}"
                ),
            )
        }
    }
}

/// The plain-text report: one line per check, a fix under each problem, totals last.
pub fn report(checks: &[Check]) -> String {
    let mut out = String::new();
    for c in checks {
        let tag = match c.outcome {
            Outcome::Pass => "pass",
            Outcome::Warn => "WARN",
            Outcome::Fail => "FAIL",
            Outcome::Skip => "skip",
        };
        out.push_str(&format!("{tag}  {} / {}: {}\n", c.group, c.name, c.detail));
        if let Some(fix) = &c.fix {
            out.push_str(&format!("      Fix: {fix}\n"));
        }
    }
    let n = |o: Outcome| checks.iter().filter(|c| c.outcome == o).count();
    out.push_str(&format!(
        "\n{} passed, {} to look at, {} failed, {} skipped\n",
        n(Outcome::Pass),
        n(Outcome::Warn),
        n(Outcome::Fail),
        n(Outcome::Skip)
    ));
    out
}
