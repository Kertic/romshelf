import { useCallback, useEffect, useRef, useState } from "react";
import type { ControlsView, Layout, TargetView } from "./api";
import { assign, backToDefault, clear, difference, inputFor, merge, rebind, rows, type Row } from "./layouts";
import type { BackAction } from "./FirstRun";
import type { Button } from "./input";
import { useDialogFocus } from "./RomActions";

/** Handles a press before normal navigation; true when it was used. */
export type KeyHandler = (b: Button, from: "pad" | "keyboard") => boolean;

const HOLD_TO_CANCEL_MS = 1000;

type Scope = "game" | "default";

type Ask =
  | { kind: "swap"; layout: Layout; to: string; other: TargetView }
  | { kind: "pick"; input: string }
  | { kind: "unsaved"; then: () => void }
  | { kind: "promote"; count: number };

interface Props {
  view: ControlsView;
  /** Buttons held on the pad now, to tell a tap of B from holding it. */
  held: Button[];
  registerKeys: (h: KeyHandler | null) => void;
  setBack: (b: BackAction | null) => void;
  onBack: () => void;
  onSave: (scope: Scope, layout: Layout) => Promise<void>;
  onPromote: (layout: Layout) => Promise<void>;
  promoteCount: (layout: Layout) => Promise<number>;
}

/** The Controls editor (UI guide section 5.4). */
export function Controls({ view, held, registerKeys, setBack, onBack, onSave, onPromote, promoteCount }: Props) {
  const [scope, setScope] = useState<Scope>(view.game ? "game" : "default");
  const savedGame = merge(view.default, view.game_diff);
  const [working, setWorking] = useState<Layout>(scope === "game" ? savedGame : view.default);
  const [listening, setListening] = useState<string | null>(null);
  const [ask, setAsk] = useState<Ask | null>(null);
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const holdingB = useRef<number | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const dialog = useDialogFocus(ask ? ask.kind : "none");

  const saved = scope === "game" ? savedGame : view.default;
  const dirty = Object.keys(difference(working, saved)).length > 0;
  const custom = scope === "game" ? Object.keys(difference(working, view.default)).length : 0;
  const shown = rows(scope === "game" ? view : { ...view, game: null }, working);
  const emulatorName = view.emulator_id === "retroarch" ? `${view.emulator} ${view.system_label}` : view.emulator;

  const switchTo = useCallback(
    (next: Scope) => {
      if (next === scope || (next === "game" && !view.game)) return;
      const go = () => {
        setScope(next);
        setWorking(next === "game" ? merge(view.default, view.game_diff) : view.default);
        setNote(null);
      };
      if (dirty) setAsk({ kind: "unsaved", then: go });
      else go();
    },
    [scope, view, dirty],
  );

  const finishRebind = useCallback(
    (from: string, to: string) => {
      setListening(null);
      const r = rebind(view, working, from, to);
      if (r.kind === "invalid") setNote({ text: r.message, error: true });
      else if (r.kind === "swap") setAsk({ kind: "swap", layout: r.layout, to, other: r.other });
      else {
        setWorking(r.layout);
        setNote(null);
      }
    },
    [view, working],
  );

  const focusedInput = () => (document.activeElement as HTMLElement | null)?.dataset?.input ?? null;

  // Presses reach the editor before normal navigation.
  useEffect(() => {
    registerKeys((b, from) => {
      if (ask) return false;
      if (listening) {
        if (b === "b") {
          if (from === "keyboard") setListening(null);
          else holdingB.current = Date.now();
          return true;
        }
        const input = inputFor(b);
        if (input) finishRebind(listening, input);
        return true;
      }
      const input = focusedInput();
      if (b === "x" && input) {
        setWorking((w) => clear(w, input));
        return true;
      }
      if (b === "y" && input && scope === "game") {
        setWorking((w) => backToDefault(w, view.default, input));
        return true;
      }
      if (b === "l1" || b === "r1") {
        switchTo(scope === "game" ? "default" : "game");
        return true;
      }
      return false;
    });
    return () => registerKeys(null);
  }, [registerKeys, ask, listening, finishRebind, scope, view.default, switchTo]);

  // B while listening: a tap binds B, holding it for a second cancels.
  const bHeld = held.includes("b");
  useEffect(() => {
    if (!listening || holdingB.current == null) return;
    if (!bHeld) {
      // Released: B itself is the new input (handled outside the effect's own run).
      const t = setTimeout(() => {
        holdingB.current = null;
        finishRebind(listening, "b");
      });
      return () => clearTimeout(t);
    }
    const t = setTimeout(() => {
      holdingB.current = null;
      setListening(null);
    }, HOLD_TO_CANCEL_MS);
    return () => clearTimeout(t);
  }, [bHeld, listening, finishRebind]);

  const leave = useCallback(() => {
    if (dirty) setAsk({ kind: "unsaved", then: onBack });
    else onBack();
  }, [dirty, onBack]);

  useEffect(() => {
    if (ask) setBack({ label: "Back", run: () => setAsk(null) });
    else if (listening) setBack({ label: "Hold to cancel", run: () => {} });
    else setBack({ label: "Back", run: leave });
  }, [ask, listening, leave, setBack]);

  // Focus starts on the first row.
  useEffect(() => {
    const t = setTimeout(() => root.current?.querySelector<HTMLElement>("[data-input]")?.focus());
    return () => clearTimeout(t);
  }, [scope]);

  const save = async () => {
    setBusy(true);
    try {
      await onSave(scope, scope === "game" ? difference(working, view.default) : working);
      setNote({ text: "Saved. The next launch uses these controls.", error: false });
    } catch (e) {
      setNote({ text: e && typeof e === "object" && "message" in e ? String(e.message) : String(e), error: true });
    }
    setBusy(false);
  };

  const askPromote = async () => {
    const count = await promoteCount(difference(working, view.default)).catch(() => 0);
    setAsk({ kind: "promote", count });
  };

  const rowButton = (r: Row) => (
    <button
      key={r.input.id}
      className={listening === r.input.id ? "row control-row listening" : "row control-row"}
      data-focusable
      data-input={r.input.id}
      data-hint={r.target ? "Rebind" : "Bind"}
      type="button"
      onClick={() => {
        setNote(null);
        if (r.target) setListening(r.input.id);
        else setAsk({ kind: "pick", input: r.input.id });
      }}
    >
      <span className="mono key-cap">{r.input.label}</span>
      <span className={r.target ? "control-target" : "control-target muted"}>
        {listening === r.input.id ? "Press a button…" : (r.target?.label ?? "Not bound")}
      </span>
      {scope === "game" && <span className={r.custom ? "tag accent" : "tag muted"}>{r.custom ? "This game" : "Default"}</span>}
    </button>
  );

  return (
    <main className="content controls" ref={root}>
      <button className="back-link" data-focusable data-hint="Back" type="button" onClick={leave}>
        ‹ {view.game ? view.game.name : "Emulators"}
      </button>
      <div className="controls-head">
        <h1 className="screen-title">Controls</h1>
        <div className="scope-switch" role="tablist" aria-label="Editing">
          <span className="key-cap">L1</span>
          <button
            className={scope === "default" ? "tab selected" : "tab"}
            role="tab"
            aria-selected={scope === "default"}
            type="button"
            onClick={() => switchTo("default")}
          >
            {emulatorName} default
          </button>
          {view.game && (
            <button
              className={scope === "game" ? "tab selected" : "tab"}
              role="tab"
              aria-selected={scope === "game"}
              type="button"
              onClick={() => switchTo("game")}
            >
              {view.game.name} only
            </button>
          )}
          <span className="key-cap">R1</span>
        </div>
      </div>
      <div className="controls-body">
        <div className="controls-lists">
          <section aria-label="Game buttons">
            <h2 className="group-label">Game buttons</h2>
            <div className="folder-list">{shown.game.map(rowButton)}</div>
          </section>
          <section aria-label="Emulator hotkeys">
            <h2 className="group-label">Emulator hotkeys</h2>
            <div className="folder-list">{shown.hotkeys.map(rowButton)}</div>
          </section>
        </div>
        <aside className="card controls-side">
          <h2 className="card-heading">How this layout is built</h2>
          {scope === "game" && view.game ? (
            <p className="body-text">
              {view.game.name} starts from the {emulatorName} default. Anything you change here applies to this game
              only. Rows marked Default keep following the emulator layout when it changes.
            </p>
          ) : (
            <p className="body-text">
              Every {view.system_label} game in {view.emulator} starts from this layout. Games keep any buttons you
              changed for them.
            </p>
          )}
          <p className="muted small">Hold View and Menu for a second to quit any game, whatever its layout.</p>
          <p className="muted small">
            Steam's controller layout for a shortcut sits above this. If you change a game's layout in Steam, Steam's
            change applies first.
          </p>
          {scope === "game" && (
            <>
              <span className="group-label">Custom bindings</span>
              <span className="card-heading">{custom}</span>
              <button
                className="button outline"
                data-focusable
                data-hint="Reset"
                type="button"
                disabled={custom === 0}
                onClick={() => setWorking(view.default)}
              >
                Reset to {emulatorName} default
              </button>
              <button
                className="button outline"
                data-focusable
                data-hint="Make default"
                type="button"
                disabled={custom === 0}
                onClick={() => void askPromote()}
              >
                Make this the {emulatorName} default
              </button>
            </>
          )}
          {note && <p className={note.error ? "error" : "body-text"}>{note.text}</p>}
          <button
            className="button primary"
            data-focusable
            data-hint="Save"
            type="button"
            disabled={busy || !dirty}
            onClick={() => void save()}
          >
            Save
          </button>
        </aside>
      </div>

      {ask && (
        <div className="dialog-backdrop">
          <div className="dialog" role="dialog" aria-modal="true" aria-label={askTitle(ask, view, emulatorName)} data-dialog ref={dialog}>
            <h2 className="dialog-title">{askTitle(ask, view, emulatorName)}</h2>
            {ask.kind === "swap" && (
              <>
                <p className="body-text">{view.inputs.find((i) => i.id === ask.to)?.label} already does {ask.other.label}. They trade places.</p>
                <div className="dialog-actions">
                  <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={() => setAsk(null)}>
                    Cancel
                  </button>
                  <button
                    className="button primary"
                    data-focusable
                    data-hint="Swap"
                    type="button"
                    onClick={() => {
                      setWorking(ask.layout);
                      setAsk(null);
                    }}
                  >
                    Swap
                  </button>
                </div>
              </>
            )}
            {ask.kind === "pick" && (
              <>
                <div className="folder-list">
                  {view.targets
                    .filter((t) => t.directional === view.inputs.find((i) => i.id === ask.input)?.directional)
                    .map((t) => (
                      <button
                        key={t.id}
                        className="row"
                        data-focusable
                        data-hint="Choose"
                        type="button"
                        onClick={() => {
                          setWorking((w) => assign(w, ask.input, t.id));
                          setAsk(null);
                        }}
                      >
                        <span>{t.label}</span>
                        <span className="muted small">{t.kind === "game" ? "Game button" : "Hotkey"}</span>
                      </button>
                    ))}
                </div>
                <div className="dialog-actions">
                  <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={() => setAsk(null)}>
                    Cancel
                  </button>
                </div>
              </>
            )}
            {ask.kind === "unsaved" && (
              <div className="dialog-actions">
                <button
                  className="button primary"
                  data-focusable
                  data-autofocus
                  data-hint="Save"
                  type="button"
                  onClick={() => {
                    const then = ask.then;
                    setAsk(null);
                    void save().then(then);
                  }}
                >
                  Save
                </button>
                <button
                  className="button outline"
                  data-focusable
                  data-hint="Discard"
                  type="button"
                  onClick={() => {
                    const then = ask.then;
                    setAsk(null);
                    setWorking(saved);
                    then();
                  }}
                >
                  Discard
                </button>
                <button className="button outline" data-focusable data-hint="Keep editing" type="button" onClick={() => setAsk(null)}>
                  Keep editing
                </button>
              </div>
            )}
            {ask.kind === "promote" && (
              <>
                <p className="body-text">
                  {ask.count === 0
                    ? "No other games change."
                    : `${ask.count} ${ask.count === 1 ? "game uses" : "games use"} the default and will change.`}
                </p>
                <div className="dialog-actions">
                  <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={() => setAsk(null)}>
                    Cancel
                  </button>
                  <button
                    className="button primary"
                    data-focusable
                    data-hint="Make default"
                    type="button"
                    onClick={() => {
                      setAsk(null);
                      void onPromote(difference(working, view.default));
                    }}
                  >
                    Make default
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </main>
  );
}

function askTitle(ask: Ask, view: ControlsView, emulatorName: string): string {
  switch (ask.kind) {
    case "swap":
      return "Swap buttons?";
    case "pick":
      return `What should ${view.inputs.find((i) => i.id === ask.input)?.label} do?`;
    case "unsaved":
      return "Save changes?";
    case "promote":
      return `Make this the ${emulatorName} default?`;
  }
}
