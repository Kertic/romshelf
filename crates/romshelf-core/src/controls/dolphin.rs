//! A merged layout as Dolphin settings: GameCube pad 1 in `GCPadNew.ini` and the hotkeys in
//! `Hotkeys.ini`, both in Romshelf's own Dolphin user folder.
//!
//! The pad is Steam's virtual gamepad as Dolphin's SDL backend names it (from the settings
//! Dolphin wrote on the Deck). The back grips reach Dolphin as keys on the keyboard device, so
//! those bindings name the device in full.

use super::model::Layout;
use crate::model::Binding;
use std::fmt::Write;

pub const PAD: &str = "SDL/0/Steam Virtual Gamepad";
/// Dolphin's keyboard on Linux (X11, which Dolphin uses under Game Mode too).
pub const KEYBOARD: &str = "XInput2/0/Virtual core pointer";

/// The keys Romshelf's Steam Input layout makes the back grips send, as Dolphin names them.
const GRIP_KEYS: &[(&str, &str)] = &[("l4", "F9"), ("r4", "F10"), ("l5", "F11"), ("r5", "F12")];

const HOTKEYS: &[(&str, &str)] = &[
    (
        "fast_forward",
        "Emulation Speed/Disable Emulation Speed Limit",
    ),
    ("save_state", "Save State/Save to Selected Slot"),
    ("load_state", "Load State/Load from Selected Slot"),
    ("quit", "General/Exit"),
];

const CALIBRATION: &str = "100.00 141.42 100.00 141.42 100.00 141.42 100.00 141.42";

/// A button input as a Dolphin control expression.
fn control(input: &str) -> Option<String> {
    let pad = match input {
        "a" => "Button S",
        "b" => "Button E",
        "x" => "Button W",
        "y" => "Button N",
        "view" => "Back",
        "menu" => "Start",
        "l3" => "Thumb L",
        "r3" => "Thumb R",
        "l1" => "Shoulder L",
        "r1" => "Shoulder R",
        "l2" => "Trigger L",
        "r2" => "Trigger R",
        other => {
            let key = GRIP_KEYS.iter().find(|(i, _)| *i == other)?.1;
            return Some(format!("`{KEYBOARD}:{key}`"));
        }
    };
    Some(format!("`{pad}`"))
}

/// Up, down, left, right of a directional input.
fn directions(input: &str) -> Option<[&'static str; 4]> {
    Some(match input {
        "dpad" => ["Pad N", "Pad S", "Pad W", "Pad E"],
        "left_stick" => ["Axis 1-", "Axis 1+", "Axis 0-", "Axis 0+"],
        "right_stick" => ["Axis 4-", "Axis 4+", "Axis 3-", "Axis 3+"],
        _ => return None,
    })
}

/// Every control bound to `target`, joined with `|` (Dolphin's "either").
fn sources(layout: &Layout, target: &str) -> Vec<String> {
    layout
        .iter()
        .filter(|(_, b)| matches!(b, Binding::Bound(t) if t == target))
        .filter_map(|(input, _)| control(input))
        .collect()
}

fn dir_sources(layout: &Layout, target: &str) -> [Vec<String>; 4] {
    let mut out: [Vec<String>; 4] = Default::default();
    for (input, b) in layout {
        if !matches!(b, Binding::Bound(t) if t == target) {
            continue;
        }
        if let Some(dirs) = directions(input) {
            for (slot, d) in out.iter_mut().zip(dirs) {
                slot.push(format!("`{d}`"));
            }
        }
    }
    out
}

/// `[GCPad1]` for a layout.
pub fn gcpad(layout: &Layout) -> String {
    let mut out = format!("[GCPad1]\nDevice = {PAD}\n");
    let mut line = |key: &str, values: &[String]| {
        let _ = writeln!(out, "{key} = {}", values.join("|"));
    };
    for (key, target) in [
        ("Buttons/A", "a"),
        ("Buttons/B", "b"),
        ("Buttons/X", "x"),
        ("Buttons/Y", "y"),
        ("Buttons/Z", "z"),
        ("Buttons/Start", "start"),
    ] {
        line(key, &sources(layout, target));
    }
    for (section, target) in [("Main Stick", "stick"), ("C-Stick", "c_stick")] {
        let [up, down, left, right] = dir_sources(layout, target);
        line(&format!("{section}/Up"), &up);
        line(&format!("{section}/Down"), &down);
        line(&format!("{section}/Left"), &left);
        line(&format!("{section}/Right"), &right);
        line(
            &format!("{section}/Calibration"),
            &[CALIBRATION.to_string()],
        );
    }
    for (key, target) in [
        ("Triggers/L", "l"),
        ("Triggers/R", "r"),
        ("Triggers/L-Analog", "l"),
        ("Triggers/R-Analog", "r"),
    ] {
        line(key, &sources(layout, target));
    }
    let [up, down, left, right] = dir_sources(layout, "dpad");
    line("D-Pad/Up", &up);
    line("D-Pad/Down", &down);
    line("D-Pad/Left", &left);
    line("D-Pad/Right", &right);
    line("Rumble/Motor", &["Strong".to_string()]);
    out
}

/// `GCPadNew.ini` with pad 1 replaced and any other pads kept as they were.
pub fn gcpad_file(existing: &str, layout: &Layout) -> String {
    let mut out = gcpad(layout);
    let mut keep = false;
    for line in existing.lines() {
        if line.starts_with('[') {
            keep = line.trim() != "[GCPad1]";
        }
        if keep {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `Hotkeys.ini`: only the layout's hotkeys. Dolphin's own keyboard defaults name keys on the
/// pad device, which has none, so they stay inert.
pub fn hotkeys(layout: &Layout) -> String {
    let mut out = format!("[Hotkeys]\nDevice = {PAD}\n");
    for (target, key) in HOTKEYS {
        let _ = writeln!(out, "{key} = {}", sources(layout, target).join("|"));
    }
    out
}
