/** The Controls editor's rules: rows, tags, rebinding and what to save. Mirrors `controls/merge.rs`. */

import type { Binding, ControlsView, InputView, Layout, TargetView } from "./api";

const CLEARED: Binding = { kind: "cleared" };

const same = (a: Binding | undefined, b: Binding | undefined): boolean =>
  (a ?? CLEARED).kind === (b ?? CLEARED).kind &&
  (a?.kind === "bound" ? a.action : null) === (b?.kind === "bound" ? b.action : null);

/** The default with a game's differences on top. */
export function merge(base: Layout, diff: Layout): Layout {
  return { ...base, ...diff };
}

/** What `full` changes from `base`: a game's differences. */
export function difference(full: Layout, base: Layout): Layout {
  const out: Layout = {};
  for (const input of new Set([...Object.keys(full), ...Object.keys(base)])) {
    if (!same(full[input], base[input])) out[input] = full[input] ?? CLEARED;
  }
  return out;
}

export interface Row {
  input: InputView;
  target: TargetView | null;
  /** The game changes this row from the default ("This game"). */
  custom: boolean;
}

/** Game buttons, then emulator hotkeys and unbound inputs, in the Deck's input order. */
export function rows(view: ControlsView, working: Layout): { game: Row[]; hotkeys: Row[] } {
  const game: Row[] = [];
  const hotkeys: Row[] = [];
  for (const input of view.inputs) {
    const b = working[input.id];
    const target = b?.kind === "bound" ? (view.targets.find((t) => t.id === b.action) ?? null) : null;
    const row = { input, target, custom: view.game != null && !same(b, view.default[input.id]) };
    (target?.kind === "game" ? game : hotkeys).push(row);
  }
  return { game, hotkeys };
}

export type Rebind =
  | { kind: "done"; layout: Layout }
  /** The new input already does something: ask before swapping. */
  | { kind: "swap"; layout: Layout; other: TargetView }
  | { kind: "invalid"; message: string };

/** Moves `from`'s target to the input just pressed. */
export function rebind(view: ControlsView, working: Layout, from: string, to: string): Rebind {
  const b = working[from];
  const target = b?.kind === "bound" ? view.targets.find((t) => t.id === b.action) : undefined;
  const input = view.inputs.find((i) => i.id === to);
  if (!target || !input) return { kind: "invalid", message: "That input can't be used here." };
  if (target.directional !== input.directional) {
    return {
      kind: "invalid",
      message: target.directional
        ? `${target.label} needs the D-pad or a stick. Press one of those.`
        : `${target.label} needs a button, not ${input.label}.`,
    };
  }
  if (from === to) return { kind: "done", layout: working };
  const existing = working[to];
  const other =
    existing?.kind === "bound" ? view.targets.find((t) => t.id === existing.action) : undefined;
  if (other && other.directional === view.inputs.find((i) => i.id === from)?.directional) {
    return { kind: "swap", layout: { ...working, [to]: b!, [from]: existing! }, other };
  }
  return { kind: "done", layout: { ...working, [to]: b!, [from]: CLEARED } };
}

/** Binds an unbound input to a target chosen from a list. */
export function assign(working: Layout, input: string, target: string): Layout {
  return { ...working, [input]: { kind: "bound", action: target } };
}

export function clear(working: Layout, input: string): Layout {
  return { ...working, [input]: CLEARED };
}

export function backToDefault(working: Layout, base: Layout, input: string): Layout {
  return { ...working, [input]: base[input] ?? CLEARED };
}

/** What the press of a pad button or key means as a Deck input, while listening. */
export function inputFor(button: string): string | null {
  switch (button) {
    case "up":
    case "down":
    case "left":
    case "right":
      return "dpad";
    case "lstick":
      return "left_stick";
    case "rstick":
      return "right_stick";
    case "guide":
      return null;
    default:
      return button;
  }
}
