//! A merged layout as RetroArch settings for one game: a controls config appended after
//! Romshelf's own, and a remap file.
//!
//! Each Deck button is bound to its own RetroPad slot, by position, as RetroArch's
//! `Steam Virtual Gamepad.cfg` autoconfig on the Deck numbers them (so RetroArch's menu works
//! as usual). The remap file then sends each slot to the game button the layout chooses, which
//! lets any number of Deck buttons drive one game button. RetroArch 1.19.1 reads it from
//! `<input_remapping_directory>/<core name>/<core name>.rmp` (its own source, `configuration.c`),
//! and each game gets its own remap folder, so the file never depends on the ROM's name.
//!
//! The back grips reach RetroArch as the keys Romshelf's Steam Input layout makes them send.
//! Every hotkey is written, so nothing falls back to the user's own (EmuDeck's need View held,
//! and its button hotkeys would fire on a single press without it).

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

/// RetroPad slots, in RetroArch's order; a slot's number is its id in a remap file.
const SLOTS: &[&str] = &[
    "b", "y", "select", "start", "up", "down", "left", "right", "a", "x", "l", "r", "l2", "r2",
    "l3", "r3",
];
/// Analog directions, ids 16 to 23 in a remap file.
const STICKS: &[&str] = &[
    "l_x+", "l_x-", "l_y+", "l_y-", "r_x+", "r_x-", "r_y+", "r_y-",
];

fn slot_id(slot: &str) -> Option<usize> {
    SLOTS
        .iter()
        .position(|s| *s == slot)
        .or_else(|| STICKS.iter().position(|s| *s == slot).map(|i| i + 16))
}

/// How a Deck button reaches RetroArch, and the RetroPad slot it's bound to by position.
#[derive(Clone, Copy)]
enum Source {
    Button(&'static str),
    Axis(&'static str),
}

fn physical(input: &str) -> Option<(&'static str, Source)> {
    Some(match input {
        "a" => ("b", Source::Button("0")),
        "b" => ("a", Source::Button("1")),
        "x" => ("y", Source::Button("2")),
        "y" => ("x", Source::Button("3")),
        "view" => ("select", Source::Button("4")),
        "menu" => ("start", Source::Button("6")),
        "l3" => ("l3", Source::Button("7")),
        "r3" => ("r3", Source::Button("8")),
        "l1" => ("l", Source::Button("9")),
        "r1" => ("r", Source::Button("10")),
        "l2" => ("l2", Source::Axis("+4")),
        "r2" => ("r2", Source::Axis("+5")),
        _ => return None,
    })
}

/// The button number Romshelf's hotkey bindings use for a Deck button.
fn hotkey_source(input: &str) -> Option<(&'static str, &'static str)> {
    if let Some(key) = GRIP_KEYS.iter().find(|(i, _)| *i == input) {
        return Some(("", key.1));
    }
    match physical(input)? {
        (_, Source::Button(n)) => Some(("_btn", n)),
        (_, Source::Axis(n)) => Some(("_axis", n)),
    }
}

/// Where a game button lands on the RetroPad, as its core reads it. A direction target is
/// `dirs` (the D-pad) or an analog stick, `l` or `r`.
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

/// Up, down, left, right of a direction target, as remap ids.
fn direction_ids(pad: &str) -> [usize; 4] {
    match pad {
        "l" => [19, 18, 17, 16],
        "r" => [23, 22, 21, 20],
        _ => [4, 5, 6, 7],
    }
}

/// Up, down, left, right of a directional Deck input: the remap keys that carry it.
fn direction_keys(input: &str) -> Option<[&'static str; 4]> {
    Some(match input {
        "dpad" => ["btn_up", "btn_down", "btn_left", "btn_right"],
        "left_stick" => ["stk_l_y-", "stk_l_y+", "stk_l_x-", "stk_l_x+"],
        "right_stick" => ["stk_r_y-", "stk_r_y+", "stk_r_x-", "stk_r_x+"],
        _ => return None,
    })
}

/// The core's name as RetroArch uses it for the remap folder, from its info file on the Deck.
pub fn core_name(core: &str) -> Option<&'static str> {
    Some(match core {
        "mgba" => "mGBA",
        "snes9x" => "Snes9x",
        "mupen64plus_next" => "Mupen64Plus-Next",
        _ => return None,
    })
}

/// The grips bound to game buttons, each given the slot of a Deck button the layout leaves
/// unbound (whose physical button is then not bound, so it can't press it).
fn grip_slots(layout: &Layout) -> Vec<(&'static str, &'static str, String)> {
    let mut free: Vec<&'static str> = ["x", "y", "l3", "r3", "view", "l1", "r1", "a", "b", "menu"]
        .iter()
        .filter(|i| matches!(layout.get(**i), None | Some(Binding::Cleared)))
        .filter_map(|i| physical(i).map(|(slot, _)| slot))
        .collect();
    let mut out = Vec::new();
    for (grip, key) in GRIP_KEYS {
        if let Some(Binding::Bound(target)) = layout.get(*grip) {
            if HOTKEYS.iter().any(|(t, _)| t == target) {
                continue;
            }
            if let Some(slot) = (!free.is_empty()).then(|| free.remove(0)) {
                out.push((slot, *key, target.clone()));
            }
        }
    }
    out
}

/// The controls config: each Deck button on its own slot, grips as keys, every hotkey.
pub fn config(_system: System, layout: &Layout) -> String {
    let mut binds: BTreeMap<String, &'static str> = BTreeMap::new();
    let grips = grip_slots(layout);
    for input in [
        "a", "b", "x", "y", "view", "menu", "l3", "r3", "l1", "r1", "l2", "r2",
    ] {
        let (slot, src) = physical(input).expect("a Deck button");
        if grips.iter().any(|(s, _, _)| *s == slot) {
            continue;
        }
        match src {
            Source::Button(n) => binds.insert(format!("input_player1_{slot}_btn"), n),
            Source::Axis(n) => binds.insert(format!("input_player1_{slot}_axis"), n),
        };
    }
    for (dir, btn) in [
        ("up", "11"),
        ("down", "12"),
        ("left", "13"),
        ("right", "14"),
    ] {
        binds.insert(format!("input_player1_{dir}_btn"), btn);
    }
    for (stick, axis) in [
        ("l_x_plus", "+0"),
        ("l_x_minus", "-0"),
        ("l_y_plus", "+1"),
        ("l_y_minus", "-1"),
        ("r_x_plus", "+2"),
        ("r_x_minus", "-2"),
        ("r_y_plus", "+3"),
        ("r_y_minus", "-3"),
    ] {
        binds.insert(format!("input_player1_{stick}_axis"), axis);
    }
    for (slot, key, _) in &grips {
        binds.insert(format!("input_player1_{slot}"), key);
    }
    for (input, binding) in layout {
        let Binding::Bound(target) = binding else {
            continue;
        };
        if let Some((_, hotkey)) = HOTKEYS.iter().find(|(t, _)| t == target) {
            if let Some((suffix, value)) = hotkey_source(input) {
                binds.entry(format!("{hotkey}{suffix}")).or_insert(value);
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
    let analog = [
        "l_x_plus",
        "l_x_minus",
        "l_y_plus",
        "l_y_minus",
        "r_x_plus",
        "r_x_minus",
        "r_y_plus",
        "r_y_minus",
    ];
    for pad in SLOTS.iter().chain(analog.iter()) {
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

/// The remap file: each slot to the game button the layout gives its Deck button, `-1` for
/// none. Several slots may send the same button.
pub fn remap(system: System, layout: &Layout) -> String {
    let mut ids: BTreeMap<String, i32> = BTreeMap::new();
    for slot in SLOTS {
        ids.insert(format!("btn_{slot}"), -1);
    }
    for stick in STICKS {
        ids.insert(format!("stk_{stick}"), -1);
    }
    for (input, binding) in layout {
        let Binding::Bound(target) = binding else {
            continue;
        };
        let Some(pad) = retropad(system, target) else {
            continue;
        };
        if let Some((slot, _)) = physical(input) {
            if let Some(id) = slot_id(pad) {
                ids.insert(format!("btn_{slot}"), id as i32);
            }
        } else if let Some(keys) = direction_keys(input) {
            for (key, id) in keys.iter().zip(direction_ids(pad)) {
                ids.insert((*key).to_string(), id as i32);
            }
        }
    }
    for (slot, _, target) in grip_slots(layout) {
        if let Some(id) = retropad(system, &target).and_then(slot_id) {
            ids.insert(format!("btn_{slot}"), id as i32);
        }
    }
    let mut out = String::from(
        "# Written by Romshelf: this game's buttons.\n\
         input_libretro_device_p1 = \"1\"\n\
         input_remap_port_p1 = \"0\"\n",
    );
    for slot in SLOTS {
        let _ = writeln!(
            out,
            "input_player1_btn_{slot} = \"{}\"",
            ids[&format!("btn_{slot}")]
        );
    }
    for stick in STICKS {
        let _ = writeln!(
            out,
            "input_player1_stk_{stick} = \"{}\"",
            ids[&format!("stk_{stick}")]
        );
    }
    out
}
