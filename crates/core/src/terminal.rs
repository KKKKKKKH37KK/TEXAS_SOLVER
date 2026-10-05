//! Counterfactual values at terminal nodes, vectorised over the traverser's hands (PRD §4.1).
//!
//! For traverser hand h the opponent reach that is compatible with h (shares no card) is
//!   total − Σ_{c∈h} cardSum[c] + reach[identical hand],
//! and at a showdown the same correction is applied to running sums over hands sorted by strength, so a
//! terminal costs O(n + m) instead of O(n·m). Sums are accumulated in f64 to avoid cancellation.

use crate::game::Game;

const NONE: u8 = u8::MAX;

/// A hand in strength order, with what the sweep needs stored inline.
#[derive(Clone, Copy)]
struct Entry {
    strength: u32,
    idx: u32,
    cards: [u8; 2],
}

pub struct Terminals {
    /// Private cards per player per hand (second card NONE for one-card games).
    cards: [Vec<[u8; 2]>; 2],
    /// Index of the opponent hand with the same two cards, or u32::MAX.
    same: [Vec<u32>; 2],
    /// Per board, per player: hands that do not touch the board, by ascending strength.
    sorted: Vec<[Vec<Entry>; 2]>,
}

fn cards_of(mask: u64) -> [u8; 2] {
    assert!(
        (1..=2).contains(&mask.count_ones()),
        "hands must hold one or two cards"
    );
    let a = mask.trailing_zeros() as u8;
    let rest = mask & !(1 << a);
    [
        a,
        if rest == 0 {
            NONE
        } else {
            rest.trailing_zeros() as u8
        },
    ]
}

impl Terminals {
    pub fn new(game: &Game) -> Self {
        let cards = [0, 1].map(|p| {
            game.hands[p]
                .iter()
                .map(|&m| cards_of(m))
                .collect::<Vec<_>>()
        });
        let same = [0, 1].map(|p| {
            let o = 1 - p;
            game.hands[p]
                .iter()
                .map(|&m| {
                    if m.count_ones() < 2 {
                        return u32::MAX;
                    }
                    game.hands[o]
                        .iter()
                        .position(|&x| x == m)
                        .map_or(u32::MAX, |i| i as u32)
                })
                .collect()
        });
        // Hands that share a card with the board never reach its showdowns (zero reach), so they
        // are left out of the sweeps.
        let sorted = game
            .strengths
            .iter()
            .zip(&game.board_masks)
            .map(|(s, &board)| {
                [0, 1].map(|p| {
                    let mut v: Vec<Entry> = (0..game.hands[p].len())
                        .filter(|&h| game.hands[p][h] & board == 0)
                        .map(|h| Entry {
                            strength: s[p][h],
                            idx: h as u32,
                            cards: cards[p][h],
                        })
                        .collect();
                    v.sort_by_key(|e| e.strength);
                    v
                })
            })
            .collect();
        Terminals {
            cards,
            same,
            sorted,
        }
    }

    fn removal(&self, p: usize, h: usize, card_sum: &[f64; 64], reach_o: &[f32]) -> f64 {
        let [a, b] = self.cards[p][h];
        let mut x = card_sum[a as usize];
        if b != NONE {
            x += card_sum[b as usize];
            let s = self.same[p][h];
            if s != u32::MAX {
                x -= reach_o[s as usize] as f64;
            }
        }
        x
    }

    fn card_sums(&self, o: usize, reach_o: &[f32]) -> (f64, [f64; 64]) {
        let mut total = 0.0;
        let mut cs = [0.0f64; 64];
        for (h, &r) in reach_o.iter().enumerate() {
            if r == 0.0 {
                continue;
            }
            let r = r as f64;
            total += r;
            for c in self.cards[o][h] {
                if c != NONE {
                    cs[c as usize] += r;
                }
            }
        }
        (total, cs)
    }

    /// Opponent reach compatible with each traverser hand.
    pub fn compatible(&self, p: usize, reach_o: &[f32]) -> Vec<f64> {
        let (total, cs) = self.card_sums(1 - p, reach_o);
        (0..self.cards[p].len())
            .map(|h| total - self.removal(p, h, &cs, reach_o))
            .collect()
    }

    /// Every traverser hand gets `payoff` × compatible opponent reach.
    pub fn fold(&self, p: usize, reach_o: &[f32], payoff: f64) -> Vec<f32> {
        let (total, cs) = self.card_sums(1 - p, reach_o);
        (0..self.cards[p].len())
            .map(|h| ((total - self.removal(p, h, &cs, reach_o)) * payoff) as f32)
            .collect()
    }

    /// Showdown values with the given payoffs for a win, a loss and a tie.
    ///
    /// cfv = win·u_w + loss·u_l + tie·u_t with tie = compat − win − loss, written as
    /// win·(u_w − u_t) + loss·(u_l − u_t) + compat·u_t so each sweep adds its part directly.
    pub fn showdown(
        &self,
        p: usize,
        board: usize,
        reach_o: &[f32],
        [win, lose, tie]: [f64; 3],
    ) -> Vec<f32> {
        let [sp, so] = if p == 0 {
            [&self.sorted[board][0], &self.sorted[board][1]]
        } else {
            [&self.sorted[board][1], &self.sorted[board][0]]
        };
        let mut out = vec![0.0f32; self.cards[p].len()];
        let pair = |cs: &[f64; 64], [a, b]: [u8; 2]| {
            cs[a as usize] + if b != NONE { cs[b as usize] } else { 0.0 }
        };
        let add = |cs: &mut [f64; 64], [a, b]: [u8; 2], r: f64| {
            cs[a as usize] += r;
            if b != NONE {
                cs[b as usize] += r;
            }
        };

        // Totals over every opponent hand that can reach this board.
        let (mut total, mut cs_all) = (0.0f64, [0.0f64; 64]);
        for e in so {
            let r = reach_o[e.idx as usize] as f64;
            total += r;
            add(&mut cs_all, e.cards, r);
        }
        if total == 0.0 {
            return out;
        }
        let same = &self.same[p];

        // Ascending: opponent hands strictly weaker than the traverser hand. An identical opponent
        // hand has equal strength, so it never enters these sums.
        let (mut cum, mut cs, mut j) = (0.0f64, [0.0f64; 64], 0);
        for e in sp {
            while j < so.len() && so[j].strength < e.strength {
                let r = reach_o[so[j].idx as usize] as f64;
                cum += r;
                add(&mut cs, so[j].cards, r);
                j += 1;
            }
            let h = e.idx as usize;
            let mut compat = total - pair(&cs_all, e.cards);
            if same[h] != u32::MAX {
                compat += reach_o[same[h] as usize] as f64;
            }
            let wins = cum - pair(&cs, e.cards);
            out[h] = (wins * (win - tie) + compat * tie) as f32;
        }

        // Descending: opponent hands strictly stronger.
        let (mut cum, mut cs, mut j) = (0.0f64, [0.0f64; 64], so.len());
        for e in sp.iter().rev() {
            while j > 0 && so[j - 1].strength > e.strength {
                let r = reach_o[so[j - 1].idx as usize] as f64;
                cum += r;
                add(&mut cs, so[j - 1].cards, r);
                j -= 1;
            }
            let losses = cum - pair(&cs, e.cards);
            out[e.idx as usize] += (losses * (lose - tie)) as f32;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::{Card, all_combos, mask_of, parse_cards};
    use crate::eval::evaluate;

    /// Simple deterministic PRNG for tests (xorshift64*).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545F4914F6CDD1D)
        }
        fn unit(&mut self) -> f32 {
            (self.next() >> 40) as f32 / (1u64 << 24) as f32
        }
    }

    /// A river game with random subsets of combos and random reach.
    fn random_river(seed: u64) -> (Game, [Vec<f32>; 2]) {
        let mut rng = Rng(seed);
        let board = parse_cards("Kd8s5h2c2d").unwrap();
        let dead = mask_of(&board);
        let live: Vec<(Card, Card)> = all_combos()
            .into_iter()
            .filter(|&(a, b)| dead & mask_of(&[a, b]) == 0)
            .collect();
        let pick = |rng: &mut Rng| -> Vec<(Card, Card)> {
            live.iter().copied().filter(|_| rng.unit() < 0.3).collect()
        };
        let hands = [pick(&mut rng), pick(&mut rng)];
        let strengths = [0, 1].map(|p| {
            hands[p]
                .iter()
                .map(|&(a, b)| {
                    let mut c = board.clone();
                    c.extend([a, b]);
                    evaluate(&c)
                })
                .collect::<Vec<_>>()
        });
        let masks = [0, 1].map(|p| {
            hands[p]
                .iter()
                .map(|&(a, b)| mask_of(&[a, b]))
                .collect::<Vec<_>>()
        });
        let reach = [0, 1].map(|p| {
            hands[p]
                .iter()
                .map(|_| if rng.unit() < 0.2 { 0.0 } else { rng.unit() })
                .collect::<Vec<_>>()
        });
        let game = Game {
            nodes: vec![],
            weights: [vec![1.0; masks[0].len()], vec![1.0; masks[1].len()]],
            hands: masks,
            strengths: vec![strengths],
            board_masks: vec![dead],
            swaps: vec![],
            start_pot: 1.0,
        };
        (game, reach)
    }

    #[test]
    fn fast_terminals_match_brute_force() {
        for seed in 1..6u64 {
            let (game, reach) = random_river(seed * 7919);
            let t = Terminals::new(&game);
            for p in 0..2 {
                let o = 1 - p;
                let payoffs = [3.0, -2.0, 0.5];
                let fast_sd = t.showdown(p, 0, &reach[o], payoffs);
                let fast_fold = t.fold(p, &reach[o], -1.5);
                for h in 0..game.hands[p].len() {
                    let (mut sd, mut fold) = (0.0f64, 0.0f64);
                    for (k, &opp) in game.hands[o].iter().enumerate() {
                        if game.hands[p][h] & opp != 0 {
                            continue;
                        }
                        let r = reach[o][k] as f64;
                        let (a, b) = (game.strengths[0][p][h], game.strengths[0][o][k]);
                        sd += r * if a > b {
                            payoffs[0]
                        } else if a < b {
                            payoffs[1]
                        } else {
                            payoffs[2]
                        };
                        fold += r * -1.5;
                    }
                    assert!(
                        (fast_sd[h] as f64 - sd).abs() < 1e-3 * (1.0 + sd.abs()),
                        "showdown h={h}"
                    );
                    assert!(
                        (fast_fold[h] as f64 - fold).abs() < 1e-3 * (1.0 + fold.abs()),
                        "fold h={h}"
                    );
                }
            }
        }
    }
}
