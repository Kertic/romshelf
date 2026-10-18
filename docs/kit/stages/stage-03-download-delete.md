# Stage 3: download, delete and storage

**Goal:** ROMs move between Drive and the Deck reliably, and deletion removes exactly what the user chose.

**Depends on:** Stage 2.
**Read first:** UI guide sections 5.1, 5.2 and 7 (delete dialog, progress states); `LINKS.md` → Google (downloads).

## Implementation steps

1. Storage root setting: a directory on internal storage or an SD card. ROMs go to `<root>/roms/<system>/<file name>`.
2. Download queue persisted in `transfer_queue`: states `queued`, `running`, `paused`, `failed`, `done`. One or two concurrent transfers.
3. Downloads are ranged and resumable: write to `<name>.part`, record bytes received, resume with a `Range` request, verify the MD5 Drive reports, then rename into place.
4. Free-space check before starting, with headroom. Refuse with a message naming the shortfall.
5. Progress events to the front end (bytes, total, rate) throttled to a few per second.
6. Location state per ROM: `drive_only`, `downloading`, `on_deck`, `gone`. A missing local file is detected on start and corrected to `drive_only`.
7. Delete with three scopes: "From this Deck", "From Drive", "From both". Drive deletion uses trash, never permanent delete. Deleting from Drive also asks whether to keep save backups (default: keep).
8. Commands: `romshelf download <id>`, `romshelf delete <id> --scope deck|drive|both`.
9. Front end: Download, Download again, and Delete actions; the delete dialog; tile and ROM-page progress states.

## Automated tests

- Happy path: file arrives, checksum matches, state is `on_deck`, no `.part` remains.
- Interrupted at 40%: resuming requests the correct range and the result is byte-identical.
- Checksum mismatch: file is discarded, state returns to `drive_only`, error surfaced.
- Insufficient space: nothing is written.
- App restart with a `running` item: it resumes.
- Delete scope `deck`: local file gone, Drive untouched, state `drive_only`.
- Delete scope `drive`: file trashed in Drive, local copy untouched, row marked `gone` after sync.
- Delete scope `both`: both of the above.
- In every delete test, assert that no other file in the fake Drive or temp directory changed.
- Storage root changed: existing files are found at the new root after a move; nothing is re-downloaded.

## On-Deck checks

- [ ] Download a large ROM; progress moves and the time estimate is sensible.
- [ ] Turn Wi-Fi off mid-download and back on; it completes.
- [ ] Delete from Drive, then find the file in Drive's trash.
- [ ] Set the storage root to the SD card and download there.

## Done when

Tests pass, on-Deck checks pass, and the Library tile states match reality after each action.

## Pitfalls

- SD cards can be removed. Treat a missing storage root as "storage unavailable", not as every ROM having been deleted.
- Never trust the local file's presence alone; compare size, and checksum on demand in `doctor`.
