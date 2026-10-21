/**
 * Stage 9's end-to-end flows: the real app in fake mode, driven only by gamepad keys.
 * D-pad presses steer focus to each control (`navigateTo`); A, B, L1 and R1 do the rest.
 */
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { KEYS, launch } from "../harness.mjs";

describe("first run", () => {
  let s;
  before(async () => (s = await launch({ firstRun: true })));
  after(() => s?.close());

  it("1. sign in (faked) → choose location → Library", async () => {
    const { app } = s;
    await app.waitForHeading("Welcome to Romshelf");
    await app.press(KEYS.A); // Connect Google Drive
    await app.waitForHeading("Sign in to Google");
    assert.match(await app.focused(), /Open browser/);
    await app.press(KEYS.A);
    await app.waitForHeading("Choose where to keep your games");
    // The primary action waits for the folder list.
    await app.waitFor(async () => /Games/.test(await app.bodyText()), "the folders");
    await app.navigateTo("Create Emulation folder here");
    await app.press(KEYS.A);
    await app.waitForHeading("Your Drive is ready", 30_000);
    assert.match(await app.focused(), /Open Library/);
    await app.press(KEYS.A);
    await app.waitForHeading("All systems");
  });
});

describe("Library and a game", () => {
  let s;
  before(async () => {
    s = await launch();
    await s.app.dismissStartupPrompt();
  });
  after(() => s?.close());

  const tileStatus = (app, name) =>
    app.js(
      "return [...document.querySelectorAll('.tile')].find((t) => t.textContent.includes(arguments[0]))?.closest('.tile-wrap, li, div')?.textContent ?? ''",
      name,
    );

  it("2. R1 switches system → open a ROM → Download → Play (fake emulator) → back", async () => {
    const { app } = s;
    await app.waitForHeading("All systems");
    await app.press(KEYS.R1);
    await app.waitForHeading("SNES");
    await app.press(KEYS.R1);
    await app.waitForHeading("GBA");
    await app.navigateTo("Tidewalker");
    await app.press(KEYS.A);
    await app.waitForHeading("Tidewalker");
    await app.navigateTo("Download to Deck");
    await app.press(KEYS.A);
    await app.waitFor(async () => /On Deck/.test(await app.bodyText()), "the download", 30_000);
    await app.navigateTo("Play");
    await app.press(KEYS.A);
    // The fake emulator runs for a few seconds, then Romshelf is back on the page with Play.
    await app.waitFor(async () => /Playing|Starting|Running/.test(await app.bodyText()), "the game to start");
    await app.waitFor(
      async () => !/Playing|Starting|Running/.test(await app.bodyText()) && (await app.heading()) === "Tidewalker",
      "the game to end",
      30_000,
    );
    await app.press(KEYS.B);
    await app.waitForHeading("GBA");
    assert.match(await app.focused(), /Tidewalker/);
  });

  it("3. ROM page → Controls → rebind one button → Save → card shows the differences", async () => {
    const { app } = s;
    await app.navigateTo("Tidewalker");
    await app.press(KEYS.A);
    await app.waitForHeading("Tidewalker");
    await app.navigateTo("Edit controls");
    await app.press(KEYS.A);
    await app.waitForHeading("Controls");
    // R4 does Rewind by default. Press A while it listens: A is taken, so the two trade places
    // (the keyboard can't press a free input such as X, which would move it instead).
    await app.navigateTo("R4");
    await app.press(KEYS.A);
    await app.waitFor(async () => /Press a button/.test(await app.bodyText()), "listening");
    await app.press(KEYS.A);
    await app.waitFor(async () => /Swap buttons?/.test(await app.bodyText()), "the swap question");
    await app.navigateTo("Swap");
    await app.press(KEYS.A);
    await app.navigateTo("Save");
    await app.press(KEYS.A);
    await app.press(KEYS.B);
    await app.waitForHeading("Tidewalker");
    assert.match(await app.bodyText(), /2 buttons differ from default/);
  });

  it("4. ROM page → Steam shortcut → rename → Save → restart prompt appears once", async () => {
    const { app } = s;
    await app.navigateTo("Edit shortcut");
    await app.press(KEYS.A);
    await app.waitForHeading("Steam shortcut");
    await app.navigateTo("Name in Steam");
    await app.driver.actions().sendKeys(" (GBA)").perform();
    await app.press(KEYS.DOWN);
    await app.navigateTo("Save shortcut");
    await app.press(KEYS.A);
    await app.waitFor(async () => /Saved/.test(await app.bodyText()), "the save");
    // Leaving the shortcut screen asks once for the batch.
    const prompts = () => app.js("return document.querySelectorAll('[role=dialog][aria-label=\"Restart Steam to apply\"]').length");
    await app.press(KEYS.B);
    await app.waitFor(async () => (await prompts()) === 1, "the restart prompt");
    assert.match(await app.hints(), /Later/);
    await app.press(KEYS.B); // Later
    await app.waitFor(async () => (await prompts()) === 0, "the prompt to close");
    assert.equal(await app.heading(), "Tidewalker");
    // Not again: back into the shortcut screen and out with nothing changed.
    await app.navigateTo("Edit shortcut");
    await app.press(KEYS.A);
    await app.waitForHeading("Steam shortcut");
    await app.press(KEYS.B);
    await app.waitForHeading("Tidewalker");
    await new Promise((r) => setTimeout(r, 1500));
    assert.equal(await prompts(), 0);
  });

  it("5. ROM page (emulator missing) → Set up → page becomes ready", async () => {
    const { app } = s;
    // Back to the Library from wherever the last flow ended.
    for (let i = 0; i < 4 && (await app.heading()) !== "All systems" && (await app.heading()) !== "GBA"; i++) {
      await app.press(KEYS.B);
    }
    await app.press(KEYS.L1); // GBA → SNES → All
    await app.press(KEYS.L1);
    await app.waitForHeading("All systems");
    await app.navigateTo("Moss & Lantern");
    await app.press(KEYS.A);
    await app.waitForHeading("Moss & Lantern");
    assert.match(await app.focused(), /Set up Dolphin/);
    await app.press(KEYS.A);
    await app.waitFor(async () => !/Set up Dolphin/.test(await app.bodyText()), "Dolphin to be set up", 60_000);
    assert.match(await app.bodyText(), /Download to Deck|Play/);
  });

  it("6. Remove from this Deck → tile shows In Drive only", async () => {
    const { app } = s;
    await app.press(KEYS.B);
    await app.waitForHeading("All systems");
    await app.navigateTo("Tidewalker");
    await app.press(KEYS.A);
    await app.waitForHeading("Tidewalker");
    await app.navigateTo("Remove from Deck");
    await app.press(KEYS.A);
    await app.waitFor(async () => /Remove Tidewalker from this Deck\?/.test(await app.bodyText()), "the question");
    assert.equal(await app.focused(), "Cancel");
    await app.navigateTo("Remove");
    await app.press(KEYS.A);
    await app.waitFor(async () => /In Drive only/.test(await app.bodyText()), "the removal");
    await app.press(KEYS.B);
    await app.waitForHeading("All systems");
    assert.match(await tileStatus(app, "Tidewalker"), /In Drive only/);
  });
});
