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
  /** Last backup, and whether one is owed. */
  save: SaveSync;
  /** Romshelf keeps this game's save states (RetroArch). */
  keeps_states: boolean;
  /** How many inputs this game maps differently from its emulator's default. */
  control_differences: number;
}

/** Where a game's save stands against Drive. */
export interface SaveSync {
  last_synced_hash: string | null;
  /** Seconds since the epoch. */
  last_backup_at: number | null;
  last_backup_device: string | null;
  /** A backup is owed: the game was played and its save hasn't reached Drive yet. */
  pending: boolean;
  last_error: string | null;
}

/** A backup in Drive. */
export interface SaveVersion {
  id: string;
  taken_at: number;
  device: string;
  size: number;
  emulator: string;
  core: string | null;
  hash: string;
}

export type BackupOutcome = { kind: "uploaded"; version: SaveVersion } | { kind: "unchanged" } | { kind: "no_save" };

export interface RestoreOutcome {
  version: SaveVersion;
  /** Set when the save was made with another emulator or core. */
  warning: string | null;
}

export interface SaveSide {
  taken_at: number;
  device: string;
}

/** This Deck and Drive both have newer saves (UI guide section 7). */
export interface SaveConflict {
  rom_id: string;
  name: string;
  deck: SaveSide;
  drive: SaveSide;
}

export type SaveChoice = "keep_deck" | "use_drive" | "cancel";

export interface StateSlot {
  name: string;
  size: number;
  modified: number;
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

/** Progress and outcome while a game is prepared and played. */
export interface LaunchUpdate {
  rom_id: string;
  kind: "step" | "progress" | "notice" | "question" | "finished" | "failed";
  step: "download" | "setup" | "save_check" | "controls" | "run" | "backup" | null;
  /** "Downloading", "Setting up Dolphin", … */
  label: string | null;
  percent: number | null;
  /** The emulator's exit code, when finished. */
  code: number | null;
  message: string | null;
  /** The save conflict to answer ("question"). */
  conflict: SaveConflict | null;
}

/** Starts a game: downloads it and sets up its emulator if needed, then runs it. */
export async function play(romId: string): Promise<void> {
  if (inTauri()) await invoke("play", { romId });
}

/** The game `romshelf launch` opened this window for, or null for the full interface. */
export async function launchTarget(): Promise<string | null> {
  if (!inTauri()) {
    const id = new URLSearchParams(location.search).get("launch");
    return id;
  }
  return invoke<string | null>("launch_target");
}

export async function onLaunchUpdate(handler: (u: LaunchUpdate) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<LaunchUpdate>("launch-update", (e) => handler(e.payload));
}

/** Answers the save question a launch is waiting on. */
export async function answerSaveQuestion(choice: SaveChoice): Promise<void> {
  if (inTauri()) await invoke("answer_save_question", { choice });
}

const PREVIEW_VERSIONS: SaveVersion[] = [
  { id: "v2", taken_at: Math.floor(Date.now() / 1000) - 3600, device: "steamdeck", size: 32768, emulator: "retroarch", core: "mgba", hash: "b" },
  { id: "v1", taken_at: Math.floor(Date.now() / 1000) - 3 * 86400, device: "steamdeck", size: 32768, emulator: "retroarch", core: "mgba", hash: "a" },
];

/** Backups in Drive, newest first. */
export async function saveVersions(romId: string): Promise<SaveVersion[]> {
  if (!inTauri()) return PREVIEW_VERSIONS;
  return invoke<SaveVersion[]>("save_versions", { romId });
}

/** "Back up save data". */
export async function backupSave(romId: string): Promise<BackupOutcome> {
  if (!inTauri()) return { kind: "uploaded", version: PREVIEW_VERSIONS[0]! };
  return invoke<BackupOutcome>("backup_save", { romId });
}

/** Replaces the Deck's save with a backup; the current save is backed up first. */
export async function restoreSave(romId: string, versionId: string): Promise<RestoreOutcome> {
  if (!inTauri()) return { version: PREVIEW_VERSIONS.find((v) => v.id === versionId) ?? PREVIEW_VERSIONS[0]!, warning: null };
  return invoke<RestoreOutcome>("restore_save", { romId, versionId });
}

export async function saveStates(romId: string): Promise<StateSlot[]> {
  if (!inTauri()) return [{ name: "Tidewalker.state1", size: 512 * 1024, modified: Math.floor(Date.now() / 1000) - 600 }];
  return invoke<StateSlot[]>("save_states", { romId });
}

/** Deletes a save state from the Deck; its Drive copy goes to Drive's trash. */
export async function deleteSaveState(romId: string, slot: string): Promise<void> {
  if (inTauri()) await invoke("delete_save_state", { romId, slot });
}

/** "Backed up Tidewalker", for a toast, when an owed backup reaches Drive in the background. */
export async function onSaveBackedUp(handler: (message: string) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<string>("save-backed-up", (e) => handler(e.payload));
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
    save: {
      last_synced_hash: null,
      last_backup_at: system === "gba" ? Math.floor(Date.now() / 1000) - 3600 : null,
      last_backup_device: system === "gba" ? "steamdeck" : null,
      pending: false,
      last_error: null,
    },
    keeps_states: system !== "gamecube" && system !== "ps2",
    control_differences: system === "gba" ? 1 : 0,
  }));
}

/** A binding: a target, or explicitly nothing. */
export type Binding = { kind: "bound"; action: string } | { kind: "cleared" };
/** Deck input id → binding. */
export type Layout = Record<string, Binding>;

export interface InputView {
  id: string;
  label: string;
  directional: boolean;
  back_grip: boolean;
}

export interface TargetView {
  id: string;
  label: string;
  kind: "game" | "hotkey";
  directional: boolean;
}

/** One emulator's layout for one system, and one game's differences when a game is given. */
export interface ControlsView {
  emulator_id: string;
  emulator: string;
  system: string;
  system_label: string;
  game: { id: string; name: string } | null;
  inputs: InputView[];
  targets: TargetView[];
  default: Layout;
  game_diff: Layout;
}

export async function controlsView(emulatorId: string, system: string, romId: string | null): Promise<ControlsView> {
  if (!inTauri()) return previewControls(emulatorId, system, romId);
  return invoke<ControlsView>("controls_view", { emulatorId, system, romId });
}

/** Saves the default (no game: the full layout) or the game's differences. */
export async function saveControls(emulatorId: string, system: string, romId: string | null, layout: Layout): Promise<void> {
  if (inTauri()) await invoke("save_controls", { emulatorId, system, romId, layout });
}

export async function resetControls(emulatorId: string, system: string, romId: string): Promise<void> {
  if (inTauri()) await invoke("reset_controls", { emulatorId, system, romId });
}

export async function promoteCount(emulatorId: string, system: string, romId: string, layout: Layout): Promise<number> {
  if (!inTauri()) return 2;
  return invoke<number>("promote_count", { emulatorId, system, romId, layout });
}

export async function promoteControls(emulatorId: string, system: string, romId: string, layout: Layout): Promise<void> {
  if (inTauri()) await invoke("promote_controls", { emulatorId, system, romId, layout });
}

function previewControls(emulatorId: string, system: string, romId: string | null): ControlsView {
  const b = (action: string): Binding => ({ kind: "bound", action });
  const c: Binding = { kind: "cleared" };
  const inputs: [string, string, boolean, boolean][] = [
    ["dpad", "D-pad", true, false], ["left_stick", "Left stick", true, false], ["right_stick", "Right stick", true, false],
    ["a", "A", false, false], ["b", "B", false, false], ["x", "X", false, false], ["y", "Y", false, false],
    ["l1", "L1", false, false], ["r1", "R1", false, false], ["l2", "L2", false, false], ["r2", "R2", false, false],
    ["l3", "L3", false, false], ["r3", "R3", false, false], ["menu", "Menu", false, false], ["view", "View", false, false],
    ["l4", "L4", false, true], ["r4", "R4", false, true], ["l5", "L5", false, true], ["r5", "R5", false, true],
  ];
  return {
    emulator_id: emulatorId,
    emulator: "RetroArch",
    system,
    system_label: "GBA",
    game: romId ? { id: romId, name: "Tidewalker" } : null,
    inputs: inputs.map(([id, label, directional, back_grip]) => ({ id, label, directional, back_grip })),
    targets: [
      { id: "dpad", label: "D-pad", kind: "game", directional: true },
      { id: "a", label: "A", kind: "game", directional: false },
      { id: "b", label: "B", kind: "game", directional: false },
      { id: "l", label: "L shoulder", kind: "game", directional: false },
      { id: "r", label: "R shoulder", kind: "game", directional: false },
      { id: "start", label: "Start", kind: "game", directional: false },
      { id: "select", label: "Select", kind: "game", directional: false },
      { id: "fast_forward", label: "Fast forward", kind: "hotkey", directional: false },
      { id: "rewind", label: "Rewind", kind: "hotkey", directional: false },
      { id: "save_state", label: "Save state", kind: "hotkey", directional: false },
      { id: "load_state", label: "Load state", kind: "hotkey", directional: false },
      { id: "menu", label: "Open menu", kind: "hotkey", directional: false },
      { id: "quit", label: "Quit game", kind: "hotkey", directional: false },
    ],
    default: {
      dpad: b("dpad"), left_stick: b("dpad"), right_stick: c, a: b("a"), b: b("b"), x: c, y: c,
      l1: b("l"), r1: b("r"), l2: c, r2: c, l3: c, r3: c, menu: b("start"), view: b("select"),
      l4: b("fast_forward"), r4: b("rewind"), l5: b("save_state"), r5: b("load_state"),
    },
    game_diff: romId ? { r4: c } : {},
  };
}

export type ArtSlot = "capsule" | "wide_capsule" | "hero" | "logo" | "icon";

export interface SlotView {
  slot: ArtSlot;
  label: string;
  /** The user's file, Romshelf's generated artwork, or nothing (Logo only). */
  source: "chosen" | "generated" | "missing";
  preview: string | null;
}

/** A game's Steam shortcut, for the shortcut screen (UI guide 5.5). */
export interface ShortcutView {
  rom_id: string;
  shown: boolean;
  name: string;
  default_name: string;
  target: string;
  launch_options: string;
  slots: SlotView[];
  auto_add: boolean;
}

/** What writing to Steam changed. */
export interface Applied {
  added: number;
  updated: number;
  removed: number;
  artwork: number;
  restart_needed: boolean;
}

export interface ImageFile {
  path: string;
  name: string;
  folder: string;
}

const previewSlots = (): SlotView[] =>
  (["capsule", "wide_capsule", "hero", "logo", "icon"] as ArtSlot[]).map((slot) => ({
    slot,
    label: { capsule: "Capsule", wide_capsule: "Wide capsule", hero: "Hero", logo: "Logo", icon: "Icon" }[slot],
    source: slot === "logo" ? "missing" : "generated",
    preview: null,
  }));

export async function steamShortcut(romId: string): Promise<ShortcutView> {
  if (!inTauri()) {
    return {
      rom_id: romId,
      shown: true,
      name: "Tidewalker",
      default_name: "Tidewalker",
      target: "/home/deck/Applications/romshelf.AppImage",
      launch_options: `launch --id ${romId}`,
      slots: previewSlots(),
      auto_add: false,
    };
  }
  return invoke<ShortcutView>("steam_shortcut", { romId });
}

/** Saves "show in Steam" and the name, and writes Steam's files. */
export async function saveShortcut(romId: string, shown: boolean, name: string): Promise<Applied> {
  if (!inTauri()) return { added: shown ? 1 : 0, updated: 0, removed: shown ? 0 : 1, artwork: 4, restart_needed: true };
  return invoke<Applied>("save_shortcut", { romId, shown, name });
}

/** Uses an image for a slot; `null` goes back to the generated artwork. */
export async function setShortcutArtwork(romId: string, slot: ArtSlot, path: string | null): Promise<ShortcutView> {
  if (!inTauri()) return steamShortcut(romId);
  return invoke<ShortcutView>("set_shortcut_artwork", { romId, slot, path });
}

export async function setSteamAutoAdd(on: boolean): Promise<Applied> {
  if (!inTauri()) return { added: 0, updated: 0, removed: 0, artwork: 0, restart_needed: false };
  return invoke<Applied>("set_steam_auto_add", { on });
}

/** True once per batch of changes Steam hasn't picked up yet. */
export async function steamRestartNotice(): Promise<boolean> {
  if (!inTauri()) return false;
  return invoke<boolean>("steam_restart_notice");
}

export async function imageFiles(): Promise<ImageFile[]> {
  if (!inTauri()) return [{ path: "/home/deck/Pictures/tidewalker.png", name: "tidewalker.png", folder: "Pictures" }];
  return invoke<ImageFile[]>("image_files");
}

/** Shortcut changes written after a sync (new games added automatically): the count. */
export async function onSteamChanged(handler: (changes: number) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<number>("steam-changed", (e) => handler(e.payload));
}

export interface StorageChoice {
  path: string;
  /** "This Deck" or "SD card (FF8S5)". */
  label: string;
  free: number | null;
  current: boolean;
}

export interface SettingsView {
  account: string | null;
  drive_folder: string | null;
  storage_root: string;
  storage_available: boolean;
  storage_free: number | null;
  storage_choices: StorageChoice[];
  save_versions: number;
  auto_add: boolean;
  version: string;
  update_ready: string | null;
}

export async function settingsView(): Promise<SettingsView> {
  if (!inTauri()) {
    return {
      account: "player@example.com",
      drive_folder: "My Drive / Emulation",
      storage_root: "/home/deck/Romshelf",
      storage_available: true,
      storage_free: 40 * 1024 ** 3,
      storage_choices: [
        { path: "/home/deck/Romshelf", label: "This Deck", free: 40 * 1024 ** 3, current: true },
        { path: "/run/media/deck/SD/Romshelf", label: "SD card (SD)", free: 200 * 1024 ** 3, current: false },
      ],
      save_versions: 10,
      auto_add: false,
      version: "dev",
      update_ready: null,
    };
  }
  return invoke<SettingsView>("settings_view");
}

export async function setSaveVersions(count: number): Promise<void> {
  if (inTauri()) await invoke("set_save_versions", { count });
}

export async function signOut(): Promise<void> {
  if (inTauri()) await invoke("sign_out");
}

/** Moves downloaded ROMs to another folder; Romshelf restarts when it's done. */
export async function changeStorage(path: string): Promise<void> {
  if (inTauri()) await invoke("change_storage", { path });
}

export async function checkForUpdates(): Promise<void> {
  if (inTauri()) await invoke("check_for_updates");
}

export async function diagnostics(): Promise<string> {
  if (!inTauri()) return "Romshelf dev\n(browser preview)\n";
  return invoke<string>("diagnostics");
}

export interface TransferRow {
  rom_id: string;
  name: string;
  kind: "download" | "backup";
  state: "queued" | "running" | "waiting";
  size: number;
  message: string | null;
}

export async function transfers(): Promise<TransferRow[]> {
  if (!inTauri()) return [];
  return invoke<TransferRow[]>("transfers");
}

/** Asks Steam to show its on-screen keyboard. */
export async function openSteamKeyboard(): Promise<void> {
  if (inTauri()) await invoke("open_steam_keyboard");
}

/** One self-test check: the core's (`romshelf_core::selftest::Check`) and the interface's. */
export interface SelfTestCheck {
  group: string;
  name: string;
  outcome: "pass" | "warn" | "fail" | "skip";
  detail: string;
  fix: string | null;
}

const PREVIEW_CHECKS: SelfTestCheck[] = [
  { group: "Setup", name: "Google account", outcome: "pass", detail: "signed in as player@example.com", fix: null },
  { group: "Steam", name: "Tidewalker", outcome: "pass", detail: "Launches gba-e2bb15b4; artwork and Steam Input layout in place.", fix: null },
  { group: "Drive", name: "Round trip", outcome: "pass", detail: "Sent 4 MB up and back; resumed after a dropped connection; checksums matched.", fix: null },
];

/** Runs every check Romshelf can do by itself; `onCheck` hears each as it finishes. */
export async function selftestRun(onCheck: (c: SelfTestCheck) => void): Promise<SelfTestCheck[]> {
  if (!inTauri()) {
    PREVIEW_CHECKS.forEach(onCheck);
    return PREVIEW_CHECKS;
  }
  const unlisten = await listen<SelfTestCheck>("selftest-check", (e) => onCheck(e.payload));
  try {
    return await invoke<SelfTestCheck[]>("selftest_run");
  } finally {
    unlisten();
  }
}

/** What the guided steps watch. */
export interface SelfTestProbe {
  online: boolean;
  pending_backups: number;
  storage_available: boolean;
  storage_removable: boolean;
  storage_root: string;
  launches: number;
}

export async function selftestProbe(): Promise<SelfTestProbe> {
  if (!inTauri()) {
    return { online: true, pending_backups: 0, storage_available: true, storage_removable: false, storage_root: "~/Romshelf", launches: 0 };
  }
  return invoke<SelfTestProbe>("selftest_probe");
}

/** Backs up owed saves now. */
export async function selftestFlush(): Promise<void> {
  if (inTauri()) await invoke("selftest_flush");
}

/** Saves the report; where it went. */
export async function selftestSave(text: string, json: unknown): Promise<string> {
  if (!inTauri()) return "(browser preview: not saved)";
  return invoke<string>("selftest_save", { text, json });
}
