# Romshelf: initial prompt for Claude Code

You are building **Romshelf**, a Steam Deck application that manages emulation from one place. Read this file fully before doing anything. It is the standing brief for the whole project; the files it points to hold the detail.

## 1. What you are building

- ROMs live in a Google Drive folder that Romshelf creates and structures. The user uploads their own ROM files there from any device.
- On the Deck, Romshelf lists what is in Drive, downloads and deletes ROMs, installs and configures emulators, backs up saves, manages controls (a default per emulator plus per-game differences), and publishes each game to the Steam library as a shortcut with its own name and artwork.
- **Every Steam shortcut runs `romshelf launch --id <rom-id>`.** Romshelf decides which emulator to start, with which controls and which save. No shortcut ever points at an emulator directly. This is the central rule of the project: all behaviour changes happen inside Romshelf, never by re-creating shortcuts.
- Romshelf ships no ROMs and no BIOS files. Never add any to the repository, tests or fixtures. Use tiny generated dummy files.

## 2. Files in this kit

Assume the kit is at `docs/kit/` in the repository.

| File | Use |
|---|---|
| `00-INITIAL-PROMPT.md` | This brief |
| `stages/stage-00-…` to `stage-10-…` | One file per stage: goal, steps, automated tests, on-Deck checks, done criteria |
| `LINKS.md` | Reference documentation, grouped by stage |
| `SPEC.md` | The product description, platform facts and the reasoning behind the chosen approach. Background reading; where it differs from this brief or a stage file, they win |
| `ui/UI-UX-GUIDE.md` and `.pdf` | What every screen looks like and how it behaves. Same content in both; the PDF has the screenshots inline |
| `ui/screens/*.png` | Screenshots of the five approved screens at 1280×800 |
| `ui/reference-html/*.html` | Static HTML and CSS for those screens. Treat as the visual source of truth: copy tokens, spacing and structure from it |

## 3. Fixed decisions

Do not revisit these without asking.

1. **Shape:** one binary named `romshelf`. With no arguments it opens the full-screen interface. With a subcommand (`launch`, `sync`, `doctor`, `auth`, `steam`) it runs headless, opening a small progress or error window only when a launch needs one.
2. **Stack:** Tauri 2. Rust for all logic. Front end in TypeScript with React and Vite, plain CSS using custom properties for the design tokens. No UI component library.
3. **Core is a library.** All logic lives in the `romshelf-core` crate with no dependency on Tauri. The Tauri app and the command line are thin layers over it.
4. **Drive access:** Google Drive API v3 directly, with the user's own Google Cloud desktop client ID. Permission requested is the full `drive` scope, because the narrow `drive.file` scope cannot see files the user uploaded through the Drive website. Sign-in is the desktop loopback flow with PKCE in the system browser. Embedded web views and the device-code flow do not work for this.
5. **Identity:** a ROM is identified by its Drive file ID. Romshelf assigns each a short stable `rom-id` of the form `<system>-<8 hex>`. Renaming or moving the file keeps the ID.
6. **Emulators:** installed as per-user Flatpaks. First adapters are RetroArch (SNES, GBA, N64) and Dolphin (GameCube). PCSX2 follows the same pattern later.
7. **Steam shortcuts:** written to Steam's `shortcuts.vdf` and artwork to the `grid` folder. Steam must restart to show changes, so batch them and show one prompt.
8. **Packaging:** AppImage, installed at a fixed path so shortcut targets never change.
9. **Local data:** SQLite index and logs under `~/.local/share/romshelf/`; settings under `~/.config/romshelf/`; ROMs under a user-chosen root (internal storage or SD card).

## 4. Repository layout

```
romshelf/
  Cargo.toml                 workspace
  crates/
    romshelf-core/           library: all logic
      src/
        model.rs             System, Rom, Emulator, ControlProfile, Shortcut
        db/                  SQLite schema and queries
        drive/               DriveClient trait, Google implementation, fake
        sync/                index building and change handling
        transfer/            download queue, delete
        emulators/           EmulatorAdapter trait, registry, retroarch.rs, dolphin.rs
        launch/              the launch pipeline
        saves/               backup, restore, conflict rules
        controls/            canonical model, merge, per-adapter writers
        steam/               shortcuts file, artwork, app IDs
        paths.rs             every filesystem location, injectable
    romshelf-testkit/        fakes shared by all tests
  src-tauri/                 the `romshelf` binary: argument parsing, Tauri commands
  ui/                        React front end
  fixtures/                  sample shortcuts.vdf, emulator configs, dummy ROMs
  docs/kit/                  this kit
  CLAUDE.md
  PROGRESS.md
```

## 5. Seams you must keep

Everything that touches the outside world goes through a trait with a real and a fake implementation. Tests use only fakes.

| Trait | Real implementation | Fake |
|---|---|---|
| `DriveClient` | Google Drive REST | In-memory folder tree with change feed |
| `PackageRunner` | `flatpak` command | Records calls, scripted results |
| `ProcessRunner` | Spawns the emulator | Scripted exit codes and delays |
| `SteamFiles` | Real Steam user directory | Temp directory seeded from `fixtures/` |
| `TokenStore` | Keyring, with a 0600 file fallback | In-memory |
| `Clock` | System time | Settable |

Setting `ROMSHELF_FAKE=1` makes the app start with all fakes and a fixture library, so the interface can be developed and tested without a Deck, a Google account or Steam.

## 6. How to work

1. Work one stage at a time, in order. Each stage is a branch named `stage-NN-name`.
2. At the start of a stage, read its file and the UI guide sections it names. Read any linked documentation you need from `LINKS.md` before writing code that depends on an external format.
3. Write the stage's automated tests first, from the "Automated tests" list. Then implement until they pass. Add further tests wherever you find an edge the list missed.
4. Run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test --workspace`, and the front-end lint and tests before calling a stage complete.
5. Update `PROGRESS.md`: what was built, what was deferred and why, and the on-Deck checklist for the human copied from the stage file with empty checkboxes.
6. **Stop at the end of each stage.** The on-Deck checks need a person with the device. Do not start the next stage until they report results. If a check fails, fix it inside the same stage.
7. Where a stage file says **Confirm**, the fact comes from community documentation or memory and has not been verified. Verify it (read the source, write a spike, or ask for an on-Deck check) before building on it, and record what you found in `PROGRESS.md`.

## 7. Rules

- Never read, write or delete anything in a real Google Drive, a real Steam directory or a real emulator configuration from a test. Tests run entirely against fakes and temp directories.
- Never commit a client ID, client secret, token or API key. They are supplied at runtime with `romshelf auth set-client` and stored outside the repository.
- Destructive actions (delete from Drive, overwrite a save, modify `shortcuts.vdf`) always take a backup or go to trash first, and always have a test proving nothing else was touched.
- Every user-facing string in the interface follows the UI guide: plain sentence-case labels, the action named on the button, errors that say what happened and what to do.
- Everything must be operable with the gamepad alone.
- If documentation and observed behaviour disagree, stop and report it. Do not guess.

## 8. Your first task

Start Stage 0 (`stages/stage-00-foundations.md`). As part of it, create `CLAUDE.md` at the repository root containing sections 1, 3, 5, 6 and 7 of this file in condensed form, plus the build and test commands once they exist. Then proceed through the stage and stop for the on-Deck checks.
