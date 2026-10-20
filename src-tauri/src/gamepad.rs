//! Spike A, second input path: read pads with `gilrs` and forward snapshots as the `gilrs` event.

use gilrs::{Axis, Button, EventType, Gilrs};
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

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

fn snapshot(gilrs: &Gilrs, last_event: &str) -> Snapshot {
    let pads = gilrs
        .gamepads()
        .map(|(id, pad)| Pad {
            id: usize::from(id),
            name: pad.name().to_string(),
            pressed: BUTTONS
                .iter()
                .filter(|b| pad.is_pressed(**b))
                .filter_map(|b| button_name(*b))
                .collect(),
            axes: AXES
                .iter()
                .map(|(a, n)| (*n, (pad.value(*a) * 100.0).round() / 100.0))
                .collect(),
        })
        .collect();
    Snapshot {
        pads,
        last_event: last_event.to_string(),
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
        loop {
            while let Some(ev) = gilrs.next_event() {
                last = match ev.event {
                    EventType::ButtonPressed(b, _) => format!("{b:?} pressed"),
                    EventType::ButtonReleased(b, _) => format!("{b:?} released"),
                    EventType::Connected => "pad connected".into(),
                    EventType::Disconnected => "pad disconnected".into(),
                    _ => continue,
                };
            }
            let snap = snapshot(&gilrs, &last);
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
