import { useEffect, useRef } from "react";
import type { Progress, TransferRow } from "./api";
import { humanSize, progressLine } from "./RomActions";

interface Props {
  rows: TransferRow[];
  progress: Record<string, Progress>;
  onCancel: (romId: string) => void;
  onClose: () => void;
}

/** The Transfers panel (UI guide section 6), from the right: downloads and queued backups. */
export function Transfers({ rows, progress, onCancel, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const t = setTimeout(() => ref.current?.querySelector<HTMLElement>("[data-focusable]")?.focus());
    return () => clearTimeout(t);
  }, []);
  return (
    <div className="panel-backdrop">
      <aside className="panel" role="dialog" aria-modal="true" aria-label="Transfers" data-dialog ref={ref}>
        <h2 className="dialog-title">Transfers</h2>
        {rows.length === 0 && <p className="body-text muted">Nothing in progress.</p>}
        {rows.map((r) => {
          const p = progress[r.rom_id];
          const fraction = p && p.total > 0 ? p.bytes / p.total : 0;
          return (
            <div className="transfer" key={`${r.kind}-${r.rom_id}`}>
              <div className="transfer-head">
                <span className="card-label">{r.name}</span>
                <span className="muted small">
                  {r.kind === "download"
                    ? r.state === "queued"
                      ? `Waiting to download · ${humanSize(r.size)}`
                      : p
                        ? progressLine(p)
                        : "Downloading"
                    : "Save waiting to back up"}
                </span>
              </div>
              {r.kind === "download" && (
                <div className="transfer-row">
                  <div className="progress-track">
                    <div className="progress-fill" style={{ width: `${Math.round(fraction * 100)}%` }} />
                  </div>
                  <button
                    className="button outline"
                    data-focusable
                    data-hint="Cancel"
                    type="button"
                    aria-label={`Cancel downloading ${r.name}`}
                    onClick={() => onCancel(r.rom_id)}
                  >
                    Cancel
                  </button>
                </div>
              )}
              {r.message && <span className="tile-status warn">{r.message}</span>}
            </div>
          );
        })}
        <div className="dialog-actions">
          <button className="button outline" data-focusable data-hint="Close" type="button" onClick={onClose}>
            Close
          </button>
        </div>
      </aside>
    </div>
  );
}
