import { useCallback, useEffect, useRef, useState } from "react";
import {
  appInfo,
  connectionStatus,
  library,
  onUpdateReady,
  quit,
  updateReady,
  type AppInfo,
  type ConnectionStatus,
  type LibraryEntry,
} from "./api";
import { FirstRun, type BackAction } from "./FirstRun";
import { focusables, moveFocus, type Direction } from "./focus";
import { Footer, type Hint } from "./Footer";
import { GamepadSpike } from "./GamepadSpike";
import { Home } from "./Home";
import type { Button } from "./input";
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
      })
      .catch((e) => {
        setError(`Couldn't read the library: ${String(e)}. Restart Romshelf.`);
        setScreen("home");
      });
  }, []);

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

  useEffect(() => {
    if (screen === "home") {
      // Initial focus: the first tile, or the first action when the library is empty.
      const timer = setTimeout(() => {
        focusables()[0]?.focus();
        syncHint();
      });
      return () => clearTimeout(timer);
    }
  }, [screen, entries, syncHint]);

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
          onOpenSpike={() => setScreen("spike")}
          onQuit={() => void quit()}
        />
      )}
      {screen === "spike" && <GamepadSpike input={input} holdingB={holdingB} />}
      <Footer hints={hints} />
    </div>
  );
}
