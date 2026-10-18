# Stage 7: controls

**Goal:** each emulator has a default layout, each game can override individual buttons, and the merged layout is applied at launch.

**Depends on:** Stage 5 (Stage 6 not required).
**Read first:** UI guide section 5.4 (Controls); `LINKS.md` → Emulators (overrides), Steam Input.

## Model

- **Inputs:** what the emulator can see. Under Steam Input's default layout that is a standard gamepad: D-pad, two sticks, A B X Y, L1 R1, L2 R2, stick clicks, Menu, View.
- **Targets:** per adapter and system: game buttons (for GBA: D-pad, A, B, L, R, Start, Select) and emulator hotkeys (fast forward, rewind, save state, load state, open menu).
- **Layout:** a map from input to target. An emulator default is a complete layout. A game profile is a set of differences: input → target, or input → explicitly unbound.
- **Merge:** start from the default, apply the game's differences.

## Decision to make first: back grips and trackpads

The approved Controls screen shows L4, R4, L5 and R5. Emulators cannot see those inputs directly; they exist only inside Steam Input, which decides what they send. Before building the editor, run this spike and report the result:

- **Recommended approach:** ship one Steam Input layout for Romshelf shortcuts that leaves the standard pad alone and makes the four back grips send four otherwise unused keyboard keys. Romshelf then treats those keys as four more inputs and binds emulator hotkeys to them per game. **Confirm** that a layout can be supplied as a file and applied to shortcuts without the user configuring each one by hand.
- **Fallback:** version one supports only the standard gamepad inputs, with hotkeys on button combinations (for example View plus R1). The back-grip rows are hidden.

Do not proceed past step 3 until the human has chosen.

## Implementation steps

1. `controls/model.rs`: inputs, targets, layouts, differences; serialization.
2. `controls/merge.rs`: the merge function and a function that computes the difference between a full layout and a default.
3. Built-in defaults per adapter and system.
4. Writers: `write_controls(layout, rom)` renders into the session configuration.
   - RetroArch: input binds in the appended config for game buttons and hotkeys. **Confirm** key names against a config written by RetroArch itself and keep that file as a fixture.
   - Dolphin: a controller profile in the Romshelf-owned user directory, selected for the game. **Confirm** the file format the same way.
5. Pipeline: fill the Stage 5 stub (step 5).
6. Storage: defaults and game profiles in the database and mirrored to Drive `.romshelf/controls/`, so they follow the user to another device.
7. Editor (screen 3): scope switch between "{emulator} default" and "{game} only"; rebind by pressing A on a row and then the new input; clear; return a row to default; reset the game; promote the game's layout to the default (with a confirmation that names how many games will change).
8. ROM page Controls card: mode, count of differences, the differing rows.
9. Setup step two ("Set default controls") applies the built-in default and offers the editor.

## Automated tests

- Merge: override wins; an explicitly unbound input stays unbound; untouched inputs follow the default.
- Change the default after a game has overrides: non-overridden inputs change, overridden ones do not.
- Difference then merge returns the original layout (property test over random layouts).
- Promote to default: afterwards the game has zero differences and other games' effective layouts are unchanged except where they had no override.
- Reset game: zero differences.
- Two inputs bound to the same target is allowed; one input bound to two targets is rejected.
- Golden files: a known layout produces exactly the expected RetroArch config fragment and Dolphin profile.
- Round trip through Drive mirror on a second fake device.
- Front end: rebind flow driven by simulated input; row tags read "Default" or "This game" correctly.

## On-Deck checks

- [ ] Default layout works in a GBA game and a GameCube game with no editing.
- [ ] Swap A and B for one game only: swapped there, normal in another game on the same emulator.
- [ ] Bind fast forward for one game: it works in that game.
- [ ] Change the emulator default: both games follow, except the overridden buttons.
- [ ] If the back-grip approach was chosen: L4 triggers its bound hotkey in game.

## Done when

Tests pass, the spike outcome is recorded, and per-game differences demonstrably apply on the device.

## Pitfalls

- The player can also change the Steam Input layout for a shortcut in Steam's own UI. Romshelf's mapping sits beneath that. Say so in the editor's help text.
- Rebinding must have a way out that needs no working binding: holding B for one second cancels.
