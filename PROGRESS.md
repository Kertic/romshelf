# Progress

## Stage 0: foundations (branch `stage-00-foundations`)

Status: **built; waiting for on-Deck checks.**

### Built

- Cargo workspace: `crates/romshelf-core` (all logic, no Tauri), `crates/romshelf-testkit` (fakes + fixture library), `src-tauri` (the `romshelf` binary).
- `model.rs`: `System`, `RomId` (`<system>-<8 hex>`, FNV-1a of the Drive file ID), `Rom`, `LocationState`, `Emulator`, `Binding`, `ControlProfile`, `ArtSlot`, `Shortcut`. All serde.
- `db/`: SQLite (bundled) with versioned migrations recorded in `PRAGMA user_version`. Tables: `roms`, `systems`, `emulators`, `save_backups`, `control_profiles`, `shortcuts`, `settings`, `transfer_queue`. Refuses to open a schema newer than the binary.
- `paths.rs`: `Paths::from_env()` (XDG + `ROMSHELF_ROM_ROOT`) and `Paths::under(root)` for tests. `rom_file` rejects names that would escape the system folder.
- Seams with real and fake implementations: `DriveClient` (real one is stage 1), `PackageRunner`, `ProcessRunner`, `SteamFiles`, `TokenStore`, `Clock`.
- `EmulatorAdapter` trait (`id`, `systems`, `flatpak_id`, `detect`, `install`, `launch_command`, `save_paths`, `write_controls`, `declared_actions`) with `Registry`, `RetroArch` and `Dolphin`.
- CLI (`clap`): no args opens the interface; `launch --id|--rom`, `sync`, `doctor [--id]`, `auth set-client|login|logout|status`, `steam apply|list`. Each subcommand prints "not implemented yet" and exits 2.
- `ROMSHELF_FAKE=1`: temp root seeded with the eight fixture ROMs (Ember Circuit, Tidewalker, Starfall Relay, Moss & Lantern, Kite Rally, Pocket Kingdoms, Hollow Vane, Nine Orchards), RetroArch installed, Dolphin missing.
- Front end: tokens on `:root` from the UI guide; Sora and JetBrains Mono bundled via `@fontsource` (no network); shell with top band, plain tile grid, footer hint bar, D-pad/stick focus movement, keyboard fallback.
- Spike A page ("Open gamepad test"): live state from the browser Gamepad API and from `gilrs` side by side. Navigation locks to whichever path reports a press first. Hold B for one second to leave.
- CI: `.github/workflows/ci.yml` on Ubuntu 22.04 runs UI lint/test/build, `cargo fmt --check`, `clippy -D warnings`, `cargo test --workspace`, and uploads `romshelf.AppImage`.

### Tests

33 Rust tests, 24 front-end tests. Covers: JSON and database round-trips for every model type; migrations on an empty file, re-applying is a no-op, version recorded, newer schema rejected; no path escapes a temp root; every CLI subcommand and flag parses, `launch` with neither or both targets is an error; fake Drive create/list/rename/move/trash/change feed; fake process runner scripted codes; fake mode returns the eight fixture ROMs; `shortcuts.vdf` write backs up and touches nothing else; shell renders; tokens present on `:root`; fonts not loaded from the network; input edge detection and auto-repeat; spatial focus choice.

### Choices made (not covered by the kit)

- Release builds start full screen; debug builds windowed. `ROMSHELF_WINDOWED=1` / `ROMSHELF_FULLSCREEN=1` override.
- On Linux the app sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` unless already set, a common fix for blank WebKitGTK windows under gamescope. **Confirm** on the Deck whether it is needed.
- The shell shows the library as plain tiles (no drawer, no tabs yet) so fonts and colours can be compared with the screenshots; A on a tile does nothing until stage 9.
- RetroArch cores (`snes9x`, `mgba`, `mupen64plus_next`) and Dolphin's `-b -e` flags are placeholders. **Confirm** in stages 4–5.
- AppImage built on Ubuntu 22.04 for a low glibc floor.

### Deferred

- Real `DriveClient` and keyring `TokenStore` (stage 1).
- Spike B and the input-path decision need the Deck (below).

### On-Deck checklist

Install (Desktop Mode, Konsole), after downloading `romshelf-appimage.zip` from the CI run into `~/Downloads`:

```
mkdir -p ~/Applications && cd ~/Downloads && unzip -o romshelf-appimage.zip && mv romshelf.AppImage ~/Applications/ && chmod +x ~/Applications/romshelf.AppImage
```

Add to Steam: Steam → Games → Add a Non-Steam Game → Browse → `~/Applications/romshelf.AppImage`. To see the fixture library, set the shortcut's launch options to `ROMSHELF_FAKE=1 %command%`.

- [ ] The AppImage starts from Desktop Mode (double-click, or `~/Applications/romshelf.AppImage` in Konsole).
- [ ] Added to Steam, it starts in Game Mode, fills the screen, and returns to Steam on exit ("Quit Romshelf").
- [ ] Spike A: on "Open gamepad test", D-pad, face buttons, bumpers and sticks all register. Note which path (browser API, `gilrs`, or both) shows them and which one the header says navigation uses.
- [ ] Fonts render as in the screenshots (`docs/kit/ui/screens/01-library.png`).

Results:

- Spike A (input path):
- Spike B (Game Mode):
- `WEBKIT_DISABLE_DMABUF_RENDERER` needed?:
