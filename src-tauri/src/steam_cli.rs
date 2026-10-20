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
    for f in in_file
        .iter()
        .filter(|f| !rows.iter().any(|r| r.app_id == f.app_id))
    {
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
