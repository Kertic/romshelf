# Progress

## Stage 4: emulator registry and setup (branch `stage-04-emulator-setup`, on top of stage 3)

Status: **built; waiting for on-Deck checks and the two Confirm items.**

### Built

- **Registry:** SNES `snes9x`, GBA `mgba`, N64 `mupen64plus_next` in RetroArch; GameCube in Dolphin. `Registry::require` gives "Romshelf can't run PS2 games yet." for a system with no emulator.
- **Detect:** `flatpak info <id>` counts an install for this user *or* system-wide, so an emulator EmuDeck installed is used rather than installed twice.
- **Install:** `flatpak remote-add --user --if-not-exists flathub …`, then `flatpak install --user --noninteractive -y flathub <id>`, streaming Flatpak's output and reading the percentages for progress. A failure gives Flatpak's own message ("Couldn't install Dolphin: … Try again.") and a later attempt starts cleanly.
- **RetroArch cores:** each missing core is downloaded from `buildbot.libretro.com/nightly/linux/x86_64/latest/<core>_libretro.so.zip`, unzipped, checked to be an ELF file, and put in the Flatpak's cores folder. A core already there (yours or EmuDeck's) is left alone.
- **Session folders, never your settings:**
  - RetroArch: `~/.var/app/org.libretro.RetroArch/config/romshelf/` with `saves/`, `states/`, `system/`, and `romshelf.cfg` pointing RetroArch at them. That file is meant for `--appendconfig` in stage 5. `retroarch.cfg` is never edited.
  - Dolphin: `~/.var/app/org.DolphinEmu.dolphin-emu/data/romshelf-user/`, meant for Dolphin's `-u` user-folder option. Your Dolphin settings and memory cards are not touched.
- **BIOS:** adapters can declare required and optional files per system. Setup copies them from Drive's `bios/<system>/` (reading only) and lists what's missing: "Missing BIOS: rom1.bin. Add it to Drive's Emulation/bios/ps2 folder." RetroArch and Dolphin need none for these systems.
- **Setup steps:** Install, Default controls, Link saves. Each is recorded in the database when done, so an interrupted setup resumes at the next step. The last two only record completion until stages 7 and 6 fill them in. If the Flatpak disappears outside Romshelf, the steps start over. **Repair** runs them all again.
- **Status:** Not installed / Setup incomplete (installed, perhaps by EmuDeck, but Romshelf's setup or a core is missing) / Installed. Checking also keeps the database's "installed" flag (the Library's "Needs Dolphin") in step, and runs once at start-up.
- **Declared actions:** the game's actions dialog lists the emulator's own functions ("Memory cards", "Save states", "Texture packs" for Dolphin; "Save states", "Cheats", "Shaders" for RetroArch), each marked "Comes in a later update".
- **Command line:** `romshelf emulators` lists them; `romshelf emulators --setup dolphin` sets one up and prints the steps and progress.
- **Interface:**
  - An **Emulators** button in the top band opens the Emulators screen: each emulator with its systems, status and **Set up** / **Repair** (or **Try again**), with a progress bar while it installs. Setup keeps running in the background if you go back.
  - A game's actions dialog says "Runs in RetroArch with the mGBA core", "Needs Dolphin" or "Romshelf can't run PS2 games yet". When the emulator isn't ready, **Set up {emulator}** comes first with its progress (guide screen 2b).
- **Fake mode:** a simulated Flatpak (RetroArch installed and set up, Dolphin not) that installs with visible progress.

### Tests

17 emulator tests: every recognized extension resolves to one emulator or "can't run … yet"; detecting installed and not installed (any installation); the exact install commands once, with progress 12 → 58 → 100; an installed emulator not installed again; an install failure with Flatpak's message, then a working retry; a missing core fetched and present ones left alone; a bad core download reported with nothing written; setup resuming at step two; the three statuses; session folders created while the user's `retroarch.cfg` and `Dolphin.ini` stay byte-identical; BIOS copied and missing ones named; different declared actions; listing with systems and status keeping the installed flag; nothing written to Drive; the simulated Flatpak. Plus the progress parser, `romshelf emulators` (list, set up, unknown name), CLI parsing, and 12 UI tests (Emulators screen rows, actions, progress, failure; the dialog's emulator line, functions, Set up first, setup progress and retry, unsupported system).

### Choices made

- **Installed by anyone counts.** The stage file says `flatpak info --user`; that would miss EmuDeck's system-wide installs and add a duplicate.
- **Romshelf's settings live inside each Flatpak's own data folder**, because the sandbox can always reach it. Nothing outside it is written.
- **Status has no "Update available" yet.** Flathub updates come through Discover or `flatpak update`. Noted for stage 9's Emulators screen.
- **The stage file calls this the "ROM page".** Until stage 9 builds it, the game's actions dialog carries its parts: the emulator line, setup and functions.
- `Error::Invalid` no longer prefixes messages with "invalid:", since many reach the user (stage 3's storage messages too).
- `romshelf emulators` is a new subcommand (CLAUDE.md updated).

### Confirm on the Deck

- **Where the RetroArch Flatpak keeps cores.** Assumed `~/.var/app/org.libretro.RetroArch/config/retroarch/cores` (its default `libretro_directory`). If EmuDeck changed `libretro_directory`, RetroArch still finds cores when stage 5 passes the full path with `-L`.
- **Dolphin's user folder option:** `-u <folder>` (`--user`). Checked in stage 5 when it is first used.
- That the buildbot's generic Linux cores load in the Flatpak RetroArch (they're what its own core updater downloads).

### On-Deck checklist

- [ ] With Dolphin not installed, open a GameCube game's actions: it says "Needs Dolphin" and offers **Set up Dolphin** first (screen 2b).
- [ ] Press **Set up Dolphin**: it installs with visible progress and the game then shows "Runs in Dolphin" (screen 2).
- [ ] Do the same for RetroArch (or **Repair** it if EmuDeck already installed it) and confirm the snes9x, mgba and mupen64plus_next cores are present.
- [ ] The Emulators screen shows both as installed.

## Stage 3: download, remove and storage (branch `stage-03-download-delete`)

Status: **built; waiting for on-Deck checks.**

### Built

- **Storage root:** ROMs go to `<root>/roms/<system>/<file>`. The default root is `~/Romshelf` (see choices). `romshelf storage` shows it and the free space; `romshelf storage --set <folder>` moves every downloaded ROM there (checking all destinations first, refusing to overwrite a different file, never while downloads run) and saves it in `~/.config/romshelf/settings.json`.
- **Downloads** (`transfer/mod.rs`):
  - Stream into `<file>.part` instead of memory, so GameCube images over 1 GB are fine. The HTTP seam has a streaming `send_to`, and the Drive trait has `download_to`.
  - Resume from the `.part` length with a `Range` request. The Google client retries only before the first byte arrives; after that the downloader resumes. A dropped connection is retried after 2 s, doubling to 30 s, 12 times in all (about four minutes).
  - Check size and the MD5 Drive reports, then rename into place. A mismatch discards the file and says so.
  - Check free space first, with 256 MB headroom, and refuse with the shortfall named ("Free up 1.2 GB more…") before writing anything.
  - The queue lives in `transfer_queue` (queued, running, paused, failed, done) and runs one download at a time. A download running when Romshelf closed resumes on the next start. Cancel stops the running one (removing its `.part`) or takes a waiting one off the queue.
  - Progress events (bytes, total, rate) at most four a second; the tile's percentage is kept in the database too.
- **Locations on start** (`storage::reconcile`): an on-Deck ROM whose file is missing or the wrong size becomes "In Drive only", a matching file found on disk becomes "On Deck", and a download with nothing queued is cleared. If the storage root and its parent are both missing (the SD card is out), nothing is changed and the Library shows "The storage at … isn't available…"; downloads refuse with the same message.
- **Remove from Deck** (`transfer::remove_from_deck`): deletes the local file and any `.part` to free space; the game stays in Drive and shows "In Drive only". A game that is downloading can't be removed until the download is cancelled. **There is no way to delete from Drive** (see choices).
- **Commands:** `romshelf download <id>` (progress on stderr), `romshelf remove <id>`, `romshelf storage [--set <folder>]`.
- **Interface:**
  - A on a tile opens the game's actions: metadata in mono, "Download to Deck" / "Download again", a progress bar with percentage, rate and time left plus "Cancel download", "Remove from Deck…" (only for games on the Deck), and the last error.
  - X on a tile that is only in Drive downloads it (hint shown in the footer). Tiles show live "Downloading 64%".
  - The remove dialog: "Remove {name} from this Deck?", how much space it frees, and that the game stays in Google Drive. Cancel is focused first.
  - B closes or goes back. The D-pad stays inside an open dialog, and focus returns to the tile afterwards.

### Tests

19 transfer tests: happy path; interrupted at 40% resumes with the right range and identical bytes; restart with a running item resumes from the `.part`; checksum mismatch discards and marks failed; not enough space writes nothing and names the shortfall; headroom boundary; cancel removes the `.part`; queueing once; progress ends at the total; removing from the Deck leaves Drive and every other local file unchanged; downloading and removing never change ROM files in Drive; a queued download cancelled by removing; reconcile (missing, copied in, wrong size); SD card removed is unavailable, not deleted; downloads refuse while unavailable; the default root is created on first download; moving the root keeps files without downloading again; refusing to overwrite a different file. Plus a streamed Google download (range, retry before the first byte, 416), three CLI tests, CLI parsing (including that `delete` and `--scope drive` don't exist), sizes, and 10 UI tests for the dialogs and progress wording.

### Choices made (not covered by the kit)

- **No deleting from Drive** (your decision, 2026-10-19). The stage file's "From Drive" and "From both" scopes are gone, along with the "Keep save backups" question that only applied to them. You manage the files in Drive; Romshelf only reads ROMs from it. This is now a rule in CLAUDE.md. The delete dialog's single choice also removed the confusing "two things look selected" border.
- **Default storage root `~/Romshelf`, not `~/Emulation`.** This Deck has EmuDeck, which keeps its own ROMs in `~/Emulation/roms/<system>`. Romshelf stays out of it. `ROMSHELF_ROM_ROOT` still overrides everything.
- **"SD card removed" means the root and its parent are both missing.** SteamOS removes the card's mount point, so that is the signal. A missing root whose parent exists (`~/Romshelf` before the first download) is simply created.
- **The stage file's paths:** it says `~/Applications/Romshelf/romshelf.AppImage`; the fixed decision 8 path `~/Applications/romshelf.AppImage` is unchanged (that's stage 10's concern).
- **One download at a time.** The stage allows one or two.
- **The actions dialog stands in for the drawer and ROM page** until stage 9. It has no Play yet (stage 5).
- **Storage location in the interface:** only the CLI for now. The Settings screen ("Storage") comes in stage 9.
- **New subcommands** `download`, `remove`, `storage` and the `DiskSpace` seam are added to CLAUDE.md.

### On-Deck checklist

- [ ] Download a large ROM; progress moves and the time estimate is sensible.
- [ ] Turn Wi-Fi off mid-download and back on; it completes.
- [ ] Remove a downloaded game from the Deck: it frees the space, shows "In Drive only", and the file is untouched in Drive. (Replaces "Delete from Drive", which no longer exists.)
- [ ] Set the storage root to the SD card and download there. Postponed 2026-10-19 at your request: ROMs stay on internal storage for now. The card is mounted at `/run/media/deck/FF8S5` (121 GB free), so the command is `romshelf storage --set /run/media/deck/FF8S5/Romshelf`.

## Stage 2: library index and sync (branch `stage-02-library-sync`)

Status: **on-Deck checks passed 2026-10-19.**

### Built

- `model.rs`: the stage's extension table (SNES `sfc smc zip 7z`, GBA `gba zip 7z`, N64 `z64 n64 v64 zip 7z`, GameCube `iso rvz gcz ciso`, PS2 `iso chd`), matched in any case by `System::recognizes`. The folder decides the system; other files are kept as "not recognized" and never launched.
- `sync/mod.rs`:
  - The first sync lists every `roms/<system>/` folder. Later syncs read Drive's change feed from the stored token; a rejected token (or a different Drive folder) means a full scan again.
  - Files are matched by Drive file id: a rename keeps the rom-id and updates the name; a move between system folders keeps the rom-id and changes the system.
  - Trashed, deleted or moved-out files are marked `gone`, never deleted, so saves, controls and shortcut records stay. Restoring one brings back the same rom-id (on Deck if the local file is there).
  - Only real files directly in a system folder count: not sub-folders, Google Docs or Drive shortcuts.
  - Rows are written only when something changed, and `.romshelf/index.json` (file id → rom-id and chosen name) only when it changed. The sync steps past its own index write, so the next sync with nothing new writes nothing.
  - A second device adopts ids and chosen names from `index.json`. An index from a newer Romshelf stops sync with "Please update Romshelf".
- Display names: the stem with `(…)`/`[…]` tags removed and `_`/`-` as spaces, title-cased only when the stem was all lower case. A chosen name (`sync::set_display_name`) wins and follows renames; the interface to set one comes later.
- Database migration 2: `name_overrides`. Also `rom_by_drive_id`, save-backup rows (for the "saves untouched" test) and `total_changes`.
- Google client: HTTP 400 (such as an expired change token) is now `Error::Invalid`, so sync falls back to a full scan.
- `romshelf sync` prints one line, such as "Synced 12 files: 2 new, 1 not recognized."
- Interface: the Library syncs when it opens, and when the sync line in the top band is selected. The sync line reads "Google Drive synced 2 min ago", "Syncing with Google Drive…", "Offline" or "Sync failed", with the reason shown above the grid.
  - System tabs (All, SNES, GBA, N64, PS2, GameCube) change with L1/R1 (Page Up/Page Down on a keyboard). The header counts "{n} ROMs in Drive · {n} on Deck" for the tab.
  - Tiles show recognized files; a "Not recognized" group lists the rest with their folder.
  - A folder strip shows the Drive path, and an empty library offers "Sync now".
  - Focus goes to the first tile on arrival and on a tab change, and stays put when a sync reloads the library.
- Fake mode puts the fixture ROMs in its fake Drive, so it syncs for real and finds nothing to change.

### Tests

21 sync tests: extension table (upper case too, `.txt` unrecognized, GameCube `.zip` unrecognized); 14 display-name cases; 500 files in one full scan; a second sync with no database or Drive writes; new upload; unrecognized files; only real files in system folders; rename; chosen name surviving a rename and being cleared; move between systems; trash and restore with save rows untouched; restore when the file is on the Deck; permanent delete and moving out; full scan marking missing files gone; second device adopting ids (including a moved file's) and names; index written only on change; rejected token; new Drive folder; id collision; newer index; summary wording. Plus Google 400 → invalid, `Connection::sync`, fake-mode sync, and 11 UI tests (tabs and L1/R1 wrap, counts, singular, sync wording and ages, tiles vs not recognized, tab filter, empty tab, Sync now, disabled while syncing, failure message).

### Choices made (not covered by the kit)

- **rom-id:** the stage file says `<system>-<8 random hex>`; CLAUDE.md's fixed decision 5 (FNV-1a of the Drive file id), already built in stage 0, wins. On a collision the file id is hashed with a counter (`derive_salted`), so every device picks the same id. `index.json` still matters: a moved file keeps its first id, which a fresh hash would not give.
- GameCube `.gcm` (stage 0's list) is dropped to match the stage table.
- "Not recognized" files are listed under the grid, not as tiles; "ROMs in Drive" counts only recognized files.
- The time of the last sync lives in memory, not the database, so a sync with nothing new writes nothing.
- Paging is the Drive client's job (tested in `google.rs`); the fake Drive returns whole listings.
- A system folder that is itself trashed isn't detected until a full scan.

### On-Deck checklist

- [x] Upload a ROM from a phone, press Sync, and it appears under the right system.
- [x] Rename it in Drive, sync, and the tile updates without duplicating.
- [x] Counts in the header match Drive.

Results:

- 2026-10-19: `romshelf sync` on the Deck against the real Drive: "Synced 2 files: 2 new, 1 not recognized.", then "Synced 2 files. Nothing changed." The GameCube `.zip` is listed under "Not recognized"; all three checks passed in the interface.

## Stage 10a: self-update, pulled forward (branch `stage-10a-self-update`, on top of stage 1)

Status: **merged; on-Deck checks passed 2026-10-18 except the offline start. The Deck runs 0.1.2 with 0.1.1 kept as previous.**

Only stage 10's step 3 (self-update) and step 9 (release workflow), brought forward so a Deck stays current without copying builds over SSH. The rest of stage 10 is unchanged and still comes last.

### Built

- `update.rs`: reads the latest GitHub release, ignores same, older and rolled-back versions, downloads `romshelf-update.json` and its `.sig`, verifies the Ed25519 signature against the public key built in from `crates/romshelf-core/update-key.pub`, requires the manifest's version to match the tag, downloads the AppImage, checks size, SHA-256 and the ELF header, writes it beside the current file, then swaps with renames, keeping `romshelf.AppImage.previous` (put back if the second rename fails). Checks at most once a day, retries at the next start after a failure, and does nothing while `launch.lock` names a live process (stage 5 will write it). Off when the build has no key or isn't running from an AppImage (`APPIMAGE` unset).
- Rollback swaps `.previous` back and records the rolled-back version so it isn't installed again; the next newer release is.
- CLI: `romshelf update` (check and install now), `--rollback`, `--set-token` (read-only GitHub token while the repository is private; kept in the token store).
- Interface: a background check when the interface opens (not in fake mode, never from `romshelf launch`); a line in the top band, "Romshelf {version} is ready. Restart Romshelf to use it."
- `.github/workflows/release.yml`: on a `v*` tag, runs the full CI build (`ci.yml` is now reusable), refuses a tag that doesn't match `Cargo.toml`, writes and signs the manifest with the `UPDATE_SIGNING_KEY` secret, verifies the signature against the committed public key, and publishes the release.
- `scripts/new-update-key.sh`: creates the key, writes the public half, stores the private half as the secret and saves a backup outside the repository.
- `tauri.conf.json` no longer has its own version; Cargo's workspace version is the only one.
- `docs/releasing.md`: one-time setup and per-release steps.

### Tests

17 updater tests (18 on Linux): newer release installed with the previous kept and the file executable; same and older ignored with no download; bad checksum; forged signature rejected before the AppImage is fetched; a signed manifest replayed under a newer tag; a non-AppImage download; an interrupted download leaves the current version and the check is retried next start; once a day unless forced; skipped while a launch runs; a stale lock from a crashed launch ignored (Linux); rollback restores the old file and skips the bad version but installs a later one; rollback with nothing kept; token sent; private repository without a token explains `--set-token`; release missing a file; no key → off; an OpenSSL-made signature (as CI makes them) verifies. Plus CLI parsing and four `romshelf update` command tests, and two UI tests for the notice.

### Choices made

- A small updater in `romshelf-core` instead of Tauri's updater plugin: the plugin keeps no previous version for rollback, and its logic can't be tested with our fakes.
- The release is signed (Ed25519, OpenSSL in CI, `ed25519-dalek` on the Deck), not just checksummed, so a tampered release is refused even though the checksum travels with it.
- `romshelf update` is a new subcommand (added to CLAUDE.md's fixed decision 1).
- The new version is installed at once and runs from the next start; nothing restarts Romshelf by itself.
- Downloads go through the same `HttpTransport` as Drive, so the AppImage is held in memory (about 80 MB) before it's written.

### Confirm

- GitHub's release JSON and asset downloads (`Accept: application/octet-stream`, redirect to storage) behave as coded: confirmed 2026-10-18 with the public repository and no token. The private-repository path with a fine-grained token was never used (the repository was made public instead).
- The AppImage runtime sets `APPIMAGE` on SteamOS, including when started from a Steam shortcut: confirmed 2026-10-18, the update replaced the file while Romshelf ran from its Steam shortcut in Game Mode.
- Tauri reads the version from Cargo when `tauri.conf.json` has none: confirmed, `--version` and the top band show 0.1.1 and 0.1.2 for those releases.
- Found on the Deck: `romshelf update` run by hand counts as that day's check, so the interface doesn't check again for 24 hours. Kept as is.

### On-Deck checklist

Before these: run `scripts/new-update-key.sh`, commit the public key, set the GitHub token on the Deck (`update --set-token`) while the repository is private, merge, tag a release (say `v0.1.1`), and install that build by hand once. Then publish `v0.1.2`.

- [x] Open Romshelf: within a minute the top bar says "Romshelf 0.1.2 is ready". `~/Applications/romshelf.AppImage.previous` exists. 2026-10-18: yes, opened from Steam in Game Mode; the new file matches the release checksum.
- [x] Quit and open it again: it runs 0.1.2 (`~/Applications/romshelf.AppImage --version`).
- [x] Existing Steam shortcuts (Romshelf itself) still launch.
- [x] `romshelf update --rollback` puts 0.1.1 back, and opening Romshelf doesn't reinstall 0.1.2. 2026-10-18: rolled back over SSH; `update` then reported 0.1.1 up to date. 0.1.2 was put back by hand afterwards.
- [ ] With Wi-Fi off, Romshelf opens normally and updates the next time it's online. Not tried yet.

## Stage 1: Google Drive connection (branch `stage-01-drive-connection`)

Status: **on-Deck checks passed 2026-10-18, except the weekly re-sign-in (due about 2026-10-25).**

### Built

- `http.rs`: an `HttpTransport` seam (`ReqwestTransport`, rustls) under the Google client so its retry and token logic is tested with `FakeHttp`.
- `auth.rs`: client JSON parsing ("Desktop app" only), PKCE S256, consent URL (`drive` scope, offline, `prompt=consent`), one-shot loopback listener on `127.0.0.1:<random>` that checks `state`, ignores stray requests such as `/favicon.ico`, times out and can be cancelled; code exchange (refuses a grant without the Drive scope), refresh ahead of expiry and after a 401 (once), `invalid_grant` signs out with "Sign in again", logout revokes (best effort). `open_browser` uses `xdg-open` with the AppImage's library variables removed.
- `drive/google.rs`: `DriveClient` over Drive v3: about, list (paged, trash excluded, query escaped), get, create folder, multipart create, media update, rename, move, trash (never delete), ranged download (416 at the end → empty), start page token, paged change feed. Up to 5 attempts with exponential backoff on 429, 5xx, rate-limit 403s and network errors; other errors surface Google's message.
- `drive/scaffold.rs`: `Emulation/{roms,saves,states,bios,.romshelf}` plus `roms/<system>` and `bios/<system>` under any parent. Lookup is parent id + name (folders only), one listing per parent, idempotent; `.romshelf/settings.json` (schema 1) written or updated in place; a newer schema stops setup with "Please update Romshelf" and is left untouched.
- `settings.rs`: local `~/.config/romshelf/settings.json` (account, Drive location, folder ids), atomic writes, newer schema refused.
- `tokens.rs`: `KeyringTokenStore` (Secret Service on Linux) behind `FallbackTokenStore`, which falls back silently to the 0600 `FileTokenStore`.
- `connection.rs`: what the CLI and UI share: status, set client (a new client id signs out), login (records the account; a different account forgets the old folder), logout (keeps the folder), folder listing, setup.
- CLI: `auth set-client --file <json>` or `--client-id` (secret prompted), `auth login` (prints the address if no browser opens), `auth logout`, `auth status` (exit 0 when signed in; it also counts the files in each `roms/<system>/` so the scope check can be done before stage 2).
- UI: first run per guide §6: welcome, sign in (waiting, cancel, "Can't see a browser?", Desktop Mode steps, set-client instructions when no client is stored), folder browser (A enters, B goes up, path in mono, note when an `Emulation` folder already exists), progress, done with the tree. The library's empty state shows the chosen folder.
- `ROMSHELF_FAKE=1` starts signed in on a fake Drive; add `ROMSHELF_FAKE_FIRST_RUN=1` to walk the first run against it. In a plain browser, `?first-run` previews it.
- `docs/google-setup.md`: Cloud project, Drive API, consent screen, Testing (weekly sign-in, add yourself as a test user) or In production, Desktop client, `set-client` over SSH.

### Tests

90 Rust tests, 34 front-end tests. New: PKCE RFC 7636 vector and generated verifiers; consent URL; client JSON (desktop, web rejected, malformed); loopback right state, wrong state, refused consent, timeout, cancel; expired token refreshes exactly once then retries; 401 refreshes once and retries, a second 401 is not refreshed; revoked refresh token signs out; 429 then 200; five 500s fail with the last error and no sixth request; network errors and quota 403s retried, permission 403s not; 404 → not found; paging for list and changes; query escaping; multipart upload; media update; Range downloads; never `DELETE`; full login against the loopback; login without Drive scope, without a client, without a browser; logout online and offline; scaffold on empty, twice, partial, same names elsewhere, a file named like a folder, under a chosen folder, progress; settings round-trip, update in place, newer schema refused and untouched, damaged; local settings; keyring fallback; connection first run, folder listing, logout keeps folder, account switch, client switch; CLI status/login/set-client/logout; fake-mode state; first-run screens (each step, waiting, no browser, errors, set-client, folder navigation and B, create, failure, returning account).

### Choices made (not covered by the kit)

- The Google project may stay in "Testing" (your choice): sign-in then lasts seven days. The kit's "In production" remains in the setup guide as the other option.
- Kept the stage 0 trait names (`list`, `create_file`, `download(offset)`) instead of the stage file's (`list_children`, `upload`, `download_range`); added `about` and `update_content`.
- `HttpTransport` is a new seam (real `ReqwestTransport`, fake `FakeHttp`); added to CLAUDE.md's table.
- The client ID and secret live in the token store too (key `google_client`), not in a plain config file.
- `prompt=consent` on every sign-in so Google always returns a refresh token.
- Step 3 focuses "Create Emulation folder here" first; folders are one D-pad press up.
- Sign-in waits 10 minutes before timing out.

### Confirm on the Deck

- **Keyring in Game Mode.** Tokens saved to KWallet while signing in from Desktop Mode may be unreadable in Game Mode, where no wallet service runs, and Romshelf would look signed out there. If so, the fix is to keep tokens in the 0600 file only on SteamOS. That changes a fixed decision, so it needs your OK.
- Whether KWallet asks to create a wallet or for a password the first time Romshelf saves a token.
- Whether `xdg-open` can open a browser from Game Mode at all.
- CI builds the vendored `libdbus` for `keyring` on Ubuntu 22.04.

### Deferred

- Stage file step 7's integration test against a throwaway Google account: needs a real account and client, so it is the on-Deck checks below.
- Settings screen (account, "Sign out", "Change location…"): stage 9.

### On-Deck checklist

Before these, follow `docs/google-setup.md` to create the client and run `auth set-client`.

- [x] `auth login` opens a browser on the Deck and completes. Record whether this works from Game Mode or only from Desktop Mode. 2026-10-18: the interface's sign-in, started from Steam in Desktop Mode, opened Chrome and completed. Game Mode: to be seen at the first weekly expiry.
- [x] The folder tree appears in Google Drive at the chosen location. 2026-10-18: created under My Drive / EmuSuite.
- [x] Upload a small file to `roms/gba/` through the Drive website from another device, then confirm Romshelf can list it: `auth status` shows `gba 1` under "Files in roms/". This proves the scope decision. 2026-10-18: files uploaded through the Drive website to `roms/gba/` and `roms/gamecube/` show as `gamecube 1, gba 1`.
- [x] Quit and reopen: still signed in. Check in both Desktop Mode and Game Mode. 2026-10-18: reopened from Steam in Game Mode, it went straight to the library.
- [ ] The project stays in "Testing" (your choice, 2026-10-18), so instead of "eight days later: still signed in": after about seven days Romshelf says the sign-in expired, and signing in again works without setting up the folder again.

Results:

- Browser from Game Mode: not tried yet; signed in from Desktop Mode. Will show at the first weekly expiry.
- Keyring in Game Mode: not an issue on this Deck. Signing in from the interface in Desktop Mode saved the client and token to the 0600 file (`~/.config/romshelf/token.json`), not KWallet, and Game Mode reads them.
- KWallet prompt: none.

## Stage 0: foundations (branch `stage-00-foundations`)

Status: **done; on-Deck checks passed 2026-10-18.**

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

- [x] The AppImage starts from Desktop Mode (double-click, or `~/Applications/romshelf.AppImage` in Konsole). 2026-10-18: started over SSH with `ROMSHELF_FAKE=1`; full screen, eight fixture tiles.
- [x] Added to Steam, it starts in Game Mode, fills the screen, and returns to Steam on exit ("Quit Romshelf").
- [x] Spike A: on "Open gamepad test", D-pad, face buttons, bumpers and sticks all register. Note which path (browser API, `gilrs`, or both) shows them and which one the header says navigation uses.
- [x] Fonts render as in the screenshots (`docs/kit/ui/screens/01-library.png`). 2026-10-18: sans and mono faces match in a Desktop Mode screenshot.

Results:

- Spike A (input path): 2026-10-18: D-pad, face buttons, bumpers and sticks register on both the browser Gamepad API and `gilrs`. Navigation locks to whichever source reports first, so either works; no change needed.
- Spike B (Game Mode): 2026-10-18: starts from a non-Steam shortcut, fills the screen, and "Quit Romshelf" returns to Steam.
- `WEBKIT_DISABLE_DMABUF_RENDERER` needed?: Not on this Deck: with it set to `0` in Game Mode the UI still renders. Kept as a harmless default.
