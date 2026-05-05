// Shared helpers for the Track 2 production-path integration
// tests and Track 3 triple-bisimilarity proptests.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.3.5
// (production-path integration), §14.5 (cross-tier bisim).
// Plan-agent design from session ending at commit 030336a.

#![allow(dead_code)]

use std::collections::HashMap;

use casper::rust::errors::CasperError;
use casper::rust::util::proto_util;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisContext;

use super::production_adapter::SlashingProductionAdapter;

/// Capture a production-tier snapshot at the post-state hash of
/// `block`. Reads bonds and active set from the node's
/// `RuntimeManager`, projects equivocation records from the node's
/// `BlockDagKeyValueStorage`, and computes `coop_vault` via the
/// stake-conservation formula.
///
/// `validators` is the canonical "v0", "v1", ... mapping established
/// by `canonical_validator_order(genesis)`.
pub async fn production_snapshot_at(
    node: &TestNode,
    block: &BlockMessage,
    genesis_block: &BlockMessage,
    validators: Vec<Bytes>,
) -> Result<SlashingProductionAdapter, CasperError> {
    let post_state_hash = proto_util::post_state_hash(block);

    let bonds = node
        .runtime_manager
        .compute_bonds(&post_state_hash)
        .await?;
    let active_set = node
        .runtime_manager
        .get_active_validators(&post_state_hash)
        .await?;

    // Convert Bond { validator: ByteString, stake: i64 } to the
    // adapter's expected `&[(Bytes, i64)]`.
    let bonds_table: Vec<(Bytes, i64)> = bonds
        .iter()
        .map(|b| (b.validator.clone(), b.stake))
        .collect();
    let active_table: Vec<Bytes> = active_set.iter().map(|v| v.clone()).collect();

    // Coop-vault formula per design §14.6 stake-conservation
    // invariant: genesis_total_stake = sum(current_bonds) + coop_vault
    // (modulo bond-floor residue from already-slashed validators).
    let genesis_total: i64 = genesis_block.body.state.bonds.iter().map(|b| b.stake).sum();
    let current_total: i64 = bonds.iter().map(|b| b.stake).sum();
    // Bond-floor residue: bonds at the test-mode floor (typically 1)
    // are validators that have been slashed but still hold the floor
    // amount. `coop_vault = genesis_total - current_total - residue`.
    let bond_floor: i64 = 1;
    let residue: i64 = bonds.iter().filter(|b| b.stake == bond_floor).count() as i64 * bond_floor;
    let coop_vault = genesis_total - current_total - residue;

    SlashingProductionAdapter::snapshot(
        validators,
        &bonds_table,
        &active_table,
        &node.block_dag_storage,
        coop_vault.max(0),
    )
    .map_err(CasperError::RuntimeError)
}

/// Build the canonical "v0", "v1", ... validator-label mapping
/// from a genesis block's bond list. The order is determined by
/// the genesis bond entries' iteration order, which mirrors the
/// `validator_key_pairs` order in `GenesisContext`.
pub fn canonical_validator_order(genesis: &GenesisContext) -> Vec<Bytes> {
    genesis
        .validator_pks()
        .into_iter()
        .map(|pk| pk.bytes)
        .collect()
}

/// Map a validator's public-key bytes back to its harness label.
/// `None` if the validator is not in the canonical order.
pub fn label_for(validator_bytes: &Bytes, validators: &[Bytes]) -> Option<String> {
    validators
        .iter()
        .position(|v| v.as_ref() == validator_bytes.as_ref())
        .map(|i| format!("v{}", i))
}

/// Build a label → bond mapping from the production tier's bonds
/// list, indexed by the canonical order.
pub fn bonds_by_label(
    bonds: &[(Bytes, i64)],
    validators: &[Bytes],
) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for (b, stake) in bonds {
        if let Some(label) = label_for(b, validators) {
            out.insert(label, *stake);
        }
    }
    out
}
