/** The "Restart Steam to apply" prompt with the gamepad alone: B is Later, like every dialog. */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";

vi.mock("./api", async (importOriginal) => {
  const real = await importOriginal<typeof import("./api")>();
  // A shortcut change is waiting for Steam to restart.
  return { ...real, steamRestartNotice: async () => true };
});

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
const tile = (name: string) =>
  [...document.querySelectorAll<HTMLElement>(".tile")].find((t) => t.textContent?.includes(name))!;

describe("restart prompt", () => {
  it("B closes it, gives focus back and the hint says Later while it's open", async () => {
    render(<App />);
    await settle();
    await settle();
    act(() => tile("Tidewalker").focus());
    await press("Enter");
    await settle();
    act(() => screen.getByRole("button", { name: "Edit shortcut…" }).focus());
    await press("Enter");
    await settle();
    act(() => screen.getByRole("button", { name: "Save shortcut" }).focus());
    await press("Enter");
    await settle();
    await press("Escape");
    await settle();
    expect(screen.getByRole("dialog", { name: "Restart Steam to apply" })).toBeTruthy();
    expect(screen.getByLabelText("Button hints").textContent).toContain("Later");
    await press("Escape");
    await settle();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.querySelector("h1")?.textContent).toBe("Tidewalker");
    expect(document.activeElement).not.toBe(document.body);
    // B is the page's own again: back to the Library.
    await press("Escape");
    await settle();
    expect(document.querySelector("h1")?.textContent).toBe("All systems");
  });
});
