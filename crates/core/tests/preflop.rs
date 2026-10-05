//! 6-max preflop: equity table, tree rules (PRD §3.2) and solve sanity (PRD §8.6).

use hexas_core::preflop::classes::{NUM_CLASSES, class_name, combo_count};
use hexas_core::preflop::equity;
use hexas_core::preflop::game::{PNode, PreflopConfig, PreflopGame, PreflopSolver};

fn idx(name: &str) -> usize {
    (0..NUM_CLASSES).find(|&i| class_name(i) == name).unwrap()
}

#[test]
fn equity_table_matches_known_matchups() {
    let eq = equity::table();
    let e = |a: &str, b: &str| eq[idx(a) * NUM_CLASSES + idx(b)];
    // Well-known all-in equities (±1 %).
    assert!(
        (e("AA", "KK") - 0.82).abs() < 0.01,
        "AA vs KK {}",
        e("AA", "KK")
    );
    assert!(
        (e("QQ", "AKs") - 0.54).abs() < 0.01,
        "QQ vs AKs {}",
        e("QQ", "AKs")
    );
    assert!(
        (e("AA", "72o") - 0.88).abs() < 0.015,
        "AA vs 72o {}",
        e("AA", "72o")
    );
    for i in 0..NUM_CLASSES {
        assert_eq!(eq[i * NUM_CLASSES + i], 0.5);
        for j in 0..NUM_CLASSES {
            let s = eq[i * NUM_CLASSES + j] + eq[j * NUM_CLASSES + i];
            assert!((s - 1.0).abs() < 1e-6);
        }
    }
}

fn labels(g: &PreflopGame, node: usize) -> Vec<String> {
    match &g.nodes[node] {
        PNode::Act { actions, .. } => actions.iter().map(|a| a.to_string()).collect(),
        _ => vec![],
    }
}

fn child(g: &PreflopGame, node: usize, label: &str) -> usize {
    let PNode::Act {
        actions, children, ..
    } = &g.nodes[node]
    else {
        panic!("not an action node")
    };
    children[actions.iter().position(|a| a.to_string() == label).unwrap()]
}

#[test]
fn tree_follows_the_sizing_rules() {
    let g = PreflopGame::new(PreflopConfig::default());
    // UTG opens to 2.5, no limp.
    assert_eq!(labels(&g, 0), ["Fold", "Raise 2.5"]);
    // Folded to the SB: opens to 3.
    let mut n = 0;
    for _ in 0..4 {
        n = child(&g, n, "Fold");
    }
    assert_eq!(labels(&g, n), ["Fold", "Raise 3.0"]);
    // BB against the SB open is in position: 3-bet 3x = 9.
    let bb = child(&g, n, "Raise 3.0");
    assert_eq!(labels(&g, bb), ["Fold", "Call", "Raise 9.0"]);
    // BTN open, BB 3-bets out of position 4x = 10, BTN 4-bets 2.3x = 23, then all-in.
    let mut n = 0;
    for _ in 0..3 {
        n = child(&g, n, "Fold");
    }
    let n = child(&g, n, "Raise 2.5");
    let n = child(&g, n, "Fold"); // SB
    assert_eq!(labels(&g, n), ["Fold", "Call", "Raise 10.0"]);
    let n = child(&g, n, "Raise 10.0");
    assert_eq!(labels(&g, n), ["Fold", "Call", "Raise 23.0"]);
    let n = child(&g, n, "Raise 23.0");
    assert_eq!(labels(&g, n), ["Fold", "Call", "All-in 100"]);
    let n = child(&g, n, "All-in 100");
    assert_eq!(labels(&g, n), ["Fold", "Call"]);
    // A call ends the hand heads-up: BTN calls the all-in.
    assert!(matches!(
        g.nodes[child(&g, n, "Call")],
        PNode::AllIn {
            players: [5, 3],
            ..
        }
    ));
}

#[test]
fn solve_converges_to_sensible_ranges() {
    let mut s = PreflopSolver::new(PreflopGame::new(PreflopConfig::default()));
    for _ in 0..300 {
        s.iterate();
    }
    let r = s.report();
    for (p, g) in r.br_gain_bb100.iter().enumerate() {
        assert!(*g < 0.5, "player {p} gains {g} bb/100 by deviating");
    }
    // Rake is the only money leaving the table.
    let total: f64 = r.ev.iter().sum();
    assert!((-1.0..=0.0).contains(&total), "EV sum {total}");

    // RFI widens with position; premium hands always open, trash never does from UTG.
    let mut path = vec![];
    let mut prev = 0.0;
    for p in 0..4 {
        let (node, _) = s.walk(&path).unwrap();
        let st = s.strategy(node);
        let open: f64 = (0..NUM_CLASSES)
            .map(|h| st[NUM_CLASSES + h] as f64 * combo_count(h) as f64)
            .sum::<f64>()
            / 1326.0;
        assert!(
            open > prev,
            "position {p}: RFI {open} not wider than {prev}"
        );
        prev = open;
        if p == 0 {
            assert!(st[NUM_CLASSES + idx("AA")] > 0.99);
            assert!(st[NUM_CLASSES + idx("72o")] < 0.01);
        }
        path.push(0);
    }
}
