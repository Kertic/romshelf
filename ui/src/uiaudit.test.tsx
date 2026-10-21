/** The self-test's check of a screen as shown: reach, focus, wrapping, overflow, targets, hints. */
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { audit } from "./uiaudit";

afterEach(cleanup);

/** Gives each element with `data-box="x y w h"` that box. */
function layOut() {
  for (const el of document.querySelectorAll<HTMLElement>("[data-box]")) {
    const [x, y, w, h] = el.dataset.box!.split(" ").map(Number) as [number, number, number, number];
    el.getBoundingClientRect = () =>
      ({ x, y, left: x, top: y, width: w, height: h, right: x + w, bottom: y + h, toJSON() {} }) as DOMRect;
  }
}

const opts = { focusVisible: () => true, lines: () => 1, width: 1280, windowFocused: true };

function Grid() {
  return (
    <div>
      <header className="top-band">
        <button data-focusable data-hint="Open" data-box="0 0 120 44">
          Emulators
        </button>
        <button data-focusable data-hint="Open" data-box="140 0 120 44" className="sync-status">
          Synced just now
        </button>
      </header>
      <button data-focusable data-autofocus data-hint="Open" data-box="0 100 200 200">
        Tidewalker
      </button>
      <button data-focusable data-hint="Open" data-box="220 100 200 200">
        Ember Circuit
      </button>
    </div>
  );
}

describe("audit", () => {
  it("a screen where everything is reachable, visible and fits passes", () => {
    render(<Grid />);
    layOut();
    (document.querySelector("[data-autofocus]") as HTMLElement).focus();
    const c = audit("Library", opts);
    expect(c.outcome).toBe("pass");
    expect(c.detail).toContain("4 controls");
    expect(c.group).toBe("Interface");
  });

  it("a control the D-pad can't reach fails, by name", () => {
    render(
      <div>
        <button data-focusable data-autofocus data-hint="Open" data-box="0 0 100 100">
          Play
        </button>
        {/* Exactly behind Play: no press ever lands on it. */}
        <button data-focusable data-hint="Open" data-box="0 0 100 100">
          Hidden twin
        </button>
      </div>,
    );
    layOut();
    (document.querySelector("[data-autofocus]") as HTMLElement).focus();
    const c = audit("ROM page", opts);
    expect(c.outcome).toBe("fail");
    expect(c.detail).toContain("can't reach: Hidden twin");
  });

  it("text that wraps in the top band is named", () => {
    render(<Grid />);
    layOut();
    (document.querySelector("[data-autofocus]") as HTMLElement).focus();
    const c = audit("Library", {
      ...opts,
      lines: (el: HTMLElement) => (el.textContent === "Synced just now" ? 3 : 1),
    });
    expect(c.outcome).toBe("warn");
    expect(c.detail).toContain("\"Synced just now\" wraps onto 3 lines");
  });

  it("a page wider than the screen fails", () => {
    render(<Grid />);
    layOut();
    (document.querySelector("[data-autofocus]") as HTMLElement).focus();
    const c = audit("Library", { ...opts, width: 1280, pageWidth: 1400 });
    expect(c.outcome).toBe("fail");
    expect(c.detail).toContain("1400 px wide");
  });

  it("small targets and missing hints are listed", () => {
    render(
      <div>
        <button data-focusable data-autofocus data-hint="Open" data-box="0 0 200 60">
          Big
        </button>
        <button data-focusable data-box="0 100 30 30">
          Tiny
        </button>
      </div>,
    );
    layOut();
    (document.querySelector("[data-autofocus]") as HTMLElement).focus();
    const c = audit("Settings", opts);
    expect(c.outcome).toBe("warn");
    expect(c.detail).toContain("smaller than 44 px: Tiny");
    expect(c.detail).toContain("no button hint: Tiny");
  });

  it("nothing focused, or focus that can't be seen, fails", () => {
    render(<Grid />);
    layOut();
    (document.activeElement as HTMLElement | null)?.blur();
    expect(audit("Library", opts).detail).toContain("Nothing has focus");
    (document.querySelector("[data-autofocus]") as HTMLElement).focus();
    const c = audit("Library", { ...opts, focusVisible: () => false });
    expect(c.outcome).toBe("fail");
    expect(c.detail).toContain("focus on \"Tidewalker\" isn't visible");
    // A window without focus draws no ring: that's noted, not failed.
    const unfocused = audit("Library", { ...opts, focusVisible: () => false, windowFocused: false });
    expect(unfocused.outcome).toBe("pass");
    expect(unfocused.detail).toContain("the window wasn't focused");
  });
});
