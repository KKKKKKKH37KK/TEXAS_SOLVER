//! `hexas` CLI: inspect cards and hands, size and solve postflop spots. Run `hexas --help`.

use hexas_core::cards::parse_cards;
use hexas_core::eval::{Category, evaluate};
use hexas_core::export;
use hexas_core::game::{Game, Node};
use hexas_core::holdem::{Rake, Spot, TreeConfig, build, estimate, hand_labels};
use hexas_core::range::Range;
use hexas_core::solver::{DcfrParams, Solver};
use hexas_core::spec::SpotSpec;
use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::Instant;

const HELP: &str = "\
HEXAS Solver: 6-max NLHE cash game solver (study use only)

usage:
  hexas cards <cards>            card indices, e.g. hexas cards AsKd7c
  hexas eval <5-7 cards>         best five-card hand, e.g. hexas eval AsKsQsJsTs2d3c
  hexas tree [options]           tree size and memory estimate, without solving
  hexas solve [options]          solve a heads-up spot, print the OOP root strategy
  hexas --version | --help

options (defaults in brackets):
  --board <cards>       3 to 5 board cards, e.g. Qs9h5d (required)
  --oop <range>         OOP range, e.g. \"AA-22,AKs-A2s,KQo\" (required)
  --ip <range>          IP range (required)
  --pot <bb>            starting pot [20]
  --stack <bb>          effective stack behind [90]
  --rake-pct <pct>      rake in % of the pot [5]
  --rake-cap <bb>       rake cap [3]
  --budget-mb <mb>      refuse to solve above this estimate [3000]
  --iters <n>           maximum iterations [1000]
  --target <pct>        stop below this exploitability, % of pot [0.3]
  --out <file.hxs>      (solve) write a result file for the web app; a flop solve keeps flop and
                        turn strategies, rivers are re-solved in the browser

tree presets (PRD 3.3), changed by the options below:
  flop spot:        flop 33,66,100,125 no donk | turn, river 66,125 | 1 raise per street
  turn/river spot:  33,66,100,125 on every street | donk allowed | 3 raises per street
  raises are 3x the bet faced, plus all-in

  --bets <list>         bet sizes in % of pot on every street, e.g. 33,66
  --turn-bets <list>    turn sizes
  --river-bets <list>   river sizes
  --raise-mult <x>      raise to x times the bet faced
  --max-raises <n>      raises allowed per street
  --donk | --no-donk    whether OOP may bet first on the flop";

struct SpotArgs {
    board: String,
    oop: String,
    ip: String,
    pot: f64,
    stack: f64,
    /// Overrides on top of the preset.
    bets: Option<String>,
    turn_bets: Option<String>,
    river_bets: Option<String>,
    raise_mult: Option<f64>,
    max_raises: Option<usize>,
    donk: Option<bool>,
    rake_pct: f64,
    rake_cap: f64,
    budget_mb: f64,
    iters: u32,
    target: f64,
    /// Result file to write after solving.
    out: Option<String>,
}

fn parse_args(args: &[String]) -> Result<SpotArgs, String> {
    let mut a = SpotArgs {
        board: String::new(),
        oop: String::new(),
        ip: String::new(),
        pot: 20.0,
        stack: 90.0,
        bets: None,
        turn_bets: None,
        river_bets: None,
        raise_mult: None,
        max_raises: None,
        donk: None,
        rake_pct: 5.0,
        rake_cap: 3.0,
        budget_mb: 3000.0,
        iters: 1000,
        target: 0.3,
        out: None,
    };
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        if flag == "--donk" || flag == "--no-donk" {
            a.donk = Some(flag == "--donk");
            continue;
        }
        let v = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let num = |v: &str| {
            v.parse::<f64>()
                .map_err(|_| format!("{flag}: not a number: {v}"))
        };
        match flag.as_str() {
            "--board" => a.board = v.clone(),
            "--oop" => a.oop = v.clone(),
            "--ip" => a.ip = v.clone(),
            "--pot" => a.pot = num(v)?,
            "--stack" => a.stack = num(v)?,
            "--bets" => a.bets = Some(v.clone()),
            "--turn-bets" => a.turn_bets = Some(v.clone()),
            "--river-bets" => a.river_bets = Some(v.clone()),
            "--raise-mult" => a.raise_mult = Some(num(v)?),
            "--max-raises" => a.max_raises = Some(num(v)? as usize),
            "--rake-pct" => a.rake_pct = num(v)?,
            "--rake-cap" => a.rake_cap = num(v)?,
            "--budget-mb" => a.budget_mb = num(v)?,
            "--iters" => a.iters = num(v)? as u32,
            "--target" => a.target = num(v)?,
            "--out" => a.out = Some(v.clone()),
            _ => return Err(format!("unknown option {flag} (see --help)")),
        }
    }
    if a.board.is_empty() || a.oop.is_empty() || a.ip.is_empty() {
        return Err("--board, --oop and --ip are required (see --help)".into());
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            println!("{HELP}");
            Ok(())
        }
        Some("--version") | Some("-V") => {
            println!("hexas {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("cards") if args.len() == 2 => cmd_cards(&args[1]),
        Some("eval") if args.len() == 2 => cmd_eval(&args[1]),
        Some("tree") => parse_args(&args[1..]).and_then(|a| cmd_tree(&a)),
        Some("solve") => parse_args(&args[1..]).and_then(|a| cmd_solve(&a)),
        _ => Err("unknown command (see --help)".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_cards(s: &str) -> Result<(), String> {
    for c in parse_cards(s).map_err(|e| e.to_string())? {
        println!(
            "{c}\tindex {:2}  rank {:2}  suit {}",
            c.index(),
            c.rank(),
            c.suit()
        );
    }
    Ok(())
}

fn cmd_eval(s: &str) -> Result<(), String> {
    let cards = parse_cards(s).map_err(|e| e.to_string())?;
    if !(5..=7).contains(&cards.len()) {
        return Err("need 5 to 7 cards".into());
    }
    let v = evaluate(&cards);
    println!("{:?} (strength {v:#08x})", Category::of(v));
    Ok(())
}

/// "33,66" → [0.33, 0.66].
fn bet_list(list: &str) -> Result<Vec<f64>, String> {
    list.split(',')
        .map(|x| {
            x.trim()
                .parse::<f64>()
                .map(|p| p / 100.0)
                .map_err(|_| format!("bad bet size {x:?}"))
        })
        .collect()
}

/// The preset for the board length (PRD §3.3), with any size options applied on top.
fn spot_of(a: &SpotArgs) -> Result<Spot, String> {
    let board = parse_cards(&a.board).map_err(|e| e.to_string())?;
    let mut config = TreeConfig::preset(a.pot, a.stack, board.len());
    for (street, list) in [(0, &a.bets), (1, &a.bets), (2, &a.bets)]
        .into_iter()
        .chain([(1, &a.turn_bets), (2, &a.river_bets)])
    {
        if let Some(l) = list {
            config.sizes[street].bets = bet_list(l)?;
        }
    }
    for s in &mut config.sizes {
        if let Some(m) = a.raise_mult {
            s.raise_mult = m;
        }
        if let Some(n) = a.max_raises {
            s.max_raises = n;
        }
    }
    if let Some(d) = a.donk {
        config.oop_flop_bets = d;
    }
    config.rake = Rake {
        pct: a.rake_pct / 100.0,
        cap: a.rake_cap,
    };
    let parse = |s: &str| s.parse::<Range>().map_err(|e| e.to_string());
    Ok(Spot {
        board,
        ranges: [parse(&a.oop)?, parse(&a.ip)?],
        config,
    })
}

/// Prints the estimate and returns it in MB.
fn print_estimate(spot: &Spot) -> Result<f64, String> {
    let (t, hands) = estimate(spot)?;
    let mb = t.solver_bytes(hands) as f64 / 1e6;
    println!(
        "tree: {} nodes ({} action, {} chance, {} terminal), hands OOP {} / IP {}",
        t.nodes(),
        t.action_nodes,
        t.chance_nodes,
        t.terminal_nodes,
        hands[0],
        hands[1]
    );
    println!("solver memory (regrets + average strategy, f32): {mb:.1} MB");
    Ok(mb)
}

fn cmd_tree(a: &SpotArgs) -> Result<(), String> {
    let mb = print_estimate(&spot_of(a)?)?;
    if mb > a.budget_mb {
        println!("over the {:.0} MB budget", a.budget_mb);
    }
    Ok(())
}

fn cmd_solve(a: &SpotArgs) -> Result<(), String> {
    let spot = spot_of(a)?;
    let mb = print_estimate(&spot)?;
    if mb > a.budget_mb {
        return Err(format!(
            "estimated {mb:.0} MB is over the {:.0} MB budget (raise --budget-mb or use fewer sizes)",
            a.budget_mb
        ));
    }
    let t0 = Instant::now();
    let game = build(&spot)?;
    let mut solver = Solver::new(&game, DcfrParams::default());
    println!("built in {:.2}s", t0.elapsed().as_secs_f64());

    let t0 = Instant::now();
    let mut report = solver.report();
    while solver.iterations() < a.iters {
        solver.iterate();
        let n = solver.iterations();
        if n.is_multiple_of(25) || n == a.iters {
            report = solver.report();
            println!(
                "iter {n:5}  exploitability {:.3}% pot  ({:.1}s)",
                report.exploitability_pct,
                t0.elapsed().as_secs_f64()
            );
            if report.exploitability_pct < a.target {
                break;
            }
        }
    }
    println!(
        "done in {:.2}s: EV OOP {:.3} / IP {:.3} bb, exploitability {:.3}% pot",
        t0.elapsed().as_secs_f64(),
        report.ev[0],
        report.ev[1],
        report.exploitability_pct
    );
    print_root(&game, &solver);

    if let Some(path) = &a.out {
        // Flop solves keep flop and turn strategies; the web app re-solves rivers on demand.
        let max_board = if spot.board.len() == 3 { 4 } else { 5 };
        let t0 = Instant::now();
        let bytes = export::write(
            &solver,
            &SpotSpec::describe(&spot, &a.oop, &a.ip),
            max_board,
        );
        std::fs::write(path, &bytes).map_err(|e| format!("cannot write {path}: {e}"))?;
        println!(
            "\nwrote {path}: {:.1} MB in {:.1}s (open it in the web app)",
            bytes.len() as f64 / 1e6,
            t0.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

/// OOP root strategy: overall frequencies and per hand class.
fn print_root(game: &Game, solver: &Solver) {
    let Node::Action { actions, .. } = &game.nodes[0] else {
        return;
    };
    let st = solver.strategy(0);
    let n = game.num_hands(0);
    let w = &game.weights[0];
    let total: f32 = w.iter().sum();
    println!("\nOOP root strategy:");
    for (i, act) in actions.iter().enumerate() {
        let f: f32 = (0..n).map(|h| st[i * n + h] * w[h]).sum::<f32>() / total;
        println!("  {act:<14} {:5.1}%", f * 100.0);
    }

    // Per hand class: weighted average frequency of each action.
    let labels = hand_labels(game, 0);
    let mut by_class: BTreeMap<String, (Vec<f32>, f32)> = BTreeMap::new();
    for h in 0..n {
        let e = by_class
            .entry(labels[h].clone())
            .or_insert((vec![0.0; actions.len()], 0.0));
        for (a, x) in e.0.iter_mut().enumerate() {
            *x += st[a * n + h] * w[h];
        }
        e.1 += w[h];
    }
    print!("\n  {:<5}", "hand");
    for act in actions {
        print!(" {:>13}", act.to_string());
    }
    println!();
    for (class, (freq, wsum)) in &by_class {
        print!("  {class:<5}");
        for x in freq {
            print!(" {:>12.0}%", x / wsum * 100.0);
        }
        println!();
    }
}
