//! Seams for running external programs: Flatpak and emulators.

use crate::Result;
use serde::{Deserialize, Serialize};

/// A program and its arguments, built by an adapter and run by a `ProcessRunner`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>, args: &[&str]) -> CommandSpec {
        CommandSpec {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Variables the AppImage's start-up scripts set so Romshelf finds its own bundled libraries
/// (read from a running Romshelf on the Deck). A host program that inherits them loads the
/// wrong libraries: `flatpak` fails with "symbol lookup error".
const APPIMAGE_VARS: &[&str] = &[
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "GTK_THEME",
    "GTK_DATA_PREFIX",
    "GTK_EXE_PREFIX",
    "GTK_PATH",
    "GTK_IM_MODULE_FILE",
    "GDK_PIXBUF_MODULE_FILE",
    "GSETTINGS_SCHEMA_DIR",
    "GI_TYPELIB_PATH",
    "GIO_MODULE_DIR",
    "PYTHONHOME",
    "PYTHONPATH",
    "PYTHONDONTWRITEBYTECODE",
    "QT_PLUGIN_PATH",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
];

/// Changes to Romshelf's environment for a program on the host.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HostEnv {
    pub remove: Vec<String>,
    pub set: Vec<(String, String)>,
}

/// What to change, given Romshelf's environment. Nothing unless running from an AppImage
/// (`APPDIR` set): then the AppImage's variables go, and its folders leave `XDG_DATA_DIRS`.
pub fn host_env(env: &[(String, String)]) -> HostEnv {
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let Some(appdir) = get("APPDIR").filter(|d| !d.is_empty()) else {
        return HostEnv::default();
    };
    let appdir = appdir.trim_end_matches('/');
    let mut out = HostEnv::default();
    for var in APPIMAGE_VARS {
        if get(var).is_some() {
            out.remove.push(var.to_string());
        }
    }
    if let Some(dirs) = get("XDG_DATA_DIRS") {
        let host: Vec<&str> = dirs
            .split(':')
            .filter(|d| !d.is_empty() && !d.starts_with(appdir))
            .collect();
        if host.is_empty() {
            out.remove.push("XDG_DATA_DIRS".into());
        } else {
            out.set.push(("XDG_DATA_DIRS".into(), host.join(":")));
        }
    }
    out
}

/// A command for a program on the host (Flatpak, an emulator, the browser) without the
/// AppImage's environment.
pub fn host_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    let env: Vec<(String, String)> = std::env::vars().collect();
    let fixes = host_env(&env);
    for var in &fixes.remove {
        cmd.env_remove(var);
    }
    for (k, v) in &fixes.set {
        cmd.env(k, v);
    }
    cmd
}

/// Runs `flatpak` with the given arguments and captures output.
pub trait PackageRunner: Send + Sync {
    fn flatpak(&self, args: &[&str]) -> Result<CommandOutput>;

    /// Like `flatpak`, but passes each piece of output (split at `\r` and `\n`, which Flatpak
    /// uses for progress) to `on_output` as it arrives.
    fn flatpak_streaming(
        &self,
        args: &[&str],
        on_output: &mut dyn FnMut(&str),
    ) -> Result<CommandOutput> {
        let out = self.flatpak(args)?;
        for piece in out.stdout.split(['\r', '\n']).filter(|p| !p.is_empty()) {
            on_output(piece);
        }
        Ok(out)
    }
}

/// Starts a program (an emulator) and waits for it to exit.
pub trait ProcessRunner: Send + Sync {
    /// Starts the program and waits for it to exit. A stop signal sent to Romshelf (Steam
    /// ending the game, Ctrl+C) is passed on to the program, and Romshelf keeps waiting.
    fn run_and_wait(&self, cmd: &CommandSpec) -> Result<i32>;

    /// Asks the program running now to close, as a stop signal to Romshelf would (Romshelf's
    /// own quit combination). Does nothing when none is running.
    fn request_stop(&self) {}

    /// The last lines the most recent program wrote to stderr, for error reports.
    fn last_output(&self) -> Vec<String> {
        Vec::new()
    }

    /// A program started by `run_and_wait` is running now (a game is being played).
    fn is_running(&self) -> bool {
        false
    }
}

pub struct FlatpakRunner;

impl PackageRunner for FlatpakRunner {
    fn flatpak(&self, args: &[&str]) -> Result<CommandOutput> {
        let out = host_command("flatpak").args(args).output()?;
        Ok(CommandOutput {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn flatpak_streaming(
        &self,
        args: &[&str],
        on_output: &mut dyn FnMut(&str),
    ) -> Result<CommandOutput> {
        use std::io::Read;
        let mut child = host_command("flatpak")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        // Stderr is read on its own thread so a chatty install can't block on a full pipe.
        let mut err_pipe = child.stderr.take();
        let err_thread = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(e) = err_pipe.as_mut() {
                let _ = e.read_to_string(&mut s);
            }
            s
        });
        let mut stdout = String::new();
        let mut pending = String::new();
        if let Some(mut out) = child.stdout.take() {
            let mut buf = [0u8; 4096];
            loop {
                let n = out.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                let text = String::from_utf8_lossy(&buf[..n]);
                stdout.push_str(&text);
                pending.push_str(&text);
                while let Some(i) = pending.find(['\r', '\n']) {
                    let piece: String = pending.drain(..=i).collect();
                    let piece = piece.trim_end_matches(['\r', '\n']);
                    if !piece.is_empty() {
                        on_output(piece);
                    }
                }
            }
        }
        if !pending.is_empty() {
            on_output(&pending);
        }
        let status = child.wait()?;
        Ok(CommandOutput {
            code: status.code().unwrap_or(-1),
            stdout,
            stderr: err_thread.join().unwrap_or_default(),
        })
    }
}

#[derive(Default)]
pub struct SystemProcessRunner {
    tail: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// Set by `request_stop`; cleared when a program starts.
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Set while a program started here runs.
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Clears the running flag however `run_and_wait` ends.
struct Running<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Lines of stderr kept for error reports.
const TAIL_LINES: usize = 20;

impl ProcessRunner for SystemProcessRunner {
    fn run_and_wait(&self, cmd: &CommandSpec) -> Result<i32> {
        use std::io::{BufRead, Write};
        let mut child = host_command(&cmd.program)
            .args(&cmd.args)
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let _running = Running(&self.running);
        // Stderr still reaches the terminal (and Steam's log); the last lines are kept too.
        let tail = std::sync::Arc::clone(&self.tail);
        if let Ok(mut t) = tail.lock() {
            t.clear();
        }
        let reader = child.stderr.take().map(|err| {
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(err).lines().map_while(|l| l.ok()) {
                    let _ = writeln!(std::io::stderr(), "{line}");
                    if let Ok(mut t) = tail.lock() {
                        t.push(line);
                        let excess = t.len().saturating_sub(TAIL_LINES);
                        t.drain(..excess);
                    }
                }
            })
        });
        self.stop.store(false, std::sync::atomic::Ordering::SeqCst);
        let stop = stop_requests()?;
        // When the stop was passed on; the stop is escalated if the program doesn't close.
        let mut forwarded: Option<std::time::Instant> = None;
        let mut escalated = 0;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            let asked = stop.load(std::sync::atomic::Ordering::SeqCst)
                || self.stop.load(std::sync::atomic::Ordering::SeqCst);
            if forwarded.is_none() && asked {
                forwarded = Some(std::time::Instant::now());
                stop_tree(child.id(), StopLevel::Ask);
            }
            if let Some(at) = forwarded {
                let waited = at.elapsed();
                if escalated == 0 && waited >= STOP_GRACE {
                    escalated = 1;
                    stop_tree(child.id(), StopLevel::All);
                } else if escalated == 1 && waited >= STOP_GRACE * 2 {
                    escalated = 2;
                    stop_tree(child.id(), StopLevel::Force);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        if let Some(r) = reader {
            let _ = r.join();
        }
        Ok(status.code().unwrap_or(-1))
    }

    fn last_output(&self) -> Vec<String> {
        self.tail.lock().map(|t| t.clone()).unwrap_or_default()
    }

    fn request_stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// A flag set when Romshelf is asked to stop (SIGTERM, SIGINT, SIGHUP).
#[cfg(unix)]
fn stop_requests() -> Result<std::sync::Arc<std::sync::atomic::AtomicBool>> {
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(signal, std::sync::Arc::clone(&flag))?;
    }
    Ok(flag)
}

#[cfg(not(unix))]
fn stop_requests() -> Result<std::sync::Arc<std::sync::atomic::AtomicBool>> {
    Ok(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
        false,
    )))
}

/// How long a program gets to close after a stop before the stop is widened, then forced.
const STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopLevel {
    /// SIGTERM to the programs at the end of the chain: the emulator itself.
    Ask,
    /// SIGTERM to every program in the chain.
    All,
    /// SIGKILL to every program in the chain.
    Force,
}

/// The programs started (directly or not) by `root`, from `(pid, parent pid)` pairs.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn descendants(root: u32, procs: &[(u32, u32)]) -> Vec<u32> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let parent = out[i];
        out.extend(
            procs
                .iter()
                .filter(|(_, pp)| *pp == parent)
                .map(|(p, _)| *p),
        );
        i += 1;
    }
    out
}

/// The programs at the end of `root`'s chain: those that started nothing themselves.
/// A Flatpak game runs as `bwrap → bwrap → wrapper script → emulator`; the sandbox doesn't
/// pass signals on, and stopping `bwrap` would kill the emulator before it saves.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn leaves(root: u32, procs: &[(u32, u32)]) -> Vec<u32> {
    let all = descendants(root, procs);
    all.iter()
        .copied()
        .filter(|p| !procs.iter().any(|(c, pp)| pp == p && all.contains(c)))
        .collect()
}

/// Every process as `(pid, parent pid)`, read from /proc.
#[cfg(target_os = "linux")]
fn process_table() -> Vec<(u32, u32)> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // The name in parentheses may contain spaces; the parent pid is the second field after it.
            let after = &stat[stat.rfind(')')? + 1..];
            let ppid = after.split_whitespace().nth(1)?.parse::<u32>().ok()?;
            Some((pid, ppid))
        })
        .collect()
}

/// Passes a stop on to the program Romshelf started and what it started in turn.
#[cfg(target_os = "linux")]
fn stop_tree(root: u32, level: StopLevel) {
    let procs = process_table();
    let (targets, signal) = match level {
        StopLevel::Ask => (leaves(root, &procs), libc::SIGTERM),
        StopLevel::All => (descendants(root, &procs), libc::SIGTERM),
        StopLevel::Force => (descendants(root, &procs), libc::SIGKILL),
    };
    for pid in targets {
        if let Ok(pid) = i32::try_from(pid) {
            // SAFETY: kill(2) on a process Romshelf started (or its descendant).
            unsafe {
                libc::kill(pid, signal);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn stop_tree(_root: u32, _level: StopLevel) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chain seen on the Deck for Dolphin started by `romshelf launch` (pid 61582), plus
    /// unrelated processes and Flatpak's D-Bus proxy, which belongs to systemd, not Romshelf.
    const DECK: &[(u32, u32)] = &[
        (1, 0),
        (1376, 1),
        (61582, 1376),
        (61604, 61582),
        (61610, 1376),
        (61614, 61604),
        (61615, 61614),
        (61626, 61615),
        (58077, 1376),
    ];

    /// Romshelf's quit combination stops the running program and returns its exit.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_requested_stop_ends_the_running_program() {
        let runner = std::sync::Arc::new(SystemProcessRunner::default());
        let r = std::sync::Arc::clone(&runner);
        let started = std::time::Instant::now();
        let asker = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            r.request_stop();
        });
        let code = runner
            .run_and_wait(&CommandSpec::new("sleep", &["30"]))
            .unwrap();
        asker.join().unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert_ne!(code, 0, "ended by the stop, not by finishing");
    }

    #[test]
    fn a_stale_request_does_not_stop_the_next_program() {
        let runner = SystemProcessRunner::default();
        runner.request_stop();
        let ok = if cfg!(windows) {
            ("cmd", vec!["/C", "exit 0"])
        } else {
            ("true", vec![])
        };
        assert_eq!(
            runner.run_and_wait(&CommandSpec::new(ok.0, &ok.1)).unwrap(),
            0
        );
    }

    #[test]
    fn a_stop_goes_to_the_emulator_at_the_end_of_the_chain() {
        // Romshelf's child is the outer bwrap; Dolphin is the only program that started nothing.
        assert_eq!(leaves(61604, DECK), [61626]);
        assert_eq!(descendants(61604, DECK), [61604, 61614, 61615, 61626]);
    }

    #[test]
    fn a_program_that_started_nothing_is_its_own_end_of_chain() {
        assert_eq!(leaves(42, &[(42, 1)]), [42]);
        assert_eq!(descendants(42, &[]), [42]);
    }

    #[test]
    fn several_branches_each_get_the_stop() {
        let procs = [(10, 1), (11, 10), (12, 10), (13, 12)];
        let mut l = leaves(10, &procs);
        l.sort();
        assert_eq!(l, [11, 13]);
    }

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// As read from `romshelf auth login` running from the AppImage on the Deck.
    #[test]
    fn the_appimage_environment_is_removed_for_host_programs() {
        let m = "/tmp/.mount_romsheAbC123";
        let e = env(&[
            ("HOME", "/home/deck"),
            ("PATH", "/usr/bin"),
            ("APPDIR", m),
            ("APPIMAGE", "/home/deck/Applications/romshelf.AppImage"),
            ("ARGV0", "/home/deck/Applications/romshelf.AppImage"),
            ("OWD", "/tmp"),
            ("GTK_THEME", "Adwaita:dark"),
            ("LD_LIBRARY_PATH", &format!("{m}/usr/lib/:{m}/lib/x86_64-linux-gnu/:")),
            ("GIO_MODULE_DIR", &format!("{m}//usr/lib/gio/modules")),
            ("PYTHONHOME", &format!("{m}/usr/")),
            ("QT_PLUGIN_PATH", &format!("{m}/usr/lib/qt5/plugins/:")),
            ("GST_PLUGIN_SYSTEM_PATH_1_0", &format!("{m}/usr/lib/gstreamer-1.0:")),
            ("XDG_DATA_DIRS", &format!("{m}/usr/share/:{m}/usr/share:/usr/share:/home/deck/.local/share/flatpak/exports/share:")),
        ]);
        let fixes = host_env(&e);
        for var in [
            "LD_LIBRARY_PATH",
            "APPDIR",
            "APPIMAGE",
            "GTK_THEME",
            "GIO_MODULE_DIR",
            "PYTHONHOME",
            "QT_PLUGIN_PATH",
            "GST_PLUGIN_SYSTEM_PATH_1_0",
        ] {
            assert!(fixes.remove.iter().any(|r| r == var), "{var} removed");
        }
        assert!(!fixes.remove.iter().any(|r| r == "HOME" || r == "PATH"));
        assert_eq!(
            fixes.set,
            vec![(
                "XDG_DATA_DIRS".to_string(),
                "/usr/share:/home/deck/.local/share/flatpak/exports/share".to_string()
            )]
        );
    }

    #[test]
    fn outside_an_appimage_nothing_changes() {
        let e = env(&[
            ("GTK_THEME", "MyTheme"),
            ("LD_LIBRARY_PATH", "/opt/mine"),
            ("XDG_DATA_DIRS", "/usr/share"),
        ]);
        assert_eq!(host_env(&e), HostEnv::default());
    }

    #[test]
    fn data_dirs_only_from_the_appimage_are_removed_entirely() {
        let e = env(&[
            ("APPDIR", "/tmp/.mount_x"),
            ("XDG_DATA_DIRS", "/tmp/.mount_x/usr/share:"),
        ]);
        let fixes = host_env(&e);
        assert!(fixes.remove.contains(&"XDG_DATA_DIRS".to_string()));
        assert!(fixes.set.is_empty());
    }
}
