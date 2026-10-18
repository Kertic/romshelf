# Stage 2: library index and sync

**Goal:** Romshelf knows every ROM in the Drive folder, assigns each a stable ID, and keeps the index current cheaply.

**Depends on:** Stage 1.
**Read first:** UI guide section 5.1 (Library); `LINKS.md` → Google (changes).

## Implementation steps

1. System table in `model.rs`. Initial entries:

   | System | Folder | Extensions |
   |---|---|---|
   | SNES | `snes` | `.sfc` `.smc` `.zip` `.7z` |
   | Game Boy Advance | `gba` | `.gba` `.zip` `.7z` |
   | Nintendo 64 | `n64` | `.z64` `.n64` `.v64` `.zip` `.7z` |
   | GameCube | `gamecube` | `.iso` `.rvz` `.gcz` `.ciso` |
   | PlayStation 2 | `ps2` | `.iso` `.chd` |

   The folder decides the system. The extension validates the file; anything else is recorded as "unrecognized" and shown separately, never launched.
2. Full scan: list each `roms/<system>/` folder (paged), upsert rows keyed by Drive file ID.
3. ID assignment: on first sight, generate `rom-id` as `<system>-<8 random hex>`; check for collisions.
4. Display name: file stem with underscores and hyphens turned into spaces, bracketed tags removed, title-cased only if the stem was all lower case. User overrides are stored and win.
5. Incremental sync: store the Drive change token after a full scan; `changes_since` then applies adds, renames, moves between system folders, trashes and restores. If the token is rejected, fall back to a full scan.
6. A ROM that disappears from Drive is marked `gone`, not deleted. Its saves, controls and shortcut record are kept until the user removes them.
7. Write the index to `.romshelf/index.json` in Drive after any change (rom-id to file ID map, display-name overrides), so a second device adopts the same IDs.
8. `romshelf sync` command with a summary line; sync on app start and on demand.
9. Front end: Library grid, system tabs, counts and sync status, driven by real data. Tiles show the generated cover described in the UI guide.

## Automated tests

- Extension table: every listed extension maps to its system; upper-case extensions match; a `.txt` in `roms/gba/` is "unrecognized".
- Full scan of a fake Drive with 500 files across systems produces 500 rows in one pass and handles paging.
- A second sync with no changes performs no database writes and no Drive writes.
- Rename in Drive: same rom-id, new file name, display name updated unless overridden.
- Move from `roms/snes/` to `roms/gba/`: same rom-id, system updated.
- Trash in Drive: row marked `gone`; save backup rows untouched. Restore from trash: row active again with the same rom-id.
- Two devices: device B reading `index.json` adopts device A's rom-ids instead of generating new ones.
- Rejected change token triggers a full scan and ends in the same state.
- Display-name derivation table: at least ten cases including tags like `(USA)` and `[!]`.

## On-Deck checks

- [ ] Upload a ROM from a phone, press Sync, and it appears under the right system.
- [ ] Rename it in Drive, sync, and the tile updates without duplicating.
- [ ] Counts in the header match Drive.

## Done when

Tests pass and the Library shows the real Drive contents on the Deck.

## Pitfalls

- A file can have several parents or be a shortcut to another file. Index only real files whose parent is one of the system folders.
- Do not download anything in this stage; metadata only.
