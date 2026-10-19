/** Gamepad input: the browser Gamepad API and the Rust `gilrs` path, reduced to one button set. */

export type Button =
  | "a"
  | "b"
  | "x"
  | "y"
  | "l1"
  | "r1"
  | "l2"
  | "r2"
  | "view"
  | "menu"
  | "l3"
  | "r3"
  | "up"
  | "down"
  | "left"
  | "right"
  | "guide";

/** Button order of the W3C "standard" gamepad mapping. */
export const STANDARD_ORDER: Button[] = [
  "a", "b", "x", "y", "l1", "r1", "l2", "r2", "view", "menu", "l3", "r3",
  "up", "down", "left", "right", "guide",
];

export const BUTTON_LABEL: Record<Button, string> = {
  a: "A", b: "B", x: "X", y: "Y", l1: "L1", r1: "R1", l2: "L2", r2: "R2",
  view: "View", menu: "Menu", l3: "L3", r3: "R3",
  up: "Up", down: "Down", left: "Left", right: "Right", guide: "Steam",
};

const STICK_THRESHOLD = 0.5;

/** Buttons held on a browser pad. The left stick also counts as the D-pad. */
export function browserPressed(gp: Pick<Gamepad, "buttons" | "axes">): Set<Button> {
  const out = new Set<Button>();
  gp.buttons.forEach((b, i) => {
    const name = STANDARD_ORDER[i];
    if (name && (b.pressed || b.value > 0.5)) out.add(name);
  });
  addStick(out, gp.axes[0] ?? 0, gp.axes[1] ?? 0);
  return out;
}

/** Buttons held according to a `gilrs` snapshot of one pad. */
export function gilrsPressed(pressed: string[], axes: Record<string, number>): Set<Button> {
  const out = new Set<Button>();
  for (const p of pressed) if ((STANDARD_ORDER as string[]).includes(p)) out.add(p as Button);
  // gilrs reports stick Y positive when pushed up.
  addStick(out, axes["left_x"] ?? 0, -(axes["left_y"] ?? 0));
  return out;
}

function addStick(out: Set<Button>, x: number, y: number) {
  if (x < -STICK_THRESHOLD) out.add("left");
  if (x > STICK_THRESHOLD) out.add("right");
  if (y < -STICK_THRESHOLD) out.add("up");
  if (y > STICK_THRESHOLD) out.add("down");
}

export function newlyPressed(prev: Set<Button>, next: Set<Button>): Button[] {
  return [...next].filter((b) => !prev.has(b));
}

const DIRECTIONS: Button[] = ["up", "down", "left", "right"];
const REPEAT_DELAY = 400;
const REPEAT_RATE = 120;

/**
 * Turns successive held-button sets into press events, with auto-repeat for directions.
 * Fed by one source at a time so a press is never counted twice.
 */
export class PressTracker {
  private held = new Set<Button>();
  private nextRepeat = new Map<Button, number>();

  update(now: number, next: Set<Button>): Button[] {
    const presses = newlyPressed(this.held, next);
    for (const b of presses) if (DIRECTIONS.includes(b)) this.nextRepeat.set(b, now + REPEAT_DELAY);
    for (const [b, at] of this.nextRepeat) {
      if (!next.has(b)) this.nextRepeat.delete(b);
      else if (now >= at && !presses.includes(b)) {
        presses.push(b);
        this.nextRepeat.set(b, now + REPEAT_RATE);
      }
    }
    this.held = new Set(next);
    return presses;
  }

  isHeld(b: Button): boolean {
    return this.held.has(b);
  }
}

export const KEY_TO_BUTTON: Record<string, Button> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Enter: "a",
  " ": "a",
  Escape: "b",
  Backspace: "b",
  PageUp: "l1",
  PageDown: "r1",
};
