// UC-28 through UC-36 — Tier B: every non-equivocation slashable
// `InvalidBlock` variant exercises the post-fix dispatcher's
// catch-all and produces an EquivocationRecord.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12.
// Theorem: T-9.3 (catch-all dispatcher records every slashable
// variant), formal/rocq/slashing/theories/BugFixDispatcher.v.
//
// The harness's projected `Status::SlashableOther` represents any
// of these 14 variants:
//   InvalidBlockNumber, InvalidParents, InvalidFollows,
//   InvalidSequenceNumber, InvalidShardId, InvalidRepeatDeploy,
//   DeployNotSigned, InvalidTransaction, InvalidBondsCache,
//   InvalidBlockHash, ContainsExpiredDeploy,
//   ContainsTimeExpiredDeploy, ContainsFutureDeploy
// Each test below corresponds to one UC entry from spec §12 with
// the same dispatcher-routing assertion. The production
// per-variant validator distinctions are covered by the existing
// classifier tests at casper/tests/batch2/validate_test.rs.

use super::harness::SlashingTestHarness;
use super::types::Status;

fn assert_dispatch_records(uc_label: &str, validator: &str, seq: u64) {
    let mut harness = SlashingTestHarness::new(3, 100);
    let hash = harness.sign_block(validator, seq);
    let status = harness.dispatch_with_status(hash, Status::SlashableOther);
    assert_eq!(status, Status::SlashableOther, "{}", uc_label);
    assert!(
        harness.has_record(validator, seq.saturating_sub(1)),
        "{}: post-fix #3 records the offender",
        uc_label
    );
    assert!(harness.dag.invalid.contains(&hash), "{}: block is invalid", uc_label);
}

#[test] fn uc_28_invalid_block_number()         { assert_dispatch_records("UC-28 InvalidBlockNumber",         "v0", 5); }
#[test] fn uc_29_invalid_parents()              { assert_dispatch_records("UC-29 InvalidParents",             "v0", 6); }
#[test] fn uc_30_invalid_follows()              { assert_dispatch_records("UC-30 InvalidFollows",             "v0", 7); }
#[test] fn uc_31_invalid_sequence_number()      { assert_dispatch_records("UC-31 InvalidSequenceNumber",      "v0", 8); }
#[test] fn uc_32_invalid_shard_id()             { assert_dispatch_records("UC-32 InvalidShardId",             "v0", 9); }
#[test] fn uc_33_invalid_repeat_deploy()        { assert_dispatch_records("UC-33 InvalidRepeatDeploy",        "v0", 10); }
#[test] fn uc_34_invalid_transaction()          { assert_dispatch_records("UC-34 InvalidTransaction",         "v0", 11); }
#[test] fn uc_35_invalid_bonds_cache()          { assert_dispatch_records("UC-35 InvalidBondsCache",          "v0", 12); }
#[test] fn uc_36_contains_future_deploy()       { assert_dispatch_records("UC-36 ContainsFutureDeploy",       "v0", 13); }
