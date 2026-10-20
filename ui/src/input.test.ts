import { describe, expect, it } from "vitest";
import { browserPressed, gilrsPressed, isTyping, keyButton, PressTracker, type Button } from "./input";
import { pick } from "./focus";

const pad = (pressedIdx: number[], axes = [0, 0, 0, 0]) => ({
  buttons: Array.from({ length: 17 }, (_, i) => ({
    pressed: pressedIdx.includes(i),
    touched: false,
    value: pressedIdx.includes(i) ? 1 : 0,
  })),
  axes,
});

describe("input", () => {
  it("maps standard-mapping indices to buttons", () => {
    expect([...browserPressed(pad([0, 4, 12]))].sort()).toEqual(["a", "l1", "up"]);
  });

  it("treats the left stick as the D-pad", () => {
    expect([...browserPressed(pad([], [-0.9, 0.8, 0, 0]))].sort()).toEqual(["down", "left", "lstick"]);
    // The stick is reported first, then its direction.
    expect([...gilrsPressed([], { left_x: 0, left_y: 0.9 })]).toEqual(["lstick", "up"]);
    expect([...gilrsPressed([], { right_x: -0.8 })]).toEqual(["rstick"]);
  });

  it("ignores unknown gilrs names", () => {
    expect([...gilrsPressed(["a", "unknown"], {})]).toEqual(["a"]);
  });

  it("reports a press once and repeats held directions", () => {
    const t = new PressTracker();
    const s = (...b: Button[]) => new Set<Button>(b);
    expect(t.update(0, s("a"))).toEqual(["a"]);
    expect(t.update(16, s("a"))).toEqual([]);
    expect(t.update(32, s())).toEqual([]);
    expect(t.update(48, s("down"))).toEqual(["down"]);
    expect(t.update(300, s("down"))).toEqual([]);
    expect(t.update(460, s("down"))).toEqual(["down"]);
    expect(t.update(500, s("down"))).toEqual([]);
    expect(t.update(590, s("down"))).toEqual(["down"]);
    expect(t.update(600, s())).toEqual([]);
  });
});

describe("focus", () => {
  const r = (x: number, y: number) => new DOMRect(x, y, 100, 50);
  it("picks the nearest element in the pressed direction", () => {
    const from = r(0, 0);
    const cands = [r(200, 0), r(120, 0), r(0, 100), r(-150, 0)];
    expect(pick(from, cands, "right")).toBe(1);
    expect(pick(from, cands, "down")).toBe(2);
    expect(pick(from, cands, "left")).toBe(3);
    expect(pick(from, cands, "up")).toBe(-1);
  });
});

describe("keys while typing", () => {
  it("Backspace, Space, Enter and left/right edit text in a text field", () => {
    for (const key of ["Backspace", " ", "Enter", "ArrowLeft", "ArrowRight", "F9"]) {
      expect(keyButton(key, true)).toBeNull();
    }
    // Elsewhere they still navigate.
    expect(keyButton("Backspace", false)).toBe("b");
    expect(keyButton(" ", false)).toBe("a");
    expect(keyButton("ArrowLeft", false)).toBe("left");
  });

  it("Escape and up/down still leave the field", () => {
    expect(keyButton("Escape", true)).toBe("b");
    expect(keyButton("ArrowDown", true)).toBe("down");
    expect(keyButton("ArrowUp", true)).toBe("up");
  });

  it("knows what counts as a text field", () => {
    const text = document.createElement("input");
    const box = document.createElement("input");
    box.type = "checkbox";
    expect(isTyping(text)).toBe(true);
    expect(isTyping(box)).toBe(false);
    expect(isTyping(document.createElement("textarea"))).toBe(true);
    expect(isTyping(document.createElement("button"))).toBe(false);
    expect(isTyping(null)).toBe(false);
  });
});
