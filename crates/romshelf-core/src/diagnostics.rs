//! Logs that don't grow without end, and the "Copy diagnostics" text: versions, the state of
//! things, `doctor`'s checks and recent launches, with tokens, secrets and ROM names taken out.

use crate::db::Database;
use crate::paths::Paths;
use std::path::Path;

/// A log is rotated past this size.
pub const LOG_MAX_BYTES: u64 = 1024 * 1024;
/// Rotated logs kept: `launch.log.1` (newest) to `launch.log.3`.
pub const LOGS_KEPT: usize = 3;

/// Moves `file` to `file.1` (and `.1` to `.2`, …) once it passes `max_bytes`; the oldest goes.
pub fn rotate(file: &Path, max_bytes: u64, kept: usize) -> std::io::Result<()> {
    let big = std::fs::metadata(file).is_ok_and(|m| m.len() > max_bytes);
    if !big || kept == 0 {
        return Ok(());
    }
    let numbered = |n: usize| {
        let mut name = file.as_os_str().to_owned();
        name.push(format!(".{n}"));
        std::path::PathBuf::from(name)
    };
    let _ = std::fs::remove_file(numbered(kept));
    for n in (1..kept).rev() {
        let from = numbered(n);
        if from.exists() {
            std::fs::rename(&from, numbered(n + 1))?;
        }
    }
    std::fs::rename(file, numbered(1))
}

/// Replaces what must not leave the Deck: Google tokens and client secrets, and every ROM's file
/// name and display name.
pub fn redact(text: &str, rom_names: &[String]) -> String {
    let mut out = String::with_capacity(text.len());
    for word in
        text.split_inclusive(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '=')
    {
        let bare =
            word.trim_end_matches(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '=');
        let sep = &word[bare.len()..];
        // Access tokens (ya29.…), refresh tokens (1//…), client secrets (GOCSPX-…).
        if bare.starts_with("ya29.") || bare.starts_with("1//") || bare.starts_with("GOCSPX-") {
            out.push_str("<redacted>");
            out.push_str(sep);
        } else {
            out.push_str(word);
        }
    }
    let mut names: Vec<&String> = rom_names.iter().filter(|n| n.len() > 2).collect();
    // Longest first, so "Kirby (USA).gba" goes before "Kirby".
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for n in names {
        out = out.replace(n.as_str(), "<rom>");
    }
    out
}

/// The text "Copy diagnostics" puts on the clipboard. `doctor` is the checks' report.
pub fn bundle(db: &Database, paths: &Paths, version: &str, doctor: &str) -> String {
    let mut s = format!(
        "Romshelf {version}\nData: {}\nStorage: {}\n\nChecks:\n{doctor}\n",
        paths.data_dir.display(),
        paths.storage_root().display()
    );
    let roms = db.roms().unwrap_or_default();
    s.push_str(&format!(
        "Games: {} ({} on this Deck)\n",
        roms.len(),
        roms.iter()
            .filter(|r| r.location == crate::model::LocationState::OnDeck)
            .count()
    ));
    s.push_str("\nRecent launches:\n");
    let dir = paths.log_dir();
    let mut lines: Vec<String> = Vec::new();
    for name in ["launch.log.1", "launch.log"] {
        if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
            lines.extend(text.lines().map(str::to_string));
        }
    }
    let start = lines.len().saturating_sub(40);
    for l in &lines[start..] {
        s.push_str("  ");
        s.push_str(l);
        s.push('\n');
    }
    let names: Vec<String> = roms
        .iter()
        .flat_map(|r| {
            let stem = Path::new(&r.file_name)
                .file_stem()
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_default();
            [r.file_name.clone(), r.display_name.clone(), stem]
        })
        .collect();
    redact(&s, &names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_keeps_three_and_drops_the_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("launch.log");
        for i in 0..5 {
            std::fs::write(&log, format!("generation {i} {}", "x".repeat(20))).unwrap();
            rotate(&log, 10, 3).unwrap();
        }
        assert!(!log.exists());
        let read =
            |n: usize| std::fs::read_to_string(dir.path().join(format!("launch.log.{n}"))).unwrap();
        assert!(read(1).starts_with("generation 4"));
        assert!(read(3).starts_with("generation 2"));
        assert!(!dir.path().join("launch.log.4").exists());
    }

    #[test]
    fn a_small_log_stays() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("launch.log");
        std::fs::write(&log, "short").unwrap();
        rotate(&log, LOG_MAX_BYTES, LOGS_KEPT).unwrap();
        assert!(log.exists());
    }

    #[test]
    fn tokens_secrets_and_rom_names_are_taken_out() {
        let text = "token=ya29.a0AfH6SMBx refresh \"1//0gLx-abc\" secret GOCSPX-abc123\n\
                    error opening /home/deck/Romshelf/roms/gba/Kirby (USA).gba for Kirby\n";
        let out = redact(text, &["Kirby (USA).gba".into(), "Kirby".into()]);
        assert!(!out.contains("ya29"));
        assert!(!out.contains("1//"));
        assert!(!out.contains("GOCSPX"));
        assert!(!out.contains("Kirby"));
        assert!(out.contains("roms/gba/<rom> for <rom>"));
        assert!(out.contains("token=<redacted> refresh \"<redacted>\" secret <redacted>"));
    }
}
