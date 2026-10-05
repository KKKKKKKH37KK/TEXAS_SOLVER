//! Result files (PRD §5.1): a flop spot is solved natively and browsed in the web app.
//!
//! The file holds the spot (so the reader rebuilds the identical tree), solve statistics, and the
//! average strategy of every action node up to a chosen street, quantised to one byte per action
//! and hand. River nodes of a flop solve are left out (they are most of the tree); the web app
//! re-solves a river from the reach at that point. Root-street nodes also carry per-hand EVs.
//!
//! Layout: "HXS1", u32 header length, header JSON, then records until the end:
//!   1, u32 node, u16 actions, u16 hands, actions × hands × u8   (strategy, [action][hand])
//!   2, u32 node, for each player: u16 hands, hands × f32         (EV per hand)
//! Integers are little-endian.

use crate::game::{Game, Node};
use crate::query::Strategies;
use crate::solver::Solver;
use crate::spec::SpotSpec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const MAGIC: &[u8; 4] = b"HXS1";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub spot: SpotSpec,
    pub iterations: u32,
    pub exploitability_pct: f64,
    pub ev: [f64; 2],
    /// Node count of the tree, to check the reader rebuilt the same one.
    pub nodes: usize,
    pub hands: [usize; 2],
    /// Strategies are stored for action nodes on boards of at most this many cards.
    pub max_board: u32,
}

/// Action nodes reachable from the root with their board size, plus each node's reach when it is
/// on the root street (no chance node above it, so no suit mapping is needed).
/// Both players' reach, per hand.
type Reach = [Vec<f32>; 2];

fn collect(game: &Game, solver: &Solver) -> Vec<(usize, u32, Option<Reach>)> {
    let mut out = Vec::new();
    let root_len = game.root_board.count_ones();
    // (node, board size, reach if still on the root street)
    let mut stack: Vec<(usize, u32, Option<Reach>)> =
        vec![(0, root_len, Some(game.weights.clone()))];
    while let Some((n, len, reach)) = stack.pop() {
        match &game.nodes[n] {
            Node::Action {
                player, children, ..
            } => {
                let p = *player;
                let child_reach: Vec<Option<Reach>> = match &reach {
                    Some(r) => {
                        let s = solver.strategy(n);
                        let h = game.num_hands(p);
                        (0..children.len())
                            .map(|a| {
                                let mut c = r.clone();
                                for (k, x) in c[p].iter_mut().enumerate() {
                                    *x *= s[a * h + k];
                                }
                                Some(c)
                            })
                            .collect()
                    }
                    None => vec![None; children.len()],
                };
                out.push((n, len, reach));
                for (&c, r) in children.iter().zip(child_reach) {
                    stack.push((c, len, r));
                }
            }
            Node::Chance { children, .. } => {
                for &c in children {
                    stack.push((c, len + 1, None));
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|x| x.0);
    out
}

/// Serialises a solved game. `max_board` limits stored strategies to nodes on boards of at most
/// that many cards (4 = flop and turn).
pub fn write(solver: &Solver, spot: &SpotSpec, max_board: u32) -> Vec<u8> {
    let game = solver.game();
    let report = solver.report();
    let header = Header {
        spot: spot.clone(),
        iterations: solver.iterations(),
        exploitability_pct: report.exploitability_pct,
        ev: report.ev,
        nodes: game.nodes.len(),
        hands: [game.num_hands(0), game.num_hands(1)],
        max_board,
    };
    let json = serde_json::to_vec(&header).expect("header serialises");
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    for (node, len, reach) in collect(game, solver) {
        if len > max_board {
            continue;
        }
        let Node::Action {
            player, actions, ..
        } = &game.nodes[node]
        else {
            continue;
        };
        let s = solver.strategy(node);
        out.push(1);
        out.extend_from_slice(&(node as u32).to_le_bytes());
        out.extend_from_slice(&(actions.len() as u16).to_le_bytes());
        out.extend_from_slice(&(game.num_hands(*player) as u16).to_le_bytes());
        out.extend(s.iter().map(|&x| (x.clamp(0.0, 1.0) * 255.0).round() as u8));
        if let Some(r) = reach {
            out.push(2);
            out.extend_from_slice(&(node as u32).to_le_bytes());
            for p in 0..2 {
                let ev = solver.node_ev(node, p, &r[1 - p]);
                out.extend_from_slice(&(ev.len() as u16).to_le_bytes());
                for x in ev {
                    out.extend_from_slice(&(x as f32).to_le_bytes());
                }
            }
        }
    }
    out
}

/// A result file read back: header plus decoded strategies and EVs by node.
pub struct Loaded {
    pub header: Header,
    pub strategies: HashMap<usize, Vec<f32>>,
    pub evs: HashMap<usize, [Vec<f32>; 2]>,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self
            .b
            .get(self.at..self.at + n)
            .ok_or("result file is truncated")?;
        self.at += n;
        Ok(s)
    }
    fn u16(&mut self) -> Result<usize, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as usize)
    }
    fn u32(&mut self) -> Result<usize, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize)
    }
}

pub fn read(bytes: &[u8]) -> Result<Loaded, String> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(4)? != MAGIC {
        return Err("not a HEXAS result file".into());
    }
    let len = r.u32()?;
    let header: Header =
        serde_json::from_slice(r.take(len)?).map_err(|e| format!("bad result header: {e}"))?;
    let mut strategies = HashMap::new();
    let mut evs = HashMap::new();
    while r.at < bytes.len() {
        match r.take(1)?[0] {
            1 => {
                let node = r.u32()?;
                let (na, nh) = (r.u16()?, r.u16()?);
                let q = r.take(na * nh)?;
                let mut s = vec![0.0f32; na * nh];
                for h in 0..nh {
                    let sum: u32 = (0..na).map(|a| q[a * nh + h] as u32).sum();
                    for a in 0..na {
                        s[a * nh + h] = if sum > 0 {
                            q[a * nh + h] as f32 / sum as f32
                        } else {
                            1.0 / na as f32
                        };
                    }
                }
                strategies.insert(node, s);
            }
            2 => {
                let node = r.u32()?;
                let mut ev = [Vec::new(), Vec::new()];
                for e in &mut ev {
                    let n = r.u16()?;
                    let raw = r.take(n * 4)?;
                    *e = raw
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| f32::from_le_bytes(*c))
                        .collect();
                }
                evs.insert(node, ev);
            }
            t => return Err(format!("unknown record type {t}")),
        }
    }
    Ok(Loaded {
        header,
        strategies,
        evs,
    })
}

/// A result file together with the rebuilt game tree, browsable with `query::walk`.
pub struct Imported {
    pub game: Game,
    pub loaded: Loaded,
}

impl Imported {
    pub fn new(bytes: &[u8]) -> Result<Self, String> {
        let loaded = read(bytes)?;
        let game = crate::holdem::build(&loaded.header.spot.to_spot()?)?;
        if game.nodes.len() != loaded.header.nodes {
            return Err(format!(
                "the rebuilt tree has {} nodes, the file {}: written by a different version?",
                game.nodes.len(),
                loaded.header.nodes
            ));
        }
        Ok(Imported { game, loaded })
    }
}

impl Strategies for Imported {
    fn game(&self) -> &Game {
        &self.game
    }

    fn strategy(&self, node: usize) -> Option<Vec<f32>> {
        self.loaded.strategies.get(&node).cloned()
    }
}
