//! In-memory fakes for every outside-world seam, plus the fixture library.
//! Tests use only these. `ROMSHELF_FAKE=1` makes the app use them too.

pub mod connection;
mod drive;
pub mod fixtures;

pub use drive::{FakeDrive, FAKE_ACCOUNT};

use romshelf_core::clock::Clock;
use romshelf_core::http::{HttpRequest, HttpResponse, HttpTransport};
use romshelf_core::process::{CommandOutput, CommandSpec, PackageRunner, ProcessRunner};
use romshelf_core::steam::FsSteamFiles;
use romshelf_core::tokens::TokenStore;
use romshelf_core::{Error, Result};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Records every `flatpak` call and answers from a script; unscripted calls succeed.
#[derive(Default)]
pub struct FakePackageRunner {
    pub calls: Mutex<Vec<Vec<String>>>,
    script: Mutex<VecDeque<CommandOutput>>,
    /// `Some` when simulating Flatpak (see `simulating`).
    installed: Mutex<Option<std::collections::BTreeSet<String>>>,
    /// Pause between simulated progress lines, so an install can be watched.
    pub step_delay: Duration,
}

impl FakePackageRunner {
    /// Unscripted calls behave like Flatpak with these apps installed: `info` answers from the
    /// set and `install` adds to it, reporting progress. Fake mode uses this.
    pub fn simulating(installed: &[&str]) -> FakePackageRunner {
        let runner = FakePackageRunner::default();
        *runner.installed.lock().unwrap() = Some(installed.iter().map(|s| s.to_string()).collect());
        runner
    }

    /// The answer an unscripted call gets.
    fn simulate(&self, args: &[&str]) -> CommandOutput {
        let mut installed = self.installed.lock().unwrap();
        let Some(set) = installed.as_mut() else {
            return CommandOutput::default();
        };
        let id = args.last().copied().unwrap_or_default();
        match args.first().copied() {
            Some("info") if !set.contains(id) => CommandOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("error: {id}/*unspecified*/*unspecified* not installed"),
            },
            Some("install") => {
                set.insert(id.to_string());
                CommandOutput {
                    code: 0,
                    stdout: (1..=10)
                        .map(|i| format!("Installing {id}… {}%\r", i * 10))
                        .collect(),
                    stderr: String::new(),
                }
            }
            _ => CommandOutput::default(),
        }
    }

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
        let scripted = self.script.lock().unwrap().pop_front();
        Ok(scripted.unwrap_or_else(|| self.simulate(args)))
    }

    fn flatpak_streaming(
        &self,
        args: &[&str],
        on_output: &mut dyn FnMut(&str),
    ) -> Result<CommandOutput> {
        let out = self.flatpak(args)?;
        for piece in out.stdout.split(['\r', '\n']).filter(|p| !p.is_empty()) {
            if !self.step_delay.is_zero() {
                std::thread::sleep(self.step_delay);
            }
            on_output(piece);
        }
        Ok(out)
    }
}

/// Returns scripted exit codes after optional delays and records each command.
#[derive(Default)]
pub struct FakeProcessRunner {
    pub runs: Mutex<Vec<CommandSpec>>,
    script: Mutex<VecDeque<(i32, Duration)>>,
    /// A file to look for each time a program "runs" (the launch lock).
    watched: Mutex<Option<std::path::PathBuf>>,
    existed: Mutex<Vec<bool>>,
    /// Runs while each program "runs", as the emulator would (writing a save, say).
    during: Mutex<Option<RunHook>>,
    /// Set by `request_stop`: the running "program" ends early, as a stopped one would.
    stop: std::sync::atomic::AtomicBool,
    /// How many stops were asked for.
    pub stops: std::sync::atomic::AtomicUsize,
}

type RunHook = Box<dyn FnMut(&CommandSpec) + Send>;

impl FakeProcessRunner {
    pub fn push_exit(&self, code: i32, delay: Duration) {
        self.script.lock().unwrap().push_back((code, delay));
    }

    /// Records, at each run, whether `path` exists.
    pub fn watch(&self, path: &Path) {
        *self.watched.lock().unwrap() = Some(path.to_path_buf());
    }

    /// Calls `f` during every run, as if the program did it.
    pub fn during_run(&self, f: impl FnMut(&CommandSpec) + Send + 'static) {
        *self.during.lock().unwrap() = Some(Box::new(f));
    }

    pub fn watched_existed(&self) -> Vec<bool> {
        self.existed.lock().unwrap().clone()
    }

    pub fn runs(&self) -> Vec<CommandSpec> {
        self.runs.lock().unwrap().clone()
    }
}

impl ProcessRunner for FakeProcessRunner {
    fn request_stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
    }

    fn run_and_wait(&self, cmd: &CommandSpec) -> Result<i32> {
        self.runs.lock().unwrap().push(cmd.clone());
        if let Some(p) = self.watched.lock().unwrap().as_ref() {
            self.existed.lock().unwrap().push(p.exists());
        }
        if let Some(f) = self.during.lock().unwrap().as_mut() {
            f(cmd);
        }
        let (code, delay) = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or((0, Duration::ZERO));
        self.stop.store(false, Ordering::SeqCst);
        let until = std::time::Instant::now() + delay;
        while std::time::Instant::now() < until {
            if self.stop.load(Ordering::SeqCst) {
                // Ended by the stop, as SIGTERM would.
                return Ok(143);
            }
            std::thread::sleep(Duration::from_millis(10).min(delay));
        }
        Ok(code)
    }
}

enum Scripted {
    Response(HttpResponse),
    Network(String),
}

/// Answers HTTP requests from a script, in order, and records every request.
#[derive(Default)]
pub struct FakeHttp {
    requests: Mutex<Vec<HttpRequest>>,
    script: Mutex<VecDeque<Scripted>>,
}

impl FakeHttp {
    pub fn push(&self, status: u16, body: &str) {
        self.script
            .lock()
            .unwrap()
            .push_back(Scripted::Response(HttpResponse {
                status,
                body: body.as_bytes().to_vec(),
            }));
    }

    /// A response with a binary body, such as a zipped core.
    pub fn push_bytes(&self, status: u16, body: Vec<u8>) {
        self.script
            .lock()
            .unwrap()
            .push_back(Scripted::Response(HttpResponse { status, body }));
    }

    pub fn push_network_error(&self, message: &str) {
        self.script
            .lock()
            .unwrap()
            .push_back(Scripted::Network(message.into()));
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// How many scripted answers are still unused.
    pub fn remaining(&self) -> usize {
        self.script.lock().unwrap().len()
    }
}

impl HttpTransport for FakeHttp {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        self.requests.lock().unwrap().push(req.clone());
        match self.script.lock().unwrap().pop_front() {
            Some(Scripted::Response(r)) => Ok(r),
            Some(Scripted::Network(m)) => Err(Error::Network(m)),
            None => Err(Error::External(format!(
                "FakeHttp: unscripted {} {}",
                req.method, req.url
            ))),
        }
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

/// Free space that is whatever the test says.
pub struct FixedSpace(AtomicU64);

impl FixedSpace {
    pub fn new(bytes: u64) -> FixedSpace {
        FixedSpace(AtomicU64::new(bytes))
    }
}

impl romshelf_core::storage::DiskSpace for FixedSpace {
    fn available(&self, _path: &Path) -> Result<u64> {
        Ok(self.0.load(Ordering::SeqCst))
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
    fn a_stop_ends_a_running_program_early() {
        let runner = std::sync::Arc::new(FakeProcessRunner::default());
        runner.push_exit(0, Duration::from_secs(5));
        let r = std::sync::Arc::clone(&runner);
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            r.request_stop();
        });
        let started = std::time::Instant::now();
        assert_eq!(
            runner.run_and_wait(&CommandSpec::new("emu", &[])).unwrap(),
            143
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        t.join().unwrap();
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
