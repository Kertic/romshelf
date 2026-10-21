/** Settings → Run the self-test, in the whole app: the tour visits each screen and comes back. */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";

afterEach(cleanup);

const settle = (ms = 50) =>
  act(async () => {
    await new Promise((r) => setTimeout(r, ms));
  });
const press = async (key: string) => {
  await act(async () => {
    fireEvent.keyDown(window, { key });
    await new Promise((r) => setTimeout(r, 20));
  });
};

describe("self-test in the app", () => {
  it("opens from Settings, tours every screen, and comes back for the guided steps", async () => {
    render(<App />);
    await settle();
    act(() => screen.getByRole("button", { name: "Settings" }).focus());
    await press("Enter");
    await settle();
    act(() => screen.getByRole("button", { name: "Run the self-test" }).focus());
    await press("Enter");
    // The tour shows seven screens, waiting for each to settle.
    for (let i = 0; i < 400 && !document.querySelector("h2")?.textContent?.startsWith("Step 1"); i++) await settle(100);
    expect(document.querySelector("h1")?.textContent).toBe("Self-test");
    expect(document.querySelector("h2")?.textContent).toBe("Step 1 of 6: Buttons");
    const results = screen.getByRole("list", { name: "Results" }).textContent ?? "";
    for (const name of ["Interface / Library", "Interface / Transfers", "Interface / Closing a panel", "Interface / ROM page", "Interface / Controls", "Interface / Steam shortcut", "Interface / Emulators", "Interface / Settings"]) {
      expect(results).toContain(name);
    }
    expect(results).not.toContain("didn't open");
    // B is Settings again once the tour is back.
    expect(screen.getByLabelText("Button hints").textContent).toContain("Settings");
  }, 60_000);
});
