import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LibraryEntry, LocationState } from "./api";
import { tileStatus } from "./Home";
import { emulatorLine, humanSize, progressLine, RomDialog } from "./RomActions";

afterEach(cleanup);

function entry(location: LocationState): LibraryEntry {
  return {
    rom: {
      id: "gba-0a1b2c3d",
      drive_file_id: "f1",
      system: "gba",
      file_name: "Kirby (USA).gba",
      display_name: "Kirby",
      size: 16 * 1024 * 1024,
      checksum: null,
      location,
      recognized: true,
    },
    system_label: "GBA",
    system_colour: "#1F6B62",
    emulator: "RetroArch",
    emulator_id: "retroarch",
    core: "mgba",
    emulator_installed: true,
    emulator_ready: true,
    actions: [
      { id: "save_states", label: "Save states" },
      { id: "cheats", label: "Cheats" },
    ],
    in_steam: false,
    save: { last_synced_hash: null, last_backup_at: null, last_backup_device: null, pending: false, last_error: null },
    keeps_states: false,
  };
}

function setup(location: LocationState, extra: Partial<Parameters<typeof RomDialog>[0]> = {}) {
  const props = {
    entry: entry(location),
    onDownload: vi.fn(),
    onCancel: vi.fn(),
    onRemove: vi.fn(async () => {}),
    onClose: vi.fn(),
    setBack: vi.fn(),
    ...extra,
  };
  render(<RomDialog {...props} />);
  return props;
}

const button = (name: string | RegExp) => screen.getByRole("button", { name });

describe("download wording", () => {
  it("sizes and progress read naturally", () => {
    expect(humanSize(16 * 1024 * 1024)).toBe("16 MB");
    expect(humanSize(1_503_238_553)).toBe("1.4 GB");
    expect(progressLine({ bytes: 64, total: 100, rate: 0 })).toBe("64%");
    expect(progressLine({ bytes: 640 * 1024 * 1024, total: 1024 * 1024 * 1024, rate: 2 * 1024 * 1024 })).toBe(
      "62% · 2 MB/s · about 3 min left",
    );
    expect(progressLine({ bytes: 99, total: 100, rate: 1024 })).toMatch(/less than a minute/);
  });

  it("a tile shows live progress, even before the emulator is set up", () => {
    const e = entry({ state: "downloading", percent: 10 });
    expect(tileStatus(e).text).toBe("Downloading 10%");
    expect(tileStatus(e, { bytes: 64, total: 100, rate: 1 }).text).toBe("Downloading 64%");
  });
});

describe("game actions", () => {
  it("offers a download for a game in Drive only", () => {
    const p = setup({ state: "drive_only" });
    expect(screen.getByText("GBA · Kirby (USA).gba · 16 MB")).toBeTruthy();
    expect(screen.getByText(/In Drive only/)).toBeTruthy();
    fireEvent.click(button("Download to Deck"));
    expect(p.onDownload).toHaveBeenCalled();
    expect(p.setBack).toHaveBeenLastCalledWith(expect.objectContaining({ label: "Close" }));
  });

  it("offers Download again for a game on the Deck", () => {
    setup({ state: "on_deck" });
    expect(button("Download again")).toBeTruthy();
    expect(button("Remove from Deck…")).toBeTruthy();
  });

  it("shows progress and a cancel while downloading", () => {
    const p = setup({ state: "downloading", percent: 5 }, { progress: { bytes: 50, total: 100, rate: 0 } });
    expect(screen.getByText("50%")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Remove from Deck…" })).toBeNull();
    fireEvent.click(button("Cancel download"));
    expect(p.onCancel).toHaveBeenCalled();
  });

  it("shows what went wrong with the last download", () => {
    setup({ state: "drive_only" }, { error: "Not enough space for Kirby: it needs 16 MB and 2 MB is free." });
    expect(screen.getByText(/Not enough space for Kirby/)).toBeTruthy();
  });
});

describe("remove from Deck", () => {
  it("is only offered for a game on the Deck, and never offers Drive", () => {
    setup({ state: "drive_only" });
    expect(screen.queryByRole("button", { name: /Remove/ })).toBeNull();
    expect(screen.queryByText(/Drive's trash/)).toBeNull();
  });

  it("asks once with Cancel first, says Drive keeps the game, then removes", async () => {
    const p = setup({ state: "on_deck" });
    await act(async () => fireEvent.click(button("Remove from Deck…")));
    expect(screen.getByRole("heading", { name: "Remove Kirby from this Deck?" })).toBeTruthy();
    expect(screen.getByText(/stays in your Google Drive/)).toBeTruthy();
    expect(screen.getByText(/frees 16 MB/)).toBeTruthy();
    // No choices to select: nothing but the focused button can look selected.
    expect(screen.queryAllByRole("radio")).toHaveLength(0);
    expect(screen.queryAllByRole("checkbox")).toHaveLength(0);
    const actions = screen.getAllByRole("button").map((b) => b.textContent);
    expect(actions).toEqual(["Cancel", "Remove from Deck"]);
    expect(p.setBack).toHaveBeenLastCalledWith(expect.objectContaining({ label: "Back" }));
    await act(async () => fireEvent.click(button("Remove from Deck")));
    expect(p.onRemove).toHaveBeenCalledTimes(1);
  });

  it("Cancel goes back without removing", async () => {
    const p = setup({ state: "on_deck" });
    await act(async () => fireEvent.click(button("Remove from Deck…")));
    await act(async () => fireEvent.click(button("Cancel")));
    expect(p.onRemove).not.toHaveBeenCalled();
    expect(button("Download again")).toBeTruthy();
  });

  it("shows why removing failed and stays open", async () => {
    const onRemove = vi.fn(async () => {
      throw { kind: "error", message: "Kirby is downloading. Cancel the download, then remove it." };
    });
    setup({ state: "on_deck" }, { onRemove });
    await act(async () => fireEvent.click(button("Remove from Deck…")));
    await act(async () => fireEvent.click(button("Remove from Deck")));
    expect(screen.getByText(/Cancel the download, then remove it/)).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Remove Kirby from this Deck?" })).toBeTruthy();
  });
});

describe("emulator setup from a game", () => {
  it("says which emulator and core run the game", () => {
    setup({ state: "on_deck" });
    expect(screen.getByText("Runs in RetroArch with the mGBA core")).toBeTruthy();
    expect(emulatorLine({ ...entry({ state: "on_deck" }), emulator: "Dolphin", core: null }).text).toBe(
      "Runs in Dolphin",
    );
  });

  it("lists exactly the emulator's own functions", () => {
    setup({ state: "on_deck" });
    const group = screen.getByRole("region", { name: "RetroArch functions" });
    expect([...group.querySelectorAll(".function-row > span:first-child")].map((s) => s.textContent)).toEqual([
      "Save states",
      "Cheats",
    ]);
  });

  it("offers Set up first when the emulator is missing (screen 2b)", () => {
    const gc: LibraryEntry = {
      ...entry({ state: "drive_only" }),
      emulator: "Dolphin",
      emulator_id: "dolphin",
      core: null,
      emulator_installed: false,
      emulator_ready: false,
      actions: [{ id: "memory_cards", label: "Memory cards" }],
    };
    const onSetupEmulator = vi.fn();
    render(
      <RomDialog
        entry={gc}
        onDownload={vi.fn()}
        onCancel={vi.fn()}
        onRemove={vi.fn(async () => {})}
        onSetupEmulator={onSetupEmulator}
        onClose={vi.fn()}
        setBack={vi.fn()}
      />,
    );
    expect(screen.getByText("Needs Dolphin")).toBeTruthy();
    const buttons = screen.getAllByRole("button").map((b) => b.textContent);
    expect(buttons[0]).toBe("Set up Dolphin");
    expect(screen.getByRole("button", { name: "Download to Deck" }).className).toContain("outline");
    fireEvent.click(button("Set up Dolphin"));
    expect(onSetupEmulator).toHaveBeenCalled();
    expect(screen.getByRole("region", { name: "Dolphin functions" }).textContent).toContain("Memory cards");
  });

  it("shows setup progress, and a failure with a retry", () => {
    const gc: LibraryEntry = {
      ...entry({ state: "drive_only" }),
      emulator: "Dolphin",
      emulator_id: "dolphin",
      emulator_installed: false,
      emulator_ready: false,
    };
    const props = {
      entry: gc,
      onDownload: vi.fn(),
      onCancel: vi.fn(),
      onRemove: vi.fn(async () => {}),
      onClose: vi.fn(),
      setBack: vi.fn(),
    };
    const { rerender } = render(
      <RomDialog {...props} setup={{ step: "Installing", percent: 45, running: true, message: null, failed: false }} />,
    );
    expect(screen.getByText("Installing… 45%")).toBeTruthy();
    expect((button("Set up Dolphin") as HTMLButtonElement).disabled).toBe(true);
    rerender(
      <RomDialog
        {...props}
        setup={{
          step: null,
          percent: null,
          running: false,
          failed: true,
          message: "Couldn't install Dolphin: error: Unable to connect to dl.flathub.org. Try again.",
        }}
      />,
    );
    expect(screen.getByText(/Unable to connect/)).toBeTruthy();
    expect(button("Try setting up Dolphin again")).toBeTruthy();
  });

  it("says when Romshelf can't run a system yet", () => {
    const ps2: LibraryEntry = {
      ...entry({ state: "drive_only" }),
      system_label: "PS2",
      emulator: null,
      emulator_id: null,
      core: null,
      emulator_ready: false,
      emulator_installed: false,
      actions: [],
    };
    expect(emulatorLine(ps2)).toEqual({ text: "Romshelf can't run PS2 games yet.", warn: true });
  });
});
