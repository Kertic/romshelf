import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LibraryEntry, LocationState, SyncStatus } from "./api";
import { Home } from "./Home";
import { ago, counts, inTab, nextTab, syncLine, visibleRows } from "./library";

afterEach(cleanup);

function entry(name: string, system: string, label: string, location: LocationState, recognized = true): LibraryEntry {
  return {
    rom: {
      id: `${system}-${name.length.toString(16).padStart(8, "0")}${name}`,
      drive_file_id: name,
      system,
      file_name: name,
      display_name: name.replace(/\.\w+$/, ""),
      size: 1,
      checksum: null,
      location,
      recognized,
    },
    system_label: label,
    system_colour: "#1F6B62",
    emulator: "RetroArch",
    emulator_id: "retroarch",
    core: null,
    emulator_installed: true,
    emulator_ready: true,
    actions: [],
    in_steam: false,
    save: { last_synced_hash: null, last_backup_at: null, last_backup_device: null, pending: false, last_error: null },
    keeps_states: false,
    control_differences: 0,
  };
}

const LIB: LibraryEntry[] = [
  entry("Kirby.gba", "gba", "GBA", { state: "drive_only" }),
  entry("Tidewalker.gba", "gba", "GBA", { state: "on_deck" }),
  entry("Ember.sfc", "snes", "SNES", { state: "on_deck" }),
  entry("Shadow the Hedgehog (USA).zip", "gamecube", "GameCube", { state: "drive_only" }, false),
];

const NOW = Date.UTC(2026, 9, 19, 12, 0, 0);
const synced = (agoSecs: number): SyncStatus => ({
  state: "synced",
  last_synced_at: Math.floor(NOW / 1000) - agoSecs,
  message: "Synced 4 files. Nothing changed.",
});

describe("library helpers", () => {
  it("L1 and R1 cycle through the tabs and wrap", () => {
    expect(nextTab("all", 1)).toBe("snes");
    expect(nextTab("all", -1)).toBe("gamecube");
    expect(nextTab("gamecube", 1)).toBe("all");
    expect(nextTab("unknown", 1)).toBe("snes");
  });

  it("filters by system and counts ROMs and what is on the Deck", () => {
    expect(inTab(LIB, "all")).toHaveLength(4);
    expect(inTab(LIB, "gba").map((e) => e.rom.file_name)).toEqual(["Kirby.gba", "Tidewalker.gba"]);
    expect(counts(LIB)).toEqual({ inDrive: 3, onDeck: 2 });
    expect(counts(inTab(LIB, "gamecube"))).toEqual({ inDrive: 0, onDeck: 0 });
  });

  it("says how long ago the last sync was", () => {
    const now = NOW;
    const t = Math.floor(now / 1000);
    expect(ago(t - 5, now)).toBe("just now");
    expect(ago(t - 120, now)).toBe("2 min ago");
    expect(ago(t - 3 * 3600, now)).toBe("3 h ago");
    expect(ago(t - 13 * 86400, now)).toBe("Oct 6");
  });

  it("words each sync state", () => {
    expect(syncLine(null, NOW)).toEqual({ text: "Not synced yet", warn: false });
    expect(syncLine(synced(120), NOW).text).toBe("Google Drive synced 2 min ago");
    expect(syncLine({ state: "syncing", last_synced_at: null, message: null }, NOW).text).toBe(
      "Syncing with Google Drive…",
    );
    expect(syncLine({ state: "offline", last_synced_at: null, message: "x" }, NOW)).toEqual({
      text: "Offline",
      warn: true,
    });
    expect(syncLine({ state: "error", last_synced_at: null, message: "x" }, NOW).warn).toBe(true);
  });
});

describe("Library screen", () => {
  const base = {
    info: null,
    error: null,
    driveFolder: "My Drive / EmuSuite / Emulation",
    onOpenSpike: () => {},
    onQuit: () => {},
  };

  it("shows recognized files as tiles and the rest under Not recognized", () => {
    render(<Home {...base} entries={LIB} />);
    expect(screen.getByRole("heading", { name: "All systems" })).toBeTruthy();
    expect(screen.getByText("3 ROMs in Drive · 2 on Deck")).toBeTruthy();
    expect(document.querySelector(".grid")!.textContent).toContain("Kirby");
    expect(screen.getByText("Not recognized")).toBeTruthy();
    expect(screen.getByText("GameCube / Shadow the Hedgehog (USA).zip")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Shadow/ })).toBeNull();
    expect(screen.getByText("My Drive / EmuSuite / Emulation / roms")).toBeTruthy();
  });

  it("a system tab shows only that system", () => {
    const onTab = vi.fn();
    render(<Home {...base} entries={LIB} tab="gba" onTab={onTab} />);
    expect(screen.getByRole("heading", { name: "GBA" })).toBeTruthy();
    expect(screen.getByText("2 ROMs in Drive · 1 on Deck")).toBeTruthy();
    expect(screen.queryByText("Ember")).toBeNull();
    expect(screen.getByText("My Drive / EmuSuite / Emulation / roms / gba")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "SNES" }));
    expect(onTab).toHaveBeenCalledWith("snes");
  });

  it("counts one ROM in the singular", () => {
    render(<Home {...base} entries={LIB} tab="snes" />);
    expect(screen.getByText("1 ROM in Drive · 1 on Deck")).toBeTruthy();
  });

  it("an empty tab says so", () => {
    render(<Home {...base} entries={LIB} tab="n64" />);
    expect(screen.getByText("No N64 ROMs yet.")).toBeTruthy();
  });

  it("an empty library offers Sync now", () => {
    const onSync = vi.fn();
    render(<Home {...base} entries={[]} onSync={onSync} />);
    expect(screen.getByText("Upload ROMs to this folder from any device and they appear here.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Sync now" }));
    expect(onSync).toHaveBeenCalled();
  });

  it("the sync line syncs when selected, and not while syncing", () => {
    const onSync = vi.fn();
    const { rerender } = render(<Home {...base} entries={LIB} sync={null} onSync={onSync} />);
    fireEvent.click(screen.getByRole("button", { name: "Not synced yet" }));
    expect(onSync).toHaveBeenCalledTimes(1);
    rerender(
      <Home {...base} entries={LIB} sync={{ state: "syncing", last_synced_at: null, message: null }} onSync={onSync} />,
    );
    const busy = screen.getByRole("button", { name: "Syncing with Google Drive…" }) as HTMLButtonElement;
    expect(busy.disabled).toBe(true);
  });

  it("shows why a sync failed", () => {
    const message = "Couldn't reach Google. Check the Deck's internet connection and try again.";
    render(<Home {...base} entries={LIB} sync={{ state: "offline", last_synced_at: null, message }} />);
    expect(screen.getByText(message)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Offline" })).toBeTruthy();
  });
});

describe("drawer", () => {
  const base = {
    info: null,
    error: null,
    driveFolder: "My Drive / EmuSuite / Emulation",
    onOpenSpike: () => {},
    onQuit: () => {},
  };
  it("shows the focused tile's game, with its main action", () => {
    const onPlay = vi.fn();
    const onOpen = vi.fn();
    render(<Home {...base} entries={LIB} onPlay={onPlay} onOpen={onOpen} />);
    const tiles = document.querySelectorAll<HTMLElement>(".tile");
    act(() => tiles[1]!.focus());
    const drawer = screen.getByRole("complementary", { name: /details/ });
    const title = drawer.querySelector(".drawer-title")!.textContent!;
    expect(tiles[1]!.textContent).toContain(title);
    fireEvent.click(screen.getByRole("button", { name: "Open ROM page: controls and Steam shortcut" }));
    expect(onOpen).toHaveBeenCalledWith(tiles[1]!.dataset.romId, undefined);
  });
});

describe("a large library", () => {
  const base = {
    info: null,
    error: null,
    driveFolder: "My Drive / Emulation",
    onOpenSpike: () => {},
    onQuit: () => {},
  };

  it("works out the rows near the screen", () => {
    // 500 rows; the screen shows about three.
    expect(visibleRows(0, 700, 80, 500)).toEqual({ first: 0, last: 5 });
    expect(visibleRows(244 * 100 + 80, 700, 80, 500)).toEqual({ first: 98, last: 105 });
    expect(visibleRows(244 * 499 + 80, 700, 80, 500)).toEqual({ first: 497, last: 500 });
  });

  it("draws only those rows of 2,000 ROMs", () => {
    const many = Array.from({ length: 2000 }, (_, i) =>
      entry(`Game ${i}`, "gba", "GBA", { state: "drive_only" }),
    ).map((e, i) => ({ ...e, rom: { ...e.rom, id: `gba-${String(i).padStart(8, "0")}` } }));
    const started = performance.now();
    render(<Home {...base} entries={many} />);
    const elapsed = performance.now() - started;
    const tiles = document.querySelectorAll(".tile");
    expect(tiles.length).toBeLessThan(60);
    expect(tiles.length).toBeGreaterThan(0);
    const grid = document.querySelector<HTMLElement>(".grid.windowed")!;
    expect(grid.getAttribute("aria-rowcount")).toBe("500");
    // The space for the rest is kept, so scrolling looks the same as with every tile drawn.
    expect(parseInt(grid.style.paddingBottom, 10)).toBeGreaterThan(244 * 400);
    expect(elapsed).toBeLessThan(1000);
  });
});
