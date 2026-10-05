//! `hexas` CLI: inspect cards and hands, solve river spots. Run `hexas --help`.

use hexas_core::cards::parse_cards;
use hexas_core::eval::{Category, evaluate};
use hexas_core::game::{Game, Node};
use hexas_core::holdem::{BetSizes, Rake, RiverSpot, TreeConfig, build_river, hand_labels};
use hexas_core::range::Range;
use hexas_core::solver::{DcfrParams, Solver};
use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::Instant;

const HELP: &str = "\
HEXAS Solver: 6-max NLHE cash game solver (study use only)

usage:
  hexas cards <cards>            card indices, e.g. hexas cards AsKd7c
  hexas eval <5-7 cards>         best five-card hand, e.g. hexas eval AsKsQsJsTs2d3c
  hexas river [options]          solve a heads-up river spot, print the OOP root strategy
  hexas --version | --help

river options (defaults in brackets):
  --board <cards>       five board cards, e.g. Qs9h5d3c2s (required)
  --oop <range>         OOP range, e.g. \"AA-22,AKs-A2s,KQo\" (required)
  --ip <range>          IP range (required)
  --pot <bb>            starting pot [20]
  --stack <bb>          effective stack behind [90]
  --bets <list>         bet sizes in % of pot [33,66,100,125]
  --raise-mult <x>      raise to x times the bet faced [3]
  --max-raises <n>      raises allowed per street [3]
  --rake-pct <pct>      rake in % of the pot [5]
  --rake-cap <bb>       rake cap [3]
  --iters <n>           maximum iterations [1000]
  --target <pct>        stop below this exploitability, % of pot [0.3]";

struct RiverArgs {
    board: String,
    oop: String,
    ip: String,
    pot: f64,
    stack: f64,
    bets: String,
    raise_mult: f64,
    max_raises: usize,
    rake_pct: f64,
    rake_cap: f64,
    iters: u32,
    target: f64,
}

fn parse_river(args: &[String]) -> Result<RiverArgs, String> {
    let mut a = RiverArgs {
        board: String::new(),
        oop: String::new(),
        ip: String::new(),
        pot: 20.0,
        stack: 90.0,
        bets: "33,66,100,125".into(),
        raise_mult: 3.0,
        max_raises: 3,
        rake_pct: 5.0,
        rake_cap: 3.0,
        iters: 1000,
        target: 0.3,
    };
    let mut it = args.iter();
    while let Some(flag) = it.next() {
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
            "--bets" => a.bets = v.clone(),
            "--raise-mult" => a.raise_mult = num(v)?,
            "--max-raises" => a.max_raises = num(v)? as usize,
            "--rake-pct" => a.rake_pct = num(v)?,
            "--rake-cap" => a.rake_cap = num(v)?,
            "--iters" => a.iters = num(v)? as u32,
            "--target" => a.target = num(v)?,
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
        Some("river") => parse_river(&args[1..]).and_then(|a| cmd_river(&a)),
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

fn cmd_river(a: &RiverArgs) -> Result<(), String> {
    let bets = a
        .bets
        .split(',')
        .map(|x| {
            x.trim()
                .parse::<f64>()
                .map(|p| p / 100.0)
                .map_err(|_| format!("bad bet size {x:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut config = TreeConfig::new(a.pot, a.stack);
    config.sizes = BetSizes {
        bets,
        raise_mult: a.raise_mult,
        max_raises: a.max_raises,
        bet_allin: false,
    };
    config.rake = Rake {
        pct: a.rake_pct / 100.0,
        cap: a.rake_cap,
    };
    let parse = |s: &str| s.parse::<Range>().map_err(|e| e.to_string());
    let spot = RiverSpot {
        board: parse_cards(&a.board).map_err(|e| e.to_string())?,
        ranges: [parse(&a.oop)?, parse(&a.ip)?],
        config,
    };
    let game = build_river(&spot)?;
    let actions = game
        .nodes
        .iter()
        .filter(|n| matches!(n, Node::Action { .. }))
        .count();
    let mut solver = Solver::new(&game, DcfrParams::default());
    println!(
        "tree: {} nodes ({} action), hands OOP {} / IP {}, memory {:.1} MB",
        game.nodes.len(),
        actions,
        game.num_hands(0),
        game.num_hands(1),
        solver.memory_bytes() as f64 / 1e6
    );

    let t0 = Instant::now();
    let mut report = solver.report();
    while solver.iterations() < a.iters {
        solver.iterate();
        let n = solver.iterations();
        if n.is_multiple_of(25) || n == a.iters {
            report = solver.report();
            println!(
                "iter {n:5}  exploitability {:.3}% pot",
                report.exploitability_pct
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
