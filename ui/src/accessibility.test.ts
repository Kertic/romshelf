import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const read = (name: string) =>
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), name), "utf8").replace(/\r\n/g, "\n");
const tokens = read("tokens.css");
const css = read("app.css");

function token(name: string): string {
  const m = tokens.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6})`));
  if (!m) throw new Error(`no --${name}`);
  return m[1]!;
}

/** WCAG relative luminance and contrast ratio. */
function luminance(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const [r, g, b] = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

describe("contrast", () => {
  // Every text colour on every surface it appears on.
  const texts = ["text", "text-secondary", "text-muted", "accent", "warning", "danger"];
  const surfaces = ["bg", "surface", "raised"];
  for (const t of texts) {
    for (const s of surfaces) {
      it(`--${t} on --${s} is at least 4.5:1`, () => {
        expect(contrast(token(t), token(s))).toBeGreaterThanOrEqual(4.5);
      });
    }
  }
  it("text on the accent (primary buttons) is at least 4.5:1", () => {
    expect(contrast(token("on-accent"), token("accent"))).toBeGreaterThanOrEqual(4.5);
  });
  it("white initials on every system colour are at least 4.5:1", () => {
    for (const s of ["snes", "gba", "n64", "ps2", "gamecube"]) {
      expect(contrast("#ffffff", token(`system-${s}`)), s).toBeGreaterThanOrEqual(4.5);
    }
  });
});

describe("targets", () => {
  /** The rule block for a selector, as written in app.css. */
  const block = (sel: string) => {
    const at = css.indexOf(`\n${sel} {`);
    if (at < 0) throw new Error(`no ${sel}`);
    return css.slice(at, css.indexOf("}", at));
  };
  const tall = /(min-height|height):\s*(var\(--(target-min|primary-height|row-height)\)|(4[4-9]|[5-9]\d|\d{3})px)/;
  for (const sel of [".button", ".tab", ".top-button", ".row", ".sync-status", ".back-link", ".function-row", ".function-card", ".key", ".link-button"]) {
    it(`${sel} is at least 44 px tall`, () => {
      expect(block(sel)).toMatch(tall);
    });
  }
  it("motion stops when the system asks for less", () => {
    expect(css).toContain("@media (prefers-reduced-motion: reduce)");
  });
});
