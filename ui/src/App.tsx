import { useCallback, useEffect, useRef, useState } from "react";
import {
  appInfo,
  cancelDownload,
  connectionStatus,
  download,
  library,
  onLibraryChanged,
  onSyncStatus,
  onTransfer,
  onUpdateReady,
  quit,
  removeFromDeck,
  storageStatus,
  syncNow,
  syncStatus,
  updateReady,
  type AppInfo,
  type ConnectionStatus,
  type LibraryEntry,
  type Progress,
  type StorageStatus,
  type SyncStatus,
} from "./api";
import { FirstRun, type BackAction } from "./FirstRun";
import { focusables, moveFocus, type Direction } from "./focus";
import { Footer, type Hint } from "./Footer";
import { GamepadSpike } from "./GamepadSpike";
import { Home } from "./Home";
import type { Button } from "./input";
import { nextTab } from "./library";
import { RomDialog } from "./RomActions";
import { useInput } from "./useInput";

type Screen = "loading" | "first-run" | "home" | "spike";

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
    Promise.all([appInfo(), connectionStatus()])
      .then(([i, s]) => {
        setInfo(i);
        setStatus(s);
        if (s.ready) openLibrary();
        else setScreen("first-run");
      })
      .catch((e) => {
        setError(`Couldn't read Romshelf's settings: ${String(e)}. Restart Romshelf.`);
        setScreen("home");
      });
  }, [openLibrary]);

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
  const holdingB = screen === "spike" && input.held.includes("b");
  useEffect(() => {
    if (!holdingB) return;
    const t = setTimeout(() => setScreen("home"), HOLD_TO_LEAVE_MS);
    return () => clearTimeout(t);
  }, [holdingB]);
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
  const dialogEntry = dialogRom ? (entries?.find((e) => e.rom.id === dialogRom) ?? null) : null;
  const hints: Hint[] =
    screen === "spike"
      ? [{ glyph: "B", label: "Hold to go back" }]
      : [
          ...(focusHint ? [{ glyph: "A", label: focusHint }] : []),
          ...(focusHintX && !dialogRom ? [{ glyph: "X", label: focusHintX }] : []),
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
          onClose={closeDialog}
          setBack={setBack}
        />
      )}
      {screen === "spike" && <GamepadSpike input={input} holdingB={holdingB} />}
      <Footer hints={hints} />
    </div>
  );
}
