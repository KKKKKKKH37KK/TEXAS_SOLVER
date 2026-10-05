import { describe, expect, it } from 'vitest';
import { cardIndex, comboIndex, toComboWeights } from '../src/ui/cards';

describe('card and combo indices (same as the Rust core)', () => {
  it('indexes cards rank * 4 + suit', () => {
    expect(cardIndex('2c')).toBe(0);
    expect(cardIndex('As')).toBe(51);
    expect(cardIndex('Kd')).toBe(45);
  });

  it('indexes combos like combo_index', () => {
    expect(comboIndex('3c2c')).toBe(comboIndex('2c3c'));
    expect(comboIndex('2d2c')).toBe(0); // hi = 1, lo = 0
    expect(comboIndex('AsAh')).toBe(1325); // hi = 51, lo = 50
    const all = new Set<number>();
    for (let hi = 1; hi < 52; hi++) for (let lo = 0; lo < hi; lo++) all.add((hi * (hi - 1)) / 2 + lo);
    expect(all.size).toBe(1326);
  });

  it('spreads weights over the 1326 combos', () => {
    const w = toComboWeights(['AsAh', 'KdQc'], [0.5, 1]);
    expect(w.length).toBe(1326);
    expect(w[1325]).toBe(0.5);
    expect(w[comboIndex('QcKd')]).toBe(1);
    expect(w.reduce((a, b) => a + b, 0)).toBe(1.5);
  });
});
