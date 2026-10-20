import { act, cleanup, render } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { controlsView, emulators, library, settingsView, steamShortcut, type LibraryEntry } from "./api";
import { Controls } from "./Controls";
import { Emulators } from "./Emulators";
import { focusables, moveFocus, reachable, type Direction } from "./focus";
import { Home } from "./Home";
import { RomPage } from "./RomActions";
import { Settings } from "./Settings";
import { SteamShortcut } from "./SteamShortcut";

afterEach(cleanup);

// jsdom has no layout: give every element a box by its place in the page, four to a row, so
// "nearest in the pressed direction" means something.
const original = Element.prototype.getBoundingClientRect;
beforeAll(() => {
  Element.prototype.getBoundingClientRect = function (this: Element) {
    const all = [...document.querySelectorAll("*")];
    const i = all.indexOf(this);
    const x = (i % 4) * 200;
    const y = Math.floor(i / 4) * 100;
    return { x, y, left: x, top: y, right: x + 150, bottom: y + 60, width: 150, height: 60, toJSON() {} } as DOMRect;
  };
});
afterAll(() => {
  Element.prototype.getBoundingClientRect = original;
});

const LIB = await library();
const READY = LIB.find((e) => e.emulator_ready && e.rom.location.state === "on_deck")!;
const noop = () => {};
const asyncNoop = async () => {};

const screens: [string, () => ReactElement][] = [
  [
    "Library",
    () => <Home info={null} entries={LIB} error={null} driveFolder="My Drive / Emulation" onOpenSpike={noop} onQuit={noop} />,
  ],
  [
    "ROM page",
    () => <RomPage entry={READY} onDownload={noop} onCancel={noop} onRemove={asyncNoop} onClose={noop} setBack={noop} onPlay={noop} onEditControls={noop} onSteamShortcut={noop} />,
  ],
  [
    "ROM page, emulator missing",
    () => (
      <RomPage
        entry={{ ...READY, emulator_ready: false, emulator_installed: false } as LibraryEntry}
        onDownload={noop}
        onCancel={noop}
        onRemove={asyncNoop}
        onClose={noop}
        setBack={noop}
        onSetupEmulator={noop}
      />
    ),
  ],
  [
    "Settings",
    () => (
      <Settings
        view={SETTINGS}
        onBack={noop}
        setBack={noop}
        reload={noop}
        actions={{
          signOut: asyncNoop,
          changeDriveFolder: noop,
          changeStorage: asyncNoop,
          setSaveVersions: asyncNoop,
          setAutoAdd: asyncNoop,
          checkForUpdates: asyncNoop,
          diagnostics: async () => "",
          copy: asyncNoop,
        }}
      />
    ),
  ],
  [
    "Steam shortcut",
    () => (
      <SteamShortcut
        view={SHORTCUT}
        setBack={noop}
        onBack={noop}
        onSave={async () => ({ added: 0, updated: 0, removed: 0, artwork: 0, restart_needed: false })}
        onAutoAdd={asyncNoop}
        onArtwork={asyncNoop}
        loadImages={async () => []}
      />
    ),
  ],
  ["Emulators", () => <Emulators list={EMULATORS} error={null} setups={{}} onSetup={noop} onEditControls={noop} onBack={noop} />],
  [
    "Controls",
    () => (
      <Controls
        view={CONTROLS}
        held={[]}
        registerKeys={noop}
        setBack={noop}
        onBack={noop}
        onSave={asyncNoop}
        onPromote={asyncNoop}
        promoteCount={async () => 0}
      />
    ),
  ],
];

const SETTINGS = await settingsView();
const SHORTCUT = await steamShortcut("gba-00000001");
const EMULATORS = await emulators();
const CONTROLS = await controlsView("retroarch", "gba", "gba-00000001");
const DIRECTIONS: Direction[] = ["up", "down", "left", "right"];

describe("focus on every screen", () => {
  for (const [name, make] of screens) {
    it(`${name}: each direction moves to something reachable, or stays`, () => {
      render(make());
      const all = focusables();
      expect(all.length).toBeGreaterThan(0);
      for (const from of all.filter((el) => el.isConnected)) {
        for (const dir of DIRECTIONS) {
          act(() => from.focus());
          act(() => void moveFocus(dir));
          const now = document.activeElement as HTMLElement;
          // Focusing a tile redraws the drawer, so compare with what can be reached now.
          const ok = now === from || (focusables().includes(now) && reachable(now));
          expect(ok, `${name}: ${dir} from "${from.textContent}"`).toBe(true);
        }
      }
    });

    it(`${name}: every focusable says what A does`, () => {
      render(make());
      for (const el of focusables()) {
        const hint = el.dataset.hint ?? (el.tagName === "INPUT" ? "edit" : "");
        expect(hint, `"${el.textContent}" has no hint`).not.toBe("");
      }
    });
  }

  it("a dialog keeps focus inside it: the page behind can't be reached", async () => {
    render(
      <RomPage entry={READY} onDownload={noop} onCancel={noop} onRemove={asyncNoop} onClose={noop} setBack={noop} onPlay={noop} startWith="remove" />,
    );
    await act(async () => {
      await new Promise((r) => setTimeout(r));
    });
    const dialog = document.querySelector("[data-dialog]")!;
    expect(focusables().every((el) => dialog.contains(el))).toBe(true);
    for (const dir of DIRECTIONS) {
      act(() => void moveFocus(dir));
      expect(dialog.contains(document.activeElement)).toBe(true);
    }
  });
});

vi.mock("@tauri-apps/api/core", async (importOriginal) => {
  const real = await importOriginal<typeof import("@tauri-apps/api/core")>();
  return { ...real, isTauri: () => false };
});
