import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LibraryEntry } from "./api";
import { LaunchWindow } from "./LaunchWindow";
import { RomDialog } from "./RomActions";

afterEach(cleanup);

const ENTRY: LibraryEntry = {
  rom: {
    id: "gamecube-2260f1ec",
    drive_file_id: "f1",
    system: "gamecube",
    file_name: "Twilight Princess.rvz",
    display_name: "Twilight Princess",
    size: 1024,
    checksum: null,
    location: { state: "on_deck" },
    recognized: true,
  },
  system_label: "GameCube",
  system_colour: "#4A3470",
  emulator: "Dolphin",
  emulator_id: "dolphin",
  core: null,
  emulator_installed: true,
  emulator_ready: true,
  actions: [],
  in_steam: false,
  save: { last_synced_hash: null, last_backup_at: null, last_backup_device: null, pending: false, last_error: null },
  keeps_states: false,
  control_differences: 0,
};

describe("launch window", () => {
  it("shows the step and its progress under the game", () => {
    render(
      <LaunchWindow
        entry={ENTRY}
        launch={{ romId: ENTRY.rom.id, label: "Downloading", percent: 40, running: true, error: null }}
        onRetry={vi.fn()}
        onOpenRomshelf={vi.fn()}
      />,
    );
    expect(screen.getByRole("heading", { name: "Twilight Princess" })).toBeTruthy();
    expect(screen.getByText("Downloading…")).toBeTruthy();
    expect(screen.getByText("40%")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("an error offers Try again and Open Romshelf", async () => {
    const onRetry = vi.fn();
    const onOpenRomshelf = vi.fn();
    render(
      <LaunchWindow
        entry={ENTRY}
        launch={{
          romId: ENTRY.rom.id,
          label: null,
          percent: null,
          running: false,
          error: "Couldn't reach Google. Check the Deck's internet connection and try again.",
        }}
        onRetry={onRetry}
        onOpenRomshelf={onOpenRomshelf}
      />,
    );
    expect(screen.getByRole("alert").textContent).toContain("Couldn't reach Google");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Try again" })));
    fireEvent.click(screen.getByRole("button", { name: "Open Romshelf" }));
    expect(onRetry).toHaveBeenCalled();
    expect(onOpenRomshelf).toHaveBeenCalled();
  });
});

describe("Play", () => {
  const base = {
    onDownload: vi.fn(),
    onCancel: vi.fn(),
    onRemove: vi.fn(async () => {}),
    onClose: vi.fn(),
    setBack: vi.fn(),
  };

  it("is the first action for a downloaded game with its emulator ready", () => {
    const onPlay = vi.fn();
    render(<RomDialog {...base} entry={ENTRY} onPlay={onPlay} />);
    const buttons = screen.getAllByRole("button").map((b) => b.textContent);
    expect(buttons[0]).toBe("Play");
    fireEvent.click(screen.getByRole("button", { name: "Play" }));
    expect(onPlay).toHaveBeenCalled();
  });

  it("isn't offered before the game is downloaded or its emulator set up", () => {
    render(<RomDialog {...base} entry={{ ...ENTRY, rom: { ...ENTRY.rom, location: { state: "drive_only" } } }} />);
    expect(screen.queryByRole("button", { name: "Play" })).toBeNull();
    cleanup();
    render(<RomDialog {...base} entry={{ ...ENTRY, emulator_ready: false }} />);
    expect(screen.queryByRole("button", { name: "Play" })).toBeNull();
  });

  it("shows the step while starting, and why it failed", () => {
    const { rerender } = render(
      <RomDialog
        {...base}
        entry={ENTRY}
        launch={{ romId: ENTRY.rom.id, label: "Playing in Dolphin", percent: null, running: true, error: null }}
      />,
    );
    const busy = screen.getByRole("button", { name: "Playing in Dolphin…" }) as HTMLButtonElement;
    expect(busy.disabled).toBe(true);
    rerender(
      <RomDialog
        {...base}
        entry={ENTRY}
        launch={{ romId: ENTRY.rom.id, label: null, percent: null, running: false, error: "Dolphin can't run it." }}
      />,
    );
    expect(screen.getByText("Dolphin can't run it.")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Play" }) as HTMLButtonElement).disabled).toBe(false);
  });
});
