import { useCallback, useEffect, useRef, useState } from "react";
import * as realApi from "./api";
import { asUiError, type ConnectionStatus, type Folder, type SetupDone } from "./api";

/** The calls first run makes; tests pass fakes. */
export interface FirstRunApi {
  connectionStatus: () => Promise<ConnectionStatus>;
  authLogin: () => Promise<string>;
  authCancel: () => Promise<void>;
  driveFolders: (parent: string) => Promise<Folder[]>;
  driveSetup: (parent: string, location: string) => Promise<SetupDone>;
  onSetupProgress: (handler: (path: string) => void) => Promise<() => void>;
}

/** What B does on the current step, and its hint label. */
export interface BackAction {
  label: string;
  run: () => void;
}

interface Props {
  status: ConnectionStatus;
  onDone: () => void;
  setBack: (back: BackAction | null) => void;
  api?: FirstRunApi;
}

type Step = 1 | 2 | 3 | 4;
type SignIn =
  | { state: "idle" }
  | { state: "waiting" }
  | { state: "no_browser" }
  | { state: "error"; message: string };

const ROOT: Folder = { id: "root", name: "My Drive" };

/** Four steps: welcome, sign in, choose a location, done (UI guide section 6). */
export function FirstRun({ status: initial, onDone, setBack, api = realApi }: Props) {
  const [status, setStatus] = useState(initial);
  const [step, setStep] = useState<Step>(initial.signed_in ? 3 : 1);
  const [signIn, setSignIn] = useState<SignIn>({ state: "idle" });
  const [trail, setTrail] = useState<Folder[]>([ROOT]);
  const [reload, setReload] = useState(0);
  // The last listing, tagged with the folder and attempt it answers.
  const [listing, setListing] = useState<{ key: string; folders?: Folder[]; error?: string } | null>(null);
  const [creating, setCreating] = useState<string[] | null>(null);
  const [setupError, setSetupError] = useState<string | null>(null);
  const [done, setDone] = useState<SetupDone | null>(null);
  const column = useRef<HTMLDivElement>(null);

  const here = trail[trail.length - 1];
  const location = trail.map((f) => f.name).join(" / ");
  const listKey = `${here.id}:${reload}`;
  const current = listing?.key === listKey ? listing : null;
  const folders = current?.folders ?? null;
  const listError = current?.error ?? null;

  // Focus the step's main action whenever the content changes shape.
  useEffect(() => {
    const t = setTimeout(() => {
      const root = column.current;
      if (!root || root.contains(document.activeElement)) return;
      const el =
        root.querySelector<HTMLElement>("[data-autofocus]:not([disabled])") ??
        root.querySelector<HTMLElement>("[data-focusable]:not([disabled])");
      el?.focus();
    });
    return () => clearTimeout(t);
  }, [step, signIn, folders, creating, done, status]);

  const startSignIn = useCallback(async () => {
    setSignIn({ state: "waiting" });
    try {
      await api.authLogin();
      const next = await api.connectionStatus();
      setStatus(next);
      setSignIn({ state: "idle" });
      if (next.ready) onDone();
      else setStep(3);
    } catch (e) {
      const err = asUiError(e);
      // A cancel may come from "Can't see a browser?", which has already moved on.
      if (err.kind === "cancelled") setSignIn((s) => (s.state === "waiting" ? { state: "idle" } : s));
      else if (err.kind === "no_browser") setSignIn({ state: "no_browser" });
      else setSignIn({ state: "error", message: err.message });
    }
  }, [api, onDone]);

  const cancelSignIn = useCallback(() => {
    void api.authCancel();
    setSignIn({ state: "idle" });
    setStep(1);
  }, [api]);

  const checkClient = useCallback(async () => {
    try {
      setStatus(await api.connectionStatus());
    } catch (e) {
      setSignIn({ state: "error", message: asUiError(e).message });
    }
  }, [api]);

  // Load the folder list whenever the location changes.
  useEffect(() => {
    if (step !== 3) return;
    let live = true;
    api
      .driveFolders(here.id)
      .then((f) => live && setListing({ key: listKey, folders: f }))
      .catch((e) => live && setListing({ key: listKey, error: asUiError(e).message }));
    return () => {
      live = false;
    };
  }, [api, step, here.id, listKey]);

  const goUp = useCallback(() => {
    setTrail((t) => (t.length > 1 ? t.slice(0, -1) : t));
    (document.activeElement as HTMLElement | null)?.blur();
  }, []);

  const enter = useCallback((f: Folder) => {
    setTrail((t) => [...t, f]);
    (document.activeElement as HTMLElement | null)?.blur();
  }, []);

  const create = useCallback(async () => {
    setCreating([]);
    setSetupError(null);
    const unlisten = await api.onSetupProgress((path) => setCreating((c) => [...(c ?? []), path]));
    try {
      const result = await api.driveSetup(here.id, location);
      setDone(result);
      setStep(4);
    } catch (e) {
      setSetupError(asUiError(e).message);
    } finally {
      unlisten();
      setCreating(null);
    }
  }, [api, here.id, location]);

  // B: cancel sign-in on step 2, go up a folder on step 3.
  useEffect(() => {
    if (step === 2 && signIn.state === "waiting") setBack({ label: "Cancel", run: cancelSignIn });
    else if (step === 2) setBack({ label: "Back", run: () => setStep(1) });
    else if (step === 3 && !creating && trail.length > 1) setBack({ label: "Up a folder", run: goUp });
    else setBack(null);
  }, [step, signIn, creating, trail.length, setBack, cancelSignIn, goUp]);
  useEffect(() => () => setBack(null), [setBack]);

  const hasEmulation = folders?.some((f) => f.name === "Emulation") ?? false;

  return (
    <main className="content first-run">
      <div className="first-run-column" ref={column}>
        <span className="mono muted step-count">Step {step} of 4</span>

        {step === 1 && (
          <>
            <h1 className="screen-title">Welcome to Romshelf</h1>
            <p className="body-text">
              Romshelf keeps your games in your own Google Drive and gets them ready to play on this Deck. It
              downloads ROMs, sets up emulators, backs up your saves and adds each game to Steam.
            </p>
            <p className="body-text">Romshelf doesn&rsquo;t include any games. You add your own files to Drive.</p>
            <div className="actions">
              <button
                className="button primary"
                data-focusable
                data-autofocus
                data-hint="Connect"
                type="button"
                onClick={() => setStep(2)}
              >
                Connect Google Drive
              </button>
            </div>
          </>
        )}

        {step === 2 && !status.client_configured && (
          <>
            <h1 className="screen-title">Set up your Google client</h1>
            <p className="body-text">Romshelf signs in with your own Google Cloud client. Add it once:</p>
            <ol className="steps">
              <li>
                On a computer, follow <span className="mono">docs/google-setup.md</span> to create a Desktop app
                client and download its JSON file.
              </li>
              <li>Copy the file to this Deck.</li>
              <li>
                In Desktop Mode, open Konsole and run{" "}
                <span className="mono">~/Applications/romshelf.AppImage auth set-client --file &lt;file&gt;</span>
              </li>
            </ol>
            {signIn.state === "error" && <p className="error">{signIn.message}</p>}
            <div className="actions">
              <button
                className="button primary"
                data-focusable
                data-autofocus
                data-hint="Check"
                type="button"
                onClick={() => void checkClient()}
              >
                Check again
              </button>
              <button className="button outline" data-focusable data-hint="Cancel" type="button" onClick={() => setStep(1)}>
                Cancel
              </button>
            </div>
          </>
        )}

        {step === 2 && status.client_configured && (
          <>
            <h1 className="screen-title">Sign in to Google</h1>
            {signIn.state === "no_browser" ? (
              <>
                <p className="body-text">No browser could be opened here. Finish sign-in in Desktop Mode:</p>
                <ol className="steps">
                  <li>Press the Steam button, choose Power, then Switch to Desktop.</li>
                  <li>
                    Open Konsole and run <span className="mono">~/Applications/romshelf.AppImage auth login</span>
                  </li>
                  <li>Sign in to Google in the browser that opens.</li>
                  <li>Return to Gaming Mode and open Romshelf again. You stay signed in.</li>
                </ol>
              </>
            ) : signIn.state === "waiting" ? (
              <>
                <p className="body-text">Waiting for sign-in…</p>
                <p className="body-text muted">Finish signing in to Google in the browser, then come back here.</p>
              </>
            ) : (
              <p className="body-text">A browser will open for Google sign-in.</p>
            )}
            {signIn.state === "error" && <p className="error">{signIn.message}</p>}
            <div className="actions">
              {signIn.state === "waiting" ? (
                <>
                  <button
                    className="button outline"
                    data-focusable
                    data-autofocus
                    data-hint="Cancel"
                    type="button"
                    onClick={cancelSignIn}
                  >
                    Cancel
                  </button>
                  <button
                    className="button outline"
                    data-focusable
                    data-hint="Show steps"
                    type="button"
                    onClick={() => {
                      void api.authCancel();
                      setSignIn({ state: "no_browser" });
                    }}
                  >
                    Can&rsquo;t see a browser?
                  </button>
                </>
              ) : (
                <>
                  <button
                    className="button primary"
                    data-focusable
                    data-autofocus
                    data-hint="Open"
                    type="button"
                    onClick={() => void startSignIn()}
                  >
                    {signIn.state === "idle" ? "Open browser" : "Try again"}
                  </button>
                  <button className="button outline" data-focusable data-hint="Cancel" type="button" onClick={cancelSignIn}>
                    Cancel
                  </button>
                </>
              )}
            </div>
          </>
        )}

        {step === 3 && (
          <>
            <h1 className="screen-title">Choose where to keep your games</h1>
            <span className="mono path">{location}</span>
            {creating ? (
              <div className="progress-list">
                <p className="body-text">Creating folders…</p>
                <pre className="mono tree">{creating.slice(-6).join("\n")}</pre>
              </div>
            ) : (
              <>
                <div className="folder-list" aria-label="Folders">
                  {listError && <p className="error">{listError}</p>}
                  {!folders && !listError && <p className="muted">Loading folders…</p>}
                  {folders && folders.length === 0 && <p className="muted">No folders here.</p>}
                  {folders?.map((f) => (
                    <button
                      key={f.id}
                      className="row"
                      data-focusable
                      data-hint="Open folder"
                      type="button"
                      onClick={() => enter(f)}
                    >
                      <span>{f.name}</span>
                      <span className="row-chevron" aria-hidden>
                        ›
                      </span>
                    </button>
                  ))}
                </div>
                {hasEmulation && (
                  <p className="body-text muted">
                    This folder already has an Emulation folder. Romshelf will use it and add anything missing.
                  </p>
                )}
                {setupError && <p className="error">{setupError}</p>}
                <div className="actions">
                  <button
                    className="button primary"
                    data-focusable
                    data-autofocus
                    data-hint="Create"
                    type="button"
                    disabled={!folders}
                    onClick={() => void create()}
                  >
                    Create Emulation folder here
                  </button>
                  {listError && (
                    <button
                      className="button outline"
                      data-focusable
                      data-hint="Retry"
                      type="button"
                      onClick={() => setReload((n) => n + 1)}
                    >
                      Try again
                    </button>
                  )}
                </div>
              </>
            )}
          </>
        )}

        {step === 4 && done && (
          <>
            <h1 className="screen-title">Your Drive is ready</h1>
            <span className="mono path">{done.path}</span>
            <pre className="mono tree">{done.tree.join("\n")}</pre>
            <p className="body-text">
              Upload ROMs to <span className="mono">Emulation/roms/&lt;system&gt;</span> in Google Drive from any
              device, and they appear here.
            </p>
            <div className="actions">
              <button
                className="button primary"
                data-focusable
                data-autofocus
                data-hint="Open"
                type="button"
                onClick={onDone}
              >
                Open Library
              </button>
            </div>
          </>
        )}
      </div>
    </main>
  );
}
