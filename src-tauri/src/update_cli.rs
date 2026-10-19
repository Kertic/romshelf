//! `romshelf update`: install a newer build now, roll back, or set the GitHub token.

use crate::cli::UpdateArgs;
use romshelf_core::tokens::TokenStore;
use romshelf_core::update::{Outcome, Updater, GITHUB_TOKEN_KEY};
use romshelf_core::Result;
use std::io::{BufRead, Write};

const NOT_APPIMAGE: &str =
    "This copy of Romshelf isn't running from its AppImage, so it can't update itself.";

/// Runs `romshelf update` and returns the exit code. `updater` is `None` outside an AppImage.
pub fn run(
    args: UpdateArgs,
    updater: Option<&Updater>,
    tokens: &dyn TokenStore,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> i32 {
    match execute(args, updater, tokens, input, out) {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(out, "{e}");
            1
        }
    }
}

fn execute(
    args: UpdateArgs,
    updater: Option<&Updater>,
    tokens: &dyn TokenStore,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<i32> {
    if args.set_token {
        write!(out, "GitHub token (press Enter to remove the stored one): ")?;
        out.flush()?;
        let mut token = String::new();
        input.read_line(&mut token)?;
        let token = token.trim();
        if token.is_empty() {
            tokens.delete(GITHUB_TOKEN_KEY)?;
            writeln!(out, "Removed the GitHub token.")?;
        } else {
            tokens.set(GITHUB_TOKEN_KEY, token)?;
            writeln!(
                out,
                "Saved the GitHub token. Run `romshelf update` to check now."
            )?;
        }
        return Ok(0);
    }
    let Some(updater) = updater else {
        writeln!(out, "{NOT_APPIMAGE}")?;
        return Ok(1);
    };
    if args.rollback {
        updater.rollback()?;
        writeln!(
            out,
            "Rolled back to the previous version of Romshelf. Version {} won't be installed again automatically.",
            updater.current
        )?;
        return Ok(0);
    }
    let code = match updater.run(true)? {
        Outcome::NotConfigured => {
            writeln!(
                out,
                "This build of Romshelf has no update key, so it can't check for updates."
            )?;
            1
        }
        Outcome::LaunchInProgress => {
            writeln!(
                out,
                "A game is starting or running. Try again after it closes."
            )?;
            1
        }
        Outcome::UpToDate | Outcome::NotDue => {
            writeln!(out, "Romshelf {} is up to date.", updater.current)?;
            0
        }
        Outcome::Updated(v) => {
            writeln!(
                out,
                "Updated to Romshelf {v}. It runs from the next start. To go back, run `romshelf update --rollback`."
            )?;
            0
        }
    };
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use romshelf_core::update::Version;
    use romshelf_testkit::{FakeClock, FakeHttp, MemoryTokenStore};
    use std::sync::Arc;

    fn updater(dir: &std::path::Path, http: Arc<FakeHttp>) -> Updater {
        let install = dir.join("romshelf.AppImage");
        std::fs::write(&install, b"\x7fELF").unwrap();
        Updater {
            http,
            clock: Arc::new(FakeClock::at(1_800_000_000)),
            feed_url: "https://example.invalid/latest".into(),
            public_key: [1; 32],
            token: None,
            current: Version(0, 1, 0),
            install,
            state_file: dir.join("update.json"),
            launch_lock: dir.join("launch.lock"),
        }
    }

    fn run_cmd(
        args: UpdateArgs,
        u: Option<&Updater>,
        tokens: &MemoryTokenStore,
        stdin: &str,
    ) -> (i32, String) {
        let mut out = Vec::new();
        let code = run(args, u, tokens, &mut stdin.as_bytes(), &mut out);
        (code, String::from_utf8(out).unwrap())
    }

    #[test]
    fn up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let http = Arc::new(FakeHttp::default());
        http.push(200, r#"{"tag_name":"v0.1.0","assets":[]}"#);
        let u = updater(dir.path(), http);
        let (code, text) = run_cmd(
            UpdateArgs::default(),
            Some(&u),
            &MemoryTokenStore::default(),
            "",
        );
        assert_eq!(code, 0);
        assert_eq!(text.trim(), "Romshelf 0.1.0 is up to date.");
    }

    #[test]
    fn outside_an_appimage_explains_why_not() {
        let (code, text) = run_cmd(
            UpdateArgs::default(),
            None,
            &MemoryTokenStore::default(),
            "",
        );
        assert_eq!(code, 1);
        assert!(text.contains("isn't running from its AppImage"));
    }

    #[test]
    fn rollback_without_previous_is_an_error_message() {
        let dir = tempfile::tempdir().unwrap();
        let u = updater(dir.path(), Arc::new(FakeHttp::default()));
        let args = UpdateArgs {
            rollback: true,
            set_token: false,
        };
        let (code, text) = run_cmd(args, Some(&u), &MemoryTokenStore::default(), "");
        assert_eq!(code, 1);
        assert!(text.contains("No previous version"));
    }

    #[test]
    fn set_token_stores_and_removes() {
        let tokens = MemoryTokenStore::default();
        let args = || UpdateArgs {
            rollback: false,
            set_token: true,
        };
        let (code, _) = run_cmd(args(), None, &tokens, "github_pat_x\n");
        assert_eq!(code, 0);
        assert_eq!(
            tokens.get(GITHUB_TOKEN_KEY).unwrap().as_deref(),
            Some("github_pat_x")
        );
        run_cmd(args(), None, &tokens, "\n");
        assert_eq!(tokens.get(GITHUB_TOKEN_KEY).unwrap(), None);
    }
}
