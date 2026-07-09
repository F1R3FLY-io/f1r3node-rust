// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// RocqOracleAdapter — Tier 2 implementation of SlashingObserver
// over the hand-translated Rocq oracle in `oracle.rs`.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.2.4
// (tier model). The oracle provides the formal-mechanization
// projection of the LTS — pure functions over `(DagState,
// EqRecordSet, PoSState)` mirroring the Rocq theorems at
// formal/rocq/slashing/theories/{EquivocationDetector,PoSContract,
// TwoLevelSlashing}.v.
//
// The adapter exists because the oracle is a function library,
// not a stateful object. The adapter holds the oracle's input
// state explicitly so tests can drive it through transitions
// while reading observables via the `SlashingObserver` trait.
//
// Usage pattern:
//   let mut oracle_state = RocqOracleAdapter::default();
//   oracle_state.dispatch_with_status(hash, status);
//   assert!(oracle_state.has_record("v0", 4));

#![allow(dead_code)]

use std::collections::BTreeSet;

use super::observer::SlashingObserver;
use super::oracle::{oracle_dispatch, oracle_slash};
use super::types::{
    BlockHash, BlockMeta, DagState, EqRecordSet, PoSState, SeqNum, SlashResult, Status, ValidatorId,
};

/// Tier 2: Rocq oracle adapter — wraps `oracle.rs` pure functions
/// behind a stateful, mutable interface that mirrors the harness's
/// API.
#[derive(Debug, Clone, Default)]
pub struct RocqOracleAdapter {
    pub dag: DagState,
    pub tracker: EqRecordSet,
    pub pos_state: PoSState,
}

impl RocqOracleAdapter {
    /// Initialize with `validator_count` validators, each at
    /// `stake` (matching `SlashingTestHarness::new`).
    pub fn new(validator_count: usize, stake: i64) -> Self {
        let mut pos_state = PoSState::default();
        for i in 0..validator_count {
            let v = format!("v{}", i);
            pos_state.bonds.insert(v.clone(), stake);
            pos_state.active.insert(v);
        }
        Self {
            dag: DagState::default(),
            tracker: EqRecordSet::default(),
            pos_state,
        }
    }

    /// Insert a synthetic block into the DAG. Mirrors a tier-3
    /// `sign_block` after the harness has computed the hash.
    pub fn insert_block(&mut self, block: BlockMeta) { self.dag.blocks.insert(block.hash, block); }

    /// Apply the oracle's dispatch transition: classifies via the
    /// pure `oracle_dispatch` and folds the result into adapter state.
    pub fn dispatch_with_status(&mut self, hash: BlockHash, status: Status) {
        let (new_dag, new_tracker) = oracle_dispatch(&self.dag, &self.tracker, hash, &status);
        self.dag = new_dag;
        self.tracker = new_tracker;
    }

    /// Apply the oracle's slash transition.
    pub fn execute_slash(&mut self, target: &str) -> SlashResult {
        let (new_pos, result) = oracle_slash(&self.pos_state, target);
        self.pos_state = new_pos;
        result
    }
}

impl SlashingObserver for RocqOracleAdapter {
    fn bond(&self, validator: &str) -> i64 { self.pos_state.bond(validator) }
    fn coop_vault(&self) -> i64 { self.pos_state.coop_vault }
    fn is_active(&self, validator: &str) -> bool { self.pos_state.is_active(validator) }
    fn has_record(&self, validator: &str, base_seq: SeqNum) -> bool {
        self.tracker.contains(validator, base_seq)
    }
    fn record_witnesses(&self, validator: &str, base_seq: SeqNum) -> BTreeSet<BlockHash> {
        self.tracker.witnesses(validator, base_seq)
    }
    fn fork_choice(&self) -> Vec<ValidatorId> {
        let mut out: Vec<String> = self.pos_state.active.iter().cloned().collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod adapter_smoke {
    use super::*;

    #[test]
    fn oracle_adapter_initial_state_matches_harness() {
        let oracle = RocqOracleAdapter::new(3, 100);
        assert_eq!(oracle.bond("v0"), 100);
        assert_eq!(oracle.bond("v3"), 0);
        assert_eq!(oracle.coop_vault(), 0);
        assert!(oracle.is_active("v0"));
        assert!(!oracle.is_active("v3"));
        assert_eq!(oracle.fork_choice(), vec!["v0", "v1", "v2"]);
    }

    #[test]
    fn oracle_adapter_slash_zeros_bond() {
        let mut oracle = RocqOracleAdapter::new(3, 100);
        let r = oracle.execute_slash("v0");
        assert!(r.success);
        assert_eq!(oracle.bond("v0"), 0);
        assert!(!oracle.is_active("v0"));
        assert_eq!(oracle.coop_vault(), 100);
    }

    #[test]
    fn oracle_adapter_dispatch_records() {
        let mut oracle = RocqOracleAdapter::new(2, 100);
        oracle.insert_block(BlockMeta {
            hash: 1,
            sender: "v0".into(),
            seq: 5,
            justifications: vec![],
            slash_targets: vec![],
        });
        oracle.dispatch_with_status(1, Status::IgnorableEquivocation);
        assert!(oracle.has_record("v0", 4));
    }
}
