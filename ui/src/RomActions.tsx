import { useEffect, useRef, useState } from "react";
import type { BackupOutcome, LibraryEntry, Progress, RestoreOutcome, SaveChoice, SaveVersion, StateSlot } from "./api";
import type { SetupState } from "./Emulators";
import type { LaunchState } from "./LaunchWindow";
import type { BackAction } from "./FirstRun";
import { formatWhen, RestoreDialog, SaveConflictDialog, saveLine, StatesDialog } from "./Saves";

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
export function useDialogFocus(key: string) {
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
  /** Backing up, restoring and save states for this game. */
  saves?: SaveActions;
  /** Answers a save conflict the launch is waiting on. */
  onAnswer?: (choice: SaveChoice) => void;
  /** Opens the Controls editor on this game. */
  onEditControls?: () => void;
  /** Drops this game's differences, back to the emulator's default. */
  onResetControls?: () => Promise<void>;
  /** For tests: what "today" is. */
  now?: Date;
}

/** What the game's dialog can do with its save. */
export interface SaveActions {
  backup: () => Promise<BackupOutcome>;
  versions: () => Promise<SaveVersion[]>;
  restore: (versionId: string) => Promise<RestoreOutcome>;
  states: () => Promise<StateSlot[]>;
  deleteState: (slot: string) => Promise<void>;
}

type View = "main" | "remove" | "restore" | "states";

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
  saves,
  onAnswer,
  onEditControls,
  onResetControls,
  now,
}: Props) {
  const [view, setView] = useState<View>("main");
  /** The outcome of the last save action, and a warning that came with it. */
  const [saveNote, setSaveNote] = useState<{ text: string; error: boolean; warning?: string | null } | null>(null);
  const [backingUp, setBackingUp] = useState(false);
  const rom = entry.rom;
  const state = rom.location.state;
  const ref = useDialogFocus(view === "main" ? state : view);

  useEffect(() => {
    setBack(view !== "main" ? { label: "Back", run: () => setView("main") } : { label: "Close", run: onClose });
  }, [view, onClose, setBack]);

  // The launch is waiting for an answer about this game's save.
  if (launch?.conflict && onAnswer) {
    return <SaveConflictDialog conflict={launch.conflict} onAnswer={onAnswer} now={now} />;
  }
  if (view === "remove") {
    return <RemoveDialog entry={entry} onCancel={() => setView("main")} onRemove={onRemove} dialogRef={ref} />;
  }
  if (view === "restore" && saves) {
    return (
      <RestoreDialog
        name={rom.display_name}
        load={saves.versions}
        onCancel={() => setView("main")}
        now={now}
        onRestore={async (v) => {
          const out = await saves.restore(v.id);
          setSaveNote({ text: `Restored the save from ${formatWhen(v.taken_at, now)}.`, error: false, warning: out.warning });
          setView("main");
        }}
      />
    );
  }
  if (view === "states" && saves) {
    return <StatesDialog load={saves.states} onDelete={saves.deleteState} onClose={() => setView("main")} setBack={setBack} now={now} />;
  }

  const backup = async () => {
    if (!saves) return;
    setBackingUp(true);
    setSaveNote(null);
    try {
      const out = await saves.backup();
      setSaveNote({
        text:
          out.kind === "uploaded"
            ? `Backed up ${rom.display_name}`
            : out.kind === "unchanged"
              ? "Drive already has this save."
              : `${rom.display_name} has no save on this Deck yet. Play it first.`,
        error: false,
      });
    } catch (e) {
      setSaveNote({ text: e && typeof e === "object" && "message" in e ? String(e.message) : String(e), error: true });
    }
    setBackingUp(false);
  };
  const backupLine = saveLine(entry.save, now);

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
        {onEditControls && entry.emulator_ready && (
          <section className="functions" aria-label="Controls">
            <h3 className="group-label">Controls</h3>
            <span className="body-text">
              {entry.control_differences === 0
                ? `${entry.emulator} default`
                : `Custom for this game · ${entry.control_differences} ${entry.control_differences === 1 ? "button differs" : "buttons differ"} from default`}
            </span>
            <div className="dialog-actions start">
              <button className="button outline" data-focusable data-hint="Edit" type="button" disabled={playing} onClick={onEditControls}>
                Edit controls
              </button>
              {entry.control_differences > 0 && onResetControls && (
                <button
                  className="button outline"
                  data-focusable
                  data-hint="Reset"
                  type="button"
                  disabled={playing}
                  onClick={() => void onResetControls()}
                >
                  Reset to {entry.emulator} default
                </button>
              )}
            </div>
          </section>
        )}
        {saves && entry.emulator_ready && (
          <section className="functions" aria-label="Save backup">
            <h3 className="group-label">Save backup</h3>
            <span className={backupLine.warn ? "tile-status warn" : "body-text"}>{backupLine.text}</span>
            {saveNote && <p className={saveNote.error ? "error" : "body-text"}>{saveNote.text}</p>}
            {saveNote?.warning && <p className="tile-status warn">{saveNote.warning}</p>}
            <div className="dialog-actions start">
              <button
                className="button outline"
                data-focusable
                data-hint="Back up"
                type="button"
                disabled={backingUp || playing}
                onClick={() => void backup()}
              >
                {backingUp ? "Backing up…" : "Back up save data"}
              </button>
              <button
                className="button outline"
                data-focusable
                data-hint="Restore"
                type="button"
                disabled={playing}
                onClick={() => setView("restore")}
              >
                Restore save from Drive…
              </button>
              {entry.keeps_states && (
                <button
                  className="button outline"
                  data-focusable
                  data-hint="Open"
                  type="button"
                  disabled={playing}
                  onClick={() => setView("states")}
                >
                  Save states…
                </button>
              )}
            </div>
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
              onClick={() => setView("remove")}
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
