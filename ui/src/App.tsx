import { useCallback, useEffect, useRef, useState } from "react";
import {
  appInfo,
  connectionStatus,
  library,
  onLibraryChanged,
  onSyncStatus,
  onUpdateReady,
  quit,
  syncNow,
  syncStatus,
  updateReady,
  type AppInfo,
  type SyncStatus,
  type ConnectionStatus,
  type LibraryEntry,
} from "./api";
import { FirstRun, type BackAction } from "./FirstRun";
import { focusables, moveFocus, type Direction } from "./focus";
import { Footer, type Hint } from "./Footer";
import { GamepadSpike } from "./GamepadSpike";
import { Home } from "./Home";
import type { Button } from "./input";
import { nextTab } from "./library";
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
  const [back, setBackState] = useState<BackAction | null>(null);
  const backRef = useRef<BackAction | null>(null);
  const [newVersion, setNewVersion] = useState<string | null>(null);
  const [tab, setTab] = useState("all");
  const [sync, setSync] = useState<SyncStatus | null>(null);

  const reloadLibrary = useCallback(() => {
    library()
      .then(setEntries)
      .catch((e) => setError(`Couldn't read the library: ${String(e)}. Restart Romshelf.`));
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

  const setBack = useCallback((b: BackAction | null) => {
    backRef.current = b;
    setBackState(b);
  }, []);

  const openLibrary = useCallback(() => {
    Promise.all([connectionStatus(), library()])
      .then(([s, l]) => {
        setStatus(s);
        setEntries(l);
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

  // Read the hint from whatever holds focus: after pad moves, and on touch or mouse focus.
  const syncHint = useCallback(() => {
    setFocusHint((document.activeElement as HTMLElement | null)?.dataset?.hint ?? null);
  }, []);
  useEffect(() => {
    document.addEventListener("focusin", syncHint);
    return () => document.removeEventListener("focusin", syncHint);
  }, [syncHint]);

  const onPress = useCallback(
    (b: Button, from: "pad" | "keyboard") => {
      if (b === "up" || b === "down" || b === "left" || b === "right") {
        moveFocus(b as Direction);
        syncHint();
      } else if (b === "a" && screen !== "spike") {
        (document.activeElement as HTMLElement | null)?.click();
      } else if ((b === "l1" || b === "r1") && screen === "home") {
        setTab((t) => nextTab(t, b === "l1" ? -1 : 1));
      } else if (b === "b" && screen === "first-run") {
        backRef.current?.run();
      } else if (b === "b" && screen === "spike" && from === "keyboard") {
        // Escape on a keyboard leaves at once; a pad's B must be held.
        setScreen("home");
      }
    },
    [screen, syncHint],
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

  const hints: Hint[] =
    screen === "spike"
      ? [{ glyph: "B", label: "Hold to go back" }]
      : [
          ...(focusHint ? [{ glyph: "A", label: focusHint }] : []),
          ...(screen === "first-run" && back ? [{ glyph: "B", label: back.label }] : []),
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
          onOpenSpike={() => setScreen("spike")}
          onQuit={() => void quit()}
        />
      )}
      {screen === "spike" && <GamepadSpike input={input} holdingB={holdingB} />}
      <Footer hints={hints} />
    </div>
  );
}
