import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LibraryEntry, SaveConflict, SaveSync, SaveVersion } from "./api";
import { LaunchWindow } from "./LaunchWindow";
import { RomPage, type SaveActions } from "./RomActions";
import { formatWhen, saveLine, SaveConflictDialog } from "./Saves";

afterEach(cleanup);

// Thursday 2026-10-15, 3:42 PM local time.
const NOW = new Date(2026, 9, 15, 15, 42);
const at = (d: Date) => Math.floor(d.getTime() / 1000);

const NO_SAVE: SaveSync = {
  last_synced_hash: null,
  last_backup_at: null,
  last_backup_device: null,
  pending: false,
  last_error: null,
};

function entry(save: Partial<SaveSync> = {}, keepsStates = true): LibraryEntry {
  return {
    rom: {
      id: "gba-8f031392",
      drive_file_id: "f1",
      system: "gba",
      file_name: "Kirby.gba",
      display_name: "Kirby",
      size: 16 * 1024 * 1024,
      checksum: null,
      location: { state: "on_deck" },
      recognized: true,
    },
    system_label: "GBA",
    system_colour: "#1F6B62",
    emulator: "RetroArch",
    emulator_id: "retroarch",
    core: "mgba",
    emulator_installed: true,
    emulator_ready: true,
    actions: [],
    in_steam: false,
    save: { ...NO_SAVE, ...save },
    keeps_states: keepsStates,
    control_differences: 0,
  };
}

const VERSIONS: SaveVersion[] = [
  { id: "v2", taken_at: at(new Date(2026, 9, 15, 9, 5)), device: "steamdeck", size: 32 * 1024, emulator: "retroarch", core: "mgba", hash: "b" },
  { id: "v1", taken_at: at(new Date(2026, 9, 6, 20, 0)), device: "desktop", size: 32 * 1024, emulator: "retroarch", core: "mgba", hash: "a" },
];

function actions(extra: Partial<SaveActions> = {}): SaveActions {
  return {
    backup: vi.fn(async () => ({ kind: "uploaded" as const, version: VERSIONS[0]! })),
    versions: vi.fn(async () => VERSIONS),
    restore: vi.fn(async (id: string) => ({ version: VERSIONS.find((v) => v.id === id)!, warning: null })),
    states: vi.fn(async () => [{ name: "Kirby.state1", size: 512 * 1024, modified: at(new Date(2026, 9, 14, 21, 10)) }]),
    deleteState: vi.fn(async () => {}),
    ...extra,
  };
}

function open(e: LibraryEntry, saves: SaveActions) {
  render(
    <RomPage
      entry={e}
      onDownload={vi.fn()}
      onCancel={vi.fn()}
      onRemove={vi.fn(async () => {})}
      onClose={vi.fn()}
      setBack={vi.fn()}
      saves={saves}
      now={NOW}
    />,
  );
}

const button = (name: string | RegExp) => screen.getByRole("button", { name });
const flush = () => act(async () => {});

describe("save wording", () => {
  it("times read as the guide says", () => {
    expect(formatWhen(at(new Date(2026, 9, 15, 15, 42)), NOW)).toBe("Today 3:42 PM");
    expect(formatWhen(at(new Date(2026, 9, 14, 21, 10)), NOW)).toBe("Yesterday 9:10 PM");
    expect(formatWhen(at(new Date(2026, 9, 6, 9, 0)), NOW)).toBe("Oct 6");
  });

  it("the save backup line covers every state", () => {
    expect(saveLine(NO_SAVE, NOW)).toEqual({ text: "Not backed up yet", warn: false });
    expect(
      saveLine({ ...NO_SAVE, last_backup_at: at(new Date(2026, 9, 15, 9, 5)), last_backup_device: "steamdeck" }, NOW),
    ).toEqual({ text: "Backed up Today 9:05 AM from steamdeck", warn: false });
    expect(saveLine({ ...NO_SAVE, pending: true }, NOW)).toEqual({
      text: "Waiting to back up. Romshelf tries again when Drive can be reached.",
      warn: true,
    });
  });
});

describe("the game's save actions", () => {
  it("shows the last backup and backs up on request", async () => {
    const saves = actions();
    open(entry({ last_backup_at: at(new Date(2026, 9, 6, 9, 0)), last_backup_device: "steamdeck" }), saves);
    expect(screen.getAllByText("Backed up Oct 6 from steamdeck").length).toBeGreaterThan(0);
    fireEvent.click(button("Back up save data"));
    await flush();
    expect(saves.backup).toHaveBeenCalled();
    expect(screen.getByText("Backed up Kirby")).toBeTruthy();
  });

  it("says when the save was already backed up, or there isn't one", async () => {
    const saves = actions({ backup: vi.fn(async () => ({ kind: "unchanged" as const })) });
    open(entry(), saves);
    fireEvent.click(button("Back up save data"));
    await flush();
    expect(screen.getByText("Drive already has this save.")).toBeTruthy();
    cleanup();
    open(entry(), actions({ backup: vi.fn(async () => ({ kind: "no_save" as const })) }));
    fireEvent.click(button("Back up save data"));
    await flush();
    expect(screen.getByText("Kirby has no save on this Deck yet. Play it first.")).toBeTruthy();
  });

  it("a failed backup says what happened", async () => {
    const saves = actions({
      backup: vi.fn(async () => {
        throw { kind: "offline", message: "Couldn't reach Google. Check the Deck's internet connection and try again." };
      }),
    });
    open(entry(), saves);
    fireEvent.click(button("Back up save data"));
    await flush();
    expect(screen.getByText(/Couldn't reach Google/)).toBeTruthy();
  });

  it("restoring lists versions and backs up the current save first", async () => {
    const saves = actions();
    open(entry(), saves);
    fireEvent.click(button("Restore save from Drive…"));
    await flush();
    expect(screen.getByRole("dialog", { name: "Restore a save" })).toBeTruthy();
    expect(screen.getByText("Your current save is backed up first.")).toBeTruthy();
    // Date and time, device name, size.
    expect(button(/Today 9:05 AM.*steamdeck.*32 KB/)).toBeTruthy();
    expect(button(/Oct 6.*desktop.*32 KB/)).toBeTruthy();
    // The safe choice is focused first.
    await act(async () => {
      await new Promise((r) => setTimeout(r));
    });
    expect(document.activeElement).toBe(button("Cancel"));
    fireEvent.click(button(/Oct 6/));
    fireEvent.click(button("Restore"));
    await flush();
    expect(saves.restore).toHaveBeenCalledWith("v1");
    expect(screen.getByText("Restored the save from Oct 6.")).toBeTruthy();
  });

  it("restore shows a core warning", async () => {
    const saves = actions({
      restore: vi.fn(async () => ({
        version: VERSIONS[1]!,
        warning: "This save was made with retroarch (vba_next); retroarch (mgba) may not load it.",
      })),
    });
    open(entry(), saves);
    fireEvent.click(button("Restore save from Drive…"));
    await flush();
    fireEvent.click(button(/Oct 6/));
    fireEvent.click(button("Restore"));
    await flush();
    expect(screen.getByText(/may not load it/)).toBeTruthy();
  });

  it("no backups yet says so", async () => {
    open(entry(), actions({ versions: vi.fn(async () => []) }));
    fireEvent.click(button("Restore save from Drive…"));
    await flush();
    expect(screen.getByText("Drive has no backups of Kirby's save yet.")).toBeTruthy();
    expect((button("Restore") as HTMLButtonElement).disabled).toBe(true);
  });

  it("save states are listed and deleted after confirming", async () => {
    const saves = actions();
    open(entry(), saves);
    fireEvent.click(button("Save states…"));
    await flush();
    expect(screen.getByRole("dialog", { name: "Save states" })).toBeTruthy();
    fireEvent.click(button(/Kirby\.state1.*Yesterday 9:10 PM/));
    expect(screen.getByRole("dialog", { name: "Delete save state Kirby.state1?" })).toBeTruthy();
    expect(screen.getByText(/Drive's trash/)).toBeTruthy();
    fireEvent.click(button("Delete save state"));
    await flush();
    expect(saves.deleteState).toHaveBeenCalledWith("Kirby.state1");
  });

  it("save states aren't offered when Romshelf doesn't keep them", () => {
    open(entry({}, false), actions());
    expect(screen.queryByRole("button", { name: "Save states…" })).toBeNull();
  });
});

const CONFLICT: SaveConflict = {
  rom_id: "gba-8f031392",
  name: "Kirby",
  deck: { taken_at: at(new Date(2026, 9, 15, 15, 0)), device: "steamdeck" },
  drive: { taken_at: at(new Date(2026, 9, 14, 21, 10)), device: "desktop" },
};

describe("save conflict", () => {
  it("names both sides and answers", () => {
    const onAnswer = vi.fn();
    render(<SaveConflictDialog conflict={CONFLICT} onAnswer={onAnswer} now={NOW} />);
    expect(screen.getByRole("dialog", { name: "Two different saves" })).toBeTruthy();
    expect(screen.getByText("This Deck and Drive both have newer saves for Kirby.")).toBeTruthy();
    expect(screen.getByText("This Deck: Today 3:00 PM, steamdeck")).toBeTruthy();
    expect(screen.getByText("Drive: Yesterday 9:10 PM, desktop")).toBeTruthy();
    const names = screen.getAllByRole("button").map((b) => b.textContent);
    expect(names).toEqual(["Use this Deck's", "Use Drive's", "Cancel"]);
    fireEvent.click(button("Use Drive's"));
    expect(onAnswer).toHaveBeenCalledWith("use_drive");
    fireEvent.click(button("Use this Deck's"));
    expect(onAnswer).toHaveBeenCalledWith("keep_deck");
    fireEvent.click(button("Cancel"));
    expect(onAnswer).toHaveBeenCalledWith("cancel");
  });

  it("the launch window asks it", () => {
    const onAnswer = vi.fn();
    render(
      <LaunchWindow
        entry={entry()}
        launch={{ romId: "gba-8f031392", label: "Checking save", percent: null, running: true, error: null, conflict: CONFLICT }}
        onRetry={vi.fn()}
        onOpenRomshelf={vi.fn()}
        onAnswer={onAnswer}
      />,
    );
    fireEvent.click(button("Use this Deck's"));
    expect(onAnswer).toHaveBeenCalledWith("keep_deck");
  });

  it("Play from the game's dialog asks it too", () => {
    const onAnswer = vi.fn();
    render(
      <RomPage
        entry={entry()}
        onDownload={vi.fn()}
        onCancel={vi.fn()}
        onRemove={vi.fn(async () => {})}
        onClose={vi.fn()}
        setBack={vi.fn()}
        launch={{ romId: "gba-8f031392", label: "Checking save", percent: null, running: true, error: null, conflict: CONFLICT }}
        onAnswer={onAnswer}
      />,
    );
    fireEvent.click(button("Use Drive's"));
    expect(onAnswer).toHaveBeenCalledWith("use_drive");
  });
});
