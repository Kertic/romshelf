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
    fn run_and_wait(&self, cmd: &CommandSpec) -> Result<i32>;
}

pub struct FlatpakRunner;

impl PackageRunner for FlatpakRunner {
    fn flatpak(&self, args: &[&str]) -> Result<CommandOutput> {
        let out = std::process::Command::new("flatpak").args(args).output()?;
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
        let mut child = std::process::Command::new("flatpak")
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

pub struct SystemProcessRunner;

impl ProcessRunner for SystemProcessRunner {
    fn run_and_wait(&self, cmd: &CommandSpec) -> Result<i32> {
        let status = std::process::Command::new(&cmd.program)
            .args(&cmd.args)
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .status()?;
        Ok(status.code().unwrap_or(-1))
    }
}
