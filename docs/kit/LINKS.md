# Romshelf: relevant links

Status column: **Checked** means the page was opened on 10 October 2026 and says what the note claims. **Not checked** means the address is from memory or the page could not be read; open it before relying on it.

## Tauri (Stages 0, 9, 10)

| Link | What it is for | Status |
|---|---|---|
| https://v2.tauri.app/start/ | Tauri 2 getting started | Checked |
| https://v2.tauri.app/develop/calling-rust/ | Calling Rust commands from the front end | Not checked |
| https://v2.tauri.app/plugin/cli/ | Plugin for defining and reading command-line arguments | Checked |
| https://v2.tauri.app/develop/tests/webdriver/ | WebDriver end-to-end testing; Linux supported | Checked |
| https://v2.tauri.app/distribute/appimage/ | Packaging as an AppImage | Checked |
| https://docs.rs/gilrs | Rust gamepad library, the fallback input path | Checked |
| https://docs.github.com/en/rest/releases/releases#get-the-latest-release | Self-update feed: `tag_name`, `assets[].name`, `assets[].url` | Not checked (Confirm) |
| https://docs.github.com/en/rest/releases/assets#get-a-release-asset | Asset download with `Accept: application/octet-stream` (redirects to storage) | Not checked (Confirm) |
| https://docs.appimage.org/packaging-guide/environment-variables.html | `APPIMAGE`: path of the running AppImage, used as the file to replace | Not checked (Confirm) |
| https://v2.tauri.app/reference/config/#version | `version` may be left out of `tauri.conf.json` to use Cargo's | Not checked (Confirm) |

## Google sign-in and Drive (Stages 1 to 3, 6)

| Link | What it is for | Status |
|---|---|---|
| https://developers.google.com/identity/protocols/oauth2/native-app | Desktop sign-in: loopback redirect, PKCE; embedded web views and copy-paste flow not allowed | Checked |
| https://developers.google.com/identity/protocols/oauth2/limited-input-device | Device-code flow; only `drive.file` and `drive.appdata` allowed, so not usable here | Checked |
| https://developers.google.com/workspace/drive/api/guides/api-specific-auth | Drive scopes; `drive.file` limits and which scopes are restricted | Checked |
| https://support.google.com/cloud/answer/15549945 | Publishing status: seven-day expiry in Testing, 100-user cap unverified | Checked |
| https://developers.google.com/workspace/drive/api/reference/rest/v3 | Drive API v3 reference | Checked |
| https://developers.google.com/workspace/drive/api/guides/manage-changes | Change feed: start page token and `changes.list` | Checked |
| https://developers.google.com/workspace/drive/api/guides/manage-downloads | Downloads with `alt=media` and partial ranges | Checked |

## Emulators and Flatpak (Stages 4 to 7)

| Link | What it is for | Status |
|---|---|---|
| https://docs.flatpak.org/en/latest/flatpak-command-reference.html | `flatpak install --user`, `info`, `run` | Checked |
| https://flathub.org/apps/org.libretro.RetroArch | RetroArch Flatpak | Checked |
| https://flathub.org/apps/org.DolphinEmu.dolphin-emu | Dolphin Flatpak | Checked |
| https://flathub.org/apps/net.pcsx2.PCSX2 | PCSX2 Flatpak (later adapter) | Checked |
| https://docs.libretro.com/guides/cli-intro/ | RetroArch command line: `-L`, `--config`, `--appendconfig` | Checked |
| https://docs.libretro.com/guides/overrides/ | RetroArch per-core and per-game overrides and remap file locations | Checked |
| https://buildbot.libretro.com/nightly/linux/x86_64/latest/ | Libretro core downloads (`<core>_libretro.so.zip`), used by stage 4 setup | Not checked (Confirm on the Deck) |
| https://wiki.dolphin-emu.org/index.php?title=GameINI | Dolphin per-game settings | Not checked (page blocked automated reading) |
| https://pcsx2.net/docs/ | PCSX2 documentation overview | Checked (overview only) |

## Steam (Stages 7, 8)

| Link | What it is for | Status |
|---|---|---|
| https://developer.valvesoftware.com/wiki/Add_Non-Steam_Game | `shortcuts.vdf` location, binary format and entry fields | Checked |
| https://docs.rs/steam_shortcuts_util | Rust crate: `parse_shortcuts`, `shortcuts_to_bytes`, `calculate_app_id_for_shortcut` | Checked |
| https://github.com/PhilipK/BoilR | Rust tool that adds games to Steam with artwork; reference implementation | Checked |
| https://github.com/SteamGridDB/steam-rom-manager | Established shortcut and artwork manager; reference for artwork naming in its source | Checked (README only) |
| https://github.com/360900/tinkergame/wiki/Add-Non-Steam-Game | Grid folder path and artwork file names; inconsistent about two slots | Checked |
| https://www.steamgriddb.com/api/v2 | SteamGridDB API for optional artwork search | Not checked (page loaded, content unreadable) |
| https://partner.steamgames.com/doc/features/steam_controller | Steam Input documentation | Checked |
| https://tomevault.io/tome/danielcopper/decky-romm-sync | Notes from a similar project on shortcut quirks | Checked |

## Rust crates likely to be used

| Link | What it is for | Status |
|---|---|---|
| https://docs.rs/clap | Argument parsing | Not checked |
| https://docs.rs/rusqlite | SQLite | Not checked |
| https://docs.rs/reqwest | HTTP client | Not checked |
| https://docs.rs/keyring | System keyring access | Not checked |

## Claude Code

| Link | What it is for | Status |
|---|---|---|
| https://code.claude.com/docs/en/memory | `CLAUDE.md` project memory | Not checked (redirect target, content not read) |
