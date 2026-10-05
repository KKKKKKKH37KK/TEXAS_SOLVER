//! Multi-street subgames: chance nodes, run-outs, size estimate (PRD §4.1, §5, §8.5).

use hexas_core::cards::{Card, mask_of, parse_cards};
use hexas_core::eval::evaluate;
use hexas_core::game::{Game, Node};
use hexas_core::holdem::{BetSizes, Rake, Spot, TreeConfig, build, estimate};
use hexas_core::solver::{DcfrParams, Solver};

fn spot(board: &str, oop: &str, ip: &str, cfg: TreeConfig) -> Spot {
    Spot {
        board: parse_cards(board).unwrap(),
        ranges: [oop.parse().unwrap(), ip.parse().unwrap()],
        config: cfg,
    }
}

fn small_sizes() -> BetSizes {
    BetSizes {
        bets: vec![0.5],
        raise_mult: 3.0,
        max_raises: 1,
        bet_allin: false,
    }
}

fn solve(game: &Game, iters: u32) -> Solver<'_> {
    let mut s = Solver::new(game, DcfrParams::default());
    for _ in 0..iters {
        s.iterate();
    }
    s
}

#[test]
fn estimate_matches_built_tree() {
    let cases = [
        spot(
            "Ks7d2c5h",
            "AA-22,AKs,KQs",
            "AA-77,AKo",
            TreeConfig::new(10.0, 50.0),
        ),
        spot(
            "Ks7d2c",
            "AA-TT",
            "KK-99",
            TreeConfig::new(10.0, 30.0).with_sizes(small_sizes()),
        ),
    ];
    for s in &cases {
        let (t, hands) = estimate(s).unwrap();
        let game = build(s).unwrap();
        let count = |f: fn(&Node) -> bool| game.nodes.iter().filter(|n| f(n)).count() as u64;
        assert_eq!(t.action_nodes, count(|n| matches!(n, Node::Action { .. })));
        assert_eq!(t.chance_nodes, count(|n| matches!(n, Node::Chance { .. })));
        assert_eq!(
            t.terminal_nodes,
            count(|n| matches!(n, Node::Fold { .. } | Node::Showdown { .. }))
        );
        assert_eq!(hands, [game.num_hands(0), game.num_hands(1)]);
        let solver = Solver::new(&game, DcfrParams::default());
        assert_eq!(t.solver_bytes(hands), solver.memory_bytes() as u64);
    }
}

/// Exact all-in equity of one combo against another by enumerating the run-outs.
fn equity(board: &[Card], a: &[Card], b: &[Card]) -> f64 {
    let dead = mask_of(board) | mask_of(a) | mask_of(b);
    let rest: Vec<Card> = (0..52u8)
        .filter(|&c| dead >> c & 1 == 0)
        .map(Card::from_index)
        .collect();
    let need = 5 - board.len();
    let (mut won, mut n) = (0.0, 0.0);
    let mut score = |extra: &[Card]| {
        let mut x = board.to_vec();
        x.extend_from_slice(extra);
        let (mut xa, mut xb) = (x.clone(), x);
        xa.extend_from_slice(a);
        xb.extend_from_slice(b);
        let (sa, sb) = (evaluate(&xa), evaluate(&xb));
        won += if sa > sb {
            1.0
        } else if sa == sb {
            0.5
        } else {
            0.0
        };
        n += 1.0;
    };
    if need == 1 {
        for &c in &rest {
            score(&[c]);
        }
    } else {
        for i in 0..rest.len() {
            for j in i + 1..rest.len() {
                score(&[rest[i], rest[j]]);
            }
        }
    }
    won / n
}

#[test]
fn check_down_values_equal_runout_equity() {
    // No chips behind: both players can only check, so OOP's EV is pot × equity over the run-outs.
    for board in ["Qs7h2d", "Qs7h2d8c"] {
        let mut cfg = TreeConfig::new(10.0, 0.0);
        cfg.rake = Rake::NONE;
        let s = spot(board, "AhKh", "Tc9c", cfg);
        let game = build(&s).unwrap();
        let r = solve(&game, 1).report();
        let b = parse_cards(board).unwrap();
        let eq = equity(
            &b,
            &parse_cards("AhKh").unwrap(),
            &parse_cards("Tc9c").unwrap(),
        );
        assert!(
            (r.ev[0] - 10.0 * eq).abs() < 1e-4,
            "{board}: EV {} vs {}",
            r.ev[0],
            10.0 * eq
        );
        assert!((r.ev[0] + r.ev[1] - 10.0).abs() < 1e-4);
    }
}

#[test]
fn turn_spot_converges() {
    let mut cfg = TreeConfig::new(10.0, 40.0);
    cfg.rake = Rake {
        pct: 0.05,
        cap: 3.0,
    };
    let s = spot("Ks7d2c5h", "AA-22,AKs,KQs,QJs", "AA-77,AKo,KJs,T9s", cfg);
    let game = build(&s).unwrap();
    let solver = solve(&game, 150);
    let r = solver.report();
    assert!(
        r.exploitability_pct < 0.5,
        "{:.3}% pot",
        r.exploitability_pct
    );
    let sum = r.ev[0] + r.ev[1];
    assert!((10.0 - 3.0 - 1e-6..10.0).contains(&sum), "EV sum {sum}");
}

#[test]
fn flop_spot_with_allin_runouts_converges() {
    // Short stacks so bets and raises reach all-in on the flop and turn, exercising run-outs.
    let mut cfg = TreeConfig::new(10.0, 15.0).with_sizes(small_sizes());
    cfg.rake = Rake::NONE;
    let s = spot("Ks7d2c", "AA,KK,77,AKs,KQs", "QQ-88,AQs,KJs,T9s", cfg);
    let game = build(&s).unwrap();
    assert!(game.nodes.iter().any(|n| matches!(n, Node::Chance { .. })));
    let solver = solve(&game, 200);
    let r = solver.report();
    assert!(
        r.exploitability_pct < 0.5,
        "{:.3}% pot",
        r.exploitability_pct
    );
    assert!((r.ev[0] + r.ev[1] - 10.0).abs() < 1e-3);
}

/// Check-down EV of OOP over every compatible hand pair and run-out, by enumeration.
fn brute_force_checkdown(game: &Game, board: &[Card], pot: f64) -> f64 {
    let bm = mask_of(board);
    let cards = |m: u64| -> Vec<Card> {
        (0..52u8)
            .filter(|&c| m >> c & 1 == 1)
            .map(Card::from_index)
            .collect()
    };
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for &h in &game.hands[0] {
        for &o in &game.hands[1] {
            if h & o != 0 {
                continue;
            }
            let dead = bm | h | o;
            let rest: Vec<u8> = (0..52u8).filter(|&c| dead >> c & 1 == 0).collect();
            let mut runouts: Vec<u64> = vec![];
            for i in 0..rest.len() {
                for j in i + 1..rest.len() {
                    runouts.push(1 << rest[i] | 1 << rest[j]);
                }
            }
            let mut eq = 0.0;
            for r in &runouts {
                let (a, b) = (evaluate(&cards(bm | r | h)), evaluate(&cards(bm | r | o)));
                eq += if a > b {
                    1.0
                } else if a == b {
                    0.5
                } else {
                    0.0
                };
            }
            num += eq / runouts.len() as f64;
            den += 1.0;
        }
    }
    pot * num / den
}

#[test]
fn isomorphic_runouts_equal_enumeration() {
    // Rainbow: the turn can create a river symmetry the flop does not have (2h makes c and h
    // interchangeable on Ks7d2c). Paired: d and s. Monotone: c, d and h.
    for board in ["Ks7d2c", "KsKd2c", "Ks7s2s"] {
        let mut cfg = TreeConfig::new(10.0, 0.0);
        cfg.rake = Rake::NONE;
        let s = spot(board, "AA,A5s", "77,QQ", cfg);
        let game = build(&s).unwrap();
        let ev = solve(&game, 1).report().ev[0];
        let bf = brute_force_checkdown(&game, &s.board, 10.0);
        assert!((ev - bf).abs() < 1e-4, "{board}: {ev} vs {bf}");
    }
}

#[test]
fn isomorphism_matches_the_full_tree() {
    // Monotone flop (three interchangeable suits), paired flop (two), and a turn spot. Float
    // rounding takes the two runs down slightly different paths, so compare loosely.
    for (board, saving) in [("Ks7s2s", 0.4), ("KsKd2c", 0.15), ("Ks7s2s5d", 0.2)] {
        let mut cfg = TreeConfig::new(10.0, 20.0).with_sizes(small_sizes());
        cfg.rake = Rake::NONE;
        let iso_spot = spot(
            board,
            "AA,KK,77,AKs,KQs,T9s",
            "QQ-88,AQs,KJs,JTs",
            cfg.clone(),
        );
        cfg.isomorphism = false;
        let full_spot = spot(board, "AA,KK,77,AKs,KQs,T9s", "QQ-88,AQs,KJs,JTs", cfg);

        let (t_iso, hands) = estimate(&iso_spot).unwrap();
        let (t_full, _) = estimate(&full_spot).unwrap();
        let ratio = t_iso.solver_bytes(hands) as f64 / t_full.solver_bytes(hands) as f64;
        assert!(ratio < 1.0 - saving, "{board}: memory ratio {ratio:.2}");

        let (g_iso, g_full) = (build(&iso_spot).unwrap(), build(&full_spot).unwrap());
        assert_eq!(
            t_iso.action_nodes,
            g_iso
                .nodes
                .iter()
                .filter(|n| matches!(n, Node::Action { .. }))
                .count() as u64
        );
        let (s_iso, s_full) = (solve(&g_iso, 100), solve(&g_full, 100));
        let (r_iso, r_full) = (s_iso.report(), s_full.report());
        for p in 0..2 {
            assert!(
                (r_iso.ev[p] - r_full.ev[p]).abs() < 1e-3,
                "{board}: EV {p} {} vs {}",
                r_iso.ev[p],
                r_full.ev[p]
            );
        }
        assert!(
            r_iso.exploitability_pct < 0.5 && r_full.exploitability_pct < 0.5,
            "{board}: exploitability {:.3}% vs {:.3}% pot",
            r_iso.exploitability_pct,
            r_full.exploitability_pct
        );
        // Root strategies agree hand by hand.
        let (a, b) = (s_iso.strategy(0), s_full.strategy(0));
        let diff = a
            .iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max);
        assert!(diff < 0.02, "{board}: root strategy differs by {diff}");
    }
}

#[test]
fn isomorphism_only_uses_suits_the_ranges_are_symmetric_in() {
    let cfg = TreeConfig::new(10.0, 20.0).with_sizes(small_sizes());
    let mut off = cfg.clone();
    off.isomorphism = false;
    let nodes = |oop: &str, c: &TreeConfig| {
        estimate(&spot("Ks7s2s", oop, "QQ", c.clone()))
            .unwrap()
            .0
            .action_nodes
    };
    // d, h, c are interchangeable on a spade flop. "AhAc" is symmetric only in h <-> c.
    let all = nodes("AA,KK", &cfg);
    let partial = nodes("AhAc,KK", &cfg);
    let none = nodes("AA,KK", &off);
    assert!(
        all < partial && partial < none,
        "{all} < {partial} < {none}"
    );
}

#[test]
fn flop_preset_follows_prd() {
    let cfg = TreeConfig::preset(5.5, 97.5, 3);
    let g = build(&spot("Ks7d2c", "AA", "KK", cfg)).unwrap();
    let labels = |n: usize| match &g.nodes[n] {
        Node::Action { actions, .. } => actions.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        _ => panic!(),
    };
    assert_eq!(labels(0), ["Check"]); // no donk
    assert_eq!(
        labels(g.find(&["Check"]).unwrap()),
        ["Check", "Bet 1.82", "Bet 3.63", "Bet 5.50", "Bet 6.88"]
    );
    // One raise per street: facing the raise there is no re-raise or all-in.
    let n = g.find(&["Check", "Bet 5.50", "Raise 16.50"]).unwrap();
    assert_eq!(labels(n), ["Fold", "Call"]);
    // Turn after check-check: 66 % and 125 %, and OOP may lead.
    let Node::Chance { children, .. } = &g.nodes[g.find(&["Check", "Check"]).unwrap()] else {
        panic!()
    };
    assert_eq!(labels(children[0]), ["Check", "Bet 3.63", "Bet 6.88"]);
    assert_eq!(TreeConfig::preset(5.5, 97.5, 4).sizes[0].bets.len(), 4);
}

#[test]
fn flop_donk_switch() {
    let mut cfg = TreeConfig::new(10.0, 50.0);
    let root_actions = |cfg: &TreeConfig| {
        let g = build(&spot("Ks7d2c", "AA", "KK", cfg.clone())).unwrap();
        let Node::Action { actions, .. } = &g.nodes[0] else {
            panic!()
        };
        actions.len()
    };
    assert_eq!(root_actions(&cfg), 5); // check + 4 bets
    cfg.oop_flop_bets = false;
    assert_eq!(root_actions(&cfg), 1);
}
