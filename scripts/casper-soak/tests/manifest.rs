use std::fs;
use std::os::unix::fs::symlink;

use casper_soak::{manifest, *};
use serde_json::{json, Value};

pub fn sample() -> Value {
    json!({"schema_version":1,"run_id":"fixture-run","phase":"pre_pr216_merge","candidate_id":"fixture-candidate","node_revision":"1".repeat(40),"node_binary_digest":"2".repeat(64),"image_digest":"3".repeat(64),"harness_revision":"4".repeat(40),"external_harness_revision":"5".repeat(40),"source_digests":{"fixture.rs":"6".repeat(64)},"configuration_digest":"7".repeat(64),"profile_id":"authority-finality","profile_digest":"8".repeat(64),"fixture_digest":"9".repeat(64),"expectation_digest":"a".repeat(64),"seed":"1","provider":"docker","policy_variant":"baseline","evidence_kind":"synthetic_fixture","capabilities":{"paired-evaluation":{"status":"unknown"}},"tool_versions":{"fixture":"1"},"bounds":{"scenarios":2,"observations":3},"assumptions":["Synthetic process boundary."],"resource_limits":{"children":1,"timeout_seconds":1},"required_scenarios":["fixture-scenario"],"deadline":{"clock_id":"wall","epoch_seconds":"1"},"merge_gate":null})
}
#[test]
fn strict_json_and_manifest_validation() {
    for bytes in [
        b"[]".as_slice(),
        b"{\"x\":1,\"x\":2}",
        b"{\"x\":{\"a\":1,\"a\":2}}",
        b"{\"x\":NaN}",
        b"{\"x\":1e999}",
        b"\xff",
        b"{} garbage",
    ] {
        assert!(parse(bytes).is_err());
    }
    manifest::validate(&sample()).unwrap();
    for value in [json!(true), json!(1.0), json!(2), json!(null)] {
        let mut m = sample();
        m["schema_version"] = value;
        assert!(manifest::validate(&m).is_err());
    }
    for name in [
        "../source.rs",
        "/source.rs",
        "./source.rs",
        "a//b",
        "a/../b",
        "a\\b",
        ".",
    ] {
        let mut m = sample();
        m["source_digests"] = json!({name:"a".repeat(64)});
        assert!(manifest::validate(&m).is_err());
    }
}
#[test]
fn exact_bytes_and_all_fields_are_immutable() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("manifest.json");
    let output = dir.path().join("output");
    let m = sample();
    let bytes = encoded(&m).unwrap();
    fs::write(&input, &bytes).unwrap();
    let hash = manifest::bind(&input, &output).unwrap();
    assert_eq!(manifest::bind(&input, &output).unwrap(), hash);
    for field in manifest::FIELDS {
        let mut changed = m.clone();
        changed[*field] = json!("changed");
        fs::write(&input, encoded(&changed).unwrap()).unwrap();
        assert!(manifest::bind(&input, &output).is_err(), "{field}");
        assert_eq!(
            fs::read(output.join(".casper-manifest.json")).unwrap(),
            bytes
        );
    }
    let mut equivalent = bytes.clone();
    equivalent.push(b' ');
    fs::write(&input, equivalent).unwrap();
    assert!(manifest::bind(&input, &output).is_err());
    fs::write(&input, &bytes).unwrap();
    fs::write(output.join(".soak-state"), b"ITERATIONS=0\n").unwrap();
    assert!(manifest::bind(&input, &output).is_err());
    fs::write(
        output.join(".soak-checkpoint-state.json"),
        encoded(&json!({"manifest_digest":hash})).unwrap(),
    )
    .unwrap();
    assert!(manifest::bind(&input, &output).is_ok());
}
#[test]
fn unsafe_inputs_do_not_create_runs() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let out = dir.path().join("output");
    fs::write(&source, encoded(&sample()).unwrap()).unwrap();
    let link = dir.path().join("link");
    symlink(&source, &link).unwrap();
    assert!(manifest::bind(&link, &out).is_err());
    assert!(manifest::bind(dir.path(), &out).is_err());
    fs::write(&source, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
    assert!(manifest::bind(&source, &out).is_err());
    assert!(!out.exists());
    fs::write(&source, encoded(&sample()).unwrap()).unwrap();
    fs::create_dir(&out).unwrap();
    fs::write(out.join("existing"), b"data").unwrap();
    assert!(manifest::bind(&source, &out).is_err());
}
