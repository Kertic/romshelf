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

    /// The last lines the most recent program wrote to stderr, for error reports.
    fn last_output(&self) -> Vec<String> {
        Vec::new()
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
        let stop = stop_requests()?;
        let mut forwarded = false;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if !forwarded && stop.load(std::sync::atomic::Ordering::SeqCst) {
                forwarded = true;
                ask_to_stop(&child);
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

/// Passes a stop on to the program, which then exits in its own time.
#[cfg(unix)]
fn ask_to_stop(child: &std::process::Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: kill(2) with a pid we started and a valid signal number.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

#[cfg(not(unix))]
fn ask_to_stop(_child: &std::process::Child) {}

#[cfg(test)]
mod tests {
    use super::*;

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
