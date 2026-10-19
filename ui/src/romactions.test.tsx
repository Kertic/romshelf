import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LibraryEntry, LocationState } from "./api";
import { tileStatus } from "./Home";
import { humanSize, progressLine, RomDialog } from "./RomActions";

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
    emulator_installed: false,
    in_steam: false,
  };
}

function setup(location: LocationState, extra: Partial<Parameters<typeof RomDialog>[0]> = {}) {
  const props = {
    entry: entry(location),
    onDownload: vi.fn(),
    onCancel: vi.fn(),
    onDelete: vi.fn(async () => {}),
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
    expect(button("Delete…")).toBeTruthy();
  });

  it("shows progress and a cancel while downloading", () => {
    const p = setup({ state: "downloading", percent: 5 }, { progress: { bytes: 50, total: 100, rate: 0 } });
    expect(screen.getByText("50%")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Delete…" })).toBeNull();
    fireEvent.click(button("Cancel download"));
    expect(p.onCancel).toHaveBeenCalled();
  });

  it("shows what went wrong with the last download", () => {
    setup({ state: "drive_only" }, { error: "Not enough space for Kirby: it needs 16 MB and 2 MB is free." });
    expect(screen.getByText(/Not enough space for Kirby/)).toBeTruthy();
  });
});

describe("delete dialog", () => {
  it("lists the three scopes with Cancel first, and keeps save backups by default", async () => {
    const p = setup({ state: "on_deck" });
    await act(async () => fireEvent.click(button("Delete…")));
    expect(screen.getByRole("heading", { name: "Delete Kirby?" })).toBeTruthy();
    const radios = screen.getAllByRole("radio");
    expect(radios.map((r) => r.textContent)).toEqual([
      "From this Deck (keeps it in Drive)●",
      "From Drive (moves it to Drive's trash)○",
      "From both○",
    ]);
    // From this Deck: no save-backup question.
    expect(screen.queryByRole("checkbox")).toBeNull();
    const actions = screen.getAllByRole("button").filter((b) => ["Cancel", "Delete"].includes(b.textContent ?? ""));
    expect(actions.map((b) => b.textContent)).toEqual(["Cancel", "Delete"]);
    expect(p.setBack).toHaveBeenLastCalledWith(expect.objectContaining({ label: "Back" }));

    fireEvent.click(screen.getByRole("radio", { name: /From both/ }));
    const keep = screen.getByRole("checkbox", { name: /Keep save backups/ });
    expect(keep.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(keep);
    await act(async () => fireEvent.click(button("Delete")));
    expect(p.onDelete).toHaveBeenCalledWith("both", false);
  });

  it("can't delete from the Deck what isn't on it", async () => {
    const p = setup({ state: "drive_only" });
    await act(async () => fireEvent.click(button("Delete…")));
    expect((screen.getByRole("radio", { name: /From this Deck/ }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("radio", { name: /From both/ }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole("radio", { name: /From Drive/ }).getAttribute("aria-checked")).toBe("true");
    await act(async () => fireEvent.click(button("Delete")));
    expect(p.onDelete).toHaveBeenCalledWith("drive", true);
  });

  it("Cancel goes back without deleting", async () => {
    const p = setup({ state: "on_deck" });
    await act(async () => fireEvent.click(button("Delete…")));
    await act(async () => fireEvent.click(button("Cancel")));
    expect(p.onDelete).not.toHaveBeenCalled();
    expect(button("Download again")).toBeTruthy();
  });

  it("shows why a delete failed and stays open", async () => {
    const onDelete = vi.fn(async () => {
      throw { kind: "offline", message: "Couldn't reach Google. Check the Deck's internet connection and try again." };
    });
    setup({ state: "on_deck" }, { onDelete });
    await act(async () => fireEvent.click(button("Delete…")));
    fireEvent.click(screen.getByRole("radio", { name: /From Drive/ }));
    await act(async () => fireEvent.click(button("Delete")));
    expect(screen.getByText(/Couldn't reach Google/)).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Delete Kirby?" })).toBeTruthy();
  });
});
