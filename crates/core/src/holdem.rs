//! NLHE heads-up postflop subgames from the flop, turn or river (PRD §3.3).
//!
//! Amounts are in bb. Player 0 is OOP and acts first on every street. The starting pot is dead money:
//! both players enter with equal stacks (`eff_stack`). Between streets a chance node deals every card
//! not on the board; hands that hold the card get zero reach in that branch (see `solver`).

use crate::cards::{Card, all_combos, combo_index, hand_class, mask_of};
use crate::eval::evaluate;
use crate::game::{Action, Game, Iso, Node};
use crate::range::{Range, live_combos};
use std::collections::HashMap;

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
    /// Sizes for the flop, turn and river.
    pub sizes: [BetSizes; 3],
    /// Whether OOP may bet first on the flop (a donk bet when OOP is not the preflop aggressor).
    /// PRD §11: on by default.
    pub oop_flop_bets: bool,
    /// A bet that leaves less than this fraction of the resulting pot behind becomes all-in.
    pub allin_threshold: f64,
    pub rake: Rake,
    /// Share subtrees between suit-isomorphic turn / river cards (PRD §4.1). Only applies when both
    /// ranges are symmetric in the suits involved.
    pub isomorphism: bool,
}

impl TreeConfig {
    pub fn new(start_pot: f64, eff_stack: f64) -> Self {
        TreeConfig {
            start_pot,
            eff_stack,
            sizes: [
                BetSizes::default(),
                BetSizes::default(),
                BetSizes::default(),
            ],
            oop_flop_bets: true,
            allin_threshold: 0.10,
            // GG NL10 as measured in HH Stats Viewer: 5 %, cap 3 bb.
            rake: Rake {
                pct: 0.05,
                cap: 3.0,
            },
            isomorphism: true,
        }
    }

    /// Default tree for a spot starting on the flop (PRD §3.3 / §5.1): flop 33/66/100/125 without a
    /// donk bet, turn and river 66/125, one raise per street.
    pub fn flop_default(start_pot: f64, eff_stack: f64) -> Self {
        let flop = BetSizes {
            max_raises: 1,
            ..BetSizes::default()
        };
        let later = BetSizes {
            bets: vec![0.66, 1.25],
            ..flop.clone()
        };
        let mut c = TreeConfig::new(start_pot, eff_stack);
        c.sizes = [flop, later.clone(), later];
        c.oop_flop_bets = false;
        c
    }

    /// The default for a board of `board_len` cards: `flop_default` on the flop, `new` otherwise.
    pub fn preset(start_pot: f64, eff_stack: f64, board_len: usize) -> Self {
        if board_len == 3 {
            Self::flop_default(start_pot, eff_stack)
        } else {
            Self::new(start_pot, eff_stack)
        }
    }

    /// Same sizes on every street.
    pub fn with_sizes(mut self, sizes: BetSizes) -> Self {
        self.sizes = [sizes.clone(), sizes.clone(), sizes];
        self
    }
}

#[derive(Clone, Debug)]
pub struct Spot {
    /// Three, four or five cards: the street the subgame starts on.
    pub board: Vec<Card>,
    pub ranges: [Range; 2],
    pub config: TreeConfig,
}

/// Betting state.
#[derive(Clone, Copy)]
struct State {
    /// Chips each player put in during this subgame.
    contrib: [f64; 2],
    /// Chips each player put in on the current street.
    street: [f64; 2],
    to_act: usize,
    raises: usize,
    /// Cards on the board.
    board: u64,
}

impl State {
    fn board_len(&self) -> u32 {
        self.board.count_ones()
    }

    /// 0 = flop, 1 = turn, 2 = river.
    fn street_index(&self) -> usize {
        self.board_len() as usize - 3
    }

    /// Fresh betting round after a card is dealt.
    fn next_street(&self, card: u8) -> State {
        State {
            contrib: self.contrib,
            street: [0.0; 2],
            to_act: 0,
            raises: 0,
            board: self.board | 1 << card,
        }
    }
}

/// Where an action leads.
enum Next {
    Fold,
    /// The betting round is over (call, or check behind).
    Close(State),
    Act(State),
}

const EPS: f64 = 1e-9;

/// The six suit transpositions, indexed as in `Game::swaps`.
pub const SUIT_PAIRS: [(u8, u8); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

pub fn swap_card(c: u8, (a, b): (u8, u8)) -> u8 {
    let s = c % 4;
    if s == a {
        c - a + b
    } else if s == b {
        c - b + a
    } else {
        c
    }
}

pub fn swap_mask(mut m: u64, pair: (u8, u8)) -> u64 {
    let mut out = 0;
    while m != 0 {
        let c = m.trailing_zeros() as u8;
        m &= m - 1;
        out |= 1 << swap_card(c, pair);
    }
    out
}

/// Ranks of `suit` present in a board mask.
fn suit_ranks(board: u64, suit: u8) -> u16 {
    (0..13u8).fold(0, |acc, r| {
        if board >> (r * 4 + suit) & 1 == 1 {
            acc | 1 << r
        } else {
            acc
        }
    })
}

/// For each suit pair: are both ranges unchanged when the two suits are exchanged?
fn range_symmetry(ranges: &[Range; 2]) -> [bool; 6] {
    SUIT_PAIRS.map(|pair| {
        all_combos().into_iter().enumerate().all(|(i, (a, b))| {
            let j = combo_index(
                Card::from_index(swap_card(a.index(), pair)),
                Card::from_index(swap_card(b.index(), pair)),
            );
            ranges.iter().all(|r| r.weights[i] == r.weights[j])
        })
    })
}

/// Sizing rules shared by the tree builder and the size estimate.
struct Rules<'a> {
    cfg: &'a TreeConfig,
    start_street: usize,
    /// Suit pairs the ranges are symmetric in (all false when isomorphism is off).
    sym: [bool; 6],
}

impl Rules<'_> {
    fn stack(&self, st: &State, p: usize) -> f64 {
        self.cfg.eff_stack - st.contrib[p]
    }

    fn pot(&self, st: &State) -> f64 {
        self.cfg.start_pot + st.contrib[0] + st.contrib[1]
    }

    fn all_in(&self, st: &State) -> bool {
        self.stack(st, 0) <= EPS || self.stack(st, 1) <= EPS
    }

    /// Chips to add for a bet of `add`; snaps to all-in when the bet would leave less than
    /// `allin_threshold` × the pot (after a call) behind.
    fn sized(&self, st: &State, add: f64, to_call: f64) -> f64 {
        let stack = self.stack(st, st.to_act);
        if add >= stack - EPS {
            return stack;
        }
        let pot_after = self.pot(st) + add + (add - to_call);
        if stack - add < self.cfg.allin_threshold * pot_after {
            stack
        } else {
            add
        }
    }

    fn options(&self, st: &State) -> Vec<(Action, Next)> {
        let (p, o) = (st.to_act, 1 - st.to_act);
        let sizes = &self.cfg.sizes[st.street_index()];
        let stack = self.stack(st, p);
        let put = |add: f64| {
            let mut s = *st;
            s.contrib[p] += add;
            s.street[p] += add;
            s.to_act = o;
            s
        };
        let mut out = Vec::new();

        if st.street[o] > st.street[p] + EPS {
            // Facing a bet or raise. Stacks are equal at the start of a street, so a call never
            // exceeds the caller's stack.
            let to_call = (st.street[o] - st.street[p]).min(stack);
            out.push((Action::Fold, Next::Fold));
            out.push((Action::Call, Next::Close(put(to_call))));
            let opp_all_in = self.stack(st, o) <= EPS;
            if st.raises < sizes.max_raises && stack > to_call + EPS && !opp_all_in {
                let raise_to = sizes.raise_mult * st.street[o];
                let mut adds = vec![self.sized(st, raise_to - st.street[p], to_call), stack];
                adds.dedup_by(|a, b| (*a - *b).abs() < 0.005);
                for add in adds {
                    let mut s = put(add);
                    s.raises += 1;
                    let total = s.street[p];
                    let a = if (add - stack).abs() < EPS {
                        Action::AllIn(total)
                    } else {
                        Action::Raise(total)
                    };
                    out.push((a, Next::Act(s)));
                }
            }
            return out;
        }

        // Nobody has bet on this street; IP checking behind closes it.
        let check = if p == 1 {
            Next::Close(*st)
        } else {
            Next::Act(State { to_act: o, ..*st })
        };
        out.push((Action::Check, check));
        let flop_donk_blocked =
            p == 0 && st.street_index() == 0 && self.start_street == 0 && !self.cfg.oop_flop_bets;
        if stack > EPS && !flop_donk_blocked {
            let pot = self.pot(st);
            let mut adds: Vec<f64> = sizes
                .bets
                .iter()
                .map(|f| self.sized(st, f * pot, 0.0))
                .collect();
            if sizes.bet_allin {
                adds.push(stack);
            }
            adds.sort_by(|a, b| a.partial_cmp(b).unwrap());
            adds.dedup_by(|a, b| (*a - *b).abs() < 0.005);
            for add in adds {
                let a = if (add - stack).abs() < EPS {
                    Action::AllIn(add)
                } else {
                    Action::Bet(add)
                };
                out.push((a, Next::Act(put(add))));
            }
        }
        out
    }
}

/// Tree size without building it (PRD §5). Betting is the same after every card, so a chance node
/// counts as (cards that can come) × one child.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TreeSize {
    pub action_nodes: u64,
    pub chance_nodes: u64,
    pub terminal_nodes: u64,
    /// Σ actions over each player's decision nodes; storage is this × the player's hand count.
    pub action_slots: [u64; 2],
}

impl TreeSize {
    fn add(&mut self, o: &TreeSize, times: u64) {
        self.action_nodes += o.action_nodes * times;
        self.chance_nodes += o.chance_nodes * times;
        self.terminal_nodes += o.terminal_nodes * times;
        self.action_slots[0] += o.action_slots[0] * times;
        self.action_slots[1] += o.action_slots[1] * times;
    }

    pub fn nodes(&self) -> u64 {
        self.action_nodes + self.chance_nodes + self.terminal_nodes
    }

    /// Regrets plus cumulative strategy as f32 for the given hand counts.
    pub fn solver_bytes(&self, hands: [usize; 2]) -> u64 {
        8 * (self.action_slots[0] * hands[0] as u64 + self.action_slots[1] * hands[1] as u64)
    }
}

impl Rules<'_> {
    fn count_action(&self, st: &State) -> TreeSize {
        let mut t = TreeSize {
            action_nodes: 1,
            ..Default::default()
        };
        let opts = self.options(st);
        t.action_slots[st.to_act] += opts.len() as u64;
        for (_, next) in &opts {
            let child = match next {
                Next::Fold => TreeSize {
                    terminal_nodes: 1,
                    ..Default::default()
                },
                Next::Close(s) => self.count_close(s),
                Next::Act(s) => self.count_action(s),
            };
            t.add(&child, 1);
        }
        t
    }

    fn count_close(&self, st: &State) -> TreeSize {
        if st.board_len() == 5 {
            return TreeSize {
                terminal_nodes: 1,
                ..Default::default()
            };
        }
        let mut t = TreeSize {
            chance_nodes: 1,
            ..Default::default()
        };
        // Betting is the same after every card, but the board (and so the isomorphism of the next
        // chance node) is not, so only the last street can be multiplied out.
        let (canon, _) = self.deal(st.board);
        if st.board_len() == 4 {
            let s = st.next_street(canon[0]);
            let child = if self.all_in(&s) {
                self.count_close(&s)
            } else {
                self.count_action(&s)
            };
            t.add(&child, canon.len() as u64);
        } else {
            for c in canon {
                let s = st.next_street(c);
                let child = if self.all_in(&s) {
                    self.count_close(&s)
                } else {
                    self.count_action(&s)
                };
                t.add(&child, 1);
            }
        }
        t
    }

    /// Cards that can be dealt on `board`: those with their own subtree, and (card, canonical card,
    /// suit pair) for cards that are a suit swap of a canonical one. Two suits are interchangeable
    /// when both ranges are symmetric in them and the board holds the same ranks in each.
    fn deal(&self, board: u64) -> (Vec<u8>, Vec<(u8, u8, u8)>) {
        let mut canon = Vec::new();
        let mut iso = Vec::new();
        for c in (0..52u8).filter(|&c| board >> c & 1 == 0) {
            let (r, s) = (c / 4, c % 4);
            let twin = (0..s).find_map(|s2| {
                let pair = SUIT_PAIRS.iter().position(|&p| p == (s2, s)).unwrap();
                (self.sym[pair] && suit_ranks(board, s2) == suit_ranks(board, s))
                    .then_some((r * 4 + s2, pair as u8))
            });
            match twin {
                Some((cc, pair)) => iso.push((c, cc, pair)),
                None => canon.push(c),
            }
        }
        (canon, iso)
    }
}

struct Builder<'a> {
    rules: Rules<'a>,
    nodes: Vec<Node>,
    /// Final 5-card board mask → index into `Game::strengths`.
    boards: HashMap<u64, usize>,
    board_list: Vec<u64>,
}

impl Builder<'_> {
    fn push(&mut self, n: Node) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    fn placeholder(&mut self) -> usize {
        self.push(Node::Fold {
            folder: 0,
            pot: 0.0,
            contrib: [0.0; 2],
            rake: 0.0,
        })
    }

    fn fold(&mut self, st: &State, folder: usize) -> usize {
        let r = &self.rules;
        let pot = r.pot(st);
        // The uncalled part of the last bet goes back to the bettor before rake is taken.
        let matched = r.cfg.start_pot + 2.0 * st.contrib[0].min(st.contrib[1]);
        let rake = r.cfg.rake.of(matched);
        self.push(Node::Fold {
            folder,
            pot,
            contrib: st.contrib,
            rake,
        })
    }

    fn close(&mut self, st: &State) -> usize {
        if st.board_len() == 5 {
            let next = self.board_list.len();
            let board = *self.boards.entry(st.board).or_insert(next);
            if board == next {
                self.board_list.push(st.board);
            }
            let pot = self.rules.pot(st);
            let rake = self.rules.cfg.rake.of(pot);
            return self.push(Node::Showdown {
                board,
                pot,
                contrib: st.contrib,
                rake,
            });
        }
        let id = self.placeholder();
        let (cards, twins) = self.rules.deal(st.board);
        let mut children = Vec::with_capacity(cards.len());
        for &c in &cards {
            let s = st.next_street(c);
            children.push(if self.rules.all_in(&s) {
                self.close(&s)
            } else {
                self.action(&s)
            });
        }
        let iso = twins
            .into_iter()
            .map(|(card, cc, pair)| Iso {
                card,
                canon: cards.iter().position(|&x| x == cc).unwrap() as u16,
                swap: pair,
            })
            .collect::<Vec<_>>();
        // Both players' four hole cards are also out of the deck.
        let factor = 1.0 / (cards.len() + iso.len() - 4) as f32;
        self.nodes[id] = Node::Chance {
            cards,
            children,
            factor,
            iso,
        };
        id
    }

    fn action(&mut self, st: &State) -> usize {
        let id = self.placeholder();
        let opts = self.rules.options(st);
        let mut actions = Vec::with_capacity(opts.len());
        let mut children = Vec::with_capacity(opts.len());
        for (a, next) in opts {
            actions.push(a);
            children.push(match next {
                Next::Fold => self.fold(st, st.to_act),
                Next::Close(s) => self.close(&s),
                Next::Act(s) => self.action(&s),
            });
        }
        self.nodes[id] = Node::Action {
            player: st.to_act,
            actions,
            children,
        };
        id
    }
}

/// Checks the spot and returns the board mask and each player's live combos.
#[allow(clippy::type_complexity)]
fn prepare(spot: &Spot) -> Result<(u64, [Vec<((Card, Card), f32)>; 2]), String> {
    let n = spot.board.len();
    if !(3..=5).contains(&n) {
        return Err(format!("the board needs 3 to 5 cards, got {n}"));
    }
    let dead = mask_of(&spot.board);
    if dead.count_ones() as usize != n {
        return Err("board has duplicate cards".into());
    }
    let cfg = &spot.config;
    if !(cfg.start_pot > 0.0 && cfg.eff_stack >= 0.0) {
        return Err("pot must be positive and stack non-negative".into());
    }
    let live = [0, 1].map(|p| live_combos(&spot.ranges[p], dead));
    for (p, l) in live.iter().enumerate() {
        if l.is_empty() {
            return Err(format!("player {p} has no live combos on this board"));
        }
    }
    Ok((dead, live))
}

fn root_state(board: u64) -> State {
    State {
        contrib: [0.0; 2],
        street: [0.0; 2],
        to_act: 0,
        raises: 0,
        board,
    }
}

fn rules_of(spot: &Spot) -> Rules<'_> {
    Rules {
        cfg: &spot.config,
        start_street: spot.board.len() - 3,
        sym: if spot.config.isomorphism {
            range_symmetry(&spot.ranges)
        } else {
            [false; 6]
        },
    }
}

/// Tree size and hand counts of a spot, without building it.
pub fn estimate(spot: &Spot) -> Result<(TreeSize, [usize; 2]), String> {
    let (board, live) = prepare(spot)?;
    let rules = rules_of(spot);
    Ok((
        rules.count_action(&root_state(board)),
        [live[0].len(), live[1].len()],
    ))
}

/// Builds the game: tree from the config, hands from the ranges minus board cards, and hand
/// strengths for every possible final board.
pub fn build(spot: &Spot) -> Result<Game, String> {
    let (board, live) = prepare(spot)?;
    let mut b = Builder {
        rules: rules_of(spot),
        nodes: Vec::new(),
        boards: HashMap::new(),
        board_list: Vec::new(),
    };
    b.action(&root_state(board));

    let hands = [0, 1].map(|p| {
        live[p]
            .iter()
            .map(|&((a, c), _)| mask_of(&[a, c]))
            .collect::<Vec<_>>()
    });
    let weights = [0, 1].map(|p| live[p].iter().map(|&(_, w)| w).collect::<Vec<_>>());
    // Hands that share a card with the final board never reach its showdowns; give them 0.
    let strengths = b
        .board_list
        .iter()
        .map(|&bm| {
            let board_cards: Vec<Card> = (0..52u8)
                .filter(|&c| bm >> c & 1 == 1)
                .map(Card::from_index)
                .collect();
            [0, 1].map(|p| {
                live[p]
                    .iter()
                    .map(|&((x, y), _)| {
                        if bm & mask_of(&[x, y]) != 0 {
                            return 0;
                        }
                        let mut seven = board_cards.clone();
                        seven.extend([x, y]);
                        evaluate(&seven)
                    })
                    .collect::<Vec<u32>>()
            })
        })
        .collect();

    // Hand permutation for every suit pair the ranges are symmetric in. A pair is used at a chance
    // node whose board it maps onto itself; that board may not be the starting one (a turn 2h makes
    // c and h interchangeable on Ks7d2c), so a swapped hand can be missing from the hand list. Such a
    // hand holds a card of the current board (2h here), is skipped where the permutation is used, and
    // maps to itself. Pairs the ranges are not symmetric in are never used and keep the identity.
    let swaps = SUIT_PAIRS
        .iter()
        .enumerate()
        .map(|(i, &pair)| {
            let usable = b.rules.sym[i];
            [0, 1].map(|p| {
                let index: HashMap<u64, u32> = hands[p]
                    .iter()
                    .enumerate()
                    .map(|(h, &m)| (m, h as u32))
                    .collect();
                hands[p]
                    .iter()
                    .enumerate()
                    .map(|(h, &m)| {
                        if usable {
                            index.get(&swap_mask(m, pair)).copied().unwrap_or(h as u32)
                        } else {
                            h as u32
                        }
                    })
                    .collect::<Vec<u32>>()
            })
        })
        .collect();

    let game = Game {
        nodes: b.nodes,
        hands,
        weights,
        strengths,
        board_masks: b.board_list,
        swaps,
        start_pot: spot.config.start_pot,
        eff_stack: spot.config.eff_stack,
        root_board: board,
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
