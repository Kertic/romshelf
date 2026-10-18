# Stage 8: Steam shortcuts and artwork

**Goal:** every chosen ROM appears in the Steam library with its own name and artwork, and launching it runs `romshelf launch --id <rom-id>`.

**Depends on:** Stage 5.
**Read first:** UI guide sections 5.5 (Steam shortcut) and 7 (restart prompt); `LINKS.md` → Steam.

## Facts

- Shortcuts are stored in a binary file at `<Steam>/userdata/<user id>/config/shortcuts.vdf`. Entry fields include `appid`, `AppName`, `Exe`, `StartDir`, `icon`, `LaunchOptions` and `tags`.
- Steam reads the file at start-up. New entries appear after Steam restarts.
- Artwork is image files in `<Steam>/userdata/<user id>/config/grid/`, named after the shortcut's app ID.
- The `steam_shortcuts_util` crate parses and writes the file and calculates app IDs.

## Implementation steps

1. `SteamFiles` real implementation: locate the Steam root and the active user directory (handle more than one user: pick the most recently used and let settings override).
2. Read and write `shortcuts.vdf` through `steam_shortcuts_util`. Before every write: copy the current file to a timestamped backup (keep five). Write to a temp file and rename.
3. Ownership: Romshelf's entries carry the tag `Romshelf` and are recorded in the `shortcuts` table with their app ID. Never modify or remove an entry Romshelf did not create.
4. Entry contents:
   - `AppName`: the display name (editable).
   - `Exe`: the absolute path of the installed Romshelf AppImage, quoted.
   - `StartDir`: its directory.
   - `LaunchOptions`: `launch --id <rom-id>`.
   - `icon`: path to the icon image.
   - App ID: deterministic for the ROM, from the crate's calculation; stored so a later rename of `AppName` does not orphan artwork (**confirm** whether renaming changes the calculated ID; if so, keep the stored ID and write it explicitly).
5. Artwork slots: capsule (portrait), wide capsule, hero, logo, icon. **Confirm** the file name suffix for each slot by setting artwork through Steam's own UI on the Deck and reading what it wrote. Record the mapping in `PROGRESS.md` and in a table in the code.
6. Artwork sources, in order: a file the user chose; a result from SteamGridDB if the user has entered an API key in settings; a generated fallback in the same style as the Library tile (system colour, initials, system label). The fallback means every shortcut always has artwork.
7. Reconcile function: desired state (ROMs with "show in Steam" on) versus the file's Romshelf entries → adds, updates, removals. Idempotent.
8. Settings: per-ROM "Show in Steam library"; global "Add every new ROM to Steam automatically".
9. Pending-changes state and the "Restart Steam to apply" prompt. Changes are batched; the prompt appears once.
10. Commands: `romshelf steam list`, `romshelf steam apply`.
11. Removing a ROM from Drive, or turning the setting off, removes its entry and artwork files.
12. Front end: the Steam shortcut screen (name, slots, preview, launch definition, save and remove), the card on the ROM page, and the "In Steam" marker on Library tiles.

## Automated tests

- Parse a real fixture `shortcuts.vdf` and write it back: bytes identical.
- Add one entry: all original entries unchanged; the new one has the expected fields.
- Rename, then remove, a Romshelf entry: other entries unchanged.
- An entry without the Romshelf tag is never touched, even if its name matches a ROM.
- App ID is identical across runs for the same ROM, and distinct for two ROMs with the same display name.
- Reconcile twice in a row: the second run makes no writes.
- Backup exists before each write; an injected failure during write leaves the original file intact.
- Missing file or missing `config` directory: created correctly.
- Artwork: each slot writes the mapped file name; replacing artwork removes the old file of a different extension; removal deletes all five.
- Fallback artwork is generated at the right dimensions for each slot.
- `LaunchOptions` survives display names containing quotes and non-ASCII characters.

## On-Deck checks

- [ ] Add a game, restart Steam: it appears under Non-Steam with the right name.
- [ ] All artwork slots show correctly: library grid, the game's page (hero and logo), recent games, and the icon.
- [ ] Launch it from Steam: the full pipeline runs, including save backup on quit.
- [ ] Rename the ROM file in Drive, sync, launch the shortcut again: it still works.
- [ ] Existing non-Steam shortcuts that Romshelf did not create are still present and unchanged.
- [ ] Make a change while Steam is running, then exit Steam normally and start it again: the change is still there. If Steam overwrote it, report this.

## Done when

Tests pass, artwork mapping is confirmed and recorded, and a shortcut created by Romshelf launches a game from Game Mode.

## Pitfalls

- Steam may rewrite `shortcuts.vdf` from memory when it exits, discarding edits made while it was running. The last on-Deck check exists to detect this. If it happens, stop and report; options include applying pending changes at the next Romshelf start before Steam's library loads, or applying them from Desktop Mode with Steam closed.
- The AppImage path is baked into every shortcut. Self-update (Stage 10) must replace the file in place.
- Launching by `steam://rungameid/` uses a different, longer ID than the one in file names. Not needed for this stage; do not mix them up.
