//! Hand ranges in PioSolver-style text: `AA,KK:0.5,QQ-99,ATs+,A5s-A2s,KQo,KQ,AhKh`.
//!
//! A range is a weight in [0, 1] for each of the 1326 combos. Later tokens override earlier ones.

use crate::cards::{Card, NUM_COMBOS, RANKS, all_combos, combo_index};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq)]
pub struct Range {
    /// Weight per combo, indexed by `combo_index`.
    pub weights: Vec<f32>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseRangeError(pub String);

impl fmt::Display for ParseRangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid range: {}", self.0)
    }
}

impl std::error::Error for ParseRangeError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Suitedness {
    Suited,
    Offsuit,
    Any,
}

impl Range {
    pub fn empty() -> Self {
        Range {
            weights: vec![0.0; NUM_COMBOS],
        }
    }

    /// Number of combos weighted by their frequency.
    pub fn combos(&self) -> f32 {
        self.weights.iter().sum()
    }

    fn set_class(&mut self, hi: u8, lo: u8, s: Suitedness, w: f32) {
        for s1 in 0..4 {
            for s2 in 0..4 {
                let (a, b) = (Card::new(hi, s1), Card::new(lo, s2));
                if a == b || (hi == lo && s1 >= s2) {
                    continue;
                }
                let ok = match s {
                    Suitedness::Suited => s1 == s2,
                    Suitedness::Offsuit => s1 != s2,
                    Suitedness::Any => true,
                };
                if ok {
                    self.weights[combo_index(a, b)] = w;
                }
            }
        }
    }
}

fn rank_of(ch: char) -> Option<u8> {
    let up = ch.to_ascii_uppercase() as u8;
    RANKS.iter().position(|&r| r == up).map(|i| i as u8)
}

/// "AKs" → (12, 11, Suited); "KA" → (12, 11, Any); "77" → (5, 5, Any).
fn parse_class(s: &str) -> Option<(u8, u8, Suitedness)> {
    let ch: Vec<char> = s.chars().collect();
    if ch.len() < 2 || ch.len() > 3 {
        return None;
    }
    let (a, b) = (rank_of(ch[0])?, rank_of(ch[1])?);
    let suit = match ch.get(2).map(|c| c.to_ascii_lowercase()) {
        None => Suitedness::Any,
        Some('s') => Suitedness::Suited,
        Some('o') => Suitedness::Offsuit,
        _ => return None,
    };
    if a == b && suit != Suitedness::Any {
        return None;
    }
    Some((a.max(b), a.min(b), suit))
}

impl FromStr for Range {
    type Err = ParseRangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut r = Range::empty();
        for raw in s.split(',') {
            let tok = raw.trim();
            if tok.is_empty() {
                continue;
            }
            let err = || ParseRangeError(format!("{tok:?}"));
            let (body, w) = match tok.split_once(':') {
                Some((b, w)) => {
                    let w: f32 = w.trim().parse().map_err(|_| err())?;
                    if !(0.0..=1.0).contains(&w) {
                        return Err(err());
                    }
                    (b.trim(), w)
                }
                None => (tok, 1.0),
            };

            // A specific combo such as "AhKh".
            if body.len() == 4
                && let Ok(cards) = crate::cards::parse_cards(body)
            {
                if cards[0] == cards[1] {
                    return Err(err());
                }
                r.weights[combo_index(cards[0], cards[1])] = w;
                continue;
            }

            if let Some((lo_s, hi_s)) = body.split_once('-') {
                // "TT-77" or "A5s-A2s": same shape, the varying rank spans both ends.
                let (a1, b1, s1) = parse_class(lo_s.trim()).ok_or_else(err)?;
                let (a2, b2, s2) = parse_class(hi_s.trim()).ok_or_else(err)?;
                if s1 != s2 {
                    return Err(err());
                }
                if a1 == b1 && a2 == b2 {
                    for p in a1.min(a2)..=a1.max(a2) {
                        r.set_class(p, p, s1, w);
                    }
                } else if a1 == a2 && a1 != b1 && a2 != b2 {
                    for k in b1.min(b2)..=b1.max(b2) {
                        r.set_class(a1, k, s1, w);
                    }
                } else {
                    return Err(err());
                }
            } else if let Some(base) = body.strip_suffix('+') {
                // "QQ+" = QQ..AA; "ATs+" = ATs..AKs.
                let (a, b, s) = parse_class(base).ok_or_else(err)?;
                if a == b {
                    for p in a..13 {
                        r.set_class(p, p, s, w);
                    }
                } else {
                    for k in b..a {
                        r.set_class(a, k, s, w);
                    }
                }
            } else {
                let (a, b, s) = parse_class(body).ok_or_else(err)?;
                r.set_class(a, b, s, w);
            }
        }
        Ok(r)
    }
}

/// Every combo with a positive weight that does not touch `dead` (a card mask).
pub fn live_combos(range: &Range, dead: u64) -> Vec<((Card, Card), f32)> {
    all_combos()
        .into_iter()
        .zip(range.weights.iter().copied())
        .filter(|&((a, b), w)| w > 0.0 && dead & (1 << a.index() | 1 << b.index()) == 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> f32 {
        s.parse::<Range>().unwrap().combos()
    }

    #[test]
    fn combo_counts() {
        assert_eq!(n("AA"), 6.0);
        assert_eq!(n("AKs"), 4.0);
        assert_eq!(n("AKo"), 12.0);
        assert_eq!(n("AK"), 16.0);
        assert_eq!(n("KA"), 16.0);
        assert_eq!(n("QQ+"), 18.0);
        assert_eq!(n("22+"), 78.0);
        assert_eq!(n("ATs+"), 16.0);
        assert_eq!(n("A5s-A2s"), 16.0);
        assert_eq!(n("TT-77"), 24.0);
        assert_eq!(n("AhKh"), 1.0);
        assert_eq!(n("AA:0.5, KK"), 9.0);
        assert_eq!(n("AA,AA:0.25"), 1.5);
        assert_eq!(n("K2o+"), 11.0 * 12.0);
    }

    #[test]
    fn whole_deck() {
        let all: Vec<String> = (0..13u8)
            .rev()
            .flat_map(|a| (0..=a).map(move |b| (a, b)))
            .map(|(a, b)| format!("{}{}", RANKS[a as usize] as char, RANKS[b as usize] as char))
            .collect();
        assert_eq!(n(&all.join(",")), NUM_COMBOS as f32);
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["AAs", "AK+x", "XX", "AKs-QJs", "AA:2", "AhAh", "AKs-A2o"] {
            assert!(bad.parse::<Range>().is_err(), "{bad} should fail");
        }
    }

    #[test]
    fn live_combos_drop_board_cards() {
        let r: Range = "AA".parse().unwrap();
        let board = crate::cards::mask_of(&crate::cards::parse_cards("AsKd7c").unwrap());
        assert_eq!(live_combos(&r, board).len(), 3);
    }
}
