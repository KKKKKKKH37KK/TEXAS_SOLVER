//! A two-player game tree that the DCFR solver runs on.
//!
//! Nothing here is specific to hold'em: a "hand" is a set of private cards given as a bit mask, so Kuhn
//! poker, Leduc hold'em and NLHE subgames all use the same solver and terminal code (PRD §8.2).
//! Amounts are in chips (bb for hold'em). Player 0 is OOP / first to act.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Fold,
    Check,
    Call,
    /// Bet of this many chips on the current street.
    Bet(f64),
    /// Raise to this street total.
    Raise(f64),
    /// All-in: the actor's street total after the action.
    AllIn(f64),
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Fold => write!(f, "Fold"),
            Action::Check => write!(f, "Check"),
            Action::Call => write!(f, "Call"),
            Action::Bet(x) => write!(f, "Bet {x:.2}"),
            Action::Raise(x) => write!(f, "Raise {x:.2}"),
            Action::AllIn(x) => write!(f, "All-in {x:.2}"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Node {
    Action {
        player: usize,
        actions: Vec<Action>,
        children: Vec<usize>,
    },
    /// Deals one public card. `factor` = 1 / (number of cards that can actually come, given both hands).
    /// `cards` have their own subtrees; each `iso` card reuses the subtree of a suit-isomorphic card
    /// with hands mapped through `Game::swaps`.
    Chance {
        cards: Vec<u8>,
        children: Vec<usize>,
        factor: f32,
        iso: Vec<Iso>,
    },
    /// `pot` = everything in the middle including dead money; `contrib` = what each player put in during
    /// this game; `rake` is taken from the winner.
    Fold {
        folder: usize,
        pot: f64,
        contrib: [f64; 2],
        rake: f64,
    },
    /// `board` indexes `Game::strengths`.
    Showdown {
        board: usize,
        pot: f64,
        contrib: [f64; 2],
        rake: f64,
    },
}

/// A card dealt at a chance node whose subtree is the one of `cards[canon]` with two suits swapped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Iso {
    pub card: u8,
    /// Index into the chance node's `cards` / `children`.
    pub canon: u16,
    /// Index into `Game::swaps`.
    pub swap: u8,
}

#[derive(Clone, Debug)]
pub struct Game {
    /// Root is node 0.
    pub nodes: Vec<Node>,
    /// Private-card masks per player.
    pub hands: [Vec<u64>; 2],
    /// Initial reach (range weight) per hand.
    pub weights: [Vec<f32>; 2],
    /// Hand strength per showdown board, per player, per hand. Larger wins.
    pub strengths: Vec<[Vec<u32>; 2]>,
    /// Card mask of each showdown board (parallel to `strengths`); hands touching it are skipped.
    pub board_masks: Vec<u64>,
    /// Hand permutations used by `Iso`: `swaps[s][p][h]` is the index of hand h of player p with the
    /// two suits of swap s exchanged.
    pub swaps: Vec<[Vec<u32>; 2]>,
    /// Pot at the root, for reporting results as a share of the pot.
    pub start_pot: f64,
}

impl Game {
    pub fn num_hands(&self, player: usize) -> usize {
        self.hands[player].len()
    }

    /// Child reached by taking action `a` at an action node.
    pub fn child(&self, node: usize, a: usize) -> usize {
        match &self.nodes[node] {
            Node::Action { children, .. } | Node::Chance { children, .. } => children[a],
            _ => panic!("node {node} has no children"),
        }
    }

    /// Follows a path of action labels from the root, e.g. ["Check", "Bet 10.00"].
    pub fn find(&self, path: &[&str]) -> Option<usize> {
        let mut n = 0;
        for step in path {
            let Node::Action {
                actions, children, ..
            } = &self.nodes[n]
            else {
                return None;
            };
            let i = actions.iter().position(|a| a.to_string() == *step)?;
            n = children[i];
        }
        Some(n)
    }

    /// Structural checks; panics with a message on the first problem.
    pub fn validate(&self) {
        assert_eq!(self.strengths.len(), self.board_masks.len());
        assert_eq!(self.hands[0].len(), self.weights[0].len());
        assert_eq!(self.hands[1].len(), self.weights[1].len());
        for s in &self.swaps {
            for (table, hands) in s.iter().zip(&self.hands) {
                assert_eq!(table.len(), hands.len(), "swap table size");
            }
        }
        for (i, n) in self.nodes.iter().enumerate() {
            match n {
                Node::Action {
                    player,
                    actions,
                    children,
                } => {
                    assert!(*player < 2, "node {i}: bad player");
                    assert!(
                        !actions.is_empty() && actions.len() == children.len(),
                        "node {i}: bad actions"
                    );
                    assert!(
                        children.iter().all(|&c| c > i),
                        "node {i}: children must come later"
                    );
                }
                Node::Chance {
                    cards,
                    children,
                    factor,
                    iso,
                } => {
                    assert!(
                        cards.len() == children.len() && *factor > 0.0,
                        "node {i}: bad chance node"
                    );
                    for x in iso {
                        assert!(
                            (x.canon as usize) < cards.len()
                                && (x.swap as usize) < self.swaps.len(),
                            "node {i}: bad isomorphic card"
                        );
                    }
                }
                Node::Showdown { board, .. } => {
                    assert!(*board < self.strengths.len(), "node {i}: bad board index");
                }
                Node::Fold { .. } => {}
            }
        }
    }
}
