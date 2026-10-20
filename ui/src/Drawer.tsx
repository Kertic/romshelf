import type { LibraryEntry, Progress } from "./api";
import { emulatorLine, humanSize, progressLine, type View } from "./RomActions";
import { saveLine } from "./Saves";

interface Props {
  entry: LibraryEntry;
  progress?: Progress;
  /** Opens the ROM page, with a dialog already open when given. */
  onOpen: (view?: View) => void;
  onPlay: () => void;
  onDownload: () => void;
  onSetup: () => void;
  now?: Date;
}

/**
 * The Library's drawer (UI guide 5.1): always the focused tile's game. Play or its first step,
 * the emulator's functions, the file actions, and a link to the ROM page.
 */
export function Drawer({ entry, progress, onOpen, onPlay, onDownload, onSetup, now }: Props) {
  const rom = entry.rom;
  const state = rom.location.state;
  const line = emulatorLine(entry);
  const needsSetup = Boolean(entry.emulator) && !entry.emulator_ready;
  const canPlay = state === "on_deck" && entry.emulator_ready;
  const backup = saveLine(entry.save, now);
  const playable = Boolean(entry.emulator) && rom.recognized;

  const row = (label: string, value: string, view?: View) => (
    <button key={label} className="function-row" data-focusable data-hint="Open" type="button" onClick={() => onOpen(view)}>
      <span>{label}</span>
      <span className="muted small">{value}</span>
    </button>
  );

  return (
    <aside className="drawer" aria-label={`${rom.display_name} details`}>
      <span className="mono muted small">
        {entry.system_label} · {rom.file_name} · {humanSize(rom.size)}
      </span>
      <h2 className="drawer-title">{rom.display_name}</h2>
      <span className={line.warn ? "tile-status warn" : "body-text"}>{line.text}</span>

      {state === "downloading" ? (
        <div className="progress" aria-label="Download progress">
          <div className="progress-track">
            <div
              className="progress-fill"
              style={{ width: `${progress && progress.total > 0 ? Math.round((progress.bytes * 100) / progress.total) : (rom.location as { percent: number }).percent}%` }}
            />
          </div>
          <span className="mono small">{progress ? progressLine(progress) : `${(rom.location as { percent: number }).percent}%`}</span>
        </div>
      ) : canPlay ? (
        <button className="button primary" data-focusable data-hint="Play" type="button" onClick={onPlay}>
          <span aria-hidden="true">▶ </span>Play
        </button>
      ) : needsSetup ? (
        <button className="button primary" data-focusable data-hint="Set up" type="button" onClick={onSetup}>
          Set up {entry.emulator}
        </button>
      ) : playable ? (
        <button className="button primary" data-focusable data-hint="Download" type="button" onClick={onDownload}>
          Download to Deck
        </button>
      ) : null}

      {playable && entry.emulator_ready && (
        <section className="functions" aria-label={`${entry.emulator} functions`}>
          <h3 className="group-label">{entry.emulator} functions</h3>
          {row("Back up save data", backup.text)}
          {row("Restore save from Drive", "Choose a backup", "restore")}
          {entry.keeps_states && row("Save states", "Kept in Drive", "states")}
          {row(
            "Controls",
            entry.control_differences === 0 ? `${entry.emulator} default` : "Custom for this game",
          )}
        </section>
      )}

      {playable && state === "on_deck" && (
        <section className="functions" aria-label="File">
          <h3 className="group-label">File</h3>
          <div className="drawer-file">
            <button className="button outline" data-focusable data-hint="Download" type="button" onClick={onDownload}>
              Download again
            </button>
            <button className="button danger" data-focusable data-hint="Remove" type="button" onClick={() => onOpen("remove")}>
              Remove from Deck…
            </button>
          </div>
        </section>
      )}

      <button className="link-button" data-focusable data-hint="Open" type="button" onClick={() => onOpen()}>
        Open ROM page: controls and Steam shortcut
      </button>
    </aside>
  );
}
