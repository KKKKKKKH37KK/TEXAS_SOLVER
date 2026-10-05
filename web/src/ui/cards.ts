/** Card and combo indices, matching crates/core/src/cards.rs: card = rank * 4 + suit. */

const RANK_CHARS = '23456789TJQKA';
const SUIT_CHARS = 'cdhs';

export function cardIndex(card: string): number {
  const r = RANK_CHARS.indexOf(card[0].toUpperCase());
  const s = SUIT_CHARS.indexOf(card[1].toLowerCase());
  if (r < 0 || s < 0) throw new Error(`bad card ${card}`);
  return r * 4 + s;
}

/** Index of a two-card combo such as "AhKd" in 0..1326 (`combo_index`). */
export function comboIndex(hand: string): number {
  const a = cardIndex(hand.slice(0, 2));
  const b = cardIndex(hand.slice(2, 4));
  const [lo, hi] = a < b ? [a, b] : [b, a];
  return (hi * (hi - 1)) / 2 + lo;
}

/** Per-combo weights (1326) from a list of hands and their weights. */
export function toComboWeights(hands: string[], weights: number[]): number[] {
  const out = new Array<number>(1326).fill(0);
  hands.forEach((h, i) => {
    out[comboIndex(h)] = weights[i];
  });
  return out;
}
