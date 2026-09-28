// The merge's base-protection invariant, pinned at the partition that
// enforces it (`dag_merger::partition_base_conflicts`).
//
// Two directions, and both are load-bearing for finality:
//
//  * a scope chain conflicting with the base's committed event log is
//    REJECTED — deterministically, not by cost adjudication: the base is
//    committed and cannot lose;
//  * the base's own content is never adjudicated at all — the partition's
//    input is the scope (the band above the base) and its output is a
//    total split of that input, so nothing the base lineage committed can
//    appear in a rejection set.
//
// Why this matters beyond merge hygiene: certification pins fork choice to
// the certified branch (heaviest-subtree descent), the certified branch is
// every honest proposer's BASE, and this partition is the final link in
// "certified content cannot be merged away". The ucc-i6 divergence was
// exactly a rejection finalizing against content another node had
// certified — possible only because fork choice let the rejecting branch
// win; with fork choice fixed, this invariant closes the loop.

#![allow(clippy::mutable_key_type)]
use std::collections::HashSet;

use casper::rust::merging::dag_merger::partition_base_conflicts;
use casper::rust::merging::deploy_chain_index::{DeployChainIndex, DeployIdWithCost};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};
use shared::rust::hashable_set::HashableSet;

fn produce_on(channel_byte: u8, salt: u8) -> Produce {
    let mut ch = [0u8; 32];
    ch[0] = channel_byte;
    let mut ph = ch;
    ph[4] = salt;
    Produce {
        channel_hash: Blake2b256Hash::from_bytes(ch.to_vec()),
        hash: Blake2b256Hash::from_bytes(ph.to_vec()),
        persistent: false,
        is_deterministic: true,
        output_value: vec![],
        failed: false,
    }
}

fn consume_on(channel_byte: u8, salt: u8) -> Consume {
    let mut ch = [0u8; 32];
    ch[0] = channel_byte;
    let mut cs = ch;
    cs[5] = salt;
    Consume {
        channel_hashes: vec![Blake2b256Hash::from_bytes(ch.to_vec())],
        hash: Blake2b256Hash::from_bytes(cs.to_vec()),
        persistent: false,
    }
}

/// A scope chain with one un-consumed linear produce on `channel_byte`.
fn chain_producing_on(idx: usize, channel_byte: u8) -> DeployChainIndex {
    let mut dh = [0u8; 32];
    dh[29] = idx as u8;
    dh[31] = 0xD1;
    let mut event_log = EventLogIndex::empty();
    let mut produces = HashSet::new();
    produces.insert(produce_on(channel_byte, idx as u8));
    event_log.produces_linear = HashableSet(produces);
    let mut deploys = HashSet::new();
    deploys.insert(DeployIdWithCost {
        deploy_id: Bytes::from(dh.to_vec()),
        cost: 100,
    });
    DeployChainIndex::from_parts(
        HashableSet(deploys),
        Blake2b256Hash::from_bytes(vec![1u8; 32]),
        event_log,
        rspace_plus_plus::rspace::merger::state_change::StateChange::empty(),
        Bytes::from(dh.to_vec()),
        0,
    )
}

/// The base lineage's combined log: an active linear consume waiting on
/// `channel_byte` — committed content a rival produce would race.
fn base_log_consuming_on(channel_byte: u8) -> EventLogIndex {
    let mut log = EventLogIndex::empty();
    let mut consumes = HashSet::new();
    consumes.insert(consume_on(channel_byte, 0x77));
    log.consumes_linear_and_peeks = HashableSet(consumes);
    log
}

/// A chain racing the base's committed consume is rejected; a disjoint
/// chain is kept; the split is total. The rejection is a DECISION — no
/// cost ordering is consulted, so a maximally expensive racing chain
/// loses to the base identically.
#[test]
fn a_chain_racing_the_base_is_rejected_and_a_disjoint_chain_is_kept() {
    let racing = chain_producing_on(0, 0xA0);
    let expensive_racing = chain_producing_on(1, 0xA0);
    let disjoint = chain_producing_on(2, 0xB0);
    let base_log = base_log_consuming_on(0xA0);

    let (kept, rejected) = partition_base_conflicts(
        vec![racing.clone(), expensive_racing.clone(), disjoint.clone()],
        &base_log,
    );

    assert_eq!(
        rejected.len(),
        2,
        "both chains racing the base's committed consume must be rejected"
    );
    assert!(rejected.contains(&racing) && rejected.contains(&expensive_racing));
    assert_eq!(kept, vec![disjoint], "the disjoint chain must be kept");
}

/// The partition is a total split of exactly the scope it was given: no
/// chain is dropped, none invented — so nothing outside the scope (in
/// particular, nothing the base lineage committed) can ever appear in a
/// rejection set. The base's protection is by construction, not by
/// adjudication in its favor.
#[test]
fn the_partition_is_total_over_the_scope_and_the_scope_only() {
    let chains: Vec<DeployChainIndex> = (0..6)
        .map(|i| chain_producing_on(i, if i % 2 == 0 { 0xA0 } else { 0xB0 }))
        .collect();
    let base_log = base_log_consuming_on(0xA0);

    let (kept, rejected) = partition_base_conflicts(chains.clone(), &base_log);

    let mut recombined: Vec<&DeployChainIndex> = kept.iter().chain(rejected.iter()).collect();
    assert_eq!(
        recombined.len(),
        chains.len(),
        "no chain dropped or invented"
    );
    for chain in &chains {
        assert!(
            recombined.contains(&chain),
            "every scope chain must land on exactly one side"
        );
        recombined.retain(|c| *c != chain);
    }
}

/// An empty base log (the parents did not diverge, the base carries no own
/// contribution) rejects nothing — the ordinary multi-parent merge is
/// unaffected by the protection.
#[test]
fn an_empty_base_log_rejects_nothing() {
    let chains: Vec<DeployChainIndex> = (0..4).map(|i| chain_producing_on(i, 0xA0)).collect();
    let (kept, rejected) = partition_base_conflicts(chains.clone(), &EventLogIndex::empty());
    assert!(rejected.is_empty());
    assert_eq!(kept, chains);
}
