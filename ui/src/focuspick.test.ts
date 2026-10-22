/** Which element a D-pad press picks, on real layouts measured in the app. */
import { describe, expect, it } from "vitest";
import { pick } from "./focus";

const rect = (left: number, top: number, width: number, height: number) =>
  ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top }) as DOMRect;

describe("pick", () => {
  // The ROM page's top row and the functions under it, at 1280×800.
  const play = rect(404, 28, 418, 56);
  const downloadAgain = rect(834, 28, 184, 56);
  const backUp = rect(404, 125, 273, 64);
  const restore = rect(689, 125, 273, 64);

  it("left stays in the row: Download again → Play, not the card below", () => {
    expect(pick(downloadAgain, [play, backUp, restore], "left")).toBe(0);
  });

  it("down still goes to the row below when nothing is beside", () => {
    expect(pick(play, [backUp, restore], "down")).toBe(0);
  });

  it("with nothing in line, the nearest that way", () => {
    expect(pick(downloadAgain, [backUp, restore], "left")).toBe(1);
  });

  it("up stays in the column when something is straight above", () => {
    const above = rect(689, 28, 120, 56);
    const aboveLeft = rect(560, 100, 100, 20);
    expect(pick(restore, [aboveLeft, above], "up")).toBe(1);
  });
});
