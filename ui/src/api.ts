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
  /** The emulator's name, or null when Romshelf can't run this system yet. */
  emulator: string | null;
  emulator_id: string | null;
  /** RetroArch's core for this system ("mgba"). */
  core: string | null;
  emulator_installed: boolean;
  /** Installed and set up: the game can be played once downloaded. */
  emulator_ready: boolean;
  /** The emulator's own functions, for the ROM page. */
  actions: { id: string; label: string }[];
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

export interface ConnectionStatus {
  client_configured: boolean;
  signed_in: boolean;
  account: string | null;
  /** "My Drive / Games / Emulation" */
  drive_folder: string | null;
  ready: boolean;
}

export interface Folder {
  id: string;
  name: string;
}

export interface SetupDone {
  path: string;
  tree: string[];
}

/** An error from a Rust command: a kind to branch on and a sentence for the user. */
export interface UiError {
  kind: "no_browser" | "cancelled" | "timeout" | "offline" | "auth" | "needs_update" | "error";
  message: string;
}

export function asUiError(e: unknown): UiError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) return e as UiError;
  return { kind: "error", message: `Something went wrong: ${String(e)}. Try again.` };
}

/** In a plain browser, add `?first-run` to the address to preview the first-run screens. */
const previewFirstRun = (): boolean =>
  typeof location !== "undefined" && new URLSearchParams(location.search).has("first-run");

export async function connectionStatus(): Promise<ConnectionStatus> {
  if (!inTauri()) {
    const first = previewFirstRun();
    return {
      client_configured: true,
      signed_in: !first,
      account: first ? null : "player@example.com",
      drive_folder: first ? null : "My Drive / Emulation",
      ready: !first,
    };
  }
  return invoke<ConnectionStatus>("connection_status");
}

/** Opens the browser for Google sign-in and resolves with the account address. */
export async function authLogin(): Promise<string> {
  if (!inTauri()) {
    await new Promise((r) => setTimeout(r, 1500));
    return "player@example.com";
  }
  return invoke<string>("auth_login");
}

export async function authCancel(): Promise<void> {
  if (inTauri()) await invoke("auth_cancel");
}

const PREVIEW_FOLDERS: Record<string, Folder[]> = {
  root: [
    { id: "games", name: "Games" },
    { id: "photos", name: "Photos" },
    { id: "work", name: "Work" },
  ],
  games: [{ id: "retro", name: "Retro" }],
};

export async function driveFolders(parent: string): Promise<Folder[]> {
  if (!inTauri()) return PREVIEW_FOLDERS[parent] ?? [];
  return invoke<Folder[]>("drive_folders", { parent });
}

/** Creates the Emulation tree under `parent`; `location` is the parent's display path. */
export async function driveSetup(parent: string, location: string): Promise<SetupDone> {
  if (!inTauri()) {
    return {
      path: `${location} / Emulation`,
      tree: ["Emulation/", "  roms/      snes, gba, n64, ps2, gamecube", "  saves/", "  states/", "  bios/      snes, gba, n64, ps2, gamecube", "  .romshelf/"],
    };
  }
  return invoke<SetupDone>("drive_setup", { parent, location });
}

/** Each folder path as setup reaches it. */
export async function onSetupProgress(handler: (path: string) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<string>("setup-progress", (e) => handler(e.payload));
}

/** The last sync, as the top band shows it. */
export interface SyncStatus {
  state: "idle" | "syncing" | "synced" | "offline" | "error";
  /** Seconds since the epoch of the last successful sync. */
  last_synced_at: number | null;
  /** The summary line, or what went wrong. */
  message: string | null;
}

export async function syncStatus(): Promise<SyncStatus> {
  if (!inTauri()) return { state: "idle", last_synced_at: null, message: null };
  return invoke<SyncStatus>("sync_status");
}

/** Syncs the library with Drive and resolves with the outcome (failures are a status, not an error). */
export async function syncNow(): Promise<SyncStatus> {
  if (!inTauri()) {
    await new Promise((r) => setTimeout(r, 600));
    return { state: "synced", last_synced_at: Math.floor(Date.now() / 1000), message: "Synced 5 files. Nothing changed." };
  }
  return invoke<SyncStatus>("sync_now");
}

export async function onSyncStatus(handler: (s: SyncStatus) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<SyncStatus>("sync-status", (e) => handler(e.payload));
}

/** Fired when a sync changed the library, so it should be read again. */
export async function onLibraryChanged(handler: () => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen("library-changed", () => handler());
}

/** A download's live progress. */
export interface Progress {
  bytes: number;
  total: number;
  /** Bytes per second. */
  rate: number;
}

export type TransferEvent =
  | ({ kind: "progress"; rom_id: string } & Progress)
  | { kind: "finished"; rom_id: string; ok: boolean; message: string | null };

export interface StorageStatus {
  root: string;
  available: boolean;
  free: number | null;
}

/** Queues a download (or "Download again"). */
export async function download(romId: string): Promise<void> {
  if (inTauri()) await invoke("download", { romId });
}

export async function cancelDownload(romId: string): Promise<void> {
  if (inTauri()) await invoke("cancel_download", { romId });
}

/** Removes the downloaded copy from this Deck. Drive is never touched. */
export async function removeFromDeck(romId: string): Promise<void> {
  if (inTauri()) await invoke("remove_from_deck", { romId });
}

export async function storageStatus(): Promise<StorageStatus> {
  if (!inTauri()) return { root: "~/Romshelf", available: true, free: 40 * 1024 ** 3 };
  return invoke<StorageStatus>("storage_status");
}

export async function onTransfer(handler: (e: TransferEvent) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<TransferEvent>("transfer", (e) => handler(e.payload));
}

export type EmulatorStatus = "not_installed" | "setup_incomplete" | "installed";

export interface EmulatorView {
  id: string;
  name: string;
  /** System labels: "SNES", "GBA", "N64". */
  systems: string[];
  status: EmulatorStatus;
  status_label: string;
  /** Romshelf is setting it up right now. */
  setting_up: boolean;
}

/** Progress while an emulator is set up. */
export interface SetupUpdate {
  emulator_id: string;
  kind: "step" | "progress" | "done" | "failed";
  step: string | null;
  percent: number | null;
  message: string | null;
}

/** Every emulator with its status. Checks Flatpak, so it can take a moment. */
export async function emulators(): Promise<EmulatorView[]> {
  if (!inTauri()) {
    return [
      { id: "retroarch", name: "RetroArch", systems: ["SNES", "GBA", "N64"], status: "installed", status_label: "Installed", setting_up: false },
      { id: "dolphin", name: "Dolphin", systems: ["GameCube"], status: "not_installed", status_label: "Not installed", setting_up: false },
    ];
  }
  return invoke<EmulatorView[]>("emulators");
}

/** Starts installing and setting up an emulator in the background. */
export async function setupEmulator(emulatorId: string, repair = false): Promise<void> {
  if (inTauri()) await invoke("setup_emulator", { emulatorId, repair });
}

export async function onEmulatorSetup(handler: (u: SetupUpdate) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<SetupUpdate>("emulator-setup", (e) => handler(e.payload));
}

/** A newer Romshelf installed in the background this session; it runs from the next start. */
export async function updateReady(): Promise<string | null> {
  if (!inTauri()) return null;
  return invoke<string | null>("update_ready");
}

export async function onUpdateReady(handler: (version: string) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<string>("update-ready", (e) => handler(e.payload));
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
    ["Shadow the Hedgehog (USA).zip", "gamecube", "GameCube", "#4A3470", { state: "drive_only" }, false],
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
      recognized: !name.endsWith(".zip"),
    },
    system_label: label,
    system_colour: colour,
    emulator: system === "gamecube" ? "Dolphin" : system === "ps2" ? null : "RetroArch",
    emulator_id: system === "gamecube" ? "dolphin" : system === "ps2" ? null : "retroarch",
    core: system === "snes" ? "snes9x" : system === "gba" ? "mgba" : null,
    emulator_installed: system !== "gamecube",
    emulator_ready: system !== "gamecube" && system !== "ps2",
    actions:
      system === "gamecube"
        ? [{ id: "memory_cards", label: "Memory cards" }, { id: "save_states", label: "Save states" }, { id: "texture_packs", label: "Texture packs" }]
        : system === "ps2"
          ? []
          : [{ id: "save_states", label: "Save states" }, { id: "cheats", label: "Cheats" }, { id: "shaders", label: "Shaders" }],
    in_steam,
  }));
}
