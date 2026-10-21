import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { controlsView, type ControlsView, type Layout } from "./api";
import { Controls, type KeyHandler } from "./Controls";
import type { Button } from "./input";
import { difference, merge, rebind, rows } from "./layouts";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

/** The browser-preview view: Tidewalker on RetroArch GBA, with R4 cleared for this game. */
const VIEW: ControlsView = await controlsView("retroarch", "gba", "gba-00000001");
const DEFAULT_ONLY: ControlsView = { ...VIEW, game: null, game_diff: {} };

const bound = (action: string) => ({ kind: "bound" as const, action });

describe("layout rules", () => {
  it("merge and difference round trip", () => {
    const full = merge(VIEW.default, { a: bound("b"), r4: { kind: "cleared" } });
    const diff = difference(full, VIEW.default);
    expect(diff).toEqual({ a: bound("b"), r4: { kind: "cleared" } });
    expect(merge(VIEW.default, diff)).toEqual(full);
  });

  it("rows split game buttons from hotkeys and tag this game's changes", () => {
    const working = merge(VIEW.default, VIEW.game_diff);
    const r = rows(VIEW, working);
    expect(r.game.map((x) => x.input.id)).toEqual(["dpad", "left_stick", "a", "b", "l1", "r1", "menu", "view"]);
    const r4 = r.hotkeys.find((x) => x.input.id === "r4")!;
    expect(r4.target).toBeNull();
    expect(r4.custom).toBe(true);
    expect(r.hotkeys.find((x) => x.input.id === "l4")!.custom).toBe(false);
  });

  it("rebinding onto a used button swaps, onto a free one moves", () => {
    const r = rebind(VIEW, VIEW.default, "a", "b");
    expect(r.kind).toBe("swap");
    if (r.kind === "swap") {
      expect(r.other.label).toBe("B");
      expect(r.layout.a).toEqual(bound("b"));
      expect(r.layout.b).toEqual(bound("a"));
    }
    const moved = rebind(VIEW, VIEW.default, "l4", "r3");
    expect(moved).toEqual({ kind: "done", layout: { ...VIEW.default, r3: bound("fast_forward"), l4: { kind: "cleared" } } });
  });

  it("directions need a direction", () => {
    expect(rebind(VIEW, VIEW.default, "dpad", "a")).toEqual({
      kind: "invalid",
      message: "D-pad needs the D-pad or a stick. Press one of those.",
    });
    expect(rebind(VIEW, VIEW.default, "a", "right_stick")).toEqual({
      kind: "invalid",
      message: "A needs a button, not Right stick.",
    });
  });
});

function editor(view: ControlsView, held: Button[] = []) {
  let keys: KeyHandler | null = null;
  const onSave = vi.fn<(scope: "game" | "default", layout: Layout) => Promise<void>>(async () => {});
  const props = {
    view,
    held,
    registerKeys: (h: KeyHandler | null) => {
      keys = h;
    },
    setBack: vi.fn(),
    onBack: vi.fn(),
    onSave,
    onPromote: vi.fn(async () => {}),
    promoteCount: vi.fn(async () => 2),
  };
  const utils = render(<Controls {...props} />);
  const press = (b: Button, from: "pad" | "keyboard" = "pad") => act(() => void keys?.(b, from));
  return { ...props, utils, press, rerender: (p: Partial<typeof props>) => utils.rerender(<Controls {...props} {...p} />) };
}

const row = (input: string) => document.querySelector<HTMLElement>(`[data-input="${input}"]`)!;

describe("controls editor", () => {
  it("tags rows Default or This game, and hides tags for the default", () => {
    editor(VIEW);
    expect(row("a").textContent).toContain("Default");
    expect(row("r4").textContent).toContain("Not bound");
    expect(row("r4").textContent).toContain("This game");
    cleanup();
    editor(DEFAULT_ONLY);
    expect(row("a").textContent).not.toContain("Default");
    expect(screen.queryByRole("tab", { name: /only/ })).toBeNull();
  });

  it("rebinds with the next press, asks before swapping, and saves the difference", async () => {
    const e = editor(VIEW);
    fireEvent.click(row("a"));
    expect(row("a").textContent).toContain("Press a button…");
    e.press("b");
    // B tapped (released before a second): it's the new input.
    e.rerender({ held: ["b"] });
    e.rerender({ held: [] });
    await act(async () => {
      await new Promise((r) => setTimeout(r));
    });
    expect(screen.getByRole("dialog", { name: "Swap buttons?" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Swap" }));
    expect(row("b").textContent).toContain("A");
    expect(row("b").textContent).toContain("This game");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await act(async () => {});
    expect(e.onSave).toHaveBeenCalledWith("game", { r4: { kind: "cleared" }, a: bound("b"), b: bound("a") });
  });

  it("holding B for a second cancels listening", () => {
    vi.useFakeTimers();
    const e = editor(VIEW);
    fireEvent.click(row("l1"));
    e.press("b");
    e.rerender({ held: ["b"] });
    act(() => vi.advanceTimersByTime(1000));
    expect(row("l1").textContent).toContain("L shoulder");
    expect(row("l1").textContent).not.toContain("Press a button…");
  });

  it("X clears a row, Y returns it to the default, L1 and R1 switch scope", () => {
    const e = editor(VIEW);
    act(() => row("a").focus());
    e.press("x");
    expect(row("a").textContent).toContain("Not bound");
    act(() => row("a").focus());
    e.press("y");
    expect(row("a").textContent).toContain("A");
    expect(row("a").textContent).toContain("Default");
    e.press("r1");
    expect(screen.getByRole("tab", { name: "RetroArch GBA default" }).getAttribute("aria-selected")).toBe("true");
  });

  it("a stick press binds the stick, not the D-pad", () => {
    const e = editor(VIEW);
    fireEvent.click(row("dpad"));
    e.press("rstick");
    expect(row("right_stick").textContent).toContain("D-pad");
    expect(row("dpad").textContent).toContain("Not bound");
  });

  it("a back grip arrives as its key", () => {
    const e = editor(VIEW);
    fireEvent.click(row("l1"));
    e.press("l5");
    expect(screen.getByRole("dialog", { name: "Swap buttons?" })).toBeTruthy();
  });

  it("an unbound row offers what it could do", () => {
    editor(VIEW);
    fireEvent.click(row("x"));
    const dialog = screen.getByRole("dialog", { name: "What should X do?" });
    expect(dialog.textContent).toContain("Fast forward");
    expect(dialog.textContent).not.toContain("D-pad");
    fireEvent.click(screen.getByRole("button", { name: /Rewind/ }));
    expect(row("x").textContent).toContain("Rewind");
  });

  it("making it the default names how many games change", async () => {
    const e = editor(VIEW);
    fireEvent.click(screen.getByRole("button", { name: "Make this the RetroArch GBA default" }));
    await act(async () => {});
    expect(screen.getByRole("dialog", { name: "Make this the RetroArch GBA default?" }).textContent).toContain(
      "2 games use the default and will change.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Make default" }));
    expect(e.onPromote).toHaveBeenCalledWith({ r4: { kind: "cleared" } });
  });

  it("leaving with unsaved changes asks", () => {
    const e = editor(VIEW);
    act(() => row("a").focus());
    e.press("x");
    fireEvent.click(screen.getByRole("button", { name: /Tidewalker/ }));
    expect(screen.getByRole("dialog", { name: "Save changes?" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(e.onBack).toHaveBeenCalled();
  });
});
