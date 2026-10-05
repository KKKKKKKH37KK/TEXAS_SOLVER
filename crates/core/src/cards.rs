//! Card representation: one byte per card, `rank * 4 + suit`, so 0..52.
//! Rank 0 = deuce .. 12 = ace; suit order is c, d, h, s.

use std::fmt;
use std::str::FromStr;

pub const RANKS: &[u8; 13] = b"23456789TJQKA";
pub const SUITS: &[u8; 4] = b"cdhs";

/// Number of distinct two-card starting hands.
pub const NUM_COMBOS: usize = 1326;

/// Index of the two-card combo {a, b} in 0..1326 (order of the arguments does not matter).
pub fn combo_index(a: Card, b: Card) -> usize {
    let (lo, hi) = if a.0 < b.0 { (a.0, b.0) } else { (b.0, a.0) };
    assert!(lo != hi, "a combo needs two different cards");
    hi as usize * (hi as usize - 1) / 2 + lo as usize
}

/// Every combo in `combo_index` order.
pub fn all_combos() -> Vec<(Card, Card)> {
    let mut v = Vec::with_capacity(NUM_COMBOS);
    for hi in 1..Card::COUNT as u8 {
        for lo in 0..hi {
            v.push((Card(hi), Card(lo)));
        }
    }
    v
}

/// Bit mask with bit `card.index()` set for every card.
pub fn mask_of(cards: &[Card]) -> u64 {
    cards.iter().fold(0, |m, c| m | 1u64 << c.0)
}

/// Starting-hand class such as "AA", "AKs", "T9o" (higher rank first).
pub fn hand_class(a: Card, b: Card) -> String {
    let (hi, lo) = if a.rank() >= b.rank() { (a, b) } else { (b, a) };
    let r = |c: Card| RANKS[c.rank() as usize] as char;
    if hi.rank() == lo.rank() {
        format!("{}{}", r(hi), r(lo))
    } else {
        let s = if hi.suit() == lo.suit() { 's' } else { 'o' };
        format!("{}{}{s}", r(hi), r(lo))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Card(u8);

impl Card {
    pub const COUNT: usize = 52;

    /// `rank` in 0..13 (2..A), `suit` in 0..4 (c, d, h, s).
    pub fn new(rank: u8, suit: u8) -> Self {
        assert!(
            rank < 13 && suit < 4,
            "card out of range: rank {rank}, suit {suit}"
        );
        Card(rank * 4 + suit)
    }

    pub fn from_index(i: u8) -> Self {
        assert!((i as usize) < Self::COUNT, "card index out of range: {i}");
        Card(i)
    }

    pub fn index(self) -> u8 {
        self.0
    }

    pub fn rank(self) -> u8 {
        self.0 / 4
    }

    pub fn suit(self) -> u8 {
        self.0 % 4
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            RANKS[self.rank() as usize] as char,
            SUITS[self.suit() as usize] as char
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseCardError(String);

impl fmt::Display for ParseCardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid card: {:?}", self.0)
    }
}

impl std::error::Error for ParseCardError {}

impl FromStr for Card {
    type Err = ParseCardError;

    /// Parses "As", "td", "9C" (rank case-insensitive, suit case-insensitive).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseCardError(s.to_string());
        let b = s.as_bytes();
        if b.len() != 2 {
            return Err(err());
        }
        let rank = RANKS
            .iter()
            .position(|&r| r == b[0].to_ascii_uppercase())
            .ok_or_else(err)?;
        let suit = SUITS
            .iter()
            .position(|&x| x == b[1].to_ascii_lowercase())
            .ok_or_else(err)?;
        Ok(Card::new(rank as u8, suit as u8))
    }
}

/// Parses a run of cards without separators, e.g. a board "AsKd7c".
pub fn parse_cards(s: &str) -> Result<Vec<Card>, ParseCardError> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return Err(ParseCardError(s.to_string()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| s[i..i + 2].parse())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_card() {
        for i in 0..Card::COUNT as u8 {
            let c = Card::from_index(i);
            assert_eq!(c.to_string().parse::<Card>().unwrap(), c);
        }
    }

    #[test]
    fn rank_and_suit_layout() {
        let c: Card = "As".parse().unwrap();
        assert_eq!((c.rank(), c.suit(), c.index()), (12, 3, 51));
        let c: Card = "2c".parse().unwrap();
        assert_eq!(c.index(), 0);
        assert_eq!("tD".parse::<Card>().unwrap().to_string(), "Td");
    }

    #[test]
    fn parses_boards_and_rejects_garbage() {
        let b = parse_cards("AsKd7c").unwrap();
        assert_eq!(
            b.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
            ["As", "Kd", "7c"]
        );
        assert!(parse_cards("AsK").is_err());
        assert!("1s".parse::<Card>().is_err());
        assert!("Ax".parse::<Card>().is_err());
    }

    #[test]
    fn combo_index_is_a_bijection() {
        let combos = all_combos();
        assert_eq!(combos.len(), NUM_COMBOS);
        for (i, &(a, b)) in combos.iter().enumerate() {
            assert_eq!(combo_index(a, b), i);
            assert_eq!(combo_index(b, a), i);
        }
    }

    #[test]
    fn hand_classes() {
        let c = |s: &str| s.parse::<Card>().unwrap();
        assert_eq!(hand_class(c("Kd"), c("As")), "AKo");
        assert_eq!(hand_class(c("9h"), c("Th")), "T9s");
        assert_eq!(hand_class(c("2c"), c("2d")), "22");
        assert_eq!(mask_of(&[c("2c"), c("As")]), 1 | 1 << 51);
    }
}
