import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  answerSaveQuestion,
  controlsView,
  promoteControls,
  promoteCount,
  resetControls,
  saveControls,
  appInfo,
  backupSave,
  cancelDownload,
  deleteSaveState,
  connectionStatus,
  download,
  emulators as listEmulators,
  launchTarget,
  library,
  onLibraryChanged,
  onSyncStatus,
  onTransfer,
  onEmulatorSetup,
  onLaunchUpdate,
  onSaveBackedUp,
  onUpdateReady,
  play,
  quit,
  removeFromDeck,
  restoreSave,
  saveStates,
  saveVersions,
  setupEmulator,
  storageStatus,
  syncNow,
  syncStatus,
  updateReady,
  type AppInfo,
  type EmulatorView,
  type ConnectionStatus,
  type LibraryEntry,
  type SaveChoice,
  type ControlsView,
  type Progress,
  type StorageStatus,
  type SyncStatus,
} from "./api";
import { FirstRun, type BackAction } from "./FirstRun";
import { focusables, moveFocus, type Direction } from "./focus";
import { Footer, type Hint } from "./Footer";
import { GamepadSpike } from "./GamepadSpike";
import { Emulators, type SetupState } from "./Emulators";
import { LaunchWindow, type LaunchState } from "./LaunchWindow";
import { Controls, type KeyHandler } from "./Controls";
import { Home } from "./Home";
import type { Button } from "./input";
import { nextTab } from "./library";
import { RomDialog, type SaveActions } from "./RomActions";
import { useInput } from "./useInput";

type Screen = "loading" | "first-run" | "home" | "emulators" | "launch" | "controls" | "spike";

const HOLD_TO_LEAVE_MS = 1000;

export function App() {
  const [screen, setScreen] = useState<Screen>("loading");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [status, setStatus] = useState<ConnectionStatus | null>(null);
  const [entries, setEntries] = useState<LibraryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [focusHint, setFocusHint] = useState<string | null>(null);
  const [focusHintX, setFocusHintX] = useState<string | null>(null);
  const [back, setBackState] = useState<BackAction | null>(null);
  const backRef = useRef<BackAction | null>(null);
  const [newVersion, setNewVersion] = useState<string | null>(null);
  const [tab, setTab] = useState("all");
  const [sync, setSync] = useState<SyncStatus | null>(null);
  /** The game whose actions dialog is open. */
  const [dialogRom, setDialogRom] = useState<string | null>(null);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  /** The last thing that went wrong with each game's download. */
  const [transferErrors, setTransferErrors] = useState<Record<string, string>>({});
  const [storage, setStorage] = useState<StorageStatus | null>(null);
  const [emulatorList, setEmulatorList] = useState<EmulatorView[] | null>(null);
  const [emulatorError, setEmulatorError] = useState<string | null>(null);
  /** Emulator setups running or just finished, by emulator id. */
  const [setups, setSetups] = useState<Record<string, SetupState>>({});
  /** A game being prepared or played, from Play or the launch window. */
  const [launch, setLaunch] = useState<LaunchState | null>(null);
  /** A one-line toast ("Backed up Tidewalker"), shown for four seconds. */
  const [toast, setToast] = useState<string | null>(null);
  /** The Controls editor: which layout, and where B goes back to. */
  const [controlsFor, setControlsFor] = useState<{ emulatorId: string; system: string; romId: string | null; from: Screen } | null>(null);
  const [controls, setControls] = useState<ControlsView | null>(null);
  const [controlsError, setControlsError] = useState<string | null>(null);
  /** The editor sees presses first (rebinding, X, Y, L1, R1). */
  const keyHandler = useRef<KeyHandler | null>(null);
  const registerKeys = useCallback((h: KeyHandler | null) => {
    keyHandler.current = h;
  }, []);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(t);
  }, [toast]);
  const setBack = useCallback((b: BackAction | null) => {
    backRef.current = b;
    setBackState(b);
  }, []);
  const reloadLibrary = useCallback(() => {
    library()
      .then(setEntries)
      .catch((e) => setError(`Couldn't read the library: ${String(e)}. Restart Romshelf.`));
    storageStatus()
      .then(setStorage)
      .catch(() => {});
  }, []);
  const startSync = useCallback(() => {
    setSync((s) => ({ state: "syncing", last_synced_at: s?.last_synced_at ?? null, message: null }));
    syncNow()
      .then(setSync)
      .catch((e) => setSync({ state: "error", last_synced_at: null, message: `Couldn't sync: ${String(e)}. Try again.` }));
  }, []);

  // Sync results arrive as events too (a sync started elsewhere), and a changed library is read again.
  useEffect(() => {
    let live = true;
    const unlisteners: (() => void)[] = [];
    syncStatus().then((s) => live && setSync((cur) => cur ?? s)).catch(() => {});
    onSyncStatus((s) => setSync(s)).then((u) => (live ? unlisteners.push(u) : u()));
    onLibraryChanged(reloadLibrary).then((u) => (live ? unlisteners.push(u) : u()));
    return () => {
      live = false;
      unlisteners.forEach((u) => u());
    };
  }, [reloadLibrary]);

  // Download progress and results.
  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | null = null;
    onTransfer((e) => {
      if (e.kind === "progress") {
        setProgress((p) => ({ ...p, [e.rom_id]: { bytes: e.bytes, total: e.total, rate: e.rate } }));
        return;
      }
      setProgress((p) => {
        const next = { ...p };
        delete next[e.rom_id];
        return next;
      });
      setTransferErrors((errs) => {
        const next = { ...errs };
        if (e.message) next[e.rom_id] = e.message;
        else delete next[e.rom_id];
        return next;
      });
    }).then((u) => (live ? (unlisten = u) : u()));
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);

  const loadEmulators = useCallback(() => {
    setEmulatorError(null);
    listEmulators()
      .then(setEmulatorList)
      .catch((e) => setEmulatorError(e?.message ?? `Couldn't check the emulators: ${String(e)}. Try again.`));
  }, []);

  // Emulator setup progress; a finished setup changes the library's "Needs …" too.
  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | null = null;
    onEmulatorSetup((u) => {
      setSetups((all) => {
        const prev = all[u.emulator_id];
        const running = u.kind === "step" || u.kind === "progress";
        return {
          ...all,
          [u.emulator_id]: {
            step: u.kind === "step" ? u.step : (prev?.step ?? null),
            percent: u.kind === "progress" ? u.percent : u.kind === "step" ? null : (prev?.percent ?? null),
            running,
            message: running ? null : u.message,
            failed: u.kind === "failed",
          },
        };
      });
      if (u.kind === "done" || u.kind === "failed") loadEmulators();
    }).then((un) => (live ? (unlisten = un) : un()));
    return () => {
      live = false;
      unlisten?.();
    };
  }, [loadEmulators]);

  // Progress while a game is prepared; a finished game clears it, a failure keeps the reason.
  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | null = null;
    onLaunchUpdate((u) => {
      if (u.kind === "notice") {
        if (u.message) setToast(u.message);
        return;
      }
      setLaunch((cur) => {
        if (u.kind === "finished") return null;
        const base: LaunchState = cur?.romId === u.rom_id ? cur : { romId: u.rom_id, label: null, percent: null, running: true, error: null };
        if (u.kind === "failed") return { ...base, running: false, error: u.message ?? "The game couldn't start. Try again." };
        if (u.kind === "question") return { ...base, conflict: u.conflict };
        if (u.kind === "step") return { ...base, step: u.step, label: u.label, percent: null, running: true, error: null };
        return { ...base, percent: u.percent };
      });
    }).then((un) => (live ? (unlisten = un) : un()));
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);

  const startPlay = useCallback((romId: string) => {
    setLaunch({ romId, label: null, percent: null, running: true, error: null });
    play(romId).catch((e) =>
      setLaunch({ romId, label: null, percent: null, running: false, error: e?.message ?? `Couldn't start the game: ${String(e)}` }),
    );
  }, []);

  // Owed backups that reach Drive in the background.
  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | null = null;
    onSaveBackedUp((m) => setToast(m)).then((un) => (live ? (unlisten = un) : un()));
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);

  const answerSave = useCallback((choice: SaveChoice) => {
    setLaunch((cur) => (cur ? { ...cur, conflict: null } : cur));
    void answerSaveQuestion(choice);
  }, []);

  const startSetup = useCallback((id: string, repair = false) => {
    setSetups((all) => ({ ...all, [id]: { step: "Starting", percent: null, running: true, message: null, failed: false } }));
    setupEmulator(id, repair).catch((e) =>
      setSetups((all) => ({
        ...all,
        [id]: { step: null, percent: null, running: false, failed: true, message: e?.message ?? String(e) },
      })),
    );
  }, []);

  const openEmulators = useCallback(() => {
    setScreen("emulators");
    // B goes back to the Library.
    setBack({
      label: "Library",
      run: () => {
        setBack(null);
        setScreen("home");
      },
    });
    loadEmulators();
  }, [loadEmulators, setBack]);

  const loadControls = useCallback((t: { emulatorId: string; system: string; romId: string | null }) => {
    setControlsError(null);
    controlsView(t.emulatorId, t.system, t.romId)
      .then(setControls)
      .catch((e) => setControlsError(e?.message ?? `Couldn't read the controls: ${String(e)}. Try again.`));
  }, []);
  const openControls = useCallback(
    (emulatorId: string, system: string, romId: string | null, from: Screen) => {
      const t = { emulatorId, system, romId, from };
      setControlsFor(t);
      setControls(null);
      setDialogRom(null);
      setScreen("controls");
      loadControls(t);
    },
    [loadControls],
  );

  // A newer build may finish downloading at any time while Romshelf is open.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let live = true;
    updateReady().then((v) => live && v && setNewVersion(v)).catch(() => {});
    onUpdateReady((v) => setNewVersion(v)).then((u) => (live ? (unlisten = u) : u()));
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);
  const startDownload = useCallback(
    (romId: string) => {
      setTransferErrors((errs) => {
        const next = { ...errs };
        delete next[romId];
        return next;
      });
      download(romId)
        .then(reloadLibrary)
        .catch((e) =>
          setTransferErrors((errs) => ({
            ...errs,
            [romId]: e?.message ?? `Couldn't start the download: ${String(e)}. Try again.`,
          })),
        );
    },
    [reloadLibrary],
  );
  const closeDialog = useCallback(() => {
    const opener = dialogRom;
    setDialogRom(null);
    setBack(null);
    // Focus returns to the tile that opened the dialog.
    setTimeout(() => {
      if (opener) document.querySelector<HTMLElement>(`[data-rom-id="${opener}"]`)?.focus();
    });
  }, [dialogRom, setBack]);
  const openLibrary = useCallback(() => {
    Promise.all([connectionStatus(), library(), storageStatus()])
      .then(([s, l, st]) => {
        setStatus(s);
        setEntries(l);
        setStorage(st);
        setScreen("home");
        // Sync on start, after the last known library is on screen.
        startSync();
      })
      .catch((e) => {
        setError(`Couldn't read the library: ${String(e)}. Restart Romshelf.`);
        setScreen("home");
      });
  }, [startSync]);
  useEffect(() => {
    Promise.all([appInfo(), connectionStatus(), launchTarget()])
      .then(([i, s, target]) => {
        setInfo(i);
        setStatus(s);
        if (target) {
          // Opened by `romshelf launch`: just this game, then back to Steam.
          setScreen("launch");
          library().then(setEntries).catch(() => {});
          startPlay(target);
          setBack({ label: "Cancel", run: () => { void cancelDownload(target).finally(() => void quit()); } });
        } else if (s.ready) openLibrary();
        else setScreen("first-run");
      })
      .catch((e) => {
        setError(`Couldn't read Romshelf's settings: ${String(e)}. Restart Romshelf.`);
        setScreen("home");
      });
  }, [openLibrary, startPlay, setBack]);

  // Read the hints from whatever holds focus: after pad moves, and on touch or mouse focus.
  const syncHint = useCallback(() => {
    const el = document.activeElement as HTMLElement | null;
    setFocusHint(el?.dataset?.hint ?? null);
    setFocusHintX(el?.dataset?.hintX ?? null);
  }, []);
  useEffect(() => {
    document.addEventListener("focusin", syncHint);
    return () => document.removeEventListener("focusin", syncHint);
  }, [syncHint]);
  const onPress = useCallback(
    (b: Button, from: "pad" | "keyboard") => {
      if (screen === "controls" && keyHandler.current?.(b, from)) return;
      if (b === "up" || b === "down" || b === "left" || b === "right") {
        // An open dialog keeps focus inside it.
        moveFocus(b as Direction, document.querySelector("[data-dialog]") ?? document);
        syncHint();
      } else if (b === "a" && screen !== "spike") {
        (document.activeElement as HTMLElement | null)?.click();
      } else if (b === "x" && screen === "home" && !dialogRom) {
        // X on a tile that is only in Drive downloads it (UI guide section 4).
        const tile = document.activeElement as HTMLElement | null;
        if (tile?.dataset.hintX && tile.dataset.romId) startDownload(tile.dataset.romId);
      } else if ((b === "l1" || b === "r1") && screen === "home" && !dialogRom) {
        setTab((t) => nextTab(t, b === "l1" ? -1 : 1));
      } else if (b === "b" && screen !== "spike") {
        backRef.current?.run();
      } else if (b === "b" && screen === "spike" && from === "keyboard") {
        // Escape on a keyboard leaves at once; a pad's B must be held.
        setScreen("home");
      }
    },
    [screen, syncHint, dialogRom, startDownload],
  );
  const input = useInput(onPress);

  // On the gamepad test every button is under test, so leaving needs B held for one second.
  // View + Menu held for a second leaves the launch window for Steam while it downloads or sets
  // up (during the game itself, Romshelf's own watcher stops the emulator).
  const holdingQuit =
    screen === "launch" && launch?.step !== "run" && input.held.includes("view") && input.held.includes("menu");
  useEffect(() => {
    if (!holdingQuit) return;
    const t = setTimeout(() => backRef.current?.run(), HOLD_TO_LEAVE_MS);
    return () => clearTimeout(t);
  }, [holdingQuit]);
  const holdingB = screen === "spike" && input.held.includes("b");
  useEffect(() => {
    if (!holdingB) return;
    const t = setTimeout(() => setScreen("home"), HOLD_TO_LEAVE_MS);
    return () => clearTimeout(t);
  }, [holdingB]);
  // The Emulators screen: focus starts on the first emulator's button.
  useEffect(() => {
    if (screen !== "emulators") return;
    const t = setTimeout(() => {
      if (document.activeElement && document.activeElement !== document.body) return;
      (document.querySelector<HTMLElement>(".emulator-row button:not([disabled])") ?? focusables()[0])?.focus();
      syncHint();
    });
    return () => clearTimeout(t);
  }, [screen, emulatorList, syncHint]);

  const focusedTab = useRef<string | null>(null);
  useEffect(() => {
    if (screen !== "home") {
      focusedTab.current = null;
      return;
    }
    // Focus the first tile (or the first action when there are none) on arrival and on a tab
    // change. A library reload after a sync keeps focus where it is if that element remains.
    const timer = setTimeout(() => {
      const active = document.activeElement as HTMLElement | null;
      const kept = active && active !== document.body && document.body.contains(active);
      if (focusedTab.current === tab && kept) return;
      focusedTab.current = tab;
      (document.querySelector<HTMLElement>(".tile[data-focusable]") ?? focusables()[0])?.focus();
      syncHint();
    });
    return () => clearTimeout(timer);
  }, [screen, entries, tab, syncHint]);
  const closeControls = useCallback(() => {
    const f = controlsFor;
    setControlsFor(null);
    setControls(null);
    setBack(null);
    reloadLibrary();
    if (f?.from === "emulators") openEmulators();
    else {
      setScreen("home");
      // Back to the game's dialog it was opened from.
      if (f?.romId) setDialogRom(f.romId);
    }
  }, [controlsFor, setBack, reloadLibrary, openEmulators]);
  const dialogEntry = dialogRom ? (entries?.find((e) => e.rom.id === dialogRom) ?? null) : null;
  // The open game's save actions; a change to its save reads the library again.
  const dialogSaves = useMemo<SaveActions | null>(() => {
    if (!dialogRom) return null;
    const after = <T,>(p: Promise<T>) => p.finally(reloadLibrary);
    return {
      backup: () => after(backupSave(dialogRom)),
      versions: () => saveVersions(dialogRom),
      restore: (versionId: string) => after(restoreSave(dialogRom, versionId)),
      states: () => saveStates(dialogRom),
      deleteState: (slot: string) => deleteSaveState(dialogRom, slot),
    };
  }, [dialogRom, reloadLibrary]);
  const hints: Hint[] =
    screen === "spike"
      ? [{ glyph: "B", label: "Hold to go back" }]
      : [
          ...(focusHint ? [{ glyph: "A", label: focusHint }] : []),
          ...(focusHintX && !dialogRom ? [{ glyph: "X", label: focusHintX }] : []),
          ...(screen === "controls" && controlsFor
            ? [
                { glyph: "X", label: "Clear" },
                ...(controlsFor.romId ? [{ glyph: "Y", label: "Back to default" }] : []),
              ]
            : []),
          ...(back ? [{ glyph: "B", label: back.label }] : []),
          { glyph: "✥", label: "Move" },
        ];
  return (
    <div className="frame">
      {screen === "loading" && <main className="content" />}
      {screen === "first-run" && status && <FirstRun status={status} onDone={openLibrary} setBack={setBack} />}
      {screen === "home" && (
        <Home
          info={info}
          entries={entries}
          error={error}
          driveFolder={status?.drive_folder ?? null}
          newVersion={newVersion}
          tab={tab}
          onTab={setTab}
          sync={sync}
          onSync={startSync}
          progress={progress}
          storage={storage}
          onOpen={setDialogRom}
          onOpenSpike={() => setScreen("spike")}
          onOpenEmulators={openEmulators}
          onQuit={() => void quit()}
        />
      )}
      {screen === "home" && dialogEntry && (
        <RomDialog
          key={dialogEntry.rom.id}
          entry={dialogEntry}
          progress={progress[dialogEntry.rom.id]}
          error={transferErrors[dialogEntry.rom.id] ?? null}
          onDownload={() => startDownload(dialogEntry.rom.id)}
          onCancel={() => void cancelDownload(dialogEntry.rom.id).then(reloadLibrary)}
          onRemove={async () => {
            await removeFromDeck(dialogEntry.rom.id);
            closeDialog();
            reloadLibrary();
          }}
          onSetupEmulator={() => dialogEntry.emulator_id && startSetup(dialogEntry.emulator_id)}
          onPlay={() => startPlay(dialogEntry.rom.id)}
          launch={launch?.romId === dialogEntry.rom.id ? launch : undefined}
          setup={dialogEntry.emulator_id ? setups[dialogEntry.emulator_id] : undefined}
          saves={dialogSaves ?? undefined}
          onAnswer={answerSave}
          onEditControls={() =>
            dialogEntry.emulator_id && openControls(dialogEntry.emulator_id, dialogEntry.rom.system, dialogEntry.rom.id, "home")
          }
          onResetControls={async () => {
            if (dialogEntry.emulator_id) await resetControls(dialogEntry.emulator_id, dialogEntry.rom.system, dialogEntry.rom.id);
            reloadLibrary();
          }}
          onClose={closeDialog}
          setBack={setBack}
        />
      )}
      {screen === "launch" && launch && (
        <LaunchWindow
          entry={entries?.find((e) => e.rom.id === launch.romId) ?? null}
          launch={launch}
          onRetry={() => startPlay(launch.romId)}
          onAnswer={answerSave}
          onOpenRomshelf={() => {
            setBack(null);
            setLaunch(null);
            openLibrary();
          }}
        />
      )}
      {screen === "controls" && controlsError && (
        <main className="content">
          <p className="error">{controlsError}</p>
        </main>
      )}
      {screen === "controls" && controls && controlsFor && (
        <Controls
          key={`${controlsFor.emulatorId}/${controlsFor.system}/${controlsFor.romId ?? ""}`}
          view={controls}
          held={input.held}
          registerKeys={registerKeys}
          setBack={setBack}
          onBack={closeControls}
          onSave={async (scope, layout) => {
            await saveControls(controlsFor.emulatorId, controlsFor.system, scope === "game" ? controlsFor.romId : null, layout);
            loadControls(controlsFor);
          }}
          onPromote={async (layout) => {
            await promoteControls(controlsFor.emulatorId, controlsFor.system, controlsFor.romId!, layout);
            loadControls(controlsFor);
            setToast(`Made this the ${controls.emulator} default`);
          }}
          promoteCount={(layout) => promoteCount(controlsFor.emulatorId, controlsFor.system, controlsFor.romId!, layout)}
        />
      )}
      {screen === "emulators" && (
        <Emulators
          list={emulatorList}
          error={emulatorError}
          setups={setups}
          onSetup={startSetup}
          onEditControls={(emulatorId, system) => openControls(emulatorId, system, null, "emulators")}
          onBack={() => {
            setBack(null);
            setScreen("home");
          }}
        />
      )}
      {screen === "spike" && <GamepadSpike input={input} holdingB={holdingB} />}
      {toast && (
        <div className="toast" role="status">
          {toast}
        </div>
      )}
      <Footer hints={hints} />
    </div>
  );
}
