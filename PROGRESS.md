# Progress

## Stage 10a: self-update, pulled forward (branch `stage-10a-self-update`, on top of stage 1)

Status: **built; needs the signing key and a first release before on-Deck checks.**

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

- GitHub's release JSON and asset downloads (`Accept: application/octet-stream`, redirect to storage without the token) behave as coded; private-repository downloads with a fine-grained token.
- The AppImage runtime sets `APPIMAGE` on SteamOS, including when started from a Steam shortcut.
- Tauri reads the version from Cargo when `tauri.conf.json` has none (the AppImage's file name and About show the right version).

### On-Deck checklist

Before these: run `scripts/new-update-key.sh`, commit the public key, set the GitHub token on the Deck (`update --set-token`) while the repository is private, merge, tag a release (say `v0.1.1`), and install that build by hand once. Then publish `v0.1.2`.

- [ ] Open Romshelf: within a minute the top bar says "Romshelf 0.1.2 is ready". `~/Applications/romshelf.AppImage.previous` exists.
- [ ] Quit and open it again: it runs 0.1.2 (`~/Applications/romshelf.AppImage --version`).
- [ ] Existing Steam shortcuts (Romshelf itself) still launch.
- [ ] `romshelf update --rollback` puts 0.1.1 back, and opening Romshelf doesn't reinstall 0.1.2.
- [ ] With Wi-Fi off, Romshelf opens normally and updates the next time it's online.

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
