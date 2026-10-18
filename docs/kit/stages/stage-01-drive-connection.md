# Stage 1: Google Drive connection and folder scaffold

**Goal:** the user signs in to Google once, picks a location, and Romshelf creates its folder tree. Tokens survive restarts.

**Depends on:** Stage 0.
**Read first:** UI guide section 6 (first run); `LINKS.md` → Google.

## Background you must respect

- Scope requested: `https://www.googleapis.com/auth/drive`. The narrow `drive.file` scope only covers files the app created or the user opened with it, so ROMs uploaded through the Drive website would be invisible.
- Flow: desktop loopback redirect (`http://127.0.0.1:<random port>`) with PKCE (S256), opened in the system browser. Google rejects embedded web views, the manual copy-and-paste flow is gone, and the device-code flow does not offer this scope.
- The Google Cloud project must be set to "In production". In "Testing", authorizations expire after seven days.
- The client ID and secret belong to the user and are entered with `romshelf auth set-client`. They are never in the repository.

## Implementation steps

1. `drive/google.rs`: implement `DriveClient` over the Drive v3 REST API with `reqwest`: `about`, `list_children`, `create_folder`, `get_metadata`, `upload`, `download_range`, `trash`, `start_page_token`, `changes_since`. Automatic token refresh; retry with backoff on 429 and 5xx.
2. `auth`: PKCE generation, loopback listener, browser launch via `xdg-open`, code exchange, refresh. `TokenStore` real implementation: system keyring if available, otherwise a file with mode 0600 under the config directory.
3. Commands: `auth set-client`, `auth login`, `auth logout`, `auth status`.
4. Scaffold: given a parent folder, create `Emulation/` with `roms/`, `saves/`, `states/`, `bios/`, `.romshelf/`, and a `roms/<system>/` and `bios/<system>/` folder per supported system. Idempotent: look up by name under the known parent before creating.
5. Write `.romshelf/settings.json` in Drive (schema version, folder IDs) and mirror the folder IDs locally.
6. Front end: first-run flow from the UI guide: sign in, choose location (folder browser driven by `list_children`), confirm, progress, done.
7. Write a short `docs/google-setup.md` for the human: create the Cloud project, enable the Drive API, create a Desktop client, set "In production".

## Automated tests

- PKCE: verifier and challenge match the specification's test vector.
- Loopback: a simulated redirect with the right `state` yields a code; a wrong `state` is rejected; a timeout produces a clear error.
- Token refresh: an expired access token triggers exactly one refresh and the original request is retried.
- Scaffold on empty fake Drive: every expected folder exists exactly once.
- Scaffold twice: no new folders, no errors.
- Scaffold on a partial tree (only `Emulation/roms/snes` exists): the rest is created and nothing is duplicated.
- Scaffold when a same-named folder exists in a different parent: it is ignored.
- Settings file round-trips; an unknown newer schema version produces a "please update" error, not a crash.
- Retry: a scripted 429 then 200 succeeds; five 500s fail with the last error surfaced.

## On-Deck checks

- [ ] `auth login` opens a browser on the Deck and completes. Record whether this works from Game Mode or only from Desktop Mode.
- [ ] The folder tree appears in Google Drive at the chosen location.
- [ ] Upload a small file to `roms/gba/` through the Drive website from another device, then confirm Romshelf can list it. This proves the scope decision.
- [ ] Quit and reopen: still signed in.
- [ ] Eight days later: still signed in.

## Done when

Tests pass, the on-Deck checks pass, and `auth status` reports the account and the Drive folder.

## Pitfalls

- Game Mode may have no browser to open. If so, the first-run screen must say "Finish sign-in in Desktop Mode" with the exact steps, and `auth login` must work there. Do not try to embed the consent page.
- No secret service may be running in Game Mode. The token store must fall back to the file silently.
- Folder names are not unique in Drive. Always resolve by parent ID plus name, then store IDs.
