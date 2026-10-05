//! PRD §8.2–§8.3, §8.5: the solver against games with known solutions, plus invariants.

use hexas_core::cards::parse_cards;
use hexas_core::game::{Game, Node};
use hexas_core::holdem::{BetSizes, Rake, RiverSpot, TreeConfig, build_river};
use hexas_core::solver::{DcfrParams, Solver};
use hexas_core::toy::{kuhn, leduc};

fn solve(game: &Game, iters: u32) -> Solver<'_> {
    let mut s = Solver::new(game, DcfrParams::default());
    for _ in 0..iters {
        s.iterate();
    }
    s
}

#[test]
fn kuhn_value_and_exploitability() {
    let game = kuhn();
    let s = solve(&game, 2000);
    let r = s.report();
    assert!(
        (r.ev[0] - (-1.0 / 18.0)).abs() < 1e-3,
        "P0 value {}",
        r.ev[0]
    );
    assert!((r.ev[0] + r.ev[1]).abs() < 1e-6, "zero-sum");
    assert!(
        r.exploitability < 1e-3,
        "exploitability {}",
        r.exploitability
    );

    // Known equilibrium facts: P1 (second player) always calls a bet with K, never with J;
    // with Q after a check it never bets.
    let after_bet = game.find(&["Bet 1.00"]).unwrap();
    let st = s.strategy(after_bet); // [Fold, Call] x [J, Q, K]
    assert!(st[3] < 0.01, "P1 calls with J: {}", st[3]);
    assert!(st[5] > 0.99, "P1 folds K: {}", st[5]);
}

#[test]
fn leduc_converges() {
    let game = leduc();
    let s = solve(&game, 3000);
    let r = s.report();
    assert!(
        r.exploitability < 1e-3,
        "exploitability {}",
        r.exploitability
    );
    // Published game value of Leduc hold'em for the first player is about −0.0856.
    assert!((r.ev[0] - (-0.0856)).abs() < 3e-3, "P0 value {}", r.ev[0]);
}

#[test]
fn exploitability_falls_with_iterations() {
    let game = leduc();
    let mut s = Solver::new(&game, DcfrParams::default());
    let mut prev = f64::INFINITY;
    for checkpoint in [10, 100, 1000] {
        while s.iterations() < checkpoint {
            s.iterate();
        }
        let e = s.report().exploitability;
        assert!(
            e < prev,
            "exploitability did not fall by iteration {checkpoint}: {e} >= {prev}"
        );
        prev = e;
    }
}

/// OOP holds nuts (AA) or air (65o), IP holds bluff catchers (QQ). Pot 10, one pot-sized bet,
/// no raises. Equilibrium: OOP bets all AA and bluffs so that bluffs are 1/3 of the betting range
/// (3 of 12 air combos); IP calls 50 % (MDF).
fn polarized_spot() -> Game {
    let mut cfg = TreeConfig::new(10.0, 100.0);
    cfg.sizes = BetSizes {
        bets: vec![1.0],
        raise_mult: 3.0,
        max_raises: 0,
        bet_allin: false,
    };
    cfg.rake = Rake::NONE;
    let spot = RiverSpot {
        board: parse_cards("KdKs7h4c2d").unwrap(),
        ranges: ["AA,65o".parse().unwrap(), "QQ".parse().unwrap()],
        config: cfg,
    };
    build_river(&spot).unwrap()
}

#[test]
fn polarized_river_matches_theory() {
    let game = polarized_spot();
    let s = solve(&game, 3000);
    assert!(s.report().exploitability_pct < 0.1);

    let root = s.strategy(0); // [Check, Bet 10] x OOP hands
    let n = game.num_hands(0);
    let is_aa = |h: usize| {
        game.hands[0][h].count_ones() == 2 && {
            let m = game.hands[0][h];
            (m.trailing_zeros() / 4) == 12 && ((63 - m.leading_zeros()) / 4) == 12
        }
    };
    let (mut aa_bet, mut aa_n, mut air_bet, mut air_n) = (0.0, 0.0, 0.0, 0.0);
    for h in 0..n {
        if is_aa(h) {
            aa_bet += root[n + h];
            aa_n += 1.0;
        } else {
            air_bet += root[n + h];
            air_n += 1.0;
        }
    }
    assert_eq!((aa_n, air_n), (6.0, 12.0));
    assert!(aa_bet / aa_n > 0.99, "AA bet freq {}", aa_bet / aa_n);
    let bluffs = air_bet; // combos
    let bluff_share = bluffs / (bluffs + aa_bet);
    assert!(
        (bluff_share - 1.0 / 3.0).abs() < 0.01,
        "bluff share {bluff_share}"
    );

    let facing = game.find(&["Bet 10.00"]).unwrap();
    let st = s.strategy(facing); // [Fold, Call] x IP hands
    let m = game.num_hands(1);
    let call = st[m..].iter().sum::<f32>() / m as f32;
    assert!((call - 0.5).abs() < 0.01, "IP call freq {call}");
}

fn realistic_spot(rake: Rake) -> Game {
    let mut cfg = TreeConfig::new(20.0, 90.0);
    cfg.rake = rake;
    let spot = RiverSpot {
        board: parse_cards("Qs9h5d3c2s").unwrap(),
        ranges: [
            "AA-22,AKs-A2s,KQs-K9s,QJs-Q9s,JTs,T9s,98s,87s,AKo-ATo,KQo,QJo"
                .parse()
                .unwrap(),
            "TT-22,AQs-A2s,KQs-K9s,QJs-Q8s,JTs-J9s,T9s,98s,87s,76s,AQo-ATo,KQo-KJo,QJo"
                .parse()
                .unwrap(),
        ],
        config: cfg,
    };
    build_river(&spot).unwrap()
}

#[test]
fn realistic_river_converges_and_strategies_are_distributions() {
    let game = realistic_spot(Rake {
        pct: 0.05,
        cap: 3.0,
    });
    let s = solve(&game, 300);
    let r = s.report();
    assert!(
        r.exploitability_pct < 0.5,
        "exploitability {:.3}% pot",
        r.exploitability_pct
    );
    for (i, n) in game.nodes.iter().enumerate() {
        if let Node::Action {
            player, actions, ..
        } = n
        {
            let st = s.strategy(i);
            let h = game.num_hands(*player);
            for k in 0..h {
                let sum: f32 = (0..actions.len()).map(|a| st[a * h + k]).sum();
                assert!((sum - 1.0).abs() < 1e-4, "node {i} hand {k} sums to {sum}");
            }
        }
    }
}

#[test]
fn chips_are_conserved_with_rake() {
    // With rake the players' EVs sum to the pot minus the expected rake, which is at most the cap.
    let raked = realistic_spot(Rake {
        pct: 0.05,
        cap: 3.0,
    });
    let s = solve(&raked, 200);
    let r = s.report();
    let sum = r.ev[0] + r.ev[1];
    assert!((20.0 - 3.0 - 1e-6..20.0).contains(&sum), "EV sum {sum}");

    let free = realistic_spot(Rake::NONE);
    let s = solve(&free, 200);
    let r = s.report();
    assert!(
        (r.ev[0] + r.ev[1] - 20.0).abs() < 1e-3,
        "EV sum without rake {}",
        r.ev[0] + r.ev[1]
    );
}

#[test]
fn tree_follows_sizing_rules() {
    let game = realistic_spot(Rake::NONE);
    let root = &game.nodes[0];
    let Node::Action { actions, .. } = root else {
        panic!()
    };
    let labels: Vec<String> = actions.iter().map(|a| a.to_string()).collect();
    assert_eq!(
        labels,
        ["Check", "Bet 6.60", "Bet 13.20", "Bet 20.00", "Bet 25.00"]
    );
    // Facing a 20 bet: fold, call, raise to 60, all-in for 90.
    let n = game.find(&["Bet 20.00"]).unwrap();
    let Node::Action { actions, .. } = &game.nodes[n] else {
        panic!()
    };
    let labels: Vec<String> = actions.iter().map(|a| a.to_string()).collect();
    assert_eq!(labels, ["Fold", "Call", "Raise 60.00", "All-in 90.00"]);
    // A 3× raise of a 25 bet would leave 15 behind, less than 10 % of the 170 pot: snapped to all-in.
    let n = game.find(&["Bet 25.00"]).unwrap();
    let Node::Action { actions, .. } = &game.nodes[n] else {
        panic!()
    };
    let labels: Vec<String> = actions.iter().map(|a| a.to_string()).collect();
    assert_eq!(labels, ["Fold", "Call", "All-in 90.00"]);
}
