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
