//! `romshelf auth …`: set the Google client, sign in and out, report status.

use crate::cli::AuthCommand;
use romshelf_core::auth::ClientConfig;
use romshelf_core::connection::Connection;
use romshelf_core::{Error, Result};
use std::io::{BufRead, Write};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// How long `auth login` waits for the browser.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Runs one auth subcommand and returns the exit code. `open` opens the consent page.
pub fn run(
    cmd: AuthCommand,
    conn: &Connection,
    open: &dyn Fn(&str) -> Result<()>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> i32 {
    match execute(cmd, conn, open, input, out) {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(out, "{e}");
            1
        }
    }
}

fn execute(
    cmd: AuthCommand,
    conn: &Connection,
    open: &dyn Fn(&str) -> Result<()>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<i32> {
    match cmd {
        AuthCommand::SetClient { client_id, file } => {
            let client = match (client_id, file) {
                (_, Some(file)) => {
                    let bytes = std::fs::read(&file).map_err(|e| {
                        Error::Invalid(format!("Couldn't read {file}: {e}. Check the path."))
                    })?;
                    ClientConfig::from_google_json(&bytes)?
                }
                (Some(client_id), None) => {
                    write!(out, "Client secret (press Enter if there is none): ")?;
                    out.flush()?;
                    let mut secret = String::new();
                    input.read_line(&mut secret)?;
                    let secret = secret.trim();
                    ClientConfig {
                        client_id: client_id.trim().into(),
                        client_secret: (!secret.is_empty()).then(|| secret.to_string()),
                    }
                }
                (None, None) => {
                    return Err(Error::Invalid(
                        "Give the client JSON with --file <path>, or the id with --client-id."
                            .into(),
                    ))
                }
            };
            conn.set_client(&client)?;
            writeln!(
                out,
                "Saved your Google client. Next, run `romshelf auth login` or open Romshelf."
            )?;
            Ok(0)
        }
        AuthCommand::Login => {
            writeln!(out, "Opening your browser for Google sign-in…")?;
            // The address goes to stderr, which stays visible even when stdout is redirected.
            let open_or_print = |url: &str| match open(url) {
                Err(Error::NoBrowser) => {
                    eprintln!(
                        "Couldn't open a browser. Open this address in a browser on this Deck:\n{url}"
                    );
                    Ok(())
                }
                other => other,
            };
            let account = conn.login(&open_or_print, LOGIN_TIMEOUT, &AtomicBool::new(false))?;
            writeln!(out, "Signed in as {}.", account.email)?;
            if conn.status()?.drive_folder.is_none() {
                writeln!(
                    out,
                    "Open Romshelf to choose where in Drive to create the Emulation folder."
                )?;
            }
            Ok(0)
        }
        AuthCommand::Logout => {
            conn.logout()?;
            writeln!(
                out,
                "Signed out of Google. Your Drive folder choice is kept for next time."
            )?;
            Ok(0)
        }
        AuthCommand::Status => {
            let s = conn.status()?;
            writeln!(
                out,
                "Google client: {}",
                if s.client_configured {
                    "set"
                } else {
                    "not set"
                }
            )?;
            writeln!(
                out,
                "Account: {}",
                s.account.as_deref().unwrap_or("not signed in")
            )?;
            writeln!(
                out,
                "Drive folder: {}",
                s.drive_folder.as_deref().unwrap_or("not chosen")
            )?;
            if s.signed_in && s.drive_folder.is_some() {
                // Proves Romshelf can see files uploaded from elsewhere (the full Drive scope).
                match conn.rom_file_counts() {
                    Ok(counts) => {
                        let list = counts
                            .iter()
                            .map(|(system, n)| format!("{system} {n}"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        writeln!(out, "Files in roms/: {list}")?;
                    }
                    Err(e) => writeln!(out, "Files in roms/: couldn't check ({e})")?,
                }
            }
            Ok(if s.signed_in { 0 } else { 1 })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use romshelf_core::drive::{DriveClient, ROOT};
    use romshelf_testkit::connection::{simulate_browser, FakeConnection};

    fn run_cmd(fake: &FakeConnection, cmd: AuthCommand, stdin: &str) -> (i32, String) {
        let mut out = Vec::new();
        let code = run(
            cmd,
            &fake.connection,
            &simulate_browser,
            &mut stdin.as_bytes(),
            &mut out,
        );
        (code, String::from_utf8(out).unwrap())
    }

    fn rig() -> (tempfile::TempDir, FakeConnection) {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeConnection::new(dir.path().join("settings.json"));
        (dir, fake)
    }

    #[test]
    fn status_before_and_after_sign_in() {
        let (_d, fake) = rig();
        let (code, text) = run_cmd(&fake, AuthCommand::Status, "");
        assert_eq!(code, 1);
        assert!(text.contains("Account: not signed in"));

        fake.script_login();
        let (code, text) = run_cmd(&fake, AuthCommand::Login, "");
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("Signed in as player@example.com."));
        assert!(text.contains("choose where in Drive"));

        fake.connection
            .setup_folder(ROOT, "My Drive", &mut |_: &str| {})
            .unwrap();
        let (code, text) = run_cmd(&fake, AuthCommand::Status, "");
        assert_eq!(code, 0);
        assert!(text.contains("Account: player@example.com"));
        assert!(text.contains("Drive folder: My Drive / Emulation"));
        assert!(text.contains("Files in roms/: gamecube 0, gba 0, n64 0, ps2 0, snes 0"));

        // A file uploaded from another device shows up.
        let layout = fake.connection.settings().unwrap().drive.unwrap().layout;
        fake.drive
            .create_file("tiny.gba", &layout.rom_folders["gba"], b"x")
            .unwrap();
        let (_, text) = run_cmd(&fake, AuthCommand::Status, "");
        assert!(text.contains("gba 1"), "{text}");
    }

    #[test]
    fn set_client_from_file_and_from_prompt() {
        let (dir, fake) = rig();
        let file = dir.path().join("client.json");
        std::fs::write(
            &file,
            br#"{"installed":{"client_id":"new.apps.googleusercontent.com","client_secret":"s"}}"#,
        )
        .unwrap();
        let (code, text) = run_cmd(
            &fake,
            AuthCommand::SetClient {
                client_id: None,
                file: Some(file.display().to_string()),
            },
            "",
        );
        assert_eq!(code, 0, "{text}");
        assert_eq!(
            fake.connection.auth.client().unwrap().client_id,
            "new.apps.googleusercontent.com"
        );

        let (code, _) = run_cmd(
            &fake,
            AuthCommand::SetClient {
                client_id: Some("typed.apps.googleusercontent.com".into()),
                file: None,
            },
            "typed-secret\n",
        );
        assert_eq!(code, 0);
        let client = fake.connection.auth.client().unwrap();
        assert_eq!(client.client_secret.as_deref(), Some("typed-secret"));
    }

    #[test]
    fn set_client_errors_say_what_to_do() {
        let (_d, fake) = rig();
        let (code, text) = run_cmd(
            &fake,
            AuthCommand::SetClient {
                client_id: None,
                file: None,
            },
            "",
        );
        assert_eq!(code, 1);
        assert!(text.contains("--file"));
        let (code, text) = run_cmd(
            &fake,
            AuthCommand::SetClient {
                client_id: None,
                file: Some("/does/not/exist.json".into()),
            },
            "",
        );
        assert_eq!(code, 1);
        assert!(text.contains("Check the path"));
    }

    #[test]
    fn logout_reports_and_signs_out() {
        let (_d, fake) = rig();
        fake.script_login();
        run_cmd(&fake, AuthCommand::Login, "");
        fake.http.push(200, "");
        let (code, text) = run_cmd(&fake, AuthCommand::Logout, "");
        assert_eq!(code, 0);
        assert!(text.contains("Signed out"));
        assert!(!fake.connection.status().unwrap().signed_in);
    }
}
