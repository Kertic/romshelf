//! Romshelf's built-in default layout for each emulator and system: the console's buttons where
//! the Deck has them, and hotkeys on the back grips.

use super::model::{Layout, Scope, INPUTS};
use crate::model::{Binding, System};

/// The built-in default for a scope. Every input appears: bound, or cleared.
pub fn built_in(scope: &Scope) -> Layout {
    let game: &[(&str, &str)] = match scope.system {
        System::Gba => &[
            ("dpad", "dpad"),
            ("left_stick", "dpad"),
            ("a", "a"),
            ("b", "b"),
            ("l1", "l"),
            ("r1", "r"),
            ("menu", "start"),
            ("view", "select"),
        ],
        // By position, as on the SNES pad: the Deck's bottom button is the SNES's bottom (B).
        System::Snes => &[
            ("dpad", "dpad"),
            ("left_stick", "dpad"),
            ("a", "b"),
            ("b", "a"),
            ("x", "y"),
            ("y", "x"),
            ("l1", "l"),
            ("r1", "r"),
            ("menu", "start"),
            ("view", "select"),
        ],
        System::N64 => &[
            ("dpad", "dpad"),
            ("left_stick", "stick"),
            ("right_stick", "c_buttons"),
            ("a", "a"),
            ("x", "b"),
            ("l2", "z"),
            ("l1", "l"),
            ("r1", "r"),
            ("menu", "start"),
        ],
        // By position, as on the GameCube pad (and EmuDeck's layout): B to the left, X to the right.
        System::Gamecube => &[
            ("dpad", "dpad"),
            ("left_stick", "stick"),
            ("right_stick", "c_stick"),
            ("a", "a"),
            ("x", "b"),
            ("b", "x"),
            ("y", "y"),
            ("r1", "z"),
            ("l2", "l"),
            ("r2", "r"),
            ("menu", "start"),
        ],
        // By position, as on the DualShock 2: the Deck's bottom button is Cross.
        System::Ps2 => &[
            ("dpad", "dpad"),
            ("left_stick", "left_stick"),
            ("right_stick", "right_stick"),
            ("a", "cross"),
            ("b", "circle"),
            ("x", "square"),
            ("y", "triangle"),
            ("l1", "l1"),
            ("r1", "r1"),
            ("l2", "l2"),
            ("r2", "r2"),
            ("l3", "l3"),
            ("r3", "r3"),
            ("menu", "start"),
            ("view", "select"),
        ],
    };
    let hotkeys: &[(&str, &str)] = match scope.emulator_id.as_str() {
        "retroarch" => &[
            ("l4", "fast_forward"),
            ("r4", "rewind"),
            ("l5", "save_state"),
            ("r5", "load_state"),
        ],
        "dolphin" | "pcsx2" => &[
            ("l4", "fast_forward"),
            ("l5", "save_state"),
            ("r5", "load_state"),
        ],
        _ => &[],
    };
    let mut layout: Layout = INPUTS
        .iter()
        .map(|i| (i.id.to_string(), Binding::Cleared))
        .collect();
    for (input, target) in game.iter().chain(hotkeys) {
        layout.insert((*input).into(), Binding::Bound((*target).into()));
    }
    layout
}
