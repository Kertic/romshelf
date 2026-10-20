import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { steamShortcut, type Applied, type ShortcutView } from "./api";
import { RestartSteamDialog, SteamShortcut } from "./SteamShortcut";

afterEach(cleanup);

const VIEW: ShortcutView = await steamShortcut("gba-00000001");
const APPLIED: Applied = { added: 0, updated: 1, removed: 0, artwork: 0, restart_needed: true };

function open(view: ShortcutView = VIEW) {
  const props = {
    view,
    setBack: vi.fn(),
    onBack: vi.fn(),
    onSave: vi.fn(async () => APPLIED),
    onAutoAdd: vi.fn(async () => {}),
    onArtwork: vi.fn(async () => {}),
    loadImages: vi.fn(async () => [{ path: "/home/deck/Pictures/cover.png", name: "cover.png", folder: "Pictures" }]),
  };
  render(<SteamShortcut {...props} />);
  return props;
}

const button = (name: string | RegExp) => screen.getByRole("button", { name });
const flush = () => act(async () => {});

describe("Steam shortcut screen", () => {
  it("shows the name, five slots and what Steam launches", () => {
    open();
    expect((screen.getByRole("textbox", { name: "Name in Steam" }) as HTMLInputElement).value).toBe("Tidewalker");
    for (const slot of ["Capsule", "Wide capsule", "Hero", "Logo", "Icon"]) expect(button(new RegExp(slot))).toBeTruthy();
    // Only Logo can be missing.
    expect(button(/Logo/).textContent).toContain("Missing");
    expect(button(/^Capsule, generated/)).toBeTruthy();
    expect(screen.getByText("/home/deck/Applications/romshelf.AppImage")).toBeTruthy();
    expect(screen.getByText("launch --id gba-00000001")).toBeTruthy();
    expect(screen.getByText(/Steam starts Romshelf, and Romshelf starts the game/)).toBeTruthy();
  });

  it("saves the name and showing it", async () => {
    const p = open();
    fireEvent.change(screen.getByRole("textbox", { name: "Name in Steam" }), { target: { value: "Tidewalker DX" } });
    fireEvent.click(button("Save shortcut"));
    await flush();
    expect(p.onSave).toHaveBeenCalledWith(true, "Tidewalker DX");
    expect(screen.getByText("Saved. Restart Steam to see the change.")).toBeTruthy();
    expect(screen.getByText("Show Tidewalker DX in Steam library")).toBeTruthy();
  });

  it("removes from Steam", async () => {
    const p = open();
    fireEvent.click(button("Remove from Steam"));
    await flush();
    expect(p.onSave).toHaveBeenCalledWith(false, "Tidewalker");
  });

  it("chooses a file for the focused slot", async () => {
    const p = open();
    fireEvent.click(button(/^Hero/));
    fireEvent.click(button("Choose file…"));
    await flush();
    expect(screen.getByRole("dialog", { name: "Choose hero artwork" })).toBeTruthy();
    fireEvent.click(button(/cover\.png/));
    await flush();
    expect(p.onArtwork).toHaveBeenCalledWith("hero", "/home/deck/Pictures/cover.png");
  });

  it("a chosen image can go back to generated", async () => {
    const view = {
      ...VIEW,
      slots: VIEW.slots.map((s) => (s.slot === "capsule" ? { ...s, source: "chosen" as const } : s)),
    };
    const p = open(view);
    fireEvent.click(button(/^Capsule/));
    fireEvent.click(button("Use generated artwork"));
    await flush();
    expect(p.onArtwork).toHaveBeenCalledWith("capsule", null);
  });

  it("adding every new ROM is a whole-library setting", async () => {
    const p = open();
    expect(screen.getByText("This applies to your whole library.")).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: "Add every new ROM to Steam automatically" }));
    await flush();
    expect(p.onAutoAdd).toHaveBeenCalledWith(true);
  });
});

describe("restart prompt", () => {
  it("counts the changes and explains how to restart", () => {
    const onClose = vi.fn();
    render(<RestartSteamDialog changes={3} onClose={onClose} />);
    expect(screen.getByRole("dialog", { name: "Restart Steam to apply" })).toBeTruthy();
    expect(screen.getByText(/3 shortcut changes are ready\. They appear after Steam restarts\./)).toBeTruthy();
    fireEvent.click(button("How to restart"));
    expect(screen.getByText(/press the Steam button, choose Power, then Restart/)).toBeTruthy();
    fireEvent.click(button("Later"));
    expect(onClose).toHaveBeenCalled();
  });
});
