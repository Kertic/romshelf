# Stage 5: launch pipeline

**Goal:** `romshelf launch --id <rom-id>` starts the right game in the right emulator from anywhere: the Play button, a terminal, or a Steam shortcut. This is the contract the whole project rests on.

**Depends on:** Stage 4.
**Read first:** UI guide section 8 (launch window); `LINKS.md` → Emulators.

## The pipeline

1. Resolve the ROM from the local index (by rom-id, or by `--rom <system>/<file>`).
2. If it is not on the Deck, download it, showing progress.
3. If the emulator is missing or setup is incomplete, run setup, then continue.
4. Save check (stub until Stage 6): if Drive has a newer save, offer to restore.
5. Controls (stub until Stage 7): write the merged layout into the session configuration.
6. Start the emulator and wait for it to exit.
7. Save backup (stub until Stage 6), then exit with the emulator's exit code.

## Implementation steps

1. `launch/pipeline.rs`: the steps above as an explicit sequence where each step returns `Continue`, `NeedsUser(prompt)` or `Fail(error)`. The same code runs from the interface and from the command line.
2. `launch_command` for each adapter:
   - RetroArch: `flatpak run org.libretro.RetroArch -L <core> --appendconfig <session.cfg> <rom>`, full screen.
   - Dolphin: `flatpak run org.DolphinEmu.dolphin-emu` in batch mode with the ROM path. **Confirm** the exact flags.
   - Grant the Flatpak access to the ROM directory (`--filesystem=`) when the storage root is outside its sandbox.
3. `ProcessRunner` real implementation: spawn, wait, capture exit code and the last lines of output for error reports. Forward termination signals to the child.
4. Launch window: when run headless and a step needs the user or takes time, open a small full-screen Tauri window showing progress, a prompt or an error. If every precondition is already met, no window appears at all.
5. Single-instance behaviour: `launch` works whether or not the main interface is already running.
6. `romshelf doctor --id <id>`: print each pipeline step and what it would do, without doing it.
7. Wire the Play button on the Library drawer and ROM page.
8. Log every launch (rom-id, emulator, duration, exit code) to the local log.

## Automated tests

- With a fake emulator binary: the command line is exactly as specified for each adapter.
- Exit code 0 and exit code 3 from the emulator are both returned unchanged by `romshelf launch`.
- ROM `drive_only`: download step runs first, then launch.
- Emulator missing: setup runs first, then launch.
- Both missing: download, then setup, then launch, in that order.
- `--id` and `--rom` forms resolve to the same ROM.
- Unknown id: exit code 2 and a message naming the id.
- ROM marked `gone`: clear error, no emulator started.
- Step order is asserted with a recording fake: save check and control write happen before spawn; backup happens after exit even when the exit code is non-zero.
- `doctor --id` performs no writes and no spawns.

## On-Deck checks

- [ ] Play from the Library starts the game full screen; quitting returns to Romshelf.
- [ ] From Desktop Mode terminal, `romshelf launch --id <id>` starts the same game.
- [ ] Add a manual non-Steam shortcut with target `romshelf` and launch options `launch --id <id>`; it starts the game from Game Mode and returns to Steam on quit.
- [ ] Launch a ROM that is not downloaded from that shortcut: the progress window appears, then the game.
- [ ] The Steam overlay and the Deck's quick-access menu work while the game runs.

## Done when

Tests pass and a hand-made Steam shortcut launches a game end to end.

## Pitfalls

- Steam tracks the shortcut by its process tree. Romshelf must stay alive until the emulator exits, or Steam will think the game ended.
- Do not detach or daemonize the emulator.
- Flatpak sandbox permissions are the most likely cause of "ROM not found" inside the emulator.
