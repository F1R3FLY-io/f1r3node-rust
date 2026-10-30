use std::collections::HashSet;

use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;

use super::body::V6Body;

#[derive(Clone, Debug)]
pub struct Limits {
    pub demand_cap: i64,
    pub max_envelopes: usize,
    pub max_records: usize,
    pub min_phlo_price: i64,
}

impl Default for Limits {
    fn default() -> Self { super::resource_policy::ShardResourcePolicy::default().limits() }
}

impl Limits {
    pub fn with_floor(self, min_phlo_price: i64) -> Self {
        Limits {
            min_phlo_price,
            ..self
        }
    }
}

pub fn check_structure(
    block: &BlockMessage,
    limits: &Limits,
    block_number_of: &dyn Fn(&BlockHash) -> Option<i64>,
) -> Result<V6Body, String> {
    let body = &block.body;
    let number = body.state.block_number;
    let ext = V6Body::decode(&body.extra_bytes)?
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
        if deploy.deploy.data.phlo_price < limits.min_phlo_price {
            return Err(format!(
                "envelope phlo price {} is below the shard floor {}",
                deploy.deploy.data.phlo_price, limits.min_phlo_price
            ));
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
    let mut seen = HashSet::new();
    for record in &ext.records {
        if record.target == block.block_hash {
            return Err("a block cannot carry its own record".to_string());
        }
        if !seen.insert((record.target.clone(), record.pre_root.clone())) {
            return Err("duplicate record".to_string());
        }
        match block_number_of(&record.target) {
            Some(n) if n < number => {}
            _ => return Err("a record must name an earlier block in the dag".to_string()),
        }
    }
    Ok(ext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::v6::body::{Attestation, Record};
    use crate::rust::v6::resource_policy::ShardResourcePolicy;
    use models::rhoapi::PCost;
    use models::rust::casper::protocol::casper_message::{
        Body, F1r3flyState, Header, ProcessedDeploy,
    };
    use prost::bytes::Bytes;

    fn h(s: &str) -> Bytes {
        Bytes::copy_from_slice(s.as_bytes())
    }

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
                extra_bytes: ext.encode(),
                applied_from_scope: vec![],
                merge_base: Bytes::new(),
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

    fn ext(records: Vec<Record>) -> V6Body {
        V6Body {
            attestation: Some(Attestation {
                block_hash: h("E"),
                root: h("r"),
            }),
            records,
        }
    }

    fn record(target: &str) -> Record {
        Record::empty(h(target), h("p"), h("q"))
    }

    fn envelope(ts: i64, phlo_limit: i64) -> ProcessedDeploy {
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
        ProcessedDeploy::empty(signed)
    }

    fn ordered_block(envelopes: Vec<ProcessedDeploy>) -> BlockMessage {
        let mut b = block(&ext(vec![record("t")]), "r", "r");
        b.body.deploys = envelopes;
        b
    }

    #[test]
    fn ordered_envelopes_round_trip_through_the_wire_format() {
        let b = ordered_block(vec![envelope(1, 100), envelope(2, 200)]);
        let decoded = BlockMessage::from_proto(b.to_proto()).unwrap();
        assert_eq!(decoded, b);
        let body = check_structure(&decoded, &Limits::default(), &known).unwrap();
        assert_eq!(body, ext(vec![record("t")]));
    }

    #[test]
    fn an_executed_envelope_is_not_an_ordered_envelope() {
        let mut ran = envelope(1, 100);
        ran.cost = PCost { cost: 5 };
        assert!(check_structure(&ordered_block(vec![ran]), &Limits::default(), &known).is_err());
        let mut failed = envelope(1, 100);
        failed.is_failed = true;
        assert!(check_structure(&ordered_block(vec![failed]), &Limits::default(), &known).is_err());
    }

    #[test]
    fn the_policy_bounds_envelopes_and_demand() {
        let limits = ShardResourcePolicy {
            max_envelopes: 2,
            demand_cap: 250,
            ..ShardResourcePolicy::default()
        }
        .limits();
        let fits = ordered_block(vec![envelope(1, 100), envelope(2, 150)]);
        assert!(check_structure(&fits, &limits, &known).is_ok());
        let too_many = ordered_block(vec![envelope(1, 10), envelope(2, 10), envelope(3, 10)]);
        assert!(check_structure(&too_many, &limits, &known).is_err());
        let over_cap = ordered_block(vec![envelope(1, 100), envelope(2, 151)]);
        assert!(check_structure(&over_cap, &limits, &known).is_err());
    }

    fn priced(ts: i64, phlo_price: i64) -> ProcessedDeploy {
        let signed = crate::rust::util::construct_deploy::source_deploy(
            format!("@{ts}!({ts})"),
            ts,
            Some(100),
            Some(phlo_price),
            None,
            None,
            Some("root".to_string()),
        )
        .unwrap();
        ProcessedDeploy::empty(signed)
    }

    #[test]
    fn an_envelope_below_the_shard_floor_makes_the_block_invalid() {
        let limits = Limits::default().with_floor(5);
        let at_floor = ordered_block(vec![priced(1, 5), priced(2, 9)]);
        assert!(check_structure(&at_floor, &limits, &known).is_ok());
        let below = ordered_block(vec![priced(1, 5), priced(2, 4)]);
        let err = check_structure(&below, &limits, &known).unwrap_err();
        assert!(err.contains("below the shard floor"), "{err}");
        assert!(check_structure(&below, &Limits::default(), &known).is_ok());
    }

    #[test]
    fn a_well_formed_block_passes() {
        assert!(check_structure(
            &block(&ext(vec![record("t")]), "r", "r"),
            &Limits::default(),
            &known
        )
        .is_ok());
    }

    #[test]
    fn state_hashes_must_be_the_attested_root() {
        assert!(check_structure(
            &block(&ext(vec![]), "r", "other"),
            &Limits::default(),
            &known
        )
        .is_err());
    }

    #[test]
    fn records_must_name_known_earlier_blocks() {
        assert!(check_structure(
            &block(&ext(vec![record("zz")]), "r", "r"),
            &Limits::default(),
            &known
        )
        .is_err());
    }

    #[test]
    fn duplicate_records_are_rejected() {
        let e = ext(vec![record("t"), record("t")]);
        assert!(check_structure(&block(&e, "r", "r"), &Limits::default(), &known).is_err());
    }

    #[test]
    fn the_merge_base_must_be_the_main_parent() {
        let mut b = block(&ext(vec![]), "r", "r");
        b.header.parents_hash_list = vec![h("E")];
        assert!(check_structure(&b, &Limits::default(), &known).is_err());
        b.body.merge_base = h("E");
        assert!(check_structure(&b, &Limits::default(), &known).is_ok());
    }

    #[test]
    fn legacy_blocks_are_not_v6_blocks() {
        let mut b = block(&ext(vec![]), "r", "r");
        b.body.extra_bytes = Bytes::new();
        assert!(check_structure(&b, &Limits::default(), &known).is_err());
    }
}
