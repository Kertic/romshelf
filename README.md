# Romshelf

Your ROMs live in Google Drive; Romshelf brings them to your Steam Deck and runs them from your Steam library.

You upload games to a Drive folder from any device. On the Deck, Romshelf lists them and downloads the ones you want. It installs the emulators, backs up your saves to Drive, keeps your controls, and puts each game in Steam with its own artwork. Every game's Steam shortcut runs Romshelf, which then starts the right emulator with the right save and controls.

## Install

In Desktop Mode, open Konsole and run:

```sh
curl -fsSL https://raw.githubusercontent.com/Kertic/romshelf/main/scripts/install.sh | sh
```

This puts Romshelf at `~/Applications/romshelf.AppImage`, adds a `romshelf` command and a desktop entry, and adds Romshelf to Steam. Restart Steam, then open Romshelf from your library (it works in Game Mode).

Romshelf updates itself: when a newer version is out, it downloads it in the background and uses it from the next start. `romshelf update --rollback` goes back one version.

## Set up Google Drive

Romshelf signs in with a Google Cloud client of your own, so your Drive is only ever shared with your own project. Follow [docs/google-setup.md](docs/google-setup.md) once (about ten minutes), then sign in from Romshelf and choose where its `Emulation` folder goes.

## Add games

Upload ROMs to `Emulation/roms/<system>/` in Drive from any device: `snes`, `gba`, `n64`, `gamecube` (and `ps2`, which isn't playable yet). They appear in Romshelf at the next sync. Put BIOS files an emulator needs in `Emulation/bios/<system>/`.

Romshelf runs SNES, GBA and N64 games in RetroArch, and GameCube games in Dolphin. It installs both from Flathub for your user when a game needs them.

## Playing

- Choose a game in Romshelf and press Play, or add it to Steam (its page, then **Add to Steam library**) and start it from Steam like any game.
- **Hold View and Menu for a second** to quit any game. Steam's own Exit game works too.
- The back grips are hotkeys by default: L4 fast forward, R4 rewind (RetroArch), L5 save state, R5 load state. Change any button for one game or for every game on **Edit controls**.
- Your save is backed up to Drive when you quit, and the newest from another Deck is fetched before you play. Restore any of the last ten backups from the game's page.

## What Romshelf touches, and what it never does

Romshelf **never deletes, moves, renames or changes your ROM files in Drive**. It reads ROMs from Drive and only writes in folders it created: `Emulation/saves/`, `Emulation/states/` and `Emulation/.romshelf/`. Old save backups beyond the number you keep go to Drive's trash, where they can be recovered.

On the Deck it writes:

- `~/Romshelf/roms/` (or an SD card you choose): your downloaded games. **Remove from Deck** deletes only this copy.
- `~/.local/share/romshelf/` and `~/.config/romshelf/`: its index, logs and settings.
- Its own settings folders inside each emulator's Flatpak data: Romshelf never edits the settings you or EmuDeck made.
- Steam's `shortcuts.vdf`: only shortcuts tagged `Romshelf`, after a backup (the last five are kept beside it); the artwork for those shortcuts; and their controller layout.

## When something's wrong

Run `romshelf doctor` in a terminal. It checks the sign-in, the Drive folder, storage, the emulators, Steam and the shortcuts, and gives one fix for each problem. In Romshelf, **Settings → Copy diagnostics** copies the same report, without tokens, secrets or game names, to paste wherever you're asking for help.

## Building

See [CLAUDE.md](CLAUDE.md) for the layout and every build and test command, and [docs/releasing.md](docs/releasing.md) for releases.
