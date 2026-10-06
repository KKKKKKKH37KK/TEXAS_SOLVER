/** Flop library lines, mirroring LINES in crates/core/src/library.rs. */

export interface LibraryLine {
  id: string;
  label: string;
  /** Action words from UTG, lower case. */
  actions: string[];
}

export const LIBRARY_LINES: LibraryLine[] = [
  { id: 'srp-btn-bb', label: 'BTN open, BB call（SRP）', actions: ['fold', 'fold', 'fold', 'raise', 'fold', 'call'] },
  {
    id: '3bp-bb-btn',
    label: 'BTN open, BB 3bet, BTN call（3BP）',
    actions: ['fold', 'fold', 'fold', 'raise', 'fold', 'raise', 'call'],
  },
  {
    id: '3bp-co-btn',
    label: 'CO open, BTN 3bet, CO call（3BP）',
    actions: ['fold', 'fold', 'raise', 'raise', 'fold', 'fold', 'call'],
  },
];

/** The library line a preflop path (labels like "BTN Raise 2.5") follows, if any. */
export function libraryLineOf(labels: string[]): LibraryLine | null {
  const words = labels.map((l) => (l.split(' ')[1] ?? '').toLowerCase());
  return LIBRARY_LINES.find((l) => l.actions.length === words.length && l.actions.every((a, i) => words[i] === a)) ?? null;
}

/**
 * Where library files live: `VITE_LIBRARY_URL` at build time (the Hugging Face dataset for the
 * published site), else `./library/`, which `vite dev` / `vite preview` serve from the repo.
 */
export const LIBRARY_BASE: string = (import.meta.env.VITE_LIBRARY_URL as string | undefined) || './library/';

/** URL of a library flop file. */
export function libraryUrl(line: string, canonical: string, base = LIBRARY_BASE): string {
  return `${base.endsWith('/') ? base : base + '/'}${line}/${canonical}.hxs`;
}
