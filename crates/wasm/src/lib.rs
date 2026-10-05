//! Browser interface (PRD §6, M3): one solving session, driven by JSON commands.
//!
//! JS writes a UTF-8 JSON request into memory from `hx_alloc`, calls `hx_call(ptr, len)`, and
//! reads the JSON reply at `hx_out_ptr()` / `hx_out_len()`. `hx_call` returns 0 on success and 1 when
//! the reply is `{"error": "..."}`. The same `handle` function is used natively in tests.
//!
//! Commands (field `cmd`):
//! - `estimate {spot}` → tree size and memory, without building
//! - `create {spot}` → builds the tree and allocates the solver (replaces any previous session)
//! - `step {n}` → runs n iterations
//! - `report` → iteration count, EVs and exploitability
//! - `view {path, ev}` → the node at the end of `path` (see `query::walk`)
//! - `destroy` → frees the session

use hexas_core::cards::{Card, parse_cards};
use hexas_core::game::{Action, Game};
use hexas_core::holdem::{BetSizes, Rake, Spot, TreeConfig, build, estimate};
use hexas_core::query::{Kind, Step, View, hand_ev, walk};
use hexas_core::range::Range;
use hexas_core::solver::{DcfrParams, Solver};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::mem::ManuallyDrop;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SizesIn {
    /// Pot fractions, e.g. [0.33, 0.66].
    pub bets: Vec<f64>,
    pub raise_mult: f64,
    pub max_raises: usize,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpotIn {
    pub board: String,
    pub oop: String,
    pub ip: String,
    pub pot: f64,
    pub stack: f64,
    /// Flop, turn, river. Omitted: the PRD preset for the board.
    pub sizes: Option<[SizesIn; 3]>,
    pub donk: Option<bool>,
    pub rake_pct: Option<f64>,
    pub rake_cap: Option<f64>,
    pub allin_threshold: Option<f64>,
    pub isomorphism: Option<bool>,
}

impl SpotIn {
    pub fn to_spot(&self) -> Result<Spot, String> {
        let board = parse_cards(&self.board).map_err(|e| e.to_string())?;
        let mut cfg = TreeConfig::preset(self.pot, self.stack, board.len());
        if let Some(s) = &self.sizes {
            cfg.sizes = s.clone().map(|x| BetSizes {
                bets: x.bets,
                raise_mult: x.raise_mult,
                max_raises: x.max_raises,
                bet_allin: false,
            });
        }
        if let Some(d) = self.donk {
            cfg.oop_flop_bets = d;
        }
        cfg.rake = Rake {
            pct: self.rake_pct.unwrap_or(cfg.rake.pct),
            cap: self.rake_cap.unwrap_or(cfg.rake.cap),
        };
        if let Some(t) = self.allin_threshold {
            cfg.allin_threshold = t;
        }
        if let Some(i) = self.isomorphism {
            cfg.isomorphism = i;
        }
        let parse = |s: &str| s.parse::<Range>().map_err(|e| e.to_string());
        Ok(Spot {
            board,
            ranges: [parse(&self.oop)?, parse(&self.ip)?],
            config: cfg,
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
enum Request {
    Estimate { spot: SpotIn },
    Create { spot: SpotIn },
    Step { n: u32 },
    Report,
    View { path: Vec<StepIn>, ev: Option<bool> },
    Destroy,
}

/// A path step: `{"a": 1}` takes action 1, `{"c": "5h"}` deals a card.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StepIn {
    pub a: Option<usize>,
    pub c: Option<String>,
}

/// The solver borrows the game, so the game lives on the heap behind a raw pointer and is freed
/// after the solver.
struct Session {
    solver: ManuallyDrop<Solver<'static>>,
    game: *mut Game,
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `game` came from Box::into_raw in `create` and only the solver borrows it; the
        // solver is dropped first and never used again.
        unsafe {
            ManuallyDrop::drop(&mut self.solver);
            drop(Box::from_raw(self.game));
        }
    }
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn hand_name(m: u64) -> String {
    let a = Card::from_index(63 - m.leading_zeros() as u8);
    let b = Card::from_index(m.trailing_zeros() as u8);
    format!("{a}{b}")
}

fn action_json(a: &Action) -> Value {
    let (kind, amount) = match *a {
        Action::Fold => ("fold", 0.0),
        Action::Check => ("check", 0.0),
        Action::Call => ("call", 0.0),
        Action::Bet(x) => ("bet", x),
        Action::Raise(x) => ("raise", x),
        Action::AllIn(x) => ("allin", x),
    };
    json!({ "kind": kind, "amount": amount, "label": a.to_string() })
}

fn cards_of(m: u64) -> Vec<String> {
    (0..52u8)
        .filter(|&c| m >> c & 1 == 1)
        .map(|c| Card::from_index(c).to_string())
        .collect()
}

fn to_steps(path: &[StepIn]) -> Result<Vec<Step>, String> {
    path.iter()
        .map(|s| match (s.a, &s.c) {
            (Some(a), None) => Ok(Step::Action(a)),
            (None, Some(c)) => c
                .parse::<Card>()
                .map(|c| Step::Card(c.index()))
                .map_err(|e| e.to_string()),
            _ => Err(format!("bad path step {s:?}")),
        })
        .collect()
}

fn view_json(solver: &Solver, v: &View, with_ev: bool) -> Value {
    let game = solver.game();
    let (kind, player, actions) = match &v.kind {
        Kind::Action { player, actions } => (
            "action",
            Some(*player),
            actions.iter().map(action_json).collect::<Vec<_>>(),
        ),
        Kind::Chance => ("chance", None, vec![]),
        Kind::Fold { folder } => ("fold", Some(*folder), vec![]),
        Kind::Showdown => ("showdown", None, vec![]),
    };
    let ev = if with_ev && kind != "chance" {
        Some([0, 1].map(|p| hand_ev(solver, v, p)))
    } else {
        None
    };
    let hands = [0, 1].map(|p| {
        game.hands[p]
            .iter()
            .map(|&m| hand_name(m))
            .collect::<Vec<_>>()
    });
    let dealable = if kind == "chance" {
        cards_of(!v.board & ((1u64 << 52) - 1))
    } else {
        vec![]
    };
    json!({
        "kind": kind,
        "player": player,
        "actions": actions,
        "board": cards_of(v.board),
        "pot": v.pot,
        "stacks": v.stacks,
        "street": v.street,
        "hands": hands,
        "reach": v.reach,
        "strategy": v.strategy,
        "ev": ev,
        "dealable": dealable,
    })
}

/// Runs one JSON request against the session and returns the JSON reply.
pub fn handle(req: &str) -> Result<Value, String> {
    let req: Request = serde_json::from_str(req).map_err(|e| format!("bad request: {e}"))?;
    match req {
        Request::Estimate { spot } => {
            let (t, hands) = estimate(&spot.to_spot()?)?;
            Ok(json!({
                "nodes": t.nodes(),
                "actionNodes": t.action_nodes,
                "bytes": t.solver_bytes(hands),
                "hands": hands,
            }))
        }
        Request::Create { spot } => {
            SESSION.with(|s| s.borrow_mut().take());
            let game = Box::into_raw(Box::new(build(&spot.to_spot()?)?));
            // SAFETY: freed only in Session::drop, after the solver stops using it.
            let solver = Solver::new(unsafe { &*game }, DcfrParams::default());
            let bytes = solver.memory_bytes();
            let hands = [0, 1].map(|p| solver.game().num_hands(p));
            SESSION.with(|s| {
                *s.borrow_mut() = Some(Session {
                    solver: ManuallyDrop::new(solver),
                    game,
                })
            });
            Ok(json!({ "bytes": bytes, "hands": hands }))
        }
        Request::Step { n } => with_session(|s| {
            for _ in 0..n {
                s.solver.iterate();
            }
            Ok(json!({ "iteration": s.solver.iterations() }))
        }),
        Request::Report => with_session(|s| {
            let r = s.solver.report();
            Ok(json!({
                "iteration": s.solver.iterations(),
                "ev": r.ev,
                "exploitability": r.exploitability,
                "exploitabilityPct": r.exploitability_pct,
            }))
        }),
        Request::View { path, ev } => with_session(|s| {
            let v = walk(&s.solver, &to_steps(&path)?)?;
            Ok(view_json(&s.solver, &v, ev.unwrap_or(false)))
        }),
        Request::Destroy => {
            SESSION.with(|s| s.borrow_mut().take());
            Ok(json!({}))
        }
    }
}

fn with_session(f: impl FnOnce(&mut Session) -> Result<Value, String>) -> Result<Value, String> {
    SESSION.with(|s| match s.borrow_mut().as_mut() {
        Some(sess) => f(sess),
        None => Err("no session: call create first".into()),
    })
}

// ---- C ABI for the browser ----

/// Allocates `len` bytes for a request.
#[unsafe(no_mangle)]
pub extern "C" fn hx_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// Frees a request buffer from `hx_alloc`.
///
/// # Safety
/// `ptr` / `len` must come from one `hx_alloc` call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hx_free(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// Handles the JSON request in `[ptr, ptr + len)`; the reply is at `hx_out_ptr` / `hx_out_len`.
///
/// # Safety
/// `ptr` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hx_call(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let (code, reply) = match std::str::from_utf8(bytes)
        .map_err(|e| e.to_string())
        .and_then(handle)
    {
        Ok(v) => (0, v),
        Err(e) => (1, json!({ "error": e })),
    };
    OUT.with(|o| *o.borrow_mut() = reply.to_string().into_bytes());
    code
}

#[unsafe(no_mangle)]
pub extern "C" fn hx_out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn hx_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}
