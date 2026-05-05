// SlashingTestHarness — state-machine projection of the slashing LTS.
//
// Reference: docs/theory/slashing/design/14-test-plan.md §14.2.1.
//
// The harness keeps three pieces of state in memory:
//   • DagState     — blocks and the invalid-block index (`D`, `I`)
//   • EqRecordSet  — equivocation tracker contents (`E`)
//   • PoSState     — bonds, active set, slashed set, Coop vault (`B`, `A`, `Sl`, `C`)
//
// Operations mutate these projections according to the post-fix LTS
// rules from spec §3 / Rocq theories. Tests then assert post-fix
// invariants (T-1, T-3, T-7, T-8, T-Idem, T-9.M, etc.).
//
// What the harness does NOT do:
//   • exercise the production BlockDagKeyValueStorage / KeyValueBlockStore
//     (the cross-implementation bisim test UC-39 covers that path)
//   • run the rholang interpreter for SlashDeploy execution (the
//     existing `multi_parent_casper_should_succeed_at_slashing`
//     integration test at casper/tests/add_block/* covers that)

#![allow(dead_code)]

use std::collections::{BTreeSet, HashSet};

use super::types::{
    BlockHash, BlockMeta, DagState, EqRecord, EqRecordSet, PoSState, SeqNum, SlashResult, Status,
    ValidatorId,
};

#[derive(Debug, Clone)]
pub struct SlashingTestHarness {
    pub validator_count: usize,
    pub stake_per_validator: i64,
    pub dag: DagState,
    pub tracker: EqRecordSet,
    pub pos_state: PoSState,
    next_hash: BlockHash,
}

impl SlashingTestHarness {
    /// Constructs a harness with `validator_count` validators (labelled
    /// "v0", "v1", ...), each bonded with `stake_per_validator` and
    /// in the active set. Coop vault starts empty.
    pub fn new(validator_count: usize, stake_per_validator: i64) -> Self {
        let mut pos_state = PoSState::default();
        for i in 0..validator_count {
            let v = format!("v{}", i);
            pos_state.bonds.insert(v.clone(), stake_per_validator);
            pos_state.active.insert(v);
        }
        Self {
            validator_count,
            stake_per_validator,
            dag: DagState::default(),
            tracker: EqRecordSet::default(),
            pos_state,
            next_hash: 1,
        }
    }

    /// Signs a fresh block at `(validator, seq)` and adds it to the DAG.
    /// Returns the assigned hash.
    pub fn sign_block(&mut self, validator: &str, seq: SeqNum) -> BlockHash {
        let hash = self.next_hash;
        self.next_hash += 1;
        let creator_just = self
            .dag
            .blocks
            .values()
            .filter(|b| b.sender == validator && b.seq < seq)
            .max_by_key(|b| b.seq)
            .map(|b| (b.sender.clone(), b.hash));
        let justifications = creator_just.into_iter().collect();
        self.dag.blocks.insert(
            hash,
            BlockMeta {
                hash,
                sender: validator.to_string(),
                seq,
                justifications,
            },
        );
        hash
    }

    /// Signs a *second* distinct block at `(validator, seq)`. Used to
    /// construct equivocations: two blocks by the same sender at the
    /// same seq number with distinct hashes.
    pub fn sign_block_distinct(&mut self, validator: &str, seq: SeqNum) -> BlockHash {
        // sign_block always allocates a fresh hash; calling it twice
        // with the same (validator, seq) yields two distinct hashes.
        self.sign_block(validator, seq)
    }

    /// Detects the equivocation/validation status of `hash`. Mirrors
    /// `EquivocationDetector::check_equivocations` plus the
    /// non-equivocation slashable-variant classifier.
    pub fn detect(&self, hash: BlockHash) -> Status {
        let block = match self.dag.blocks.get(&hash) {
            Some(b) => b,
            None => return Status::Valid,
        };
        // Equivocation: any other block by the same sender at the same seq.
        let twin_exists = self
            .dag
            .blocks
            .values()
            .any(|other| other.hash != block.hash && other.sender == block.sender && other.seq == block.seq);
        if twin_exists {
            // Whether it's Admissible or Ignorable depends on whether
            // some other block cites this hash. The test driver controls
            // citation by adding witnesses via `record_neglect`.
            // For the simple harness, default to Admissible if there is
            // an existing record, else Ignorable.
            let base = block.seq.saturating_sub(1);
            return if self.tracker.contains(&block.sender, base) {
                Status::AdmissibleEquivocation
            } else {
                Status::IgnorableEquivocation
            };
        }
        // Self-regression: this block's own justification points to a
        // sender-block whose seq is >= this block's seq.
        let self_regress = block
            .justifications
            .iter()
            .any(|(v, h)| {
                v == &block.sender
                    && self.dag.blocks.get(h).map(|b| b.seq >= block.seq).unwrap_or(false)
            });
        if self_regress {
            return Status::JustificationRegression;
        }
        Status::Valid
    }

    /// Mirrors the dispatcher's `record_evidence` step: mints an
    /// EquivocationRecord under the atomic-tracker invariant.
    pub fn record_equivocation(&mut self, validator: &str, base_seq: SeqNum, witness: BlockHash) {
        let record = EqRecord {
            equivocator: validator.to_string(),
            base_seq,
            witnesses: {
                let mut s = BTreeSet::new();
                s.insert(witness);
                s
            },
        };
        self.tracker.insert_or_update(record);
    }

    /// Mirrors `MultiParentCasperImpl::handle_invalid_block` post-fix
    /// behaviour: classifies the block, mints an EquivocationRecord for
    /// every slashable status (bug fixes #1 + #3), and adds the block
    /// to the invalid index. Returns the classification.
    pub fn dispatch(&mut self, hash: BlockHash) -> Status {
        let status = self.detect(hash);
        self.apply_dispatch_effect(hash, &status);
        status
    }

    /// Mirrors the dispatcher with a *forced* classification — useful
    /// for testing the catch-all arm against the 14 non-equivocation
    /// slashable variants (UC-28..UC-36) without simulating each
    /// validation rule. The provided `status` plays the role of the
    /// upstream validator's verdict.
    pub fn dispatch_with_status(&mut self, hash: BlockHash, status: Status) -> Status {
        self.apply_dispatch_effect(hash, &status);
        status
    }

    fn apply_dispatch_effect(&mut self, hash: BlockHash, status: &Status) {
        match status {
            Status::AdmissibleEquivocation
            | Status::IgnorableEquivocation
            | Status::NeglectedEquivocation
            | Status::JustificationRegression
            | Status::SlashableOther => {
                let (sender, base) = {
                    let block = self
                        .dag
                        .blocks
                        .get(&hash)
                        .expect("dispatch: block exists in DAG");
                    (block.sender.clone(), block.seq.saturating_sub(1))
                };
                self.record_equivocation(&sender, base, hash);
                self.dag.invalid.insert(hash);
            }
            Status::Valid => {}
        }
    }

    pub fn has_record(&self, validator: &str, base_seq: SeqNum) -> bool {
        self.tracker.contains(validator, base_seq)
    }

    pub fn record_witnesses(&self, validator: &str, base_seq: SeqNum) -> BTreeSet<BlockHash> {
        self.tracker.witnesses(validator, base_seq)
    }

    pub fn bond(&self, validator: &str) -> i64 {
        self.pos_state.bond(validator)
    }

    pub fn coop_vault(&self) -> i64 {
        self.pos_state.coop_vault
    }

    pub fn is_active(&self, validator: &str) -> bool {
        self.pos_state.is_active(validator)
    }

    /// Mirrors `PoSContract.slash`: zeroes bonds, removes from active,
    /// adds to slashed, transfers bond into Coop vault. Idempotent
    /// (T-Idem): a second call is a no-op.
    pub fn execute_slash(&mut self, target: &str) -> SlashResult {
        if self.pos_state.slashed.contains(target) {
            return SlashResult { success: true, error: None }; // T-Idem no-op
        }
        let bond = self.pos_state.bond(target);
        self.pos_state.bonds.insert(target.to_string(), 0);
        self.pos_state.active.remove(target);
        self.pos_state.slashed.insert(target.to_string());
        self.pos_state.coop_vault += bond;
        SlashResult { success: true, error: None }
    }

    /// Returns validators counted in the GHOST estimator: active set
    /// minus those whose latest message is invalid (`filterFC` from
    /// spec §3.5 / estimator.rs:65-70). For the simple harness this
    /// is just the active set.
    pub fn fork_choice(&self) -> Vec<String> {
        let mut out: Vec<String> = self.pos_state.active.iter().cloned().collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod harness_smoke {
    use super::*;

    #[test]
    fn harness_initial_state() {
        let h = SlashingTestHarness::new(3, 100);
        assert_eq!(h.bond("v0"), 100);
        assert_eq!(h.bond("v1"), 100);
        assert_eq!(h.bond("v2"), 100);
        assert_eq!(h.bond("v3"), 0); // unknown validator
        assert_eq!(h.coop_vault(), 0);
        assert!(h.is_active("v0"));
        assert!(!h.is_active("v3"));
        assert_eq!(h.fork_choice(), vec!["v0", "v1", "v2"]);
    }

    #[test]
    fn sign_block_assigns_distinct_hashes() {
        let mut h = SlashingTestHarness::new(1, 100);
        let h1 = h.sign_block("v0", 5);
        let h2 = h.sign_block_distinct("v0", 5);
        assert_ne!(h1, h2, "two distinct sign calls yield distinct hashes");
    }

    #[test]
    fn record_equivocation_then_query() {
        let mut h = SlashingTestHarness::new(2, 100);
        let bad = h.sign_block_distinct("v0", 5);
        assert!(!h.has_record("v0", 4), "no record before mint");
        h.record_equivocation("v0", 4, bad);
        assert!(h.has_record("v0", 4));
        assert!(h.record_witnesses("v0", 4).contains(&bad));
    }

    /// T-Idem: slashing twice is a no-op.
    #[test]
    fn slash_is_idempotent() {
        let mut h = SlashingTestHarness::new(2, 100);
        let first = h.execute_slash("v0");
        assert!(first.success);
        assert_eq!(h.bond("v0"), 0);
        assert!(!h.is_active("v0"));
        assert_eq!(h.coop_vault(), 100);
        let second = h.execute_slash("v0");
        assert!(second.success);
        assert_eq!(h.bond("v0"), 0);
        assert_eq!(h.coop_vault(), 100, "second slash does not double-charge");
    }
}
