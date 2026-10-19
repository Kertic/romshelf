import type { AppInfo, LibraryEntry } from "./api";

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

interface Props {
  info: AppInfo | null;
  entries: LibraryEntry[] | null;
  error: string | null;
  /** "My Drive / Emulation", once a Drive folder is chosen. */
  driveFolder: string | null;
  /** A newer Romshelf that runs from the next start. */
  newVersion?: string | null;
  onOpenSpike: () => void;
  onQuit: () => void;
}

/** Stage 0 shell: top band, the library as plain tiles, and the two test actions. */
export function Home({ info, entries, error, driveFolder, newVersion, onOpenSpike, onQuit }: Props) {
  const onDeck = entries?.filter((e) => e.rom.location.state === "on_deck").length ?? 0;
  return (
    <>
      <header className="top-band">
        <div className="app-name">Romshelf</div>
        <div className="top-spacer" />
        {newVersion && (
          <span className="update-note" role="status">
            Romshelf {newVersion} is ready. Restart Romshelf to use it.
          </span>
        )}
        {info?.fake && <span className="badge">Fixture library</span>}
        <span className="mono muted">v{info?.version ?? "…"}</span>
      </header>

      <main className="content home">
        <div className="home-heading">
          <h1 className="section-heading">All systems</h1>
          {entries && (
            <span className="mono muted">
              {entries.length} ROMs in Drive · {onDeck} on Deck
            </span>
          )}
        </div>

        {error && <p className="error">{error}</p>}

        {entries && entries.length === 0 && (
          <div className="empty">
            <span className="mono">{driveFolder ?? "My Drive / Emulation"} / roms</span>
            <p>Upload ROMs to this folder from any device. Romshelf lists them once syncing arrives in the next build.</p>
          </div>
        )}

        {entries && entries.length > 0 && (
          <div className="grid">
            {entries.map((e) => {
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

        <div className="home-actions">
          <button className="button primary" data-focusable data-hint="Open" type="button" onClick={onOpenSpike}>
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
