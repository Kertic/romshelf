//! Command-line interface. No subcommand opens the full-screen interface.

use clap::{ArgGroup, Args, Parser, Subcommand};

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "romshelf", version, about = "Steam Deck emulation manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Start a game. Steam shortcuts run `romshelf launch --id <rom-id>`.
    Launch(LaunchArgs),
    /// Update the library from Google Drive.
    Sync,
    /// Check the setup, or print what a launch would do.
    Doctor {
        #[arg(long)]
        id: Option<String>,
    },
    /// Google sign-in.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Steam shortcuts.
    #[command(subcommand)]
    Steam(SteamCommand),
}

#[derive(Debug, Args, PartialEq, Eq)]
#[command(group(ArgGroup::new("target").required(true).args(["id", "rom"])))]
pub struct LaunchArgs {
    /// ROM id, for example `gba-0a1b2c3d`.
    #[arg(long)]
    pub id: Option<String>,
    /// ROM path relative to the ROM root, for example `gba/tidewalker.gba`.
    #[arg(long)]
    pub rom: Option<String>,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum AuthCommand {
    /// Store your own Google Cloud desktop client id and secret.
    SetClient {
        #[arg(long)]
        client_id: Option<String>,
        /// Path to the client JSON downloaded from Google Cloud.
        #[arg(long)]
        file: Option<String>,
    },
    Login,
    Logout,
    Status,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum SteamCommand {
    /// Write pending shortcut changes to Steam.
    Apply,
    /// List Romshelf's shortcuts.
    List,
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Command::Launch(_) => "launch",
            Command::Sync => "sync",
            Command::Doctor { .. } => "doctor",
            Command::Auth(_) => "auth",
            Command::Steam(_) => "steam",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("romshelf").chain(args.iter().copied()))
    }

    #[test]
    fn no_arguments_opens_the_interface() {
        assert_eq!(parse(&[]).unwrap().command, None);
    }

    #[test]
    fn launch_by_id_and_by_path() {
        assert_eq!(
            parse(&["launch", "--id", "gba-0a1b2c3d"]).unwrap().command,
            Some(Command::Launch(LaunchArgs {
                id: Some("gba-0a1b2c3d".into()),
                rom: None
            }))
        );
        assert_eq!(
            parse(&["launch", "--rom", "gba/tidewalker.gba"])
                .unwrap()
                .command,
            Some(Command::Launch(LaunchArgs {
                id: None,
                rom: Some("gba/tidewalker.gba".into())
            }))
        );
    }

    #[test]
    fn launch_needs_exactly_one_target() {
        assert!(parse(&["launch"]).is_err());
        assert!(parse(&["launch", "--id", "a", "--rom", "b"]).is_err());
    }

    #[test]
    fn every_other_subcommand_parses() {
        assert_eq!(parse(&["sync"]).unwrap().command, Some(Command::Sync));
        assert_eq!(
            parse(&["doctor"]).unwrap().command,
            Some(Command::Doctor { id: None })
        );
        assert_eq!(
            parse(&["doctor", "--id", "snes-00000001"]).unwrap().command,
            Some(Command::Doctor {
                id: Some("snes-00000001".into())
            })
        );
        for (args, expected) in [
            (vec!["auth", "login"], AuthCommand::Login),
            (vec!["auth", "logout"], AuthCommand::Logout),
            (vec!["auth", "status"], AuthCommand::Status),
            (
                vec!["auth", "set-client", "--client-id", "x"],
                AuthCommand::SetClient {
                    client_id: Some("x".into()),
                    file: None,
                },
            ),
            (
                vec!["auth", "set-client", "--file", "c.json"],
                AuthCommand::SetClient {
                    client_id: None,
                    file: Some("c.json".into()),
                },
            ),
        ] {
            assert_eq!(parse(&args).unwrap().command, Some(Command::Auth(expected)));
        }
        assert_eq!(
            parse(&["steam", "apply"]).unwrap().command,
            Some(Command::Steam(SteamCommand::Apply))
        );
        assert_eq!(
            parse(&["steam", "list"]).unwrap().command,
            Some(Command::Steam(SteamCommand::List))
        );
    }

    #[test]
    fn unknown_input_is_rejected() {
        assert!(parse(&["frobnicate"]).is_err());
        assert!(parse(&["auth"]).is_err());
        assert!(parse(&["steam", "nope"]).is_err());
        assert!(parse(&["doctor", "--bogus"]).is_err());
    }
}
