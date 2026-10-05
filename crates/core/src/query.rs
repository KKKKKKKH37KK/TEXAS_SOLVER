//! Walking a solved hold'em game along real actions and cards, for UIs and result export.
//!
//! With suit isomorphism (see `holdem`), a dealt card may have no subtree of its own; the walk then
//! continues in the subtree of the isomorphic card and remembers the suit permutation, so callers
//! always see real cards and real hands.

use crate::game::{Action, Game, Node};
use crate::holdem::{SUIT_PAIRS, swap_card};
use crate::solver::Solver;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// Index into the current action node's actions.
    Action(usize),
    /// A real card dealt at a chance node.
    Card(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Action { player: usize, actions: Vec<Action> },
    Chance,
    Fold { folder: usize },
    Showdown,
}

/// The game state at the end of a path, in real cards. Per-hand vectors are indexed like
/// `Game::hands`.
#[derive(Clone, Debug)]
pub struct View {
    /// Node id in the game tree.
    pub node: usize,
    pub kind: Kind,
    /// Real board cards.
    pub board: u64,
    pub pot: f64,
    /// Chips behind.
    pub stacks: [f64; 2],
    /// Chips put in on the current street.
    pub street: [f64; 2],
    /// Range weight × probability of having taken this path, per player and hand.
    pub reach: [Vec<f32>; 2],
    /// Average strategy at an action node, `[action][hand]`.
    pub strategy: Option<Vec<f32>>,
    /// Real suit → suit of the tree that was solved.
    perm: [u8; 4],
}

impl View {
    /// Cards that can be dealt at a chance node.
    pub fn dealable(&self) -> Vec<u8> {
        (0..52u8).filter(|&c| self.board >> c & 1 == 0).collect()
    }
}

fn map_card(perm: &[u8; 4], c: u8) -> u8 {
    c - c % 4 + perm[(c % 4) as usize]
}

fn map_mask(perm: &[u8; 4], mut m: u64) -> u64 {
    let mut out = 0;
    while m != 0 {
        let c = m.trailing_zeros() as u8;
        m &= m - 1;
        out |= 1 << map_card(perm, c);
    }
    out
}

/// For each real hand of each player: the index of the matching hand in the solved tree.
fn internal_index(game: &Game, perm: &[u8; 4]) -> [Vec<usize>; 2] {
    [0, 1].map(|p| {
        let index: HashMap<u64, usize> = game.hands[p]
            .iter()
            .enumerate()
            .map(|(h, &m)| (m, h))
            .collect();
        game.hands[p]
            .iter()
            .enumerate()
            .map(|(h, &m)| index.get(&map_mask(perm, m)).copied().unwrap_or(h))
            .collect()
    })
}

/// Follows `path` from the root.
pub fn walk(solver: &Solver, path: &[Step]) -> Result<View, String> {
    let game = solver.game();
    let mut node = 0;
    let mut perm = [0u8, 1, 2, 3];
    let mut board = game.root_board;
    let mut contrib = [0.0f64; 2];
    let mut street = [0.0f64; 2];
    let mut reach = game.weights.clone();

    for (i, step) in path.iter().enumerate() {
        let err = |m: &str| format!("step {i} ({step:?}): {m}");
        match (&game.nodes[node], *step) {
            (
                Node::Action {
                    player,
                    actions,
                    children,
                },
                Step::Action(a),
            ) => {
                let p = *player;
                let act = *actions.get(a).ok_or_else(|| err("no such action"))?;
                let sigma = solver.strategy(node);
                let n = game.num_hands(p);
                let idx = internal_index(game, &perm);
                for (h, r) in reach[p].iter_mut().enumerate() {
                    *r *= sigma[a * n + idx[p][h]];
                }
                let o = 1 - p;
                let add = match act {
                    Action::Fold | Action::Check => 0.0,
                    Action::Call => street[o] - street[p],
                    Action::Bet(x) | Action::Raise(x) | Action::AllIn(x) => x - street[p],
                };
                street[p] += add;
                contrib[p] += add;
                node = children[a];
            }
            (
                Node::Chance {
                    cards,
                    children,
                    iso,
                    ..
                },
                Step::Card(c),
            ) => {
                if c >= 52 || board >> c & 1 == 1 {
                    return Err(err("card is not available"));
                }
                let ic = map_card(&perm, c);
                node = if let Some(k) = cards.iter().position(|&x| x == ic) {
                    children[k]
                } else {
                    let x = iso
                        .iter()
                        .find(|x| x.card == ic)
                        .ok_or_else(|| err("card has no subtree"))?;
                    let pair = SUIT_PAIRS[x.swap as usize];
                    perm = perm.map(|s| swap_card(s, pair));
                    children[x.canon as usize]
                };
                board |= 1 << c;
                street = [0.0; 2];
                for (rp, hp) in reach.iter_mut().zip(&game.hands) {
                    for (r, &m) in rp.iter_mut().zip(hp) {
                        if m >> c & 1 == 1 {
                            *r = 0.0;
                        }
                    }
                }
            }
            _ => return Err(err("does not match the node type")),
        }
    }

    let (kind, strategy) = match &game.nodes[node] {
        Node::Action {
            player, actions, ..
        } => {
            let p = *player;
            let sigma = solver.strategy(node);
            let n = game.num_hands(p);
            let idx = internal_index(game, &perm);
            let n_act = actions.len();
            let mut s = vec![0.0f32; n_act * n];
            for a in 0..n_act {
                for h in 0..n {
                    s[a * n + h] = sigma[a * n + idx[p][h]];
                }
            }
            (
                Kind::Action {
                    player: p,
                    actions: actions.clone(),
                },
                Some(s),
            )
        }
        Node::Chance { .. } => (Kind::Chance, None),
        Node::Fold { folder, .. } => (Kind::Fold { folder: *folder }, None),
        Node::Showdown { .. } => (Kind::Showdown, None),
    };
    Ok(View {
        node,
        kind,
        board,
        pot: game.start_pot + contrib[0] + contrib[1],
        stacks: [game.eff_stack - contrib[0], game.eff_stack - contrib[1]],
        street,
        reach,
        strategy,
        perm,
    })
}

/// EV in chips of each of player `p`'s hands at the view's node (indexed like `Game::hands`).
pub fn hand_ev(solver: &Solver, view: &View, p: usize) -> Vec<f64> {
    let game = solver.game();
    let idx = internal_index(game, &view.perm);
    let o = 1 - p;
    // Opponent reach in the solved tree's suits.
    let mut reach_o = vec![0.0f32; game.num_hands(o)];
    for (h, &r) in view.reach[o].iter().enumerate() {
        reach_o[idx[o][h]] = r;
    }
    let ev = solver.node_ev(view.node, p, &reach_o);
    (0..game.num_hands(p)).map(|h| ev[idx[p][h]]).collect()
}
