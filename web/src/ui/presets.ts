/** Tree presets, mirroring TreeConfig::preset in crates/core/src/holdem.rs (PRD §3.3). */
import type { StreetSizes } from '../solver/protocol';

export interface TreeForm {
  /** Bet sizes in % of pot, per street, as typed ("33,66,100,125"). */
  bets: [string, string, string];
  maxRaises: [number, number, number];
  raiseMult: number;
  donk: boolean;
}

export function preset(boardLen: number): TreeForm {
  if (boardLen === 3) {
    return { bets: ['33,66,100,125', '66,125', '66,125'], maxRaises: [1, 1, 1], raiseMult: 3, donk: false };
  }
  const all = '33,66,100,125';
  return { bets: [all, all, all], maxRaises: [3, 3, 3], raiseMult: 3, donk: true };
}

/** "33, 66" → [0.33, 0.66]; throws on anything that is not a positive number. */
export function parseBets(s: string): number[] {
  const out = s
    .split(',')
    .map((x) => x.trim())
    .filter((x) => x !== '')
    .map((x) => {
      const v = Number(x.replace('%', ''));
      if (!Number.isFinite(v) || v <= 0) throw new Error(`下注尺寸格式錯誤：${x}`);
      return v / 100;
    });
  return out;
}

export function toSizes(f: TreeForm): [StreetSizes, StreetSizes, StreetSizes] {
  return [0, 1, 2].map((i) => ({ bets: parseBets(f.bets[i]), raiseMult: f.raiseMult, maxRaises: f.maxRaises[i] })) as [
    StreetSizes,
    StreetSizes,
    StreetSizes,
  ];
}

/** Number of cards in a board string like "Ks7d2c". */
export function boardLen(board: string): number {
  return Math.floor(board.replace(/\s/g, '').length / 2);
}
