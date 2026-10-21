import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  library,
  onLaunchUpdate,
  play,
  selftestFlush,
  selftestProbe,
  selftestRun,
  selftestSave,
  type LibraryEntry,
  type SelfTestCheck,
  type SelfTestProbe,
} from "./api";
import type { KeyHandler } from "./Controls";
import type { BackAction } from "./FirstRun";
import { TextField } from "./Keyboard";

type Outcome = SelfTestCheck["outcome"];
type Done = (outcome: Outcome, detail: string) => void;

interface Props {
  entries: LibraryEntry[] | null;
  version: string;
  setBack: (b: BackAction | null) => void;
  onBack: () => void;
  registerKeys: (h: KeyHandler | null) => void;
  /** Shows each screen in turn and checks it (App's part: it owns the screens). */
  tour: () => Promise<SelfTestCheck[]>;
  copy: (text: string) => Promise<void>;
}

/** The plain-text report, in the same form as `romshelf doctor --selftest`. */
export function report(checks: SelfTestCheck[], version: string, at: Date): string {
  const tag: Record<Outcome, string> = { pass: "pass", warn: "WARN", fail: "FAIL", skip: "skip" };
  const lines = [`Romshelf ${version} self-test, ${at.toISOString()}`, ""];
  for (const c of checks) {
    lines.push(`${tag[c.outcome]}  ${c.group} / ${c.name}: ${c.detail}`);
    if (c.fix) lines.push(`      Fix: ${c.fix}`);
  }
  const n = (o: Outcome) => checks.filter((c) => c.outcome === o).length;
  lines.push("", `${n("pass")} passed, ${n("warn")} to look at, ${n("fail")} failed, ${n("skip")} skipped`, "");
  return lines.join("\n");
}

const playable = (e: LibraryEntry) => e.rom.location.state === "on_deck" && e.emulator_ready;

interface Step {
  id: "buttons" | "keyboard" | "readable" | "controls" | "offline" | "sdcard";
  name: string;
}

const STEPS: Step[] = [
  { id: "buttons", name: "Buttons" },
  { id: "keyboard", name: "On-screen keyboard" },
  { id: "readable", name: "Readable at arm's length" },
  { id: "controls", name: "A game's own controls" },
  { id: "offline", name: "Airplane mode" },
  { id: "sdcard", name: "SD card out and back" },
];

/** How often the guided steps look again (tests shorten it). */
export const timing = { pollMs: 2000, buttonsSeconds: 45, secondMs: 1000 };

/** Polls `fn` every `ms` while `active`, until it returns true. */
function usePoll(fn: () => Promise<boolean>, ms: number, active: boolean) {
  const saved = useRef(fn);
  useEffect(() => {
    saved.current = fn;
  });
  useEffect(() => {
    if (!active) return;
    let live = true;
    let t: ReturnType<typeof setTimeout>;
    const tick = async () => {
      const stop = await saved.current().catch(() => false);
      if (live && !stop) t = setTimeout(tick, ms);
    };
    void tick();
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [ms, active]);
}

/** Focuses the step's main action when it changes. */
function useStepFocus(key: unknown) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const t = setTimeout(() => {
      const root = ref.current;
      if (!root || root.contains(document.activeElement)) return;
      (root.querySelector<HTMLElement>("[data-autofocus]:not([disabled])") ??
        root.querySelector<HTMLElement>("[data-focusable]:not([disabled])"))?.focus();
    });
    return () => clearTimeout(t);
  }, [key]);
  return ref;
}

function Choice({ onDone, worked = "It worked", didnt = "It didn't" }: { onDone: Done; worked?: string; didnt?: string }) {
  return (
    <div className="actions">
      <button className="button primary" data-focusable data-autofocus data-hint="Pass" type="button" onClick={() => onDone("pass", "Confirmed on the Deck.")}>
        {worked}
      </button>
      <button className="button outline" data-focusable data-hint="Fail" type="button" onClick={() => onDone("fail", "Reported not working on the Deck.")}>
        {didnt}
      </button>
      <SkipButton onDone={onDone} />
    </div>
  );
}

function SkipButton({ onDone, detail = "Skipped." }: { onDone: Done; detail?: string }) {
  return (
    <button className="button outline" data-focusable data-hint="Skip" type="button" onClick={() => onDone("skip", detail)}>
      Skip
    </button>
  );
}

const BUTTONS: [string, string][] = [
  ["a", "A"],
  ["b", "B"],
  ["x", "X"],
  ["y", "Y"],
  ["l1", "L1"],
  ["r1", "R1"],
  ["l4", "L4"],
  ["r4", "R4"],
  ["l5", "L5"],
  ["r5", "R5"],
];

/** Every button once. The back grips arrive as F9 to F12 through Romshelf's Steam Input layout. */
export function ButtonsStep({ registerKeys, onDone }: { registerKeys: Props["registerKeys"]; onDone: Done }) {
  const [seen, setSeen] = useState<Set<string>>(new Set());
  const [left, setLeft] = useState(timing.buttonsSeconds);
  const finished = useRef(false);
  const finish = useCallback(
    (pressed: Set<string>) => {
      if (finished.current) return;
      finished.current = true;
      const missing = BUTTONS.filter(([id]) => !pressed.has(id)).map(([, l]) => l);
      if (!missing.length) onDone("pass", "Every button, back grips included, reached Romshelf.");
      else {
        const grips = missing.filter((l) => /^[LR][45]$/.test(l));
        const why = grips.length
          ? " The back grips reach Romshelf only through its Steam Input layout, on the shortcut Romshelf was opened from."
          : "";
        onDone(missing.length === BUTTONS.length ? "fail" : "warn", `Not seen: ${missing.join(", ")}.${why}`);
      }
    },
    [onDone],
  );
  useEffect(() => {
    registerKeys((b) => {
      setSeen((s) => (!BUTTONS.some(([id]) => id === b) || s.has(b) ? s : new Set(s).add(b)));
      return true;
    });
    return () => registerKeys(null);
  }, [registerKeys]);
  useEffect(() => {
    if (seen.size === BUTTONS.length || left <= 0) {
      finish(seen);
      return;
    }
    const t = setTimeout(() => setLeft((n) => n - 1), timing.secondMs);
    return () => clearTimeout(t);
  }, [left, seen, finish]);
  return (
    <>
      <p className="body-text">Press each button once, the four back grips too. Buttons do nothing else here.</p>
      <div className="selftest-buttons" aria-label="Buttons pressed">
        {BUTTONS.map(([id, l]) => (
          <span key={id} className={seen.has(id) ? "selftest-button seen" : "selftest-button"} aria-label={`${l}${seen.has(id) ? ", pressed" : ""}`}>
            {l}
          </span>
        ))}
      </div>
      <p className="muted">{left} s left</p>
    </>
  );
}

function KeyboardStep({ onDone }: { onDone: Done }) {
  const [value, setValue] = useState("");
  useEffect(() => {
    if (value.trim().toLowerCase() === "ok") onDone("pass", "Typed \"ok\" in a text field.");
  }, [value, onDone]);
  return (
    <>
      <p className="body-text">
        Select ⌨ (or the field) to open Steam&rsquo;s keyboard, type <span className="mono">ok</span> and close it.
      </p>
      <TextField label="Type ok" value={value} onChange={setValue} />
      <div className="actions">
        <button className="button outline" data-focusable data-hint="Fail" type="button" onClick={() => onDone("fail", `The keyboard didn't work; the field held "${value}".`)}>
          It didn&rsquo;t open
        </button>
        <SkipButton onDone={onDone} />
      </div>
    </>
  );
}

/** Plays a game from here and reports how it ended. */
function usePlay(entry: LibraryEntry | undefined) {
  const [state, setState] = useState<"idle" | "playing" | "ended">("idle");
  const [ending, setEnding] = useState("");
  useEffect(() => {
    if (state !== "playing" || !entry) return;
    let live = true;
    let unlisten: (() => void) | null = null;
    onLaunchUpdate((u) => {
      if (u.rom_id !== entry.rom.id) return;
      if (u.kind === "finished") {
        setEnding(`${entry.rom.display_name} ended with exit ${u.code ?? 0}`);
        setState("ended");
      } else if (u.kind === "failed") {
        setEnding(`${entry.rom.display_name} couldn't start: ${u.message ?? "no reason given"}`);
        setState("ended");
      }
    }).then((un) => (live ? (unlisten = un) : un()));
    return () => {
      live = false;
      unlisten?.();
    };
  }, [state, entry]);
  const start = useCallback(() => {
    if (!entry) return;
    setState("playing");
    play(entry.rom.id).catch((e) => {
      setEnding(`${entry.rom.display_name} couldn't start: ${e?.message ?? String(e)}`);
      setState("ended");
    });
  }, [entry]);
  return { state, ending, start, failed: ending.includes("couldn't start") };
}

function ControlsStep({ entry, onDone }: { entry: LibraryEntry; onDone: Done }) {
  const game = usePlay(entry);
  if (game.state === "idle") {
    return (
      <>
        <p className="body-text">
          {entry.rom.display_name}&rsquo;s controls differ from {entry.emulator}&rsquo;s default in {entry.control_differences}{" "}
          {entry.control_differences === 1 ? "button" : "buttons"}. Play it, check they do what its Controls page says, then quit.
        </p>
        <div className="actions">
          <button className="button primary" data-focusable data-autofocus data-hint="Play" type="button" onClick={game.start}>
            Play {entry.rom.display_name}
          </button>
          <SkipButton onDone={onDone} />
        </div>
      </>
    );
  }
  if (game.state === "playing") return <p className="body-text">Playing {entry.rom.display_name}… Quit it to come back here.</p>;
  return (
    <>
      <p className="body-text">{game.ending}. Did its buttons do what the Controls page says?</p>
      <Choice onDone={(o, d) => onDone(o, `${d} ${game.ending}.`)} worked="Yes" didnt="No" />
    </>
  );
}

type OfflineStage = "on" | "play" | "playing" | "off" | "uploading";

export function OfflineStep({ entry, onDone }: { entry: LibraryEntry | undefined; onDone: Done }) {
  const [stage, setStage] = useState<OfflineStage>("on");
  const notes = useRef<string[]>([]);
  const game = usePlay(entry);
  const deadline = useRef(0);
  const note = (s: string) => notes.current.push(s);
  const finish = (o: Outcome, last?: string) => {
    if (last) note(last);
    onDone(o, notes.current.join(" "));
  };

  usePoll(
    async () => {
      const p = await selftestProbe();
      if (p.online) return false;
      note("Romshelf saw the network go.");
      const lib = await library().catch(() => null);
      note(lib ? `The Library opened offline (${lib.length} games).` : "The Library didn't open offline.");
      setStage(entry ? "play" : "off");
      if (!entry) note("No downloaded game to play offline.");
      return true;
    },
    timing.pollMs,
    stage === "on",
  );
  useEffect(() => {
    if (game.state !== "ended" || stage !== "playing") return;
    note(`${game.ending}.`);
    void selftestProbe().then((p) => {
      note(p.pending_backups > 0 ? `${p.pending_backups} save backup waiting for the network.` : "No save backup was waiting.");
      setStage("off");
    });
  }, [game.state, game.ending, stage]);
  usePoll(
    async () => {
      const p = await selftestProbe();
      if (!p.online) return false;
      note("The network came back.");
      if (p.pending_backups === 0) {
        finish(game.failed ? "fail" : "pass", "Nothing was left to back up.");
        return true;
      }
      await selftestFlush();
      deadline.current = Date.now() + 120_000;
      setStage("uploading");
      return true;
    },
    timing.pollMs,
    stage === "off",
  );
  usePoll(
    async () => {
      const p = await selftestProbe();
      if (p.pending_backups === 0) {
        finish(game.failed ? "fail" : "pass", "The waiting backup reached Drive.");
        return true;
      }
      if (Date.now() > deadline.current) {
        finish("warn", `${p.pending_backups} backup still waiting after two minutes.`);
        return true;
      }
      return false;
    },
    timing.pollMs,
    stage === "uploading",
  );

  const skip = <SkipButton onDone={(o) => finish(o, "Skipped here.")} />;
  if (stage === "on")
    return (
      <>
        <p className="body-text">
          Turn on airplane mode: press the <span className="mono">…</span> button, then the airplane in Quick Settings. This waits until the
          network is gone.
        </p>
        <div className="actions">{skip}</div>
      </>
    );
  if (stage === "play" && entry)
    return (
      <>
        <p className="body-text">Offline now. Play {entry.rom.display_name}, save in the game, then quit it.</p>
        <div className="actions">
          <button
            className="button primary"
            data-focusable
            data-autofocus
            data-hint="Play"
            type="button"
            onClick={() => {
              setStage("playing");
              game.start();
            }}
          >
            Play {entry.rom.display_name}
          </button>
          {skip}
        </div>
      </>
    );
  if (stage === "playing") return <p className="body-text">Playing {entry?.rom.display_name}… Quit it to come back here.</p>;
  if (stage === "off")
    return (
      <>
        <p className="body-text">Now turn airplane mode off. This waits until Google answers again.</p>
        <div className="actions">{skip}</div>
      </>
    );
  return <p className="body-text">Backing up the waiting save…</p>;
}

function SdCardStep({ onDone }: { onDone: Done }) {
  const [probe, setProbe] = useState<SelfTestProbe | null>(null);
  const [stage, setStage] = useState<"out" | "in">("out");
  const removedAt = useRef(0);
  useEffect(() => {
    void selftestProbe().then((p) => {
      if (p.storage_removable) setProbe(p);
      else onDone("skip", `ROMs are on this Deck's own storage (${p.storage_root}). Move them to the SD card in Settings to test this.`);
    });
  }, [onDone]);
  usePoll(
    async () => {
      const p = await selftestProbe();
      if (p.storage_available) return false;
      removedAt.current = Date.now();
      setStage("in");
      return true;
    },
    timing.pollMs,
    probe != null && stage === "out",
  );
  usePoll(
    async () => {
      const p = await selftestProbe();
      if (!p.storage_available) return false;
      onDone("pass", `Romshelf saw the card go and come back (${Math.round((Date.now() - removedAt.current) / 1000)} s out).`);
      return true;
    },
    timing.pollMs,
    probe != null && stage === "in",
  );
  if (!probe) return <p className="body-text">Looking at where ROMs are kept…</p>;
  return (
    <>
      <p className="body-text">
        {stage === "out"
          ? `Take out the SD card holding ${probe.storage_root}. Don't play anything while it's out.`
          : "Romshelf saw it go. Put it back in."}
      </p>
      <div className="actions">
        <SkipButton onDone={onDone} />
      </div>
    </>
  );
}

const ICON: Record<Outcome, string> = { pass: "✓", warn: "!", fail: "✕", skip: "–" };

export function SelfTest({ entries, version, setBack, onBack, registerKeys, tour, copy }: Props) {
  const [checks, setChecks] = useState<SelfTestCheck[]>([]);
  const [phase, setPhase] = useState<"auto" | "guided" | "done">("auto");
  const [status, setStatus] = useState("Checking Romshelf's setup…");
  const [stepIndex, setStepIndex] = useState(0);
  const [savedTo, setSavedTo] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [started] = useState(() => new Date());

  const controlsGame = useMemo(() => entries?.find((e) => playable(e) && e.control_differences > 0), [entries]);
  const offlineGame = useMemo(() => entries?.find(playable), [entries]);

  const ownBack = useCallback(() => setBack({ label: "Settings", run: onBack }), [setBack, onBack]);
  useEffect(() => {
    ownBack();
  }, [ownBack]);

  // The automatic part, once: Romshelf's checks, then each screen. (Once even where React runs
  // effects twice: the tour would otherwise run twice.)
  const ran = useRef(false);
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    if (ran.current) return;
    ran.current = true;
    (async () => {
      await selftestRun((c) => mounted.current && setChecks((cs) => [...cs, c]));
      if (!mounted.current) return;
      setStatus("Checking each screen…");
      const screens = await tour();
      if (!mounted.current) return;
      ownBack();
      setChecks((cs) => [...cs, ...screens]);
      setPhase("guided");
    })().catch((e) => {
      if (!mounted.current) return;
      setChecks((cs) => [
        ...cs,
        { group: "Self-test", name: "Automatic checks", outcome: "fail", detail: `They stopped: ${e?.message ?? String(e)}`, fix: "Send this report." },
      ]);
      setPhase("guided");
    });
  }, [tour, ownBack]);

  // The report is saved whenever a step finishes, so nothing is lost if Romshelf closes.
  const text = useMemo(() => report(checks, version, started), [checks, version, started]);
  useEffect(() => {
    if (phase === "auto") return;
    void selftestSave(text, { version, started: started.toISOString(), checks }).then(setSavedTo);
  }, [phase, text, version, checks, started]);

  const step = phase === "guided" ? STEPS[stepIndex] : undefined;
  const record = useCallback(
    (outcome: Outcome, detail: string) => {
      const s = STEPS[stepIndex];
      if (!s) return;
      setChecks((cs) => [...cs, { group: "Guided", name: s.name, outcome, detail, fix: outcome === "fail" ? "Send this report." : null }]);
      if (stepIndex + 1 >= STEPS.length) setPhase("done");
      else setStepIndex(stepIndex + 1);
    },
    [stepIndex],
  );
  const focusRef = useStepFocus(`${phase}-${stepIndex}`);

  // Steps with nothing to do here finish at once.
  useEffect(() => {
    if (step?.id === "controls" && !controlsGame) {
      const t = setTimeout(() => record("skip", "No downloaded game has controls of its own."));
      return () => clearTimeout(t);
    }
  }, [step, controlsGame, record]);

  const count = (o: Outcome) => checks.filter((c) => c.outcome === o).length;
  return (
    <main className="content selftest" data-region="grid">
      <h1 className="screen-title">Self-test</h1>
      <p className="muted">
        {phase === "auto"
          ? status
          : `${count("pass")} passed · ${count("warn")} to look at · ${count("fail")} failed · ${count("skip")} skipped`}
        {savedTo && phase !== "auto" ? ` · Saved to ${savedTo}` : ""}
      </p>
      <div className="selftest-body">
        <section className="selftest-step" ref={focusRef}>
          {phase === "auto" && <p className="body-text">Romshelf is checking itself. This takes about a minute and changes nothing of yours.</p>}
          {step && (
            <>
              <h2 className="section-heading">
                Step {stepIndex + 1} of {STEPS.length}: {step.name}
              </h2>
              {step.id === "buttons" && <ButtonsStep registerKeys={registerKeys} onDone={record} />}
              {step.id === "keyboard" && <KeyboardStep onDone={record} />}
              {step.id === "readable" && (
                <>
                  <p className="body-text">Hold the Deck at arm&rsquo;s length. Can you read this, and the smaller grey text above?</p>
                  <Choice onDone={record} worked="I can read it" didnt="It's too small" />
                </>
              )}
              {step.id === "controls" && controlsGame && <ControlsStep entry={controlsGame} onDone={record} />}
              {step.id === "offline" && <OfflineStep entry={offlineGame} onDone={record} />}
              {step.id === "sdcard" && <SdCardStep onDone={record} />}
            </>
          )}
          {phase === "done" && (
            <>
              <p className="body-text">All done. The report is saved on the Deck; Claude can read it from there.</p>
              <div className="actions">
                <button
                  className="button primary"
                  data-focusable
                  data-autofocus
                  data-hint="Copy"
                  type="button"
                  onClick={() => void copy(text).then(() => setCopied(true))}
                >
                  {copied ? "Copied" : "Copy report"}
                </button>
                <button className="button outline" data-focusable data-hint="Back" type="button" onClick={onBack}>
                  Back to Settings
                </button>
              </div>
            </>
          )}
        </section>
        <ul className="selftest-list" aria-label="Results">
          {checks.map((c, i) => (
            <li key={i} className={`selftest-row ${c.outcome}`}>
              <span className="selftest-icon" aria-label={c.outcome}>
                {ICON[c.outcome]}
              </span>
              <span>
                <span className="card-label">
                  {c.group} / {c.name}
                </span>
                {c.outcome !== "pass" && <span className="muted small"> {c.detail}</span>}
              </span>
            </li>
          ))}
        </ul>
      </div>
    </main>
  );
}
