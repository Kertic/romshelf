import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { settingsView, type SettingsView, type TransferRow } from "./api";
import { Settings, type SettingsActions } from "./Settings";
import { Transfers } from "./Transfers";

afterEach(cleanup);

const VIEW: SettingsView = await settingsView();

function open(view: SettingsView = VIEW) {
  const actions: SettingsActions = {
    signOut: vi.fn(async () => {}),
    changeDriveFolder: vi.fn(),
    changeStorage: vi.fn(async () => {}),
    setSaveVersions: vi.fn(async () => {}),
    setAutoAdd: vi.fn(async () => {}),
    checkForUpdates: vi.fn(async () => {}),
    diagnostics: vi.fn(async () => "Romshelf dev"),
    copy: vi.fn(async () => {}),
  };
  const props = { view, actions, onBack: vi.fn(), setBack: vi.fn(), reload: vi.fn() };
  render(<Settings {...props} />);
  return { ...props, actions };
}

const button = (name: string | RegExp) => screen.getByRole("button", { name });
const flush = () => act(async () => {});

describe("Settings", () => {
  it("has the guide's six groups", () => {
    open();
    for (const g of ["Google account", "Drive folder", "Storage", "Saves", "Steam", "About"]) {
      expect(screen.getByRole("region", { name: g })).toBeTruthy();
    }
    expect(screen.getByText("player@example.com")).toBeTruthy();
    expect(screen.getByText("My Drive / Emulation")).toBeTruthy();
    expect(screen.getByText("40.0 GB free")).toBeTruthy();
    expect(screen.getByText("Romshelf dev")).toBeTruthy();
  });

  it("signing out asks first, with Cancel focused", async () => {
    const p = open();
    fireEvent.click(button("Sign out"));
    expect(screen.getByRole("dialog", { name: "Sign out of Google?" })).toBeTruthy();
    await act(async () => {
      await new Promise((r) => setTimeout(r));
    });
    expect(document.activeElement).toBe(button("Cancel"));
    fireEvent.click(screen.getAllByRole("button", { name: "Sign out" }).at(-1)!);
    await flush();
    expect(p.actions.signOut).toHaveBeenCalled();
  });

  it("versions to keep step by one, within 1 to 50", async () => {
    const p = open();
    fireEvent.click(button("Keep more versions"));
    await flush();
    expect(p.actions.setSaveVersions).toHaveBeenCalledWith(11);
    cleanup();
    open({ ...VIEW, save_versions: 1 });
    expect((button("Keep fewer versions") as HTMLButtonElement).disabled).toBe(true);
  });

  it("moving storage offers the Deck and SD cards", async () => {
    const p = open();
    const storage = screen.getByRole("region", { name: "Storage" });
    fireEvent.click(within(storage).getByRole("button", { name: "Change location…" }));
    expect(screen.getByRole("dialog", { name: "Where should ROMs go?" })).toBeTruthy();
    expect((button(/This Deck · current/) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(button(/SD card \(SD\)/));
    await flush();
    expect(p.actions.changeStorage).toHaveBeenCalledWith("/run/media/deck/SD/Romshelf");
  });

  it("copies diagnostics and says so", async () => {
    const p = open();
    fireEvent.click(button("Copy diagnostics"));
    await flush();
    expect(p.actions.copy).toHaveBeenCalledWith("Romshelf dev");
    expect(screen.getByText(/Diagnostics copied/)).toBeTruthy();
  });

  it("adding new ROMs to Steam is a switch", async () => {
    const p = open();
    fireEvent.click(screen.getByRole("checkbox", { name: "Add new ROMs automatically" }));
    await flush();
    expect(p.actions.setAutoAdd).toHaveBeenCalledWith(true);
  });
});

describe("Transfers", () => {
  const ROWS: TransferRow[] = [
    { rom_id: "gba-1", name: "Kirby", kind: "download", state: "running", size: 8 * 1024 * 1024, message: null },
    { rom_id: "gba-2", name: "Tidewalker", kind: "download", state: "queued", size: 16 * 1024 * 1024, message: null },
    { rom_id: "snes-1", name: "Ember", kind: "backup", state: "waiting", size: 0, message: "Couldn't reach Google." },
  ];

  it("lists downloads and backups, and cancels a download", () => {
    const onCancel = vi.fn();
    render(
      <Transfers rows={ROWS} progress={{ "gba-1": { bytes: 4, total: 8, rate: 0 } }} onCancel={onCancel} onClose={vi.fn()} />,
    );
    expect(screen.getByRole("dialog", { name: "Transfers" })).toBeTruthy();
    expect(screen.getByText("50%")).toBeTruthy();
    expect(screen.getByText("Waiting to download · 16 MB")).toBeTruthy();
    expect(screen.getByText("Save waiting to back up")).toBeTruthy();
    expect(screen.getByText("Couldn't reach Google.")).toBeTruthy();
    fireEvent.click(button("Cancel downloading Kirby"));
    expect(onCancel).toHaveBeenCalledWith("gba-1");
  });

  it("says when nothing is happening", () => {
    render(<Transfers rows={[]} progress={{}} onCancel={vi.fn()} onClose={vi.fn()} />);
    expect(screen.getByText("Nothing in progress.")).toBeTruthy();
  });
});
