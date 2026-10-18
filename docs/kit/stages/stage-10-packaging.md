# Stage 10: packaging and hardening

**Goal:** a fresh Deck can install Romshelf, update it without breaking shortcuts, and recover from the common faults.

**Depends on:** Stages 0 to 9.
**Read first:** `LINKS.md` → Tauri (AppImage).

## Implementation steps

1. Install location: `~/Applications/Romshelf/romshelf.AppImage`, with a `romshelf` symlink in `~/.local/bin`. Shortcuts use the AppImage path.
2. Installer script: download the AppImage to that path, mark it executable, create a desktop entry, and add Romshelf itself to Steam as a shortcut (through the Stage 8 code).
3. Self-update: check a release feed, download beside the current file, verify a signature or checksum, and replace in place with a rename. Never while a launch is in progress. The previous version is kept for one-step rollback.
4. Data migrations: database and Drive `settings.json` carry schema versions; upgrades are forward-only with a backup of the database before migrating.
5. Logging: rotating log files; a "Copy diagnostics" action that bundles logs, versions and `doctor` output with tokens and file names of ROMs redacted.
6. `romshelf doctor`: checks sign-in, Drive folder IDs, storage root present and writable, each emulator installed, Steam directory found, shortcut targets pointing at the current install path, pending backups, and index consistency. Each failing check prints one fix.
7. Offline behaviour: launching a downloaded game needs no network at any step. Sync, backups and artwork fetches queue and retry.
8. Crash safety: a crash at any pipeline step leaves no partial file that is later trusted; on next start, incomplete transfers resume and stale session files are cleaned.
9. Release workflow in CI: tag → build → checksum → publish.
10. User documentation: a README covering install, Google setup, adding ROMs, and what Romshelf does and does not touch.

## Automated tests

- Update: version N with data upgrades to N+1; database, settings, saves and shortcut targets are intact.
- Update interrupted mid-download: current version still runs.
- Bad checksum on update: rejected, current version untouched.
- Rollback restores the previous binary.
- Migration from every earlier schema version in fixtures.
- `doctor`: each check has a test that makes it fail and asserts the message.
- Diagnostics bundle contains no token, client secret or ROM file name.
- Offline launch: with fake Drive erroring on every call, a downloaded ROM launches and the backup is queued.
- Fault injection: kill at each pipeline step, restart, and assert a consistent state.

## On-Deck checks

- [ ] On a Deck (or user profile) that has never had Romshelf: run the installer, complete first run, add a ROM, play it from Steam.
- [ ] Update to a newer build: existing Steam shortcuts still launch.
- [ ] Airplane mode: launch a downloaded game from Steam, play, quit; the backup uploads when the network returns.
- [ ] Remove the SD card holding the ROMs: the app explains what is wrong and recovers when it is reinserted.
- [ ] `romshelf doctor` reports all green on a healthy install.

## Done when

Tests pass, a clean install works end to end, and an update leaves every shortcut working.

## Pitfalls

- SteamOS updates replace the system image but keep the home directory. Everything Romshelf installs must live under home.
- If the install path ever has to change, a migration must rewrite every Romshelf shortcut's `Exe` and `StartDir`.
