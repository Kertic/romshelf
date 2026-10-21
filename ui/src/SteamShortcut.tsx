import { useEffect, useState } from "react";
import type { Applied, ArtSlot, ImageFile, ShortcutView } from "./api";
import type { BackAction } from "./FirstRun";
import { useDialogFocus } from "./RomActions";
import { TextField } from "./Keyboard";

interface Props {
  view: ShortcutView;
  setBack: (b: BackAction | null) => void;
  onBack: () => void;
  onSave: (shown: boolean, name: string) => Promise<Applied>;
  onAutoAdd: (on: boolean) => Promise<void>;
  onArtwork: (slot: ArtSlot, path: string | null) => Promise<void>;
  loadImages: () => Promise<ImageFile[]>;
}

const message = (e: unknown): string =>
  e && typeof e === "object" && "message" in e ? String(e.message) : `Something went wrong: ${String(e)}. Try again.`;

/** The Steam shortcut screen (UI guide section 5.5). */
export function SteamShortcut({ view, setBack, onBack, onSave, onAutoAdd, onArtwork, loadImages }: Props) {
  const [name, setName] = useState(view.name);
  const [shown, setShown] = useState(view.shown);
  const [slot, setSlot] = useState<ArtSlot>("capsule");
  const [choosing, setChoosing] = useState<ImageFile[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  const dialog = useDialogFocus(choosing ? "choose" : "none");

  useEffect(() => {
    setBack(choosing ? { label: "Back", run: () => setChoosing(null) } : { label: "Back", run: onBack });
  }, [choosing, onBack, setBack]);

  const capsule = view.slots.find((s) => s.slot === "capsule");
  const focused = view.slots.find((s) => s.slot === slot)!;

  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    setNote(null);
    try {
      await f();
    } catch (e) {
      setNote({ text: message(e), error: true });
    }
    setBusy(false);
  };

  return (
    <main className="content shortcut">
      <button className="back-link" data-focusable data-hint="Back" type="button" onClick={onBack}>
        ‹ {view.default_name}
      </button>
      <h1 className="screen-title">Steam shortcut</h1>
      <div className="shortcut-body">
        <div className="shortcut-main">
          <div className="field">
            <span className="group-label">Name in Steam</span>
            <TextField label="Name in Steam" value={name} onChange={setName} />
          </div>

          <section aria-label="Artwork">
            <h2 className="group-label">Artwork</h2>
            <div className="art-slots">
              {view.slots.map((s) => (
                <button
                  key={s.slot}
                  className={`art-slot art-${s.slot}${s.source === "missing" ? " missing" : ""}${s.slot === slot ? " selected" : ""}`}
                  data-focusable
                  data-hint="Choose"
                  type="button"
                  aria-pressed={s.slot === slot}
                  aria-label={`${s.label}, ${s.source === "chosen" ? "your image" : s.source === "generated" ? "generated" : "missing"}`}
                  onClick={() => setSlot(s.slot)}
                  onFocus={() => setSlot(s.slot)}
                >
                  {s.source === "missing" ? (
                    <span className="tile-status warn">Missing</span>
                  ) : (
                    s.preview && <img src={s.preview} alt="" />
                  )}
                  <span className="small">{s.label}</span>
                  <span className="muted small">{s.source === "chosen" ? "Your image" : s.source === "generated" ? "Generated" : ""}</span>
                </button>
              ))}
            </div>
            <div className="dialog-actions start">
              <button
                className="button outline"
                data-focusable
                data-hint="Choose"
                type="button"
                disabled={busy}
                onClick={() => void run(async () => setChoosing(await loadImages()))}
              >
                Choose file…
              </button>
              {focused.source === "chosen" && (
                <button
                  className="button outline"
                  data-focusable
                  data-hint="Reset"
                  type="button"
                  disabled={busy}
                  onClick={() => void run(() => onArtwork(slot, null))}
                >
                  {slot === "logo" ? "Remove logo" : "Use generated artwork"}
                </button>
              )}
            </div>
          </section>

          <section className="card" aria-label="What Steam launches">
            <h2 className="group-label">What Steam launches</h2>
            <span className="small muted">Target</span>
            <code className="mono well">{view.target}</code>
            <span className="small muted">Launch options</span>
            <code className="mono well">{view.launch_options}</code>
            <p className="body-text">
              Steam starts Romshelf, and Romshelf starts the game in its emulator. Changes you make in Romshelf apply
              the next time you play, without touching this shortcut.
            </p>
          </section>
        </div>

        <aside className="card shortcut-side">
          <div className="library-preview">
            {capsule?.preview && <img src={capsule.preview} alt="" />}
            <span className="body-text">{name || view.default_name}</span>
          </div>
          <label className="check">
            <input type="checkbox" data-focusable checked={shown} onChange={(e) => setShown(e.target.checked)} />
            Show {name || view.default_name} in Steam library
          </label>
          <label className="check">
            <input
              type="checkbox"
              data-focusable
              checked={view.auto_add}
              onChange={(e) => void run(() => onAutoAdd(e.target.checked))}
            />
            Add every new ROM to Steam automatically
          </label>
          <span className="muted small">This applies to your whole library.</span>
          {note && <p className={note.error ? "error" : "body-text"}>{note.text}</p>}
          <button
            className="button primary"
            data-focusable
            data-autofocus
            data-hint="Save"
            type="button"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                const done = await onSave(shown, name);
                setNote({
                  text: done.restart_needed ? "Saved. Restart Steam to see the change." : "Saved. Steam already has it.",
                  error: false,
                });
              })
            }
          >
            Save shortcut
          </button>
          {view.shown && (
            <button
              className="button outline"
              data-focusable
              data-hint="Remove"
              type="button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  setShown(false);
                  await onSave(false, name);
                  setNote({ text: "Removed from Steam. Restart Steam to see the change.", error: false });
                })
              }
            >
              Remove from Steam
            </button>
          )}
        </aside>
      </div>

      {choosing && (
        <div className="dialog-backdrop">
          <div className="dialog" role="dialog" aria-modal="true" aria-label={`Choose ${focused.label.toLowerCase()} artwork`} data-dialog ref={dialog}>
            <h2 className="dialog-title">Choose {focused.label.toLowerCase()} artwork</h2>
            {choosing.length === 0 ? (
              <p className="body-text">No images in Pictures, Downloads or Desktop. Put a PNG or JPEG there and try again.</p>
            ) : (
              <div className="folder-list">
                {choosing.map((f) => (
                  <button
                    key={f.path}
                    className="row"
                    data-focusable
                    data-hint="Use"
                    type="button"
                    onClick={() => {
                      setChoosing(null);
                      void run(() => onArtwork(slot, f.path));
                    }}
                  >
                    <span>{f.name}</span>
                    <span className="muted small">{f.folder}</span>
                  </button>
                ))}
              </div>
            )}
            <div className="dialog-actions">
              <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={() => setChoosing(null)}>
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </main>
  );
}

interface RestartProps {
  changes: number;
  onClose: () => void;
}

/** "Restart Steam to apply" (UI guide section 7), once per batch of changes. */
export function RestartSteamDialog({ changes, onClose }: RestartProps) {
  const [how, setHow] = useState(false);
  const ref = useDialogFocus(how ? "how" : "ask");
  return (
    <div className="dialog-backdrop">
      <div className="dialog" role="dialog" aria-modal="true" aria-label="Restart Steam to apply" data-dialog ref={ref}>
        <h2 className="dialog-title">Restart Steam to apply</h2>
        <p className="body-text">
          {changes === 1 ? "1 shortcut change is ready." : `${changes} shortcut changes are ready.`} They appear after Steam
          restarts.
        </p>
        {how && (
          <p className="body-text">
            In Game Mode, press the Steam button, choose Power, then Restart. In Desktop Mode, quit Steam from its menu
            and open it again.
          </p>
        )}
        <div className="dialog-actions">
          <button className="button outline" data-focusable data-autofocus data-hint="Later" type="button" onClick={onClose}>
            Later
          </button>
          {!how && (
            <button className="button outline" data-focusable data-hint="How" type="button" onClick={() => setHow(true)}>
              How to restart
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
