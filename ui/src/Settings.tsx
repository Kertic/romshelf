import { useEffect, useState } from "react";
import type { SettingsView } from "./api";
import type { BackAction } from "./FirstRun";
import { humanSize, useDialogFocus } from "./RomActions";

export interface SettingsActions {
  signOut: () => Promise<void>;
  changeDriveFolder: () => void;
  changeStorage: (path: string) => Promise<void>;
  setSaveVersions: (n: number) => Promise<void>;
  setAutoAdd: (on: boolean) => Promise<void>;
  checkForUpdates: () => Promise<void>;
  diagnostics: () => Promise<string>;
  copy: (text: string) => Promise<void>;
  /** The gamepad test from stage 0, and quitting Romshelf. */
  openGamepadTest?: () => void;
  /** Everything Romshelf can check by itself, then a few guided steps; saves a report. */
  openSelfTest?: () => void;
  quit?: () => void;
}

interface Props {
  view: SettingsView;
  actions: SettingsActions;
  onBack: () => void;
  setBack: (b: BackAction | null) => void;
  /** Reads the settings again after a change. */
  reload: () => void;
}

const message = (e: unknown): string =>
  e && typeof e === "object" && "message" in e ? String(e.message) : `Something went wrong: ${String(e)}. Try again.`;

type Ask = "sign-out" | "storage" | null;

/** Settings (UI guide section 6), reached with Menu. */
export function Settings({ view, actions, onBack, setBack, reload }: Props) {
  const [ask, setAsk] = useState<Ask>(null);
  const [note, setNote] = useState<{ group: string; text: string; error: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const dialog = useDialogFocus(ask ?? "none");

  useEffect(() => {
    setBack(ask ? { label: "Back", run: () => setAsk(null) } : { label: "Library", run: onBack });
  }, [ask, onBack, setBack]);

  const run = async (group: string, f: () => Promise<string | void>) => {
    setBusy(true);
    setNote(null);
    try {
      const text = await f();
      if (text) setNote({ group, text, error: false });
      reload();
    } catch (e) {
      setNote({ group, text: message(e), error: true });
    }
    setBusy(false);
  };
  const noteFor = (group: string) =>
    note?.group === group ? <p className={note.error ? "error" : "body-text"}>{note.text}</p> : null;

  return (
    <main className="content settings">
      <button className="back-link" data-focusable data-hint="Back" type="button" onClick={onBack}>
        ‹ Library
      </button>
      <h1 className="screen-title">Settings</h1>
      <div className="settings-groups">
        <section className="card settings-group" aria-label="Google account">
          <h2 className="group-label">Google account</h2>
          <span className="body-text">{view.account ?? "Not signed in"}</span>
          {noteFor("account")}
          {view.account && (
            <button className="button outline" data-focusable data-hint="Sign out" type="button" onClick={() => setAsk("sign-out")}>
              Sign out
            </button>
          )}
        </section>

        <section className="card settings-group" aria-label="Drive folder">
          <h2 className="group-label">Drive folder</h2>
          <span className="mono body-text">{view.drive_folder ?? "Not chosen"}</span>
          <button className="button outline" data-focusable data-hint="Change" type="button" onClick={actions.changeDriveFolder}>
            Change location…
          </button>
        </section>

        <section className="card settings-group" aria-label="Storage">
          <h2 className="group-label">Storage</h2>
          <span className="mono body-text">{view.storage_root}</span>
          <span className={view.storage_available ? "muted small" : "tile-status warn"}>
            {view.storage_available
              ? view.storage_free != null
                ? `${humanSize(view.storage_free)} free`
                : ""
              : "Not available. Insert the SD card that holds your ROMs."}
          </span>
          {noteFor("storage")}
          <button className="button outline" data-focusable data-hint="Change" type="button" disabled={busy} onClick={() => setAsk("storage")}>
            Change location…
          </button>
        </section>

        <section className="card settings-group" aria-label="Saves">
          <h2 className="group-label">Saves</h2>
          <span className="body-text">Versions to keep in Drive for each game</span>
          <div className="stepper">
            <button
              className="button outline"
              data-focusable
              data-hint="Fewer"
              type="button"
              aria-label="Keep fewer versions"
              disabled={busy || view.save_versions <= 1}
              onClick={() => void run("saves", () => actions.setSaveVersions(view.save_versions - 1))}
            >
              −
            </button>
            <span className="stepper-value" aria-live="polite">
              {view.save_versions}
            </span>
            <button
              className="button outline"
              data-focusable
              data-hint="More"
              type="button"
              aria-label="Keep more versions"
              disabled={busy || view.save_versions >= 50}
              onClick={() => void run("saves", () => actions.setSaveVersions(view.save_versions + 1))}
            >
              +
            </button>
          </div>
          {noteFor("saves")}
        </section>

        <section className="card settings-group" aria-label="Steam">
          <h2 className="group-label">Steam</h2>
          <label className="check">
            <input
              type="checkbox"
              data-focusable
              data-hint="Toggle"
              checked={view.auto_add}
              onChange={(e) => void run("steam", () => actions.setAutoAdd(e.target.checked))}
            />
            Add new ROMs automatically
          </label>
          <span className="muted small">New games appear in Steam after it restarts.</span>
          {noteFor("steam")}
        </section>

        <section className="card settings-group" aria-label="About">
          <h2 className="group-label">About</h2>
          <span className="body-text">Romshelf {view.version}</span>
          {view.update_ready && (
            <span className="body-text accent">Romshelf {view.update_ready} is ready. Restart Romshelf to use it.</span>
          )}
          {noteFor("about")}
          <div className="dialog-actions start">
            <button
              className="button outline"
              data-focusable
              data-hint="Check"
              type="button"
              disabled={busy}
              onClick={() =>
                void run("about", async () => {
                  await actions.checkForUpdates();
                  return "Checking for a newer Romshelf. It installs in the background if there is one.";
                })
              }
            >
              Check for updates
            </button>
            <button
              className="button outline"
              data-focusable
              data-hint="Copy"
              type="button"
              disabled={busy}
              onClick={() =>
                void run("about", async () => {
                  await actions.copy(await actions.diagnostics());
                  return "Diagnostics copied. Paste them wherever you're asking for help.";
                })
              }
            >
              Copy diagnostics
            </button>
            {actions.openSelfTest && (
              <button className="button outline" data-focusable data-hint="Run" type="button" onClick={actions.openSelfTest}>
                Run the self-test
              </button>
            )}
            {actions.openGamepadTest && (
              <button className="button outline" data-focusable data-hint="Open" type="button" onClick={actions.openGamepadTest}>
                Gamepad test
              </button>
            )}
            {actions.quit && (
              <button className="button outline" data-focusable data-hint="Quit" type="button" onClick={actions.quit}>
                Quit Romshelf
              </button>
            )}
          </div>
        </section>
      </div>

      {ask === "sign-out" && (
        <div className="dialog-backdrop">
          <div className="dialog" role="dialog" aria-modal="true" aria-label="Sign out of Google?" data-dialog ref={dialog}>
            <h2 className="dialog-title">Sign out of Google?</h2>
            <p className="body-text">
              Romshelf stops syncing and backing up saves until you sign in again. Your games and saves stay in Drive and
              on this Deck.
            </p>
            <div className="dialog-actions">
              <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={() => setAsk(null)}>
                Cancel
              </button>
              <button
                className="button outline"
                data-focusable
                data-hint="Sign out"
                type="button"
                onClick={() => {
                  setAsk(null);
                  void run("account", async () => {
                    await actions.signOut();
                    return "Signed out.";
                  });
                }}
              >
                Sign out
              </button>
            </div>
          </div>
        </div>
      )}

      {ask === "storage" && (
        <div className="dialog-backdrop">
          <div className="dialog" role="dialog" aria-modal="true" aria-label="Where should ROMs go?" data-dialog ref={dialog}>
            <h2 className="dialog-title">Where should ROMs go?</h2>
            <p className="body-text">Downloaded ROMs move to the new place, then Romshelf restarts.</p>
            <div className="folder-list">
              {view.storage_choices.map((c) => (
                <button
                  key={c.path}
                  className={c.current ? "row selected" : "row"}
                  data-focusable
                  data-hint={c.current ? "Current" : "Move here"}
                  type="button"
                  disabled={c.current}
                  onClick={() => {
                    setAsk(null);
                    void run("storage", () => actions.changeStorage(c.path));
                  }}
                >
                  <span>
                    {c.label}
                    {c.current ? " · current" : ""}
                  </span>
                  <span className="muted small">
                    {c.free != null ? `${humanSize(c.free)} free` : ""}
                  </span>
                </button>
              ))}
            </div>
            <div className="dialog-actions">
              <button className="button outline" data-focusable data-autofocus data-hint="Cancel" type="button" onClick={() => setAsk(null)}>
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </main>
  );
}
