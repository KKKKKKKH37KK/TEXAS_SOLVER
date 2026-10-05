import { describe, expect, it } from 'vitest';
import type { PreflopView } from '../src/solver/protocol';
import { preflopActionInfo, preflopAggregate, rangeText } from '../src/ui/preflop';

describe('preflop view helpers', () => {
  it('parses action labels', () => {
    expect(preflopActionInfo(['Fold', 'Call', 'Raise 2.5', 'All-in 100']).map((a) => [a.kind, a.amount])).toEqual([
      ['fold', 0],
      ['call', 0],
      ['raise', 2.5],
      ['allin', 100],
    ]);
  });

  it('aggregates the acting range by class', () => {
    const view: PreflopView = {
      kind: 'action',
      player: 0,
      positions: ['UTG', 'HJ', 'CO', 'BTN', 'SB', 'BB'],
      actions: ['Fold', 'Raise 2.5'],
      // Two classes for brevity: AA always opens, 72o half the time.
      classes: ['AA', '72o'],
      strategy: [0, 0.5, 1, 0.5],
      reach: [[6, 12], [6, 12], [6, 12], [6, 12], [6, 12], [6, 12]],
      contrib: null,
      players: [],
    };
    const g = preflopAggregate(view, 0);
    expect(g.byClass.get('AA')!.freq).toEqual([0, 1]);
    expect(g.byClass.get('72o')!.weight).toBe(1);
    // Open frequency weighted by combos: (6 * 1 + 12 * 0.5) / 18.
    expect(g.total[1]).toBeCloseTo(12 / 18);
    expect(preflopAggregate(view, 1).total).toEqual([]);
  });

  it('writes a range as weighted classes', () => {
    const view = {
      classes: ['AA', 'AKs', '72o'],
      reach: [[6, 2, 0.0001]],
    } as unknown as PreflopView;
    expect(rangeText(view, 0)).toBe('AA,AKs:0.500');
  });
});
