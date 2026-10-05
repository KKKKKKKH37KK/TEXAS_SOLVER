//! Heads-up all-in equity between hand classes (PRD §4.2), estimated by Monte Carlo and shipped as
//! a table so neither the CLI nor the browser has to recompute it.
//!
//! `eq[i][j]` = P(class i wins) + P(tie) / 2, averaged uniformly over card-disjoint combo pairs and
//! boards. Generated with `hexas gen-equity`; the table is `data/preflop_equity.bin` (169 × 169
//! little-endian f32).

use super::classes::{NUM_CLASSES, combos};
use crate::cards::Card;
use crate::eval::evaluate;
use rayon::prelude::*;

const TABLE: &[u8] = include_bytes!("../../data/preflop_equity.bin");

/// The shipped equity table, `[i * 169 + j]`.
pub fn table() -> Vec<f32> {
    assert_eq!(
        TABLE.len(),
        NUM_CLASSES * NUM_CLASSES * 4,
        "equity table size"
    );
    TABLE
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

/// splitmix64, for reproducible tables.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Estimates the table with `samples` deals per class pair (seeded, so the result is
/// reproducible). Symmetric by construction: eq[j][i] = 1 − eq[i][j], eq[i][i] = 0.5.
pub fn generate(samples: usize, seed: u64) -> Vec<f32> {
    let classes: Vec<Vec<(Card, Card)>> = (0..NUM_CLASSES).map(combos).collect();
    let rows: Vec<Vec<f32>> = (0..NUM_CLASSES)
        .into_par_iter()
        .map(|i| {
            let mut rng = Rng(seed ^ (i as u64).wrapping_mul(0x2545_F491_4F6C_DD1D));
            let mut row = vec![0.5f32; NUM_CLASSES];
            for j in i + 1..NUM_CLASSES {
                let mut score = 0.0f64;
                let mut n = 0usize;
                while n < samples {
                    let (a1, a2) = classes[i][rng.below(classes[i].len())];
                    let (b1, b2) = classes[j][rng.below(classes[j].len())];
                    let dead = 1u64 << a1.index() | 1u64 << a2.index();
                    let dead_b = 1u64 << b1.index() | 1u64 << b2.index();
                    if dead & dead_b != 0 {
                        continue; // rejection keeps the pair distribution uniform over disjoint pairs
                    }
                    let mut used = dead | dead_b;
                    let mut board = [Card::from_index(0); 5];
                    for slot in &mut board {
                        loop {
                            let c = rng.below(52) as u8;
                            if used >> c & 1 == 0 {
                                used |= 1 << c;
                                *slot = Card::from_index(c);
                                break;
                            }
                        }
                    }
                    let mut x = [board[0], board[1], board[2], board[3], board[4], a1, a2];
                    let sa = evaluate(&x);
                    x[5] = b1;
                    x[6] = b2;
                    let sb = evaluate(&x);
                    score += if sa > sb {
                        1.0
                    } else if sa == sb {
                        0.5
                    } else {
                        0.0
                    };
                    n += 1;
                }
                row[j] = (score / n as f64) as f32;
            }
            row
        })
        .collect();
    let mut eq = vec![0.5f32; NUM_CLASSES * NUM_CLASSES];
    for i in 0..NUM_CLASSES {
        for j in i + 1..NUM_CLASSES {
            eq[i * NUM_CLASSES + j] = rows[i][j];
            eq[j * NUM_CLASSES + i] = 1.0 - rows[i][j];
        }
    }
    eq
}

/// Little-endian bytes of a table, for writing `data/preflop_equity.bin`.
pub fn to_bytes(eq: &[f32]) -> Vec<u8> {
    eq.iter().flat_map(|x| x.to_le_bytes()).collect()
}
