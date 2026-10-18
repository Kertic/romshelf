# Stage 0: foundations

**Goal:** a repository that builds, tests and runs an empty Romshelf on the Deck, with the data model, command skeleton, seams and fakes that every later stage depends on. Two early spikes remove the biggest unknowns of the standalone approach.

**Depends on:** nothing.
**Read first:** `00-INITIAL-PROMPT.md`; UI guide sections 1 to 3; `LINKS.md` → Tauri.

## Implementation steps

1. Create the Cargo workspace and the layout from the initial prompt. Scaffold the Tauri 2 app in `src-tauri/` with the React and Vite front end in `ui/`.
2. Write `CLAUDE.md` and `PROGRESS.md`.
3. Add CI (GitHub Actions): format, clippy, `cargo test --workspace`, front-end lint and tests, and an AppImage build artifact.
4. `model.rs`: `System`, `Rom` (rom-id, Drive file ID, system, file name, size, checksum, location state), `Emulator`, `ControlProfile`, `Shortcut`. All serializable.
5. `db/`: SQLite schema with migrations. Tables: `roms`, `systems`, `emulators`, `save_backups`, `control_profiles`, `shortcuts`, `settings`, `transfer_queue`.
6. `paths.rs`: one `Paths` struct holding every filesystem location, constructed from environment or from a temp directory in tests. Nothing else in the code base builds a path from `~`.
7. Define the traits from the initial prompt (`DriveClient`, `PackageRunner`, `ProcessRunner`, `SteamFiles`, `TokenStore`, `Clock`) and put in-memory fakes in `romshelf-testkit`.
8. Define `EmulatorAdapter`: `id`, `systems`, `flatpak_id`, `detect`, `install`, `launch_command(rom, session)`, `save_paths(rom)`, `write_controls(layout, rom)`, `declared_actions`.
9. Argument parsing in `src-tauri` with `clap`: no arguments opens the interface; `launch --id <id>` / `launch --rom <system>/<file>`, `sync`, `doctor [--id <id>]`, `auth set-client|login|logout|status`, `steam apply|list`. Each subcommand prints "not implemented" and exits non-zero for now.
10. `ROMSHELF_FAKE=1` mode with a fixture library of eight ROMs matching the screenshots (Ember Circuit, Tidewalker, Starfall Relay, Moss & Lantern, Kite Rally, Pocket Kingdoms, Hollow Vane, Nine Orchards).
11. Front end: design tokens as CSS custom properties copied from the UI guide; load the Sora and JetBrains Mono fonts from bundled files, not from the network; an empty full-screen shell with the footer hint bar.
12. **Spike A, gamepad in the web view.** A test page that shows live state from the browser Gamepad API and, separately, from the `gilrs` crate forwarded over a Tauri event. Decide which input path the app will use.
13. **Spike B, Game Mode.** Add the AppImage to Steam as a non-Steam game and confirm it starts full screen at 1280×800 under gamescope and exits cleanly.

## Automated tests

- Model: every type round-trips through JSON and through the database.
- Database: migrations apply to an empty file; applying twice is a no-op; schema version is recorded.
- Paths: with a temp root, no path escapes the root.
- Arguments: each subcommand and flag parses; `launch` with neither `--id` nor `--rom` is an error; `--id` and `--rom` together is an error.
- Fakes: fake Drive supports create, list, rename, move, trash and a change feed; fake process runner returns scripted exit codes.
- Fake mode: the app's data layer returns the eight fixture ROMs.
- Front end: the shell renders; tokens are present on `:root`.

## On-Deck checks

- [ ] The AppImage starts from Desktop Mode.
- [ ] Added to Steam, it starts in Game Mode, fills the screen, and returns to Steam on exit.
- [ ] Spike A: D-pad, face buttons, bumpers and sticks all register. Note which path (browser API or `gilrs`) worked.
- [ ] Fonts render as in the screenshots.

## Done when

All automated tests pass in CI, the AppImage artifact runs on the Deck, and `PROGRESS.md` records the outcome of both spikes and the chosen gamepad input path.

## Pitfalls

- If the browser Gamepad API does not report the Deck's pad inside the web view, use `gilrs` in Rust and forward events. If neither works under gamescope, stop and report; the fallback is to move the shell to Electron, which is a decision for the human.
- Steam Input is active for non-Steam games. The app sees whatever the active layout emits (by default a standard gamepad), not the Deck's raw inputs.
