/** Spatial focus movement: the nearest focusable element in the pressed direction. */

export type Direction = "up" | "down" | "left" | "right";

const FOCUSABLE = "[data-focusable]:not([disabled])";

export function focusables(root: ParentNode = document): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)];
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
    all[0].focus();
    return true;
  }
  const others = all.filter((el) => el !== current);
  const i = pick(
    current.getBoundingClientRect(),
    others.map((el) => el.getBoundingClientRect()),
    dir,
  );
  if (i < 0) return false;
  others[i].focus();
  others[i].scrollIntoView?.({ block: "nearest" });
  return true;
}
