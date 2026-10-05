import { describe, expect, it } from 'vitest';
import { libraryLineOf, libraryUrl } from '../src/ui/library';

describe('library lines', () => {
  it('recognises the three library lines from preflop labels', () => {
    expect(libraryLineOf(['UTG Fold', 'HJ Fold', 'CO Fold', 'BTN Raise 2.5', 'SB Fold', 'BB Call'])?.id).toBe(
      'srp-btn-bb',
    );
    expect(
      libraryLineOf(['UTG Fold', 'HJ Fold', 'CO Fold', 'BTN Raise 2.5', 'SB Fold', 'BB Raise 10.0', 'BTN Call'])?.id,
    ).toBe('3bp-bb-btn');
    expect(
      libraryLineOf(['UTG Fold', 'HJ Fold', 'CO Raise 2.5', 'BTN Raise 7.5', 'SB Fold', 'BB Fold', 'CO Call'])?.id,
    ).toBe('3bp-co-btn');
    expect(libraryLineOf(['UTG Raise 2.5', 'HJ Fold', 'CO Fold', 'BTN Fold', 'SB Fold', 'BB Call'])).toBeNull();
  });

  it('builds file URLs', () => {
    expect(libraryUrl('srp-btn-bb', 'KsJsJh')).toBe('./library/srp-btn-bb/KsJsJh.hxs');
  });
});
