/** 13×13 hand-class grid: pairs on the diagonal, suited above it, offsuit below. */
import type { ActionInfo, NodeView } from '../solver/protocol';

export const RANKS = 'AKQJT98765432';

/** "AhKd" → "AKo"; "QsQc" → "QQ"; "9h8h" → "98s". Higher rank first. */
export function classOf(hand: string): string {
  let [r1, s1, r2, s2] = [hand[0], hand[1], hand[2], hand[3]];
  if (RANKS.indexOf(r1) > RANKS.indexOf(r2)) [r1, s1, r2, s2] = [r2, s2, r1, s1];
  if (r1 === r2) return r1 + r2;
  return r1 + r2 + (s1 === s2 ? 's' : 'o');
}

/** Class name at a grid cell. */
export function cellClass(row: number, col: number): string {
  const [a, b] = [RANKS[row], RANKS[col]];
  if (row === col) return a + b;
  return row < col ? a + b + 's' : b + a + 'o';
}

export interface ClassStats {
  /** Σ reach over the class's combos. */
  weight: number;
  /** Combos in the class with positive reach. */
  combos: number;
  /** Reach-weighted frequency of each action (empty when nobody acts). */
  freq: number[];
  /** Reach-weighted EV, or null without EVs. */
  ev: number | null;
  /** Combo indices into view.hands[player]. */
  hands: number[];
}

export interface Aggregate {
  byClass: Map<string, ClassStats>;
  /** Range-wide reach-weighted action frequencies. */
  total: number[];
  /** Largest class weight, for shading. */
  maxWeight: number;
}

/** Aggregates `player`'s range at a node. Action frequencies only for the player to act. */
export function aggregate(view: NodeView, player: number): Aggregate {
  const hands = view.hands[player];
  const reach = view.reach[player];
  const n = hands.length;
  const acting = view.kind === 'action' && view.player === player && view.strategy !== null;
  const nAct = acting ? view.actions.length : 0;
  const ev = view.ev?.[player] ?? null;
  const byClass = new Map<string, ClassStats>();
  const total = new Array<number>(nAct).fill(0);
  let totalW = 0;
  for (let h = 0; h < n; h++) {
    const w = reach[h];
    const k = classOf(hands[h]);
    let c = byClass.get(k);
    if (!c) {
      c = { weight: 0, combos: 0, freq: new Array<number>(nAct).fill(0), ev: ev ? 0 : null, hands: [] };
      byClass.set(k, c);
    }
    c.hands.push(h);
    if (w <= 0) continue;
    c.weight += w;
    c.combos += 1;
    totalW += w;
    for (let a = 0; a < nAct; a++) {
      const f = w * view.strategy![a * n + h];
      c.freq[a] += f;
      total[a] += f;
    }
    if (ev && c.ev !== null) c.ev += w * ev[h];
  }
  let maxWeight = 0;
  for (const c of byClass.values()) {
    if (c.weight > 0) {
      for (let a = 0; a < nAct; a++) c.freq[a] /= c.weight;
      if (c.ev !== null) c.ev /= c.weight;
    }
    maxWeight = Math.max(maxWeight, c.weight);
  }
  if (totalW > 0) for (let a = 0; a < nAct; a++) total[a] /= totalW;
  return { byClass, total, maxWeight };
}

/** Colour per action: passive green / blue, bets from orange to dark red by size. */
export function actionColors(actions: ActionInfo[], pot: number): string[] {
  return actions.map((a) => {
    switch (a.kind) {
      case 'fold':
        return 'var(--a-fold)';
      case 'check':
      case 'call':
        return 'var(--a-call)';
      case 'allin':
        return 'var(--a-allin)';
      default: {
        // Lightness falls as the bet grows relative to the pot.
        const ratio = Math.min(a.amount / Math.max(pot, 1e-9), 2);
        const light = 62 - ratio * 14;
        return `hsl(${18 - ratio * 8} 75% ${light}%)`;
      }
    }
  });
}

export function pct(x: number, digits = 1): string {
  return `${(x * 100).toFixed(digits)}%`;
}
