//! A merged layout as RetroArch settings, appended after Romshelf's own for one game.
//!
//! Inputs are numbered as RetroArch's SDL2 driver sees Steam's virtual gamepad (checked against
//! its `Steam Virtual Gamepad.cfg` autoconfig on the Deck). The back grips reach RetroArch as
//! the keys Romshelf's Steam Input layout makes them send.
//!
//! Every RetroPad bind and every hotkey is written, so nothing falls back to the device's
//! autoconfig or the user's own hotkeys (EmuDeck's need View held, and its button hotkeys would
//! fire on a single press without it).

use super::model::Layout;
use crate::model::{Binding, System};
use std::collections::BTreeMap;
use std::fmt::Write;

/// The keys Romshelf's Steam Input layout makes the back grips send.
pub const GRIP_KEYS: &[(&str, &str)] = &[("l4", "f9"), ("r4", "f10"), ("l5", "f11"), ("r5", "f12")];

/// RetroArch's hotkey setting for each Romshelf hotkey.
const HOTKEYS: &[(&str, &str)] = &[
    ("fast_forward", "input_toggle_fast_forward"),
    ("rewind", "input_rewind"),
    ("save_state", "input_save_state"),
    ("load_state", "input_load_state"),
    ("menu", "input_menu_toggle"),
    ("quit", "input_exit_emulator"),
];

/// Every RetroArch hotkey, cleared unless the layout binds it (from RetroArch 1.19.1's own
/// `retroarch.cfg`).
pub const ALL_HOTKEYS: &[&str] = &[
    "input_ai_service",
    "input_audio_mute",
    "input_cheat_index_minus",
    "input_cheat_index_plus",
    "input_cheat_toggle",
    "input_close_content",
    "input_desktop_menu_toggle",
    "input_disk_eject_toggle",
    "input_disk_next",
    "input_disk_prev",
    "input_enable_hotkey",
    "input_exit_emulator",
    "input_fps_toggle",
    "input_frame_advance",
    "input_game_focus_toggle",
    "input_grab_mouse_toggle",
    "input_halt_replay",
    "input_hold_fast_forward",
    "input_hold_slowmotion",
    "input_load_state",
    "input_menu_toggle",
    "input_movie_record_toggle",
    "input_netplay_fade_chat_toggle",
    "input_netplay_game_watch",
    "input_netplay_host_toggle",
    "input_netplay_ping_toggle",
    "input_osk_toggle",
    "input_overlay_next",
    "input_pause_toggle",
    "input_play_replay",
    "input_preempt_toggle",
    "input_record_replay",
    "input_recording_toggle",
    "input_replay_slot_decrease",
    "input_replay_slot_increase",
    "input_reset",
    "input_rewind",
    "input_runahead_toggle",
    "input_save_state",
    "input_screenshot",
    "input_send_debug_info",
    "input_shader_next",
    "input_shader_prev",
    "input_shader_toggle",
    "input_state_slot_decrease",
    "input_state_slot_increase",
    "input_streaming_toggle",
    "input_toggle_fast_forward",
    "input_toggle_fullscreen",
    "input_toggle_slowmotion",
    "input_toggle_statistics",
    "input_toggle_vrr_runloop",
    "input_volume_down",
    "input_volume_up",
];

/// RetroPad buttons, in the order RetroArch writes them.
const RETROPAD: &[&str] = &[
    "b", "y", "select", "start", "up", "down", "left", "right", "a", "x", "l", "r", "l2", "r2",
    "l3", "r3",
];
const ANALOG: &[&str] = &[
    "l_x_plus",
    "l_x_minus",
    "l_y_plus",
    "l_y_minus",
    "r_x_plus",
    "r_x_minus",
    "r_y_plus",
    "r_y_minus",
];

/// How a Deck input reaches RetroArch.
#[derive(Clone, Copy)]
enum Source {
    Button(&'static str),
    Axis(&'static str),
    Key(&'static str),
}

impl Source {
    fn suffix(self) -> &'static str {
        match self {
            Source::Button(_) => "_btn",
            Source::Axis(_) => "_axis",
            Source::Key(_) => "",
        }
    }
    fn value(self) -> &'static str {
        match self {
            Source::Button(v) | Source::Axis(v) | Source::Key(v) => v,
        }
    }
}

fn button_source(input: &str) -> Option<Source> {
    Some(match input {
        "a" => Source::Button("0"),
        "b" => Source::Button("1"),
        "x" => Source::Button("2"),
        "y" => Source::Button("3"),
        "view" => Source::Button("4"),
        "menu" => Source::Button("6"),
        "l3" => Source::Button("7"),
        "r3" => Source::Button("8"),
        "l1" => Source::Button("9"),
        "r1" => Source::Button("10"),
        "l2" => Source::Axis("+4"),
        "r2" => Source::Axis("+5"),
        other => Source::Key(GRIP_KEYS.iter().find(|(i, _)| *i == other)?.1),
    })
}

/// Up, down, left, right of a directional input.
fn directions(input: &str) -> Option<[Source; 4]> {
    Some(match input {
        "dpad" => [
            Source::Button("11"),
            Source::Button("12"),
            Source::Button("13"),
            Source::Button("14"),
        ],
        "left_stick" => [
            Source::Axis("-1"),
            Source::Axis("+1"),
            Source::Axis("-0"),
            Source::Axis("+0"),
        ],
        "right_stick" => [
            Source::Axis("-3"),
            Source::Axis("+3"),
            Source::Axis("-2"),
            Source::Axis("+2"),
        ],
        _ => return None,
    })
}

/// Where a system's button lands on the RetroPad, as its core reads it. A direction target is
/// `dirs` (the RetroPad's D-pad) or an analog stick, `l` or `r`.
fn retropad(system: System, target: &str) -> Option<&'static str> {
    Some(match (system, target) {
        (_, "dpad") => "dirs",
        // Mupen64Plus-Next: the RetroPad's B is the N64's A, Y its B, L2 its Z, and the right
        // stick the C buttons.
        (System::N64, "a") => "b",
        (System::N64, "b") => "y",
        (System::N64, "z") => "l2",
        (System::N64, "stick") => "l",
        (System::N64, "c_buttons") => "r",
        (System::N64, "l") => "l",
        (System::N64, "r") => "r",
        (System::N64, "start") => "start",
        (System::N64, _) => return None,
        (_, "a") => "a",
        (_, "b") => "b",
        (_, "x") => "x",
        (_, "y") => "y",
        (_, "l") => "l",
        (_, "r") => "r",
        (_, "start") => "start",
        (_, "select") => "select",
        _ => return None,
    })
}

/// The settings for one game: `input_player1_*` binds and hotkeys. An unbound RetroPad button
/// is `nul`. When two inputs of the same kind drive one button, the first in the layout wins.
pub fn config(system: System, layout: &Layout) -> String {
    let mut binds: BTreeMap<String, &'static str> = BTreeMap::new();
    let mut set = |key: String, value: &'static str| {
        binds.entry(key).or_insert(value);
    };
    for (input, binding) in layout {
        let Binding::Bound(target) = binding else {
            continue;
        };
        if let Some((_, hotkey)) = HOTKEYS.iter().find(|(t, _)| t == target) {
            if let Some(src) = button_source(input) {
                set(format!("{hotkey}{}", src.suffix()), src.value());
            }
            continue;
        }
        let Some(pad) = retropad(system, target) else {
            continue;
        };
        if let Some(src) = button_source(input) {
            set(format!("input_player1_{pad}{}", src.suffix()), src.value());
        } else if let Some([up, down, left, right]) = directions(input) {
            let names: [String; 4] = if pad == "dirs" {
                ["up", "down", "left", "right"].map(String::from)
            } else {
                [
                    format!("{pad}_y_minus"),
                    format!("{pad}_y_plus"),
                    format!("{pad}_x_minus"),
                    format!("{pad}_x_plus"),
                ]
            };
            for (name, src) in names.iter().zip([up, down, left, right]) {
                set(format!("input_player1_{name}{}", src.suffix()), src.value());
            }
        }
    }

    let mut out = String::from(
        "# Written by Romshelf: this game's controls. Appended after romshelf.cfg.\n\
         input_autodetect_enable = \"false\"\n\
         input_player1_joypad_index = \"0\"\n",
    );
    let mut line = |key: String| {
        let value = binds.get(&key).copied().unwrap_or("nul");
        let _ = writeln!(out, "{key} = \"{value}\"");
    };
    for pad in RETROPAD.iter().chain(ANALOG) {
        for suffix in ["", "_btn", "_axis"] {
            line(format!("input_player1_{pad}{suffix}"));
        }
    }
    for hotkey in ALL_HOTKEYS {
        for suffix in ["", "_btn", "_axis"] {
            line(format!("{hotkey}{suffix}"));
        }
    }
    out
}
