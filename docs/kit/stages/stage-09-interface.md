# Stage 9: interface completion

**Goal:** every screen matches the UI guide, every state is designed, and the whole app is comfortable with the gamepad alone.

Earlier stages each built the part of the interface they needed. This stage closes the gaps and raises the finish.

**Depends on:** Stages 0 to 8.
**Read first:** the whole UI guide; `ui/reference-html/`; `LINKS.md` → Tauri (testing).

## Implementation steps

1. Visual audit: open each built screen beside its screenshot at 1280×800. Fix differences in spacing, type size, colour and radius. The reference HTML is the source of truth for values.
2. Focus system: one focus ring style; spatial navigation with the D-pad and left stick (nearest focusable in the pressed direction within the current region, then across regions); focus memory per screen; focus is never lost after a list changes.
3. Global bindings from UI guide section 4, and a footer hint bar that always reflects what the buttons do for the focused element.
4. Text entry: fields open the Steam on-screen keyboard. **Confirm** how to trigger it from a non-Steam app under gamescope; fall back to a simple in-app keyboard if it cannot be triggered.
5. Screens and dialogs not yet built, from UI guide sections 6 and 7: Emulators, Settings, first-run, delete dialog, restore list, conflict dialog, restart-Steam prompt, launch window.
6. States for every screen: loading (skeletons, not spinners, for grids), empty (what to do next), offline (what still works), error (what happened and one action).
7. Library at scale: virtualized grid; smooth with 2,000 ROMs.
8. Background activity: one transfers indicator in the top bar opening a list of downloads and queued backups.
9. Accessibility: text contrast at least 4.5:1; no information by colour alone; all targets at least 44 px; reduced-motion setting respected.
10. End-to-end tests in fake mode with WebDriver.

## Automated tests

- Component tests for every screen in each state (loading, empty, populated, offline, error).
- Focus: from any focusable element, each direction either moves focus or does nothing; it never leaves focus on a hidden element. Run as a generated test over all screens.
- Footer hints match the focused element's available actions.
- End-to-end, fake mode, simulated gamepad only:
  1. First run → sign in (faked) → choose location → Library.
  2. Library → switch system with R1 → open a ROM → Download → Play (fake emulator) → back.
  3. ROM page → Controls → rebind one button → Save → card shows one difference.
  4. ROM page → Steam shortcut → rename → Save → restart prompt appears once.
  5. ROM page (emulator missing) → Set up → page becomes ready.
  6. Delete from this Deck → tile shows "In Drive only".
- Visual regression: screenshots of the five reference screens in fake mode compared with `ui/screens/*.png` within a small tolerance.
- Performance: Library with 2,000 fixture ROMs scrolls without dropped frames in the test harness's measurement.

## On-Deck checks

- [ ] Every action in the app can be reached and completed without touching the screen.
- [ ] Text is readable at arm's length on the Deck's screen.
- [ ] The on-screen keyboard opens for the shortcut name field and the result is saved.
- [ ] Focus is always visible, including after dialogs close.
- [ ] Offline: the Library opens, downloaded games play, and Drive-only actions explain why they are unavailable.

## Done when

Automated tests pass, the visual regression is within tolerance, and a person can use the whole app on the Deck with the gamepad alone.

## Pitfalls

- Do not add hover-only affordances; there is no pointer in normal use.
- Touch must still work for every control, since the Deck has a touchscreen.
