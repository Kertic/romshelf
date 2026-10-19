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
