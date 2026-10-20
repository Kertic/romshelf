import { useEffect, useRef } from "react";
import type { LibraryEntry, SaveChoice, SaveConflict } from "./api";
import { SaveConflictDialog } from "./Saves";

/** What the interface knows about a game being prepared or played. */
export interface LaunchState {
  romId: string;
  /** "Downloading", "Setting up Dolphin", "Playing in Dolphin", … */
  label: string | null;
  percent: number | null;
  running: boolean;
  /** What went wrong, for the error view. */
  error: string | null;
  /** A save conflict the launch is waiting on an answer to. */
  conflict?: SaveConflict | null;
}

function initials(name: string): string {
  return name
    .split(/[\s&,]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
}

interface Props {
  entry: LibraryEntry | null;
  launch: LaunchState;
  onRetry: () => void;
  onOpenRomshelf: () => void;
  /** Answers the save conflict the launch is waiting on. */
  onAnswer?: (choice: SaveChoice) => void;
}

/**
 * The launch window (UI guide section 8): shown only when `romshelf launch` has to download or
 * set something up first, ask about a save conflict, or something went wrong. Small cover, title,
 * then progress, the question or the error.
 */
export function LaunchWindow({ entry, launch, onRetry, onOpenRomshelf, onAnswer }: Props) {
  const title = entry?.rom.display_name ?? "Starting your game";
  const column = useRef<HTMLDivElement>(null);
  // An error puts focus on Try again, so A retries straight away.
  useEffect(() => {
    if (!launch.error) return;
    const t = setTimeout(() => column.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus());
    return () => clearTimeout(t);
  }, [launch.error]);
  return (
    <main className="content launch-window">
      <div className="launch-column" ref={column}>
        {entry && (
          <span className="launch-cover" style={{ background: entry.system_colour }} aria-hidden>
            <span className="cover-system mono">{entry.system_label}</span>
            <span className="launch-initials">{initials(entry.rom.display_name)}</span>
          </span>
        )}
        <h1 className="screen-title">{title}</h1>
        {launch.error ? (
          <>
            <p className="error" role="alert">
              {launch.error}
            </p>
            <div className="actions">
              <button
                className="button primary"
                data-focusable
                data-autofocus
                data-hint="Retry"
                type="button"
                onClick={onRetry}
              >
                Try again
              </button>
              <button className="button outline" data-focusable data-hint="Open" type="button" onClick={onOpenRomshelf}>
                Open Romshelf
              </button>
            </div>
          </>
        ) : (
          <div className="progress" aria-label="Launch progress">
            <span className="body-text">{launch.label ?? "Getting ready"}…</span>
            <div className="progress-track">
              <div
                className="progress-fill"
                style={{ width: launch.percent != null ? `${launch.percent}%` : "100%" }}
                data-indeterminate={launch.percent == null ? "true" : undefined}
              />
            </div>
            {launch.percent != null && <span className="mono small">{launch.percent}%</span>}
          </div>
        )}
      </div>
      {launch.conflict && onAnswer && <SaveConflictDialog conflict={launch.conflict} onAnswer={onAnswer} />}
    </main>
  );
}
