/**
 * Visual regression: the five reference screens (docs/kit/ui/screens) in fake mode, compared
 * with the app's own baselines in screens/<platform>/. The reference images show a different
 * library (eight games, other counts) and predate later decisions (Transfers and Settings in the
 * top band; "Remove from Deck" for "Delete"), so they stay a by-eye reference; the baselines
 * catch any change from what was last accepted.
 *
 * ROMSHELF_E2E_UPDATE=1 writes new baselines instead of comparing.
 */
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { after, before, describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";
import { KEYS, OUT, launch } from "../harness.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const BASELINES = path.join(here, "..", "screens", process.platform);
const UPDATE = process.env.ROMSHELF_E2E_UPDATE === "1";
/** Share of pixels allowed to differ: room for font hinting, none for a moved or missing control. */
const TOLERANCE = 0.005;

function compare(name, png) {
  const file = path.join(BASELINES, `${name}.png`);
  if (UPDATE) {
    mkdirSync(BASELINES, { recursive: true });
    writeFileSync(file, png);
    return;
  }
  assert.ok(existsSync(file), `No baseline for ${name} on ${process.platform}: run with ROMSHELF_E2E_UPDATE=1 and review out/${name}.png`);
  const want = PNG.sync.read(readFileSync(file));
  const got = PNG.sync.read(png);
  assert.equal(`${got.width}×${got.height}`, `${want.width}×${want.height}`, `${name}: size changed`);
  const diff = new PNG({ width: want.width, height: want.height });
  const changed = pixelmatch(want.data, got.data, diff.data, want.width, want.height, { threshold: 0.1 });
  const share = changed / (want.width * want.height);
  if (share > TOLERANCE) writeFileSync(path.join(OUT, `${name}.diff.png`), PNG.sync.write(diff));
  assert.ok(share <= TOLERANCE, `${name}: ${(share * 100).toFixed(2)}% of pixels changed (see out/${name}.diff.png)`);
}

describe("reference screens", () => {
  let s;
  before(async () => {
    s = await launch();
    const { app } = s;
    await app.dismissStartupPrompt();
    await app.waitFor(async () => !/Syncing/.test(await app.bodyText()), "the first sync", 30_000);
    // Nothing that moves: no transitions, no blinking caret, and the sync time ("synced 1 min
    // ago") hidden, the one text that changes with the clock.
    await app.js(`const st = document.createElement("style");
      st.textContent = "*, *::before, *::after { animation: none !important; transition: none !important; caret-color: transparent !important; } .sync-status { visibility: hidden !important; }";
      document.head.append(st);`);
  });
  after(() => s?.close());

  const shot = async (name) => {
    await new Promise((r) => setTimeout(r, 400));
    compare(name, await s.app.screenshot(name));
  };

  it("01 Library, a game focused", async () => {
    await s.app.waitForHeading("All systems");
    await s.app.navigateTo("Tidewalker");
    await shot("01-library");
  });

  it("02 ROM page", async () => {
    await s.app.press(KEYS.A);
    await s.app.waitForHeading("Tidewalker");
    await shot("02-rom-page");
  });

  it("03 Controls", async () => {
    await s.app.navigateTo("Edit controls");
    await s.app.press(KEYS.A);
    await s.app.waitForHeading("Controls");
    await shot("03-controls");
    await s.app.press(KEYS.B);
    await s.app.waitForHeading("Tidewalker");
  });

  it("04 Steam shortcut", async () => {
    await s.app.navigateTo("Edit shortcut");
    await s.app.press(KEYS.A);
    await s.app.waitForHeading("Steam shortcut");
    await shot("04-steam-shortcut");
    await s.app.press(KEYS.B);
    await s.app.waitForHeading("Tidewalker");
  });

  it("02b ROM page, emulator missing", async () => {
    await s.app.press(KEYS.B);
    await s.app.waitForHeading("All systems");
    await s.app.navigateTo("Moss & Lantern");
    await s.app.press(KEYS.A);
    await s.app.waitForHeading("Moss & Lantern");
    await shot("02b-rom-page-emulator-missing");
  });
});
