//! Where ROMs live on the Deck: free space, whether the storage is there at all, and keeping
//! each ROM's location in step with the files on disk.

use crate::db::Database;
use crate::model::{LocationState, Rom};
use crate::paths::Paths;
use crate::transfer;
use crate::{Error, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Free-space seam: the real disk, or a fixed number in tests.
pub trait DiskSpace: Send + Sync {
    /// Bytes available to this user on the filesystem holding `path`.
    fn available(&self, path: &Path) -> Result<u64>;
}

pub struct FsDiskSpace;

impl DiskSpace for FsDiskSpace {
    fn available(&self, path: &Path) -> Result<u64> {
        // The storage root may not exist yet; ask about its nearest existing ancestor.
        let existing = path
            .ancestors()
            .find(|p| p.exists())
            .unwrap_or_else(|| Path::new("/"));
        Ok(fs4::available_space(existing)?)
    }
}

/// The storage root is there, or can be created: its parent exists. When an SD card is
/// removed its mount point disappears too, so a root on a missing card is unavailable rather
/// than every ROM on it looking deleted.
pub fn is_available(paths: &Paths) -> bool {
    let root = paths.storage_root();
    root.exists() || root.parent().is_some_and(Path::exists)
}

pub fn unavailable_error(paths: &Paths) -> Error {
    Error::External(format!(
        "The storage at {} isn't available. Insert the SD card that holds your ROMs, or choose another storage location.",
        paths.storage_root().display()
    ))
}

/// The local file matches what Drive reported: present with the same size.
pub fn file_matches(paths: &Paths, rom: &Rom) -> bool {
    paths
        .rom_file(rom.system, &rom.file_name)
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .is_some_and(|m| m.is_file() && m.len() == rom.size)
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct ReconcileReport {
    /// The storage root is missing (SD card removed); nothing was changed.
    pub unavailable: bool,
    /// ROMs whose location was corrected.
    pub fixed: usize,
}

/// Corrects each ROM's location to match the disk: an "on Deck" ROM whose file is missing or
/// the wrong size becomes "in Drive only", a file put there by hand becomes "on Deck", and a
/// download with nothing queued is cleared. Run at start-up.
pub fn reconcile(db: &Database, paths: &Paths) -> Result<ReconcileReport> {
    if !is_available(paths) {
        return Ok(ReconcileReport {
            unavailable: true,
            fixed: 0,
        });
    }
    let mut report = ReconcileReport::default();
    for mut rom in db.roms()? {
        let on_disk = file_matches(paths, &rom);
        let wanted = match &rom.location {
            LocationState::Gone => continue,
            LocationState::Downloading { .. } => {
                if transfer::is_pending(db, &rom.id)? {
                    continue;
                }
                if on_disk {
                    LocationState::OnDeck
                } else {
                    LocationState::DriveOnly
                }
            }
            LocationState::OnDeck | LocationState::DriveOnly => {
                if on_disk {
                    LocationState::OnDeck
                } else {
                    LocationState::DriveOnly
                }
            }
        };
        if wanted != rom.location {
            rom.location = wanted;
            db.upsert_rom(&rom)?;
            report.fixed += 1;
        }
    }
    Ok(report)
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct MoveReport {
    pub moved: usize,
}

/// Moves every downloaded ROM to `<new_root>/roms/…`. Checks every destination first and
/// moves nothing if one holds a different file. Saving the new root in the settings is the
/// caller's job, after this succeeds.
pub fn change_root(db: &Database, paths: &Paths, new_root: &Path) -> Result<MoveReport> {
    let new_paths = paths.clone().with_storage_root(new_root);
    if new_paths.rom_root == paths.rom_root {
        return Ok(MoveReport::default());
    }
    if !transfer::pending(db)?.is_empty() {
        return Err(Error::Invalid(
            "Downloads are in progress. Wait for them to finish, then change the storage location."
                .into(),
        ));
    }
    let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
    for rom in db.roms()? {
        if rom.location != LocationState::OnDeck || !file_matches(paths, &rom) {
            continue;
        }
        let from = paths.rom_file(rom.system, &rom.file_name)?;
        let to = new_paths.rom_file(rom.system, &rom.file_name)?;
        if to.exists() {
            return Err(Error::Invalid(format!(
                "{} already has a file named {}. Move or rename it, then try again.",
                to.parent().unwrap_or(&to).display(),
                rom.file_name
            )));
        }
        moves.push((from, to));
    }
    for (from, to) in &moves {
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir)?;
        }
        move_file(from, to)?;
    }
    Ok(MoveReport { moved: moves.len() })
}

/// Rename, or copy then remove when the destination is on another filesystem (an SD card).
fn move_file(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let tmp = transfer::part_path(to);
    std::fs::copy(from, &tmp)?;
    std::fs::File::open(&tmp)?.sync_all()?;
    std::fs::rename(&tmp, to)?;
    std::fs::remove_file(from)?;
    Ok(())
}

/// "1.4 GB", "820 MB", "12 KB".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} bytes"),
        3 => format!("{value:.1} GB"),
        _ => format!("{value:.0} {}", UNITS[unit]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(human_size(512), "512 bytes");
        assert_eq!(human_size(12 * 1024), "12 KB");
        assert_eq!(human_size(820 * 1024 * 1024), "820 MB");
        assert_eq!(human_size(1_503_238_553), "1.4 GB");
    }
}
