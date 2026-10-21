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

/** A Library row: a tile (224 px) and the gap below it (20 px). Fixed so the grid can be windowed. */
export const ROW_HEIGHT = 244;
/** Libraries bigger than this draw only the rows near the screen. */
export const WINDOW_FROM = 120;
/** Rows drawn above and below the visible ones, so focus always has a neighbour to move to. */
export const OVERSCAN = 2;

/**
 * The rows to draw: those overlapping the visible part of the scroller, plus `OVERSCAN` either
 * side. `gridTop` is where the grid starts inside the scroller.
 */
export function visibleRows(
  scrollTop: number,
  viewHeight: number,
  gridTop: number,
  rowCount: number,
): { first: number; last: number } {
  const top = scrollTop - gridTop;
  const first = Math.max(0, Math.floor(top / ROW_HEIGHT) - OVERSCAN);
  const last = Math.min(rowCount, Math.ceil((top + Math.max(viewHeight, ROW_HEIGHT)) / ROW_HEIGHT) + OVERSCAN);
  return { first, last: Math.max(first, last) };
}
