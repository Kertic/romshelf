//! Spike A, second input path: read pads with `gilrs` and forward snapshots as the `gilrs` event.

use gilrs::{Axis, Button, EventType, Gilrs};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone, PartialEq)]
struct Pad {
    id: usize,
    name: String,
    pressed: Vec<&'static str>,
    axes: BTreeMap<&'static str, f32>,
}

#[derive(Serialize, Clone, PartialEq)]
struct Snapshot {
    pads: Vec<Pad>,
    last_event: String,
    /// A game is running: the pad is the game's, so the interface hears nothing from it.
    blocked: bool,
}

/// What Romshelf may act on from a pad. Nothing while a game runs: Romshelf reads the pad
/// whichever window has focus, so a press meant for the game would otherwise land on whatever
/// Romshelf has focused (found on the Deck, 2026-10-21). After the game, a button still held
/// from it counts only once released, so the press that closed the game presses nothing here.
fn gate(
    pressed: Vec<&'static str>,
    blocked: bool,
    held_over: &mut BTreeSet<&'static str>,
) -> Vec<&'static str> {
    if blocked {
        held_over.extend(pressed);
        return Vec::new();
    }
    held_over.retain(|b| pressed.contains(b));
    pressed
        .into_iter()
        .filter(|b| !held_over.contains(b))
        .collect()
}

/// A game is running: one this Romshelf started, or one a Steam shortcut started in another
/// Romshelf (which holds the launch lock). A launch window's own lock doesn't count until its
/// emulator starts, so a save question before the game can still be answered.
fn game_running(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return false;
    };
    if state.process.is_running() {
        return true;
    }
    let other = std::fs::read_to_string(state.paths.launch_lock())
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        .is_some_and(|pid| pid != std::process::id());
    other && romshelf_core::launch::game_running(&state.paths)
}

/// Same names as the front end's `Button` type.
fn button_name(b: Button) -> Option<&'static str> {
    Some(match b {
        Button::South => "a",
        Button::East => "b",
        Button::West => "x",
        Button::North => "y",
        Button::LeftTrigger => "l1",
        Button::RightTrigger => "r1",
        Button::LeftTrigger2 => "l2",
        Button::RightTrigger2 => "r2",
        Button::Select => "view",
        Button::Start => "menu",
        Button::LeftThumb => "l3",
        Button::RightThumb => "r3",
        Button::DPadUp => "up",
        Button::DPadDown => "down",
        Button::DPadLeft => "left",
        Button::DPadRight => "right",
        Button::Mode => "guide",
        _ => return None,
    })
}

const BUTTONS: [Button; 17] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::LeftThumb,
    Button::RightThumb,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
    Button::Mode,
];

const AXES: [(Axis, &str); 6] = [
    (Axis::LeftStickX, "left_x"),
    (Axis::LeftStickY, "left_y"),
    (Axis::RightStickX, "right_x"),
    (Axis::RightStickY, "right_y"),
    (Axis::LeftZ, "left_z"),
    (Axis::RightZ, "right_z"),
];

fn snapshot(
    gilrs: &Gilrs,
    last_event: &str,
    blocked: bool,
    held_over: &mut HashMap<usize, BTreeSet<&'static str>>,
) -> Snapshot {
    let pads = gilrs
        .gamepads()
        .map(|(id, pad)| {
            let id = usize::from(id);
            let pressed = BUTTONS
                .iter()
                .filter(|b| pad.is_pressed(**b))
                .filter_map(|b| button_name(*b))
                .collect();
            Pad {
                id,
                name: pad.name().to_string(),
                pressed: gate(pressed, blocked, held_over.entry(id).or_default()),
                axes: AXES
                    .iter()
                    .map(|(a, n)| {
                        (
                            *n,
                            if blocked {
                                0.0
                            } else {
                                (pad.value(*a) * 100.0).round() / 100.0
                            },
                        )
                    })
                    .collect(),
            }
        })
        .collect();
    Snapshot {
        pads,
        last_event: last_event.to_string(),
        blocked,
    }
}

/// Starts the polling thread. If gilrs can't start, the browser path is the only one.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("gilrs unavailable: {e}");
                return;
            }
        };
        let mut last = String::new();
        let mut sent: Option<Snapshot> = None;
        let mut held_over = HashMap::new();
        let mut blocked = false;
        let mut checked = std::time::Instant::now() - Duration::from_secs(1);
        loop {
            if checked.elapsed() >= Duration::from_millis(100) {
                blocked = game_running(&app);
                checked = std::time::Instant::now();
            }
            while let Some(ev) = gilrs.next_event() {
                last = match ev.event {
                    EventType::ButtonPressed(b, _) => format!("{b:?} pressed"),
                    EventType::ButtonReleased(b, _) => format!("{b:?} released"),
                    EventType::Connected => "pad connected".into(),
                    EventType::Disconnected => "pad disconnected".into(),
                    _ => continue,
                };
            }
            let snap = snapshot(&gilrs, &last, blocked, &mut held_over);
            if sent.as_ref() != Some(&snap) {
                let _ = app.emit("gilrs", &snap);
                sent = Some(snap);
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    });
}

/// A line in `launch.log` when View + Menu asked the game to close, naming the pad it came from.
fn log_quit(dir: &std::path::Path, pads: &[String]) {
    use std::io::Write;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!(
        "{now} View + Menu held on {}: asked the game to close
",
        pads.join(", ")
    );
    let _ = std::fs::create_dir_all(dir).and_then(|_| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("launch.log"))
            .and_then(|mut f| f.write_all(line.as_bytes()))
    });
}

/// Romshelf's quit combination while a game runs: View and Menu held for a second ask the
/// running emulator to close, cleanly, so it saves first. Romshelf reads the pad itself, so it
/// works in every emulator and every state, whichever program has focus and whatever the
/// game's layout.
pub fn watch_quit(
    process: std::sync::Arc<dyn romshelf_core::process::ProcessRunner>,
    log_dir: std::path::PathBuf,
) {
    std::thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("Couldn't watch the controller for View + Menu: {e}");
                return;
            }
        };
        let mut combo = romshelf_core::launch::QuitCombo::default();
        loop {
            while gilrs.next_event().is_some() {}
            // Any pad: while Dolphin runs, the Deck's buttons arrive on its own controller device
            // rather than Steam's virtual pad (seen on the Deck, 2026-10-20).
            let holding: Vec<String> = gilrs
                .gamepads()
                .filter(|(_, p)| p.is_pressed(Button::Select) && p.is_pressed(Button::Start))
                .map(|(_, p)| p.name().to_string())
                .collect();
            if combo.update(!holding.is_empty(), std::time::Instant::now()) {
                process.request_stop();
                log_quit(&log_dir, &holding);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_reaches_romshelf_while_a_game_runs() {
        let mut held = BTreeSet::new();
        assert_eq!(gate(vec!["a"], true, &mut held), Vec::<&str>::new());
        assert_eq!(gate(vec!["a", "down"], true, &mut held), Vec::<&str>::new());
    }

    #[test]
    fn a_button_held_from_the_game_counts_only_once_released() {
        let mut held = BTreeSet::new();
        gate(vec!["a"], true, &mut held);
        // The game ends with A still down: not a press here.
        assert_eq!(gate(vec!["a"], false, &mut held), Vec::<&str>::new());
        // Another button is fine at once.
        assert_eq!(gate(vec!["a", "b"], false, &mut held), vec!["b"]);
        // A released, then pressed again: now it counts.
        assert_eq!(gate(vec![], false, &mut held), Vec::<&str>::new());
        assert_eq!(gate(vec!["a"], false, &mut held), vec!["a"]);
    }
}
