//! `romshelf emulators [--setup <id>]`.

use romshelf_core::emulators::setup::{self, SetupContext, SetupEvent};
use romshelf_core::emulators::Registry;
use romshelf_core::{Error, Result};
use std::io::Write;

/// Lists the emulators, or sets one up printing each step and its progress.
pub fn run(
    ctx: &SetupContext,
    registry: &Registry,
    setup_id: Option<&str>,
    out: &mut dyn Write,
) -> i32 {
    let result = (|| -> Result<i32> {
        if let Some(id) = setup_id {
            let adapter = registry.by_id(id).ok_or_else(|| {
                let ids: Vec<&str> = registry.adapters().map(|a| a.id()).collect();
                Error::Invalid(format!(
                    "There's no emulator called {id}. Choose one of: {}.",
                    ids.join(", ")
                ))
            })?;
            let mut last = None;
            let report = setup::run_setup(ctx, adapter, &mut |e| match e {
                SetupEvent::Step { step } => {
                    let _ = writeln!(out, "{}…", step.label());
                }
                SetupEvent::Progress { percent } => {
                    // Every 10% is plenty for a terminal.
                    let bucket = percent / 10;
                    if last != Some(bucket) {
                        last = Some(bucket);
                        let _ = writeln!(out, "  {percent}%");
                    }
                }
            })?;
            writeln!(out, "{} is set up.", adapter.name())?;
            let missing = report.summary();
            if !missing.is_empty() {
                writeln!(out, "{missing}")?;
            }
            return Ok(0);
        }
        for e in setup::list(ctx, registry)? {
            writeln!(
                out,
                "{:<10} {:<16} {}",
                e.name,
                e.systems.join(", "),
                e.status.label()
            )?;
        }
        Ok(0)
    })();
    match result {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(out, "{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use romshelf_core::db::Database;
    use romshelf_core::paths::Paths;
    use romshelf_testkit::{FakeHttp, FakePackageRunner};

    #[test]
    fn lists_then_sets_up_dolphin() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let db = Database::open_in_memory().unwrap();
        let runner = FakePackageRunner::simulating(&[]);
        let http = FakeHttp::default();
        let ctx = SetupContext {
            runner: &runner,
            http: &http,
            db: &db,
            paths: &paths,
            drive: None,
        };
        let registry = Registry::default();

        let mut out = Vec::new();
        assert_eq!(run(&ctx, &registry, None, &mut out), 0);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("Dolphin    GameCube         Not installed"),
            "{text}"
        );

        let mut out = Vec::new();
        assert_eq!(run(&ctx, &registry, Some("dolphin"), &mut out), 0);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("Installing…\n"), "{text}");
        assert!(text.contains("  100%"), "{text}");
        assert!(text.contains("Dolphin is set up."), "{text}");

        let mut out = Vec::new();
        assert_eq!(run(&ctx, &registry, Some("mame"), &mut out), 1);
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("retroarch, dolphin"));
    }
}
