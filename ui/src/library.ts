import type { LibraryEntry, SyncStatus } from "./api";

export interface Tab {
  /** "all" or a system id. */
  id: string;
  label: string;
  /** The heading above the grid. */
  heading: string;
}

/** All, then one per system, in the guide's order. Every system has a folder after setup. */
export const TABS: Tab[] = [
  { id: "all", label: "All", heading: "All systems" },
  { id: "snes", label: "SNES", heading: "SNES" },
  { id: "gba", label: "GBA", heading: "GBA" },
  { id: "n64", label: "N64", heading: "N64" },
  { id: "ps2", label: "PS2", heading: "PS2" },
  { id: "gamecube", label: "GameCube", heading: "GameCube" },
];

/** The tab L1 (-1) or R1 (+1) moves to, wrapping around. */
export function nextTab(current: string, step: 1 | -1): string {
  const i = Math.max(
    0,
    TABS.findIndex((t) => t.id === current),
  );
  return TABS[(i + step + TABS.length) % TABS.length].id;
}

export function inTab(entries: LibraryEntry[], tab: string): LibraryEntry[] {
  return tab === "all" ? entries : entries.filter((e) => e.rom.system === tab);
}

/** Header counts: ROMs (recognized files) in Drive, and how many of them are on the Deck. */
export function counts(entries: LibraryEntry[]): { inDrive: number; onDeck: number } {
  const roms = entries.filter((e) => e.rom.recognized);
  return { inDrive: roms.length, onDeck: roms.filter((e) => e.rom.location.state === "on_deck").length };
}

/** "Today 3:42 PM"-style ages for the sync line: just now, minutes, hours, then the date. */
export function ago(thenSecs: number, nowMs: number): string {
  const secs = Math.max(0, Math.floor(nowMs / 1000) - thenSecs);
  if (secs < 60) return "just now";
  if (secs < 3600) return `${Math.floor(secs / 60)} min ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)} h ago`;
  return new Date(thenSecs * 1000).toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

/** The top band's sync line. */
export function syncLine(s: SyncStatus | null, nowMs: number): { text: string; warn: boolean } {
  if (!s || s.state === "idle") return { text: "Not synced yet", warn: false };
  if (s.state === "syncing") return { text: "Syncing with Google Drive…", warn: false };
  if (s.state === "offline") return { text: "Offline", warn: true };
  if (s.state === "error") return { text: "Sync failed", warn: true };
  return {
    text: s.last_synced_at ? `Google Drive synced ${ago(s.last_synced_at, nowMs)}` : "Google Drive synced",
    warn: false,
  };
}
