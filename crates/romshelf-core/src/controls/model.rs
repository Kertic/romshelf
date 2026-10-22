//! Inputs (what the emulator can see from the Deck), targets (what a game or emulator does with
//! them) and layouts (input → target).

use crate::model::{Binding, System};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A Deck input as Romshelf names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Input {
    pub id: &'static str,
    pub label: &'static str,
    /// D-pad and sticks: can only drive a directional target.
    pub directional: bool,
    /// Back grips: not seen by emulators unless Steam Input sends something for them.
    pub back_grip: bool,
}

const fn input(id: &'static str, label: &'static str) -> Input {
    Input {
        id,
        label,
        directional: false,
        back_grip: false,
    }
}

const fn stick(id: &'static str, label: &'static str) -> Input {
    Input {
        id,
        label,
        directional: true,
        back_grip: false,
    }
}

const fn grip(id: &'static str, label: &'static str) -> Input {
    Input {
        id,
        label,
        directional: false,
        back_grip: true,
    }
}

/// Every input, in the order the editor lists them. Under Steam Input's default gamepad layout,
/// all but the back grips reach the emulator as a standard gamepad.
pub const INPUTS: &[Input] = &[
    stick("dpad", "D-pad"),
    stick("left_stick", "Left stick"),
    stick("right_stick", "Right stick"),
    input("a", "A"),
    input("b", "B"),
    input("x", "X"),
    input("y", "Y"),
    input("l1", "L1"),
    input("r1", "R1"),
    input("l2", "L2"),
    input("r2", "R2"),
    input("l3", "L3"),
    input("r3", "R3"),
    input("menu", "Menu"),
    input("view", "View"),
    grip("l4", "L4"),
    grip("r4", "R4"),
    grip("l5", "L5"),
    grip("r5", "R5"),
];

pub fn input_by_id(id: &str) -> Option<&'static Input> {
    INPUTS.iter().find(|i| i.id == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// A button or stick of the console's own pad.
    Game,
    /// Something the emulator does: fast forward, save state.
    Hotkey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Target {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: TargetKind,
    /// A direction pad or stick: needs a directional input.
    pub directional: bool,
}

const fn button(id: &'static str, label: &'static str) -> Target {
    Target {
        id,
        label,
        kind: TargetKind::Game,
        directional: false,
    }
}

const fn pad(id: &'static str, label: &'static str) -> Target {
    Target {
        id,
        label,
        kind: TargetKind::Game,
        directional: true,
    }
}

const fn hotkey(id: &'static str, label: &'static str) -> Target {
    Target {
        id,
        label,
        kind: TargetKind::Hotkey,
        directional: false,
    }
}

const GBA: &[Target] = &[
    pad("dpad", "D-pad"),
    button("a", "A"),
    button("b", "B"),
    button("l", "L shoulder"),
    button("r", "R shoulder"),
    button("start", "Start"),
    button("select", "Select"),
];

const SNES: &[Target] = &[
    pad("dpad", "D-pad"),
    button("a", "A"),
    button("b", "B"),
    button("x", "X"),
    button("y", "Y"),
    button("l", "L shoulder"),
    button("r", "R shoulder"),
    button("start", "Start"),
    button("select", "Select"),
];

const N64: &[Target] = &[
    pad("dpad", "D-pad"),
    pad("stick", "Control stick"),
    pad("c_buttons", "C buttons"),
    button("a", "A"),
    button("b", "B"),
    button("z", "Z trigger"),
    button("l", "L shoulder"),
    button("r", "R shoulder"),
    button("start", "Start"),
];

const GAMECUBE: &[Target] = &[
    pad("dpad", "D-pad"),
    pad("stick", "Control stick"),
    pad("c_stick", "C-stick"),
    button("a", "A"),
    button("b", "B"),
    button("x", "X"),
    button("y", "Y"),
    button("z", "Z"),
    button("l", "L trigger"),
    button("r", "R trigger"),
    button("start", "Start"),
];

const PS2: &[Target] = &[
    pad("dpad", "D-pad"),
    pad("left_stick", "Left stick"),
    pad("right_stick", "Right stick"),
    button("cross", "Cross"),
    button("circle", "Circle"),
    button("square", "Square"),
    button("triangle", "Triangle"),
    button("l1", "L1"),
    button("r1", "R1"),
    button("l2", "L2"),
    button("r2", "R2"),
    button("l3", "L3"),
    button("r3", "R3"),
    button("start", "Start"),
    button("select", "Select"),
];

const RETROARCH_HOTKEYS: &[Target] = &[
    hotkey("fast_forward", "Fast forward"),
    hotkey("rewind", "Rewind"),
    hotkey("save_state", "Save state"),
    hotkey("load_state", "Load state"),
    hotkey("menu", "Open menu"),
    hotkey("quit", "Quit game"),
];

const DOLPHIN_HOTKEYS: &[Target] = &[
    hotkey("fast_forward", "Fast forward"),
    hotkey("save_state", "Save state"),
    hotkey("load_state", "Load state"),
    hotkey("quit", "Quit game"),
];

const PCSX2_HOTKEYS: &[Target] = &[
    hotkey("fast_forward", "Fast forward"),
    hotkey("save_state", "Save state"),
    hotkey("load_state", "Load state"),
    hotkey("quit", "Quit game"),
];

/// The console's own buttons per system.
pub fn game_targets(system: System) -> &'static [Target] {
    match system {
        System::Gba => GBA,
        System::Snes => SNES,
        System::N64 => N64,
        System::Gamecube => GAMECUBE,
        System::Ps2 => PS2,
    }
}

/// What each emulator can do on a button.
pub fn hotkey_targets(emulator_id: &str) -> &'static [Target] {
    match emulator_id {
        "retroarch" => RETROARCH_HOTKEYS,
        "dolphin" => DOLPHIN_HOTKEYS,
        "pcsx2" => PCSX2_HOTKEYS,
        _ => &[],
    }
}

/// Where a layout applies: an emulator for one system (RetroArch runs several, each with its
/// own buttons).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Scope {
    pub emulator_id: String,
    pub system: System,
}

impl Scope {
    pub fn new(emulator_id: &str, system: System) -> Scope {
        Scope {
            emulator_id: emulator_id.into(),
            system,
        }
    }

    /// `retroarch/gba`: how a scope is stored.
    pub fn key(&self) -> String {
        format!("{}/{}", self.emulator_id, self.system.id())
    }

    pub fn from_key(key: &str) -> Option<Scope> {
        let (emulator, system) = key.split_once('/')?;
        Some(Scope::new(emulator, System::from_id(system)?))
    }

    pub fn targets(&self) -> impl Iterator<Item = &'static Target> {
        game_targets(self.system)
            .iter()
            .chain(hotkey_targets(&self.emulator_id))
    }

    pub fn target(&self, id: &str) -> Option<&'static Target> {
        self.targets().find(|t| t.id == id)
    }
}

/// Input id → binding. An emulator default binds or clears every input; a game's differences
/// hold only the inputs it changes.
pub type Layout = BTreeMap<String, Binding>;

/// Builds a layout from (input, target) pairs, as a profile file or the editor gives them.
/// Binding one input twice is an error; two inputs may share a target.
pub fn from_pairs(pairs: &[(&str, Option<&str>)]) -> Result<Layout> {
    let mut layout = Layout::new();
    for (input, target) in pairs {
        let binding = match target {
            Some(t) => Binding::Bound((*t).into()),
            None => Binding::Cleared,
        };
        if layout.insert((*input).into(), binding).is_some() {
            return Err(Error::Invalid(format!(
                "{} is bound twice. An input can do only one thing.",
                input_by_id(input).map_or(*input, |i| i.label)
            )));
        }
    }
    Ok(layout)
}

/// Checks every entry: known inputs and targets for the scope, and directions only from the
/// D-pad or a stick.
pub fn validate(scope: &Scope, layout: &Layout) -> Result<()> {
    for (input_id, binding) in layout {
        let input = input_by_id(input_id)
            .ok_or_else(|| Error::Invalid(format!("{input_id} isn't a Deck input.")))?;
        let Binding::Bound(target_id) = binding else {
            continue;
        };
        let target = scope.target(target_id).ok_or_else(|| {
            Error::Invalid(format!(
                "{target_id} isn't something {} can do for {}.",
                scope.emulator_id,
                scope.system.label()
            ))
        })?;
        if target.directional != input.directional {
            return Err(Error::Invalid(if target.directional {
                format!(
                    "{} needs the D-pad or a stick, not {}.",
                    target.label, input.label
                )
            } else {
                format!(
                    "{} is a button; bind it to a button, not {}.",
                    target.label, input.label
                )
            }));
        }
    }
    Ok(())
}
