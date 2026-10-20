import type { EmulatorView } from "./api";

/** What the interface knows about a setup in progress or just finished. */
export interface SetupState {
  /** "Installing", "Setting up controls", … */
  step: string | null;
  percent: number | null;
  running: boolean;
  /** Why it failed, or what is missing (BIOS) after it finished. */
  message: string | null;
  failed: boolean;
}

/** The status line for a row: progress while setting up, otherwise the emulator's status. */
export function rowStatus(e: EmulatorView, s?: SetupState): { text: string; warn: boolean } {
  if (s?.running || e.setting_up) {
    const step = s?.step ?? "Setting up";
    return { text: s?.percent != null ? `${step}… ${s.percent}%` : `${step}…`, warn: false };
  }
  if (s?.failed && s.message) return { text: s.message, warn: true };
  return { text: e.status_label, warn: e.status !== "installed" };
}

/** The row's button: "Set up" until installed, then "Repair" (runs setup again). */
export function rowAction(e: EmulatorView, s?: SetupState): string {
  if (s?.failed) return "Try again";
  return e.status === "installed" ? "Repair" : "Set up";
}

interface Props {
  list: EmulatorView[] | null;
  error: string | null;
  setups: Record<string, SetupState>;
  onSetup: (id: string, repair: boolean) => void;
  /** Opens the Controls editor on an emulator's default for one system. */
  onEditControls?: (emulatorId: string, system: string) => void;
  onBack: () => void;
}

/** The Emulators screen (UI guide section 6). Setup keeps running if you leave the screen. */
export function Emulators({ list, error, setups, onSetup, onEditControls, onBack }: Props) {
  return (
    <main className="content emulators">
      <button className="back-link" data-focusable data-hint="Back" type="button" onClick={onBack}>
        ‹ Library
      </button>
      <h1 className="screen-title">Emulators</h1>
      {error && <p className="error">{error}</p>}
      {!list && !error && <p className="muted">Checking emulators…</p>}
      {list && (
        <div className="emulator-list">
          {list.map((e) => {
            const s = setups[e.id];
            const status = rowStatus(e, s);
            const busy = Boolean(s?.running || e.setting_up);
            return (
              <div className="emulator-row" key={e.id}>
                <div className="emulator-text">
                  <span className="card-heading">{e.name}</span>
                  <span className="mono muted small">{e.systems.join(", ")}</span>
                  <span className={status.warn ? "tile-status warn" : "tile-status"}>{status.text}</span>
                  {busy && s?.percent != null && (
                    <div className="progress-track" aria-label={`${e.name} setup progress`}>
                      <div className="progress-fill" style={{ width: `${s.percent}%` }} />
                    </div>
                  )}
                  {!busy && !s?.failed && s?.message && <span className="tile-status warn">{s.message}</span>}
                  {e.status === "installed" && onEditControls && (
                    <div className="dialog-actions start">
                      {e.systems.map((label) => (
                        <button
                          key={label}
                          className="button outline"
                          data-focusable
                          data-hint="Edit"
                          type="button"
                          onClick={() => onEditControls(e.id, label.toLowerCase())}
                        >
                          {label} controls
                        </button>
                      ))}
                    </div>
                  )}
                </div>
                <button
                  className={e.status === "installed" && !s?.failed ? "button outline" : "button primary"}
                  data-focusable
                  data-hint={rowAction(e, s)}
                  type="button"
                  disabled={busy}
                  onClick={() => onSetup(e.id, e.status === "installed")}
                >
                  {rowAction(e, s)}
                </button>
              </div>
            );
          })}
        </div>
      )}
      <p className="muted small">
        Romshelf installs emulators from Flathub for your user. It keeps its own settings in a separate folder and
        never changes the settings you or other tools (such as EmuDeck) made.
      </p>
    </main>
  );
}
