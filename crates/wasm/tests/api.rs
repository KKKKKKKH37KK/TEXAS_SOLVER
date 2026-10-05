//! The JSON command interface, run natively.

use hexas_wasm::handle;
use serde_json::Value;

fn call(req: &str) -> Value {
    handle(req).unwrap_or_else(|e| panic!("{req}: {e}"))
}

const SPOT: &str = r#"{"board":"Qs9h5d3c2s","oop":"AA-22,AKs-A2s,KQs","ip":"TT-22,AQs-A2s,KQo","pot":20,"stack":90}"#;

#[test]
fn estimate_create_solve_view() {
    let e = call(&format!(r#"{{"cmd":"estimate","spot":{SPOT}}}"#));
    let c = call(&format!(r#"{{"cmd":"create","spot":{SPOT}}}"#));
    assert_eq!(e["bytes"], c["bytes"]);
    assert_eq!(e["hands"], c["hands"]);

    assert_eq!(call(r#"{"cmd":"step","n":30}"#)["iteration"], 30);
    let r = call(r#"{"cmd":"report"}"#);
    assert!(r["exploitabilityPct"].as_f64().unwrap() < 5.0);

    let root = call(r#"{"cmd":"view","path":[],"ev":true}"#);
    assert_eq!(root["kind"], "action");
    assert_eq!(root["player"], 0);
    assert_eq!(root["pot"], 20.0);
    assert_eq!(root["actions"][0]["kind"], "check");
    let n = root["hands"][0].as_array().unwrap().len();
    assert_eq!(
        root["strategy"].as_array().unwrap().len(),
        n * root["actions"].as_array().unwrap().len()
    );
    assert_eq!(root["ev"][0].as_array().unwrap().len(), n);

    let after = call(r#"{"cmd":"view","path":[{"a":1},{"a":1}]}"#);
    assert_eq!(after["kind"], "showdown");
    assert_eq!(after["pot"], 20.0 + 2.0 * 6.6);

    call(r#"{"cmd":"destroy"}"#);
    assert!(handle(r#"{"cmd":"report"}"#).is_err());
}

#[test]
fn preflop_session() {
    call(r#"{"cmd":"preflopCreate"}"#);
    assert_eq!(call(r#"{"cmd":"preflopStep","n":20}"#)["iteration"], 20);
    let r = call(r#"{"cmd":"preflopReport"}"#);
    assert_eq!(r["ev"].as_array().unwrap().len(), 6);
    let root = call(r#"{"cmd":"preflopView","path":[]}"#);
    assert_eq!(root["kind"], "action");
    assert_eq!(root["player"], 0);
    assert_eq!(root["actions"][1], "Raise 2.5");
    assert_eq!(root["strategy"].as_array().unwrap().len(), 2 * 169);
    assert_eq!(root["classes"][0], "AA");
    // UTG opens, everyone folds: UTG wins.
    let won = call(r#"{"cmd":"preflopView","path":[1,0,0,0,0,0]}"#);
    assert_eq!(won["kind"], "fold");
    assert_eq!(won["players"][0], 0);
}

#[test]
fn library_flop_is_shown_in_real_suits() {
    use hexas_core::export::write;
    use hexas_core::holdem::build;
    use hexas_core::solver::{DcfrParams, Solver};
    use hexas_core::spec::SpotSpec;

    // The real flop KhTh4c maps to a canonical flop; build a file for the canonical one.
    let c = call(r#"{"cmd":"canonicalFlop","board":"KhTh4c"}"#);
    let name = c["name"].as_str().unwrap().to_string();
    let spec = SpotSpec {
        board: name.clone(),
        oop: "AA,KK,AKs".into(),
        ip: "QQ,JJ,AQs".into(),
        pot: 10.0,
        stack: 20.0,
        ..Default::default()
    };
    let game = build(&spec.to_spot().unwrap()).unwrap();
    let mut s = Solver::new(&game, DcfrParams::default());
    for _ in 0..5 {
        s.iterate();
    }
    let bytes = write(
        &s,
        &SpotSpec::describe(&spec.to_spot().unwrap(), &spec.oop, &spec.ip),
        3,
    );
    hexas_wasm::load(&bytes).unwrap();
    call(&format!(r#"{{"cmd":"importMap","map":{}}}"#, c["map"]));

    let root = call(r#"{"cmd":"view","path":[],"source":"import"}"#);
    let mut board: Vec<String> = root["board"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    board.sort();
    assert_eq!(board, ["4c", "Kh", "Th"]);
    // AKs in the real suits: hearts are suited with the flop.
    let hands: Vec<&str> = root["hands"][0]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    // AKs in real suits: Kh is on the board, so exactly the other three remain.
    let aks: Vec<&&str> = hands
        .iter()
        .filter(|h| {
            h.starts_with('A') && h.as_bytes()[2] == b'K' && h.as_bytes()[1] == h.as_bytes()[3]
        })
        .collect();
    assert_eq!(aks.len(), 3, "{aks:?}");
    assert!(hands.contains(&"AsKs") && hands.contains(&"AdKd") && hands.contains(&"AcKc"));
    assert!(
        !hands.iter().any(|h| h.contains("Kh")),
        "Kh is on the board"
    );
}

#[test]
fn errors_are_reported() {
    assert!(handle("not json").is_err());
    assert!(
        handle(
            r#"{"cmd":"estimate","spot":{"board":"Qs9h","oop":"AA","ip":"KK","pot":1,"stack":1}}"#
        )
        .is_err()
    );
    assert!(handle(r#"{"cmd":"estimate","spot":{"board":"Qs9h5d","oop":"AAx","ip":"KK","pot":1,"stack":1}}"#).is_err());
}
