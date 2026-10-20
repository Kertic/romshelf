import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  answerSaveQuestion,
  controlsView,
  changeStorage,
  checkForUpdates,
  diagnostics,
  settingsView,
  setSaveVersions,
  signOut,
  transfers as listTransfers,
  imageFiles,
  onSteamChanged,
  saveShortcut,
  setShortcutArtwork,
  setSteamAutoAdd,
  steamRestartNotice,
  steamShortcut,
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
  type ShortcutView,
  type SettingsView,
  type TransferRow,
  type Progress,
  type StorageStatus,
  type SyncStatus,
} from "./api";
import { FirstRun, type BackAction } from "./FirstRun";
import { focusables, focusLost, moveFocus, restoreFocus, type Direction } from "./focus";
import { Footer, type Hint } from "./Footer";
import { GamepadSpike } from "./GamepadSpike";
import { Emulators, type SetupState } from "./Emulators";
import { LaunchWindow, type LaunchState } from "./LaunchWindow";
import { Controls, type KeyHandler } from "./Controls";
import { RestartSteamDialog, SteamShortcut } from "./SteamShortcut";
import { Settings } from "./Settings";
import { Transfers } from "./Transfers";
import { Home } from "./Home";
import type { Button } from "./input";
import { nextTab } from "./library";
import { RomPage, type SaveActions, type View } from "./RomActions";
import { useInput } from "./useInput";

type Screen = "loading" | "first-run" | "home" | "emulators" | "launch" | "controls" | "shortcut" | "settings" | "spike";

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
  /** A dialog to open the ROM page with (from the drawer). */
  const [dialogStart, setDialogStart] = useState<View | undefined>(undefined);
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
  /** The Steam shortcut screen's game and view. */
  const [shortcut, setShortcut] = useState<ShortcutView | null>(null);
  /** Shortcut changes waiting for Steam to restart: the prompt. */
  const [restart, setRestart] = useState<number | null>(null);
  const [controlsError, setControlsError] = useState<string | null>(null);
  /** The editor sees presses first (rebinding, X, Y, L1, R1). */
  const keyHandler = useRef<KeyHandler | null>(null);
  /** Menu, View and Y on the Library and ROM page, set once their handlers exist. */
  const shortcuts = useRef<Partial<Record<"menu" | "view" | "y", () => void>>>({});
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [transferRows, setTransferRows] = useState<TransferRow[] | null>(null);
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

  // New games added to Steam after a sync: the restart prompt.
  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | null = null;
    onSteamChanged((n) => {
      setRestart(n);
      void steamRestartNotice();
    }).then((un) => (live ? (unlisten = un) : un()));
    return () => {
      live = false;
      unlisten?.();
    };
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
      // Focus went away (its element was removed, or a dialog closed): the first press finds it.
      if (focusLost() && b !== "b" && b !== "menu" && b !== "view") {
        restoreFocus(document.querySelector("[data-dialog]") ?? document);
        syncHint();
        if (b === "a") return;
      }
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
      } else if ((b === "menu" || b === "view" || b === "y") && screen === "home" && !document.querySelector("[data-dialog]")) {
        // Menu: Settings. View: Transfers. Y: back up the save (UI guide section 4).
        shortcuts.current[b]?.();
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
      // Focus the player already put on a tile or the drawer (a press as the Library appeared)
      // stays where it is.
      const chosen = focusedTab.current === null && active?.closest(".grid, .drawer");
      if (kept && (focusedTab.current === tab || chosen)) {
        focusedTab.current = tab;
        return;
      }
      focusedTab.current = tab;
      (document.querySelector<HTMLElement>(".tile[data-focusable]") ?? focusables()[0])?.focus();
      syncHint();
    });
    return () => clearTimeout(timer);
  }, [screen, entries, tab, syncHint]);
  /** Shortcut changes saved on the shortcut screen, for the restart prompt. */
  const shortcutChanges = useRef(0);
  const openShortcut = useCallback((romId: string) => {
    shortcutChanges.current = 0;
    setDialogRom(null);
    setShortcut(null);
    setScreen("shortcut");
    steamShortcut(romId)
      .then(setShortcut)
      .catch((e) => setError(e?.message ?? `Couldn't read the shortcut: ${String(e)}. Try again.`));
  }, []);
  const closeShortcut = useCallback(() => {
    const romId = shortcut?.rom_id;
    setShortcut(null);
    setBack(null);
    setScreen("home");
    reloadLibrary();
    if (romId) setDialogRom(romId);
    // The prompt, once for the batch of changes just made.
    steamRestartNotice()
      .then((pending) => pending && setRestart(Math.max(1, shortcutChanges.current)))
      .catch(() => {});
  }, [shortcut, setBack, reloadLibrary]);

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
  const reloadSettings = useCallback(() => {
    settingsView()
      .then(setSettings)
      .catch(() => {});
  }, []);
  const openSettings = useCallback(() => {
    settingsView()
      .then((s) => {
        setSettings(s);
        setDialogRom(null);
        setScreen("settings");
      })
      .catch((e) => setToast(e?.message ?? `Couldn't open Settings: ${String(e)}. Try again.`));
  }, []);
  const closeSettings = useCallback(() => {
    setBack(null);
    setScreen("home");
    reloadLibrary();
  }, [setBack, reloadLibrary]);
  /** B before the Transfers panel opened, put back when it closes. */
  const backBeforePanel = useRef<BackAction | null>(null);
  const closeTransfers = useCallback(() => {
    setTransferRows(null);
    setBack(backBeforePanel.current);
  }, [setBack]);
  const openTransfers = useCallback(() => {
    backBeforePanel.current = backRef.current;
    setBack({ label: "Close", run: closeTransfers });
    listTransfers()
      .then(setTransferRows)
      .catch(() => setTransferRows([]));
  }, [setBack, closeTransfers]);
  // While the panel is open, keep it current.
  const panelOpen = transferRows != null;
  useEffect(() => {
    if (!panelOpen) return;
    const t = setInterval(() => {
      listTransfers()
        .then(setTransferRows)
        .catch(() => {});
    }, 2000);
    return () => clearInterval(t);
  }, [panelOpen]);
  /** Y: back up the save of the game on screen, or the focused tile's. */
  const backupFocused = useCallback(() => {
    const romId = dialogRom ?? (document.activeElement as HTMLElement | null)?.dataset?.romId ?? null;
    const entry = entries?.find((e) => e.rom.id === romId);
    if (!romId || !entry?.emulator_ready) return;
    const name = entry.rom.display_name;
    setToast(`Backing up ${name}…`);
    backupSave(romId)
      .then((out) =>
        setToast(
          out.kind === "uploaded"
            ? `Backed up ${name}`
            : out.kind === "unchanged"
              ? `Drive already has ${name}'s save`
              : `${name} has no save on this Deck yet`,
        ),
      )
      .catch((e) => setToast(e?.message ?? `Couldn't back up ${name}. Try again.`))
      .finally(reloadLibrary);
  }, [dialogRom, entries, reloadLibrary]);
  useEffect(() => {
    shortcuts.current = { menu: openSettings, view: openTransfers, y: backupFocused };
  }, [openSettings, openTransfers, backupFocused]);
  const hints: Hint[] =
    screen === "spike"
      ? [{ glyph: "B", label: "Hold to go back" }]
      : [
          ...(focusHint ? [{ glyph: "A", label: focusHint }] : []),
          ...(focusHintX && !dialogRom ? [{ glyph: "X", label: focusHintX }] : []),
          ...(screen === "home" && !transferRows ? [{ glyph: "Y", label: "Back up save" }] : []),
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
      {screen === "home" && !dialogEntry && (
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
          onOpen={(id, view) => {
            setDialogStart(view);
            setDialogRom(id);
          }}
          onPlay={startPlay}
          onDownload={startDownload}
          onSetup={(id) => startSetup(id)}
          onOpenSpike={() => setScreen("spike")}
          onOpenEmulators={openEmulators}
          onOpenSettings={openSettings}
          onOpenTransfers={openTransfers}
          onQuit={() => void quit()}
        />
      )}
      {screen === "home" && dialogEntry && (
        <RomPage
          key={dialogEntry.rom.id}
          startWith={dialogStart}
          offline={sync?.state === "offline"}
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
          onSteamShortcut={() => openShortcut(dialogEntry.rom.id)}
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
      {screen === "shortcut" && shortcut && (
        <SteamShortcut
          key={shortcut.rom_id}
          view={shortcut}
          setBack={setBack}
          onBack={closeShortcut}
          onSave={async (shown, name) => {
            const done = await saveShortcut(shortcut.rom_id, shown, name);
            shortcutChanges.current += done.added + done.updated + done.removed;
            setShortcut(await steamShortcut(shortcut.rom_id));
            return done;
          }}
          onAutoAdd={async (on) => {
            const done = await setSteamAutoAdd(on);
            shortcutChanges.current += done.added + done.updated + done.removed;
            setShortcut(await steamShortcut(shortcut.rom_id));
          }}
          onArtwork={async (slot, path) => {
            setShortcut(await setShortcutArtwork(shortcut.rom_id, slot, path));
            const done = await saveShortcut(shortcut.rom_id, true, shortcut.name);
            shortcutChanges.current += Math.max(1, done.added + done.updated);
          }}
          loadImages={imageFiles}
        />
      )}
      {screen === "settings" && settings && (
        <Settings
          view={settings}
          setBack={setBack}
          onBack={closeSettings}
          reload={reloadSettings}
          actions={{
            signOut: async () => {
              await signOut();
              const s = await connectionStatus();
              setStatus(s);
              setBack(null);
              setScreen("first-run");
            },
            changeDriveFolder: () => {
              setBack(null);
              setScreen("first-run");
            },
            changeStorage,
            setSaveVersions,
            setAutoAdd: async (on) => {
              await setSteamAutoAdd(on);
            },
            checkForUpdates,
            diagnostics,
            copy: async (text) => {
              await navigator.clipboard.writeText(text);
            },
            openGamepadTest: () => setScreen("spike"),
            quit: () => void quit(),
          }}
        />
      )}
      {transferRows && (
        <Transfers
          rows={transferRows}
          progress={progress}
          onCancel={(id) => void cancelDownload(id).then(() => listTransfers().then(setTransferRows))}
          onClose={closeTransfers}
        />
      )}
      {restart != null && <RestartSteamDialog changes={restart} onClose={() => setRestart(null)} />}
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
