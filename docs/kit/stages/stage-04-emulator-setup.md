# Stage 4: emulator registry and setup

**Goal:** Romshelf knows which emulator runs each system, detects whether it is installed, and can install and prepare it from the ROM page.

**Depends on:** Stage 3.
**Read first:** UI guide section 5.3 (ROM page, emulator missing) and section 6 (Emulators screen); `LINKS.md` → Emulators, Flatpak.

## Implementation steps

1. Registry: system → adapter (and core where relevant).

   | System | Adapter | Flatpak ID | Core |
   |---|---|---|---|
   | SNES | RetroArch | `org.libretro.RetroArch` | `snes9x` |
   | GBA | RetroArch | `org.libretro.RetroArch` | `mgba` |
   | N64 | RetroArch | `org.libretro.RetroArch` | `mupen64plus_next` |
   | GameCube | Dolphin | `org.DolphinEmu.dolphin-emu` | none |

2. `PackageRunner` real implementation: `flatpak info --user <id>` to detect, `flatpak install --user -y flathub <id>` to install, with progress parsed from output. Ensure the Flathub remote exists for the user.
3. RetroArch adapter: detect, install, and install the cores the registry needs. **Confirm** where the Flatpak build keeps cores and how to fetch one without the in-app updater.
4. Dolphin adapter: detect and install.
5. Romshelf-owned session directories: each adapter runs with configuration and save locations that Romshelf controls, so nothing depends on the emulator's own menus. For RetroArch, generate an extra config file passed with `--appendconfig`. For Dolphin, **confirm** the option that sets the user directory.
6. BIOS: each adapter declares required and optional files per system. Setup copies them from Drive `bios/<system>/` and reports what is missing. Neither first adapter needs one for the first systems, but the mechanism must exist.
7. Setup flow as a small state machine: `install` → `default controls` → `link saves`, each step resumable. Steps two and three are stubs that record completion until Stages 6 and 7 fill them in.
8. Declared actions: adapters return the list of emulator-specific actions (save backup, restore, save states, cheats, controls). The ROM page renders from this list.
9. Front end: ROM page in both states (5.2 and 5.3), the setup progress view, and the Emulators screen listing each emulator with status and an install or repair button.

## Automated tests

- Registry: every extension from Stage 2 resolves to exactly one adapter; an unsupported system returns a clear "no emulator yet" result.
- Detect: scripted "installed" and "not installed" outputs are read correctly.
- Install: the exact command is issued once; a failure surfaces the runner's message; a second attempt after failure works.
- Core install for RetroArch: missing core is fetched; present core is left alone.
- Setup state machine: interrupted after step one resumes at step two.
- BIOS: required file present in fake Drive is copied; missing file is listed by name.
- Declared actions: RetroArch on GBA and Dolphin on GameCube return different lists, and the front end renders exactly those.

## On-Deck checks

- [ ] With Dolphin not installed, open a GameCube ROM page: it shows the setup card (screen 2b).
- [ ] Press "Set up Dolphin": it installs with visible progress and the page changes to the ready state (screen 2).
- [ ] Do the same for RetroArch and confirm the needed core is present.
- [ ] The Emulators screen shows both as installed.

## Done when

Tests pass and both emulators can be installed from inside Romshelf in Game Mode.

## Pitfalls

- Flatpak installs can take minutes and need the network. Show progress and allow leaving the screen.
- Do not modify the user's existing emulator configuration files. Everything Romshelf needs goes in its own session directory or appended config.
