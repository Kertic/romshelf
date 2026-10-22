/** The self-test screen: automatic checks, the interface tour, then each guided step. */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LaunchUpdate, LibraryEntry, SelfTestCheck, SelfTestProbe } from "./api";
import type { KeyHandler } from "./Controls";
import { ButtonsStep, ControlsStep, OfflineStep, SelfTest, report, timing } from "./SelfTest";

const mock = vi.hoisted(() => ({
  probe: {
    online: true,
    pending_backups: 0,
    storage_available: true,
    storage_removable: false,
    storage_root: "/home/deck/Romshelf",
    launches: 0,
  } as SelfTestProbe,
  saved: [] as string[],
  launch: null as ((u: LaunchUpdate) => void) | null,
  played: [] as string[],
  flushed: 0,
  runFails: false,
}));

vi.mock("./api", async (importOriginal) => {
  const real = await importOriginal<typeof import("./api")>();
  return {
    ...real,
    selftestRun: async (on: (c: SelfTestCheck) => void) => {
      if (mock.runFails) throw new Error("Drive went away");
      const c: SelfTestCheck = { group: "Setup", name: "Google account", outcome: "pass", detail: "signed in", fix: null };
      on(c);
      return [c];
    },
    selftestProbe: async () => ({ ...mock.probe }),
    selftestSave: async (text: string) => {
      mock.saved.push(text);
      return "/home/deck/.local/share/romshelf/logs/selftest/latest.txt";
    },
    selftestFlush: async () => {
      mock.flushed++;
      mock.probe.pending_backups = 0;
    },
    library: async () => [],
    play: async (id: string) => {
      mock.played.push(id);
    },
    onLaunchUpdate: async (h: (u: LaunchUpdate) => void) => {
      mock.launch = h;
      return () => {};
    },
  };
});

beforeEach(() => {
  timing.pollMs = 5;
  timing.buttonsSeconds = 45;
  timing.secondMs = 1000;
  mock.probe = { ...mock.probe, online: true, pending_backups: 0, storage_available: true, storage_removable: false };
  mock.saved = [];
  mock.played = [];
  mock.flushed = 0;
  mock.runFails = false;
});
afterEach(cleanup);

const settle = (ms = 30) =>
  act(async () => {
    await new Promise((r) => setTimeout(r, ms));
  });
const button = (name: string | RegExp) => screen.getByRole("button", { name });
const heading = () => document.querySelector("h2")?.textContent ?? "";

const GAME = {
  rom: { id: "gba-00000001", display_name: "Kirby", location: { state: "on_deck" } },
  emulator: "RetroArch",
  emulator_ready: true,
  control_differences: 2,
} as unknown as LibraryEntry;

function start(entries: LibraryEntry[] | null = null) {
  let keys: KeyHandler | null = null;
  const copy = vi.fn(async () => {});
  const onBack = vi.fn();
  const tour = vi.fn(async (): Promise<SelfTestCheck[]> => [
    { group: "Interface", name: "Library", outcome: "pass", detail: "12 controls, all reachable", fix: null },
  ]);
  render(
    <SelfTest
      entries={entries}
      version="0.1.3"
      setBack={() => {}}
      onBack={onBack}
      registerKeys={(h) => {
        keys = h;
      }}
      tour={tour}
      copy={copy}
    />,
  );
  return { press: (b: string) => act(() => void keys?.(b as never, "pad")), copy, tour, onBack };
}

describe("self-test", () => {
  it("runs the checks and the tour, then every guided step, saving as it goes", async () => {
    const t = start();
    await settle();
    expect(t.tour).toHaveBeenCalledOnce();
    expect(screen.getByText(/Setup \/ Google account/)).toBeTruthy();
    expect(screen.getByText(/Interface \/ Library/)).toBeTruthy();
    expect(heading()).toBe("Step 1 of 6: Buttons");

    for (const b of ["a", "b", "x", "y", "l1", "r1", "l4", "r4", "l5", "r5"]) t.press(b);
    await settle();
    expect(heading()).toBe("Step 2 of 6: On-screen keyboard");

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "OK" } });
    await settle();
    expect(heading()).toBe("Step 3 of 6: Readable at arm's length");
    fireEvent.click(button("I can read it"));
    await settle();
    // No game with controls of its own: skipped by itself.
    expect(heading()).toBe("Step 5 of 6: Airplane mode");
    fireEvent.click(button("Skip"));
    await settle();
    // ROMs on internal storage: the SD card step is skipped by itself.
    await settle();
    expect(screen.getByText(/All done/)).toBeTruthy();

    const last = mock.saved.at(-1)!;
    expect(last).toContain("pass  Guided / Buttons: Every button");
    expect(last).toContain("pass  Guided / On-screen keyboard");
    expect(last).toContain("pass  Guided / Readable at arm's length");
    expect(last).toContain("skip  Guided / A game's own controls");
    expect(last).toContain("skip  Guided / Airplane mode");
    expect(last).toContain("skip  Guided / SD card out and back: ROMs are on this Deck's own storage");
    expect(last).toContain("pass  Interface / Library");
    fireEvent.click(button("Copy report"));
    await settle();
    expect(t.copy).toHaveBeenCalledWith(last);
  });

  it("buttons are swallowed while it listens, and missing grips are explained", async () => {
    timing.buttonsSeconds = 1;
    timing.secondMs = 5;
    let keys: KeyHandler | null = null;
    const done = vi.fn();
    render(<ButtonsStep registerKeys={(h) => (keys = h)} onDone={done} />);
    for (const b of ["a", "b", "x", "y", "l1", "r1"]) {
      let used = false;
      act(() => void (used = keys?.(b as never, "pad") ?? false));
      expect(used).toBe(true);
    }
    expect(screen.getByLabelText("A, pressed")).toBeTruthy();
    expect(screen.getByLabelText("L4")).toBeTruthy();
    await settle(60);
    expect(done).toHaveBeenCalledOnce();
    const [outcome, detail] = done.mock.calls[0]!;
    expect(outcome).toBe("warn");
    expect(detail).toContain("Not seen: L4, R4, L5, R5.");
    expect(detail).toContain("Steam Input layout");
  });

  it("airplane mode: waits for offline, plays, sees the backup owed, then uploaded", async () => {
    const done = vi.fn();
    render(<OfflineStep entry={GAME} onDone={done} />);
    await settle();
    expect(screen.getByText(/Turn on airplane mode/)).toBeTruthy();
    // It shows that it's looking, and what it saw.
    expect(screen.getByText(/Google still answers \(checked \d/)).toBeTruthy();
    mock.probe.online = false;
    await settle();
    fireEvent.click(button("Play Kirby"));
    await settle();
    expect(mock.played).toEqual(["gba-00000001"]);
    mock.probe.pending_backups = 1;
    act(() => mock.launch?.({ rom_id: "gba-00000001", kind: "finished", code: 0 } as LaunchUpdate));
    await settle();
    expect(screen.getByText(/turn airplane mode off/)).toBeTruthy();
    mock.probe.online = true;
    await settle(60);
    expect(mock.flushed).toBe(1);
    expect(done).toHaveBeenCalledOnce();
    const [outcome, detail] = done.mock.calls[0]!;
    expect(outcome).toBe("pass");
    for (const part of [
      "saw the network go",
      "The Library opened offline",
      "Kirby ended with exit 0",
      "1 save backup waiting",
      "The network came back",
      "reached Drive",
    ]) {
      expect(detail).toContain(part);
    }
  });

  it("a stopped automatic run is recorded and the guided steps still start", async () => {
    mock.runFails = true;
    start();
    await settle();
    expect(screen.getByText(/Self-test \/ Automatic checks/)).toBeTruthy();
    expect(heading()).toBe("Step 1 of 6: Buttons");
  });

  it("writes the same report form as the headless run", () => {
    const text = report(
      [{ group: "Drive", name: "Round trip", outcome: "fail", detail: "Romshelf couldn't reach Google.", fix: "Check the connection." }],
      "0.1.3",
      new Date("2026-10-21T12:00:00Z"),
    );
    expect(text).toContain("Romshelf 0.1.3 self-test, 2026-10-21T12:00:00.000Z");
    expect(text).toContain("FAIL  Drive / Round trip: Romshelf couldn't reach Google.\n      Fix: Check the connection.");
    expect(text).toContain("0 passed, 0 to look at, 1 failed, 0 skipped");
  });
});

describe("a game's own controls", () => {
  it("with no game's own differences, offers a downloaded game on its emulator's default", async () => {
    timing.pollMs = 5;
    const onDefault = { ...GAME, control_differences: 0 } as LibraryEntry;
    render(<ControlsStep entry={onDefault} onDone={() => {}} />);
    expect(screen.getByText(/Kirby uses RetroArch's default layout/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Play Kirby" })).toBeTruthy();
  });
});
