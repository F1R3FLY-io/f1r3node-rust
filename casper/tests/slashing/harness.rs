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

use super::observer::SlashingObserver;
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
        self.dag.blocks.insert(hash, BlockMeta {
            hash,
            sender: validator.to_string(),
            seq,
            justifications,
            slash_targets: vec![],
        });
        hash
    }

    /// Signs a block that cites `cited_block` in its justifications —
    /// useful for constructing the level-2 neglect scenario where the
    /// signer "sees" an equivocator's block but does not slash.
    pub fn sign_block_citing(
        &mut self,
        validator: &str,
        seq: SeqNum,
        cited_block: BlockHash,
    ) -> BlockHash {
        let hash = self.next_hash;
        self.next_hash += 1;
        let cited_sender = self
            .dag
            .blocks
            .get(&cited_block)
            .map(|b| b.sender.clone())
            .unwrap_or_default();
        let mut justifications = vec![];
        if !cited_sender.is_empty() {
            justifications.push((cited_sender, cited_block));
        }
        self.dag.blocks.insert(hash, BlockMeta {
            hash,
            sender: validator.to_string(),
            seq,
            justifications,
            slash_targets: vec![],
        });
        hash
    }

    /// Signs a block that both cites `cited_block` AND issues a
    /// SlashDeploy against the cited validator. Used to test the
    /// honest-neglecter path: when an honest signer slashes the
    /// equivocator they cite, no Level-2 fires.
    pub fn sign_block_citing_with_slash(
        &mut self,
        validator: &str,
        seq: SeqNum,
        cited_block: BlockHash,
        slash_target: &str,
    ) -> BlockHash {
        let hash = self.next_hash;
        self.next_hash += 1;
        let cited_sender = self
            .dag
            .blocks
            .get(&cited_block)
            .map(|b| b.sender.clone())
            .unwrap_or_default();
        let mut justifications = vec![];
        if !cited_sender.is_empty() {
            justifications.push((cited_sender, cited_block));
        }
        self.dag.blocks.insert(hash, BlockMeta {
            hash,
            sender: validator.to_string(),
            seq,
            justifications,
            slash_targets: vec![slash_target.to_string()],
        });
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
        let twin_exists = self.dag.blocks.values().any(|other| {
            other.hash != block.hash && other.sender == block.sender && other.seq == block.seq
        });
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
        let self_regress = block.justifications.iter().any(|(v, h)| {
            v == &block.sender
                && self
                    .dag
                    .blocks
                    .get(h)
                    .map(|b| b.seq >= block.seq)
                    .unwrap_or(false)
        });
        if self_regress {
            return Status::JustificationRegression;
        }
        // Level-2 neglect: the block cites a validator who has an
        // EquivocationRecord, but does not include a SlashDeploy for
        // that validator. Mirrors design §08 and Rocq theorem T-11.
        let neglected = block.justifications.iter().any(|(v, _h)| {
            // The cited validator has an outstanding record, AND
            // this block did not slash them.
            self.tracker.records.keys().any(|(rec_v, _)| rec_v == v)
                && !block.slash_targets.iter().any(|t| t == v)
        });
        if neglected {
            return Status::NeglectedEquivocation;
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

    pub fn bond(&self, validator: &str) -> i64 { self.pos_state.bond(validator) }

    pub fn coop_vault(&self) -> i64 { self.pos_state.coop_vault }

    pub fn is_active(&self, validator: &str) -> bool { self.pos_state.is_active(validator) }

    /// Mirrors `PoSContract.slash`: zeroes bonds, removes from active,
    /// adds to slashed, transfers bond into Coop vault. Idempotent
    /// (T-Idem): a second call is a no-op.
    pub fn execute_slash(&mut self, target: &str) -> SlashResult {
        if self.pos_state.slashed.contains(target) {
            return SlashResult {
                success: true,
                error: None,
            }; // T-Idem no-op
        }
        let bond = self.pos_state.bond(target);
        self.pos_state.bonds.insert(target.to_string(), 0);
        self.pos_state.active.remove(target);
        self.pos_state.slashed.insert(target.to_string());
        self.pos_state.coop_vault += bond;
        SlashResult {
            success: true,
            error: None,
        }
    }

    /// Mirrors `PoSContract.slash` post-fix #4 with a forced
    /// transfer-failure outcome. When `transfer_succeeded` is true,
    /// behaves identically to `execute_slash`; when false, returns
    /// `(false, "transfer failed")` and leaves the entire PoS state
    /// unchanged (validator stays in EquivocatorRecorded for retry).
    /// Mirrors the post-fix branch of PoS.rhox lines 461-490.
    pub fn execute_slash_with_transfer_outcome(
        &mut self,
        target: &str,
        transfer_succeeded: bool,
    ) -> SlashResult {
        if !transfer_succeeded {
            return SlashResult {
                success: false,
                error: Some("transfer failed".to_string()),
            };
        }
        self.execute_slash(target)
    }

    /// Mirrors the system auth-token guard at PoS.rhox:437-439:
    /// `sysAuthTokenOps!("check", sysAuthToken, *isValidTokenCh)`.
    /// When `valid_token` is false (spoofed token), the slash is
    /// rejected with `(false, "Invalid system auth token")` and no
    /// state changes. The harness threads this through
    /// `execute_slash` so tests for T-AuthCheck can exercise the
    /// validation path without a full Rholang interpreter.
    pub fn execute_slash_with_auth(&mut self, target: &str, valid_token: bool) -> SlashResult {
        if !valid_token {
            return SlashResult {
                success: false,
                error: Some("Invalid system auth token".to_string()),
            };
        }
        self.execute_slash(target)
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

    /// Bond the specified validator (test-setup helper, not part of the
    /// SlashingObserver contract).
    fn _separator() {}
}

/// Tier 3: harness implements `SlashingObserver` directly.
impl SlashingObserver for SlashingTestHarness {
    fn bond(&self, validator: &str) -> i64 { SlashingTestHarness::bond(self, validator) }
    fn coop_vault(&self) -> i64 { SlashingTestHarness::coop_vault(self) }
    fn is_active(&self, validator: &str) -> bool { SlashingTestHarness::is_active(self, validator) }
    fn has_record(&self, validator: &str, base_seq: SeqNum) -> bool {
        SlashingTestHarness::has_record(self, validator, base_seq)
    }
    fn record_witnesses(&self, validator: &str, base_seq: SeqNum) -> BTreeSet<BlockHash> {
        SlashingTestHarness::record_witnesses(self, validator, base_seq)
    }
    fn fork_choice(&self) -> Vec<ValidatorId> { SlashingTestHarness::fork_choice(self) }
}

impl SlashingTestHarness {
    /// Mirrors `PoS(@"bond", deployerId, amount, returnCh)` post-fix #5:
    /// a bond request with `amount <= 0` is rejected; otherwise the
    /// validator is added to the bonds map at `amount` and (if not
    /// already slashed) joins the active set.
    pub fn try_bond(&mut self, validator: &str, amount: i64) -> Result<(), String> {
        if amount <= 0 {
            return Err("Bond amount must be positive.".to_string());
        }
        if self.pos_state.bonds.contains_key(validator) {
            return Err("Public key is already bonded.".to_string());
        }
        if self.pos_state.slashed.contains(validator) {
            return Err("Validator is slashed; cannot re-bond.".to_string());
        }
        self.pos_state.bonds.insert(validator.to_string(), amount);
        self.pos_state.active.insert(validator.to_string());
        Ok(())
    }

    /// Mirrors `BlockCreator::prepare_slashing_deploys` post-fix:
    /// returns the list of validators the proposer would target with
    /// a SlashDeploy. Empty when the proposer's own bond is zero or
    /// absent (bug fix #8: an unbonded proposer cannot effect a slash,
    /// so the call short-circuits to avoid wasted work). When the
    /// proposer is bonded, returns every validator with an outstanding
    /// EquivocationRecord.
    pub fn simulate_slash_proposal(&self, proposer: &str) -> Vec<ValidatorId> {
        // Bug #8 post-fix: skip emission entirely for unbonded proposers.
        if self.pos_state.bond(proposer) <= 0 {
            return Vec::new();
        }
        let mut targets: Vec<ValidatorId> = self
            .tracker
            .records
            .keys()
            .map(|(v, _)| v.clone())
            .collect();
        targets.sort();
        targets.dedup();
        targets
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
