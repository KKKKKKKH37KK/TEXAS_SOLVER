//! Browser interface (PRD §6, M3), driven by JSON commands.
//!
//! JS writes a UTF-8 JSON request into memory from `hx_alloc`, calls `hx_call(ptr, len)`, and
//! reads the JSON reply at `hx_out_ptr()` / `hx_out_len()`. `hx_call` returns 0 on success and 1 when
//! the reply is `{"error": "..."}`. A result file is passed as raw bytes to `hx_load(ptr, len)`,
//! which replies the same way. The same `handle` / `load` functions are used natively in tests.
//!
//! There are two independent sessions: a live solve, and an imported result file.
//!
//! Commands (field `cmd`):
//! - `estimate {spot}` → tree size and memory, without building
//! - `create {spot}` → builds the tree and allocates the solver (replaces the previous solve)
//! - `step {n}` → runs n iterations
//! - `report` → iteration count, EVs and exploitability
//! - `view {path, ev, source}` → the node at the end of `path` in the solve (default) or the
//!   imported file (`source: "import"`), see `query::walk`
//! - `destroy` → frees the solve
//! - `preflopCreate {config}` / `preflopStep {n}` / `preflopReport` / `preflopView {path}` → the
//!   6-max preflop solver (a third, independent session); `path` is a list of action indices

use hexas_core::cards::Card;
use hexas_core::export::Imported;
use hexas_core::game::{Action, Game};
use hexas_core::holdem::{build, estimate};
use hexas_core::preflop::classes::{NUM_CLASSES, class_name};
use hexas_core::preflop::game::{PNode, POSITIONS, PreflopConfig, PreflopGame, PreflopSolver};
use hexas_core::query::{Kind, Step, Strategies, View, hand_ev, to_real, walk};
use hexas_core::solver::{DcfrParams, Solver};
use hexas_core::spec::SpotSpec;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::mem::ManuallyDrop;

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
enum Request {
    Estimate {
        spot: SpotSpec,
    },
    Create {
        spot: SpotSpec,
    },
    Step {
        n: u32,
    },
    Report,
    View {
        path: Vec<StepIn>,
        ev: Option<bool>,
        source: Option<String>,
    },
    Destroy,
    PreflopCreate {
        config: Option<PreflopIn>,
    },
    PreflopStep {
        n: u32,
    },
    PreflopReport,
    PreflopView {
        path: Vec<usize>,
    },
}

/// Preflop settings; anything omitted keeps the PRD default.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PreflopIn {
    stack: Option<f64>,
    rake_pct: Option<f64>,
    rake_cap: Option<f64>,
}

impl PreflopIn {
    fn config(&self) -> PreflopConfig {
        let d = PreflopConfig::default();
        PreflopConfig {
            stack: self.stack.unwrap_or(d.stack),
            rake_pct: self.rake_pct.unwrap_or(d.rake_pct),
            rake_cap: self.rake_cap.unwrap_or(d.rake_cap),
            ..d
        }
    }
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
    static IMPORT: RefCell<Option<Imported>> = const { RefCell::new(None) };
    static PREFLOP: RefCell<Option<PreflopSolver>> = const { RefCell::new(None) };
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

/// `ev`: per-hand EVs for both players, when available.
fn view_json(game: &Game, v: &View, ev: Option<[Vec<f64>; 2]>) -> Value {
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

/// Runs one JSON request and returns the JSON reply.
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
        Request::View { path, ev, source } => {
            let steps = to_steps(&path)?;
            if source.as_deref() == Some("import") {
                IMPORT.with(|i| {
                    let i = i.borrow();
                    let imp = i.as_ref().ok_or("no result file loaded")?;
                    let v = walk(imp, &steps)?;
                    // EVs are stored for root-street nodes only.
                    let ev = imp.loaded.evs.get(&v.node).map(|e| {
                        [0, 1].map(|p| {
                            let x: Vec<f64> = e[p].iter().map(|&y| y as f64).collect();
                            to_real(&imp.game, &v, p, &x)
                        })
                    });
                    Ok(view_json(imp.game(), &v, ev))
                })
            } else {
                with_session(|s| {
                    let v = walk(&*s.solver, &steps)?;
                    let ev = (ev.unwrap_or(false) && v.kind != Kind::Chance)
                        .then(|| [0, 1].map(|p| hand_ev(&s.solver, &v, p)));
                    Ok(view_json(s.solver.game(), &v, ev))
                })
            }
        }
        Request::Destroy => {
            SESSION.with(|s| s.borrow_mut().take());
            Ok(json!({}))
        }
        Request::PreflopCreate { config } => {
            let solver = PreflopSolver::new(PreflopGame::new(config.unwrap_or_default().config()));
            let nodes = solver.game.nodes.len();
            PREFLOP.with(|p| *p.borrow_mut() = Some(solver));
            Ok(json!({ "nodes": nodes }))
        }
        Request::PreflopStep { n } => with_preflop(|s| {
            for _ in 0..n {
                s.iterate();
            }
            Ok(json!({ "iteration": s.iterations() }))
        }),
        Request::PreflopReport => with_preflop(|s| {
            let r = s.report();
            Ok(json!({ "iteration": s.iterations(), "ev": r.ev, "brGainBb100": r.br_gain_bb100 }))
        }),
        Request::PreflopView { path } => with_preflop(|s| {
            let (node, reach) = s.walk(&path)?;
            let classes: Vec<String> = (0..NUM_CLASSES).map(class_name).collect();
            let (kind, player, actions, strategy, contrib, players) = match &s.game.nodes[node] {
                PNode::Act {
                    player, actions, ..
                } => (
                    "action",
                    Some(*player),
                    actions.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
                    Some(s.strategy(node)),
                    None,
                    vec![],
                ),
                PNode::Fold { winner, contrib } => {
                    ("fold", None, vec![], None, Some(*contrib), vec![*winner])
                }
                PNode::AllIn { players, contrib } => (
                    "allin",
                    None,
                    vec![],
                    None,
                    Some(*contrib),
                    players.to_vec(),
                ),
                PNode::Flop {
                    players, contrib, ..
                } => ("flop", None, vec![], None, Some(*contrib), players.to_vec()),
            };
            Ok(json!({
                "kind": kind,
                "player": player,
                "positions": POSITIONS,
                "actions": actions,
                "strategy": strategy,
                "reach": reach,
                "classes": classes,
                "contrib": contrib,
                "players": players,
            }))
        }),
    }
}

fn with_preflop(
    f: impl FnOnce(&mut PreflopSolver) -> Result<Value, String>,
) -> Result<Value, String> {
    PREFLOP.with(|p| match p.borrow_mut().as_mut() {
        Some(s) => f(s),
        None => Err("no preflop solve: call preflopCreate first".into()),
    })
}

/// Loads a result file into the import session and returns its header.
pub fn load(bytes: &[u8]) -> Result<Value, String> {
    IMPORT.with(|i| i.borrow_mut().take());
    let imp = Imported::new(bytes)?;
    let header = serde_json::to_value(&imp.loaded.header).map_err(|e| e.to_string())?;
    let stored = imp.loaded.strategies.len();
    IMPORT.with(|i| *i.borrow_mut() = Some(imp));
    Ok(json!({ "header": header, "storedNodes": stored }))
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

fn reply(r: Result<Value, String>) -> u32 {
    let (code, v) = match r {
        Ok(v) => (0, v),
        Err(e) => (1, json!({ "error": e })),
    };
    OUT.with(|o| *o.borrow_mut() = v.to_string().into_bytes());
    code
}

/// Handles the JSON request in `[ptr, ptr + len)`; the reply is at `hx_out_ptr` / `hx_out_len`.
///
/// # Safety
/// `ptr` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hx_call(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    reply(
        std::str::from_utf8(bytes)
            .map_err(|e| e.to_string())
            .and_then(handle),
    )
}

/// Loads the result file in `[ptr, ptr + len)`; the reply is at `hx_out_ptr` / `hx_out_len`.
///
/// # Safety
/// `ptr` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hx_load(ptr: *const u8, len: usize) -> u32 {
    reply(load(unsafe { std::slice::from_raw_parts(ptr, len) }))
}

#[unsafe(no_mangle)]
pub extern "C" fn hx_out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn hx_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}
