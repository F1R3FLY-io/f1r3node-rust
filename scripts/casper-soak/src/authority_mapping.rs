use std::collections::BTreeSet;

use casper_soak::{array, manifest, number, object, text};
use eyre::{ensure, eyre, Result};
use serde_json::{json, Value};

fn absent(reason: &str) -> Value { json!({"presence":"missing","value":null,"reason":reason}) }

fn observed(value: Value) -> Value { json!({"presence":"observed","value":value,"reason":null}) }

fn available<'a>(value: &'a Value, digest: Option<&str>) -> Result<Option<&'a Value>> {
    let fields = object(value)?;
    match text(&value["availability"])? {
        "available" => {
            ensure!(
                fields.len() == 3,
                "The available value has unexpected fields."
            );
            manifest::hex(&value["input_digest"], 64)?;
            ensure!(
                digest.is_none_or(|d| value["input_digest"] == d),
                "The observation input digest differs."
            );
            let payload = fields
                .get("value")
                .ok_or_else(|| eyre!("The value is absent."))?;
            ensure!(!payload.is_null(), "The available value is null.");
            Ok(Some(payload))
        }
        "unavailable" | "failed" => {
            ensure!(fields.len() == 3, "The absent value has unexpected fields.");
            ensure!(
                !text(&value["reason"])?.is_empty(),
                "The absence reason is empty."
            );
            if !value["input_digest"].is_null() {
                manifest::hex(&value["input_digest"], 64)?;
                ensure!(
                    digest.is_none_or(|d| value["input_digest"] == d),
                    "The unavailable input digest differs."
                );
            }
            Ok(None)
        }
        "not_requested" => {
            ensure!(
                fields.len() == 1,
                "The unrequested value has unexpected fields."
            );
            Ok(None)
        }
        _ => Err(eyre!("The observation availability is unsupported.")),
    }
}

pub fn binary32(bits: u32) -> Value {
    let exponent = (bits >> 23) & 255;
    let fraction = bits & 0x7f_ffff;
    let rational = if exponent == 255 {
        absent("non_finite_binary32")
    } else if exponent == 0 && fraction == 0 {
        observed(json!({"numerator":"0","denominator":"1"}))
    } else {
        let significand = if exponent == 0 {
            fraction
        } else {
            fraction | (1 << 23)
        };
        let trailing = significand.trailing_zeros();
        let magnitude = (significand >> trailing) as u128;
        let power = if exponent == 0 {
            -149
        } else {
            exponent as i32 - 150
        } + trailing as i32;
        let ratio = if power >= 0 {
            magnitude
                .checked_mul(1u128.checked_shl(power as u32).unwrap_or(0))
                .filter(|_| power < 128)
                .map(|n| (n, 1u128))
        } else {
            1u128.checked_shl((-power) as u32).map(|d| (magnitude, d))
        };
        match ratio {
            Some((n, d))
                if d <= u64::MAX as u128 && n <= i64::MAX as u128 + u128::from(bits >> 31 != 0) =>
            {
                let numerator = if bits >> 31 != 0 {
                    -(n as i128)
                } else {
                    n as i128
                };
                observed(json!({"numerator":numerator.to_string(),"denominator":d.to_string()}))
            }
            _ => absent("binary32_outside_profile_fraction_domain"),
        }
    };
    json!({"encoding":"ieee754-binary32","bits":bits,"fraction":rational})
}

fn float_observation(value: &Value, digest: Option<&str>) -> Result<Value> {
    let converted = available(value, digest)?
        .map(|v| -> Result<Value> { Ok(binary32(number(v)?.try_into()?)) })
        .transpose()?;
    Ok(json!({"source":value,"numeric":converted}))
}

fn signed(value: &Value) -> Result<i128> {
    Ok(value
        .as_i64()
        .ok_or_else(|| eyre!("A signed integer is required."))? as i128)
}

fn decision(target: &Value, digest: Option<&str>) -> Result<Value> {
    let comparator = text(&target["comparator"])?;
    ensure!(
        ["gt", "ge"].contains(&comparator),
        "The comparator is unsupported."
    );
    let n = signed(&target["threshold_numerator"])?;
    let d = signed(&target["threshold_denominator"])?;
    ensure!(
        d > 0 && (-d..=d).contains(&n),
        "The threshold is outside the node domain."
    );
    let reported = available(&target["oracle_decision"], digest)?
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| eyre!("The oracle decision is not Boolean."))
        })
        .transpose()?;
    let witness = available(&target["oracle_witness"], digest)?;
    ensure!(
        reported.is_some() == witness.is_some(),
        "The oracle witness availability differs."
    );
    let mut recomputed = None;
    if let Some(w) = witness {
        ensure!(
            w["decision"].as_bool() == reported,
            "The witness decision differs."
        );
        let computed = match w["early_return"].as_str() {
            Some("target_not_held") => {
                ensure!(
                    w["total_stake"].is_null()
                        && w["agreeing_stake"].is_null()
                        && w["clique_weight"].is_null(),
                    "The absent target has stake values."
                );
                false
            }
            Some("non_positive_stake") => {
                ensure!(
                    signed(&w["total_stake"])? <= 0
                        && w["agreeing_stake"].is_null()
                        && w["clique_weight"].is_null(),
                    "The nonpositive stake witness differs."
                );
                false
            }
            Some("no_strict_majority") => {
                let total = signed(&w["total_stake"])?;
                let agreeing = signed(&w["agreeing_stake"])?;
                ensure!(
                    total > 0
                        && agreeing >= 0
                        && agreeing * 2 <= total
                        && w["clique_weight"].is_null(),
                    "The majority witness differs."
                );
                false
            }
            None if w["early_return"].is_null() => {
                let total = signed(&w["total_stake"])?;
                let agreeing = signed(&w["agreeing_stake"])?;
                let clique = signed(&w["clique_weight"])?;
                ensure!(
                    total > 0
                        && agreeing <= total
                        && agreeing * 2 > total
                        && clique >= 0
                        && clique <= agreeing,
                    "The stake witness is outside its domain."
                );
                let lhs = 2 * clique * d;
                let rhs = total * (d + n);
                if comparator == "gt" {
                    lhs > rhs
                } else {
                    lhs >= rhs
                }
            }
            _ => return Err(eyre!("The oracle early return is unsupported.")),
        };
        ensure!(
            reported == Some(computed),
            "The exact threshold result differs from the witness."
        );
        recomputed = Some(computed);
    }
    Ok(json!({
        "source":target["oracle_decision"],"witness":target["oracle_witness"],
        "recomputed":recomputed,"comparator":comparator,
        "threshold":{"numerator":n.to_string(),"denominator":d.to_string()},
        "profile_threshold_compatible":comparator == "ge" && n >= 0
    }))
}

fn target(value: &Value, snapshot: &str, strict: bool) -> Result<Value> {
    manifest::hex(&value["target"], 64)?;
    ensure!(
        value["input_scope"] == "captured_latest_messages",
        "The target input scope differs."
    );
    ensure!(
        value["comparator"] == if strict { "gt" } else { "ge" },
        "The requested comparator differs."
    );
    let fields = [
        "oracle_decision",
        "oracle_witness",
        "original_fault_tolerance",
        "display_projection",
        "reference_comparison",
    ];
    let mut digest = None;
    for field in fields {
        available(&value[field], None)?;
        if let Some(candidate) = value[field]["input_digest"].as_str() {
            ensure!(
                digest.is_none_or(|d| d == candidate),
                "The target observations use different inputs."
            );
            digest = Some(candidate);
        }
    }
    let persisted = &value["persisted_fault_tolerance"];
    let persisted_value = available(persisted, Some(snapshot))?;
    let finalized = if persisted_value.is_some() {
        observed(json!(true))
    } else if persisted["availability"] == "unavailable"
        && persisted["reason"] == "not_finalized"
        && persisted["input_digest"] == snapshot
    {
        observed(json!(false))
    } else {
        absent("persisted_finality_unavailable")
    };
    Ok(json!({
        "target":value["target"],"evaluation_input_digest":digest,
        "oracle":decision(value, digest)?,
        "original_ft":float_observation(&value["original_fault_tolerance"], digest)?,
        "display_projection":float_observation(&value["display_projection"], digest)?,
        "persisted_ft":float_observation(persisted, Some(snapshot))?,
        "persisted_finalized":finalized,
        "reference_comparison":value["reference_comparison"],
        "selected_head":absent("fork_choice_observation_unavailable")
    }))
}

fn work(value: &Value) -> Result<Value> {
    fn usage(value: &Value) -> Result<()> {
        for field in [
            "traversal",
            "metadata",
            "oracle",
            "clique",
            "signature",
            "allocation",
            "operations",
            "allocated_bytes",
            "clique_expansions",
            "maximum_depth",
        ] {
            number(&value[field])?;
        }
        Ok(())
    }
    for field in ["aggregate", "preparation", "measured"] {
        usage(&value[field])?;
    }
    for field in ["original", "reference"] {
        if let Some(v) = available(&value[field], None)? {
            usage(v)?;
        }
    }
    let complete = value["complete"]
        .as_bool()
        .ok_or_else(|| eyre!("The work completion flag is absent."))?;
    if complete {
        ensure!(value["failure"].is_null(), "Complete work has a failure.");
    } else {
        ensure!(
            !text(&value["failure"])?.is_empty(),
            "Incomplete work has no failure."
        );
    }
    Ok(json!({"scope":"whole_authority_request","source":value,
        "visited_vertices":absent("distinct_vertex_counter_unavailable"),
        "traversed_edges":absent("edge_counter_unavailable")}))
}

pub fn map(response: &Value) -> Result<Value> {
    let mut mapped = json!({
        "schema_version":1,"status":"mapped","scope":"detached-authority-observations",
        "identity":response["identity"],"request_id":response["request_id"],
        "request_sha256":response["request_sha256"],
        "clock":response["clock"],"monotonic_ns":response["monotonic_ns"],
        "sequence":response["sequence"],"qualification":"pending",
        "profile_verdict":"blocked","soak_verdict":"non_passing",
        "blocked_reasons":["live_adapter_unqualified","fixture_application_unbound",
            "profile_input_artifacts_unbound","paired_fork_choice_unavailable"],
        "targets":[]
    });
    if response["result"]["availability"] == "unavailable" {
        ensure!(
            !text(&response["result"]["reason"])?.is_empty(),
            "The authority absence reason is empty."
        );
        mapped["authority"] = response["result"].clone();
        return Ok(mapped);
    }
    ensure!(
        response["result"]["availability"] == "available",
        "The authority availability is unsupported."
    );
    let value = &response["result"]["value"];
    ensure!(
        value["scope"] == "batch-b2-detached-authority-evaluation"
            && value["live_profile_qualified"] == false,
        "The authority scope differs."
    );
    manifest::hex(&value["snapshot_digest"], 64)?;
    manifest::hex(&value["authority_digest"], 64)?;
    let snapshot = text(&value["snapshot_digest"])?;
    let selected = array(&value["request"]["targets"])?;
    let targets = array(&value["targets"])?;
    ensure!(
        targets.len() == selected.len() && targets.len() <= 16,
        "The target inventory differs."
    );
    let strict = value["request"]["strict"]
        .as_bool()
        .ok_or_else(|| eyre!("The strict mode is absent."))?;
    let mut seen = BTreeSet::new();
    let mut converted = Vec::new();
    for (item, expected) in targets.iter().zip(selected) {
        ensure!(
            &item["target"] == expected && seen.insert(text(expected)?),
            "The target selection differs or repeats."
        );
        converted.push(target(item, snapshot, strict)?);
    }
    available(&value["floor_result"], None)?;
    available(&value["floor_comparison"], None)?;
    array(&value["events"])?;
    mapped["snapshot_digest"] = value["snapshot_digest"].clone();
    mapped["authority_digest"] = value["authority_digest"].clone();
    mapped["targets"] = json!(converted);
    mapped["floor"] = json!({"scope":"detached_derivation","result":value["floor_result"],
        "comparison":value["floor_comparison"],"establishes_persisted_finality":false});
    mapped["events"] = value["events"].clone();
    mapped["coverage"] = value["coverage"].clone();
    mapped["work"] = work(&value["work"])?;
    Ok(mapped)
}
