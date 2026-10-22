/**
 * Whole-app flows on the browser-preview data, driven only by the keys the gamepad maps to
 * (Enter is A, Escape is B, PageUp/PageDown are L1/R1). The stage file asks for these as
 * WebDriver tests in fake mode; until that harness exists (PROGRESS.md), they run here.
 */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";

afterEach(cleanup);

const press = async (key: string) => {
  await act(async () => {
    fireEvent.keyDown(window, { key });
    await new Promise((r) => setTimeout(r, 20));
  });
};
const settle = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });
const heading = () => document.querySelector("h1")?.textContent;
const tile = (name: string) =>
  [...document.querySelectorAll<HTMLElement>(".tile")].find((t) => t.textContent?.includes(name))!;

async function start() {
  render(<App />);
  await settle();
}

describe("flows with the gamepad alone", () => {
  it("Library: R1 switches system, A opens a game, B comes back to its tile", async () => {
    await start();
    expect(heading()).toBe("All systems");
    await press("PageDown");
    expect(heading()).toBe("SNES");
    await press("PageUp");
    expect(heading()).toBe("All systems");
    act(() => tile("Tidewalker").focus());
    await press("Enter");
    await settle();
    expect(heading()).toBe("Tidewalker");
    expect(screen.getByRole("button", { name: "Play" })).toBeTruthy();
    await press("Escape");
    await settle();
    expect(heading()).toBe("All systems");
    expect(document.activeElement).toBe(tile("Tidewalker"));
  });

  it("ROM page → Controls → Back, and → Steam shortcut → Save", async () => {
    await start();
    act(() => tile("Tidewalker").focus());
    await press("Enter");
    await settle();
    act(() => screen.getByRole("button", { name: "Edit controls" }).focus());
    await press("Enter");
    await settle();
    expect(heading()).toBe("Controls");
    await press("Escape");
    await settle();
    expect(heading()).toBe("Tidewalker");
    act(() => screen.getByRole("button", { name: "Edit shortcut…" }).focus());
    await press("Enter");
    await settle();
    expect(heading()).toBe("Steam shortcut");
    act(() => screen.getByRole("button", { name: "Save shortcut" }).focus());
    await press("Enter");
    await settle();
    expect(screen.getByText("Saved. Restart Steam to see the change.")).toBeTruthy();
  });

  it("a game whose emulator is missing offers Set up first", async () => {
    await start();
    act(() => tile("Moss & Lantern").focus());
    await press("Enter");
    await settle();
    expect(screen.getByRole("button", { name: "Set up Dolphin" }).hasAttribute("data-autofocus")).toBe(true);
  });

  it("the drawer follows the focused tile", async () => {
    await start();
    act(() => tile("Ember Circuit").focus());
    await settle();
    expect(document.querySelector(".drawer-title")?.textContent).toBe("Ember Circuit");
    act(() => tile("Starfall Relay").focus());
    await settle();
    expect(document.querySelector(".drawer-title")?.textContent).toBe("Starfall Relay");
  });

  it("Settings from the top band, and B back to the Library", async () => {
    await start();
    act(() => screen.getByRole("button", { name: "Settings" }).focus());
    await press("Enter");
    await settle();
    expect(heading()).toBe("Settings");
    await press("Escape");
    await settle();
    expect(heading()).toBe("All systems");
  });

  it("removing a game's copy from the drawer opens the page with the question, Cancel first", async () => {
    await start();
    act(() => tile("Tidewalker").focus());
    await settle();
    act(() => screen.getByRole("button", { name: "Remove from Deck…" }).focus());
    await press("Enter");
    await settle();
    expect(screen.getByRole("dialog", { name: "Remove Tidewalker from this Deck?" })).toBeTruthy();
    await settle();
    expect(document.activeElement?.textContent).toBe("Cancel");
    await press("Escape");
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(heading()).toBe("Tidewalker");
    // Focus stays visible when the question closes, on the page's own controls.
    expect(document.activeElement).not.toBe(document.body);
    expect(document.activeElement?.closest("[data-dialog]")).toBeNull();
  });

  it("closing Transfers puts focus back on the Transfers button", async () => {
    await start();
    await settle();
    const transfers = screen.getByRole("button", { name: "Transfers" });
    act(() => transfers.focus());
    await press("Enter");
    await settle();
    const panel = screen.getByRole("dialog", { name: "Transfers" });
    expect(panel.contains(document.activeElement)).toBe(true);
    await press("Escape");
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Transfers" }));
    // The hint bar follows focus back, rather than still offering the panel's Close.
    expect(screen.getByLabelText("Button hints").textContent).not.toMatch(/Close/);
  });
});
