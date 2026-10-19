//! Installing and preparing an emulator: the Flatpak, RetroArch's cores, Romshelf's session
//! folder and BIOS files, as resumable steps.
//!
//! Setup only reads from Drive (BIOS files) and never edits the emulator's own configuration.

use super::{EmulatorAdapter, Registry};
use crate::db::Database;
use crate::drive::scaffold::DriveLayout;
use crate::drive::DriveClient;
use crate::http::{HttpRequest, HttpTransport};
use crate::model::Emulator;
use crate::paths::Paths;
use crate::process::PackageRunner;
use crate::{Error, Result};
use serde::Serialize;
use std::io::Read;

pub use super::BiosFile;

const FLATHUB_REPO: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";
const CORES_URL: &str = "https://buildbot.libretro.com/nightly/linux/x86_64/latest";

/// Setup in order. Each is recorded when done, so an interrupted setup resumes at the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStep {
    /// The Flatpak, cores, session folder and BIOS files.
    Install,
    /// Romshelf's default controls. Recorded as done until stage 7 fills it in.
    DefaultControls,
    /// Romshelf's save folders on the Deck, and `saves/<system>/` and `states/<system>/` in
    /// Drive for each of the emulator's systems.
    LinkSaves,
}

impl SetupStep {
    pub const ALL: [SetupStep; 3] = [
        SetupStep::Install,
        SetupStep::DefaultControls,
        SetupStep::LinkSaves,
    ];

    fn key(self) -> &'static str {
        match self {
            SetupStep::Install => "install",
            SetupStep::DefaultControls => "default_controls",
            SetupStep::LinkSaves => "link_saves",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SetupStep::Install => "Installing",
            SetupStep::DefaultControls => "Setting up controls",
            SetupStep::LinkSaves => "Linking saves",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmulatorStatus {
    NotInstalled,
    /// Installed (perhaps by another tool) but Romshelf's setup hasn't finished.
    SetupIncomplete,
    Installed,
}

impl EmulatorStatus {
    /// As the Emulators screen shows it.
    pub fn label(self) -> &'static str {
        match self {
            EmulatorStatus::NotInstalled => "Not installed",
            EmulatorStatus::SetupIncomplete => "Setup incomplete",
            EmulatorStatus::Installed => "Installed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetupEvent {
    Step { step: SetupStep },
    Progress { percent: u8 },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BiosReport {
    /// The system folder the files belong in, such as "ps2".
    pub system: Option<String>,
    pub copied: Vec<String>,
    pub missing_required: Vec<String>,
    pub missing_optional: Vec<String>,
}

impl BiosReport {
    /// Empty when nothing is missing.
    pub fn summary(&self) -> String {
        if self.missing_required.is_empty() {
            return String::new();
        }
        format!(
            "Missing BIOS: {}. Add {} to Drive's Emulation/bios/{} folder.",
            self.missing_required.join(", "),
            if self.missing_required.len() == 1 {
                "it"
            } else {
                "them"
            },
            self.system.as_deref().unwrap_or("<system>")
        )
    }
}

pub struct SetupContext<'a> {
    pub runner: &'a dyn PackageRunner,
    pub http: &'a dyn HttpTransport,
    pub db: &'a Database,
    pub paths: &'a Paths,
    /// For BIOS files; `None` before a Drive folder is chosen.
    pub drive: Option<(&'a dyn DriveClient, &'a DriveLayout)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmulatorInfo {
    pub id: String,
    pub name: String,
    /// System labels: "SNES", "GBA", "N64".
    pub systems: Vec<String>,
    pub status: EmulatorStatus,
}

fn setting_key(adapter: &dyn EmulatorAdapter) -> String {
    format!("setup.{}", adapter.id())
}

/// Steps recorded as done, in order.
pub fn completed(db: &Database, adapter: &dyn EmulatorAdapter) -> Result<Vec<SetupStep>> {
    let done = db.setting(&setting_key(adapter))?.unwrap_or_default();
    Ok(SetupStep::ALL
        .into_iter()
        .filter(|s| done.split(',').any(|d| d == s.key()))
        .collect())
}

pub fn record_step(db: &Database, adapter: &dyn EmulatorAdapter, step: SetupStep) -> Result<()> {
    let mut done = completed(db, adapter)?;
    if !done.contains(&step) {
        done.push(step);
    }
    let text = done.iter().map(|s| s.key()).collect::<Vec<_>>().join(",");
    db.set_setting(&setting_key(adapter), &text)
}

/// Forgets the recorded steps, so the next setup runs them all again ("Repair").
pub fn reset(db: &Database, adapter: &dyn EmulatorAdapter) -> Result<()> {
    db.set_setting(&setting_key(adapter), "")
}

/// Keeps the database's "installed" flag, which the Library's "Needs …" reads, in step.
fn record_installed(db: &Database, adapter: &dyn EmulatorAdapter, installed: bool) -> Result<()> {
    let current = db.emulator(adapter.id())?;
    if current.as_ref().map(|e| e.installed) == Some(installed) {
        return Ok(());
    }
    db.upsert_emulator(&Emulator {
        id: adapter.id().into(),
        name: adapter.name().into(),
        flatpak_id: adapter.flatpak_id().into(),
        systems: adapter.systems().to_vec(),
        installed,
    })
}

pub fn status(ctx: &SetupContext, adapter: &dyn EmulatorAdapter) -> Result<EmulatorStatus> {
    let installed = adapter.detect(ctx.runner)?;
    record_installed(ctx.db, adapter, installed)?;
    if !installed {
        return Ok(EmulatorStatus::NotInstalled);
    }
    let cores_ok = adapter
        .required_cores()
        .iter()
        .all(|c| super::RetroArch::core_file(ctx.paths, c).is_file());
    let steps_ok = completed(ctx.db, adapter)?.len() == SetupStep::ALL.len();
    Ok(if cores_ok && steps_ok {
        EmulatorStatus::Installed
    } else {
        EmulatorStatus::SetupIncomplete
    })
}

/// Every emulator with its status, for the Emulators screen.
pub fn list(ctx: &SetupContext, registry: &Registry) -> Result<Vec<EmulatorInfo>> {
    registry
        .adapters()
        .map(|a| {
            Ok(EmulatorInfo {
                id: a.id().into(),
                name: a.name().into(),
                systems: a.systems().iter().map(|s| s.label().to_string()).collect(),
                status: status(ctx, a)?,
            })
        })
        .collect()
}

/// Runs the steps not yet done. Returns what happened with BIOS files.
pub fn run_setup(
    ctx: &SetupContext,
    adapter: &dyn EmulatorAdapter,
    on_event: &mut dyn FnMut(SetupEvent),
) -> Result<BiosReport> {
    let mut report = BiosReport::default();
    // Removed outside Romshelf since its setup ran: start again.
    let installed = adapter.detect(ctx.runner)?;
    if !installed {
        reset(ctx.db, adapter)?;
    }
    let done = completed(ctx.db, adapter)?;
    for step in SetupStep::ALL {
        if done.contains(&step) {
            continue;
        }
        on_event(SetupEvent::Step { step });
        if step == SetupStep::Install {
            if !installed {
                install_flatpak(ctx, adapter, on_event)?;
            }
            record_installed(ctx.db, adapter, true)?;
            for core in adapter.required_cores() {
                ensure_core(ctx, core)?;
            }
            adapter.prepare_session(ctx.paths)?;
            report = copy_bios(ctx, adapter)?;
        }
        if step == SetupStep::LinkSaves {
            adapter.prepare_session(ctx.paths)?;
            if let Some((drive, layout)) = ctx.drive {
                crate::saves::link_folders(drive, layout, adapter)?;
            }
        }
        record_step(ctx.db, adapter, step)?;
    }
    Ok(report)
}

fn install_flatpak(
    ctx: &SetupContext,
    adapter: &dyn EmulatorAdapter,
    on_event: &mut dyn FnMut(SetupEvent),
) -> Result<()> {
    let remote = ctx.runner.flatpak(&[
        "remote-add",
        "--user",
        "--if-not-exists",
        "flathub",
        FLATHUB_REPO,
    ])?;
    if remote.code != 0 {
        return Err(Error::External(format!(
            "Couldn't add Flathub, where {} comes from: {}. Check the Deck's internet connection and try again.",
            adapter.name(),
            remote.stderr.trim()
        )));
    }
    let mut last = None;
    let out = ctx.runner.flatpak_streaming(
        &[
            "install",
            "--user",
            "--noninteractive",
            "-y",
            "flathub",
            adapter.flatpak_id(),
        ],
        &mut |piece| {
            if let Some(percent) = last_percent(piece) {
                if last != Some(percent) {
                    last = Some(percent);
                    on_event(SetupEvent::Progress { percent });
                }
            }
        },
    )?;
    if out.code != 0 {
        let why = out.stderr.trim();
        return Err(Error::External(format!(
            "Couldn't install {}: {}. Try again.",
            adapter.name(),
            if why.is_empty() {
                "Flatpak gave no reason"
            } else {
                why
            }
        )));
    }
    Ok(())
}

/// The last "NN%" in a piece of Flatpak output.
fn last_percent(piece: &str) -> Option<u8> {
    piece
        .split(|c: char| !c.is_ascii_digit() && c != '%')
        .filter_map(|w| w.strip_suffix('%'))
        .filter_map(|n| n.parse::<u8>().ok())
        .rfind(|n| *n <= 100)
}

/// Fetches a RetroArch core from the libretro buildbot unless one is already there.
fn ensure_core(ctx: &SetupContext, core: &str) -> Result<()> {
    let target = super::RetroArch::core_file(ctx.paths, core);
    if target.is_file() {
        return Ok(());
    }
    let file = format!("{core}_libretro.so");
    let url = format!("{CORES_URL}/{file}.zip");
    let failed = |why: String| {
        Error::External(format!(
            "Couldn't download RetroArch's {core} core ({why}). Check the Deck's internet connection and try again."
        ))
    };
    let resp = ctx.http.send(&HttpRequest::new("GET", &url))?;
    if !resp.ok() {
        return Err(failed(format!("the server answered {}", resp.status)));
    }
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(resp.body)).map_err(|e| failed(e.to_string()))?;
    let mut entry = zip.by_name(&file).map_err(|e| failed(e.to_string()))?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    if !bytes.starts_with(b"\x7fELF") {
        return Err(failed("the download isn't a core".into()));
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = target.with_extension("so.part");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &target)?;
    Ok(())
}

/// Copies each declared BIOS file from Drive's `bios/<system>/` and lists what's missing.
fn copy_bios(ctx: &SetupContext, adapter: &dyn EmulatorAdapter) -> Result<BiosReport> {
    let mut report = BiosReport::default();
    for system in adapter.systems() {
        let files = adapter.bios_files(*system);
        if files.is_empty() {
            continue;
        }
        report.system = Some(system.id().to_string());
        let in_drive = match (
            ctx.drive,
            ctx.drive.and_then(|(_, l)| l.bios_folders.get(system.id())),
        ) {
            (Some((drive, _)), Some(folder)) => drive.list(folder)?,
            _ => Vec::new(),
        };
        let dest = adapter.bios_dir(ctx.paths, *system);
        for bios in files {
            let found = in_drive
                .iter()
                .find(|f| !f.is_folder() && f.name.eq_ignore_ascii_case(&bios.name));
            match (found, ctx.drive) {
                (Some(f), Some((drive, _))) => {
                    std::fs::create_dir_all(&dest)?;
                    let bytes = drive.download(&f.id, 0)?;
                    std::fs::write(dest.join(&bios.name), bytes)?;
                    report.copied.push(bios.name);
                }
                _ if bios.required => report.missing_required.push(bios.name),
                _ => report.missing_optional.push(bios.name),
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_read_from_flatpak_output() {
        assert_eq!(last_percent("Installing 1/2… 58%"), Some(58));
        assert_eq!(last_percent("[####    ] 12% 3.1 MB/s"), Some(12));
        assert_eq!(last_percent("Installing 2/2… 100%"), Some(100));
        assert_eq!(last_percent("Looking for matches…"), None);
        assert_eq!(last_percent("150%"), None);
    }
}
