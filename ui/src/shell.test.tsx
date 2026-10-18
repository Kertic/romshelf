import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import { Footer } from "./Footer";
import { tileStatus } from "./Home";
import type { LibraryEntry } from "./api";

afterEach(cleanup);

describe("shell", () => {
  it("renders the app name, the test actions and the hint bar", async () => {
    render(<App />);
    await act(async () => {});
    expect(screen.getByText("Romshelf")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open gamepad test" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Quit Romshelf" })).toBeTruthy();
    expect(screen.getByLabelText("Button hints")).toBeTruthy();
  });

  it("shows the library tiles with their status", async () => {
    render(<App />);
    await act(async () => {});
    expect(screen.getByText("Tidewalker")).toBeTruthy();
    expect(screen.getByText("Needs Dolphin")).toBeTruthy();
  });

  it("footer lists each hint", () => {
    render(<Footer hints={[{ glyph: "A", label: "Select" }, { glyph: "B", label: "Back" }]} />);
    expect(screen.getByText("Select")).toBeTruthy();
    expect(screen.getByText("Back")).toBeTruthy();
  });
});

describe("tokens", () => {
  const css = readFileSync(resolve(__dirname, "tokens.css"), "utf8");
  const root = css.slice(css.indexOf(":root"));
  const expected: Record<string, string> = {
    "--bg": "#10161b",
    "--surface": "#172027",
    "--raised": "#1d2830",
    "--border-subtle": "#25313a",
    "--border": "#2e3b45",
    "--border-strong": "#3a4852",
    "--text": "#e8eef2",
    "--text-secondary": "#c3ced6",
    "--text-muted": "#9aa8b3",
    "--accent": "#3dd6b5",
    "--on-accent": "#0b1a17",
    "--warning": "#ffb27a",
    "--danger": "#ff9c8f",
    "--danger-border": "#6b3a36",
  };
  it.each(Object.entries(expected))("%s is %s on :root", (name, value) => {
    expect(root).toMatch(new RegExp(`${name}:\\s*${value};`, "i"));
  });

  it("fonts are bundled, not loaded from the network", () => {
    const main = readFileSync(resolve(__dirname, "main.tsx"), "utf8");
    expect(main).toContain("@fontsource/sora");
    expect(main).toContain("@fontsource/jetbrains-mono");
    const html = readFileSync(resolve(__dirname, "../index.html"), "utf8");
    expect(html).not.toMatch(/fonts\.googleapis|https?:\/\//);
  });
});

describe("tile status", () => {
  const base: LibraryEntry = {
    rom: {
      id: "gba-00000000",
      drive_file_id: "x",
      system: "gba",
      file_name: "a.gba",
      display_name: "A",
      size: 0,
      checksum: null,
      location: { state: "on_deck" },
      recognized: true,
    },
    system_label: "GBA",
    system_colour: "#1F6B62",
    emulator: "RetroArch",
    emulator_installed: true,
    in_steam: false,
  };
  it("follows the guide's wording", () => {
    expect(tileStatus(base).text).toBe("On Deck");
    expect(tileStatus({ ...base, in_steam: true }).text).toBe("On Deck · In Steam");
    expect(tileStatus({ ...base, rom: { ...base.rom, location: { state: "drive_only" } } }).text).toBe(
      "In Drive only",
    );
    expect(
      tileStatus({ ...base, rom: { ...base.rom, location: { state: "downloading", percent: 64 } } }).text,
    ).toBe("Downloading 64%");
    expect(tileStatus({ ...base, emulator: "Dolphin", emulator_installed: false })).toEqual({
      text: "Needs Dolphin",
      warn: true,
    });
    expect(tileStatus({ ...base, rom: { ...base.rom, recognized: false } }).text).toBe("Unrecognized file");
  });
});
