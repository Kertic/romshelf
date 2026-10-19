import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type LocationState =
  | { state: "drive_only" }
  | { state: "downloading"; percent: number }
  | { state: "on_deck" }
  | { state: "gone" };

export interface Rom {
  id: string;
  drive_file_id: string;
  system: string;
  file_name: string;
  display_name: string;
  size: number;
  checksum: string | null;
  location: LocationState;
  recognized: boolean;
}

export interface LibraryEntry {
  rom: Rom;
  system_label: string;
  system_colour: string;
  emulator: string | null;
  emulator_installed: boolean;
  in_steam: boolean;
}

export interface AppInfo {
  version: string;
  fake: boolean;
  data_dir: string;
}

/** Live pad state from the Rust `gilrs` input path. */
export interface GilrsSnapshot {
  pads: { id: number; name: string; pressed: string[]; axes: Record<string, number> }[];
  last_event: string;
}

export const inTauri = (): boolean => isTauri();

export async function appInfo(): Promise<AppInfo> {
  if (!inTauri()) return { version: "dev", fake: true, data_dir: "(browser preview)" };
  return invoke<AppInfo>("app_info");
}

export async function library(): Promise<LibraryEntry[]> {
  if (!inTauri()) return browserPreviewLibrary();
  return invoke<LibraryEntry[]>("library");
}

export async function quit(): Promise<void> {
  if (inTauri()) await invoke("quit");
  else window.close();
}

export async function onGilrs(handler: (s: GilrsSnapshot) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<GilrsSnapshot>("gilrs", (e) => handler(e.payload));
}

/** A small stand-in library so the shell can be previewed in a plain browser. */
function browserPreviewLibrary(): LibraryEntry[] {
  const rows: [string, string, string, string, LocationState, boolean][] = [
    ["Ember Circuit", "snes", "SNES", "#2B3F6B", { state: "on_deck" }, true],
    ["Tidewalker", "gba", "GBA", "#1F6B62", { state: "on_deck" }, true],
    ["Starfall Relay", "ps2", "PS2", "#6B3A2B", { state: "drive_only" }, false],
    ["Moss & Lantern", "gamecube", "GameCube", "#4A3470", { state: "on_deck" }, false],
  ];
  return rows.map(([name, system, label, colour, location, in_steam], i) => ({
    rom: {
      id: `${system}-0000000${i}`,
      drive_file_id: `preview-${i}`,
      system,
      file_name: name,
      display_name: name,
      size: 0,
      checksum: null,
      location,
      recognized: true,
    },
    system_label: label,
    system_colour: colour,
    emulator: system === "gamecube" ? "Dolphin" : system === "ps2" ? null : "RetroArch",
    emulator_installed: system !== "gamecube",
    in_steam,
  }));
}
