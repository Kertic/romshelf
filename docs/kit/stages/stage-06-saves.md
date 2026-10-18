# Stage 6: saves and save states

**Goal:** save data is backed up to Drive automatically, can be restored to any kept version, and is never overwritten silently.

**Depends on:** Stage 5.
**Read first:** UI guide sections 5.2 and 7 (restore list, conflict dialog).

## Layout

```
Local   <data>/sessions/<emulator>/saves/<rom-id>/
        <data>/sessions/<emulator>/states/<rom-id>/
Drive   Emulation/saves/<system>/<rom-id>/<UTC timestamp>/…
        Emulation/states/<system>/<rom-id>/<slot files>
```

## Implementation steps

1. Point each emulator at the Romshelf-owned save and state directories.
   - RetroArch: set the save-file and save-state directory keys in the appended config. **Confirm** the key names and that per-game subdirectories behave as expected.
   - Dolphin: saves live inside the user directory Romshelf supplies. GameCube saves are on a memory card image or a per-game folder; **confirm** which mode gives one folder per game and use it.
2. `save_paths(rom)` returns the local files or folder for a ROM.
3. Backup: snapshot the save files into a new timestamped folder in Drive, with a small manifest (file names, sizes, hashes, device name). Skip if the hashes equal the latest backup.
4. Retention: keep the newest N versions per game (default 10); trash older ones.
5. Restore: choose a version; first back up the current local save as a new version; then replace local files.
6. Sync state per game: `last_synced_hash`. Before launch, compare local and newest Drive hashes against it:
   - only Drive changed → offer restore (default yes);
   - only local changed → nothing to do;
   - both changed → conflict dialog: keep this Deck's, use Drive's, or cancel. Whichever is replaced is backed up first.
7. Pipeline: fill the Stage 5 stubs (step 4 save check, step 7 backup on exit). Backup failure does not change the exit code; it is queued for retry and shown in the Library.
8. Save states: mirror the states directory to Drive on exit; list slots with timestamps; delete a slot.
9. Setup step three ("Link saves folder") now does real work.
10. Front end: "Back up save data", "Restore save from Drive" with the version list, "Save states", the last-backup line, and the conflict dialog.

## Automated tests

- Backup then restore yields byte-identical files.
- Unchanged save: no new version is created.
- Retention keeps exactly N newest.
- Restore creates a safety backup of the replaced local save.
- Conflict matrix: all four combinations of local-changed and Drive-changed produce the documented outcome.
- Backup after a non-zero emulator exit still runs.
- Drive unavailable at exit: backup is queued; the next successful sync uploads it; exit code unaffected.
- Two devices alternate play with a shared fake Drive: no version is lost and no prompt appears when only one side changed.
- Adapter save-path tests against fixture directories for RetroArch and Dolphin.

## On-Deck checks

- [ ] Play, save in game, quit: the ROM page shows a new backup time and the folder exists in Drive.
- [ ] Delete the local save, restore the latest version, launch: progress is intact.
- [ ] Restore an older version: the game shows the older progress, and the newer one is still in the list.
- [ ] Airplane mode: play and quit; the backup uploads after reconnecting.

## Done when

Tests pass and a save survives being deleted locally on the real device.

## Pitfalls

- Some emulators write saves a moment after the window closes. Wait for the process to exit, not for the window.
- Never upload while the emulator is running.
- A save from one core may not load in another. Record the emulator and core in the manifest and warn on mismatch.
