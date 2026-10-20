/** Spatial focus movement: the nearest focusable element in the pressed direction. */

export type Direction = "up" | "down" | "left" | "right";

const FOCUSABLE = "[data-focusable]:not([disabled])";

/** Focusable elements that can be reached: not behind a dialog (inert or aria-hidden) or hidden. */
export function focusables(root: ParentNode = document): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(reachable);
}

/** Not inside something inert, aria-hidden or hidden. */
export function reachable(el: HTMLElement): boolean {
  return !el.closest("[inert], [aria-hidden='true'], [hidden]");
}

/** The region an element belongs to (`data-region`): top band, grid, drawer. */
function region(el: Element): Element | null {
  return el.closest("[data-region]");
}

function centre(r: DOMRect) {
  return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
}

/** Picks the best candidate; exported for tests. */
export function pick(from: DOMRect, candidates: DOMRect[], dir: Direction): number {
  const c = centre(from);
  let best = -1;
  let bestScore = Infinity;
  candidates.forEach((r, i) => {
    const p = centre(r);
    const dx = p.x - c.x;
    const dy = p.y - c.y;
    const along = dir === "left" ? -dx : dir === "right" ? dx : dir === "up" ? -dy : dy;
    const across = dir === "left" || dir === "right" ? Math.abs(dy) : Math.abs(dx);
    if (along <= 1) return;
    const score = along + across * 2;
    if (score < bestScore) {
      bestScore = score;
      best = i;
    }
  });
  return best;
}

export function moveFocus(dir: Direction, root: ParentNode = document): boolean {
  const all = focusables(root);
  if (all.length === 0) return false;
  const current = document.activeElement as HTMLElement | null;
  if (!current || !all.includes(current)) {
    restoreFocus(root);
    return true;
  }
  const others = all.filter((el) => el !== current);
  // The nearest in the pressed direction within the current region, then across regions.
  const home = region(current);
  const near = home ? others.filter((el) => region(el) === home) : [];
  for (const pool of [near, others]) {
    const i = pick(
      current.getBoundingClientRect(),
      pool.map((el) => el.getBoundingClientRect()),
      dir,
    );
    if (i >= 0) {
      pool[i]!.focus();
      pool[i]!.scrollIntoView?.({ block: "nearest" });
      return true;
    }
  }
  return false;
}

/**
 * Puts focus somewhere sensible when it has been lost (the focused element went away with a
 * list change, or a dialog closed): the screen's main action, else the first focusable.
 */
export function restoreFocus(root: ParentNode = document): boolean {
  const all = focusables(root);
  const target = all.find((el) => el.hasAttribute("data-autofocus")) ?? all[0];
  if (!target) return false;
  target.focus();
  return true;
}

/** True when focus is nowhere useful: on the page itself, or on something no longer reachable. */
export function focusLost(): boolean {
  const el = document.activeElement as HTMLElement | null;
  return !el || el === document.body || !el.isConnected || !reachable(el);
}
