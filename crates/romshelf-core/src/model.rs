//! Data model shared by every layer.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// A console system. The id doubles as the Drive folder name under `roms/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum System {
    Snes,
    Gba,
    N64,
    Ps2,
    Gamecube,
}

impl System {
    pub const ALL: [System; 5] = [
        System::Snes,
        System::Gba,
        System::N64,
        System::Ps2,
        System::Gamecube,
    ];

    pub fn id(self) -> &'static str {
        match self {
            System::Snes => "snes",
            System::Gba => "gba",
            System::N64 => "n64",
            System::Ps2 => "ps2",
            System::Gamecube => "gamecube",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            System::Snes => "SNES",
            System::Gba => "GBA",
            System::N64 => "N64",
            System::Ps2 => "PS2",
            System::Gamecube => "GameCube",
        }
    }

    /// Cover colour from the UI guide.
    pub fn colour(self) -> &'static str {
        match self {
            System::Snes => "#2B3F6B",
            System::Gba => "#1F6B62",
            System::N64 => "#5C5A22",
            System::Ps2 => "#6B3A2B",
            System::Gamecube => "#4A3470",
        }
    }

    /// Recognized file extensions, lower case, without the dot (stage 2's table).
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            System::Snes => &["sfc", "smc", "zip", "7z"],
            System::Gba => &["gba", "zip", "7z"],
            System::N64 => &["z64", "n64", "v64", "zip", "7z"],
            System::Ps2 => &["iso", "chd"],
            System::Gamecube => &["iso", "rvz", "gcz", "ciso"],
        }
    }

    /// Whether a file in this system's folder has one of its extensions, in any case.
    pub fn recognizes(self, file_name: &str) -> bool {
        file_name.rsplit_once('.').is_some_and(|(_, ext)| {
            self.extensions()
                .contains(&ext.to_ascii_lowercase().as_str())
        })
    }

    pub fn from_id(id: &str) -> Option<System> {
        System::ALL.into_iter().find(|s| s.id() == id)
    }
}

impl fmt::Display for System {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Stable short id of the form `<system>-<8 hex>`, derived from the Drive file id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RomId(pub String);

impl RomId {
    pub fn derive(system: System, drive_file_id: &str) -> RomId {
        RomId(format!(
            "{}-{:08x}",
            system.id(),
            fnv1a32(drive_file_id.as_bytes())
        ))
    }

    /// The id to use when `derive` collides with another file's: hashes the file id with a
    /// counter, so every device that sees the same collision picks the same id.
    pub fn derive_salted(system: System, drive_file_id: &str, salt: u32) -> RomId {
        if salt == 0 {
            return RomId::derive(system, drive_file_id);
        }
        RomId::derive(system, &format!("{drive_file_id}#{salt}"))
    }

    pub fn parse(s: &str) -> Option<RomId> {
        let (sys, hex) = s.rsplit_once('-')?;
        System::from_id(sys)?;
        (hex.len() == 8
            && hex
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
        .then(|| RomId(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for b in bytes {
        hash ^= u32::from(*b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Where a ROM's bytes currently are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LocationState {
    DriveOnly,
    Downloading {
        percent: u8,
    },
    OnDeck,
    /// Removed from Drive; local data such as saves is kept.
    Gone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rom {
    pub id: RomId,
    pub drive_file_id: String,
    pub system: System,
    pub file_name: String,
    pub display_name: String,
    pub size: u64,
    pub checksum: Option<String>,
    pub location: LocationState,
    /// False when the extension is not one the system recognizes.
    pub recognized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Emulator {
    pub id: String,
    pub name: String,
    pub flatpak_id: String,
    pub systems: Vec<System>,
    pub installed: bool,
}

/// A binding in a control profile: either a target action or explicitly cleared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "action", rename_all = "snake_case")]
pub enum Binding {
    Bound(String),
    Cleared,
}

/// An emulator default (`rom_id` is `None`) or a per-game difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlProfile {
    pub emulator_id: String,
    pub rom_id: Option<RomId>,
    /// Deck input name → binding.
    pub bindings: BTreeMap<String, Binding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtSlot {
    Capsule,
    WideCapsule,
    Hero,
    Logo,
    Icon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub rom_id: RomId,
    pub app_id: u32,
    pub name: String,
    pub target: String,
    pub launch_options: String,
    pub artwork: BTreeMap<ArtSlot, String>,
}

impl Shortcut {
    /// Launch options every Romshelf shortcut uses. Shortcuts never point at an emulator.
    pub fn launch_options_for(rom_id: &RomId) -> String {
        format!("launch --id {rom_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_round_trip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + fmt::Debug,
    {
        let text = serde_json::to_string(value).unwrap();
        let back: T = serde_json::from_str(&text).unwrap();
        assert_eq!(&back, value, "{text}");
    }

    #[test]
    fn rom_id_is_stable_and_well_formed() {
        let a = RomId::derive(System::Gba, "1AbC");
        assert_eq!(a, RomId::derive(System::Gba, "1AbC"));
        assert_ne!(a, RomId::derive(System::Gba, "1AbD"));
        assert!(a.as_str().starts_with("gba-"));
        assert_eq!(RomId::parse(a.as_str()), Some(a));
        assert_eq!(RomId::parse("gba-1234"), None);
        assert_eq!(RomId::parse("xyz-12345678"), None);
        assert_eq!(RomId::parse("gba-1234567G"), None);
    }

    #[test]
    fn every_type_round_trips_through_json() {
        for s in System::ALL {
            json_round_trip(&s);
            assert_eq!(System::from_id(s.id()), Some(s));
        }
        for loc in [
            LocationState::DriveOnly,
            LocationState::Downloading { percent: 64 },
            LocationState::OnDeck,
            LocationState::Gone,
        ] {
            json_round_trip(&loc);
        }
        let id = RomId::derive(System::N64, "kite");
        json_round_trip(&Rom {
            id: id.clone(),
            drive_file_id: "kite".into(),
            system: System::N64,
            file_name: "Kite Rally.z64".into(),
            display_name: "Kite Rally".into(),
            size: 8,
            checksum: None,
            location: LocationState::OnDeck,
            recognized: true,
        });
        json_round_trip(&Emulator {
            id: "dolphin".into(),
            name: "Dolphin".into(),
            flatpak_id: "org.DolphinEmu.dolphin-emu".into(),
            systems: vec![System::Gamecube],
            installed: false,
        });
        let mut bindings = BTreeMap::new();
        bindings.insert("L4".into(), Binding::Bound("fast_forward".into()));
        bindings.insert("R4".into(), Binding::Cleared);
        json_round_trip(&ControlProfile {
            emulator_id: "retroarch".into(),
            rom_id: Some(id.clone()),
            bindings,
        });
        let mut artwork = BTreeMap::new();
        artwork.insert(ArtSlot::Hero, "1_hero.png".into());
        json_round_trip(&Shortcut {
            launch_options: Shortcut::launch_options_for(&id),
            rom_id: id,
            app_id: 42,
            name: "Kite Rally".into(),
            target: "romshelf".into(),
            artwork,
        });
    }

    #[test]
    fn shortcuts_launch_romshelf_by_id() {
        let id = RomId("gba-0a1b2c3d".into());
        assert_eq!(
            Shortcut::launch_options_for(&id),
            "launch --id gba-0a1b2c3d"
        );
    }
}

/// Where a game's save stands against Drive.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveSync {
    /// The save set's hash when this Deck and Drive last agreed; `None` before the first time.
    pub last_synced_hash: Option<String>,
    pub last_backup_at: Option<i64>,
    pub last_backup_device: Option<String>,
    /// A backup is owed: the game was started and its save hasn't been backed up since (the
    /// game is still running, Steam ended Romshelf, or Drive couldn't be reached).
    pub pending: bool,
    /// Why the last backup failed, for the Library.
    pub last_error: Option<String>,
}
