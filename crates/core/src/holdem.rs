//! NLHE postflop subgames (PRD §3.3). M1 builds river spots; turn and flop (chance nodes,
//! isomorphism) come with M2.
//!
//! Amounts are in bb. Player 0 is OOP and acts first. The starting pot is dead money: both players
//! entered the street with equal stacks (`eff_stack`).

use crate::cards::{Card, hand_class, mask_of};
use crate::eval::evaluate;
use crate::game::{Action, Game, Node};
use crate::range::{Range, live_combos};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rake {
    /// Fraction of the pot, e.g. 0.05.
    pub pct: f64,
    /// Maximum rake in bb.
    pub cap: f64,
}

impl Rake {
    pub const NONE: Rake = Rake { pct: 0.0, cap: 0.0 };

    fn of(&self, pot: f64) -> f64 {
        (pot * self.pct).min(self.cap)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BetSizes {
    /// Bet sizes as fractions of the pot.
    pub bets: Vec<f64>,
    /// Raise to this multiple of the bet faced.
    pub raise_mult: f64,
    /// Raises allowed per street after the first bet.
    pub max_raises: usize,
    /// Offer all-in as a bet (it is always offered as a raise).
    pub bet_allin: bool,
}

impl Default for BetSizes {
    /// PRD §3.3 / §11: 33 / 66 / 100 / 125 % pot, raise 3× plus all-in, up to 3 raises.
    fn default() -> Self {
        BetSizes {
            bets: vec![0.33, 0.66, 1.0, 1.25],
            raise_mult: 3.0,
            max_raises: 3,
            bet_allin: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TreeConfig {
    pub start_pot: f64,
    pub eff_stack: f64,
    pub sizes: BetSizes,
    /// A bet that leaves less than this fraction of the resulting pot behind becomes all-in.
    pub allin_threshold: f64,
    pub rake: Rake,
}

impl TreeConfig {
    pub fn new(start_pot: f64, eff_stack: f64) -> Self {
        TreeConfig {
            start_pot,
            eff_stack,
            sizes: BetSizes::default(),
            allin_threshold: 0.10,
            // GG NL10 as measured in HH Stats Viewer: 5 %, cap 3 bb.
            rake: Rake {
                pct: 0.05,
                cap: 3.0,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct RiverSpot {
    pub board: Vec<Card>,
    pub ranges: [Range; 2],
    pub config: TreeConfig,
}

/// Betting state within a street.
#[derive(Clone, Copy)]
struct State {
    /// Chips each player put in during this subgame.
    contrib: [f64; 2],
    /// Chips each player put in on the current street.
    street: [f64; 2],
    to_act: usize,
    raises: usize,
}

struct Builder<'a> {
    cfg: &'a TreeConfig,
    nodes: Vec<Node>,
}

const EPS: f64 = 1e-9;

impl Builder<'_> {
    fn stack(&self, st: &State, p: usize) -> f64 {
        self.cfg.eff_stack - st.contrib[p]
    }

    fn pot(&self, st: &State) -> f64 {
        self.cfg.start_pot + st.contrib[0] + st.contrib[1]
    }

    fn showdown(&mut self, st: &State) -> usize {
        let pot = self.pot(st);
        self.nodes.push(Node::Showdown {
            board: 0,
            pot,
            contrib: st.contrib,
            rake: self.cfg.rake.of(pot),
        });
        self.nodes.len() - 1
    }

    fn fold(&mut self, st: &State, folder: usize) -> usize {
        let pot = self.pot(st);
        // The uncalled part of the last bet goes back to the bettor before rake is taken.
        let matched = self.cfg.start_pot + 2.0 * st.contrib[0].min(st.contrib[1]);
        self.nodes.push(Node::Fold {
            folder,
            pot,
            contrib: st.contrib,
            rake: self.cfg.rake.of(matched),
        });
        self.nodes.len() - 1
    }

    /// Puts `add` more chips in for the player to act; snaps to all-in when the bet would leave less
    /// than `allin_threshold` × the pot behind. Returns the chips added.
    fn sized(&self, st: &State, add: f64, to_call: f64) -> f64 {
        let p = st.to_act;
        let stack = self.stack(st, p);
        if add >= stack - EPS {
            return stack;
        }
        // Pot if this bet is called.
        let pot_after = self.pot(st) + add + (add - to_call);
        if stack - add < self.cfg.allin_threshold * pot_after {
            stack
        } else {
            add
        }
    }

    fn action(&mut self, st: State) -> usize {
        let p = st.to_act;
        let o = 1 - p;
        let id = self.nodes.len();
        self.nodes.push(Node::Fold {
            folder: 0,
            pot: 0.0,
            contrib: [0.0; 2],
            rake: 0.0,
        }); // placeholder
        let stack = self.stack(&st, p);
        let put = |st: &State, add: f64| {
            let mut s = *st;
            s.contrib[p] += add;
            s.street[p] += add;
            s.to_act = o;
            s
        };

        if st.street[o] > st.street[p] + EPS {
            // Facing a bet or raise. Stacks are equal at the start of a street, so a call never
            // exceeds the caller's stack.
            let to_call = (st.street[o] - st.street[p]).min(stack);
            let mut acts = vec![Action::Fold, Action::Call];
            let mut children = vec![self.fold(&st, p), self.showdown(&put(&st, to_call))];
            let opp_all_in = self.stack(&st, o) <= EPS;
            if st.raises < self.cfg.sizes.max_raises && stack > to_call + EPS && !opp_all_in {
                let raise_to = self.cfg.sizes.raise_mult * st.street[o];
                let mut adds = vec![self.sized(&st, raise_to - st.street[p], to_call), stack];
                adds.dedup_by(|a, b| (*a - *b).abs() < 0.005);
                for add in adds {
                    let mut s = put(&st, add);
                    s.raises += 1;
                    let total = s.street[p];
                    acts.push(if (add - stack).abs() < EPS {
                        Action::AllIn(total)
                    } else {
                        Action::Raise(total)
                    });
                    children.push(self.action(s));
                }
            }
            self.nodes[id] = Node::Action {
                player: p,
                actions: acts,
                children,
            };
            return id;
        }

        // Nobody has bet on this street.
        let mut acts = vec![Action::Check];
        // IP checking behind closes the street.
        let after_check = if p == 1 {
            self.showdown(&st)
        } else {
            let mut s = st;
            s.to_act = o;
            self.action(s)
        };
        let mut children = vec![after_check];
        if stack > EPS {
            let pot = self.pot(&st);
            let mut adds: Vec<f64> = self
                .cfg
                .sizes
                .bets
                .iter()
                .map(|f| self.sized(&st, f * pot, 0.0))
                .collect();
            if self.cfg.sizes.bet_allin {
                adds.push(stack);
            }
            adds.sort_by(|a, b| a.partial_cmp(b).unwrap());
            adds.dedup_by(|a, b| (*a - *b).abs() < 0.005);
            for add in adds {
                let s = put(&st, add);
                let a = if (add - stack).abs() < EPS {
                    Action::AllIn(add)
                } else {
                    Action::Bet(add)
                };
                acts.push(a);
                children.push(self.action(s));
            }
        }
        self.nodes[id] = Node::Action {
            player: p,
            actions: acts,
            children,
        };
        id
    }
}

/// Builds the river game: tree from `config`, hands from the ranges minus board cards.
pub fn build_river(spot: &RiverSpot) -> Result<Game, String> {
    if spot.board.len() != 5 {
        return Err(format!(
            "a river board needs 5 cards, got {}",
            spot.board.len()
        ));
    }
    let dead = mask_of(&spot.board);
    if dead.count_ones() != 5 {
        return Err("board has duplicate cards".into());
    }
    let cfg = &spot.config;
    if !(cfg.start_pot > 0.0 && cfg.eff_stack >= 0.0) {
        return Err("pot must be positive and stack non-negative".into());
    }
    let mut hands = [Vec::new(), Vec::new()];
    let mut weights = [Vec::new(), Vec::new()];
    let mut strengths = [Vec::new(), Vec::new()];
    for p in 0..2 {
        for ((a, b), w) in live_combos(&spot.ranges[p], dead) {
            let mut seven = spot.board.clone();
            seven.extend([a, b]);
            hands[p].push(mask_of(&[a, b]));
            weights[p].push(w);
            strengths[p].push(evaluate(&seven));
        }
        if hands[p].is_empty() {
            return Err(format!("player {p} has no live combos on this board"));
        }
    }

    let mut b = Builder {
        cfg,
        nodes: Vec::new(),
    };
    b.action(State {
        contrib: [0.0; 2],
        street: [0.0; 2],
        to_act: 0,
        raises: 0,
    });
    let game = Game {
        nodes: b.nodes,
        hands,
        weights,
        strengths: vec![strengths],
        start_pot: cfg.start_pot,
    };
    game.validate();
    Ok(game)
}

/// Hand-class label of each hand in `game` for player `p` (e.g. "AKs").
pub fn hand_labels(game: &Game, p: usize) -> Vec<String> {
    game.hands[p]
        .iter()
        .map(|&m| {
            let a = Card::from_index(m.trailing_zeros() as u8);
            let b = Card::from_index(63 - m.leading_zeros() as u8);
            hand_class(a, b)
        })
        .collect()
}
