//! Walking a solved game in real cards, including through isomorphic deals.

use hexas_core::cards::{Card, parse_cards};
use hexas_core::game::Action;
use hexas_core::holdem::{BetSizes, Rake, Spot, TreeConfig, build};
use hexas_core::query::{Kind, Step, hand_ev, walk};
use hexas_core::solver::{DcfrParams, Solver};

fn card(s: &str) -> u8 {
    s.parse::<Card>().unwrap().index()
}

fn spot(iso: bool) -> Spot {
    let mut cfg = TreeConfig::new(10.0, 20.0).with_sizes(BetSizes {
        bets: vec![0.5],
        raise_mult: 3.0,
        max_raises: 1,
        bet_allin: false,
    });
    cfg.rake = Rake::NONE;
    cfg.isomorphism = iso;
    Spot {
        board: parse_cards("Ks7s2s").unwrap(),
        ranges: [
            "AA,KK,77,AKs,KQs,T9s".parse().unwrap(),
            "QQ-88,AQs,KJs,JTs".parse().unwrap(),
        ],
        config: cfg,
    }
}

#[test]
fn walk_tracks_pot_stacks_and_reach() {
    let s = spot(true);
    let game = build(&s).unwrap();
    let mut solver = Solver::new(&game, DcfrParams::default());
    for _ in 0..50 {
        solver.iterate();
    }
    // OOP bets 5 (50 %), IP calls, turn 5h.
    let v = walk(
        &solver,
        &[Step::Action(1), Step::Action(1), Step::Card(card("5h"))],
    )
    .unwrap();
    assert_eq!(v.pot, 20.0);
    assert_eq!(v.stacks, [15.0, 15.0]);
    assert_eq!(v.street, [0.0, 0.0]);
    assert!(matches!(v.kind, Kind::Action { player: 0, .. }));
    // Hands holding the 5h are gone; reach is the range times the path probability.
    for p in 0..2 {
        for (h, &m) in game.hands[p].iter().enumerate() {
            if m >> card("5h") & 1 == 1 {
                assert_eq!(v.reach[p][h], 0.0);
            }
            assert!(v.reach[p][h] <= game.weights[p][h]);
        }
    }
    let Kind::Action { actions, .. } = &v.kind else {
        panic!()
    };
    assert_eq!(actions[1], Action::Bet(10.0));
    assert!(walk(&solver, &[Step::Card(0)]).is_err());
}

#[test]
fn isomorphic_deals_look_like_their_own_subtree() {
    // On a spade flop c, d and h are interchangeable: 5c has its own subtree, 5d and 5h reuse it.
    let (si, sf) = (spot(true), spot(false));
    let (gi, gf) = (build(&si).unwrap(), build(&sf).unwrap());
    let (mut a, mut b) = (
        Solver::new(&gi, DcfrParams::default()),
        Solver::new(&gf, DcfrParams::default()),
    );
    for _ in 0..200 {
        a.iterate();
        b.iterate();
    }
    for c in ["5c", "5d", "5h", "As"] {
        let path = [Step::Action(0), Step::Action(0), Step::Card(card(c))];
        let (vi, vf) = (walk(&a, &path).unwrap(), walk(&b, &path).unwrap());
        assert_eq!(vi.kind, vf.kind);
        let (x, y) = (vi.strategy.clone().unwrap(), vf.strategy.clone().unwrap());
        let d = x
            .iter()
            .zip(&y)
            .map(|(p, q)| (p - q).abs())
            .fold(0.0, f32::max);
        assert!(d < 0.05, "{c}: strategy differs by {d}");
        for p in 0..2 {
            let (ei, ef) = (hand_ev(&a, &vi, p), hand_ev(&b, &vf, p));
            let d = ei
                .iter()
                .zip(&ef)
                .map(|(p, q)| (p - q).abs())
                .fold(0.0, f64::max);
            assert!(d < 0.05, "{c}: EV of player {p} differs by {d}");
        }
    }
}
