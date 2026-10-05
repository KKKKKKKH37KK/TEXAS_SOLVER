//! The 169 starting-hand classes, indexed like the 13×13 grid: row and column run A..2, pairs on
//! the diagonal, suited above it (row < column), offsuit below.

use crate::cards::{Card, RANKS};

pub const NUM_CLASSES: usize = 169;

/// Grid rank (0 = A .. 12 = 2) to card rank (12 = A .. 0 = 2).
fn card_rank(grid: usize) -> u8 {
    12 - grid as u8
}

/// Class name such as "AA", "AKs", "72o".
pub fn class_name(i: usize) -> String {
    let (r, c) = (i / 13, i % 13);
    let ch = |g: usize| RANKS[card_rank(g) as usize] as char;
    match r.cmp(&c) {
        std::cmp::Ordering::Equal => format!("{}{}", ch(r), ch(c)),
        std::cmp::Ordering::Less => format!("{}{}s", ch(r), ch(c)),
        std::cmp::Ordering::Greater => format!("{}{}o", ch(c), ch(r)),
    }
}

/// All combos of a class.
pub fn combos(i: usize) -> Vec<(Card, Card)> {
    let (r, c) = (i / 13, i % 13);
    let (hi, lo) = (card_rank(r.min(c)), card_rank(r.max(c)));
    let mut v = Vec::new();
    for s1 in 0..4 {
        for s2 in 0..4 {
            let ok = match r.cmp(&c) {
                std::cmp::Ordering::Equal => s1 < s2,
                std::cmp::Ordering::Less => s1 == s2,
                std::cmp::Ordering::Greater => s1 != s2,
            };
            if ok {
                v.push((Card::new(hi, s1), Card::new(lo, s2)));
            }
        }
    }
    v
}

/// Number of combos: 6 for pairs, 4 suited, 12 offsuit.
pub fn combo_count(i: usize) -> usize {
    let (r, c) = (i / 13, i % 13);
    match r.cmp(&c) {
        std::cmp::Ordering::Equal => 6,
        std::cmp::Ordering::Less => 4,
        std::cmp::Ordering::Greater => 12,
    }
}

/// Class of two cards.
pub fn class_of(a: Card, b: Card) -> usize {
    let (hi, lo) = if a.rank() >= b.rank() { (a, b) } else { (b, a) };
    let (gh, gl) = ((12 - hi.rank()) as usize, (12 - lo.rank()) as usize);
    // Pairs and suited hands: row = higher rank; offsuit: row = lower rank.
    if gh == gl || hi.suit() == lo.suit() {
        gh * 13 + gl
    } else {
        gl * 13 + gh
    }
}

/// Range text from combo-weighted class reach, e.g. "AA,AKs:0.500": each class's weight is the
/// share of its combos still in the range; classes below 0.1 % are left out.
pub fn range_text(reach: &[f32]) -> String {
    (0..NUM_CLASSES)
        .filter_map(|i| {
            let w = (reach[i] / combo_count(i) as f32).min(1.0);
            if w < 0.001 {
                None
            } else if w > 0.999 {
                Some(class_name(i))
            } else {
                Some(format!("{}:{w:.3}", class_name(i)))
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Number of card-disjoint combo pairs between two classes, `[i][j]`.
pub fn compatible_pairs() -> Vec<f32> {
    let all: Vec<Vec<u64>> = (0..NUM_CLASSES)
        .map(|i| {
            combos(i)
                .iter()
                .map(|&(a, b)| 1u64 << a.index() | 1u64 << b.index())
                .collect()
        })
        .collect();
    let mut w = vec![0.0f32; NUM_CLASSES * NUM_CLASSES];
    for i in 0..NUM_CLASSES {
        for j in 0..NUM_CLASSES {
            w[i * NUM_CLASSES + j] = all[i]
                .iter()
                .map(|&x| all[j].iter().filter(|&&y| x & y == 0).count())
                .sum::<usize>() as f32;
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_cover_the_deck() {
        let total: usize = (0..NUM_CLASSES).map(combo_count).sum();
        assert_eq!(total, 1326);
        for i in 0..NUM_CLASSES {
            let c = combos(i);
            assert_eq!(c.len(), combo_count(i), "{}", class_name(i));
            for &(a, b) in &c {
                assert_eq!(class_of(a, b), i, "{}", class_name(i));
            }
        }
        assert_eq!(class_name(0), "AA");
        assert_eq!(class_name(1), "AKs");
        assert_eq!(class_name(13), "AKo");
        assert_eq!(class_name(168), "22");
        assert_eq!(class_name(12 * 13 + 11), "32o");
    }

    #[test]
    fn range_text_round_trips_through_the_parser() {
        let mut reach = vec![0.0f32; NUM_CLASSES];
        reach[0] = 6.0; // AA, all combos
        reach[1] = 2.0; // AKs, half
        let text = range_text(&reach);
        assert_eq!(text, "AA,AKs:0.500");
        let r: crate::range::Range = text.parse().unwrap();
        assert_eq!(r.combos(), 8.0);
    }

    #[test]
    fn compatible_pair_counts() {
        let w = compatible_pairs();
        let at = |a: usize, b: usize| w[a * NUM_CLASSES + b];
        assert_eq!(at(0, 0), 1.0 * 6.0); // AA vs AA: each AA leaves exactly one other AA
        assert_eq!(at(0, 168), 36.0); // AA vs 22: no shared cards
        assert_eq!(at(0, 1), 6.0 * 2.0); // AA vs AKs: each AA leaves the two AKs of the other aces
        let total: f32 = w.iter().sum();
        assert_eq!(total, 1326.0 * 1225.0);
    }
}
