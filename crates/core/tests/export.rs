//! Result files: what is written is what the reader walks.

use hexas_core::cards::parse_cards;
use hexas_core::export::{Imported, read, write};
use hexas_core::holdem::{BetSizes, Rake, Spot, TreeConfig, build};
use hexas_core::query::{Step, hand_ev, walk};
use hexas_core::solver::{DcfrParams, Solver};
use hexas_core::spec::SpotSpec;

fn card(s: &str) -> u8 {
    parse_cards(s).unwrap()[0].index()
}

fn setup() -> (Spot, SpotSpec) {
    let mut cfg = TreeConfig::new(10.0, 20.0).with_sizes(BetSizes {
        bets: vec![0.5],
        raise_mult: 3.0,
        max_raises: 1,
        bet_allin: false,
    });
    cfg.rake = Rake::NONE;
    let (oop, ip) = ("AA,KK,77,AKs,KQs,T9s", "QQ-88,AQs,KJs,JTs");
    let spot = Spot {
        // Two-tone flop: the walk goes through an isomorphic turn card below.
        board: parse_cards("Ks7s2d").unwrap(),
        ranges: [oop.parse().unwrap(), ip.parse().unwrap()],
        config: cfg,
    };
    let spec = SpotSpec::describe(&spot, oop, ip);
    (spot, spec)
}

#[test]
fn spec_round_trips_the_tree() {
    let (spot, spec) = setup();
    let again = spec.to_spot().unwrap();
    assert_eq!(
        build(&again).unwrap().nodes.len(),
        build(&spot).unwrap().nodes.len()
    );
    let json = serde_json::to_string(&spec).unwrap();
    assert_eq!(serde_json::from_str::<SpotSpec>(&json).unwrap(), spec);
}

#[test]
fn imported_file_matches_the_solver_up_to_the_turn() {
    let (spot, spec) = setup();
    let game = build(&spot).unwrap();
    let mut solver = Solver::new(&game, DcfrParams::default());
    for _ in 0..60 {
        solver.iterate();
    }
    let bytes = write(&solver, &spec, 4);
    let loaded = read(&bytes).unwrap();
    assert_eq!(loaded.header.iterations, 60);
    let imp = Imported::new(&bytes).unwrap();

    // Flop root, a turn node behind an isomorphic card (h and c are interchangeable), and the
    // river chance node.
    let paths: [Vec<Step>; 3] = [
        vec![],
        vec![Step::Action(0), Step::Action(0), Step::Card(card("5h"))],
        vec![
            Step::Action(0),
            Step::Action(0),
            Step::Card(card("5h")),
            Step::Action(0),
            Step::Action(0),
        ],
    ];
    for path in &paths {
        let (a, b) = (walk(&solver, path).unwrap(), walk(&imp, path).unwrap());
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.pot, b.pot);
        for p in 0..2 {
            let d = a.reach[p]
                .iter()
                .zip(&b.reach[p])
                .map(|(x, y)| (x - y).abs())
                .fold(0.0, f32::max);
            assert!(d < 0.02, "reach differs by {d}");
        }
        if let (Some(x), Some(y)) = (&a.strategy, &b.strategy) {
            let d = x
                .iter()
                .zip(y)
                .map(|(p, q)| (p - q).abs())
                .fold(0.0, f32::max);
            assert!(d <= 1.0 / 255.0 + 1e-6, "strategy differs by {d}");
        }
    }

    // River action nodes are not stored: their strategy is unknown and the walk stops there.
    let mut river = paths[2].clone();
    river.push(Step::Card(card("3c")));
    let v = walk(&imp, &river).unwrap();
    assert!(v.strategy.is_none());
    river.push(Step::Action(0));
    assert!(walk(&imp, &river).is_err());

    // Root EVs are stored.
    let root = walk(&solver, &[]).unwrap();
    let ev = &imp.loaded.evs[&0];
    for (p, stored) in ev.iter().enumerate() {
        let live = hand_ev(&solver, &root, p);
        let d = live
            .iter()
            .zip(stored)
            .map(|(x, &y)| (x - y as f64).abs())
            .fold(0.0, f64::max);
        assert!(d < 1e-3, "root EV differs by {d}");
    }
    assert!(read(b"nope").is_err());
}
