use std::collections::HashSet;

use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;

use super::body::{V6Body, V6Records};
use super::record::ExecutionRecord;

#[derive(Clone, Debug)]
pub struct Limits {
    pub demand_cap: i64,
    pub max_envelopes: usize,
    pub max_records: usize,
    pub max_record_bytes: usize,
    pub ordered_only: bool,
}

impl Default for Limits {
    fn default() -> Self { super::resource_policy::ShardResourcePolicy::default().limits() }
}

pub fn check_structure<R: ExecutionRecord>(
    block: &BlockMessage,
    limits: &Limits,
    block_number_of: &dyn Fn(&BlockHash) -> Option<i64>,
) -> Result<(V6Body, Vec<R>), String> {
    let body = &block.body;
    let number = body.state.block_number;
    if !body.extra_bytes.is_empty() {
        return Err("a v6 block must not carry body extra bytes".to_string());
    }
    let ext = body
        .v6
        .clone()
        .ok_or_else(|| "a v6 block must carry a v6 body extension".to_string())?;
    let attestation = ext
        .attestation
        .as_ref()
        .ok_or_else(|| "a v6 block must carry an attestation".to_string())?;
    if body.state.pre_state_hash != attestation.root
        || body.state.post_state_hash != attestation.root
    {
        return Err("state hashes must equal the attested root".to_string());
    }
    match block_number_of(&attestation.block_hash) {
        Some(n) if n < number => {}
        _ => return Err("the attested block must be an earlier block in the dag".to_string()),
    }
    if !body.rejected_deploys.is_empty()
        || !body.applied_from_scope.is_empty()
        || !body.system_deploys.is_empty()
    {
        return Err("a v6 block carries no merge results and no system deploys".to_string());
    }
    let main_parent = block
        .header
        .parents_hash_list
        .first()
        .cloned()
        .unwrap_or_default();
    if body.merge_base != main_parent {
        return Err("the merge base of a v6 block must be its main parent".to_string());
    }
    if body.deploys.len() > limits.max_envelopes {
        return Err("too many envelopes".to_string());
    }
    let mut demand: i64 = 0;
    for deploy in &body.deploys {
        if deploy.cost.cost != 0
            || !deploy.deploy_log.is_empty()
            || deploy.is_failed
            || deploy.system_deploy_error.is_some()
        {
            return Err("a v6 envelope must be unexecuted".to_string());
        }
        demand = demand
            .checked_add(deploy.deploy.data.phlo_limit.max(0))
            .ok_or_else(|| "envelope demand overflows".to_string())?;
    }
    if demand > limits.demand_cap {
        return Err(format!(
            "envelope demand {} exceeds the cap {}",
            demand, limits.demand_cap
        ));
    }
    if ext.records.len() > limits.max_records {
        return Err("too many records".to_string());
    }
    if ext.record_bytes() > limits.max_record_bytes {
        return Err(format!(
            "record bytes {} exceed the cap {}",
            ext.record_bytes(),
            limits.max_record_bytes
        ));
    }
    let records: Vec<R> = ext.decode_records()?;
    let mut seen = HashSet::new();
    for record in &records {
        if *record.target() == block.block_hash {
            return Err("a block cannot carry its own record".to_string());
        }
        if !seen.insert((record.target().clone(), record.pre_root().clone())) {
            return Err("duplicate record".to_string());
        }
        match block_number_of(record.target()) {
            Some(n) if n < number => {}
            _ => return Err("a record must name an earlier block in the dag".to_string()),
        }
    }
    Ok((ext, records))
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::{Body, F1r3flyState, Header};
    use prost::bytes::Bytes;
    use prost::Message;

    use super::*;
    use crate::rust::v6::block_record::BlockRecord;
    use crate::rust::v6::body::Attestation;

    fn h(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn block(ext: &V6Body, pre: &str, post: &str) -> BlockMessage {
        BlockMessage {
            block_hash: h("self"),
            header: Header {
                parents_hash_list: vec![],
                timestamp: 0,
                version: 1,
                extra_bytes: Bytes::new(),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: h(pre),
                    post_state_hash: h(post),
                    bonds: vec![],
                    block_number: 5,
                },
                deploys: vec![],
                rejected_deploys: vec![],
                system_deploys: vec![],
                extra_bytes: Bytes::new(),
                applied_from_scope: vec![],
                merge_base: Bytes::new(),
                v6: Some(ext.clone()),
            },
            justifications: vec![],
            sender: Bytes::new(),
            seq_num: 1,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: String::new(),
            extra_bytes: Bytes::new(),
        }
    }

    fn known(hash: &BlockHash) -> Option<i64> {
        match hash.as_ref() {
            b"E" => Some(2),
            b"t" => Some(3),
            _ => None,
        }
    }

    fn ext(records: Vec<BlockRecord>) -> V6Body {
        V6Body::with_records(
            Attestation {
                block_hash: h("E"),
                root: h("r"),
            },
            &records,
        )
    }

    fn check(b: &BlockMessage, limits: &Limits) -> Result<(V6Body, Vec<BlockRecord>), String> {
        check_structure::<BlockRecord>(b, limits, &known)
    }

    fn record(target: &str) -> BlockRecord { BlockRecord::empty(h(target), h("p"), h("q")) }

    #[test]
    fn a_well_formed_block_passes() {
        assert!(check(
            &block(&ext(vec![record("t")]), "r", "r"),
            &Limits::default()
        )
        .is_ok());
    }

    #[test]
    fn state_hashes_must_be_the_attested_root() {
        assert!(check(&block(&ext(vec![]), "r", "other"), &Limits::default()).is_err());
    }

    #[test]
    fn records_must_name_known_earlier_blocks() {
        assert!(check(
            &block(&ext(vec![record("zz")]), "r", "r"),
            &Limits::default()
        )
        .is_err());
    }

    #[test]
    fn duplicate_records_are_rejected() {
        let e = ext(vec![record("t"), record("t")]);
        assert!(check(&block(&e, "r", "r"), &Limits::default()).is_err());
    }

    #[test]
    fn the_merge_base_must_be_the_main_parent() {
        let mut b = block(&ext(vec![]), "r", "r");
        b.header.parents_hash_list = vec![h("E")];
        assert!(check(&b, &Limits::default()).is_err());
        b.body.merge_base = h("E");
        assert!(check(&b, &Limits::default()).is_ok());
    }

    #[test]
    fn legacy_blocks_are_not_v6_blocks() {
        let mut b = block(&ext(vec![]), "r", "r");
        b.body.v6 = None;
        assert!(check(&b, &Limits::default())
            .unwrap_err()
            .contains("must carry a v6 body extension"));
    }

    #[test]
    fn the_old_prefixed_form_is_refused() {
        let e = ext(vec![record("t")]);
        let mut prefixed = e.to_proto().encode_to_vec();
        prefixed.splice(0..0, b"F1R3V6\x02".iter().copied());
        let mut b = block(&e, "r", "r");
        b.body.v6 = None;
        b.body.extra_bytes = Bytes::from(prefixed);
        assert!(check(&b, &Limits::default()).is_err());
        let mut both = block(&e, "r", "r");
        both.body.extra_bytes = Bytes::from_static(b"anything");
        assert!(check(&both, &Limits::default())
            .unwrap_err()
            .contains("must not carry body extra bytes"));
    }

    fn fresh_envelope() -> models::rust::casper::protocol::casper_message::ProcessedDeploy {
        crate::rust::util::construct_deploy::basic_processed_deploy(1, None)
            .expect("basic envelope")
    }

    #[test]
    fn an_envelope_with_nonzero_cost_is_rejected() {
        let mut b = block(&ext(vec![]), "r", "r");
        let mut pd = fresh_envelope();
        pd.cost.cost = 1;
        b.body.deploys = vec![pd];
        let err = check(&b, &Limits::default()).expect_err("a priced envelope must be rejected");
        assert!(
            err.contains("unexecuted"),
            "unexpected error wording: {err}",
        );
    }

    #[test]
    fn an_envelope_with_a_log_is_rejected() {
        let mut b = block(&ext(vec![]), "r", "r");
        let mut pd = fresh_envelope();
        pd.deploy_log.push(
            models::rust::casper::protocol::casper_message::Event::Consume(
                models::rust::casper::protocol::casper_message::ConsumeEvent {
                    channels_hashes: vec![],
                    hash: Bytes::new(),
                    persistent: false,
                },
            ),
        );
        b.body.deploys = vec![pd];
        let err = check(&b, &Limits::default())
            .expect_err("an envelope carrying log entries must be rejected");
        assert!(
            err.contains("unexecuted"),
            "unexpected error wording: {err}",
        );
    }

    #[test]
    fn an_envelope_marked_failed_is_rejected() {
        let mut b = block(&ext(vec![]), "r", "r");
        let mut pd = fresh_envelope();
        pd.is_failed = true;
        b.body.deploys = vec![pd];
        let err = check(&b, &Limits::default()).expect_err("a failed envelope must be rejected");
        assert!(
            err.contains("unexecuted"),
            "unexpected error wording: {err}",
        );
    }

    #[test]
    fn records_over_the_byte_cap_are_rejected() {
        let e = ext(vec![record("t")]);
        let b = block(&e, "r", "r");
        let fits = Limits {
            max_record_bytes: e.record_bytes(),
            ..Limits::default()
        };
        assert!(check(&b, &fits).is_ok());
        let tight = Limits {
            max_record_bytes: e.record_bytes() - 1,
            ..Limits::default()
        };
        let err = check(&b, &tight).expect_err("the record bytes exceed the cap");
        assert!(err.contains("record bytes"), "{err}");
    }

    #[test]
    fn an_unreadable_record_is_rejected() {
        let mut e = ext(vec![]);
        e.records.push(h("not a record"));
        assert!(check(&block(&e, "r", "r"), &Limits::default()).is_err());
    }

    fn envelope(
        ts: i64,
        phlo_limit: i64,
    ) -> models::rust::casper::protocol::casper_message::ProcessedDeploy {
        let signed = crate::rust::util::construct_deploy::source_deploy(
            format!("@{ts}!({ts})"),
            ts,
            Some(phlo_limit),
            Some(0),
            None,
            None,
            Some("root".to_string()),
        )
        .unwrap();
        models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(signed)
    }

    fn ordered_block(
        envelopes: Vec<models::rust::casper::protocol::casper_message::ProcessedDeploy>,
    ) -> BlockMessage {
        let mut b = block(&ext(vec![record("t")]), "r", "r");
        b.body.deploys = envelopes;
        b
    }

    #[test]
    fn ordered_envelopes_round_trip_through_the_wire_format() {
        let b = ordered_block(vec![envelope(1, 100), envelope(2, 200)]);
        let decoded = BlockMessage::from_proto(b.to_proto()).unwrap();
        assert_eq!(decoded, b);
        let (body, records) = check(&decoded, &Limits::default()).unwrap();
        assert_eq!(body, ext(vec![record("t")]));
        assert_eq!(records, vec![record("t")]);
    }

    #[test]
    fn the_policy_bounds_envelopes_and_demand() {
        let limits = crate::rust::v6::resource_policy::ShardResourcePolicy {
            max_envelopes: 2,
            demand_cap: 250,
            ..Default::default()
        }
        .limits();
        let fits = ordered_block(vec![envelope(1, 100), envelope(2, 150)]);
        assert!(check(&fits, &limits).is_ok());
        let too_many = ordered_block(vec![envelope(1, 10), envelope(2, 10), envelope(3, 10)]);
        assert!(check(&too_many, &limits).is_err());
        let over_cap = ordered_block(vec![envelope(1, 100), envelope(2, 151)]);
        assert!(check(&over_cap, &limits).is_err());
    }
}
