//! In-memory fakes for every outside-world seam, plus the fixture library.
//! Tests use only these. `ROMSHELF_FAKE=1` makes the app use them too.

mod drive;
pub mod fixtures;

pub use drive::FakeDrive;

use romshelf_core::clock::Clock;
use romshelf_core::process::{CommandOutput, CommandSpec, PackageRunner, ProcessRunner};
use romshelf_core::steam::FsSteamFiles;
use romshelf_core::tokens::TokenStore;
use romshelf_core::Result;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Records every `flatpak` call and answers from a script; unscripted calls succeed.
#[derive(Default)]
pub struct FakePackageRunner {
    pub calls: Mutex<Vec<Vec<String>>>,
    script: Mutex<VecDeque<CommandOutput>>,
}

impl FakePackageRunner {
    pub fn push_result(&self, code: i32, stdout: &str, stderr: &str) {
        self.script.lock().unwrap().push_back(CommandOutput {
            code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        });
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl PackageRunner for FakePackageRunner {
    fn flatpak(&self, args: &[&str]) -> Result<CommandOutput> {
        self.calls
            .lock()
            .unwrap()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(self.script.lock().unwrap().pop_front().unwrap_or_default())
    }
}

/// Returns scripted exit codes after optional delays and records each command.
#[derive(Default)]
pub struct FakeProcessRunner {
    pub runs: Mutex<Vec<CommandSpec>>,
    script: Mutex<VecDeque<(i32, Duration)>>,
}

impl FakeProcessRunner {
    pub fn push_exit(&self, code: i32, delay: Duration) {
        self.script.lock().unwrap().push_back((code, delay));
    }

    pub fn runs(&self) -> Vec<CommandSpec> {
        self.runs.lock().unwrap().clone()
    }
}

impl ProcessRunner for FakeProcessRunner {
    fn run_and_wait(&self, cmd: &CommandSpec) -> Result<i32> {
        self.runs.lock().unwrap().push(cmd.clone());
        let (code, delay) = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or((0, Duration::ZERO));
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        Ok(code)
    }
}

#[derive(Default)]
pub struct MemoryTokenStore {
    map: Mutex<HashMap<String, String>>,
}

impl TokenStore for MemoryTokenStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.map.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.map.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.map.lock().unwrap().remove(key);
        Ok(())
    }
}

pub struct FakeClock(AtomicI64);

impl FakeClock {
    pub fn at(secs: i64) -> FakeClock {
        FakeClock(AtomicI64::new(secs))
    }

    pub fn set(&self, secs: i64) {
        self.0.store(secs, Ordering::SeqCst);
    }

    pub fn advance(&self, secs: i64) {
        self.0.fetch_add(secs, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A Steam directory in a temp folder, seeded from `fixtures/steam` when given.
pub struct FakeSteam {
    pub dir: tempfile::TempDir,
    pub files: FsSteamFiles,
}

impl FakeSteam {
    pub fn new(seed: Option<&Path>) -> std::io::Result<FakeSteam> {
        let dir = tempfile::tempdir()?;
        if let Some(seed) = seed {
            copy_dir(seed, dir.path())?;
        }
        let files = FsSteamFiles {
            steam_root: dir.path().to_path_buf(),
        };
        Ok(FakeSteam { dir, files })
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use romshelf_core::steam::SteamFiles;

    #[test]
    fn process_runner_returns_scripted_codes() {
        let runner = FakeProcessRunner::default();
        runner.push_exit(3, Duration::ZERO);
        runner.push_exit(0, Duration::from_millis(5));
        let cmd = CommandSpec::new("emu", &["a"]);
        assert_eq!(runner.run_and_wait(&cmd).unwrap(), 3);
        assert_eq!(runner.run_and_wait(&cmd).unwrap(), 0);
        assert_eq!(runner.run_and_wait(&cmd).unwrap(), 0);
        assert_eq!(runner.runs().len(), 3);
    }

    #[test]
    fn package_runner_records_calls() {
        let runner = FakePackageRunner::default();
        runner.push_result(1, "", "not installed");
        assert_eq!(runner.flatpak(&["info", "x"]).unwrap().code, 1);
        assert_eq!(runner.flatpak(&["info", "x"]).unwrap().code, 0);
        assert_eq!(runner.calls()[0], vec!["info", "x"]);
    }

    #[test]
    fn clock_is_settable() {
        let c = FakeClock::at(100);
        c.advance(5);
        assert_eq!(c.now(), 105);
        c.set(1);
        assert_eq!(c.now(), 1);
    }

    #[test]
    fn fake_steam_is_a_temp_copy() {
        let seed = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(seed.path().join("userdata/7/config")).unwrap();
        let steam = FakeSteam::new(Some(seed.path())).unwrap();
        assert_eq!(steam.files.user_ids().unwrap(), vec!["7".to_string()]);
    }
}
