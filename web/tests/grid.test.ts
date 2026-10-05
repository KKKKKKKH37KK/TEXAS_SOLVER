import { describe, expect, it } from 'vitest';
import type { NodeView } from '../src/solver/protocol';
import { aggregate, cellClass, classOf } from '../src/ui/grid';

describe('hand classes', () => {
  it('names classes higher rank first', () => {
    expect(classOf('AhKd')).toBe('AKo');
    expect(classOf('KdAh')).toBe('AKo');
    expect(classOf('9h8h')).toBe('98s');
    expect(classOf('QsQc')).toBe('QQ');
    expect(classOf('2c3c')).toBe('32s');
  });

  it('lays out the grid', () => {
    expect(cellClass(0, 0)).toBe('AA');
    expect(cellClass(0, 1)).toBe('AKs');
    expect(cellClass(1, 0)).toBe('AKo');
    expect(cellClass(12, 12)).toBe('22');
    expect(cellClass(11, 12)).toBe('32s');
  });
});

describe('aggregate', () => {
  const view: NodeView = {
    kind: 'action',
    player: 0,
    actions: [
      { kind: 'check', amount: 0, label: 'Check' },
      { kind: 'bet', amount: 5, label: 'Bet 5.00' },
    ],
    board: ['Ks', '7d', '2c'],
    pot: 10,
    stacks: [50, 50],
    street: [0, 0],
    hands: [['AhAd', 'AhAc', 'QsQd'], ['JhJd']],
    // AhAc has half the reach of AhAd; QsQd is gone.
    reach: [[1, 0.5, 0], [1]],
    // [action][hand]: check 0.2 / 0.8 / -, bet 0.8 / 0.2 / -
    strategy: [0.2, 0.8, 1, 0.8, 0.2, 0],
    ev: [[10, 4, 0], [1]],
    dealable: [],
  };

  it('weights frequencies and EV by reach', () => {
    const g = aggregate(view, 0);
    const aa = g.byClass.get('AA')!;
    expect(aa.weight).toBe(1.5);
    expect(aa.combos).toBe(2);
    expect(aa.freq[0]).toBeCloseTo((0.2 + 0.5 * 0.8) / 1.5);
    expect(aa.freq[1]).toBeCloseTo((0.8 + 0.5 * 0.2) / 1.5);
    expect(aa.ev).toBeCloseTo((10 + 0.5 * 4) / 1.5);
    expect(g.byClass.get('QQ')!.combos).toBe(0);
    expect(g.total[0] + g.total[1]).toBeCloseTo(1);
  });

  it('gives no frequencies for the player not acting', () => {
    const g = aggregate(view, 1);
    expect(g.total).toEqual([]);
    expect(g.byClass.get('JJ')!.weight).toBe(1);
  });
});
