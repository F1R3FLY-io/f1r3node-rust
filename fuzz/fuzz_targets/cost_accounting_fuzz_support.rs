//! Shared helpers for cost-accounting fuzz targets.
//!
//! The builders stay deterministic and in-memory so each fuzz iteration checks
//! production serialization, settlement, hashing, and runtime-budget paths
//! without depending on disk state or cross-iteration caches.

#![allow(dead_code)]

use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1_eth::Secp256k1Eth;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, DeployData, F1r3flyState, Header, ProcessedDeploy,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, SourcePath,
    MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES, MAX_COST_TRACE_SOURCE_PATH_COMPONENTS,
};

pub fn runtime_budget(initial: u16, label: &'static str) -> RuntimeBudget {
    RuntimeBudget::new(Cost::create(i64::from(initial), label))
}

pub fn billable_event(
    index: u64,
    tag: u8,
    weight: u64,
    descriptor_len: usize,
    path_len: usize,
) -> BillableTokenEvent {
    let kind = match tag % 3 {
        0 => BillableKind::SourceStep,
        1 => BillableKind::Primitive("p".repeat(descriptor_len)),
        _ => BillableKind::Substitution,
    };
    BillableTokenEvent {
        deploy_id: [tag; 32],
        // D0: per-deploy lane key, keyed off the deploy tag (constant within
        // a deploy, distinct across deploys).
        sig_hash: [tag; 32],
        source_path: SourcePath(vec![u32::from(tag); path_len]),
        redex_id: RedexId(index),
        local_index: index,
        kind,
        weight,
    }
}

pub fn event_is_invalid(event: &BillableTokenEvent) -> bool {
    event.weight == 0
        || event.weight > i64::MAX as u64
        || event.source_path.0.len() > MAX_COST_TRACE_SOURCE_PATH_COMPONENTS
        || matches!(
            &event.kind,
            BillableKind::Primitive(name)
                if name.len() > MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES
        )
}

pub fn deploy_data(phlo_limit: i64, phlo_price: i64) -> DeployData {
    DeployData {
        term: "Nil".to_string(),
        time_stamp: 0,
        phlo_price,
        phlo_limit,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    }
}

pub fn signed_deploy(seed: u8, phlo_limit: i64, phlo_price: i64) -> Signed<DeployData> {
    Signed {
        data: deploy_data(phlo_limit, phlo_price),
        pk: PublicKey::from_bytes(&[seed; 65]),
        sig: Bytes::from(vec![seed.wrapping_add(1); 64]),
        sig_algorithm: Box::new(Secp256k1Eth),
    }
}

pub fn processed_deploy(seed: u8, cost: u64, failed: bool) -> ProcessedDeploy {
    ProcessedDeploy {
        deploy: signed_deploy(seed, 100, 1),
        cost: PCost { cost },
        deploy_log: Vec::new(),
        is_failed: failed,
        system_deploy_error: failed.then(|| "fuzz failure".to_string()),
        cosigners: Vec::new(),
        primary_phlo_share: 0,
        cosigner_threshold: 0,
    }
}

pub fn block_with_deploy(deploy: ProcessedDeploy) -> BlockMessage {
    BlockMessage {
        block_hash: Vec::<u8>::new().into(),
        header: Header {
            parents_hash_list: Vec::new(),
            timestamp: 0,
            version: 1,
            extra_bytes: Vec::<u8>::new().into(),
        },
        body: Body {
            state: F1r3flyState {
                pre_state_hash: vec![0; 32].into(),
                post_state_hash: vec![1; 32].into(),
                bonds: Vec::new(),
                block_number: 0,
            },
            deploys: vec![deploy],
            rejected_deploys: Vec::new(),
            system_deploys: Vec::new(),
            extra_bytes: Vec::<u8>::new().into(),
        },
        justifications: Vec::new(),
        sender: vec![7; 65].into(),
        seq_num: 0,
        sig: Vec::<u8>::new().into(),
        sig_algorithm: "secp256k1".to_string(),
        shard_id: "root".to_string(),
        extra_bytes: Vec::<u8>::new().into(),
    }
}
