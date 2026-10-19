import { useEffect, useState } from "react";
import type { SaveChoice, SaveConflict, SaveSync, SaveVersion, StateSlot } from "./api";
import type { BackAction } from "./FirstRun";
import { humanSize, useDialogFocus } from "./RomActions";

const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

/** "Today 3:42 PM", "Yesterday 9:10 PM", then "Oct 6" (UI guide section 9). */
export function formatWhen(secs: number, now: Date = new Date()): string {
  const when = new Date(secs * 1000);
  const days = Math.round((startOfDay(now) - startOfDay(when)) / 86_400_000);
  const time = when.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  if (days === 0) return `Today ${time}`;
  if (days === 1) return `Yesterday ${time}`;
  return when.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

/** The "Save backup" fact for a game. */
export function saveLine(save: SaveSync, now: Date = new Date()): { text: string; warn: boolean } {
  // Owed after a failed attempt, or never backed up at all (Steam ended Romshelf with the game).
  if (save.pending && (save.last_error != null || save.last_backup_at == null)) {
    return { text: "Waiting to back up. Romshelf tries again when Drive can be reached.", warn: true };
  }
  if (save.last_backup_at == null) return { text: "Not backed up yet", warn: false };
  const from = save.last_backup_device ? ` from ${save.last_backup_device}` : "";
  return { text: `Backed up ${formatWhen(save.last_backup_at, now)}${from}`, warn: false };
}

const message = (e: unknown): string =>
  e && typeof e === "object" && "message" in e ? String(e.message) : `Something went wrong: ${String(e)}. Try again.`;

interface ConflictProps {
  conflict: SaveConflict;
  onAnswer: (choice: SaveChoice) => void;
  now?: Date;
}

/** "Two different saves" (UI guide section 7). Whichever save is replaced is backed up first. */
export function SaveConflictDialog({ conflict, onAnswer, now }: ConflictProps) {
  const ref = useDialogFocus(conflict.rom_id);
  return (
    <div className="dialog-backdrop">
      <div className="dialog" role="dialog" aria-modal="true" aria-label="Two different saves" data-dialog ref={ref}>
        <h2 className="dialog-title">Two different saves</h2>
        <p className="body-text">This Deck and Drive both have newer saves for {conflict.name}.</p>
        <p className="body-text">
          This Deck: {formatWhen(conflict.deck.taken_at, now)}, {conflict.deck.device}
        </p>
        <p className="body-text">
          Drive: {formatWhen(conflict.drive.taken_at, now)}, {conflict.drive.device}
        </p>
        <p className="muted small">The save you don't use is backed up to Drive first.</p>
        <div className="dialog-actions">
          <button className="button outline" data-focusable data-hint="Use" type="button" onClick={() => onAnswer("keep_deck")}>
            Use this Deck's
          </button>
          <button className="button outline" data-focusable data-hint="Use" type="button" onClick={() => onAnswer("use_drive")}>
            Use Drive's
          </button>
          <button
            className="button outline"
            data-focusable
            data-autofocus
            data-hint="Cancel"
            type="button"
            onClick={() => onAnswer("cancel")}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

interface RestoreProps {
  name: string;
  load: () => Promise<SaveVersion[]>;
  onRestore: (version: SaveVersion) => Promise<void>;
  onCancel: () => void;
  now?: Date;
}

/** "Restore a save": the versions in Drive, newest first. */
export function RestoreDialog({ name, load, onRestore, onCancel, now }: RestoreProps) {
  const [versions, setVersions] = useState<SaveVersion[] | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ref = useDialogFocus(versions ? "loaded" : "loading");

  useEffect(() => {
    let live = true;
    load()
      .then((v) => {
        if (!live) return;
        setVersions(v);
        setChosen(v[0]?.id ?? null);
      })
      .catch((e) => live && setError(message(e)));
    return () => {
      live = false;
    };
  }, [load]);

  const restore = async () => {
    const version = versions?.find((v) => v.id === chosen);
    if (!version) return;
    setBusy(true);
    setError(null);
    try {
      await onRestore(version);
    } catch (e) {
      setError(message(e));
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop">
      <div className="dialog" role="dialog" aria-modal="true" aria-label="Restore a save" data-dialog ref={ref}>
        <h2 className="dialog-title">Restore a save</h2>
        {versions == null && !error && <p className="body-text muted">Reading backups from Drive…</p>}
        {versions?.length === 0 && <p className="body-text">Drive has no backups of {name}'s save yet.</p>}
        {versions && versions.length > 0 && (
          <div className="folder-list" role="list">
            {versions.map((v) => (
              <button
                key={v.id}
                className={v.id === chosen ? "row selected" : "row"}
                data-focusable
                data-hint="Choose"
                type="button"
                aria-pressed={v.id === chosen}
                onClick={() => setChosen(v.id)}
              >
                <span>{formatWhen(v.taken_at, now)}</span>
                <span className="muted small">
                  {v.device} · {humanSize(v.size)}
                </span>
              </button>
            ))}
          </div>
        )}
        {error && <p className="error">{error}</p>}
        <div className="dialog-actions">
          <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={onCancel}>
            Cancel
          </button>
          <button
            className="button primary"
            data-focusable
            data-hint="Restore"
            type="button"
            disabled={busy || !chosen}
            onClick={() => void restore()}
          >
            Restore
          </button>
        </div>
        <p className="muted small">Your current save is backed up first.</p>
      </div>
    </div>
  );
}

interface StatesProps {
  load: () => Promise<StateSlot[]>;
  onDelete: (slot: string) => Promise<void>;
  onClose: () => void;
  setBack: (back: BackAction | null) => void;
  now?: Date;
}

/** A game's save state slots on this Deck, each of which can be deleted. */
export function StatesDialog({ load, onDelete, onClose, setBack, now }: StatesProps) {
  const [slots, setSlots] = useState<StateSlot[] | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ref = useDialogFocus(confirming ? `delete-${confirming}` : slots ? "list" : "loading");

  // Bumped to read the slots again after a delete.
  const [reads, setReads] = useState(0);
  const reload = () => setReads((n) => n + 1);
  useEffect(() => {
    let live = true;
    load()
      .then((s) => live && setSlots(s))
      .catch((e) => live && setError(message(e)));
    return () => {
      live = false;
    };
  }, [load, reads]);

  useEffect(() => {
    if (confirming) setBack({ label: "Back", run: () => setConfirming(null) });
  }, [confirming, setBack]);

  if (confirming) {
    const remove = async () => {
      setBusy(true);
      setError(null);
      try {
        await onDelete(confirming);
        setConfirming(null);
        setBack({ label: "Back", run: onClose });
        reload();
      } catch (e) {
        setError(message(e));
      }
      setBusy(false);
    };
    return (
      <div className="dialog-backdrop">
        <div
          className="dialog"
          role="dialog"
          aria-modal="true"
          aria-label={`Delete save state ${confirming}?`}
          data-dialog
          ref={ref}
        >
          <h2 className="dialog-title">Delete save state {confirming}?</h2>
          <p className="body-text">
            It's removed from this Deck. Its copy in Drive goes to Drive's trash, where it can be recovered.
          </p>
          {error && <p className="error">{error}</p>}
          <div className="dialog-actions">
            <button
              className="button outline"
              data-focusable
              data-autofocus
              data-hint="Cancel"
              type="button"
              onClick={() => {
                setConfirming(null);
                setBack({ label: "Back", run: onClose });
              }}
            >
              Cancel
            </button>
            <button
              className="button outline"
              data-focusable
              data-hint="Delete"
              type="button"
              disabled={busy}
              onClick={() => void remove()}
            >
              Delete save state
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="dialog-backdrop">
      <div className="dialog" role="dialog" aria-modal="true" aria-label="Save states" data-dialog ref={ref}>
        <h2 className="dialog-title">Save states</h2>
        {slots == null && !error && <p className="body-text muted">Reading save states…</p>}
        {slots?.length === 0 && <p className="body-text">No save states yet. They're backed up to Drive when the game ends.</p>}
        {slots && slots.length > 0 && (
          <div className="folder-list" role="list">
            {slots.map((s) => (
              <button
                key={s.name}
                className="row"
                data-focusable
                data-hint="Delete"
                type="button"
                onClick={() => setConfirming(s.name)}
              >
                <span className="mono">{s.name}</span>
                <span className="muted small">
                  {formatWhen(s.modified, now)} · {humanSize(s.size)}
                </span>
              </button>
            ))}
          </div>
        )}
        {error && <p className="error">{error}</p>}
        <div className="dialog-actions">
          <button className="button outline" data-focusable data-autofocus data-hint="Close" type="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
