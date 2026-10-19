import { useEffect, useState } from "react";
import type { AppInfo, LibraryEntry, SyncStatus } from "./api";
import { counts, inTab, syncLine, TABS } from "./library";

function initials(name: string): string {
  return name
    .split(/[\s&]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
}

export function tileStatus(e: LibraryEntry): { text: string; warn: boolean } {
  if (!e.rom.recognized) return { text: "Unrecognized file", warn: true };
  if (e.emulator && !e.emulator_installed) return { text: `Needs ${e.emulator}`, warn: true };
  const loc = e.rom.location;
  if (loc.state === "downloading") return { text: `Downloading ${loc.percent}%`, warn: false };
  if (loc.state === "on_deck") return { text: e.in_steam ? "On Deck · In Steam" : "On Deck", warn: false };
  return { text: "In Drive only", warn: false };
}

/** The current time, refreshed every 30 seconds so "synced 2 min ago" stays true. */
function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(t);
  }, []);
  return now;
}

interface Props {
  info: AppInfo | null;
  entries: LibraryEntry[] | null;
  error: string | null;
  /** "My Drive / Emulation", once a Drive folder is chosen. */
  driveFolder: string | null;
  /** A newer Romshelf that runs from the next start. */
  newVersion?: string | null;
  /** "all" or a system id. */
  tab?: string;
  onTab?: (tab: string) => void;
  sync?: SyncStatus | null;
  onSync?: () => void;
  onOpenSpike: () => void;
  onQuit: () => void;
}

/** The Library (UI guide 5.1): system tabs, the tile grid, files Romshelf can't run, and the folder strip. */
export function Home({
  info,
  entries,
  error,
  driveFolder,
  newVersion,
  tab = "all",
  onTab,
  sync = null,
  onSync,
  onOpenSpike,
  onQuit,
}: Props) {
  const now = useNow();
  const current = TABS.find((t) => t.id === tab) ?? TABS[0];
  const shown = entries ? inTab(entries, current.id) : null;
  const roms = shown?.filter((e) => e.rom.recognized) ?? [];
  const unrecognized = shown?.filter((e) => !e.rom.recognized) ?? [];
  const { inDrive, onDeck } = counts(shown ?? []);
  const line = syncLine(sync, now);
  const busy = sync?.state === "syncing";
  const romsPath = `${driveFolder ?? "My Drive / Emulation"} / roms`;
  const folderPath = current.id === "all" ? romsPath : `${romsPath} / ${current.id}`;
  const syncProblem = sync && (sync.state === "offline" || sync.state === "error") ? sync.message : null;

  return (
    <>
      <header className="top-band">
        <div className="app-name">Romshelf</div>
        <nav className="tabs" aria-label="Systems">
          <span className="key-cap mono">L1</span>
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              className={t.id === current.id ? "tab selected" : "tab"}
              aria-pressed={t.id === current.id}
              onClick={() => onTab?.(t.id)}
            >
              {t.label}
            </button>
          ))}
          <span className="key-cap mono">R1</span>
        </nav>
        <div className="top-spacer" />
        {newVersion && (
          <span className="update-note" role="status">
            Romshelf {newVersion} is ready. Restart Romshelf to use it.
          </span>
        )}
        {info?.fake && <span className="badge">Fixture library</span>}
        <button
          type="button"
          className={line.warn ? "sync-status warn" : "sync-status"}
          data-focusable
          data-hint="Sync now"
          disabled={busy}
          onClick={onSync}
        >
          <span className={busy ? "sync-dot busy" : "sync-dot"} aria-hidden />
          {line.text}
        </button>
      </header>

      <main className="content home">
        <div className="home-heading">
          <h1 className="section-heading">{current.heading}</h1>
          {entries && (
            <span className="mono muted">
              {inDrive} {inDrive === 1 ? "ROM" : "ROMs"} in Drive · {onDeck} on Deck
            </span>
          )}
        </div>

        {error && <p className="error">{error}</p>}
        {syncProblem && <p className="error">{syncProblem}</p>}

        {entries && entries.length === 0 && (
          <div className="empty">
            <span className="mono">{romsPath}</span>
            <p>Upload ROMs to this folder from any device and they appear here.</p>
            <button
              className="button primary"
              data-focusable
              data-hint="Sync"
              type="button"
              disabled={busy}
              onClick={onSync}
            >
              Sync now
            </button>
          </div>
        )}

        {entries && entries.length > 0 && roms.length === 0 && unrecognized.length === 0 && (
          <p className="muted">No {current.label} ROMs yet.</p>
        )}

        {roms.length > 0 && (
          <div className="grid">
            {roms.map((e) => {
              const status = tileStatus(e);
              return (
                <button key={e.rom.id} className="tile" data-focusable data-hint="Select" type="button">
                  <span className="cover" style={{ background: e.system_colour }}>
                    <span className="cover-system mono">{e.system_label}</span>
                    <span className="cover-initials">{initials(e.rom.display_name)}</span>
                  </span>
                  <span className="tile-text">
                    <span className="tile-title">{e.rom.display_name}</span>
                    <span className={status.warn ? "tile-status warn" : "tile-status"}>{status.text}</span>
                  </span>
                </button>
              );
            })}
          </div>
        )}

        {unrecognized.length > 0 && (
          <section className="unrecognized" aria-label="Files Romshelf can't run">
            <h2 className="group-label">Not recognized</h2>
            {unrecognized.map((e) => (
              <div className="unrecognized-row" key={e.rom.id}>
                <span className="mono">
                  {e.system_label} / {e.rom.file_name}
                </span>
                <span className="tile-status warn">Unrecognized file</span>
              </div>
            ))}
            <p className="muted small">
              Romshelf runs only the file types each system supports. Convert or move these files in Drive.
            </p>
          </section>
        )}

        {entries && entries.length > 0 && (
          <div className="folder-strip">
            <span className="mono">{folderPath}</span>
            <span>Upload ROMs to this folder from any device and they appear here.</span>
          </div>
        )}

        <div className="home-actions">
          <button className="button outline" data-focusable data-hint="Open" type="button" onClick={onOpenSpike}>
            Open gamepad test
          </button>
          <button className="button outline" data-focusable data-hint="Quit" type="button" onClick={onQuit}>
            Quit Romshelf
          </button>
        </div>
      </main>
    </>
  );
}
