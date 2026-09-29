#[path = "../src/authority_mapping.rs"]
mod mapping;

use serde_json::{json, Value};

fn available(value: Value, digest: char) -> Value {
    json!({"availability":"available","input_digest":digest.to_string().repeat(64),"value":value})
}

fn missing(reason: &str, digest: char) -> Value {
    json!({"availability":"unavailable","input_digest":digest.to_string().repeat(64),"reason":reason})
}

fn response() -> Value {
    let work = json!({"traversal":9,"metadata":8,"oracle":7,"clique":6,"signature":5,
        "allocation":4,"operations":39,"allocated_bytes":128,"clique_expansions":2,"maximum_depth":1});
    json!({"identity":{"incarnation":"retained"},"request_id":"retained", "request_sha256":"c".repeat(64),
    "sequence":2,"clock":"observer-monotonic","monotonic_ns":10,
    "result":{"availability":"available","value":{
        "scope":"batch-b2-detached-authority-evaluation","live_profile_qualified":false,
        "snapshot_digest":"a".repeat(64),"authority_digest":"b".repeat(64),
        "request":{"targets":["d".repeat(64)],"strict":false},
        "targets":[{
            "target":"d".repeat(64),"input_scope":"captured_latest_messages","comparator":"ge",
            "threshold_numerator":1,"threshold_denominator":3,
            "oracle_decision":available(json!(true), 'c'),
            "oracle_witness":available(json!({"decision":true,"total_stake":3,
                "agreeing_stake":3,"clique_weight":2,"early_return":null}), 'c'),
            "original_fault_tolerance":available(json!((1.0f32/3.0).to_bits()), 'c'),
            "display_projection":missing("equivocation_snapshot_unavailable", 'c'),
            "persisted_fault_tolerance":missing("not_finalized", 'a'),
            "reference_comparison":{"availability":"not_requested"}
        }],
        "floor_result":available(json!({"outcome":"advance","hash":"e".repeat(64),"block_number":7}), 'f'),
        "floor_comparison":{"availability":"not_requested"},"events":[],"coverage":null,
        "work":{"aggregate":work,"preparation":work,"measured":work,
            "original":{"availability":"not_requested"},"reference":{"availability":"not_requested"},
            "complete":true,"failure":null}
    }}})
}

#[test]
fn binary32_preserves_exact_values_and_signed_zero() {
    for (value, numerator, denominator) in [
        (0.0f32, "0", "1"),
        (-0.0, "0", "1"),
        (1.0, "1", "1"),
        (-1.5, "-3", "2"),
        (1.0 / 3.0, "11184811", "33554432"),
        (2f32.powi(-63), "1", "9223372036854775808"),
        (-2f32.powi(63), "-9223372036854775808", "1"),
    ] {
        let result = mapping::binary32(value.to_bits());
        assert_eq!(result["bits"], value.to_bits());
        assert_eq!(
            result["fraction"]["value"],
            json!({"numerator":numerator,"denominator":denominator})
        );
    }
}

#[test]
fn binary32_rejects_nonfinite_and_unrepresentable_rationals_without_losing_bits() {
    for bits in [
        f32::INFINITY.to_bits(),
        f32::NEG_INFINITY.to_bits(),
        0x7fc01234,
        1,
        0x80000001,
        f32::MAX.to_bits(),
        2f32.powi(63).to_bits(),
        2f32.powi(-64).to_bits(),
    ] {
        let result = mapping::binary32(bits);
        assert_eq!(result["bits"], bits);
        assert_eq!(result["fraction"]["presence"], "missing");
        assert!(result["fraction"]["value"].is_null());
    }
}

#[test]
fn binary32_roundtrips_a_deterministic_sample_exactly() {
    let mut bits = 0x13579bdfu32;
    let mut observed = 0;
    for _ in 0..20_000 {
        bits = bits.wrapping_mul(1664525).wrapping_add(1013904223);
        let result = mapping::binary32(bits);
        if result["fraction"]["presence"] == "observed" {
            let rational = &result["fraction"]["value"];
            let n: i64 = rational["numerator"].as_str().unwrap().parse().unwrap();
            let d: u64 = rational["denominator"].as_str().unwrap().parse().unwrap();
            assert_eq!(n as f64 / d as f64, f32::from_bits(bits) as f64);
            observed += 1;
        }
    }
    assert!(observed > 5000);
}

#[test]
fn mapping_separates_oracle_floor_and_persisted_state() {
    let mapped = mapping::map(&response()).unwrap();
    let target = &mapped["targets"][0];
    assert_eq!(target["oracle"]["recomputed"], true);
    assert_eq!(target["persisted_finalized"]["value"], false);
    assert_eq!(mapped["floor"]["result"]["value"]["outcome"], "advance");
    assert_eq!(mapped["floor"]["establishes_persisted_finality"], false);
    assert_eq!(target["selected_head"]["presence"], "missing");
    assert!(target["display_projection"]["numeric"].is_null());
    assert_eq!(mapped["work"]["source"]["measured"]["traversal"], 9);
    assert_eq!(mapped["work"]["traversed_edges"]["presence"], "missing");
    assert_eq!(mapped["profile_verdict"], "blocked");
    assert_eq!(mapped["soak_verdict"], "non_passing");
}

#[test]
fn strict_threshold_boundary_and_negative_threshold_are_not_profile_equivalents() {
    let mut value = response();
    value["result"]["value"]["request"]["strict"] = json!(true);
    let target = &mut value["result"]["value"]["targets"][0];
    target["comparator"] = json!("gt");
    target["oracle_decision"]["value"] = json!(false);
    target["oracle_witness"]["value"]["decision"] = json!(false);
    let mapped = mapping::map(&value).unwrap();
    assert_eq!(mapped["targets"][0]["oracle"]["recomputed"], false);
    assert_eq!(
        mapped["targets"][0]["oracle"]["profile_threshold_compatible"],
        false
    );
    let mut value = response();
    value["result"]["value"]["targets"][0]["threshold_numerator"] = json!(-1);
    let mapped = mapping::map(&value).unwrap();
    assert_eq!(
        mapped["targets"][0]["oracle"]["profile_threshold_compatible"],
        false
    );
}

#[test]
fn mapping_rejects_cross_input_values_bad_witnesses_and_inventory_changes() {
    for (pointer, replacement) in [
        (
            "/result/value/targets/0/original_fault_tolerance/input_digest",
            json!("e".repeat(64)),
        ),
        (
            "/result/value/targets/0/persisted_fault_tolerance/input_digest",
            json!("e".repeat(64)),
        ),
        (
            "/result/value/targets/0/oracle_witness/value/clique_weight",
            json!(1),
        ),
        (
            "/result/value/targets/0/oracle_decision/value",
            json!(false),
        ),
        ("/result/value/targets/0/comparator", json!("gt")),
        ("/result/value/targets/0/threshold_denominator", json!(0)),
        (
            "/result/value/targets/0/original_fault_tolerance/value",
            json!(4294967296u64),
        ),
        ("/result/value/request/targets", json!([])),
        ("/result/value/request/targets/0", json!("f".repeat(64))),
        ("/result/value/work/complete", json!(false)),
        ("/result/value/work/measured/traversal", json!(null)),
        ("/result/value/live_profile_qualified", json!(true)),
    ] {
        let mut value = response();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(mapping::map(&value).is_err(), "{pointer}");
    }
}

#[test]
fn early_returns_do_not_require_an_invented_clique_weight() {
    for (reason, total, agreeing) in [
        ("target_not_held", Value::Null, Value::Null),
        ("non_positive_stake", json!(0), Value::Null),
        ("no_strict_majority", json!(4), json!(2)),
    ] {
        let mut value = response();
        let target = &mut value["result"]["value"]["targets"][0];
        target["oracle_decision"]["value"] = json!(false);
        target["oracle_witness"]["value"] = json!({"decision":false,"total_stake":total,
            "agreeing_stake":agreeing,"clique_weight":null,"early_return":reason});
        assert_eq!(
            mapping::map(&value).unwrap()["targets"][0]["oracle"]["recomputed"],
            false
        );
    }
}

#[test]
fn unavailable_failed_and_persisted_values_keep_their_distinct_meanings() {
    let mut value = response();
    let target = &mut value["result"]["value"]["targets"][0];
    target["oracle_decision"] = missing("budget_exhausted", 'c');
    target["oracle_witness"] = missing("budget_exhausted", 'c');
    target["original_fault_tolerance"] =
        json!({"availability":"failed","input_digest":"c".repeat(64),"reason":"failed"});
    target["persisted_fault_tolerance"] = available(json!(1.0f32.to_bits()), 'a');
    let mapped = mapping::map(&value).unwrap();
    assert_eq!(mapped["targets"][0]["persisted_finalized"]["value"], true);
    assert!(mapped["targets"][0]["oracle"]["recomputed"].is_null());
    assert_eq!(
        mapped["targets"][0]["original_ft"]["source"]["availability"],
        "failed"
    );
    value["result"]["value"]["targets"][0]["persisted_fault_tolerance"] =
        missing("target_not_held", 'a');
    assert_eq!(
        mapping::map(&value).unwrap()["targets"][0]["persisted_finalized"]["presence"],
        "missing"
    );
    value["result"] =
        json!({"availability":"unavailable","input_digest":null,"reason":"not_attached"});
    let mapped = mapping::map(&value).unwrap();
    assert_eq!(mapped["authority"]["reason"], "not_attached");
    assert_eq!(mapped["profile_verdict"], "blocked");
}
