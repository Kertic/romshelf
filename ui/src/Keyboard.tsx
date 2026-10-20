import { useState } from "react";
import { openSteamKeyboard } from "./api";
import { useDialogFocus } from "./RomActions";

const ROWS = ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm,.-"];

interface KeyboardProps {
  label: string;
  value: string;
  onDone: (value: string) => void;
  onCancel: () => void;
}

/**
 * Romshelf's own on-screen keyboard, for the gamepad: the fallback when Steam's keyboard
 * doesn't open. D-pad to a key, A to type it.
 */
export function Keyboard({ label, value, onDone, onCancel }: KeyboardProps) {
  const [text, setText] = useState(value);
  const [shift, setShift] = useState(false);
  const ref = useDialogFocus("keyboard");
  const key = (ch: string) => {
    const c = shift ? ch.toUpperCase() : ch;
    return (
      <button
        key={ch}
        className="key"
        data-focusable
        data-hint="Type"
        data-autofocus={ch === "q" ? true : undefined}
        type="button"
        onClick={() => {
          setText((t) => t + c);
          setShift(false);
        }}
      >
        {c}
      </button>
    );
  };
  return (
    <div className="dialog-backdrop">
      <div className="dialog keyboard" role="dialog" aria-modal="true" aria-label={label} data-dialog ref={ref}>
        <h2 className="dialog-title">{label}</h2>
        <output className="text-input keyboard-text" aria-live="polite">
          {text}
          <span className="caret" aria-hidden>
            |
          </span>
        </output>
        {ROWS.map((row) => (
          <div className="key-row" key={row}>
            {[...row].map(key)}
          </div>
        ))}
        <div className="key-row">
          <button className={shift ? "key wide on" : "key wide"} data-focusable data-hint="Shift" type="button" aria-pressed={shift} onClick={() => setShift((s) => !s)}>
            Shift
          </button>
          <button className="key space" data-focusable data-hint="Type" type="button" onClick={() => setText((t) => t + " ")}>
            Space
          </button>
          <button className="key wide" data-focusable data-hint="Delete" type="button" onClick={() => setText((t) => t.slice(0, -1))}>
            Delete
          </button>
        </div>
        <div className="dialog-actions">
          <button className="button outline" data-focusable data-hint="Cancel" type="button" onClick={onCancel}>
            Cancel
          </button>
          <button className="button primary" data-focusable data-hint="Done" type="button" onClick={() => onDone(text)}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

interface FieldProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
}

/**
 * A text field for the Deck: choosing it asks Steam for its on-screen keyboard; the button
 * beside it opens Romshelf's own, in case Steam's doesn't appear.
 */
export function TextField({ label, value, onChange }: FieldProps) {
  const [typing, setTyping] = useState(false);
  return (
    <>
      <div className="text-field">
        <input
          className="text-input"
          data-focusable
          data-hint="Type"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onClick={() => void openSteamKeyboard()}
          aria-label={label}
        />
        <button
          className="button outline"
          data-focusable
          data-hint="Keyboard"
          type="button"
          aria-label={`Type ${label.toLowerCase()} with the controller`}
          onClick={() => setTyping(true)}
        >
          ⌨
        </button>
      </div>
      {typing && (
        <Keyboard
          label={label}
          value={value}
          onCancel={() => setTyping(false)}
          onDone={(v) => {
            onChange(v);
            setTyping(false);
          }}
        />
      )}
    </>
  );
}
