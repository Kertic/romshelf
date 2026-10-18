import { useCallback, useEffect, useState } from "react";
import { appInfo, library, quit, type AppInfo, type LibraryEntry } from "./api";
import { focusables, moveFocus, type Direction } from "./focus";
import { Footer, type Hint } from "./Footer";
import { GamepadSpike } from "./GamepadSpike";
import { Home } from "./Home";
import type { Button } from "./input";
import { useInput } from "./useInput";

type Screen = "home" | "spike";

const HOLD_TO_LEAVE_MS = 1000;

export function App() {
  const [screen, setScreen] = useState<Screen>("home");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [entries, setEntries] = useState<LibraryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [focusHint, setFocusHint] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([appInfo(), library()])
      .then(([i, l]) => {
        setInfo(i);
        setEntries(l);
      })
      .catch((e) => setError(`Couldn't read the library: ${String(e)}. Restart Romshelf.`));
  }, []);

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
      } else if (b === "a" && screen === "home") {
        (document.activeElement as HTMLElement | null)?.click();
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
      const frame = requestAnimationFrame(() => {
        focusables()[0]?.focus();
        syncHint();
      });
      return () => cancelAnimationFrame(frame);
    }
  }, [screen, entries, syncHint]);

  const hints: Hint[] =
    screen === "spike"
      ? [{ glyph: "B", label: "Hold to go back" }]
      : [
          ...(focusHint ? [{ glyph: "A", label: focusHint }] : []),
          { glyph: "✥", label: "Move" },
        ];

  return (
    <div className="frame">
      {screen === "home" ? (
        <Home
          info={info}
          entries={entries}
          error={error}
          onOpenSpike={() => setScreen("spike")}
          onQuit={() => void quit()}
        />
      ) : (
        <GamepadSpike input={input} holdingB={holdingB} />
      )}
      <Footer hints={hints} />
    </div>
  );
}
