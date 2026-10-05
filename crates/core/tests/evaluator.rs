//! PRD §8.1: category counts over every 5- and 7-card hand must match the known totals.

use hexas_core::cards::Card;
use hexas_core::eval::{Category, evaluate};

/// Counts categories over all k-card subsets of the deck whose smallest card is `first`.
fn count_from(first: u8, k: usize) -> [u64; 9] {
    let mut counts = [0u64; 9];
    let mut idx = vec![0u8; k];
    idx[0] = first;
    let mut cards = vec![Card::from_index(first); k];
    // Recursive enumeration of the remaining k-1 cards in increasing order.
    fn rec(pos: usize, start: u8, idx: &mut [u8], cards: &mut [Card], counts: &mut [u64; 9]) {
        if pos == idx.len() {
            counts[Category::of(evaluate(cards)) as usize] += 1;
            return;
        }
        let left = (idx.len() - pos) as u8;
        for c in start..=52 - left {
            idx[pos] = c;
            cards[pos] = Card::from_index(c);
            rec(pos + 1, c + 1, idx, cards, counts);
        }
    }
    rec(1, first + 1, &mut idx, &mut cards, &mut counts);
    counts
}

fn count_all(k: usize) -> [u64; 9] {
    let firsts: Vec<u8> = (0..=(52 - k) as u8).collect();
    let results: Vec<[u64; 9]> = std::thread::scope(|s| {
        let handles: Vec<_> = firsts
            .iter()
            .map(|&f| s.spawn(move || count_from(f, k)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut total = [0u64; 9];
    for r in results {
        for (t, x) in total.iter_mut().zip(r) {
            *t += x;
        }
    }
    total
}

#[test]
fn five_card_counts() {
    // High card .. straight flush (royal flushes included in straight flush).
    assert_eq!(
        count_all(5),
        [
            1_302_540, 1_098_240, 123_552, 54_912, 10_200, 5_108, 3_744, 624, 40
        ]
    );
}

/// 133,784,560 hands: run with `cargo test --release -- --ignored` (minutes in a debug build).
#[test]
#[ignore]
fn seven_card_counts() {
    assert_eq!(
        count_all(7),
        [
            23_294_460, 58_627_800, 31_433_400, 6_461_620, 6_180_020, 4_047_644, 3_473_184,
            224_848, 41_584
        ]
    );
}
