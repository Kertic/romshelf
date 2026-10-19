import { BUTTON_LABEL, STANDARD_ORDER, type Button } from "./input";
import type { InputState } from "./useInput";
import { inTauri } from "./api";

function Buttons({ pressed }: { pressed: Set<string> }) {
  return (
    <div className="pad-buttons">
      {STANDARD_ORDER.map((b) => (
        <span key={b} className={pressed.has(b) ? "pad-button on" : "pad-button"}>
          {BUTTON_LABEL[b]}
        </span>
      ))}
    </div>
  );
}

function Axis({ name, value }: { name: string; value: number }) {
  const pct = Math.max(-1, Math.min(1, value)) * 50;
  return (
    <div className="axis">
      <span className="mono muted axis-name">{name}</span>
      <span className="axis-track">
        <span
          className="axis-fill"
          style={{ left: pct < 0 ? `${50 + pct}%` : "50%", width: `${Math.abs(pct)}%` }}
        />
      </span>
      <span className="mono axis-value">{value.toFixed(2)}</span>
    </div>
  );
}

/** Spike A: live state from both input paths, side by side. */
export function GamepadSpike({ input, holdingB }: { input: InputState; holdingB: boolean }) {
  const sourceText =
    input.source === null
      ? "Press any button. The first path to report it will drive navigation."
      : `Navigation is using ${input.source === "browser" ? "the browser Gamepad API" : "gilrs (Rust)"}.`;
  return (
    <main className="content spike">
      <div className="home-heading">
        <h1 className="screen-title">Gamepad test</h1>
        <span className={holdingB ? "accent" : "muted"}>{holdingB ? "Keep holding B…" : sourceText}</span>
      </div>
      <div className="spike-columns">
        <section className="card">
          <h2 className="card-heading">Browser Gamepad API</h2>
          {input.browser.length === 0 && <p className="muted">No pad reported. Press a button to wake it.</p>}
          {input.browser.map((p) => (
            <div key={p.index} className="pad">
              <span className="mono muted pad-name">
                #{p.index} {p.id} · mapping “{p.mapping || "none"}”
              </span>
              <Buttons pressed={new Set<Button>(p.pressed)} />
              {p.axes.map((v, i) => (
                <Axis key={i} name={`axis ${i}`} value={v} />
              ))}
            </div>
          ))}
        </section>
        <section className="card">
          <h2 className="card-heading">gilrs (Rust)</h2>
          {!inTauri() && <p className="muted">Only available inside the app.</p>}
          {inTauri() && (input.gilrs?.pads.length ?? 0) === 0 && <p className="muted">No pad reported by gilrs.</p>}
          {input.gilrs?.pads.map((p) => (
            <div key={p.id} className="pad">
              <span className="mono muted pad-name">
                #{p.id} {p.name}
              </span>
              <Buttons pressed={new Set(p.pressed)} />
              {Object.entries(p.axes)
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([name, v]) => (
                  <Axis key={name} name={name} value={v} />
                ))}
            </div>
          ))}
          {input.gilrs?.last_event && <p className="mono muted">Last: {input.gilrs.last_event}</p>}
        </section>
      </div>
    </main>
  );
}
