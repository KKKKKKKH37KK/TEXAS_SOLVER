//! Discounted CFR (Brown & Sandholm 2019) with alternating updates, vectorised over hands (PRD §4.1).
//!
//! Per action node the solver stores regrets and the cumulative (average) strategy as
//! `[action][hand]` f32 arrays for the acting player. Values passed around are counterfactual values:
//! for each traverser hand, Σ over compatible opponent hands of opponent reach × payoff.

use crate::game::{Game, Node};
use crate::terminal::Terminals;

#[derive(Clone, Copy, Debug)]
pub struct DcfrParams {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
}

impl Default for DcfrParams {
    /// α = 1.5, β = 0, γ = 2: the values recommended in the paper.
    fn default() -> Self {
        DcfrParams {
            alpha: 1.5,
            beta: 0.0,
            gamma: 2.0,
        }
    }
}

/// Discount factors for one iteration.
#[derive(Clone, Copy)]
struct Discount {
    pos: f32,
    neg: f32,
    strat: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    /// Expected value per player under the average strategies, in chips per deal.
    pub ev: [f64; 2],
    /// Best-response value per player against the opponent's average strategy.
    pub best_response: [f64; 2],
    /// Mean gain from best responding: ((br0 − ev0) + (br1 − ev1)) / 2, in chips.
    pub exploitability: f64,
    /// `exploitability` as a percentage of the starting pot.
    pub exploitability_pct: f64,
}

pub struct Solver<'g> {
    game: &'g Game,
    terms: Terminals,
    params: DcfrParams,
    /// Offset of each action node's storage (unused for other nodes).
    offset: Vec<usize>,
    regrets: Vec<f32>,
    cum_strategy: Vec<f32>,
    iteration: u32,
    /// Σ over compatible hand pairs of w0·w1: turns counterfactual sums into per-deal values.
    norm: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Average,
    BestResponse,
}

/// Regret matching: positive regrets normalised per hand, uniform when none are positive.
fn regret_match(reg: &[f32], n_act: usize, n_hand: usize) -> Vec<f32> {
    let mut s = vec![0.0f32; n_act * n_hand];
    for h in 0..n_hand {
        let sum: f32 = (0..n_act).map(|a| reg[a * n_hand + h].max(0.0)).sum();
        for a in 0..n_act {
            s[a * n_hand + h] = if sum > 0.0 {
                reg[a * n_hand + h].max(0.0) / sum
            } else {
                1.0 / n_act as f32
            };
        }
    }
    s
}

/// Average strategy from cumulative sums, uniform where a hand never reached the node.
fn normalise(cum: &[f32], n_act: usize, n_hand: usize) -> Vec<f32> {
    let mut s = vec![0.0f32; n_act * n_hand];
    for h in 0..n_hand {
        let sum: f32 = (0..n_act).map(|a| cum[a * n_hand + h]).sum();
        for a in 0..n_act {
            s[a * n_hand + h] = if sum > 0.0 {
                cum[a * n_hand + h] / sum
            } else {
                1.0 / n_act as f32
            };
        }
    }
    s
}

/// Zero the entries of hands that contain `card`.
fn without_card(reach: &[f32], hands: &[u64], card: u8) -> Vec<f32> {
    reach
        .iter()
        .zip(hands)
        .map(|(&r, &m)| if m >> card & 1 == 1 { 0.0 } else { r })
        .collect()
}

impl<'g> Solver<'g> {
    pub fn new(game: &'g Game, params: DcfrParams) -> Self {
        game.validate();
        let mut offset = vec![0; game.nodes.len()];
        let mut size = 0;
        for (i, n) in game.nodes.iter().enumerate() {
            if let Node::Action {
                player, actions, ..
            } = n
            {
                offset[i] = size;
                size += actions.len() * game.num_hands(*player);
            }
        }
        let terms = Terminals::new(game);
        let norm = terms
            .compatible(0, &game.weights[1])
            .iter()
            .zip(&game.weights[0])
            .map(|(c, &w)| c * w as f64)
            .sum();
        Solver {
            game,
            terms,
            params,
            offset,
            regrets: vec![0.0; size],
            cum_strategy: vec![0.0; size],
            iteration: 0,
            norm,
        }
    }

    /// Bytes used by regrets and cumulative strategy (what PRD §5 estimates before solving).
    pub fn memory_bytes(&self) -> usize {
        (self.regrets.len() + self.cum_strategy.len()) * std::mem::size_of::<f32>()
    }

    pub fn iterations(&self) -> u32 {
        self.iteration
    }

    /// Runs one DCFR iteration: a regret update for each player in turn.
    pub fn iterate(&mut self) {
        self.iteration += 1;
        let t = self.iteration as f64;
        let p = &self.params;
        let ta = t.powf(p.alpha);
        let tb = t.powf(p.beta);
        let d = Discount {
            pos: (ta / (ta + 1.0)) as f32,
            neg: (tb / (tb + 1.0)) as f32,
            strat: (t / (t + 1.0)).powf(p.gamma) as f32,
        };
        for trav in 0..2 {
            let reach_t = self.game.weights[trav].clone();
            let reach_o = self.game.weights[1 - trav].clone();
            self.cfr(0, trav, &reach_t, &reach_o, d);
        }
    }

    fn terminal_value(&self, node: &Node, p: usize, reach_o: &[f32]) -> Vec<f32> {
        match *node {
            Node::Fold {
                folder,
                pot,
                contrib,
                rake,
            } => {
                let u = if folder == p {
                    -contrib[p]
                } else {
                    pot - rake - contrib[p]
                };
                self.terms.fold(p, reach_o, u)
            }
            Node::Showdown {
                board,
                pot,
                contrib,
                rake,
            } => {
                let net = pot - rake;
                let payoffs = [net - contrib[p], -contrib[p], net / 2.0 - contrib[p]];
                self.terms
                    .showdown(p, board, &self.game.strengths[board], reach_o, payoffs)
            }
            _ => unreachable!(),
        }
    }

    fn cfr(
        &mut self,
        node: usize,
        trav: usize,
        reach_t: &[f32],
        reach_o: &[f32],
        d: Discount,
    ) -> Vec<f32> {
        let game = self.game;
        match &game.nodes[node] {
            n @ (Node::Fold { .. } | Node::Showdown { .. }) => {
                self.terminal_value(n, trav, reach_o)
            }
            Node::Chance {
                cards,
                children,
                factor,
            } => {
                let mut out = vec![0.0f32; reach_t.len()];
                for (&c, &child) in cards.iter().zip(children) {
                    let rt = without_card(reach_t, &game.hands[trav], c);
                    let ro = without_card(reach_o, &game.hands[1 - trav], c);
                    let v = self.cfr(child, trav, &rt, &ro, d);
                    for (h, x) in v.iter().enumerate() {
                        if game.hands[trav][h] >> c & 1 == 0 {
                            out[h] += x * factor;
                        }
                    }
                }
                out
            }
            Node::Action {
                player,
                actions,
                children,
            } => {
                let (n_act, n_hand) = (actions.len(), game.num_hands(*player));
                let off = self.offset[node];
                let sigma = regret_match(&self.regrets[off..off + n_act * n_hand], n_act, n_hand);

                if *player == trav {
                    let mut value = vec![0.0f32; n_hand];
                    let mut child_vals = Vec::with_capacity(n_act);
                    for (a, &child) in children.iter().enumerate() {
                        let rt: Vec<f32> = (0..n_hand)
                            .map(|h| reach_t[h] * sigma[a * n_hand + h])
                            .collect();
                        let v = self.cfr(child, trav, &rt, reach_o, d);
                        for h in 0..n_hand {
                            value[h] += sigma[a * n_hand + h] * v[h];
                        }
                        child_vals.push(v);
                    }
                    for (a, v) in child_vals.iter().enumerate() {
                        for h in 0..n_hand {
                            let i = off + a * n_hand + h;
                            let r = self.regrets[i];
                            self.regrets[i] =
                                r * if r > 0.0 { d.pos } else { d.neg } + (v[h] - value[h]);
                            self.cum_strategy[i] =
                                self.cum_strategy[i] * d.strat + reach_t[h] * sigma[a * n_hand + h];
                        }
                    }
                    value
                } else {
                    let mut value = vec![0.0f32; reach_t.len()];
                    for (a, &child) in children.iter().enumerate() {
                        let ro: Vec<f32> = (0..n_hand)
                            .map(|h| reach_o[h] * sigma[a * n_hand + h])
                            .collect();
                        let v = self.cfr(child, trav, reach_t, &ro, d);
                        for (x, y) in value.iter_mut().zip(&v) {
                            *x += y;
                        }
                    }
                    value
                }
            }
        }
    }

    /// Average strategy at an action node as `[action][hand]`.
    pub fn strategy(&self, node: usize) -> Vec<f32> {
        let Node::Action {
            player, actions, ..
        } = &self.game.nodes[node]
        else {
            panic!("node {node} is not an action node");
        };
        let (n_act, n_hand) = (actions.len(), self.game.num_hands(*player));
        let off = self.offset[node];
        normalise(&self.cum_strategy[off..off + n_act * n_hand], n_act, n_hand)
    }

    /// Counterfactual values for player `p` with both players on the average strategy (or `p` best
    /// responding).
    fn values(&self, node: usize, p: usize, reach_o: &[f32], mode: Mode) -> Vec<f32> {
        let game = self.game;
        match &game.nodes[node] {
            n @ (Node::Fold { .. } | Node::Showdown { .. }) => self.terminal_value(n, p, reach_o),
            Node::Chance {
                cards,
                children,
                factor,
            } => {
                let mut out = vec![0.0f32; game.num_hands(p)];
                for (&c, &child) in cards.iter().zip(children) {
                    let ro = without_card(reach_o, &game.hands[1 - p], c);
                    let v = self.values(child, p, &ro, mode);
                    for (h, x) in v.iter().enumerate() {
                        if game.hands[p][h] >> c & 1 == 0 {
                            out[h] += x * factor;
                        }
                    }
                }
                out
            }
            Node::Action {
                player, children, ..
            } => {
                let sigma = self.strategy(node);
                let n_hand = game.num_hands(*player);
                if *player == p {
                    let mut out = vec![
                        if mode == Mode::BestResponse {
                            f32::NEG_INFINITY
                        } else {
                            0.0
                        };
                        n_hand
                    ];
                    for (a, &child) in children.iter().enumerate() {
                        let v = self.values(child, p, reach_o, mode);
                        for h in 0..n_hand {
                            match mode {
                                Mode::BestResponse => out[h] = out[h].max(v[h]),
                                Mode::Average => out[h] += sigma[a * n_hand + h] * v[h],
                            }
                        }
                    }
                    out
                } else {
                    let mut out = vec![0.0f32; game.num_hands(p)];
                    for (a, &child) in children.iter().enumerate() {
                        let ro: Vec<f32> = (0..n_hand)
                            .map(|h| reach_o[h] * sigma[a * n_hand + h])
                            .collect();
                        let v = self.values(child, p, &ro, mode);
                        for (x, y) in out.iter_mut().zip(&v) {
                            *x += y;
                        }
                    }
                    out
                }
            }
        }
    }

    fn total(&self, p: usize, mode: Mode) -> f64 {
        let v = self.values(0, p, &self.game.weights[1 - p], mode);
        v.iter()
            .zip(&self.game.weights[p])
            .map(|(&x, &w)| x as f64 * w as f64)
            .sum::<f64>()
            / self.norm
    }

    /// Counterfactual EV per hand of player `p` at the root under the average strategies, divided by
    /// the compatible opponent weight, i.e. the hand's EV in chips.
    pub fn hand_ev(&self, p: usize) -> Vec<f64> {
        let v = self.values(0, p, &self.game.weights[1 - p], Mode::Average);
        let c = self.terms.compatible(p, &self.game.weights[1 - p]);
        v.iter()
            .zip(c)
            .map(|(&x, c)| if c > 0.0 { x as f64 / c } else { 0.0 })
            .collect()
    }

    pub fn report(&self) -> Report {
        let ev = [self.total(0, Mode::Average), self.total(1, Mode::Average)];
        let br = [
            self.total(0, Mode::BestResponse),
            self.total(1, Mode::BestResponse),
        ];
        let exploitability = ((br[0] - ev[0]) + (br[1] - ev[1])) / 2.0;
        Report {
            ev,
            best_response: br,
            exploitability,
            exploitability_pct: exploitability / self.game.start_pot * 100.0,
        }
    }
}
