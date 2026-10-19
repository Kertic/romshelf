# Romshelf

Steam Deck app that manages emulation from one place. Full brief: `docs/kit/00-INITIAL-PROMPT.md`; stage files: `docs/kit/stages/`; UI: `docs/kit/ui/` (reference HTML is the visual source of truth).

## What it is

- ROMs live in a Google Drive folder Romshelf creates (`Emulation/roms/<system>/…`); the user uploads their own files from any device.
- On the Deck, Romshelf lists, downloads and deletes ROMs, installs emulators (per-user Flatpaks), backs up saves, manages controls (emulator default + per-game differences) and publishes each game to Steam as a shortcut with artwork.
- **Every Steam shortcut runs `romshelf launch --id <rom-id>`.** Never point a shortcut at an emulator. Behaviour changes happen inside Romshelf, never by re-creating shortcuts.
- No ROMs or BIOS files anywhere in the repo, tests or fixtures. Use tiny generated dummy files.

## Fixed decisions (ask before changing)

1. One binary, `romshelf`. No args → full-screen UI. Subcommands (`launch`, `sync`, `doctor`, `auth`, `steam`, `update`, `download`, `remove`, `storage`) run headless.
2. Tauri 2; all logic in Rust; front end React + Vite + TypeScript, plain CSS with custom-property tokens, no component library.
3. All logic in `crates/romshelf-core` (no Tauri dependency). `src-tauri` and the CLI are thin layers.
4. Drive API v3 directly with the user's own desktop client ID, full `drive` scope, loopback + PKCE sign-in in the system browser.
5. A ROM's identity is its Drive file ID; `rom-id` = `<system>-<8 hex>` (FNV-1a of the file ID), stable across rename/move.
6. Emulators as per-user Flatpaks. RetroArch (SNES, GBA, N64) and Dolphin (GameCube) first; PCSX2 later.
7. Shortcuts via `shortcuts.vdf` + `grid/` artwork; batch changes, one Steam-restart prompt.
8. AppImage at a fixed path (`~/Applications/romshelf.AppImage`) so shortcut targets never change.
9. SQLite index and logs in `~/.local/share/romshelf/`, settings in `~/.config/romshelf/`, ROMs under a user-chosen root (`<root>/roms/<system>/`; default `~/Romshelf`, away from EmuDeck's `~/Emulation`; `romshelf storage --set` moves it).

## Seams

Everything external goes through a trait with a real and a fake implementation. Tests use only fakes (`crates/romshelf-testkit`).

| Trait | Module | Real | Fake |
|---|---|---|---|
| `DriveClient` | `drive` | `GoogleDrive` (Drive v3 REST) | `FakeDrive` |
| `HttpTransport` | `http` | `ReqwestTransport` (under `GoogleDrive` and `GoogleAuth`) | `FakeHttp` |
| `PackageRunner` | `process` | `FlatpakRunner` | `FakePackageRunner` |
| `ProcessRunner` | `process` | `SystemProcessRunner` | `FakeProcessRunner` |
| `SteamFiles` | `steam` | `FsSteamFiles` on `~/.steam/steam` | `FakeSteam` (temp copy) |
| `TokenStore` | `tokens` | `KeyringTokenStore` with silent `FileTokenStore` 0600 fallback (`FallbackTokenStore`) | `MemoryTokenStore` |
| `DiskSpace` | `storage` | `FsDiskSpace` | `FixedSpace` |
| `Clock` | `clock` | `SystemClock` | `FakeClock` |

`Paths` (`paths.rs`) holds every filesystem location; nothing else builds paths from `~`.
`ROMSHELF_FAKE=1` starts the app on a temp root seeded with the eight-ROM fixture library.
Other env flags: `ROMSHELF_WINDOWED=1` (no full screen), `ROMSHELF_FULLSCREEN=1` (full screen in debug builds), `ROMSHELF_FAKE_FIRST_RUN=1` (with `ROMSHELF_FAKE=1`: start signed out to walk the first run).

## How to work

1. One stage at a time, in order, each on a branch `stage-NN-name`.
2. Read the stage file and the UI guide sections it names; read `docs/kit/LINKS.md` before depending on an external format.
3. Write the stage's automated tests first, then implement. Add tests for edges the list missed.
4. Before calling a stage done, run every command under "Build and test".
5. Update `PROGRESS.md`: built, deferred (and why), and the on-Deck checklist with empty boxes.
6. **Stop at the end of each stage** for the human's on-Deck checks. Fix failures within the same stage.
7. Anything marked **Confirm** is unverified: verify it and record the finding in `PROGRESS.md`.

## Rules

- **Romshelf never deletes, moves, renames or rewrites the user's ROM files in Drive.** The user manages Drive; Romshelf reads ROMs from it and only writes folders it created and its own files (`.romshelf/`, and later `saves/` and `states/`). "Delete" in the interface means removing the copy on the Deck. (The owner's decision, 2026-10-19, replacing stage 3's "From Drive" and "From both" scopes.)
- Tests never touch a real Google Drive, Steam directory or emulator config. Fakes and temp dirs only.
- Never commit a client ID, secret, token or API key (`romshelf auth set-client` stores them outside the repo).
- Destructive actions (Drive delete, save overwrite, `shortcuts.vdf` write) take a backup or go to trash first, with a test proving nothing else was touched.
- UI strings follow the guide: sentence case, the action named on the button, errors say what happened and what to do.
- Everything operable with the gamepad alone.
- If documentation and observed behaviour disagree, stop and report. Don't guess.

## Build and test

Run from the repository root. On this Windows machine, set `CARGO_TARGET_DIR` outside OneDrive (e.g. `%LOCALAPPDATA%\romshelf-target`) to keep build output out of sync.

```
npm ci && npm ci --prefix ui         # once
npm --prefix ui run lint
npm --prefix ui test
npm --prefix ui run build            # cargo needs ui/dist to exist
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npx tauri dev                        # run the app (add ROMSHELF_FAKE=1 for fixtures)
npx tauri build --bundles appimage   # Linux only; CI does this
```

CI (`.github/workflows/ci.yml`) runs all of the above on Ubuntu 22.04 and uploads `romshelf-appimage`.
