# Romshelf: software description and implementation plan

*Working name. Drafted 10 October 2026. Companion to the "Deck ROM Manager UI Concepts" canvas (screens 1 to 4).*

## 1. Summary

Romshelf is a Steam Deck application that manages emulation from one place. ROMs live in a Google Drive folder that the app creates and structures; the user uploads their own files there from any device. On the Deck, Romshelf lists what is in Drive, downloads and deletes ROMs, installs and configures emulators, backs up saves, manages controls, and publishes each game to the Steam library as a shortcut with its own name and artwork.

Every shortcut launches Romshelf itself with arguments naming the ROM. Romshelf then decides which emulator to start, with which controls and which save. That one rule keeps every later change (a different emulator, a new mapping, a moved file) inside Romshelf, and no shortcut ever has to be re-created.

This document describes the software, gives two ways to build it, and lays out development stages with implementation steps, verification tests, and effort estimates for building it with Claude Code.

**Recommendation:** Proposal A (standalone app) because it matches the approved screens and the "Steam runs `romshelf`" contract directly. Proposal B (Decky plugin) reaches a working Deck build sooner and is the better choice if speed matters more than owning the full UI.

Romshelf ships no ROMs and no BIOS files. Users supply their own.

## 2. What the software does

### 2.1 Drive folder

On first run the user signs in to Google and picks a location. Romshelf creates:

```
<chosen location>/Emulation/
  roms/<system>/            ROM files, uploaded by the user
  saves/<system>/<game>/    timestamped save backups
  states/<system>/<game>/   save states
  bios/<system>/            user-supplied BIOS files
  .romshelf/                index, control profiles, artwork, settings
```

A ROM is identified by the system folder it sits in plus its file extension. Each ROM gets a stable ID stored in `.romshelf/index.json`, so renaming or moving a file does not break saves, controls or shortcuts.

### 2.2 Screens

| Screen | Purpose |
|---|---|
| 1 · Library | Cover grid, system tabs on L1/R1, sync status, quick-action drawer for the selected ROM |
| 2 · ROM page | Play, download, delete, emulator functions, Controls card, Steam shortcut card |
| 2b · ROM page, emulator missing | Setup card for the emulator; download, delete and "Add to Steam" stay available |
| 3 · Controls | One editor with a switch between the emulator default and this game only |
| 4 · Steam shortcut | Name, five artwork slots, library preview, launch definition |

### 2.3 Actions on a ROM

Always available: download to Deck, delete (from Deck, Drive, or both, with confirmation), add to or remove from Steam, set up the emulator if it is missing.

Emulator-dependent: play, back up and restore save data, manage save states, edit controls, plus anything the emulator adapter declares (cheats, memory cards, texture packs). The UI builds this list from the adapter, so adding an emulator adds its actions without UI changes.

### 2.4 Controls model

Each emulator has one default layout. Each game stores only its differences from that default. At launch, Romshelf merges the two and writes the result into the emulator's own configuration. Changing the default therefore updates every game that has not overridden that button.

### 2.5 Launch contract

Steam shortcut: target `romshelf`, launch options `launch --id <rom-id>`. A path form, `launch --rom gba/tidewalker.gba`, is also accepted for hand use. Shortcuts are written with the ID form so that file renames are safe.

What `romshelf launch` does, in order:

1. Resolve the ROM from the local index.
2. If the ROM is not on the Deck, download it with a progress screen.
3. If the emulator is missing, open the setup flow, then continue.
4. If Drive holds a newer save than the Deck, offer to restore it.
5. Merge default and per-game controls and write the emulator configuration.
6. Start the emulator and wait for it to exit.
7. Back up the save to Drive, then exit with the emulator's exit code.

## 3. Platform facts that shape the design

These were checked against current documentation on 10 October 2026. Sources are listed at the end.

1. **Drive permissions.** Google's narrow `drive.file` permission only covers files the app itself created or that the user explicitly opened with the app. ROMs the user drops into the folder through the Drive website are not visible under it. Reading them needs the full `drive` permission, which Google classes as restricted.
2. **Sign-in flow.** Google's code-on-another-device sign-in flow only supports `drive.file` and `drive.appdata`, so it cannot be used. Sign-in has to be the desktop browser flow, opened on the Deck.
3. **Publishing status.** A Google Cloud project left in "Testing" expires each authorization after seven days. A project set to "In production" without verification keeps authorizations, shows an "unverified app" warning, and is capped at 100 users for its lifetime. That is fine for personal use. Public distribution would need Google's restricted-scope verification, which only certain app categories qualify for.
4. **Steam shortcuts.** Non-Steam shortcuts are stored in a binary file, `userdata/<user>/config/shortcuts.vdf`. Steam only reads it at start-up, so a shortcut written to the file appears after Steam restarts. Decky plugins can instead call Steam's internal `SteamClient.Apps.AddShortcut` and related functions, which take effect immediately but are undocumented and have quirks (properties must be set in separate calls after a short delay).
5. **Artwork.** Custom artwork is a set of image files in `userdata/<user>/config/grid/`, named after the shortcut's app ID with a suffix per slot. The exact suffix for each slot should be confirmed on the Deck during stage 8; community documentation is inconsistent about two of them.

Not checked here, from general knowledge: the common emulators (RetroArch, Dolphin, PCSX2, DuckStation, mGBA) are available as Flatpaks that install per user without unlocking SteamOS.

## 4. Two proposals

### Proposal A: standalone app

One program, `romshelf`, with two faces: a full-screen interface and the `launch` command. Built with Tauri: a Rust core for Drive, files, emulators and Steam, with a web front end that reuses the layout and styles from the approved screens. Talks to the Google Drive API directly using the user's own Google Cloud client. Writes Steam shortcuts and artwork to Steam's files and asks for a Steam restart when shortcuts change. Packaged as an AppImage and added to Steam once as a non-Steam game.

**Why choose it**

- The approved screens can be built as drawn; the canvas markup is close to production HTML and CSS.
- The launch contract is literal: the thing Steam runs is the same binary that owns all the logic.
- No dependency on Decky Loader or on undocumented Steam functions, so Steam client updates are unlikely to break it.
- The Rust core is testable without a Deck; most stages can be verified on a development machine.

**What it costs**

- New shortcuts need a Steam restart to appear. Mitigation: batch changes and show one "Restart Steam to apply" prompt.
- Gamepad navigation has to be built (focus movement, button hints). Gamepad support inside the Linux web view is a known risk; the fallback is reading the pad in Rust and forwarding events, or switching the shell to Electron.
- Google sign-in and token handling are written by hand.

### Proposal B: Decky plugin with a launcher

A Decky Loader plugin: a React front end that lives inside the Steam interface, a Python back end, and a small `romshelf` launcher script that the shortcuts run. Uses rclone for all Drive traffic, so sign-in, transfers and retries come from a mature tool. Creates shortcuts and artwork live through Steam's internal functions, with no restart.

**Why choose it**

- Fastest path to something usable on the Deck; rclone and Steam's own UI components remove two large pieces of work.
- Shortcuts and artwork appear immediately.
- Gamepad focus and navigation come for free from Steam's component library.
- Never leaves Game Mode.

**What it costs**

- The screens would use Steam's components, so they will look like Steam, not like the approved designs. Full-page routes are possible but more constrained.
- Depends on Decky Loader and on undocumented Steam functions that can change with a client update.
- Logic is split three ways (front end, Python back end, launcher script), which weakens the "everything is in one place" goal. The launcher must work even when the plugin is not loaded.
- rclone still needs a Google client ID with the same permission; its shared built-in one is rate limited, so the user's own is still recommended.

### Comparison

| | A: standalone app | B: Decky plugin |
|---|---|---|
| Stack | Tauri, Rust, web UI | React, Python, rclone |
| Matches approved screens | Yes | Partly; Steam look |
| New shortcut appears | After Steam restart | Immediately |
| Gamepad navigation | Must be built | Provided |
| Drive access | Direct API, own sign-in code | rclone |
| Exposure to Steam updates | Low | Medium to high |
| Testable off the Deck | Most of it | Back end only |
| Total effort (section 6) | 74 to 116 hours | 60 to 92 hours |

### Reasoning

Both proposals share the same data model, Drive layout, emulator adapters and launch contract, so the choice is mostly about the shell. A is the better long-term home for the requirement that all changes stay centralized in one executable, and it delivers the UI that was approved. B trades design control and stability for speed and tighter Steam integration.

A middle path keeps the option open: build A's core as a library with a command-line interface first (stages 0 to 8), and a Decky front end can be added later as a thin layer over the same commands if live shortcuts become important.

## 5. Development stages

Each stage ends with automated tests passing and the listed on-Deck checks done. "Fake Drive" means an in-memory stand-in used by tests, so nothing touches a real account. Stages are the same for both proposals unless a difference is noted.

### Stage 0: foundations

**Steps**

1. Create the repository, build scripts, continuous integration, and a `CLAUDE.md` describing conventions and the stage plan.
2. Define the data model: system, ROM, emulator, control profile, shortcut. Store the local index in SQLite.
3. Add the command skeleton: `romshelf`, `romshelf launch`, `romshelf sync`, `romshelf doctor`.
4. Define the emulator adapter interface (install, detect, launch arguments, save paths, control writer, declared actions).

**Tests**

- Unit: model round-trips through the database; argument parsing for every command.
- CI builds a runnable artifact for SteamOS.
- On Deck: the empty app starts in Game Mode and exits cleanly.

### Stage 1: Google Drive connection and folder scaffold

**Steps**

1. Create the Google Cloud project and desktop client; set publishing status to "In production".
2. Implement browser sign-in on the Deck and secure token storage. *(B: configure an rclone remote instead.)*
3. Folder picker for the location; create the directory tree idempotently.
4. Write `.romshelf/settings.json` with the folder IDs.

**Tests**

- Unit against fake Drive: scaffold creates every folder once; running it twice changes nothing; a partly existing tree is completed, not duplicated.
- Integration against a throwaway Google account: sign in, scaffold, sign out, sign in again without re-consent.
- Spike to confirm fact 1: upload a file through the Drive website and confirm the app can list it.
- On Deck: token still valid after eight days.

### Stage 2: library index and sync

**Steps**

1. List `roms/<system>/` and match files to systems by folder and extension.
2. Assign stable IDs; persist to the local database and to `.romshelf/index.json`.
3. Incremental sync using Drive's change feed; handle renames, moves and deletions.
4. Derive display names from file names; mark unknown extensions as "unrecognized".

**Tests**

- Unit: extension table per system; a rename keeps the same ID; a deleted file is marked gone but its saves are kept.
- Integration: 500 fake files sync in one pass; a second sync with no changes makes no writes.
- On Deck: upload a ROM from a phone, press Sync, see it appear in the right system.

### Stage 3: download, delete and storage

**Steps**

1. Resumable, checksummed downloads with a queue and progress events.
2. Delete with three scopes: Deck copy, Drive copy, both. Drive deletes go to Drive's trash.
3. Free-space checks before download; choice of internal storage or SD card.

**Tests**

- Unit: an interrupted download resumes and the checksum matches; a full disk stops cleanly with a clear message.
- Unit: each delete scope removes exactly what it says and nothing else.
- On Deck: pull the network mid-download, restore it, download completes; deleted file is in Drive trash.

### Stage 4: emulator registry and setup

**Steps**

1. Registry mapping each system to an emulator and, where relevant, a core.
2. Adapters for a first set: RetroArch (SNES, GBA, N64) and Dolphin (GameCube). PCSX2 follows the same pattern.
3. Detect installed emulators; install missing ones as per-user Flatpaks with progress.
4. BIOS check: copy required files from `bios/` and report what is missing.
5. The three-step setup flow from screen 2b.

**Tests**

- Unit with a fake Flatpak runner: detection, install command, failure handling.
- Unit: the registry resolves every supported extension to exactly one adapter.
- On Deck: with Dolphin absent, the setup flow installs it and the ROM page changes from 2b to 2.

### Stage 5: launch pipeline

**Steps**

1. Implement the seven launch steps from section 2.5.
2. Build launch commands per adapter; start the emulator as a child process and wait for it.
3. Small full-screen progress and error views for use when launched from Steam.
4. `romshelf doctor --id` prints what a launch would do without doing it.

**Tests**

- Unit with a fake emulator binary: correct arguments; exit code passed through; each precondition (not downloaded, emulator missing) triggers the right step.
- Unit: launch by ID and by path resolve to the same ROM.
- On Deck: launch from the Library; quit the emulator and return to Romshelf or Steam cleanly.

### Stage 6: saves and save states

**Steps**

1. Point each emulator's save and state folders at Romshelf-managed locations.
2. Back up on exit and on demand into `saves/<system>/<game>/<timestamp>/`; keep a configurable number of versions.
3. Restore a chosen version; before overwriting, back up the current local save.
4. Conflict rule: if both sides changed since the last sync, ask, never overwrite silently.

**Tests**

- Unit: backup then restore returns byte-identical files; version pruning keeps the newest N.
- Unit: every conflict case (local newer, Drive newer, both changed) produces the documented outcome.
- On Deck: save in game, quit, confirm the backup timestamp; delete the local save, restore, progress is intact.

### Stage 7: controls

**Steps**

1. Canonical model: a set of Deck inputs, the emulator's actions, a default per emulator, a per-game difference.
2. Merge function and a writer per adapter that renders the merged layout into the emulator's native configuration at launch.
3. Editor from screen 3: rebind, clear, return a row to default, reset the game, promote the game's layout to the default.
4. Store profiles in `.romshelf/` so they follow the user to another Deck.

**Tests**

- Unit: merge rules (override wins, cleared binding stays cleared, untouched rows follow a changed default).
- Golden-file tests: a known layout produces the expected emulator configuration for each adapter.
- On Deck: map L4 to fast forward for one game only; it works there and not in another game on the same emulator. Change the default; both games pick it up except for overridden buttons.

### Stage 8: Steam shortcuts and artwork

**Steps**

1. Read and write the shortcuts file safely: back it up first, preserve entries Romshelf did not create, write atomically. *(B: use Steam's internal functions instead.)*
2. Generate a deterministic app ID per ROM and tag entries as Romshelf's.
3. Artwork: accept local files per slot, generate a fallback from the title, optionally search an artwork service; write to the grid folder.
4. Shortcut editor from screen 4; "add every new ROM automatically" setting; restart prompt. *(B: no prompt needed.)*
5. Removing a ROM removes its shortcut and artwork.

**Tests**

- Unit: parse and re-write a real shortcuts file with no byte changes; add, rename and remove one entry without disturbing others.
- Unit: the same ROM always gets the same app ID.
- On Deck: shortcut appears with the right name and all artwork slots; launching it from Steam runs the full pipeline, including save backup on exit.
- On Deck: rename the ROM file in Drive, sync, launch the shortcut; it still works.

### Stage 9: interface

**Steps**

1. Build Library, ROM page, Controls and Steam shortcut screens from the canvas. *(B: build with Steam's components as plugin routes.)*
2. Gamepad navigation, button hints, on-screen keyboard for text fields. *(B: mostly provided.)*
3. Empty, loading, offline and error states for every screen.

**Tests**

- Component tests for each screen state.
- End-to-end test driving the app with simulated gamepad input through: sync, open a ROM, download, play, back up.
- On Deck: every action reachable without touch; readable at arm's length.

### Stage 10: packaging and hardening

**Steps**

1. Package (A: AppImage; B: plugin archive plus launcher install), with self-update.
2. Logging, a diagnostics export, and `romshelf doctor` checks for common faults.
3. Offline behaviour: playing downloaded games works with no network; backups queue until online.

**Tests**

- Fresh-install test on a factory-reset SteamOS image or second Deck profile.
- Upgrade test: old version with data, upgrade, nothing lost.
- On Deck: airplane mode, launch a downloaded game from Steam, play, quit; backup uploads when the network returns.

## 6. Estimates for building with Claude Code

Hours are the developer's own time: directing Claude Code, reviewing its changes, and testing on the Deck. They assume one experienced engineer, Claude Code writing most of the code and tests, and a Deck available for the on-device checks. They are judgment-based ranges, not measurements. On-device testing, not code generation, is the main driver; stages that can only be verified on the Deck (4, 5, 7, 8) carry the widest ranges.

| Stage | A: hours | B: hours |
|---|---|---|
| 0 Foundations | 3 to 5 | 3 to 4 |
| 1 Drive connection | 6 to 10 | 3 to 5 |
| 2 Library and sync | 6 to 9 | 5 to 7 |
| 3 Download and delete | 5 to 8 | 3 to 5 |
| 4 Emulator setup | 8 to 12 | 8 to 12 |
| 5 Launch pipeline | 5 to 8 | 5 to 8 |
| 6 Saves | 8 to 12 | 7 to 10 |
| 7 Controls | 10 to 16 | 10 to 16 |
| 8 Steam shortcuts | 6 to 10 | 4 to 7 |
| 9 Interface | 12 to 18 | 8 to 12 |
| 10 Packaging | 5 to 8 | 4 to 6 |
| **Total** | **74 to 116** | **60 to 92** |

**First usable version.** Stages 0 to 5 and 8, with a minimal Library and ROM page: about 45 to 70 hours for A and 35 to 55 for B. That version syncs, downloads, sets up emulators, and launches from Steam shortcuts; saves, per-game controls and polish follow.

**Calendar time.** At 8 to 10 hours a week, A is roughly two to three and a half months and B roughly six to twelve weeks.

**Usage cost.** Not estimated here. It depends on the Claude plan or API pricing in use and on how much is delegated per session.

**Working pattern that keeps the estimates honest**

- One stage per branch; start each by having Claude Code write the stage's tests from section 5, then the implementation.
- Keep the fakes (Drive, Flatpak runner, emulator binary) in the repository from stage 0, so Claude Code can run the full suite without a Deck.
- Do the on-Deck checks at the end of each stage, not at the end of the project.

## 7. Risks and open decisions

| Item | Why it matters | Suggested handling |
|---|---|---|
| Restricted Drive permission | Blocks public distribution without Google verification | Personal use first; if shared later, each user supplies their own client ID or rclone remote |
| ROM uploads outside the app | The reason the full permission is needed | Alternative: upload only through Romshelf (for example a local upload page), which would allow the narrow permission but removes "upload from any device via Drive" |
| Gamepad in the web view (A) | Could stall stage 9 | Test in stage 0; fall back to reading the pad in Rust or to Electron |
| Undocumented Steam functions (B) | Can break on client updates | Keep a shortcuts-file writer as fallback |
| Per-game control writers | Each emulator stores controls differently | Start with RetroArch and Dolphin; confirm formats with golden-file tests before adding more |
| Artwork source | "Find artwork" needs a provider and its terms | Decide before stage 8; local files and generated fallback work without one |
| Save conflicts across devices | Risk of lost progress | Never overwrite without a backup; ask on conflict |

## Sources

- [OAuth 2.0 for TV and limited-input device applications (Google)](https://developers.google.com/identity/protocols/oauth2/limited-input-device)
- [Choose Google Drive API scopes (Google)](https://developers.google.com/workspace/drive/api/guides/api-specific-auth)
- [Manage App Audience (Google Cloud Help)](https://support.google.com/cloud/answer/15549945)
- [Add Non Steam Game (TinkerGame wiki)](https://github.com/360900/tinkergame/wiki/Add-Non-Steam-Game)
- [decky-romm-sync notes on Steam shortcut functions](https://tomevault.io/tome/danielcopper/decky-romm-sync)
