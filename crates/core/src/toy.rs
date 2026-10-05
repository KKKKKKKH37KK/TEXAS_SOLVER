//! Small games with known solutions, used to validate the solver (PRD §8.2).
//!
//! Both are fixed-limit games where each player holds one card:
//! - Kuhn poker: J, Q, K; ante 1; one betting round with bet 1 and no raises. Value for player 0 = −1/18.
//! - Leduc hold'em: J J Q Q K K; ante 1; bets 2 then 4; at most 2 bets per round; one public card
//!   between rounds; pairing the board wins, otherwise the higher card.

use crate::game::{Action, Game, Node};

struct Limit {
    /// Bet size per round.
    bets: Vec<f64>,
    max_bets: usize,
    ante: f64,
    /// Cards that can be dealt as the public card between rounds (empty for one round).
    public: Vec<u8>,
    /// Cards a public card can be, given both private cards.
    chance_count: usize,
    nodes: Vec<Node>,
}

#[derive(Clone, Copy)]
struct State {
    round: usize,
    bets: usize,
    contrib: [f64; 2],
    to_act: usize,
    /// Showdown board index (the public card for Leduc).
    board: usize,
}

impl Limit {
    fn push(&mut self, n: Node) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    /// Betting round finished: deal the next card or show down.
    fn end_round(&mut self, st: State) -> usize {
        if st.round + 1 == self.bets.len() {
            let pot = st.contrib[0] + st.contrib[1];
            return self.push(Node::Showdown {
                board: st.board,
                pot,
                contrib: st.contrib,
                rake: 0.0,
            });
        }
        let id = self.push(Node::Fold {
            folder: 0,
            pot: 0.0,
            contrib: [0.0; 2],
            rake: 0.0,
        });
        let cards = self.public.clone();
        let children = cards
            .iter()
            .map(|&c| {
                let s = State {
                    round: st.round + 1,
                    bets: 0,
                    to_act: 0,
                    board: c as usize,
                    ..st
                };
                self.action(s)
            })
            .collect();
        self.nodes[id] = Node::Chance {
            cards,
            children,
            factor: 1.0 / self.chance_count as f32,
            iso: vec![],
        };
        id
    }

    fn action(&mut self, st: State) -> usize {
        let (p, o) = (st.to_act, 1 - st.to_act);
        let id = self.push(Node::Fold {
            folder: 0,
            pot: 0.0,
            contrib: [0.0; 2],
            rake: 0.0,
        });
        let size = self.bets[st.round];
        let mut acts = Vec::new();
        let mut children = Vec::new();
        let raised = |st: &State| {
            let mut s = *st;
            s.contrib[p] = s.contrib[o] + size;
            s.bets += 1;
            s.to_act = o;
            s
        };
        if st.contrib[o] > st.contrib[p] {
            let pot = st.contrib[0] + st.contrib[1];
            acts.push(Action::Fold);
            children.push(self.push(Node::Fold {
                folder: p,
                pot,
                contrib: st.contrib,
                rake: 0.0,
            }));
            acts.push(Action::Call);
            let mut s = st;
            s.contrib[p] = s.contrib[o];
            children.push(self.end_round(s));
            if st.bets < self.max_bets {
                acts.push(Action::Raise(size));
                children.push(self.action(raised(&st)));
            }
        } else {
            acts.push(Action::Check);
            children.push(if p == 1 {
                self.end_round(st)
            } else {
                self.action(State { to_act: o, ..st })
            });
            if st.bets < self.max_bets {
                acts.push(Action::Bet(size));
                children.push(self.action(raised(&st)));
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

fn build(mut g: Limit, n_cards: u8, strengths: Vec<[Vec<u32>; 2]>, board_masks: Vec<u64>) -> Game {
    let start = State {
        round: 0,
        bets: 0,
        contrib: [g.ante; 2],
        to_act: 0,
        board: 0,
    };
    g.action(start);
    let hands: Vec<u64> = (0..n_cards).map(|c| 1u64 << c).collect();
    let game = Game {
        nodes: g.nodes,
        weights: [vec![1.0; hands.len()], vec![1.0; hands.len()]],
        hands: [hands.clone(), hands],
        strengths,
        board_masks,
        swaps: vec![],
        start_pot: 2.0 * g.ante,
        eff_stack: 0.0,
        root_board: 0,
    };
    game.validate();
    game
}

pub fn kuhn() -> Game {
    let g = Limit {
        bets: vec![1.0],
        max_bets: 1,
        ante: 1.0,
        public: vec![],
        chance_count: 1,
        nodes: vec![],
    };
    let s: Vec<u32> = vec![0, 1, 2];
    build(g, 3, vec![[s.clone(), s]], vec![0])
}

pub fn leduc() -> Game {
    // Card i has rank i / 2: J J Q Q K K. Two private cards are out, so 4 public cards remain.
    let g = Limit {
        bets: vec![2.0, 4.0],
        max_bets: 2,
        ante: 1.0,
        public: (0..6).collect(),
        chance_count: 4,
        nodes: vec![],
    };
    let strengths = (0..6u32)
        .map(|board| {
            let s: Vec<u32> = (0..6u32)
                .map(|h| {
                    if h / 2 == board / 2 {
                        10 + h / 2
                    } else {
                        h / 2
                    }
                })
                .collect();
            [s.clone(), s]
        })
        .collect();
    // Board index = the public card.
    build(g, 6, strengths, (0..6).map(|c| 1u64 << c).collect())
}
