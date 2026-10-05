//! Best-five-card hand evaluator for 5–7 cards.
//!
//! The result is a `u32` where a larger value is a stronger hand: the category in bits 20..24 and up to
//! five 4-bit tie-break ranks below it (most significant first). No heap allocation: the exhaustive test
//! calls this 133M times.

use crate::cards::Card;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Category {
    HighCard = 0,
    OnePair = 1,
    TwoPair = 2,
    Trips = 3,
    Straight = 4,
    Flush = 5,
    FullHouse = 6,
    Quads = 7,
    StraightFlush = 8,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::HighCard,
        Category::OnePair,
        Category::TwoPair,
        Category::Trips,
        Category::Straight,
        Category::Flush,
        Category::FullHouse,
        Category::Quads,
        Category::StraightFlush,
    ];

    pub fn of(strength: u32) -> Category {
        Self::ALL[(strength >> 20) as usize]
    }
}

/// Builds a strength value: category plus tie-break ranks appended with `push`.
struct Packer {
    v: u32,
    n: u32,
}

impl Packer {
    fn new(cat: Category) -> Self {
        Packer {
            v: (cat as u32) << 20,
            n: 0,
        }
    }

    fn push(mut self, r: u8) -> Self {
        self.v |= (r as u32) << (16 - 4 * self.n);
        self.n += 1;
        self
    }

    /// Appends the top `k` ranks of `mask`, highest first.
    fn top(mut self, mut mask: u16, k: u32) -> Self {
        for _ in 0..k {
            if mask == 0 {
                break;
            }
            let r = 15 - mask.leading_zeros() as u8;
            mask &= !(1 << r);
            self = self.push(r);
        }
        self
    }
}

/// Highest straight in a 13-bit rank mask (bit 0 = deuce), as the rank of its top card. Handles the wheel.
fn straight_high(mask: u16) -> Option<u8> {
    // Shift up one and put the ace in bit 0 so A-2-3-4-5 is five consecutive bits.
    let m = ((mask as u32) << 1) | ((mask as u32 >> 12) & 1);
    let s = m & (m << 1) & (m << 2) & (m << 3) & (m << 4);
    if s == 0 {
        None
    } else {
        Some((31 - s.leading_zeros()) as u8 - 1)
    }
}

/// Strength of the best five-card hand in `cards` (5 to 7 cards, all distinct).
pub fn evaluate(cards: &[Card]) -> u32 {
    debug_assert!((5..=7).contains(&cards.len()));
    let mut suit_mask = [0u16; 4];
    let mut count = [0u8; 13];
    for c in cards {
        suit_mask[c.suit() as usize] |= 1 << c.rank();
        count[c.rank() as usize] += 1;
    }

    if let Some(&sm) = suit_mask.iter().find(|m| m.count_ones() >= 5) {
        if let Some(h) = straight_high(sm) {
            return Packer::new(Category::StraightFlush).push(h).v;
        }
        return Packer::new(Category::Flush).top(sm, 5).v;
    }

    let all = suit_mask[0] | suit_mask[1] | suit_mask[2] | suit_mask[3];
    // Rank masks by multiplicity.
    let (mut m4, mut m3, mut m2) = (0u16, 0u16, 0u16);
    for (r, &n) in count.iter().enumerate() {
        match n {
            4 => m4 |= 1 << r,
            3 => m3 |= 1 << r,
            2 => m2 |= 1 << r,
            _ => {}
        }
    }
    let high = |m: u16| 15 - m.leading_zeros() as u8;

    if m4 != 0 {
        let q = high(m4);
        return Packer::new(Category::Quads)
            .push(q)
            .top(all & !(1 << q), 1)
            .v;
    }
    if m3 != 0 {
        let t = high(m3);
        // A second set of trips also fills the pair slot.
        let pair_mask = (m3 & !(1 << t)) | m2;
        if pair_mask != 0 {
            return Packer::new(Category::FullHouse)
                .push(t)
                .push(high(pair_mask))
                .v;
        }
    }
    if let Some(h) = straight_high(all) {
        return Packer::new(Category::Straight).push(h).v;
    }
    if m3 != 0 {
        let t = high(m3);
        return Packer::new(Category::Trips)
            .push(t)
            .top(all & !(1 << t), 2)
            .v;
    }
    if m2.count_ones() >= 2 {
        let p1 = high(m2);
        let p2 = high(m2 & !(1 << p1));
        let rest = all & !(1 << p1) & !(1 << p2);
        return Packer::new(Category::TwoPair)
            .push(p1)
            .push(p2)
            .top(rest, 1)
            .v;
    }
    if m2 != 0 {
        let p = high(m2);
        return Packer::new(Category::OnePair)
            .push(p)
            .top(all & !(1 << p), 3)
            .v;
    }
    Packer::new(Category::HighCard).top(all, 5).v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::parse_cards;

    fn ev(s: &str) -> u32 {
        evaluate(&parse_cards(s).unwrap())
    }

    #[test]
    fn categories() {
        assert_eq!(Category::of(ev("AsKsQsJsTs9d2c")), Category::StraightFlush);
        assert_eq!(Category::of(ev("As2s3s4s5s9d9c")), Category::StraightFlush);
        assert_eq!(Category::of(ev("9s9d9h9cAs2d3c")), Category::Quads);
        assert_eq!(Category::of(ev("9s9d9hAcAsAd3c")), Category::FullHouse);
        assert_eq!(Category::of(ev("2s7s9sJsKs2d2c")), Category::Flush);
        assert_eq!(Category::of(ev("Ah2d3c4s5h9dKc")), Category::Straight);
        assert_eq!(Category::of(ev("7h7d7cAsKd2c3h")), Category::Trips);
        assert_eq!(Category::of(ev("7h7dKcKs2d2c3h")), Category::TwoPair);
        assert_eq!(Category::of(ev("7h7dKc9s2d4c3h")), Category::OnePair);
        assert_eq!(Category::of(ev("7h8dKc9s2d4c3h")), Category::HighCard);
    }

    #[test]
    fn ordering_and_kickers() {
        // Wheel is the lowest straight; six-high beats it.
        assert!(ev("Ah2d3c4s5h") < ev("2d3c4s5h6d"));
        // Third pair can be the two-pair kicker.
        assert_eq!(ev("KhKdQcQs2d2cAh"), ev("KhKdQcQsAh"));
        assert!(ev("KhKdQcQs3d3c4h") > ev("KhKdQcQs2d2c3h"));
        // Board plays: identical strength.
        assert_eq!(ev("AsKdQhJcTd2c3h"), ev("AsKdQhJcTd4c5h"));
        // Kicker decides.
        assert!(ev("AhAdKc7s2d") > ev("AhAdQc7s2d"));
        // Two trips make the best full house.
        assert_eq!(ev("9s9d9hAcAsAd3c"), ev("AcAsAd9s9d"));
        // Quads kicker can come from a pair.
        assert_eq!(ev("9s9d9h9cKsKd2c"), ev("9s9d9h9cKs"));
        // Flush beats straight on the same 7 cards.
        assert_eq!(Category::of(ev("5h6h7h8d9h2hKc")), Category::Flush);
    }
}
