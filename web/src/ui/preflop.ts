/** Turns a preflop view into the grid aggregate and action colours used by RangeGrid. */
import type { ActionInfo, PreflopView } from '../solver/protocol';
import type { Aggregate, ClassStats } from './grid';

const COMBOS = (name: string) => (name.length === 2 ? 6 : name.endsWith('s') ? 4 : 12);

/** "Raise 2.5" → bet of 2.5 bb, "All-in 100" → all-in, so actionColors can shade them. */
export function preflopActionInfo(labels: string[]): ActionInfo[] {
  return labels.map((label) => {
    const [word, amount] = label.split(' ');
    const kind = word === 'Fold' ? 'fold' : word === 'Call' ? 'call' : word === 'All-in' ? 'allin' : 'raise';
    return { kind, amount: Number(amount ?? 0), label };
  });
}

/** Aggregate for `player`'s range; action frequencies only for the player to act. */
export function preflopAggregate(view: PreflopView, player: number): Aggregate {
  const acting = view.kind === 'action' && view.player === player && view.strategy !== null;
  const nAct = acting ? view.actions.length : 0;
  const reach = view.reach[player];
  const byClass = new Map<string, ClassStats>();
  const total = new Array<number>(nAct).fill(0);
  let totalW = 0;
  let maxWeight = 0;
  view.classes.forEach((name, i) => {
    const w = reach[i];
    const freq = new Array<number>(nAct).fill(0);
    for (let a = 0; a < nAct; a++) {
      freq[a] = view.strategy![a * view.classes.length + i];
      total[a] += w * freq[a];
    }
    totalW += w;
    // Shade by the share of the class's combos still in the range.
    const share = w / COMBOS(name);
    maxWeight = Math.max(maxWeight, share);
    byClass.set(name, { weight: share, combos: w > 1e-9 ? COMBOS(name) : 0, freq, ev: null, hands: [] });
  });
  if (totalW > 0) for (let a = 0; a < nAct; a++) total[a] /= totalW;
  return { byClass, total, maxWeight };
}
