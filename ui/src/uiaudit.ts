/**
 * The self-test's look at a screen as it's shown on the Deck: can the D-pad reach every control,
 * is focus visible, does anything wrap or run off the screen, are targets big enough, does every
 * control have a button hint, and how small is the smallest text.
 */
import { focusables, nextFocus, type Direction } from "./focus";

export type Outcome = "pass" | "warn" | "fail" | "skip";

/** Same shape as the core's checks, so one report holds both. */
export interface Check {
  group: string;
  name: string;
  outcome: Outcome;
  detail: string;
  fix: string | null;
}

export interface AuditOptions {
  /** Whether focus on `el` is drawn (an outline or a ring). */
  focusVisible?: (el: HTMLElement) => boolean;
  /** How many lines `el`'s text takes. */
  lines?: (el: HTMLElement) => number;
  /** Whether the window has focus: without it no focus ring is drawn, so it can't be judged. */
  windowFocused?: boolean;
  /** The screen's width, and the page's (wider means it scrolls sideways). */
  width?: number;
  pageWidth?: number;
}

const DIRECTIONS: Direction[] = ["up", "down", "left", "right"];
/** The UI guide's smallest target. */
const MIN_TARGET = 44;
/** Single-line places: the top band and the footer's hints. */
const ONE_LINE = ".top-band > *, .footer > *";

function label(el: HTMLElement): string {
  return (el.getAttribute("aria-label") ?? el.textContent ?? "").trim().replace(/\s+/g, " ").slice(0, 40) || el.tagName;
}

/** An outline or ring on the element, or on a part of it (a tile draws its ring on its cover). */
function drawnFocus(el: HTMLElement): boolean {
  return [el, ...el.querySelectorAll<HTMLElement>("*")].some((part) => {
    const s = getComputedStyle(part);
    const outline = s.outlineStyle !== "none" && s.outlineStyle !== "" && parseFloat(s.outlineWidth) > 0;
    return outline || (s.boxShadow !== "none" && s.boxShadow !== "");
  });
}

function lineCount(el: HTMLElement): number {
  const range = document.createRange();
  range.selectNodeContents(el);
  if (typeof range.getClientRects !== "function") return 1;
  // Boxes that overlap vertically sit on one line (a key cap beside its label, say).
  const rects = [...range.getClientRects()].filter((r) => r.width > 0 && r.height > 0).sort((a, b) => a.top - b.top);
  let lines = 0;
  let bottom = -Infinity;
  for (const r of rects) {
    if (r.top >= bottom - 2) {
      lines++;
      bottom = r.bottom;
    } else bottom = Math.max(bottom, r.bottom);
  }
  return Math.max(lines, 1);
}

/** One check for the screen on show now, named `screen`. */
export function audit(screen: string, opts: AuditOptions = {}): Check {
  const focusVisible = opts.focusVisible ?? drawnFocus;
  const lines = opts.lines ?? lineCount;
  const width = opts.width ?? window.innerWidth;
  const pageWidth = opts.pageWidth ?? document.documentElement.scrollWidth;
  const windowFocused = opts.windowFocused ?? document.hasFocus();
  const fails: string[] = [];
  const warns: string[] = [];
  const notes: string[] = [];

  const root = document.querySelector("[data-dialog]") ?? document;
  const all = focusables(root);
  const active = document.activeElement as HTMLElement | null;
  const start = active && all.includes(active) ? active : null;
  if (!start) {
    fails.push("Nothing has focus, so the first press only finds it");
  } else if (!windowFocused) {
    notes.push("the window wasn't focused, so the focus ring wasn't judged");
  } else if (!focusVisible(start)) {
    fails.push(`focus on "${label(start)}" isn't visible`);
  }

  // Every control the D-pad can reach from where focus starts.
  const from = start ?? all.find((el) => el.hasAttribute("data-autofocus")) ?? all[0];
  if (from) {
    const seen = new Set<HTMLElement>([from]);
    const queue = [from];
    while (queue.length) {
      const el = queue.shift()!;
      for (const dir of DIRECTIONS) {
        const next = nextFocus(el, dir, all);
        if (next && !seen.has(next)) {
          seen.add(next);
          queue.push(next);
        }
      }
    }
    const lost = all.filter((el) => !seen.has(el)).map(label);
    if (lost.length) fails.push(`the D-pad can't reach: ${lost.join(", ")}`);
  }

  if (pageWidth > width + 1) fails.push(`the page is ${pageWidth} px wide on a ${width} px screen, so it scrolls sideways`);

  for (const el of document.querySelectorAll<HTMLElement>(ONE_LINE)) {
    if (!el.textContent?.trim()) continue;
    const n = lines(el);
    if (n > 1) warns.push(`"${label(el)}" wraps onto ${n} lines`);
  }

  const small = all.filter((el) => {
    // A checkbox's target is its whole label.
    const r = (el.closest("label") ?? el).getBoundingClientRect();
    return r.width > 0 && (r.width < MIN_TARGET || r.height < MIN_TARGET);
  });
  if (small.length) warns.push(`smaller than ${MIN_TARGET} px: ${small.map(label).join(", ")}`);
  const unhinted = all.filter((el) => !el.dataset.hint);
  if (unhinted.length) warns.push(`no button hint: ${unhinted.map(label).join(", ")}`);

  // The smallest text on screen, for "readable at arm's length".
  let smallest = Infinity;
  for (const el of document.querySelectorAll<HTMLElement>("body *")) {
    if (![...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent?.trim())) continue;
    const size = parseFloat(getComputedStyle(el).fontSize);
    if (size > 0) smallest = Math.min(smallest, size);
  }
  if (smallest < 12) warns.push(`the smallest text is ${smallest} px`);

  const outcome: Outcome = fails.length ? "fail" : warns.length ? "warn" : "pass";
  const textNote = Number.isFinite(smallest) ? `; smallest text ${smallest} px` : "";
  return {
    group: "Interface",
    name: screen,
    outcome,
    detail:
      outcome === "pass"
        ? `${all.length} controls, all reachable with the D-pad; ${notes.length ? notes.join("; ") : "focus visible"}; nothing wraps or runs off the screen${textNote}.`
        : [...fails, ...warns, ...notes].join("; ") + ".",
    fix: outcome === "pass" ? null : "Send this report: it names what to fix.",
  };
}
