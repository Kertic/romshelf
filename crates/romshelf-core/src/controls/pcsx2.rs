//! A merged layout as PCSX2 settings: pad 1 (a DualShock 2) in `[Pad1]` and the hotkeys in
//! `[Hotkeys]` of `PCSX2.ini`, in Romshelf's own PCSX2 data folder.
//!
//! Names from PCSX2's source: the DualShock 2's bindings (`PadDualshock2.cpp`), SDL controls as
//! `SDL-0/FaceSouth` and axes as `SDL-0/+LeftTrigger` (`SDLInputSource.cpp`), keys as
//! `Keyboard/F9` and several bindings for one input as repeated keys (`InputManager.cpp`), the
//! hotkey names (`Hotkeys.cpp`). The back grips reach PCSX2 as the keys Romshelf's Steam Input
//! layout gives them.

use super::model::Layout;
use crate::ini;
use crate::model::Binding;

/// Steam's virtual gamepad, the first SDL pad PCSX2 sees (Confirm on the Deck).
const PAD: &str = "SDL-0";

const GRIP_KEYS: &[(&str, &str)] = &[("l4", "F9"), ("r4", "F10"), ("l5", "F11"), ("r5", "F12")];

/// PS2 buttons and the `[Pad1]` key each is written under.
const BUTTONS: &[(&str, &str)] = &[
    ("cross", "Cross"),
    ("circle", "Circle"),
    ("square", "Square"),
    ("triangle", "Triangle"),
    ("start", "Start"),
    ("select", "Select"),
    ("l1", "L1"),
    ("r1", "R1"),
    ("l2", "L2"),
    ("r2", "R2"),
    ("l3", "L3"),
    ("r3", "R3"),
];

/// Directional PS2 targets and their up, down, left, right keys.
const DIRECTIONAL: &[(&str, [&str; 4])] = &[
    ("dpad", ["Up", "Down", "Left", "Right"]),
    ("left_stick", ["LUp", "LDown", "LLeft", "LRight"]),
    ("right_stick", ["RUp", "RDown", "RLeft", "RRight"]),
];

const HOTKEYS: &[(&str, &str)] = &[
    ("fast_forward", "HoldTurbo"),
    ("save_state", "SaveStateToSlot"),
    ("load_state", "LoadStateFromSlot"),
    ("quit", "ShutdownVM"),
];

/// A Deck button as a PCSX2 binding.
fn control(input: &str) -> Option<String> {
    let name = match input {
        "a" => "FaceSouth",
        "b" => "FaceEast",
        "x" => "FaceWest",
        "y" => "FaceNorth",
        "view" => "Back",
        "menu" => "Start",
        "l3" => "LeftStick",
        "r3" => "RightStick",
        "l1" => "LeftShoulder",
        "r1" => "RightShoulder",
        "l2" => "+LeftTrigger",
        "r2" => "+RightTrigger",
        other => {
            let key = GRIP_KEYS.iter().find(|(i, _)| *i == other)?.1;
            return Some(format!("Keyboard/{key}"));
        }
    };
    Some(format!("{PAD}/{name}"))
}

/// Up, down, left, right of a directional Deck input.
fn directions(input: &str) -> Option<[&'static str; 4]> {
    Some(match input {
        "dpad" => ["DPadUp", "DPadDown", "DPadLeft", "DPadRight"],
        "left_stick" => ["-LeftY", "+LeftY", "-LeftX", "+LeftX"],
        "right_stick" => ["-RightY", "+RightY", "-RightX", "+RightX"],
        _ => return None,
    })
}

fn bound_to<'a>(layout: &'a Layout, target: &'a str) -> impl Iterator<Item = &'a String> {
    layout
        .iter()
        .filter(move |(_, b)| matches!(b, Binding::Bound(t) if t == target))
        .map(|(input, _)| input)
}

/// `[Pad1]`'s keys for a layout, in a fixed order; an input bound twice is listed twice.
fn pad(layout: &Layout) -> Vec<(String, String)> {
    let mut out = vec![("Type".to_string(), "DualShock2".to_string())];
    for (target, key) in BUTTONS {
        for input in bound_to(layout, target) {
            if let Some(c) = control(input) {
                out.push(((*key).into(), c));
            }
        }
    }
    for (target, keys) in DIRECTIONAL {
        for input in bound_to(layout, target) {
            if let Some(dirs) = directions(input) {
                for (key, d) in keys.iter().zip(dirs) {
                    out.push(((*key).into(), format!("{PAD}/{d}")));
                }
            }
        }
    }
    out
}

fn hotkeys(layout: &Layout) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (target, key) in HOTKEYS {
        for input in bound_to(layout, target) {
            if let Some(c) = control(input) {
                out.push(((*key).into(), c));
            }
        }
    }
    out
}

/// `PCSX2.ini` with pad 1 and the hotkeys replaced by the layout's, SDL input on, and
/// everything else as it was.
pub fn settings(existing: &str, layout: &Layout) -> String {
    let text = ini::replace_section(existing, "Pad1", &refs(&pad(layout)));
    let text = ini::replace_section(&text, "Hotkeys", &refs(&hotkeys(layout)));
    ini::set(&text, "InputSources", "SDL", "true")
}

fn refs(pairs: &[(String, String)]) -> Vec<(&str, &str)> {
    pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}
