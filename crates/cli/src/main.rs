//! `hexas` CLI. Subcommands (solve / bench / compare) arrive with M1–M2.

use hexas_core::cards::parse_cards;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => {
            println!("hexas {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("cards") if args.len() == 2 => match parse_cards(&args[1]) {
            Ok(cards) => {
                for c in cards {
                    println!(
                        "{c}\tindex {:2}  rank {:2}  suit {}",
                        c.index(),
                        c.rank(),
                        c.suit()
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: hexas --version | hexas cards <cards, e.g. AsKd7c>");
            ExitCode::FAILURE
        }
    }
}
