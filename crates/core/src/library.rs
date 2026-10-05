//! Pre-solved flop library (PRD §9.2): heads-up preflop lines, the 1,755 strategically distinct
//! flops, the tree each flop is solved with, and the calibration of the preflop realisation table.

use crate::cards::{Card, all_combos};
use crate::holdem::{BetSizes, Rake, Spot, TreeConfig};
use crate::preflop::classes::{NUM_CLASSES, class_of, combo_count, compatible_pairs, range_text};
use crate::preflop::equity;
use crate::preflop::game::{
    PNode, PotType, PreflopConfig, PreflopGame, PreflopSolver, Realization,
};
use crate::range::Range;
use crate::solver::{DcfrParams, Solver};

/// A heads-up preflop line that ends in a called raise.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub id: &'static str,
    /// Action words from UTG, as accepted by `PreflopSolver::line_path`.
    pub actions: &'static [&'static str],
    pub pot: PotType,
    pub aggressor_ip: bool,
}

/// The library lines: together they cover the three calibrated realisation situations.
pub const LINES: [Line; 3] = [
    // BTN opens, BB calls: SRP, aggressor in position.
    Line {
        id: "srp-btn-bb",
        actions: &["fold", "fold", "fold", "raise", "fold", "call"],
        pot: PotType::Srp,
        aggressor_ip: true,
    },
    // BTN opens, BB 3-bets, BTN calls: 3-bet pot, aggressor out of position.
    Line {
        id: "3bp-bb-btn",
        actions: &["fold", "fold", "fold", "raise", "fold", "raise", "call"],
        pot: PotType::ThreeBet,
        aggressor_ip: false,
    },
    // CO opens, BTN 3-bets, the blinds fold, CO calls: 3-bet pot, aggressor in position.
    Line {
        id: "3bp-co-btn",
        actions: &["fold", "fold", "raise", "raise", "fold", "fold", "call"],
        pot: PotType::ThreeBet,
        aggressor_ip: true,
    },
];

pub fn line(id: &str) -> Option<&'static Line> {
    LINES.iter().find(|l| l.id == id)
}

/// The two ranges, pot and stack at the end of a line.
#[derive(Clone, Debug)]
pub struct LineSpot {
    /// Combo-weighted class reach of the out-of-position / in-position player.
    pub reach: [Vec<f32>; 2],
    pub ranges: [String; 2],
    pub pot: f64,
    pub stack: f64,
    pub oop_is_aggressor: bool,
}

pub fn line_spot(solver: &PreflopSolver, line: &Line) -> Result<LineSpot, String> {
    let path = solver.line_path(line.actions)?;
    let (node, reach) = solver.walk(&path)?;
    let PNode::Flop {
        players,
        contrib,
        aggressor,
        ..
    } = &solver.game.nodes[node]
    else {
        return Err(format!("line {} does not end in a flop", line.id));
    };
    let [oop, ip] = *players;
    Ok(LineSpot {
        reach: [reach[oop].clone(), reach[ip].clone()],
        ranges: [range_text(&reach[oop]), range_text(&reach[ip])],
        pot: contrib.iter().sum(),
        stack: solver.game.config.stack - contrib[oop],
        oop_is_aggressor: *aggressor == oop,
    })
}

/// Tree for library flops: the PRD flop preset (33/66/100/125, one raise) with one 66 % size on
/// the turn and river so the widest single-raised pots fit in ~8 GB. Out of position may lead on
/// the flop only as the preflop aggressor (a c-bet, not a donk bet).
pub fn tree_config(spot: &LineSpot, rake: Rake) -> TreeConfig {
    let mut cfg = TreeConfig::flop_default(spot.pot, spot.stack);
    let later = BetSizes {
        bets: vec![0.66],
        ..cfg.sizes[1].clone()
    };
    cfg.sizes[1] = later.clone();
    cfg.sizes[2] = later;
    cfg.oop_flop_bets = spot.oop_is_aggressor;
    cfg.rake = rake;
    cfg
}

pub fn flop_spot(spot: &LineSpot, flop: [Card; 3], rake: Rake) -> Result<Spot, String> {
    let parse = |s: &str| s.parse::<Range>().map_err(|e| e.to_string());
    Ok(Spot {
        board: flop.to_vec(),
        ranges: [parse(&spot.ranges[0])?, parse(&spot.ranges[1])?],
        config: tree_config(spot, rake),
    })
}

/// All 24 permutations of the four suits.
fn suit_perms() -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for a in 0..4u8 {
        for b in 0..4u8 {
            for c in 0..4u8 {
                for d in 0..4u8 {
                    let p = [a, b, c, d];
                    let mut seen = [false; 4];
                    if p.iter()
                        .all(|&s| !std::mem::replace(&mut seen[s as usize], true))
                    {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

fn sorted_desc(mut c: [Card; 3]) -> [Card; 3] {
    c.sort_by(|a, b| b.cmp(a));
    c
}

/// The representative of a flop's suit-isomorphism class and the permutation (real suit →
/// representative suit) that maps the flop onto it. The representative is the lexicographically
/// largest image, so a monotone flop is in spades.
pub fn canonical_flop(flop: [Card; 3]) -> ([Card; 3], [u8; 4]) {
    let mut best: Option<([Card; 3], [u8; 4])> = None;
    for p in suit_perms() {
        let image = sorted_desc(flop.map(|c| Card::new(c.rank(), p[c.suit() as usize])));
        if best.is_none_or(|(b, _)| image > b) {
            best = Some((image, p));
        }
    }
    best.unwrap()
}

/// The 1,755 representative flops, in a fixed shuffled order so a partly built library already
/// covers every texture.
pub fn all_flops() -> Vec<[Card; 3]> {
    let mut set = std::collections::BTreeSet::new();
    for a in 0..52u8 {
        for b in a + 1..52 {
            for c in b + 1..52 {
                let f = [a, b, c].map(Card::from_index);
                set.insert(canonical_flop(f).0);
            }
        }
    }
    let mut v: Vec<[Card; 3]> = set.into_iter().collect();
    let mut rng = 0x5EED_F10Au64;
    for i in (1..v.len()).rev() {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        v.swap(i, (rng % (i as u64 + 1)) as usize);
    }
    v
}

pub fn flop_name(f: &[Card; 3]) -> String {
    f.iter().map(|c| c.to_string()).collect()
}

/// Per-class weights of a range (combo weights summed by class).
pub fn class_weights(r: &Range) -> Vec<f32> {
    let mut w = vec![0.0f32; NUM_CLASSES];
    for ((a, b), x) in all_combos().into_iter().zip(&r.weights) {
        w[class_of(a, b)] += *x;
    }
    w
}

/// The realisation ratio k (R_IP / R_OOP) for which the preflop share model gives the in-position
/// player `target` of the pot against these ranges (class reach, OOP then IP).
pub fn fit_k(reach: &[Vec<f32>; 2], target: f64) -> f64 {
    let eq = equity::table();
    let comp = compatible_pairs();
    let share = |k: f64| {
        let (mut num, mut den) = (0.0, 0.0);
        for c in 0..NUM_CLASSES {
            for d in 0..NUM_CLASSES {
                let w = reach[1][c] as f64 * reach[0][d] as f64 * comp[c * NUM_CLASSES + d] as f64
                    / (combo_count(c) * combo_count(d)) as f64;
                if w == 0.0 {
                    continue;
                }
                let e = eq[c * NUM_CLASSES + d] as f64; // in-position class c vs class d
                num += w * e * k / (e * k + (1.0 - e));
                den += w;
            }
        }
        num / den
    };
    let (mut lo, mut hi) = (0.2, 5.0);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if share(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

/// Solves a spot until the exploitability is below `target_pct` % of the pot (checked every 10
/// iterations) or `max_iters`. Returns the solver's report.
pub fn solve_to(game: &crate::game::Game, target_pct: f64, max_iters: u32) -> (Solver<'_>, f64) {
    let mut s = Solver::new(game, DcfrParams::default());
    let mut expl = f64::INFINITY;
    while s.iterations() < max_iters {
        s.iterate();
        if s.iterations().is_multiple_of(10) {
            expl = s.report().exploitability_pct;
            if expl < target_pct {
                break;
            }
        }
    }
    (s, expl)
}

/// Deterministic random flops (real deal frequencies), for calibration samples.
pub fn sample_flops(n: usize, seed: u64) -> Vec<[Card; 3]> {
    let mut x = seed | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    (0..n)
        .map(|_| {
            loop {
                let c = [next() % 52, next() % 52, next() % 52].map(|i| i as u8);
                if c[0] != c[1] && c[1] != c[2] && c[0] != c[2] {
                    break c.map(Card::from_index);
                }
            }
        })
        .collect()
}

/// One calibration round: solve the preflop game with `table`, solve `flops` sampled flops of each
/// library line, and fit each line's situation. Situations without a line borrow the nearest one.
pub fn calibrate_round(
    table: Realization,
    flops: usize,
    target_pct: f64,
    log: &mut dyn FnMut(&str),
) -> Result<Realization, String> {
    let config = PreflopConfig {
        realization: table,
        ..PreflopConfig::default()
    };
    let rake = Rake {
        pct: config.rake_pct,
        cap: config.rake_cap,
    };
    let mut pre = PreflopSolver::new(PreflopGame::new(config));
    for _ in 0..300 {
        pre.iterate();
    }
    let mut out = table;
    for (li, line) in LINES.iter().enumerate() {
        let spot = line_spot(&pre, line)?;
        let (mut ev_oop, mut ev_ip) = (0.0, 0.0);
        for (i, flop) in sample_flops(flops, 0xCA11_B8A7 + li as u64)
            .into_iter()
            .enumerate()
        {
            let t0 = std::time::Instant::now();
            let game = crate::holdem::build(&flop_spot(&spot, flop, rake)?)?;
            let (s, expl) = solve_to(&game, target_pct, 600);
            let r = s.report();
            ev_oop += r.ev[0];
            ev_ip += r.ev[1];
            log(&format!(
                "  {} flop {}/{} {}: EV OOP {:.3} IP {:.3}, {:.2}% pot, {:.0}s",
                line.id,
                i + 1,
                flops,
                flop_name(&flop),
                r.ev[0],
                r.ev[1],
                expl,
                t0.elapsed().as_secs_f64()
            ));
        }
        let share = ev_ip / (ev_ip + ev_oop);
        let k = fit_k(&spot.reach, share);
        log(&format!(
            "{}: in-position share {share:.4}, fitted k {k:.3}",
            line.id
        ));
        out.k[line.pot as usize][line.aggressor_ip as usize] = k;
    }
    // Borrowed situations: SRP with the aggressor out of position takes the 3-bet value; 4-bet
    // pots take the 3-bet values.
    out.k[PotType::Srp as usize][0] = out.k[PotType::ThreeBet as usize][0];
    out.k[PotType::FourBet as usize] = out.k[PotType::ThreeBet as usize];
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_1755_distinct_flops() {
        let flops = all_flops();
        assert_eq!(flops.len(), 1755);
        // Canonical forms are fixed points.
        for f in &flops {
            assert_eq!(canonical_flop(*f).0, *f);
        }
    }

    #[test]
    fn canonical_flop_maps_isomorphic_flops_together() {
        let p = |s: &str| {
            let c = crate::cards::parse_cards(s).unwrap();
            [c[0], c[1], c[2]]
        };
        let (a, _) = canonical_flop(p("Ks7d2c"));
        let (b, _) = canonical_flop(p("Kh7s2d"));
        assert_eq!(a, b);
        let (m, _) = canonical_flop(p("9h8h2h"));
        assert_eq!(flop_name(&m), "9s8s2s");
        // The permutation really maps the flop onto the representative.
        let f = p("Td9c4d");
        let (c, perm) = canonical_flop(f);
        assert_eq!(
            sorted_desc(f.map(|x| Card::new(x.rank(), perm[x.suit() as usize]))),
            c
        );
    }

    #[test]
    fn fit_k_inverts_the_share_model() {
        let r: Range = "AA,KK,QQ,AKs,72o,T9s".parse().unwrap();
        let w = class_weights(&r);
        let reach = [w.clone(), w];
        // Same ranges: share 0.5 at k = 1.
        assert!((fit_k(&reach, 0.5) - 1.0).abs() < 1e-3);
        assert!(fit_k(&reach, 0.6) > 1.0);
    }
}
