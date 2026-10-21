//! Steam files seam: `shortcuts.vdf`, the grid artwork folder and per-shortcut controller
//! layouts. Everything Romshelf writes into Steam's folders goes through `SteamFiles`.

pub mod apply;
pub mod artwork;
pub mod input;
pub mod shortcuts;
pub mod vdf;

use crate::Result;
use std::path::{Path, PathBuf};

/// Backups of `shortcuts.vdf` kept beside it.
pub const BACKUPS_KEPT: usize = 5;

pub trait SteamFiles: Send + Sync {
    /// Steam user ids that have a `userdata/<id>/config` folder.
    fn user_ids(&self) -> Result<Vec<String>>;
    /// `config/loginusers.vdf`, which says who signed in last.
    fn login_users(&self) -> Result<Option<String>>;
    fn read_shortcuts(&self, user: &str) -> Result<Option<Vec<u8>>>;
    /// Copies the current file to a timestamped backup (keeping the newest five), then writes
    /// through a temporary file renamed into place, so a failure leaves the original.
    fn write_shortcuts(&self, user: &str, bytes: &[u8], stamp: i64) -> Result<()>;
    /// File names in `userdata/<id>/config/grid/`.
    fn grid_files(&self, user: &str) -> Result<Vec<String>>;
    fn read_grid(&self, user: &str, file_name: &str) -> Result<Option<Vec<u8>>>;
    fn write_grid(&self, user: &str, file_name: &str, bytes: &[u8]) -> Result<()>;
    fn remove_grid(&self, user: &str, file_name: &str) -> Result<()>;
    /// The path Steam should use for a grid file (a shortcut's `icon`).
    fn grid_path(&self, user: &str, file_name: &str) -> PathBuf;
    /// Steam's own gamepad layout template, which Romshelf's layout starts from.
    fn controller_template(&self) -> Result<Option<String>>;
    /// `Steam Controller Configs/<id>/config/configset_controller_neptune.vdf`.
    fn read_controller_set(&self, user: &str) -> Result<Option<String>>;
    fn write_controller_set(&self, user: &str, text: &str) -> Result<()>;
    /// A shortcut's layout: `Steam Controller Configs/<id>/config/<key>/controller_neptune.vdf`.
    fn write_controller_layout(&self, user: &str, key: &str, text: &str) -> Result<()>;
    /// A shortcut's layout, if Steam has one under `key`.
    fn read_controller_layout(&self, user: &str, key: &str) -> Result<Option<String>>;
    /// Whether `shortcuts.vdf` can be executed (`None` with no file). Off Linux, always true.
    fn shortcuts_executable(&self, user: &str) -> Result<Option<bool>>;
}

/// The real Steam directory (`~/.steam/steam`), or a temp copy of it in tests.
pub struct FsSteamFiles {
    pub steam_root: PathBuf,
}

impl FsSteamFiles {
    fn config_dir(&self, user: &str) -> PathBuf {
        self.steam_root.join("userdata").join(user).join("config")
    }

    fn controller_dir(&self, user: &str) -> PathBuf {
        self.steam_root
            .join("steamapps/common/Steam Controller Configs")
            .join(user)
            .join("config")
    }
}

/// Writes through a temporary file renamed into place, keeping the file's permissions. A new
/// file gets Steam's (`rwxr-xr-x`).
///
/// Found on the Deck (2026-10-20): another tool there, NonSteamLaunchers' scanner, treats a
/// `shortcuts.vdf` that isn't executable as broken and replaces it with an empty list. A
/// rewrite that dropped the executable bit cost every shortcut.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("romshelf-tmp");
    std::fs::write(&tmp, bytes)?;
    let permissions = match std::fs::metadata(path) {
        Ok(m) => Some(m.permissions()),
        Err(_) => steam_default_permissions(),
    };
    if let Some(p) = permissions {
        std::fs::set_permissions(&tmp, p)?;
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(unix)]
fn steam_default_permissions() -> Option<std::fs::Permissions> {
    use std::os::unix::fs::PermissionsExt;
    Some(std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn steam_default_permissions() -> Option<std::fs::Permissions> {
    None
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn plain_name(file_name: &str) -> Result<()> {
    if file_name.is_empty() || file_name.contains(['/', '\\']) || file_name.starts_with('.') {
        return Err(crate::Error::Invalid(format!(
            "{file_name} isn't a plain file name."
        )));
    }
    Ok(())
}

impl SteamFiles for FsSteamFiles {
    fn user_ids(&self) -> Result<Vec<String>> {
        let dir = self.steam_root.join("userdata");
        let mut ids = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.chars().all(|c| c.is_ascii_digit()) && entry.path().join("config").is_dir() {
                ids.push(name);
            }
        }
        ids.sort();
        Ok(ids)
    }

    fn login_users(&self) -> Result<Option<String>> {
        Ok(
            read_optional(&self.steam_root.join("config/loginusers.vdf"))?
                .map(|b| String::from_utf8_lossy(&b).into_owned()),
        )
    }

    fn read_shortcuts(&self, user: &str) -> Result<Option<Vec<u8>>> {
        read_optional(&self.config_dir(user).join("shortcuts.vdf"))
    }

    fn write_shortcuts(&self, user: &str, bytes: &[u8], stamp: i64) -> Result<()> {
        let dir = self.config_dir(user);
        let path = dir.join("shortcuts.vdf");
        if path.exists() {
            std::fs::copy(
                &path,
                dir.join(format!("shortcuts.vdf.romshelf-{stamp}.bak")),
            )?;
            let mut backups: Vec<PathBuf> = std::fs::read_dir(&dir)?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                        n.starts_with("shortcuts.vdf.romshelf-") && n.ends_with(".bak")
                    })
                })
                .collect();
            backups.sort();
            let excess = backups.len().saturating_sub(BACKUPS_KEPT);
            for old in &backups[..excess] {
                std::fs::remove_file(old)?;
            }
        }
        write_atomic(&path, bytes)
    }

    fn grid_files(&self, user: &str) -> Result<Vec<String>> {
        match std::fs::read_dir(self.config_dir(user).join("grid")) {
            Ok(entries) => Ok(entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    fn read_grid(&self, user: &str, file_name: &str) -> Result<Option<Vec<u8>>> {
        plain_name(file_name)?;
        read_optional(&self.config_dir(user).join("grid").join(file_name))
    }

    fn write_grid(&self, user: &str, file_name: &str, bytes: &[u8]) -> Result<()> {
        plain_name(file_name)?;
        write_atomic(&self.config_dir(user).join("grid").join(file_name), bytes)
    }

    fn remove_grid(&self, user: &str, file_name: &str) -> Result<()> {
        plain_name(file_name)?;
        match std::fs::remove_file(self.config_dir(user).join("grid").join(file_name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn grid_path(&self, user: &str, file_name: &str) -> PathBuf {
        self.config_dir(user).join("grid").join(file_name)
    }

    fn controller_template(&self) -> Result<Option<String>> {
        Ok(read_optional(
            &self
                .steam_root
                .join("controller_base/templates/controller_neptune_gamepad+mouse.vdf"),
        )?
        .map(|b| String::from_utf8_lossy(&b).into_owned()))
    }

    fn read_controller_set(&self, user: &str) -> Result<Option<String>> {
        Ok(read_optional(
            &self
                .controller_dir(user)
                .join("configset_controller_neptune.vdf"),
        )?
        .map(|b| String::from_utf8_lossy(&b).into_owned()))
    }

    fn write_controller_set(&self, user: &str, text: &str) -> Result<()> {
        write_atomic(
            &self
                .controller_dir(user)
                .join("configset_controller_neptune.vdf"),
            text.as_bytes(),
        )
    }

    fn write_controller_layout(&self, user: &str, key: &str, text: &str) -> Result<()> {
        plain_name(key)?;
        write_atomic(
            &self
                .controller_dir(user)
                .join(key)
                .join("controller_neptune.vdf"),
            text.as_bytes(),
        )
    }

    fn read_controller_layout(&self, user: &str, key: &str) -> Result<Option<String>> {
        plain_name(key)?;
        let path = self
            .controller_dir(user)
            .join(key)
            .join("controller_neptune.vdf");
        Ok(read_optional(&path)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
    }

    fn shortcuts_executable(&self, user: &str) -> Result<Option<bool>> {
        let path = self.config_dir(user).join("shortcuts.vdf");
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            Ok(Some(meta.permissions().mode() & 0o111 != 0))
        }
        #[cfg(not(unix))]
        {
            let _ = meta;
            Ok(Some(true))
        }
    }
}

/// The Steam account to write to: the setting if given, else the one signed in most recently
/// (`loginusers.vdf` `timestamp`, or `MostRecent`), else the only one there is.
pub fn active_user(files: &dyn SteamFiles, chosen: Option<&str>) -> Result<Option<String>> {
    let users = files.user_ids()?;
    if let Some(c) = chosen {
        return Ok(users.iter().find(|u| *u == c).cloned());
    }
    if let Some(text) = files.login_users()? {
        let mut best: Option<(u64, String)> = None;
        for (steam_id, block) in login_blocks(&text) {
            let Some(account) = steam_id.checked_sub(76_561_197_960_265_728) else {
                continue;
            };
            let account = account.to_string();
            if !users.contains(&account) {
                continue;
            }
            let rank = if field(&block, "MostRecent").as_deref() == Some("1") {
                u64::MAX
            } else {
                field(&block, "timestamp")
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0)
            };
            if best.as_ref().is_none_or(|(r, _)| rank > *r) {
                best = Some((rank, account));
            }
        }
        if let Some((_, account)) = best {
            return Ok(Some(account));
        }
    }
    Ok(users.into_iter().next_back())
}

/// `"7656…" { … }` blocks of `loginusers.vdf`.
fn login_blocks(text: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("\"7656") {
        let after = &rest[start + 1..];
        let Some(end_id) = after.find('"') else { break };
        let id = after[..end_id].parse::<u64>().ok();
        let Some(open) = after.find('{') else { break };
        let Some(close) = after[open..].find('}') else {
            break;
        };
        if let Some(id) = id {
            out.push((id, after[open..open + close].to_string()));
        }
        rest = &after[open + close..];
    }
    out
}

fn field(block: &str, key: &str) -> Option<String> {
    let lower = block.to_ascii_lowercase();
    let at = lower.find(&format!("\"{}\"", key.to_ascii_lowercase()))?;
    let rest = &block[at + key.len() + 2..];
    let open = rest.find('"')?;
    let close = rest[open + 1..].find('"')?;
    Some(rest[open + 1..open + 1 + close].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steam() -> (tempfile::TempDir, FsSteamFiles) {
        let root = tempfile::tempdir().unwrap();
        let steam = FsSteamFiles {
            steam_root: root.path().into(),
        };
        (root, steam)
    }

    #[test]
    fn write_shortcuts_keeps_five_backups_and_touches_nothing_else() {
        let (root, steam) = steam();
        let cfg = root.path().join("userdata/123/config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("shortcuts.vdf"), b"v0").unwrap();
        std::fs::write(cfg.join("localconfig.vdf"), b"keep").unwrap();
        for i in 1..=7 {
            steam
                .write_shortcuts("123", format!("v{i}").as_bytes(), 1_800_000_000 + i)
                .unwrap();
        }
        assert_eq!(steam.read_shortcuts("123").unwrap().unwrap(), b"v7");
        let mut names: Vec<_> = std::fs::read_dir(&cfg)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names.len(), 2 + BACKUPS_KEPT, "{names:?}");
        assert!(names.contains(&"localconfig.vdf".to_string()));
        assert_eq!(
            std::fs::read(cfg.join("shortcuts.vdf.romshelf-1800000007.bak")).unwrap(),
            b"v6"
        );
        assert!(!cfg.join("shortcuts.vdf.romshelf-1800000002.bak").exists());
        assert_eq!(std::fs::read(cfg.join("localconfig.vdf")).unwrap(), b"keep");
    }

    /// Another tool on the Deck empties a `shortcuts.vdf` that isn't executable.
    #[cfg(unix)]
    #[test]
    fn a_rewrite_keeps_the_files_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (root, steam) = steam();
        let cfg = root.path().join("userdata/123/config");
        std::fs::create_dir_all(&cfg).unwrap();
        let file = cfg.join("shortcuts.vdf");
        std::fs::write(&file, b"v0").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        steam.write_shortcuts("123", b"v1", 1).unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o755);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        steam.write_shortcuts("123", b"v2", 2).unwrap();
        assert_eq!(mode(&file), 0o700, "whatever it was is kept");
        // A new file is executable, as Steam's are.
        std::fs::create_dir_all(root.path().join("userdata/9/config")).unwrap();
        steam.write_shortcuts("9", b"new", 1).unwrap();
        assert_eq!(
            mode(&root.path().join("userdata/9/config/shortcuts.vdf")),
            0o755
        );
    }

    #[test]
    fn a_failed_write_leaves_the_original() {
        let (root, steam) = steam();
        let cfg = root.path().join("userdata/123/config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("shortcuts.vdf"), b"original").unwrap();
        // The temporary file can't be created: a folder is in its way.
        std::fs::create_dir_all(cfg.join("shortcuts.romshelf-tmp")).unwrap();
        assert!(steam.write_shortcuts("123", b"new", 1).is_err());
        assert_eq!(
            std::fs::read(cfg.join("shortcuts.vdf")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn a_missing_config_folder_is_created() {
        let (root, steam) = steam();
        std::fs::create_dir_all(root.path().join("userdata/5")).unwrap();
        steam.write_shortcuts("5", b"x", 1).unwrap();
        assert_eq!(steam.read_shortcuts("5").unwrap().unwrap(), b"x");
    }

    #[test]
    fn grid_names_are_plain() {
        let (_root, steam) = steam();
        assert!(steam.write_grid("1", "../x.png", b"").is_err());
        steam.write_grid("1", "123p.png", b"img").unwrap();
        assert_eq!(steam.grid_files("1").unwrap(), ["123p.png"]);
        steam.remove_grid("1", "123p.png").unwrap();
        steam.remove_grid("1", "123p.png").unwrap();
        assert!(steam.grid_files("1").unwrap().is_empty());
    }

    #[test]
    fn the_account_signed_in_last_is_chosen() {
        let (root, steam) = steam();
        for id in ["85959576", "235036999"] {
            std::fs::create_dir_all(root.path().join(format!("userdata/{id}/config"))).unwrap();
        }
        std::fs::create_dir_all(root.path().join("config")).unwrap();
        // As on the Deck: no MostRecent, a timestamp per account.
        std::fs::write(
            root.path().join("config/loginusers.vdf"),
            "\"users\"\n{\n\t\"76561198046225304\"\n\t{\n\t\t\"AccountName\"\t\t\"a\"\n\t\t\"timestamp\"\t\t\"1792474120\"\n\t}\n\t\"76561198195302727\"\n\t{\n\t\t\"AccountName\"\t\t\"b\"\n\t\t\"timestamp\"\t\t\"1790472585\"\n\t}\n}\n",
        )
        .unwrap();
        assert_eq!(
            active_user(&steam, None).unwrap().as_deref(),
            Some("85959576")
        );
        assert_eq!(
            active_user(&steam, Some("235036999")).unwrap().as_deref(),
            Some("235036999")
        );
        assert_eq!(active_user(&steam, Some("1")).unwrap(), None);
    }
}
