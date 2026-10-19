import { useEffect, useRef, useState } from "react";
import type { LibraryEntry, Progress } from "./api";
import type { SetupState } from "./Emulators";
import type { LaunchState } from "./LaunchWindow";
import type { BackAction } from "./FirstRun";

/** "1.4 GB", "820 MB". */
export function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  const units = ["KB", "MB", "GB"];
  let value = bytes;
  let unit = -1;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return unit === 2 ? `${value.toFixed(1)} GB` : `${Math.round(value)} ${units[unit]}`;
}

/** "64% · 12 MB/s · about 2 min left" */
export function progressLine(p: Progress): string {
  const pct = p.total > 0 ? Math.min(100, Math.floor((p.bytes * 100) / p.total)) : 100;
  if (p.rate <= 0) return `${pct}%`;
  const secs = Math.max(0, (p.total - p.bytes) / p.rate);
  const left = secs < 60 ? "less than a minute" : secs < 3600 ? `${Math.round(secs / 60)} min` : `${(secs / 3600).toFixed(1)} h`;
  return `${pct}% · ${humanSize(p.rate)}/s · about ${left} left`;
}

/** Puts focus on the dialog's `data-autofocus` element when it opens or changes view. */
function useDialogFocus(key: string) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const t = setTimeout(() => {
      const root = ref.current;
      if (!root || root.contains(document.activeElement)) return;
      (
        root.querySelector<HTMLElement>("[data-autofocus]:not([disabled])") ??
        root.querySelector<HTMLElement>("[data-focusable]:not([disabled])")
      )?.focus();
    });
    return () => clearTimeout(t);
  }, [key]);
  return ref;
}

interface Props {
  entry: LibraryEntry;
  progress?: Progress;
  error?: string | null;
  onDownload: () => void;
  onCancel: () => void;
  /** Removes the copy on this Deck; Drive is never touched. */
  onRemove: () => Promise<void>;
  /** Starts the game. */
  onPlay?: () => void;
  /** This game being prepared or played. */
  launch?: LaunchState;
  /** Installs and sets up this game's emulator. */
  onSetupEmulator?: () => void;
  /** The emulator's setup, while it runs or after it failed. */
  setup?: SetupState;
  onClose: () => void;
  setBack: (back: BackAction | null) => void;
}

const CORE_NAMES: Record<string, string> = {
  snes9x: "Snes9x",
  mgba: "mGBA",
  mupen64plus_next: "Mupen64Plus-Next",
};

/** "Runs in RetroArch with the mGBA core", "Needs Dolphin", or why it can't run. */
export function emulatorLine(e: LibraryEntry): { text: string; warn: boolean } {
  if (!e.emulator) return { text: `Romshelf can't run ${e.system_label} games yet.`, warn: true };
  if (!e.emulator_ready) {
    return {
      text: e.emulator_installed ? `${e.emulator} needs setting up` : `Needs ${e.emulator}`,
      warn: true,
    };
  }
  const core = e.core ? ` with the ${CORE_NAMES[e.core] ?? e.core} core` : "";
  return { text: `Runs in ${e.emulator}${core}`, warn: false };
}

/**
 * A game's download actions, opened with A on a tile. Stands in for the drawer and ROM page
 * until stage 9. Romshelf never changes files in Drive, so nothing here deletes from Drive.
 */
export function RomDialog({
  entry,
  progress,
  error,
  onDownload,
  onCancel,
  onRemove,
  onPlay,
  launch,
  onSetupEmulator,
  setup,
  onClose,
  setBack,
}: Props) {
  const [confirming, setConfirming] = useState(false);
  const rom = entry.rom;
  const state = rom.location.state;
  const view = confirming ? "remove" : state;
  const ref = useDialogFocus(view);

  useEffect(() => {
    setBack(confirming ? { label: "Back", run: () => setConfirming(false) } : { label: "Close", run: onClose });
  }, [confirming, onClose, setBack]);

  if (confirming) {
    return <RemoveDialog entry={entry} onCancel={() => setConfirming(false)} onRemove={onRemove} dialogRef={ref} />;
  }

  const pct =
    state === "downloading"
      ? progress
        ? progressLine(progress)
        : `${(rom.location as { percent: number }).percent}%`
      : null;
  const fraction = progress && progress.total > 0 ? progress.bytes / progress.total : 0;
  const line = emulatorLine(entry);
  // The guide's screen 2b: with the emulator missing, setting it up is the main action.
  const needsSetup = Boolean(entry.emulator) && !entry.emulator_ready;
  const setupRunning = Boolean(setup?.running);
  // Downloaded with its emulator ready: Play is the main action (guide screen 2).
  const canPlay = state === "on_deck" && entry.emulator_ready;
  const playing = Boolean(launch?.running);

  return (
    <div className="dialog-backdrop">
      <div className="dialog" role="dialog" aria-modal="true" aria-label={rom.display_name} data-dialog ref={ref}>
        <span className="mono muted small">
          {entry.system_label} · {rom.file_name} · {humanSize(rom.size)}
        </span>
        <h2 className="dialog-title">{rom.display_name}</h2>
        <span className={line.warn ? "tile-status warn" : "body-text"}>{line.text}</span>
        {needsSetup && setupRunning && (
          <div className="progress" aria-label={`${entry.emulator} setup progress`}>
            <div className="progress-track">
              <div className="progress-fill" style={{ width: `${setup?.percent ?? 0}%` }} />
            </div>
            <span className="mono small">
              {setup?.step ?? "Setting up"}…{setup?.percent != null ? ` ${setup.percent}%` : ""}
            </span>
          </div>
        )}
        {needsSetup && setup?.failed && setup.message && <p className="error">{setup.message}</p>}
        {state === "downloading" ? (
          <div className="progress" aria-label="Download progress">
            <div className="progress-track">
              <div className="progress-fill" style={{ width: `${Math.round(fraction * 100)}%` }} />
            </div>
            <span className="mono small">{pct}</span>
          </div>
        ) : (
          <p className="body-text">
            {state === "on_deck" ? "Downloaded to this Deck." : "In Drive only. Download it to play on this Deck."}
          </p>
        )}
        {error && <p className="error">{error}</p>}
        {launch?.error && <p className="error">{launch.error}</p>}
        {entry.emulator && entry.actions.length > 0 && (
          <section className="functions" aria-label={`${entry.emulator} functions`}>
            <h3 className="group-label">{entry.emulator} functions</h3>
            {entry.actions.map((a) => (
              <div className="function-row" key={a.id}>
                <span>{a.label}</span>
                <span className="muted small">Comes in a later update</span>
              </div>
            ))}
          </section>
        )}
        <div className="dialog-actions">
          {canPlay && (
            <button
              className="button primary"
              data-focusable
              data-autofocus
              data-hint="Play"
              type="button"
              disabled={playing}
              onClick={onPlay}
            >
              {playing ? `${launch?.label ?? "Starting"}…` : "Play"}
            </button>
          )}
          {needsSetup && (
            <button
              className="button primary"
              data-focusable
              data-autofocus
              data-hint="Set up"
              type="button"
              disabled={setupRunning}
              onClick={onSetupEmulator}
            >
              {setup?.failed ? `Try setting up ${entry.emulator} again` : `Set up ${entry.emulator}`}
            </button>
          )}
          {state === "downloading" ? (
            <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={onCancel}>
              Cancel download
            </button>
          ) : (
            <button
              className={state === "on_deck" || needsSetup ? "button outline" : "button primary"}
              data-focusable
              data-autofocus={needsSetup || canPlay ? undefined : true}
              data-hint="Download"
              type="button"
              onClick={onDownload}
            >
              {state === "on_deck" ? "Download again" : "Download to Deck"}
            </button>
          )}
          {state === "on_deck" && (
            <button
              className="button outline"
              data-focusable
              data-hint="Remove"
              type="button"
              onClick={() => setConfirming(true)}
            >
              Remove from Deck…
            </button>
          )}
          <button className="button outline" data-focusable data-hint="Close" type="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

interface RemoveProps {
  entry: LibraryEntry;
  onCancel: () => void;
  onRemove: () => Promise<void>;
  dialogRef: React.RefObject<HTMLDivElement | null>;
}

/** Confirms removing the Deck's copy. Cancel is focused first; Drive keeps the file. */
export function RemoveDialog({ entry, onCancel, onRemove, dialogRef }: RemoveProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const name = entry.rom.display_name;

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await onRemove();
    } catch (e) {
      setError(e && typeof e === "object" && "message" in e ? String(e.message) : String(e));
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop">
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-label={`Remove ${name} from this Deck?`}
        data-dialog
        ref={dialogRef}
      >
        <h2 className="dialog-title">Remove {name} from this Deck?</h2>
        <p className="body-text">
          This frees {humanSize(entry.rom.size)} on the Deck. {name} stays in your Google Drive, so you can download it
          again any time.
        </p>
        {error && <p className="error">{error}</p>}
        <div className="dialog-actions">
          <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={onCancel}>
            Cancel
          </button>
          <button
            className="button outline"
            data-focusable
            data-hint="Remove"
            type="button"
            disabled={busy}
            onClick={() => void confirm()}
          >
            Remove from Deck
          </button>
        </div>
      </div>
    </div>
  );
}
