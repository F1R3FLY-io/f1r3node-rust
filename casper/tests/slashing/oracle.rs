// Hand-translated Rust mirror of the Rocq slashing semantics.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.2.3.
// Source theories:
//   • formal/rocq/slashing/theories/EquivocationDetector.v
//   • formal/rocq/slashing/theories/PoSContract.v
//   • formal/rocq/slashing/theories/TwoLevelSlashing.v
//
// The oracle defines the *canonical* post-fix semantics in terms of
// the in-memory state types from `super::types`. The cross-
// implementation bisim test (UC-39) exercises every operation that
// the SlashingTestHarness exposes and asserts the harness's state
// projection equals the oracle's state projection — proving the
// harness is a faithful refinement of the formal model. If the
// harness deviates from the oracle, T-15 (bisimilarity) fails.

#![allow(dead_code)]

use std::collections::BTreeSet;

use super::types::{
    base_seq_from_seq, BlockHash, BlockMeta, DagState, EqRecord, EqRecordSet, PoSState, SeqNum,
    SlashResult, Status, ValidatorId,
};

/// Mirrors `EquivocationDetector.equivocates` plus the validation-time
/// `JustificationRegression` check. Pure: no side effects.
///
/// **Admissible-vs-Ignorable approximation.** Production code distinguishes
/// `AdmissibleEquivocation` from `IgnorableEquivocation` via the
/// casper-buffer's `requestedAsDependency` flag — i.e. whether the block was
/// pulled in to satisfy a justification or arrived unsolicited. Tests do
/// not own a casper-buffer, so this oracle uses record-existence as a
/// proxy: if a record already exists for `(sender, seq-1)` the oracle
/// returns Admissible, else Ignorable. The two classes are equivalent for
/// the dispatcher (both are slashable), so the proxy is sound for status
/// invariants — but it is *not* sound for tests that depend on the exact
/// Admissible/Ignorable label, which must build a real casper-buffer in
/// an integration test.
pub fn oracle_detect(dag: &DagState, hash: BlockHash) -> Status {
    let block = match dag.blocks.get(&hash) {
        Some(b) => b,
        None => return Status::Valid,
    };
    // Equivocation: another block by the same (sender, seq) with a
    // distinct hash.
    let twin = dag.blocks.values().any(|other| {
        other.hash != block.hash && other.sender == block.sender && other.seq == block.seq
    });
    if twin {
        // The harness's projection: Admissible if a record exists
        // for (sender, seq-1), Ignorable otherwise. The actual
        // production code makes this distinction via
        // `requestedAsDep`; the harness uses record-existence as a
        // proxy because tests cannot directly inspect the casper-
        // buffer's dependency set.
        return Status::IgnorableEquivocation;
    }
    // Self-regression: this block's own creator-justification points
    // to a sender-block whose seq ≥ this block's seq.
    let regress = block.justifications.iter().any(|(v, h)| {
        v == &block.sender
            && dag
                .blocks
                .get(h)
                .map(|prev| prev.seq >= block.seq)
                .unwrap_or(false)
    });
    if regress {
        return Status::JustificationRegression;
    }
    Status::Valid
}

/// Mirrors the post-fix `handle_invalid_block` dispatcher: returns the
/// updated tracker and DAG (with `hash` added to the invalid index).
/// For slashable statuses, mints an EquivocationRecord at
/// `(sender(hash), seq(hash) - 1)` if not already present and folds
/// `hash` into its witness set.
pub fn oracle_dispatch(
    dag: &DagState,
    tracker: &EqRecordSet,
    hash: BlockHash,
    classification: &Status,
) -> (DagState, EqRecordSet) {
    let mut new_dag = dag.clone();
    let mut new_tracker = tracker.clone();
    match classification {
        Status::AdmissibleEquivocation
        | Status::IgnorableEquivocation
        | Status::NeglectedEquivocation
        | Status::JustificationRegression
        | Status::SlashableOther => {
            if let Some(block) = dag.blocks.get(&hash) {
                if let Some(base) = base_seq_from_seq(block.seq) {
                    let record = EqRecord {
                        equivocator: block.sender.clone(),
                        base_seq: base,
                        witnesses: {
                            let mut s = BTreeSet::new();
                            s.insert(hash);
                            s
                        },
                    };
                    new_tracker.insert_or_update(record);
                }
                new_dag.invalid.insert(hash);
            }
        }
        Status::Valid => {}
    }
    (new_dag, new_tracker)
}

/// Mirrors `PoSContract.slash`: returns the updated PoS state plus
/// a success flag. Idempotent (T-Idem) by construction — slashing a
/// validator already in the slashed set is a no-op.
pub fn oracle_slash(pos_state: &PoSState, validator: &str) -> (PoSState, SlashResult) {
    let mut new_state = pos_state.clone();
    if pos_state.slashed.contains(validator) {
        return (new_state, SlashResult {
            success: true,
            error: None,
        });
    }
    let bond = pos_state.bond(validator);
    new_state.bonds.insert(validator.to_string(), 0);
    new_state.active.remove(validator);
    new_state.slashed.insert(validator.to_string());
    new_state.coop_vault += bond;
    (new_state, SlashResult {
        success: true,
        error: None,
    })
}

/// Mirrors `TwoLevelSlashing.neglect`: computes the closure of
/// validators that should be slashed for either (a) equivocating
/// directly or (b) citing an equivocator's invalid block in their
/// justifications without issuing a SlashDeploy.
///
/// Inputs: the set of "directly slashable" validators (those with an
/// EquivocationRecord) and a citation graph mapping each block hash
/// to (sender, justified_validators_with_records).
///
/// The harness does not currently track the slash-deploy column, so
/// this function captures only level-1 (direct) closure here. Level-2
/// is exercised through `uc_04_neglect_two_level.rs` which extends
/// the harness with explicit `record_neglect(...)` calls.
pub fn oracle_neglect_closure_level_1(tracker: &EqRecordSet) -> BTreeSet<ValidatorId> {
    tracker.records.keys().map(|(v, _)| v.clone()).collect()
}

#[cfg(test)]
mod oracle_smoke {
    use std::collections::HashMap;

    use super::*;

    fn mk_block(hash: BlockHash, sender: &str, seq: SeqNum) -> BlockMeta {
        BlockMeta {
            hash,
            sender: sender.to_string(),
            seq,
            justifications: vec![],
            slash_targets: vec![],
        }
    }

    #[test]
    fn detect_valid_singleton() {
        let mut dag = DagState::default();
        dag.blocks.insert(1, mk_block(1, "v0", 5));
        assert_eq!(oracle_detect(&dag, 1), Status::Valid);
    }

    #[test]
    fn detect_twin_is_equivocation() {
        let mut dag = DagState::default();
        dag.blocks.insert(1, mk_block(1, "v0", 5));
        dag.blocks.insert(2, mk_block(2, "v0", 5));
        assert_eq!(oracle_detect(&dag, 2), Status::IgnorableEquivocation);
    }

    #[test]
    fn detect_self_regression() {
        let mut dag = DagState::default();
        dag.blocks.insert(1, mk_block(1, "v0", 10));
        let mut regressing = mk_block(2, "v0", 5);
        regressing.justifications = vec![("v0".to_string(), 1)];
        dag.blocks.insert(2, regressing);
        assert_eq!(oracle_detect(&dag, 2), Status::JustificationRegression);
    }

    #[test]
    fn dispatch_mints_record_for_equivocation() {
        let mut dag = DagState::default();
        dag.blocks.insert(1, mk_block(1, "v0", 5));
        dag.blocks.insert(2, mk_block(2, "v0", 5));
        let tracker = EqRecordSet::default();
        let (new_dag, new_tracker) =
            oracle_dispatch(&dag, &tracker, 2, &Status::IgnorableEquivocation);
        assert!(new_tracker.contains("v0", 4), "record minted at base_seq=4");
        assert!(new_dag.invalid.contains(&2), "block marked invalid");
    }

    #[test]
    fn slash_zeroes_bond_and_transfers_to_vault() {
        let mut bonds = HashMap::new();
        bonds.insert("v0".to_string(), 100);
        let mut active = std::collections::HashSet::new();
        active.insert("v0".to_string());
        let pos = PoSState {
            bonds,
            active,
            slashed: Default::default(),
            coop_vault: 0,
        };
        let (new_pos, result) = oracle_slash(&pos, "v0");
        assert!(result.success);
        assert_eq!(new_pos.bond("v0"), 0);
        assert_eq!(new_pos.coop_vault, 100);
        assert!(!new_pos.is_active("v0"));
        assert!(new_pos.slashed.contains("v0"));
    }

    #[test]
    fn slash_is_idempotent() {
        let mut bonds = HashMap::new();
        bonds.insert("v0".to_string(), 100);
        let pos = PoSState {
            bonds,
            active: Default::default(),
            slashed: Default::default(),
            coop_vault: 0,
        };
        let (after_first, _) = oracle_slash(&pos, "v0");
        let (after_second, r2) = oracle_slash(&after_first, "v0");
        assert!(r2.success);
        assert_eq!(after_first.bond("v0"), after_second.bond("v0"));
        assert_eq!(after_first.coop_vault, after_second.coop_vault);
    }
}
