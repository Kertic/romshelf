export interface Hint {
  glyph: string;
  label: string;
}

/** The 48 px hint bar: button glyphs and what they do for the focused element. */
export function Footer({ hints }: { hints: Hint[] }) {
  return (
    <footer className="footer" aria-label="Button hints">
      {hints.map((h) => (
        <span className="hint" key={h.glyph + h.label}>
          <span className="hint-glyph">{h.glyph}</span>
          {h.label}
        </span>
      ))}
    </footer>
  );
}
