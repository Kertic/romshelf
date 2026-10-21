//! Romshelf's entries in `shortcuts.vdf`: what they contain, and bringing the file in line with
//! the games that should be in Steam.
//!
//! An entry is Romshelf's when it carries the tag `Romshelf` and its app ID is one Romshelf
//! derives for a game. Every other entry is left exactly as it is.

use super::vdf::{self, Map, Value};
use crate::{Error, Result};

/// The tag on every shortcut Romshelf creates.
pub const TAG: &str = "Romshelf";

/// What a game's shortcut should contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desired {
    pub app_id: u32,
    pub name: String,
    /// The AppImage's absolute path (written quoted, as Steam does).
    pub exe: String,
    /// Its folder.
    pub start_dir: String,
    pub launch_options: String,
    /// The icon file in Steam's grid folder; empty for none.
    pub icon: String,
}

/// A shortcut's app ID: fixed for the game (from its rom-id), so a new name keeps the artwork,
/// different for two games with the same name, and with the high bit Steam sets on shortcuts.
pub fn app_id_for(rom_id: &str) -> u32 {
    crc32(format!("romshelf/{rom_id}").as_bytes()) | 0x8000_0000
}

/// CRC-32 (IEEE), the checksum Steam's own shortcut IDs are built on.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in bytes {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn str_value(s: &str) -> Value {
    Value::Str(s.as_bytes().to_vec())
}

/// A new entry with every field Steam writes, in Steam's order.
fn new_entry(d: &Desired) -> Map {
    let tags: Map = vec![(b"0".to_vec(), str_value(TAG))];
    vec![
        (b"appid".to_vec(), Value::Int(d.app_id)),
        (b"AppName".to_vec(), str_value(&d.name)),
        (b"Exe".to_vec(), str_value(&quoted(&d.exe))),
        (b"StartDir".to_vec(), str_value(&quoted(&d.start_dir))),
        (b"icon".to_vec(), str_value(&d.icon)),
        (b"ShortcutPath".to_vec(), str_value("")),
        (b"LaunchOptions".to_vec(), str_value(&d.launch_options)),
        (b"IsHidden".to_vec(), Value::Int(0)),
        (b"AllowDesktopConfig".to_vec(), Value::Int(1)),
        (b"AllowOverlay".to_vec(), Value::Int(1)),
        (b"OpenVR".to_vec(), Value::Int(0)),
        (b"Devkit".to_vec(), Value::Int(0)),
        (b"DevkitGameID".to_vec(), str_value("")),
        (b"DevkitOverrideAppID".to_vec(), Value::Int(0)),
        (b"LastPlayTime".to_vec(), Value::Int(0)),
        (b"FlatpakAppID".to_vec(), str_value("")),
        (b"sortas".to_vec(), str_value("")),
        (b"tags".to_vec(), Value::Map(tags)),
    ]
}

fn quoted(path: &str) -> String {
    format!("\"{path}\"")
}

/// The fields Romshelf owns in its entries; anything else (play time, Steam's own additions)
/// is kept.
fn update_entry(entry: &mut Map, d: &Desired) -> bool {
    let before = entry.clone();
    vdf::set(entry, "AppName", str_value(&d.name));
    vdf::set(entry, "Exe", str_value(&quoted(&d.exe)));
    vdf::set(entry, "StartDir", str_value(&quoted(&d.start_dir)));
    vdf::set(entry, "icon", str_value(&d.icon));
    vdf::set(entry, "LaunchOptions", str_value(&d.launch_options));
    *entry != before
}

fn has_tag(entry: &Map) -> bool {
    match vdf::get(entry, "tags") {
        Some(Value::Map(tags)) => tags
            .iter()
            .any(|(_, v)| matches!(v, Value::Str(s) if s.as_slice() == TAG.as_bytes())),
        _ => false,
    }
}

/// One of Romshelf's entries as found in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub app_id: u32,
    pub name: String,
    pub launch_options: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// The new file, or `None` when nothing needs to change.
    pub bytes: Option<Vec<u8>>,
    pub added: Vec<u32>,
    pub updated: Vec<u32>,
    pub removed: Vec<u32>,
}

fn shortcuts_of(root: &mut Map) -> Result<&mut Map> {
    if vdf::get(root, "shortcuts").is_none() {
        root.push((b"shortcuts".to_vec(), Value::Map(Map::new())));
    }
    match root
        .iter_mut()
        .find(|(k, _)| k.eq_ignore_ascii_case(b"shortcuts"))
    {
        Some((_, Value::Map(m))) => Ok(m),
        _ => Err(Error::Invalid(
            "Steam's shortcuts file has no list of shortcuts, so Romshelf left it alone.".into(),
        )),
    }
}

/// Romshelf's entries in a shortcuts file.
pub fn list(bytes: Option<&[u8]>) -> Result<Vec<Found>> {
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    let mut root = vdf::parse(bytes)?;
    let shortcuts = shortcuts_of(&mut root)?;
    Ok(shortcuts
        .iter()
        .filter_map(|(_, v)| match v {
            Value::Map(e) if has_tag(e) => Some(Found {
                app_id: vdf::int_of(e, "appid")?,
                name: vdf::str_of(e, "AppName").unwrap_or_default(),
                launch_options: vdf::str_of(e, "LaunchOptions").unwrap_or_default(),
            }),
            _ => None,
        })
        .collect())
}

/// Brings Romshelf's entries in line with `desired`, given every app ID Romshelf may own
/// (`ours`: the games it knows, shown or not). An entry is touched only when it carries
/// Romshelf's tag and its app ID is in `ours` or `desired`. Running it again changes nothing.
pub fn reconcile(existing: Option<&[u8]>, desired: &[Desired], ours: &[u32]) -> Result<Reconciled> {
    let mut root = match existing {
        Some(b) if !b.is_empty() => vdf::parse(b)?,
        _ => Map::new(),
    };
    let original = vdf::write(&root);
    let shortcuts = shortcuts_of(&mut root)?;
    let mut out = Reconciled::default();
    let owned = |id: u32| ours.contains(&id) || desired.iter().any(|d| d.app_id == id);

    // Updates and removals, in place.
    let mut kept = Map::new();
    let mut seen = Vec::new();
    for (key, value) in std::mem::take(shortcuts) {
        let Value::Map(mut entry) = value else {
            kept.push((key, value));
            continue;
        };
        let id = vdf::int_of(&entry, "appid");
        match id {
            Some(id) if has_tag(&entry) && owned(id) => {
                match desired.iter().find(|d| d.app_id == id) {
                    Some(d) if !seen.contains(&id) => {
                        seen.push(id);
                        if update_entry(&mut entry, d) {
                            out.updated.push(id);
                        }
                        kept.push((key, Value::Map(entry)));
                    }
                    // Not wanted any more, or a second copy of one.
                    _ => out.removed.push(id),
                }
            }
            _ => kept.push((key, Value::Map(entry))),
        }
    }
    for d in desired {
        if !seen.contains(&d.app_id) {
            kept.push((Vec::new(), Value::Map(new_entry(d))));
            out.added.push(d.app_id);
        }
    }
    // Steam numbers its entries 0, 1, 2…
    for (i, (key, _)) in kept.iter_mut().enumerate() {
        *key = i.to_string().into_bytes();
    }
    *shortcuts = kept;

    let bytes = vdf::write(&root);
    if bytes != original {
        out.bytes = Some(bytes);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn app_ids_are_stable_distinct_and_marked_as_shortcuts() {
        let a = app_id_for("gba-8f031392");
        assert_eq!(a, app_id_for("gba-8f031392"));
        assert_ne!(a, app_id_for("gba-8f031393"));
        assert!(a & 0x8000_0000 != 0);
    }
}
