//! 6-max preflop game and its CFR solver (PRD §3.2, §4.2).
//!
//! Simplifications (all from the PRD, all approximations):
//! - no limps; the first call ends the betting and everyone still to act folds, so at most two
//!   players see a flop (no squeezes, no multiway pots);
//! - a flop is valued by an equity-realisation model instead of postflop play;
//! - card removal only between the two players in a showdown, normalised so it averages to one;
//!   the cards of folded players are ignored.
//!
//! Strategies and values are vectors over the 169 hand classes. Rake is taken only from pots that
//! see a flop (no flop, no drop).

use super::classes::{NUM_CLASSES, combo_count, compatible_pairs};
use super::equity;

pub const NUM_PLAYERS: usize = 6;
pub const POSITIONS: [&str; NUM_PLAYERS] = ["UTG", "HJ", "CO", "BTN", "SB", "BB"];
/// Postflop acting order (higher acts later, i.e. in position): SB, BB, UTG, HJ, CO, BTN.
const POSTFLOP_ORDER: [usize; NUM_PLAYERS] = [2, 3, 4, 5, 0, 1];

#[derive(Clone, Debug, PartialEq)]
pub struct PreflopConfig {
    /// Stack of every player in bb, blinds included.
    pub stack: f64,
    /// Open raise to, from UTG..BTN and from the SB.
    pub open: f64,
    pub sb_open: f64,
    /// 3-bet to this multiple of the open: in position / out of position against the opener.
    pub threebet_ip: f64,
    pub threebet_oop: f64,
    /// 4-bet to this multiple of the 3-bet. The next raise is all-in.
    pub fourbet: f64,
    /// A raise to more than this fraction of the stack becomes all-in.
    pub allin_fraction: f64,
    pub rake_pct: f64,
    pub rake_cap: f64,
    /// Equity realisation when a flop is seen, per situation.
    pub realization: Realization,
}

/// Pot type of a flop: single-raised, 3-bet, 4-bet (or more).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PotType {
    Srp = 0,
    ThreeBet = 1,
    FourBet = 2,
}

impl PotType {
    /// From the number of raises before the call (1 = open).
    pub fn from_level(level: usize) -> PotType {
        match level {
            0 | 1 => PotType::Srp,
            2 => PotType::ThreeBet,
            _ => PotType::FourBet,
        }
    }
}

/// k = R_IP / R_OOP by situation; only the ratio matters to the share model
/// share_IP = eq·k / (eq·k + (1 − eq)). Indexed `[pot type][aggressor is in position]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Realization {
    pub k: [[f64; 2]; 3],
}

impl Realization {
    pub fn k(&self, pot: PotType, aggressor_ip: bool) -> f64 {
        self.k[pot as usize][aggressor_ip as usize]
    }
}

/// Fixed realisation table (PRD §4.2, M6), written by `hexas calibrate` from postflop solves of
/// the library lines: SRP with the aggressor in position (BTN vs BB), 3-bet pots with the aggressor
/// out of position (BB vs BTN) and in position (BTN vs CO). Situations without a calibrated line
/// borrow the nearest one: a single-raised pot with the aggressor out of position (SB vs BB) uses
/// the 3-bet value; 4-bet pots use the 3-bet values.
pub const REALIZATION: Realization = Realization {
    //    aggressor OOP, aggressor IP
    k: [
        [1.176, 1.176], // SRP
        [1.176, 1.176], // 3-bet pot
        [1.176, 1.176], // 4-bet pot
    ],
};

impl Default for PreflopConfig {
    /// PRD §3.2 / §11.
    fn default() -> Self {
        PreflopConfig {
            stack: 100.0,
            open: 2.5,
            sb_open: 3.0,
            threebet_ip: 3.0,
            threebet_oop: 4.0,
            fourbet: 2.3,
            allin_fraction: 0.4,
            rake_pct: 0.05,
            rake_cap: 3.0,
            realization: REALIZATION,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PAction {
    Fold,
    Call,
    /// Raise to this many bb.
    Raise(f64),
    AllIn(f64),
}

impl std::fmt::Display for PAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PAction::Fold => write!(f, "Fold"),
            PAction::Call => write!(f, "Call"),
            PAction::Raise(x) => write!(f, "Raise {x:.1}"),
            PAction::AllIn(x) => write!(f, "All-in {x:.0}"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum PNode {
    Act {
        player: usize,
        actions: Vec<PAction>,
        children: Vec<usize>,
    },
    /// Everyone else folded.
    Fold {
        winner: usize,
        contrib: [f64; NUM_PLAYERS],
    },
    /// Two players all-in: equity decides.
    AllIn {
        players: [usize; 2],
        contrib: [f64; NUM_PLAYERS],
    },
    /// Two players see a flop: realisation model. `players` = [out of position, in position].
    Flop {
        players: [usize; 2],
        contrib: [f64; NUM_PLAYERS],
        pot: PotType,
        /// The last raiser, whom the other player called.
        aggressor: usize,
    },
}

#[derive(Clone, Copy)]
struct State {
    contrib: [f64; NUM_PLAYERS],
    folded: [bool; NUM_PLAYERS],
    /// Raises so far: 0 unopened, 1 open, 2 3-bet, 3 4-bet, 4 all-in.
    level: usize,
    aggressor: Option<usize>,
    to_act: usize,
}

pub struct PreflopGame {
    pub config: PreflopConfig,
    pub nodes: Vec<PNode>,
    eq: Vec<f32>,
    /// Card-removal factor between two classes, averaging one over all pairs.
    removal: Vec<f32>,
}

fn is_ip(a: usize, b: usize) -> bool {
    POSTFLOP_ORDER[a] > POSTFLOP_ORDER[b]
}

impl PreflopGame {
    pub fn new(config: PreflopConfig) -> Self {
        let w = compatible_pairs();
        let scale = 1326.0 / 1225.0;
        let removal = (0..NUM_CLASSES * NUM_CLASSES)
            .map(|k| {
                let (i, j) = (k / NUM_CLASSES, k % NUM_CLASSES);
                w[k] / (combo_count(i) * combo_count(j)) as f32 * scale as f32
            })
            .collect();
        let mut g = PreflopGame {
            config,
            nodes: Vec::new(),
            eq: equity::table(),
            removal,
        };
        let mut contrib = [0.0; NUM_PLAYERS];
        contrib[4] = 0.5;
        contrib[5] = 1.0;
        g.build(State {
            contrib,
            folded: [false; NUM_PLAYERS],
            level: 0,
            aggressor: None,
            to_act: 0,
        });
        g
    }

    fn push(&mut self, n: PNode) -> usize {
        self.nodes.push(n);
        self.nodes.len() - 1
    }

    fn next_player(st: &State, from: usize) -> usize {
        let mut p = (from + 1) % NUM_PLAYERS;
        while st.folded[p] {
            p = (p + 1) % NUM_PLAYERS;
        }
        p
    }

    /// Raise-to for the next level from player `p`, snapped to all-in when large.
    fn raise_to(&self, st: &State, p: usize) -> f64 {
        let c = &self.config;
        let bet = st.contrib.iter().cloned().fold(0.0, f64::max);
        let to = match st.level {
            0 => {
                if p == 4 {
                    c.sb_open
                } else {
                    c.open
                }
            }
            1 => {
                let opener = st.aggressor.unwrap();
                bet * if is_ip(p, opener) {
                    c.threebet_ip
                } else {
                    c.threebet_oop
                }
            }
            2 => bet * c.fourbet,
            _ => c.stack,
        };
        if to > c.allin_fraction * c.stack {
            c.stack
        } else {
            to
        }
    }

    fn build(&mut self, st: State) -> usize {
        let p = st.to_act;
        let live = st.folded.iter().filter(|f| !**f).count();
        if live == 1 {
            let winner = st.folded.iter().position(|f| !f).unwrap();
            return self.push(PNode::Fold {
                winner,
                contrib: st.contrib,
            });
        }
        // Unopened and folded round to the big blind: walk.
        if st.level == 0 && p == 5 {
            return self.push(PNode::Fold {
                winner: 5,
                contrib: st.contrib,
            });
        }
        // Back to the aggressor with nobody calling: everyone else folded (handled above).
        let id = self.push(PNode::Fold {
            winner: 0,
            contrib: [0.0; NUM_PLAYERS],
        });
        let stack = self.config.stack;
        let bet = st.contrib.iter().cloned().fold(0.0, f64::max);
        let mut actions = Vec::new();
        let mut kids = Vec::new();

        // Fold.
        let mut s = st;
        s.folded[p] = true;
        s.to_act = Self::next_player(&s, p);
        actions.push(PAction::Fold);
        kids.push(self.build(s));

        // Call: only facing a raise; ends the betting heads-up.
        if st.level > 0 {
            let a = st.aggressor.unwrap();
            let mut contrib = st.contrib;
            contrib[p] = bet;
            let all_in = bet >= stack - 1e-9;
            let (oop, ip) = if is_ip(p, a) { (a, p) } else { (p, a) };
            actions.push(PAction::Call);
            kids.push(self.push(if all_in {
                PNode::AllIn {
                    players: [oop, ip],
                    contrib,
                }
            } else {
                PNode::Flop {
                    players: [oop, ip],
                    contrib,
                    pot: PotType::from_level(st.level),
                    aggressor: a,
                }
            }));
        }

        // Raise (or all-in), unless already all-in.
        if st.level < 4 && bet < stack - 1e-9 {
            let to = self.raise_to(&st, p);
            let mut s = st;
            s.contrib[p] = to;
            s.level = if to >= stack - 1e-9 { 4 } else { st.level + 1 };
            s.aggressor = Some(p);
            s.to_act = Self::next_player(&s, p);
            actions.push(if to >= stack - 1e-9 {
                PAction::AllIn(to)
            } else {
                PAction::Raise(to)
            });
            kids.push(self.build(s));
        }

        self.nodes[id] = PNode::Act {
            player: p,
            actions,
            children: kids,
        };
        id
    }

    pub fn equity(&self, i: usize, j: usize) -> f32 {
        self.eq[i * NUM_CLASSES + j]
    }

    fn rake(&self, pot: f64) -> f64 {
        (pot * self.config.rake_pct).min(self.config.rake_cap)
    }

    /// Counterfactual values of player `t` at a terminal: per class, Σ over the opponents'
    /// classes of their reach × payoff, with the reach of uninvolved players as plain sums.
    fn terminal(&self, node: &PNode, t: usize, reach: &[Vec<f32>; NUM_PLAYERS]) -> Vec<f32> {
        let sums: Vec<f64> = reach
            .iter()
            .map(|r| r.iter().map(|&x| x as f64).sum())
            .collect();
        let others = |skip: &[usize]| -> f64 {
            (0..NUM_PLAYERS)
                .filter(|k| *k != t && !skip.contains(k))
                .map(|k| sums[k])
                .product()
        };
        match *node {
            PNode::Fold { winner, contrib } => {
                let pot: f64 = contrib.iter().sum();
                let u = if winner == t { pot } else { 0.0 } - contrib[t];
                vec![(u * others(&[])) as f32; NUM_CLASSES]
            }
            PNode::AllIn { players, contrib }
            | PNode::Flop {
                players, contrib, ..
            } => {
                if !players.contains(&t) {
                    return vec![(-contrib[t] * others(&[])) as f32; NUM_CLASSES];
                }
                let o = if players[0] == t {
                    players[1]
                } else {
                    players[0]
                };
                let pot: f64 = contrib.iter().sum();
                let net = pot - self.rake(pot);
                // Realisation (flops only): with k = R_IP / R_OOP for this situation, t's weight
                // is k when t is in position (players[1]), 1 otherwise, and the opponent's the rest.
                let (rt, ro) = match *node {
                    PNode::Flop { pot, aggressor, .. } => {
                        let k = self.config.realization.k(pot, aggressor == players[1]);
                        if players[1] == t { (k, 1.0) } else { (1.0, k) }
                    }
                    _ => (1.0, 1.0),
                };
                let flop = matches!(node, PNode::Flop { .. });
                let rest = others(&[o]);
                let ro_reach = &reach[o];
                (0..NUM_CLASSES)
                    .map(|c| {
                        let mut v = 0.0f64;
                        for (d, &r) in ro_reach.iter().enumerate() {
                            if r == 0.0 {
                                continue;
                            }
                            let k = c * NUM_CLASSES + d;
                            let e = self.eq[k] as f64;
                            let share = if flop {
                                let (a, b) = (e * rt, (1.0 - e) * ro);
                                if a + b > 0.0 { a / (a + b) } else { 0.5 }
                            } else {
                                e
                            };
                            v += r as f64 * self.removal[k] as f64 * (share * net - contrib[t]);
                        }
                        (v * rest) as f32
                    })
                    .collect()
            }
            PNode::Act { .. } => unreachable!(),
        }
    }
}

/// Initial reach of every player: each class weighted by its combos.
pub fn uniform_reach() -> Vec<f32> {
    (0..NUM_CLASSES).map(|i| combo_count(i) as f32).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Average,
    BestResponse,
}

pub struct PreflopSolver {
    pub game: PreflopGame,
    offset: Vec<usize>,
    regrets: Vec<f32>,
    cum: Vec<f32>,
    iteration: u32,
}

fn regret_match(reg: &[f32], n_act: usize) -> Vec<f32> {
    let mut s = vec![0.0f32; n_act * NUM_CLASSES];
    for h in 0..NUM_CLASSES {
        let sum: f32 = (0..n_act).map(|a| reg[a * NUM_CLASSES + h].max(0.0)).sum();
        for a in 0..n_act {
            s[a * NUM_CLASSES + h] = if sum > 0.0 {
                reg[a * NUM_CLASSES + h].max(0.0) / sum
            } else {
                1.0 / n_act as f32
            };
        }
    }
    s
}

#[derive(Clone, Debug)]
pub struct PreflopReport {
    /// EV per player in bb per hand under the average strategies.
    pub ev: [f64; NUM_PLAYERS],
    /// What each player gains by best responding, in bb per 100 hands.
    pub br_gain_bb100: [f64; NUM_PLAYERS],
}

impl PreflopSolver {
    pub fn new(game: PreflopGame) -> Self {
        let mut offset = vec![0; game.nodes.len()];
        let mut size = 0;
        for (i, n) in game.nodes.iter().enumerate() {
            if let PNode::Act { actions, .. } = n {
                offset[i] = size;
                size += actions.len() * NUM_CLASSES;
            }
        }
        PreflopSolver {
            game,
            offset,
            regrets: vec![0.0; size],
            cum: vec![0.0; size],
            iteration: 0,
        }
    }

    pub fn iterations(&self) -> u32 {
        self.iteration
    }

    /// One iteration of (D)CFR with alternating updates over the six players.
    pub fn iterate(&mut self) {
        self.iteration += 1;
        let t = self.iteration as f64;
        let (ta, gamma) = (t.powf(1.5), (t / (t + 1.0)).powi(2));
        let (pos, neg, strat) = ((ta / (ta + 1.0)) as f32, 0.5f32, gamma as f32);
        for p in 0..NUM_PLAYERS {
            let reach: [Vec<f32>; NUM_PLAYERS] = std::array::from_fn(|_| uniform_reach());
            self.cfr(0, p, &reach, pos, neg, strat);
        }
    }

    fn cfr(
        &mut self,
        node: usize,
        t: usize,
        reach: &[Vec<f32>; NUM_PLAYERS],
        pos: f32,
        neg: f32,
        strat: f32,
    ) -> Vec<f32> {
        let (player, n_act, children) = match &self.game.nodes[node] {
            PNode::Act {
                player,
                actions,
                children,
            } => (*player, actions.len(), children.clone()),
            n => return self.game.terminal(n, t, reach),
        };
        let off = self.offset[node];
        let sigma = regret_match(&self.regrets[off..off + n_act * NUM_CLASSES], n_act);
        let mut value = vec![0.0f32; NUM_CLASSES];
        let mut child_vals = Vec::with_capacity(n_act);
        for (a, &child) in children.iter().enumerate() {
            let mut r = reach.clone();
            for h in 0..NUM_CLASSES {
                r[player][h] *= sigma[a * NUM_CLASSES + h];
            }
            let v = self.cfr(child, t, &r, pos, neg, strat);
            if player == t {
                for h in 0..NUM_CLASSES {
                    value[h] += sigma[a * NUM_CLASSES + h] * v[h];
                }
            } else {
                for h in 0..NUM_CLASSES {
                    value[h] += v[h];
                }
            }
            child_vals.push(v);
        }
        if player == t {
            for (a, v) in child_vals.iter().enumerate() {
                for h in 0..NUM_CLASSES {
                    let i = off + a * NUM_CLASSES + h;
                    let g = self.regrets[i];
                    self.regrets[i] = g * if g > 0.0 { pos } else { neg } + (v[h] - value[h]);
                    self.cum[i] = self.cum[i] * strat + reach[t][h] * sigma[a * NUM_CLASSES + h];
                }
            }
        }
        value
    }

    /// Average strategy at an action node, `[action][class]`.
    pub fn strategy(&self, node: usize) -> Vec<f32> {
        let PNode::Act { actions, .. } = &self.game.nodes[node] else {
            panic!("node {node} is not an action node");
        };
        let n = actions.len();
        let off = self.offset[node];
        let mut s = vec![0.0f32; n * NUM_CLASSES];
        for h in 0..NUM_CLASSES {
            let sum: f32 = (0..n).map(|a| self.cum[off + a * NUM_CLASSES + h]).sum();
            for a in 0..n {
                s[a * NUM_CLASSES + h] = if sum > 0.0 {
                    self.cum[off + a * NUM_CLASSES + h] / sum
                } else {
                    1.0 / n as f32
                };
            }
        }
        s
    }

    fn values(
        &self,
        node: usize,
        t: usize,
        reach: &[Vec<f32>; NUM_PLAYERS],
        mode: Mode,
    ) -> Vec<f32> {
        let PNode::Act {
            player, children, ..
        } = &self.game.nodes[node]
        else {
            return self.game.terminal(&self.game.nodes[node], t, reach);
        };
        let sigma = self.strategy(node);
        let mut out = vec![
            if *player == t && mode == Mode::BestResponse {
                f32::NEG_INFINITY
            } else {
                0.0
            };
            NUM_CLASSES
        ];
        for (a, &child) in children.iter().enumerate() {
            let mut r = reach.clone();
            if *player != t {
                for h in 0..NUM_CLASSES {
                    r[*player][h] *= sigma[a * NUM_CLASSES + h];
                }
            }
            let v = self.values(child, t, &r, mode);
            for h in 0..NUM_CLASSES {
                if *player != t {
                    out[h] += v[h];
                } else if mode == Mode::BestResponse {
                    out[h] = out[h].max(v[h]);
                } else {
                    out[h] += sigma[a * NUM_CLASSES + h] * v[h];
                }
            }
        }
        out
    }

    pub fn report(&self) -> PreflopReport {
        let w = uniform_reach();
        let total: f64 = w.iter().map(|&x| x as f64).sum();
        let norm = total.powi(NUM_PLAYERS as i32);
        let reach: [Vec<f32>; NUM_PLAYERS] = std::array::from_fn(|_| w.clone());
        let mut ev = [0.0; NUM_PLAYERS];
        let mut gain = [0.0; NUM_PLAYERS];
        for p in 0..NUM_PLAYERS {
            let total_of = |v: Vec<f32>| -> f64 {
                v.iter()
                    .zip(&w)
                    .map(|(&x, &y)| x as f64 * y as f64)
                    .sum::<f64>()
                    / norm
            };
            ev[p] = total_of(self.values(0, p, &reach, Mode::Average));
            let br = total_of(self.values(0, p, &reach, Mode::BestResponse));
            gain[p] = (br - ev[p]) * 100.0;
        }
        PreflopReport {
            ev,
            br_gain_bb100: gain,
        }
    }

    /// Follows a line of action words from the root ("fold", "call", "raise", "all-in"; prefixes
    /// are enough) and returns the action indices.
    pub fn line_path(&self, words: &[&str]) -> Result<Vec<usize>, String> {
        let mut path = Vec::new();
        for word in words {
            let (node, _) = self.walk(&path)?;
            let PNode::Act { actions, .. } = &self.game.nodes[node] else {
                return Err(format!("the line ends before {word:?}"));
            };
            let w = word.trim().to_lowercase();
            let i = actions
                .iter()
                .position(|a| a.to_string().to_lowercase().starts_with(&w))
                .ok_or_else(|| format!("{word:?} is not possible here"))?;
            path.push(i);
        }
        Ok(path)
    }

    /// Follows actions from the root; returns the node and every player's reach there.
    pub fn walk(&self, path: &[usize]) -> Result<(usize, [Vec<f32>; NUM_PLAYERS]), String> {
        let mut node = 0;
        let mut reach: [Vec<f32>; NUM_PLAYERS] = std::array::from_fn(|_| uniform_reach());
        for (i, &a) in path.iter().enumerate() {
            let PNode::Act {
                player, children, ..
            } = &self.game.nodes[node]
            else {
                return Err(format!("step {i}: not an action node"));
            };
            let child = *children.get(a).ok_or(format!("step {i}: no action {a}"))?;
            let s = self.strategy(node);
            for h in 0..NUM_CLASSES {
                reach[*player][h] *= s[a * NUM_CLASSES + h];
            }
            node = child;
        }
        Ok((node, reach))
    }
}
