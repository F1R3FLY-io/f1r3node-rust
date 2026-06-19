// SlashingProductionAdapter — Tier 1 implementation of
// SlashingObserver over a real `TestNode`.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.2.4
// (tier model). The adapter projects the observable surface of
// the production types — `BlockDagKeyValueStorage::
// access_equivocations_tracker`, `RuntimeManager::compute_bonds`,
// `RuntimeManager::get_active_validators` — into the harness's
// `(validator-label, base_seq) → witness-set` view.
//
// Production validators are identified by their secp256k1 public-
// key bytes (`ByteString` / `Vec<u8>`); the harness uses short
// labels like "v0", "v1". The adapter is constructed with a
// `validators: Vec<ByteString>` table whose i-th entry is the
// bytes of validator `format!("v{}", i)`. Every observer method
// translates between the two representations transparently.
//
// Mutating operations (process_block, etc.) are NOT part of this
// type — they are tier-specific and live in the integration tests
// of Track 2 / triple-bisim drivers of Track 3.

#![allow(dead_code)]

use std::collections::BTreeSet;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use models::rust::block_hash::BlockHash as ProdBlockHash;
use prost::bytes::Bytes;

use super::observer::SlashingObserver;
use super::types::{BlockHash as HarnessBlockHash, SeqNum, ValidatorId};

/// Tier 1: production adapter — projects observables from a live
/// `BlockDagKeyValueStorage` + `RuntimeManager` snapshot into the
/// harness's label-based view.
///
/// The adapter is a *snapshot*: it captures the observable state
/// at a point in time. Refresh by calling `Self::snapshot(...)`
/// after each production transition.
pub struct SlashingProductionAdapter {
    /// Validator-label → public-key bytes mapping. `validators[i]`
    /// is the production-side identity of harness validator `vi`.
    pub validators: Vec<Bytes>,

    /// Bonds at the snapshot's state hash.
    bonds_by_label: Vec<i64>,

    /// Active set membership at the snapshot's state hash.
    active_by_label: Vec<bool>,

    /// Equivocation records keyed by `(validator-label, base_seq)`.
    /// Witness hashes are 8-byte truncations of the production
    /// 32-byte `BlockHash` — sufficient for the bisim tests'
    /// equality checks because the harness uses `u64`.
    records: Vec<(ValidatorId, SeqNum, BTreeSet<HarnessBlockHash>)>,

    /// Coop-vault balance supplied by the snapshot caller.
    coop_vault: i64,
}

impl SlashingProductionAdapter {
    /// Build a fresh snapshot from a `BlockDagKeyValueStorage` and
    /// the `compute_bonds` output for a given post-state hash.
    /// Caller is responsible for picking a state hash that matches
    /// the harness's expected post-state.
    ///
    /// `validators` is the full ordered list of production
    /// validator public-key bytes; `validators[i]` is the harness
    /// label `vi`.
    ///
    /// `bonds` is the production output of `runtime_manager.
    /// compute_bonds(state_hash)` — the adapter looks up each
    /// validator in this list. Validators not found in `bonds`
    /// have bond 0.
    ///
    /// `active_set` is the output of `get_active_validators`;
    /// validators not in this list are inactive.
    pub fn snapshot(
        validators: Vec<Bytes>,
        bonds: &[(Bytes, i64)],
        active_set: &[Bytes],
        block_dag_storage: &BlockDagKeyValueStorage,
        coop_vault: i64,
    ) -> Result<Self, String> {
        let n = validators.len();
        let mut bonds_by_label = vec![0i64; n];
        let mut active_by_label = vec![false; n];

        for (i, v_bytes) in validators.iter().enumerate() {
            if let Some((_, stake)) = bonds.iter().find(|(b, _)| b == v_bytes) {
                bonds_by_label[i] = *stake;
            }
            if active_set.iter().any(|a| a == v_bytes) {
                active_by_label[i] = true;
            }
        }

        // Project the production equivocation records into the
        // harness's keyed view. Each production record's witness
        // hashes are 32-byte; we 8-byte-truncate (big-endian) into
        // `u64` — this preserves equality / set semantics for the
        // bisim tests because the harness allocates u64 hashes
        // directly and the integration tests' synthetic blocks use
        // hashes that fit in 8 bytes.
        let validators_for_closure = validators.clone();
        let prod_records = block_dag_storage
            .access_equivocations_tracker(|tracker| tracker.data())
            .map_err(|e| format!("access_equivocations_tracker failed: {}", e))?;
        let records: Vec<(ValidatorId, SeqNum, BTreeSet<HarnessBlockHash>)> = prod_records
            .into_iter()
            .filter_map(|prod| {
                let label_idx = validators_for_closure
                    .iter()
                    .position(|v_bytes| v_bytes.as_ref() == prod.equivocator.as_ref())?;
                let label = format!("v{}", label_idx);
                let base_seq = prod.equivocation_base_block_seq_num as SeqNum;
                let witnesses: BTreeSet<HarnessBlockHash> = prod
                    .equivocation_detected_block_hashes
                    .iter()
                    .map(truncate_block_hash)
                    .collect();
                Some((label, base_seq, witnesses))
            })
            .collect();

        Ok(Self {
            validators,
            bonds_by_label,
            active_by_label,
            records,
            coop_vault,
        })
    }

    /// Project a triple-bisim driver's harness-side hash into the
    /// production-side hash. Tests that synthesize blocks need to
    /// keep both representations in sync; this is the conversion
    /// helper.
    pub fn synthesize_witness_hash_pair(seed: u64) -> (HarnessBlockHash, ProdBlockHash) {
        let bytes: Vec<u8> = (0..32)
            .map(|i| ((seed >> ((i % 8) * 8)) & 0xff) as u8)
            .collect();
        (seed, Bytes::from(bytes))
    }
}

/// Truncate a 32-byte production block hash to a 64-bit harness
/// block hash. Big-endian read of the first 8 bytes.
fn truncate_block_hash(h: &ProdBlockHash) -> HarnessBlockHash {
    let bytes = h.as_ref();
    let mut out = [0u8; 8];
    for (i, b) in bytes.iter().take(8).enumerate() {
        out[i] = *b;
    }
    u64::from_be_bytes(out)
}

impl SlashingObserver for SlashingProductionAdapter {
    fn bond(&self, validator: &str) -> i64 {
        self.label_index(validator)
            .map(|i| self.bonds_by_label[i])
            .unwrap_or(0)
    }

    fn coop_vault(&self) -> i64 { self.coop_vault }

    fn is_active(&self, validator: &str) -> bool {
        self.label_index(validator)
            .map(|i| self.active_by_label[i])
            .unwrap_or(false)
    }

    fn has_record(&self, validator: &str, base_seq: SeqNum) -> bool {
        self.records
            .iter()
            .any(|(v, s, _)| v == validator && *s == base_seq)
    }

    fn record_witnesses(&self, validator: &str, base_seq: SeqNum) -> BTreeSet<HarnessBlockHash> {
        self.records
            .iter()
            .find(|(v, s, _)| v == validator && *s == base_seq)
            .map(|(_, _, w)| w.clone())
            .unwrap_or_default()
    }

    fn fork_choice(&self) -> Vec<ValidatorId> {
        let mut out: Vec<ValidatorId> = (0..self.validators.len())
            .filter(|&i| self.active_by_label[i])
            .map(|i| format!("v{}", i))
            .collect();
        out.sort();
        out
    }
}

impl SlashingProductionAdapter {
    fn label_index(&self, validator: &str) -> Option<usize> {
        if let Some(rest) = validator.strip_prefix('v') {
            rest.parse::<usize>().ok()
        } else {
            None
        }
        .filter(|&i| i < self.validators.len())
    }
}
