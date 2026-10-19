use casper_soak::{manifest, text};
use eyre::{ensure, Result};
use serde_json::Value;

pub fn deferred(member: &Value) -> bool { member["incarnation_binding"] == "observed_restart" }

pub fn validate(member: &Value, faults: &[Value]) -> Result<()> {
    ensure!(
        member["incarnation_binding"].is_null() || deferred(member),
        "The incarnation binding mode is unsupported."
    );
    ensure!(
        deferred(member) == (member["incarnation"] == "pending-restart"),
        "The deferred incarnation marker differs."
    );
    if deferred(member) {
        ensure!(
            faults
                .iter()
                .filter(|fault| fault["member_id"] == member["member_id"]
                    && fault["action"] == "restart"
                    && fault["incarnation"] == member["predecessor_incarnation"])
                .count()
                == 1,
            "Deferred enrollment requires one bound restart."
        );
    }
    Ok(())
}

pub fn enroll(member: &Value, faults: &[Value], ack: &Value) -> Result<Value> {
    validate(member, faults)?;
    ensure!(
        deferred(member),
        "The member does not permit successor enrollment."
    );
    let payload = &ack["payload"];
    let fault = faults
        .iter()
        .find(|fault| {
            fault["member_id"] == member["member_id"] && fault["fault_id"] == payload["fault_id"]
        })
        .ok_or_else(|| eyre::eyre!("The successor receipt is unscheduled."))?;
    ensure!(
        ack["event_kind"] == "fault_ack"
            && ack["presence"] == "observed"
            && ack["reason"].is_null()
            && payload["status"] == "applied"
            && payload["prior_exit"] == true
            && payload["ready"] == true,
        "The successor receipt is incomplete."
    );
    for field in [
        "member_id",
        "node_id",
        "candidate_id",
        "node_revision",
        "node_binary_digest",
        "evaluation_mode",
    ] {
        ensure!(
            ack[field] == member[field] && !ack[field].is_null(),
            "The successor candidate differs: {field}."
        );
    }
    for field in ["fault_id", "action", "incarnation", "trigger_event"] {
        ensure!(
            payload[field] == fault[field] && !payload[field].is_null(),
            "The successor fault differs: {field}."
        );
    }
    let successor = text(&payload["new_incarnation"])?;
    ensure!(
        successor.len() == 36
            && successor.bytes().enumerate().all(|(i, byte)| {
                if [8, 13, 18, 23].contains(&i) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                }
            }),
        "The successor incarnation is invalid."
    );
    ensure!(
        ack["incarnation"] == successor
            && payload["new_incarnation"] != member["predecessor_incarnation"],
        "The successor did not change incarnation."
    );
    ensure!(
        ack["time"]["clock_id"] == fault["ack_deadline"]["clock_id"]
            && manifest::decimal(&ack["time"]["monotonic_ns"])?
                <= manifest::decimal(&fault["ack_deadline"]["monotonic_ns"])?,
        "The successor receipt is late or uses another clock."
    );
    let mut resolved = member.clone();
    resolved["incarnation"] = payload["new_incarnation"].clone();
    Ok(resolved)
}
