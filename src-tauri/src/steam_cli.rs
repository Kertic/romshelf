//! `romshelf steam list` and `romshelf steam apply`.

use crate::cli::SteamCommand;
use romshelf_core::steam::apply::{self, SteamContext};
use romshelf_core::steam::shortcuts;
use std::io::Write;

/// Runs a steam subcommand; returns the exit code.
pub fn run(cmd: SteamCommand, ctx: &SteamContext, out: &mut dyn Write) -> i32 {
    let result = match cmd {
        SteamCommand::List => list(ctx, out),
        SteamCommand::Apply => apply_now(ctx, out),
        SteamCommand::AddSelf => ctx
            .db
            .set_setting(apply::SELF_SETTING, "1")
            .and_then(|_| apply_now(ctx, out)),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(out, "{e}");
            1
        }
    }
}

/// The games Romshelf keeps in Steam, and whether Steam's file has each one yet.
fn list(ctx: &SteamContext, out: &mut dyn Write) -> romshelf_core::Result<()> {
    let user = romshelf_core::steam::active_user(
        ctx.files,
        ctx.db.setting(apply::USER_SETTING)?.as_deref(),
    )?;
    let in_file = match &user {
        Some(u) => shortcuts::list(ctx.files.read_shortcuts(u)?.as_deref())?,
        None => Vec::new(),
    };
    let rows = ctx.db.shortcuts()?;
    if rows.is_empty() && in_file.is_empty() {
        writeln!(out, "No games are in Steam from Romshelf yet.")?;
        return Ok(());
    }
    for row in &rows {
        let state = if in_file.iter().any(|f| f.app_id == row.app_id) {
            "in Steam"
        } else {
            "waiting for `romshelf steam apply`"
        };
        writeln!(
            out,
            "{}  {}  {}  ({state})",
            row.rom_id, row.app_id, row.name
        )?;
    }
    let keep_self = ctx.db.setting(apply::SELF_SETTING)?.as_deref() == Some("1");
    for f in in_file
        .iter()
        .filter(|f| !rows.iter().any(|r| r.app_id == f.app_id))
    {
        // `romshelf steam add-self` asked for this one: apply keeps it.
        if keep_self && f.app_id == apply::self_app_id() {
            writeln!(
                out,
                "romshelf  {}  {}  (in Steam; opens Romshelf)",
                f.app_id, f.name
            )?;
            continue;
        }
        writeln!(
            out,
            "-  {}  {}  (in Steam; removed at the next apply)",
            f.app_id, f.name
        )?;
    }
    if let Some(u) = user {
        writeln!(out, "Steam account: {u}")?;
    }
    Ok(())
}

fn apply_now(ctx: &SteamContext, out: &mut dyn Write) -> romshelf_core::Result<()> {
    let done = apply::apply(ctx)?;
    if !done.restart_needed {
        writeln!(out, "Steam already has every change.")?;
        return Ok(());
    }
    writeln!(
        out,
        "{} added, {} updated, {} removed, {} artwork files changed. Restart Steam to see them.",
        done.added, done.updated, done.removed, done.artwork
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use romshelf_core::db::Database;
    use romshelf_core::paths::Paths;
    use romshelf_testkit::FakeSteam;

    #[test]
    fn romshelfs_own_shortcut_is_listed_as_kept() {
        let seed = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(seed.path().join("userdata/85959576/config")).unwrap();
        let steam = FakeSteam::new(Some(seed.path())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let db = Database::open_in_memory().unwrap();
        let ctx = SteamContext {
            db: &db,
            files: &steam.files,
            paths: &paths,
            now: 1,
        };
        let mut out = Vec::new();
        assert_eq!(run(SteamCommand::AddSelf, &ctx, &mut out), 0);
        let mut out = Vec::new();
        assert_eq!(run(SteamCommand::List, &ctx, &mut out), 0);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains(&format!(
                "romshelf  {}  Romshelf  (in Steam; opens Romshelf)",
                apply::self_app_id()
            )),
            "{text}"
        );
        assert!(!text.contains("removed at the next apply"), "{text}");
    }
}
