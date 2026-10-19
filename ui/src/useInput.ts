import { useEffect, useRef, useState } from "react";
import { onGilrs, type GilrsSnapshot } from "./api";
import { browserPressed, gilrsPressed, KEY_TO_BUTTON, PressTracker, type Button } from "./input";

export interface BrowserPad {
  index: number;
  id: string;
  mapping: string;
  pressed: Button[];
  axes: number[];
}

export type Source = "browser" | "gilrs";

export interface InputState {
  browser: BrowserPad[];
  gilrs: GilrsSnapshot | null;
  /** Which pad source drives navigation: the first one that reported a press. */
  source: Source | null;
  /** Buttons currently held on the navigation source. */
  held: Button[];
}

/**
 * Reads both gamepad paths and the keyboard, and calls `onPress` once per press.
 * Navigation locks to whichever pad source reports a press first, so the two paths never double up.
 */
export function useInput(onPress: (b: Button, from: "pad" | "keyboard") => void): InputState {
  const handler = useRef(onPress);
  useEffect(() => {
    handler.current = onPress;
  });

  const [state, setState] = useState<InputState>({ browser: [], gilrs: null, source: null, held: [] });

  useEffect(() => {
    const trackers: Record<Source, PressTracker> = {
      browser: new PressTracker(),
      gilrs: new PressTracker(),
    };
    let source: Source | null = null;
    let gilrsHeld = new Set<Button>();
    let gilrsSnap: GilrsSnapshot | null = null;
    let lastJson = "";
    let frame = 0;

    const feed = (from: Source, held: Set<Button>, now: number) => {
      const presses = trackers[from].update(now, held);
      if (presses.length && source === null) source = from;
      if (source === from) presses.forEach((b) => handler.current(b, "pad"));
    };

    const tick = (now: number) => {
      const pads = (navigator.getGamepads?.() ?? []).filter((p): p is Gamepad => !!p);
      const browser: BrowserPad[] = pads.map((p) => ({
        index: p.index,
        id: p.id,
        mapping: p.mapping,
        pressed: [...browserPressed(p)],
        axes: p.axes.map((a) => Math.round(a * 100) / 100),
      }));
      const browserHeld = new Set<Button>(browser.flatMap((p) => p.pressed));
      feed("browser", browserHeld, now);
      // Re-feed gilrs every frame so held directions auto-repeat.
      feed("gilrs", gilrsHeld, now);

      const held = source === "gilrs" ? [...gilrsHeld] : source === "browser" ? [...browserHeld] : [];
      const next: InputState = { browser, gilrs: gilrsSnap, source, held };
      const json = JSON.stringify(next);
      if (json !== lastJson) {
        lastJson = json;
        setState(next);
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);

    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onGilrs((snap) => {
      gilrsSnap = snap;
      gilrsHeld = new Set(snap.pads.flatMap((p) => [...gilrsPressed(p.pressed, p.axes)]));
    }).then((u) => (cancelled ? u() : (unlisten = u)));

    const onKey = (e: KeyboardEvent) => {
      const b = KEY_TO_BUTTON[e.key];
      if (!b) return;
      e.preventDefault();
      handler.current(b, "keyboard");
    };
    window.addEventListener("keydown", onKey);

    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
      unlisten?.();
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  return state;
}
