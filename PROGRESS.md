# Progress

## Stage 7: controls (branch `stage-07-controls`, on top of stage 6)

Status: **built; waiting for the on-Deck checks.** You chose the back-grip approach, and it works on the Deck.

### Back-grip spike (2026-10-19, on the Deck over SSH)

- Steam keeps the controller layout chosen for each game in `~/.steam/steam/steamapps/common/Steam Controller Configs/<account id>/config/configset_controller_neptune.vdf`. Non-Steam shortcuts are keyed by their name in lower case (`"kirby's return to dream land"`), and each entry names a `template` file.
- EmuDeck (through Steam ROM Manager) used exactly this to give 21 shortcuts its layout (`"template" "emudeck_frontend_controller_steamdeck.vdf"`). That template file is no longer anywhere on the Deck, so Steam is presumably falling back to its own default for them.
- Romshelf's two test shortcuts ("test", "kirb") have no entry, so they use Steam's default gamepad layout.
- So a layout **can** be supplied as a file and applied to shortcuts without the user setting each one up by hand. Writing the entries would happen alongside the `shortcuts.vdf` write in stage 8, with Steam closed, plus a template file of Romshelf's own.
- **Confirmed end to end (2026-10-19):**
  - Steam keys a shortcut's entry by its real name in lower case with the dot dropped: "romshelf.AppImage kirb" became `"romshelfappimage kirb"`. An entry under "kirb" was ignored.
  - With `"autosave" "1"` and Romshelf's layout file at `<that name>/controller_neptune.vdf` (written with Steam shut down), Steam showed the layout "Romshelf" with L4 F9, R4 F10, L5 F11, R5 F12.
  - In Kirby, L4 toggled fast forward and L5/R5 saved and loaded a state, through the RetroArch controls Romshelf wrote.
- The layout came from Steam's own gamepad template with the four grips bound to keys. In Steam's files, `button_back_left_upper` is L4 and `button_back_left` is L5.
- **For stage 8:** write the layout and the `configset_controller_neptune.vdf` entry for each shortcut along with `shortcuts.vdf`, while Steam is closed. Work out Steam's exact rule for turning a name into the entry key. Seen so far: the dot is dropped; spaces, parentheses, `!` and apostrophes are kept.
- The spike's backup of Steam's files is in `~/romshelf-spike-backup/` on the Deck.

### Built

- **The model** (`controls/model.rs`):
  - Inputs: the D-pad, both sticks, A/B/X/Y, L1âR3, Menu, View and the four back grips.
  - Targets: each system's buttons and sticks, plus each emulator's hotkeys (fast forward, rewind (RetroArch only), save state, load state, open menu (RetroArch only), and quit game).
  - Each layout belongs to a scope: an emulator for one system (`retroarch/gba`), since RetroArch's systems have different buttons.
  - Validation:
    - Directions only from the D-pad or a stick.
    - Binding one input twice is rejected.
    - Two inputs may share a target.
- **Merge** (`controls/merge.rs`): merge, difference and promote.
- **Built-in defaults** (`controls/defaults.rs`):
  - Buttons:
    - GBA follows the reference screen (A â A, B â B).
    - SNES and GameCube go by position. GameCube matches EmuDeck's layout: Deck X â B, Deck B â X, R1 â Z, L2/R2 â L/R.
    - N64: right stick â C buttons, L2 â Z.
  - Hotkeys on the grips: L4 fast forward, R4 rewind (RetroArch), L5 save state, R5 load state.
  - Quit has no default button. Steam button â Exit game ends any game, and stage 6 backs its save up on the next run.
- **Storage** (`controls/store.rs`):
  - Defaults and game differences are in `control_profiles`. A game is stored only while it has differences.
  - Each change is copied to Drive's `.romshelf/controls/<emulator>-<system>.json`, and a sync takes newer copies from there. A newer change on this Deck wins over an older Drive copy.
- **Writers:**
  - **RetroArch** (`controls/retroarch.rs`): `games/<rom-id>.controls.cfg`, appended last (`--appendconfig "romshelf.cfg|game.cfg|controls.cfg"`). It contains:
    - Every RetroPad bind for player 1. Button numbers are Steam's virtual gamepad under SDL2, from RetroArch's own autoconfig.
    - Grips as F9âF12.
    - Every hotkey: unbound unless the layout binds it. There's no hotkey-enable button, and autodetect is off, so nothing falls back to EmuDeck's hotkeys (which need View held, and would fire alone otherwise).
  - **Dolphin** (`controls/dolphin.rs`):
    - `[GCPad1]` in Romshelf's copy of `GCPadNew.ini`, with the other pads kept.
    - A fresh `Hotkeys.ini`. Grips are named on the keyboard device (`XInput2/0/Virtual core pointer:F9`).
    - Both are rewritten before each launch.
- **Pipeline:** the Controls step writes the game's merged layout before every launch.
- **Quitting, in every game and every state** (you asked for this on 2026-10-19):
  - Hold View and Menu for a second. Romshelf reads the controller itself while a game runs (from a Steam shortcut or from Play), so it doesn't depend on the emulator, its focus or the game's layout.
  - It stops the emulator the same clean way as Steam's Exit game (ask, then widen, then force), so the emulator writes its saves, and Romshelf backs them up as usual.
  - In the launch window (downloading or setting up), the same combination goes back to Steam.
  - Tested: the hold rule (once per hold, a second, starting over when let go); a requested stop ending a real program (Linux) and a fake one; and a stale request not stopping the next program.
- **Setup's step two** stores the built-in layout as the default for each of the emulator's systems, unless one is already there.
- **Interface:**
  - **The Controls editor** (guide 5.4):
    - The scope switch: "{emulator} default" or "{game} only", switched with L1/R1.
    - Game buttons and Emulator hotkeys lists, tagged "Default" or "This game".
    - Rebind: A, then press the new input. A stick press binds the stick; a grip arrives as its key. A tap of B binds B; holding B for a second cancels.
    - Binding onto a used input asks to swap.
    - An unbound row offers the targets it could do.
    - X clears a row, and Y returns it to the default.
    - Side panel: an explanation (including that Steam's own layout sits above), the custom count, Reset, "Make this the {emulator} default" (confirms with how many games change), and Save.
    - Leaving unsaved asks: Save, Discard or Keep editing.
  - **The game's dialog:** a Controls group ("RetroArch default" or "Custom for this game Â· 2 buttons differ from default"), with **Edit controls** and **Reset**.
  - **The Emulators screen:** a "{system} controls" button per system once the emulator is installed.

### Tests

- 23 Rust tests:
  - The merge rules.
  - Difference-then-merge over 500 generated layouts.
  - Promote and reset.
  - Validation.
  - Every built-in default complete and valid.
  - Storage, and promote naming the games it will change.
  - The Drive mirror on a second device.
  - Setup storing defaults and keeping an existing one.
  - Golden files:
    - RetroArch: GBA default, GBA with differences, N64.
    - Dolphin: pad and hotkeys.
  - Every RetroArch setting is one RetroArch 1.19.1 itself writes (fixture from the Deck), and the button numbers match its Steam Virtual Gamepad autoconfig.
  - The Dolphin default matches the `GCPadNew.ini` Dolphin wrote on the Deck.
- 13 UI tests: the rules, tags, the rebind flow by simulated presses (swap, B tap and hold, sticks, grips), X/Y/L1/R1, the target picker, promote, and leaving unsaved.

### Confirm (on the Deck)

- Dolphin sees the grip keys on `XInput2/0/Virtual core pointer` under Game Mode.
- Mupen64Plus-Next reads the RetroPad as assumed (B = N64 A, Y = N64 B, L2 = Z, right stick = C buttons). There's no N64 game on the Deck yet.

### Deferred

- **Writing Romshelf's Steam Input layout to each shortcut** is stage 8's: that's where shortcuts are written, with Steam closed. The spike shows how.
- **Quit as a layout binding:** the model maps one input to one target, so quit has no default button in the layout (it can still be bound to a free input in the editor). Romshelf's own View + Menu combination, below, covers every game instead.

### On-Deck checklist

- [x] The default layout works in Kirby (GBA) and Twilight Princess (GameCube) with no editing. Twilight Princess's buttons are as before (EmuDeck's positions).
- [x] Swap A and B for Kirby only: swapped there, normal in another GBA game. (Only one GBA game so far: check Kirby swaps, then reset.)
- [x] Bind fast forward for one game: it works in that game.
- [x] Change the RetroArch GBA default: Kirby follows, except its overridden buttons.
- [ ] L4 fast forward works in Kirby (done in the spike) and in Twilight Princess. 2026-10-19: works in Kirby. In Twilight Princess it fast-forwarded only sometimes, but its shortcut ("test") doesn't have Romshelf's Steam Input layout yet, so L4 sent nothing; that wasn't Romshelf's binding. To retest with the layout on that shortcut too.

- [ ] Hold View + Menu for a second in Kirby, and in Twilight Princess: the game closes, back to Steam, and its save is backed up.

Checks 1–4 passed on the Deck, 2026-10-19 (your report).

Twilight Princess saves, checked 2026-10-19 after you reported them missing: Dolphin wrote `01-GZ2E-gczelda2.gci` into the game's own folder at 23:53 after an in-game save, and it reached Drive at 23:54. So the per-game memory card works. Earlier sessions had no new save on disk because none was made in game. That means stage 6's check 2 backed up the save moved there at 18:24, not a new one.

## Stage 6: saves and save states (branch `stage-06-saves`, on top of stage 5)

Status: **on-Deck checks passed 2026-10-19.**

### Built

- **One save folder per game, set by Romshelf for the games it starts:**
  - **RetroArch:** saves go to `config/romshelf/saves/<rom-id>/` and states to `config/romshelf/states/<rom-id>/`.
    - Each game gets its own appended config, `config/romshelf/games/<rom-id>.cfg`. The launch passes `--appendconfig "<romshelf.cfg>|<game cfg>"`; RetroArch reads `|`-separated configs in order.
    - `romshelf.cfg` now also turns off saving next to the ROM and per-core sub-folders. EmuDeck's `retroarch.cfg` has these off already, but a user's own settings might not.
    - Saves an earlier Romshelf kept in the shared folder (`Kirby - Nightmare in Dream Land (USA).srm` on the Deck) move into the game's folder the first time.
  - **Dolphin:** memory card A becomes a folder of saves that belongs to the game: `-C Dolphin.Core.SlotA=8 -C Dolphin.Core.GCIFolderAPathOverride=<data/romshelf-user/GC/Romshelf/<rom-id>>`.
    - The game's saves (`01-GZ2E-gczelda2.gci` for Twilight Princess) move there from the shared `GC/<region>/Card A/` the first time.
    - The game is identified by the id in its disc header. That's at 0x58 in RVZ and WIA files, after the 32 KB header of a CISO, and at the start of an ISO or GCM.
- **Backup** (`saves/mod.rs`):
  - Each backup is its own folder in Drive: `Emulation/saves/<system>/<rom-id>/<UTC time>/`. It holds the files plus `manifest.json` (file names, sizes, MD5s, device, emulator, core, and one hash for the whole save).
  - The manifest is written last, so a backup that didn't finish uploading is ignored.
  - Nothing is uploaded when the newest backup already has the same save.
  - The newest 10 backups are kept (the `save_versions` setting). Older ones go to Drive's trash.
- **Restore:**
  1. Downloads the chosen backup and checks every file.
  2. Backs up the Deck's current save first, unless a kept backup already has it.
  3. Swaps the folders.

  If the download fails, the Deck's save is untouched. Restoring a save made with another core gives a warning ("This save was made with retroarch (vba_next); retroarch (mgba) may not load it.").
- **Before each launch,** each game's `save_sync` row (a new table) records the save hash from the last time this Deck and Drive agreed. The Deck's save and Drive's newest backup are each compared with it:
  - Only Drive's changed (played on another machine): it's restored, and the launch says "Restored the newer save from deck-b".
  - Only the Deck's changed: nothing to do; it's backed up when the game ends.
  - Both changed: **Two different saves**, with each side's time and device, and **Use this Deck's** / **Use Drive's** / **Cancel**. Whichever save loses is backed up first.
  - Drive can't be reached: the game starts with the Deck's save.
- **After the game,** whatever the exit code, the save is backed up and new or changed save states are mirrored to `Emulation/states/<system>/<rom-id>/`. A failed backup never changes the exit code.
- **Backups still owed:** a game is marked "backup owed" just before it starts, and the mark is cleared only when a backup succeeds. So when Steam's **Exit game** ends Romshelf, or Drive was unreachable, the backup happens on Romshelf's next run: the next launch, opening Romshelf, or a sync. Nothing is uploaded while a game is running: `launch.lock` names a live process. A lock left behind by an ended Romshelf names a dead one and is ignored.
- **Save states:**
  - The game's slots are listed with their times.
  - Deleting a slot asks first. It's removed from the Deck, and its Drive copy goes to Drive's trash (it's uploaded first if Drive didn't have it).
  - Slots made on another machine are downloaded before a launch.
- **Setup's "Link saves" step** now does real work: it creates `saves/<system>/` and `states/<system>/` in Drive for each of the emulator's systems.
- **Interface:**
  - The game's dialog has a **Save backup** group: "Backed up Today 3:16 PM from steamdeck", "Not backed up yet", or "Waiting to back upÃ¢ÂÂ¦", with **Back up save data**, **Restore save from DriveÃ¢ÂÂ¦** and **Save statesÃ¢ÂÂ¦** (RetroArch games).
  - **Restore a save** lists date, device and size, with Cancel focused first and "Your current save is backed up first."
  - The conflict dialog appears both in the launch window and when you press Play.
  - Toasts: "Backed up Kirby".
- **`romshelf launch` from Steam with a conflict:** nothing starts until you answer. A launch with no window has no one to ask, so it opens the launch window and asks there.

### Tests

35 new Rust tests in `tests/saves.rs`:
- Backup then restore is byte-identical.
- The Drive layout and manifest.
- An unchanged save makes no new version, and no save uploads nothing.
- Retention keeps exactly N, the default is 10, and the old ones go to the trash.
- Two backups in the same second are both kept.
- Restore backs up the replaced save, an older version can be restored with the newer still listed, a failed restore leaves the Deck's save alone, and a different core gives a warning.
- Backups touch nothing else in Drive.
- The conflict matrix (all four combinations), plus both sides reaching the same save, and a backup removed from Drive.
- The pipeline:
  - Backup after the game, and after a non-zero exit.
  - Drive offline at exit: the backup is queued with the exit code unchanged, and uploaded later.
  - A force-quit launch is backed up at the next run, and owed backups wait while a game runs.
  - A stale lock is ignored (Linux).
  - A newer save from another Deck is restored.
  - Two Decks taking turns lose nothing and are never asked.
  - A conflict with no window starts nothing; Keep, Use Drive's and Cancel each do what they say.
  - Offline at the start still plays.
- Setup links the Drive folders.
- RetroArch's per-game folders and config, and moving saves in from the shared folder.
- Dolphin's game id from ISO, RVZ, WIA, CISO (and none from GCZ), and its per-game memory-card folder.
- Save states mirrored, fetched, listed and deleted, and a Deck-only state uploaded before deleting.

Also 2 unit tests (UTC folder names, the save hash), 13 UI tests (time wording, the save backup line, backup outcomes, the restore list, core warning, empty list, save states and delete, the conflict dialog in all three places), and updated stage 4 and 5 tests (setup now adds only its own save folders to Drive; the new RetroArch and Dolphin command lines).

### Choices made

- **"Only Drive changed" restores without asking.** The stage file says both "offer restore (default yes)" and "no prompt appears when only one side changed". Restoring is safe: the save it replaces is already in Drive, since it's the one both last agreed on. So Romshelf restores and says so, and only asks about a real conflict.
- **Saves stay separate from EmuDeck's.** Games started from Romshelf use Romshelf's folders, and EmuDeck's saves on the SD card aren't read or changed. A save made through EmuDeck or ES-DE doesn't carry over to Romshelf, and the other way round.
- **Dolphin's save states aren't kept yet.** Dolphin names them by game id in one shared `StateSaves/` folder, with no setting to separate them per game. GameCube memory-card saves are backed up. RetroArch states are mirrored.
- **Save files are kept flat:** the files in the game's folder, not sub-folders. Neither emulator writes sub-folders there.
- **The machine's name** in each backup is `/etc/hostname` ("steamdeck").

### Confirmed on the Deck (2026-10-19)

- RetroArch reads both files given to `--appendconfig` joined with `|`: Kirby's saves landed in its own folder and were backed up from there (check 1).
- Dolphin 2407 honours `GCIFolderAPathOverride`: Twilight Princess kept its moved save and backed it up from its own card folder (check 2). The setting name is in the Deck's `dolphin-emu` binary, and the RVZ header has `GZ2E01` at 0x58.

### On-Deck checklist

- [x] Play Kirby, save in game, quit with View + Menu: the game's dialog shows a new "Backed up" time, and `Emulation/saves/gba/<rom-id>/<time>/` exists in Drive with the `.srm` and `manifest.json`.
- [x] Play Twilight Princess: your existing save is still there (moved into Romshelf's per-game card folder). Save, quit: a backup with `01-GZ2E-gczelda2.gci` appears in Drive.
- [x] Delete the local save (`~/.var/app/org.libretro.RetroArch/config/romshelf/saves/<rom-id>/`), then **Restore save from DriveÃ¢ÂÂ¦** and pick the newest. Launch: progress is intact.
- [x] Restore an older version: the game shows the older progress, and the newer version is still in the list.
- [x] Airplane mode: play and quit. The dialog says "Waiting to back up". Reconnect and open Romshelf (or sync): the backup uploads and a "Backed up Ã¢ÂÂ¦" toast shows.
- [x] Quit a game with Steam button Ã¢ÂÂ **Exit game**: the next time Romshelf opens, the save is backed up.

All six passed on the Deck, 2026-10-19 (your report).

## Stage 5: launch pipeline (branch `stage-05-launch-pipeline`, on top of stage 4)

Status: **on-Deck checks passed 2026-10-19.**

### Built

- **`launch/mod.rs`, one pipeline** for every Steam shortcut (`romshelf launch --id <id>`, or `--rom gba/Kirby.gba`), the Play button and `romshelf doctor --id`:
  1. Find the game. An unknown id is "Romshelf doesn't know a game called Ã¢ÂÂ¦ Sync, then try again." (exit 2). A game gone from Drive, a system with no emulator, or a file the emulator can't run (the GameCube `.zip`) is a clear error, and nothing starts.
  2. Download it if it isn't on the Deck, with progress.
  3. Set up its emulator if Romshelf's setup hasn't run. A missing required BIOS stops here with what to add.
  4. Check saves, then 5. write controls: placeholders until stages 6 and 7.
  6. Start the emulator and wait. 7. Back up saves (placeholder), which runs whatever the exit code was.

  Romshelf exits with the emulator's exit code unchanged. Whether a step is needed is read from the database only, so checking runs no Flatpak command.
- **Emulator commands:**
  - RetroArch: `flatpak run --filesystem=<storage root>:ro org.libretro.RetroArch -L <core file> --appendconfig <Romshelf's romshelf.cfg> -f <rom>`
  - Dolphin: `flatpak run --filesystem=<storage root>:ro org.DolphinEmu.dolphin-emu -u <Romshelf's user folder> -b -C Dolphin.Display.Fullscreen=True -e <rom>`, using options confirmed from Dolphin's `--help` on the Deck.
  - `--filesystem=Ã¢ÂÂ¦:ro` gives the sandbox read access to wherever ROMs are kept (home or an SD card) for that run only.
- **Process runner:** waits for the emulator, so Steam's game tracking stays right. A stop signal to Romshelf (SIGTERM, SIGINT, SIGHUP, as when Steam ends the game) is passed on to the emulator, and Romshelf keeps waiting for it to exit. Its stderr still reaches the terminal, and the last 20 lines are kept for the log.
- **`launch.lock`** holds Romshelf's process id while a game runs (the self-updater waits for it) and is removed afterwards.
- **Launch log** at `~/.local/share/romshelf/logs/launch.log`: time, rom-id, emulator, duration and exit code per launch, plus the emulator's last lines when it failed.
- **The launch window** (UI guide section 8):
  - A game that is downloaded with its emulator ready starts with no window at all.
  - Otherwise `romshelf launch` opens a full-screen window: small cover, title, the step ("Downloading", "Setting up Dolphin") and a progress bar. It hides when the game starts, and exits with the emulator's code when the game ends.
  - On an error it shows the reason, **Try again** and **Open Romshelf**. B cancels and exits.
- **Play:** for a downloaded game with its emulator ready, **Play** is the first action in the game's actions dialog. It shows the step while starting, and the reason if it fails. When the game ends you're back in Romshelf.
- **`romshelf doctor --id <id>`** lists each step a launch would take, with the exact emulator command, and changes nothing.
- **Sharing the database:** it now waits up to 5 s for another process's write, so a launch from Steam works while Romshelf is open.
- **Fake mode:** "games" run for five seconds and exit normally.
- **Dolphin starts from your settings:** found before testing on the Deck. Romshelf's own Dolphin user folder was empty, so Dolphin would have started with default (keyboard) controls and graphics. EmuDeck's settings map the Deck (`SDL/0/Steam Virtual Gamepad`) and use Vulkan. Now Romshelf copies `Dolphin.ini`, `GFX.ini`, `GCPadNew.ini`, `GCKeyNew.ini` and `Hotkeys.ini` from your Dolphin settings into its own `Config/` the first time, reading yours only. After that its copy is its own, for the stage 7 controls editor to change. Every launch checks the session folder, so the Dolphin set up in stage 4 gets the copy too.

### Tests

17 launch tests (including that every launch prepares the session folder again), 1 more emulator test (the copied settings, with yours untouched and Romshelf's copy never replaced), and: RetroArch's and Dolphin's exact command lines; exit codes 0 and 3 returned unchanged; a ready game goes straight to the emulator with no window needed; download first; setup first; download then setup then launch; `--id` and `--rom` find the same game; an unknown id named, with nothing started; gone from Drive; no emulator, and an unrecognized file; backup after a failed exit with the save check and controls before the run; the launch lock held during the run and gone after; the launch log; `doctor` writing nothing, starting nothing and calling no Flatpak; two database connections sharing a file. Plus 5 UI tests for the launch window and Play.

### Choices made

- **One process per launch.** `romshelf launch` doesn't hand the game to an already open Romshelf; both can run at once, sharing the database. That keeps Steam's process tracking simple: the shortcut's own process waits for the game.
- **The launch window hides while the game runs**, so the emulator has the screen; it shows again only if something fails.
- **Saves are separate for now.** Games Romshelf starts save to Romshelf's own folders (Dolphin's user folder, RetroArch's `romshelf/saves`), not EmuDeck's on the SD card. So a save made through EmuDeck or ES-DE won't show in a game started from Romshelf, and the other way round. Stage 6 (save backup) decides how saves are kept and shared.
- **Quitting a game comes from the emulator's settings.** Today that's EmuDeck's hotkeys in the copied settings (View + Menu exits Dolphin). Stage 7 (controls) should give every game one quit combination that doesn't depend on another tool's settings.
- **Found on the Deck: passing a stop to a Flatpak emulator.** `flatpak run` becomes `bwrap`, which starts the sandbox (`bwrap Ã¢ÂÂ bwrap Ã¢ÂÂ dolphin-emu-wrapper Ã¢ÂÂ dolphin-emu`) and doesn't pass signals into it. So a stop sent to Romshelf's own child never reached Dolphin. Romshelf now sends SIGTERM to the programs at the end of its chain (the emulator), widens it to the whole chain after 10 s and forces it after 20 s. Tested on the Deck: Dolphin closed cleanly within a second ("A signal was receivedÃ¢ÂÂ¦"), and Romshelf exited 0. Also seen: ending an SSH session ends the programs it started but not Flatpak apps, which run in their own systemd scope. That only matters for testing over SSH.
- **`romshelf doctor` without `--id`** (the whole-setup check) is stage 10's.
- **Play isn't offered before downloading.** The dialog's first action is **Download to Deck**, or **Set up {emulator}**, until the game can start straight away. A Steam shortcut still does everything in one go.

### On-Deck checklist

- [x] Play from the Library starts the game full screen; quitting returns to Romshelf. 2026-10-19: Twilight Princess in Dolphin, with your controls (copied settings); quit with View + Menu (EmuDeck's Exit hotkey, from the copied Hotkeys.ini). Log: `gamecube-2260f1ec dolphin 1051s exit 0`.
- [x] From the Desktop Mode terminal, `~/Applications/romshelf.AppImage launch --id <id>` starts the same game (ids: `romshelf doctor --id <id>` explains one; your Twilight Princess is `gamecube-2260f1ec`). 2026-10-19: started Dolphin straight away with no window. Controls don't respond when started outside Steam: the copied Dolphin settings use the controller Steam creates for games it launches (`SDL/0/Steam Virtual Gamepad`). Expected; Steam shortcuts have it.
- [x] Add a non-Steam shortcut with target `~/Applications/romshelf.AppImage` and launch options `launch --id <id>`; it starts the game from Game Mode and returns to Steam on quit. 2026-10-19: you confirmed it on the Deck (Twilight Princess, controls working).
- [x] Launch a game that isn't downloaded from such a shortcut: the progress window appears, then the game. 2026-10-19: you confirmed it (Kirby, downloaded at 15:35 by the shortcut). Quitting with Steam button Ã¢ÂÂ Exit game left no launch log line: Steam ends Romshelf forcefully. Quitting in game with View + Menu (EmuDeck's RetroArch hotkey) logged `gba-8f031392 retroarch 53s exit 0`. So stage 6 can't rely on running after the game: it records an open session at start and backs up any session that wasn't closed the next time Romshelf runs.
- [x] The Steam overlay and the Deck's quick-access menu work while the game runs. 2026-10-19: the Steam button menu works in game.

## Stage 4: emulator registry and setup (branch `stage-04-emulator-setup`, on top of stage 3)

Status: **passed on the Deck 2026-10-19** (you counted it as passed; the "needs setting up" screen wasn't seen because Dolphin was already set up). Both Confirm items checked.

### Built

- **Registry:** SNES `snes9x`, GBA `mgba`, N64 `mupen64plus_next` in RetroArch; GameCube in Dolphin. `Registry::require` gives "Romshelf can't run PS2 games yet." for a system with no emulator.
- **Detect:** `flatpak info <id>` counts an install for this user *or* system-wide, so an emulator EmuDeck installed is used rather than installed twice.
- **Install:** `flatpak remote-add --user --if-not-exists flathub Ã¢ÂÂ¦`, then `flatpak install --user --noninteractive -y flathub <id>`, streaming Flatpak's output and reading the percentages for progress. A failure gives Flatpak's own message ("Couldn't install Dolphin: Ã¢ÂÂ¦ Try again.") and a later attempt starts cleanly.
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

17 emulator tests: every recognized extension resolves to one emulator or "can't run Ã¢ÂÂ¦ yet"; detecting installed and not installed (any installation); the exact install commands once, with progress 12 Ã¢ÂÂ 58 Ã¢ÂÂ 100; an installed emulator not installed again; an install failure with Flatpak's message, then a working retry; a missing core fetched and present ones left alone; a bad core download reported with nothing written; setup resuming at step two; the three statuses; session folders created while the user's `retroarch.cfg` and `Dolphin.ini` stay byte-identical; BIOS copied and missing ones named; different declared actions; listing with systems and status keeping the installed flag; nothing written to Drive; the simulated Flatpak. Plus the progress parser, `romshelf emulators` (list, set up, unknown name), CLI parsing, and 12 UI tests (Emulators screen rows, actions, progress, failure; the dialog's emulator line, functions, Set up first, setup progress and retry, unsupported system).

### Choices made

- **Installed by anyone counts.** The stage file says `flatpak info --user`; that would miss EmuDeck's system-wide installs and add a duplicate.
- **Romshelf's settings live inside each Flatpak's own data folder**, because the sandbox can always reach it. Nothing outside it is written.
- **Status has no "Update available" yet.** Flathub updates come through Discover or `flatpak update`. Noted for stage 9's Emulators screen.
- **The stage file calls this the "ROM page".** Until stage 9 builds it, the game's actions dialog carries its parts: the emulator line, setup and functions.
- `Error::Invalid` no longer prefixes messages with "invalid:", since many reach the user (stage 3's storage messages too).
- `romshelf emulators` is a new subcommand (CLAUDE.md updated).

### Confirm on the Deck

- **Where the RetroArch Flatpak keeps cores: confirmed 2026-10-19.** On this Deck (EmuDeck), `libretro_directory = "~/.var/app/org.libretro.RetroArch/config/retroarch/cores"`, and snes9x, mgba and mupen64plus_next are already there (93 cores in all). EmuDeck points RetroArch's saves, states and BIOS at the SD card's `Emulation/` folder. Romshelf's append-config is only used for games Romshelf starts, so EmuDeck's setup is unaffected.
- **Both emulators are installed** as user Flatpaks: `org.libretro.RetroArch` 1.19.1 and `org.DolphinEmu.dolphin-emu` 2407. The user has a `flathub` remote, and there's a system one too.
- **Found and fixed on the Deck:** the first stage 4 build said both were "Not installed". Running from the AppImage, `flatpak` inherited the AppImage's `LD_LIBRARY_PATH` and failed with `symbol lookup error: /usr/lib/libmalcontent-0.so.0: undefined symbol: g_task_set_static_name`. The full list of variables was read from a running Romshelf. `process::host_command` now removes them, and the AppImage's folders from `XDG_DATA_DIRS`, for every host program: Flatpak, emulators and the browser.
- **Dolphin's user folder option: confirmed 2026-10-19.** Dolphin 2407's `--help` (run with `QT_QPA_PLATFORM=offscreen`, since it needs a display otherwise) lists `-u USER, --user=USER  User folder path`, `-e <file>, --exec=<file>` and `-b, --batch` ("Run Dolphin without the user interface (Requires --execÃ¢ÂÂ¦)"). These match what stage 5 will use. The Flatpak's command is `dolphin-emu-wrapper`.
- **After the fix:** `romshelf emulators` on the Deck shows both as "Setup incomplete": installed by EmuDeck, with Romshelf's setup not yet run. Correct.
- That the buildbot's generic Linux cores load in the Flatpak RetroArch (they're what its own core updater downloads). Not needed on this Deck, since the cores are already present.

### On-Deck checklist

- [x] With Dolphin not installed, open a GameCube game's actions: it says "Needs Dolphin" and offers **Set up Dolphin** first (screen 2b). Not seen on the Deck: Dolphin (installed by EmuDeck) was already set up through the Emulators screen. Covered by the UI tests; counted as passed at your request.
- [x] Press **Set up Dolphin**: it installs with visible progress and the game then shows "Runs in Dolphin" (screen 2). 2026-10-19: setup ran from the Emulators screen; Twilight Princess (`.rvz`) shows "Runs in Dolphin". No install needed, since EmuDeck had installed Dolphin.
- [x] Do the same for RetroArch (or **Repair** it if EmuDeck already installed it) and confirm the snes9x, mgba and mupen64plus_next cores are present. 2026-10-19: set up; all three cores were already present.
- [x] The Emulators screen shows both as installed. 2026-10-19: `romshelf emulators` and the Emulators screen show both Installed.

## Stage 3: download, remove and storage (branch `stage-03-download-delete`)

Status: **on-Deck checks passed 2026-10-19.**

### Built

- **Storage root:** ROMs go to `<root>/roms/<system>/<file>`. The default root is `~/Romshelf` (see choices). `romshelf storage` shows it and the free space; `romshelf storage --set <folder>` moves every downloaded ROM there (checking all destinations first, refusing to overwrite a different file, never while downloads run) and saves it in `~/.config/romshelf/settings.json`.
- **Downloads** (`transfer/mod.rs`):
  - Stream into `<file>.part` instead of memory, so GameCube images over 1 GB are fine. The HTTP seam has a streaming `send_to`, and the Drive trait has `download_to`.
  - Resume from the `.part` length with a `Range` request. The Google client retries only before the first byte arrives; after that the downloader resumes. A dropped connection is retried after 2 s, doubling to 30 s, 12 times in all (about four minutes).
  - Check size and the MD5 Drive reports, then rename into place. A mismatch discards the file and says so.
  - Check free space first, with 256 MB headroom, and refuse with the shortfall named ("Free up 1.2 GB moreÃ¢ÂÂ¦") before writing anything.
  - The queue lives in `transfer_queue` (queued, running, paused, failed, done) and runs one download at a time. A download running when Romshelf closed resumes on the next start. Cancel stops the running one (removing its `.part`) or takes a waiting one off the queue.
  - Progress events (bytes, total, rate) at most four a second; the tile's percentage is kept in the database too.
- **Locations on start** (`storage::reconcile`): an on-Deck ROM whose file is missing or the wrong size becomes "In Drive only", a matching file found on disk becomes "On Deck", and a download with nothing queued is cleared. If the storage root and its parent are both missing (the SD card is out), nothing is changed and the Library shows "The storage at Ã¢ÂÂ¦ isn't availableÃ¢ÂÂ¦"; downloads refuse with the same message.
- **Remove from Deck** (`transfer::remove_from_deck`): deletes the local file and any `.part` to free space; the game stays in Drive and shows "In Drive only". A game that is downloading can't be removed until the download is cancelled. **There is no way to delete from Drive** (see choices).
- **Commands:** `romshelf download <id>` (progress on stderr), `romshelf remove <id>`, `romshelf storage [--set <folder>]`.
- **Interface:**
  - A on a tile opens the game's actions: metadata in mono, "Download to Deck" / "Download again", a progress bar with percentage, rate and time left plus "Cancel download", "Remove from DeckÃ¢ÂÂ¦" (only for games on the Deck), and the last error.
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
  - Rows are written only when something changed, and `.romshelf/index.json` (file id Ã¢ÂÂ rom-id and chosen name) only when it changed. The sync steps past its own index write, so the next sync with nothing new writes nothing.
  - A second device adopts ids and chosen names from `index.json`. An index from a newer Romshelf stops sync with "Please update Romshelf".
- Display names: the stem with `(Ã¢ÂÂ¦)`/`[Ã¢ÂÂ¦]` tags removed and `_`/`-` as spaces, title-cased only when the stem was all lower case. A chosen name (`sync::set_display_name`) wins and follows renames; the interface to set one comes later.
- Database migration 2: `name_overrides`. Also `rom_by_drive_id`, save-backup rows (for the "saves untouched" test) and `total_changes`.
- Google client: HTTP 400 (such as an expired change token) is now `Error::Invalid`, so sync falls back to a full scan.
- `romshelf sync` prints one line, such as "Synced 12 files: 2 new, 1 not recognized."
- Interface: the Library syncs when it opens, and when the sync line in the top band is selected. The sync line reads "Google Drive synced 2 min ago", "Syncing with Google DriveÃ¢ÂÂ¦", "Offline" or "Sync failed", with the reason shown above the grid.
  - System tabs (All, SNES, GBA, N64, PS2, GameCube) change with L1/R1 (Page Up/Page Down on a keyboard). The header counts "{n} ROMs in Drive ÃÂ· {n} on Deck" for the tab.
  - Tiles show recognized files; a "Not recognized" group lists the rest with their folder.
  - A folder strip shows the Drive path, and an empty library offers "Sync now".
  - Focus goes to the first tile on arrival and on a tab change, and stays put when a sync reloads the library.
- Fake mode puts the fixture ROMs in its fake Drive, so it syncs for real and finds nothing to change.

### Tests

21 sync tests: extension table (upper case too, `.txt` unrecognized, GameCube `.zip` unrecognized); 14 display-name cases; 500 files in one full scan; a second sync with no database or Drive writes; new upload; unrecognized files; only real files in system folders; rename; chosen name surviving a rename and being cleared; move between systems; trash and restore with save rows untouched; restore when the file is on the Deck; permanent delete and moving out; full scan marking missing files gone; second device adopting ids (including a moved file's) and names; index written only on change; rejected token; new Drive folder; id collision; newer index; summary wording. Plus Google 400 Ã¢ÂÂ invalid, `Connection::sync`, fake-mode sync, and 11 UI tests (tabs and L1/R1 wrap, counts, singular, sync wording and ages, tiles vs not recognized, tab filter, empty tab, Sync now, disabled while syncing, failure message).

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

17 updater tests (18 on Linux): newer release installed with the previous kept and the file executable; same and older ignored with no download; bad checksum; forged signature rejected before the AppImage is fetched; a signed manifest replayed under a newer tag; a non-AppImage download; an interrupted download leaves the current version and the check is retried next start; once a day unless forced; skipped while a launch runs; a stale lock from a crashed launch ignored (Linux); rollback restores the old file and skips the bad version but installs a later one; rollback with nothing kept; token sent; private repository without a token explains `--set-token`; release missing a file; no key Ã¢ÂÂ off; an OpenSSL-made signature (as CI makes them) verifies. Plus CLI parsing and four `romshelf update` command tests, and two UI tests for the notice.

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
- `drive/google.rs`: `DriveClient` over Drive v3: about, list (paged, trash excluded, query escaped), get, create folder, multipart create, media update, rename, move, trash (never delete), ranged download (416 at the end Ã¢ÂÂ empty), start page token, paged change feed. Up to 5 attempts with exponential backoff on 429, 5xx, rate-limit 403s and network errors; other errors surface Google's message.
- `drive/scaffold.rs`: `Emulation/{roms,saves,states,bios,.romshelf}` plus `roms/<system>` and `bios/<system>` under any parent. Lookup is parent id + name (folders only), one listing per parent, idempotent; `.romshelf/settings.json` (schema 1) written or updated in place; a newer schema stops setup with "Please update Romshelf" and is left untouched.
- `settings.rs`: local `~/.config/romshelf/settings.json` (account, Drive location, folder ids), atomic writes, newer schema refused.
- `tokens.rs`: `KeyringTokenStore` (Secret Service on Linux) behind `FallbackTokenStore`, which falls back silently to the 0600 `FileTokenStore`.
- `connection.rs`: what the CLI and UI share: status, set client (a new client id signs out), login (records the account; a different account forgets the old folder), logout (keeps the folder), folder listing, setup.
- CLI: `auth set-client --file <json>` or `--client-id` (secret prompted), `auth login` (prints the address if no browser opens), `auth logout`, `auth status` (exit 0 when signed in; it also counts the files in each `roms/<system>/` so the scope check can be done before stage 2).
- UI: first run per guide ÃÂ§6: welcome, sign in (waiting, cancel, "Can't see a browser?", Desktop Mode steps, set-client instructions when no client is stored), folder browser (A enters, B goes up, path in mono, note when an `Emulation` folder already exists), progress, done with the tree. The library's empty state shows the chosen folder.
- `ROMSHELF_FAKE=1` starts signed in on a fake Drive; add `ROMSHELF_FAKE_FIRST_RUN=1` to walk the first run against it. In a plain browser, `?first-run` previews it.
- `docs/google-setup.md`: Cloud project, Drive API, consent screen, Testing (weekly sign-in, add yourself as a test user) or In production, Desktop client, `set-client` over SSH.

### Tests

90 Rust tests, 34 front-end tests. New: PKCE RFC 7636 vector and generated verifiers; consent URL; client JSON (desktop, web rejected, malformed); loopback right state, wrong state, refused consent, timeout, cancel; expired token refreshes exactly once then retries; 401 refreshes once and retries, a second 401 is not refreshed; revoked refresh token signs out; 429 then 200; five 500s fail with the last error and no sixth request; network errors and quota 403s retried, permission 403s not; 404 Ã¢ÂÂ not found; paging for list and changes; query escaping; multipart upload; media update; Range downloads; never `DELETE`; full login against the loopback; login without Drive scope, without a client, without a browser; logout online and offline; scaffold on empty, twice, partial, same names elsewhere, a file named like a folder, under a chosen folder, progress; settings round-trip, update in place, newer schema refused and untouched, damaged; local settings; keyring fallback; connection first run, folder listing, logout keeps folder, account switch, client switch; CLI status/login/set-client/logout; fake-mode state; first-run screens (each step, waiting, no browser, errors, set-client, folder navigation and B, create, failure, returning account).

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
- Settings screen (account, "Sign out", "Change locationÃ¢ÂÂ¦"): stage 9.

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
- RetroArch cores (`snes9x`, `mgba`, `mupen64plus_next`) and Dolphin's `-b -e` flags are placeholders. **Confirm** in stages 4Ã¢ÂÂ5.
- AppImage built on Ubuntu 22.04 for a low glibc floor.

### Deferred

- Real `DriveClient` and keyring `TokenStore` (stage 1).
- Spike B and the input-path decision need the Deck (below).

### On-Deck checklist

Install (Desktop Mode, Konsole), after downloading `romshelf-appimage.zip` from the CI run into `~/Downloads`:

```
mkdir -p ~/Applications && cd ~/Downloads && unzip -o romshelf-appimage.zip && mv romshelf.AppImage ~/Applications/ && chmod +x ~/Applications/romshelf.AppImage
```

Add to Steam: Steam Ã¢ÂÂ Games Ã¢ÂÂ Add a Non-Steam Game Ã¢ÂÂ Browse Ã¢ÂÂ `~/Applications/romshelf.AppImage`. To see the fixture library, set the shortcut's launch options to `ROMSHELF_FAKE=1 %command%`.

- [x] The AppImage starts from Desktop Mode (double-click, or `~/Applications/romshelf.AppImage` in Konsole). 2026-10-18: started over SSH with `ROMSHELF_FAKE=1`; full screen, eight fixture tiles.
- [x] Added to Steam, it starts in Game Mode, fills the screen, and returns to Steam on exit ("Quit Romshelf").
- [x] Spike A: on "Open gamepad test", D-pad, face buttons, bumpers and sticks all register. Note which path (browser API, `gilrs`, or both) shows them and which one the header says navigation uses.
- [x] Fonts render as in the screenshots (`docs/kit/ui/screens/01-library.png`). 2026-10-18: sans and mono faces match in a Desktop Mode screenshot.

Results:

- Spike A (input path): 2026-10-18: D-pad, face buttons, bumpers and sticks register on both the browser Gamepad API and `gilrs`. Navigation locks to whichever source reports first, so either works; no change needed.
- Spike B (Game Mode): 2026-10-18: starts from a non-Steam shortcut, fills the screen, and "Quit Romshelf" returns to Steam.
- `WEBKIT_DISABLE_DMABUF_RENDERER` needed?: Not on this Deck: with it set to `0` in Game Mode the UI still renders. Kept as a harmless default.
