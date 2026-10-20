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
  /** Opens the Steam shortcut screen for this game. */
  onSteamShortcut?: () => void;
  /** Drops this game's differences, back to the emulator's default. */
  onResetControls?: () => Promise<void>;
  /** For tests: what "today" is. */
  now?: Date;
  /** A dialog to open the page with (the drawer's "Remove from Deck…", say). */
  startWith?: View;
}

/** What the game's dialog can do with its save. */
export interface SaveActions {
  backup: () => Promise<BackupOutcome>;
  versions: () => Promise<SaveVersion[]>;
  restore: (versionId: string) => Promise<RestoreOutcome>;
  states: () => Promise<StateSlot[]>;
  deleteState: (slot: string) => Promise<void>;
}

export type View = "main" | "remove" | "restore" | "states";

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

/** A tile-style cover: system colour, system label, initials. */
export function Cover({ entry, size }: { entry: LibraryEntry; size: "large" | "small" }) {
  return (
    <span className={`cover ${size}`} style={{ background: entry.system_colour }} aria-hidden>
      <span className="cover-system mono">{entry.system_label}</span>
      <span className="cover-initials">{initials(entry.rom.display_name)}</span>
    </span>
  );
}

/** "TW" for "Tidewalker", "EC" for "Ember Circuit". */
export function initials(name: string): string {
  return name
    .split(/[\s&,]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
}

/**
 * The ROM page (UI guide 5.2, and 5.3 when the emulator is missing): a game's actions,
 * functions, controls and Steam shortcut. Romshelf never changes files in Drive, so nothing
 * here deletes from Drive.
 */
export function RomPage({
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
  onSteamShortcut,
  now,
  startWith,
}: Props) {
  const [view, setView] = useState<View>(startWith ?? "main");
  /** The outcome of the last save action, and a warning that came with it. */
  const [saveNote, setSaveNote] = useState<{ text: string; error: boolean; warning?: string | null } | null>(null);
  const [backingUp, setBackingUp] = useState(false);
  const rom = entry.rom;
  const state = rom.location.state;
  const page = useDialogFocus(`page-${state}-${entry.emulator_ready}`);
  const dialog = useDialogFocus(view);

  useEffect(() => {
    setBack(view !== "main" ? { label: "Back", run: () => setView("main") } : { label: "Library", run: onClose });
  }, [view, onClose, setBack]);

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
  // The guide's screen 5.3: with the emulator missing, setting it up is the main action.
  const needsSetup = Boolean(entry.emulator) && !entry.emulator_ready;
  const setupRunning = Boolean(setup?.running);
  // Downloaded with its emulator ready: Play is the main action (guide 5.2).
  const canPlay = state === "on_deck" && entry.emulator_ready;
  const playing = Boolean(launch?.running);
  const location =
    state === "on_deck" ? "On Deck" : state === "downloading" ? `Downloading ${Math.round(fraction * 100)}%` : "In Drive only";

  let overlay: React.ReactNode = null;
  if (launch?.conflict && onAnswer) {
    // The launch is waiting for an answer about this game's save.
    overlay = <SaveConflictDialog conflict={launch.conflict} onAnswer={onAnswer} now={now} />;
  } else if (view === "remove") {
    overlay = <RemoveDialog entry={entry} onCancel={() => setView("main")} onRemove={onRemove} dialogRef={dialog} />;
  } else if (view === "restore" && saves) {
    overlay = (
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
  } else if (view === "states" && saves) {
    overlay = <StatesDialog load={saves.states} onDelete={saves.deleteState} onClose={() => setView("main")} setBack={setBack} now={now} />;
  }

  const card = (key: string, label: string, state: string, onClick?: () => void, hint = "Open") => (
    <button
      key={key}
      className="function-card"
      aria-label={label}
      data-focusable={onClick ? true : undefined}
      data-hint={hint}
      type="button"
      disabled={!onClick || playing}
      onClick={onClick}
    >
      <span className="card-label">{label}</span>
      <span className="muted small">{state}</span>
    </button>
  );
  const later = "Comes in a later update";

  return (
    <>
      <main
        className="content rom-page"
        ref={page}
        aria-label={rom.display_name}
        aria-hidden={overlay ? true : undefined}
        inert={overlay ? true : undefined}
      >
        <aside className="rom-left">
          <button className="back-link" data-focusable data-hint="Back" type="button" onClick={onClose}>
            ‹ Library
          </button>
          <Cover entry={entry} size="large" />
          <h1 className="rom-title">{rom.display_name}</h1>
          <span className="mono muted small">
            roms / {rom.system} / {rom.file_name}
          </span>
          <span className={line.warn ? "tile-status warn" : "body-text"}>{line.text}</span>
          <dl className="facts">
            <div>
              <dt>Size</dt>
              <dd>{humanSize(rom.size)}</dd>
            </div>
            <div>
              <dt>Location</dt>
              <dd>{location}</dd>
            </div>
            <div>
              <dt>Save backup</dt>
              <dd className={backupLine.warn ? "warn" : undefined}>{backupLine.text}</dd>
            </div>
          </dl>
        </aside>

        <div className="rom-right">
          <div className="rom-actions">
            {canPlay && (
              <button
                className="button primary wide"
                data-focusable
                data-autofocus
                data-hint="Play"
                type="button"
                disabled={playing}
                onClick={onPlay}
              >
                {playing ? (
                  `${launch?.label ?? "Starting"}…`
                ) : (
                  <>
                    <span aria-hidden="true">▶ </span>Play
                  </>
                )}
              </button>
            )}
            {needsSetup && (
              <button
                className="button primary wide"
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
              <div className="progress wide" aria-label="Download progress">
                <div className="progress-track">
                  <div className="progress-fill" style={{ width: `${Math.round(fraction * 100)}%` }} />
                </div>
                <span className="mono small">{pct}</span>
              </div>
            ) : null}
            {state === "downloading" ? (
              <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={onCancel}>
                Cancel download
              </button>
            ) : (
              <button
                className={state === "on_deck" || needsSetup ? "button outline" : "button primary wide"}
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
              <button className="button danger" data-focusable data-hint="Remove" type="button" onClick={() => setView("remove")}>
                Remove from Deck…
              </button>
            )}
          </div>

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
          {error && <p className="error">{error}</p>}
          {launch?.error && <p className="error">{launch.error}</p>}
          {saveNote && <p className={saveNote.error ? "error" : "body-text"}>{saveNote.text}</p>}
          {saveNote?.warning && <p className="tile-status warn">{saveNote.warning}</p>}

          {entry.emulator && (
            <section aria-label={`${entry.emulator} functions`}>
              <h2 className="group-label">{entry.core ? CORE_NAMES[entry.core] ?? entry.emulator : entry.emulator} functions</h2>
              <div className="function-grid">
                {saves && entry.emulator_ready && [
                  card(
                    "backup",
                    backingUp ? "Backing up…" : "Back up save data",
                    backupLine.text,
                    backingUp ? undefined : () => void backup(),
                    "Back up",
                  ),
                  card("restore", "Restore save from Drive…", "Choose a backup", () => setView("restore"), "Restore"),
                  entry.keeps_states && card("states", "Save states…", "Kept in Drive", () => setView("states")),
                ]}
                {entry.actions.map((a) => {
                  const ready = Boolean(saves) && entry.emulator_ready;
                  if (a.id === "save_states" && ready && entry.keeps_states) return null;
                  if (a.id === "memory_cards" && ready) return card(a.id, a.label, "Backed up with the save");
                  return card(a.id, a.label, later);
                })}
              </div>
            </section>
          )}

          <div className="rom-cards">
            {onEditControls && entry.emulator_ready && (
              <section className="card" aria-label="Controls">
                <div className="card-head">
                  <h2 className="card-heading">Controls</h2>
                  <span className="muted small">
                    {entry.control_differences === 0
                      ? `${entry.emulator} default`
                      : `${entry.control_differences} ${entry.control_differences === 1 ? "button differs" : "buttons differ"} from default`}
                  </span>
                </div>
                <p className="body-text">
                  {entry.control_differences === 0 ? "This game uses the emulator's default layout." : "Custom for this game."}
                </p>
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
            {onSteamShortcut && entry.emulator && entry.rom.recognized && (
              <section className="card" aria-label="Steam shortcut">
                <div className="card-head">
                  <h2 className="card-heading">Steam shortcut</h2>
                  <span className={entry.in_steam ? "small accent" : "small muted"}>
                    {entry.in_steam ? "In your library" : "Not in Steam"}
                  </span>
                </div>
                {entry.in_steam && (
                  <>
                    <span className="small muted">Steam runs</span>
                    <code className="mono well">romshelf launch --id {rom.id}</code>
                  </>
                )}
                <div className="dialog-actions start">
                  <button className="button outline" data-focusable data-hint="Open" type="button" onClick={onSteamShortcut}>
                    {entry.in_steam ? "Edit shortcut…" : "Add to Steam library…"}
                  </button>
                </div>
              </section>
            )}
          </div>
        </div>
      </main>
      {overlay}
    </>
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
