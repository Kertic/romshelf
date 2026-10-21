/**
 * Runs the real Romshelf binary in fake mode under tauri-driver and drives it with the keys the
 * gamepad maps to (Enter is A, Escape is B, PageUp/PageDown are L1/R1, F9–F12 the back grips).
 *
 * ROMSHELF_E2E_APP: the binary (default: the release build under target/).
 * ROMSHELF_E2E_NATIVE_DRIVER: msedgedriver.exe on Windows when it isn't on PATH.
 */
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Builder, Capabilities, Key } from "selenium-webdriver";

const here = path.dirname(fileURLToPath(import.meta.url));
export const OUT = path.join(here, "out");
const exe = process.platform === "win32" ? ".exe" : "";
const APP =
  process.env.ROMSHELF_E2E_APP ??
  path.join(process.env.CARGO_TARGET_DIR ?? path.join(here, "..", "target"), "release", `romshelf${exe}`);
const PORT = 4444;

export const KEYS = {
  A: Key.ENTER,
  B: Key.ESCAPE,
  L1: Key.PAGE_UP,
  R1: Key.PAGE_DOWN,
  UP: Key.ARROW_UP,
  DOWN: Key.ARROW_DOWN,
  LEFT: Key.ARROW_LEFT,
  RIGHT: Key.ARROW_RIGHT,
  L4: Key.F9,
  R4: Key.F10,
};

function portOpen() {
  return new Promise((resolve) => {
    const s = net.connect(PORT, "127.0.0.1");
    s.once("connect", () => (s.destroy(), resolve(true)));
    s.once("error", () => resolve(false));
  });
}

async function until(fn, ms, what) {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) throw new Error(`Timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

/**
 * Starts tauri-driver and Romshelf in fake mode (a temp root with the fixture library and a fake
 * Drive). `firstRun` starts signed out.
 */
export async function launch({ firstRun = false } = {}) {
  const args = [];
  if (process.env.ROMSHELF_E2E_NATIVE_DRIVER) args.push("--native-driver", process.env.ROMSHELF_E2E_NATIVE_DRIVER);
  const env = { ...process.env, ROMSHELF_FAKE: "1", ROMSHELF_WINDOWED: "1" };
  if (firstRun) env.ROMSHELF_FAKE_FIRST_RUN = "1";
  else delete env.ROMSHELF_FAKE_FIRST_RUN;
  const driverProc = spawn(process.env.TAURI_DRIVER ?? "tauri-driver", args, { env, stdio: ["ignore", "inherit", "inherit"] });
  await until(portOpen, 20_000, "tauri-driver");
  const caps = new Capabilities();
  caps.set("tauri:options", { application: APP });
  caps.setBrowserName("wry");
  const driver = await new Builder().withCapabilities(caps).usingServer(`http://127.0.0.1:${PORT}/`).build();
  const app = new App(driver);
  await app.waitFor(() => app.js("return document.querySelector('h1') != null"), "the first screen");
  return {
    app,
    async close() {
      await driver.quit().catch(() => {});
      driverProc.kill();
      await until(async () => !(await portOpen()), 10_000, "tauri-driver to stop");
    },
  };
}

export class App {
  constructor(driver) {
    this.driver = driver;
  }
  js(script, ...args) {
    return this.driver.executeScript(script, ...args);
  }
  waitFor(fn, what, ms = 15_000) {
    return until(fn, ms, what);
  }
  /** One button press, then a beat for React to render. */
  async press(key, times = 1) {
    for (let i = 0; i < times; i++) {
      await this.driver.actions().sendKeys(key).perform();
      await new Promise((r) => setTimeout(r, 120));
    }
  }
  /** Fake mode publishes the fixture games to its fake Steam at start, then asks for a restart: B. */
  async dismissStartupPrompt() {
    const asked = await this.waitFor(
      () => this.js("return document.querySelector('[role=dialog][aria-label=\"Restart Steam to apply\"]') != null"),
      "the restart prompt",
      10_000,
    ).catch(() => false);
    if (asked) {
      await this.press(KEYS.B);
      await this.waitFor(async () => !(await this.js("return document.querySelector('[role=dialog]') != null")), "the prompt to close");
    }
  }
  heading() {
    return this.js("return document.querySelector('h1')?.textContent ?? null");
  }
  async waitForHeading(text, ms) {
    await this.waitFor(async () => (await this.heading()) === text, `the heading "${text}"`, ms);
  }
  focused() {
    return this.js("return document.activeElement?.textContent?.trim() ?? ''");
  }
  /** Text of the visible button hints (the footer). */
  hints() {
    return this.js("return document.querySelector('[aria-label=\"Button hints\"]')?.textContent ?? ''");
  }
  bodyText() {
    return this.js("return document.body.innerText");
  }
  /**
   * Moves focus with the D-pad to the focusable element whose accessible name or text starts with
   * `label`: each press goes the way the target lies from what's focused, as a player would steer.
   */
  async navigateTo(label, { max = 30 } = {}) {
    for (let i = 0; i < max; i++) {
      const dir = await this.js(
        `const label = arguments[0];
        const name = (el) => [el.getAttribute("aria-label") ?? "", el.textContent.trim()];
        const all = [...document.querySelectorAll("[data-focusable]:not([disabled])")].filter(
          (el) => !el.closest("[inert], [aria-hidden='true'], [hidden]") && el.getClientRects().length);
        const target =
          all.find((el) => name(el).some((n) => n.startsWith(label))) ??
          all.find((el) => name(el).some((n) => n.includes(label)));
        if (!target) return "missing";
        const at = document.activeElement;
        if (at === target) return "here";
        if (!at || !all.includes(at)) return "down";
        const c = (r) => ({ x: r.left + r.width / 2, y: r.top + r.height / 2 });
        const t = c(target.getBoundingClientRect()), f = c(at.getBoundingClientRect());
        const dx = t.x - f.x, dy = t.y - f.y;
        const tr = target.getBoundingClientRect(), fr = at.getBoundingClientRect();
        const overlapY = Math.min(tr.bottom, fr.bottom) - Math.max(tr.top, fr.top) > 0;
        if (overlapY) return dx > 0 ? "right" : "left";
        return dy > 0 ? "down" : "up";`,
        label,
      );
      if (dir === "here") return;
      if (dir === "missing") throw new Error(`No focusable "${label}" on screen`);
      await this.press({ up: KEYS.UP, down: KEYS.DOWN, left: KEYS.LEFT, right: KEYS.RIGHT }[dir]);
    }
    throw new Error(`Couldn't reach "${label}" with the D-pad (focus on "${await this.focused()}")`);
  }
  async screenshot(name) {
    mkdirSync(OUT, { recursive: true });
    const png = Buffer.from(await this.driver.takeScreenshot(), "base64");
    writeFileSync(path.join(OUT, `${name}.png`), png);
    return png;
  }
}
