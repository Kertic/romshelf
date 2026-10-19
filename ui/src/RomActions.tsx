import { useEffect, useRef, useState } from "react";
import type { DeleteScope, LibraryEntry, Progress } from "./api";
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
  onDelete: (scope: DeleteScope, keepSaveBackups: boolean) => Promise<void>;
  onClose: () => void;
  setBack: (back: BackAction | null) => void;
}

/**
 * A game's download and delete actions, opened with A on a tile. Stands in for the drawer and
 * ROM page until stage 9.
 */
export function RomDialog({ entry, progress, error, onDownload, onCancel, onDelete, onClose, setBack }: Props) {
  const [confirming, setConfirming] = useState(false);
  const rom = entry.rom;
  const state = rom.location.state;
  const view = confirming ? "delete" : state;
  const ref = useDialogFocus(view);

  useEffect(() => {
    setBack(confirming ? { label: "Back", run: () => setConfirming(false) } : { label: "Close", run: onClose });
  }, [confirming, onClose, setBack]);

  if (confirming) {
    return (
      <DeleteDialog
        entry={entry}
        onCancel={() => setConfirming(false)}
        onDelete={async (scope, keep) => {
          await onDelete(scope, keep);
        }}
        dialogRef={ref}
      />
    );
  }

  const pct =
    state === "downloading"
      ? progress
        ? progressLine(progress)
        : `${(rom.location as { percent: number }).percent}%`
      : null;
  const fraction = progress && progress.total > 0 ? progress.bytes / progress.total : 0;

  return (
    <div className="dialog-backdrop">
      <div className="dialog" role="dialog" aria-modal="true" aria-label={rom.display_name} data-dialog ref={ref}>
        <span className="mono muted small">
          {entry.system_label} · {rom.file_name} · {humanSize(rom.size)}
        </span>
        <h2 className="dialog-title">{rom.display_name}</h2>
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
        <div className="dialog-actions">
          {state === "downloading" ? (
            <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={onCancel}>
              Cancel download
            </button>
          ) : (
            <>
              <button
                className={state === "on_deck" ? "button outline" : "button primary"}
                data-focusable
                data-autofocus
                data-hint="Download"
                type="button"
                onClick={onDownload}
              >
                {state === "on_deck" ? "Download again" : "Download to Deck"}
              </button>
              <button
                className="button outline danger"
                data-focusable
                data-hint="Delete"
                type="button"
                onClick={() => setConfirming(true)}
              >
                Delete…
              </button>
            </>
          )}
          <button className="button outline" data-focusable data-hint="Close" type="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

interface DeleteProps {
  entry: LibraryEntry;
  onCancel: () => void;
  onDelete: (scope: DeleteScope, keepSaveBackups: boolean) => Promise<void>;
  dialogRef: React.RefObject<HTMLDivElement | null>;
}

const CHOICES: { scope: DeleteScope; label: string }[] = [
  { scope: "deck", label: "From this Deck (keeps it in Drive)" },
  { scope: "drive", label: "From Drive (moves it to Drive's trash)" },
  { scope: "both", label: "From both" },
];

/** The delete dialog from UI guide section 7: three scopes, "Keep save backups", Cancel focused first. */
export function DeleteDialog({ entry, onCancel, onDelete, dialogRef }: DeleteProps) {
  const onDeck = entry.rom.location.state === "on_deck";
  const [scope, setScope] = useState<DeleteScope>(onDeck ? "deck" : "drive");
  const [keep, setKeep] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await onDelete(scope, keep);
    } catch (e) {
      const message = e && typeof e === "object" && "message" in e ? String(e.message) : String(e);
      setError(message);
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop">
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-label={`Delete ${entry.rom.display_name}?`}
        data-dialog
        ref={dialogRef}
      >
        <h2 className="dialog-title">Delete {entry.rom.display_name}?</h2>
        <div className="choices" role="radiogroup" aria-label="What to delete">
          {CHOICES.map((c) => {
            const disabled = c.scope !== "drive" && !onDeck;
            return (
              <button
                key={c.scope}
                type="button"
                role="radio"
                aria-checked={scope === c.scope}
                className={scope === c.scope ? "row choice selected" : "row choice"}
                data-focusable
                data-hint="Choose"
                disabled={disabled}
                onClick={() => setScope(c.scope)}
              >
                <span>{c.label}</span>
                <span className="choice-mark" aria-hidden>
                  {scope === c.scope ? "●" : "○"}
                </span>
              </button>
            );
          })}
        </div>
        {scope !== "deck" && (
          <button
            type="button"
            role="checkbox"
            aria-checked={keep}
            className="row choice"
            data-focusable
            data-hint={keep ? "Untick" : "Tick"}
            onClick={() => setKeep((k) => !k)}
          >
            <span>Keep save backups</span>
            <span className="choice-mark" aria-hidden>
              {keep ? "☑" : "☐"}
            </span>
          </button>
        )}
        {error && <p className="error">{error}</p>}
        <div className="dialog-actions">
          <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={onCancel}>
            Cancel
          </button>
          <button
            className="button outline danger"
            data-focusable
            data-hint="Delete"
            type="button"
            disabled={busy}
            onClick={() => void confirm()}
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
