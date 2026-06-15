// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-64 — Epoch evidence rollover filtering.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-64.
// Theorem: T-12 epoch filter (`epoch_filter_in`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: formal/sage/slashing/FINDINGS.md row 13 — "epoch /
// current-validator filtering separates stale and fresh evidence".
//
// Property: stale offender evidence outside the current epoch
// produces empty closure; fresh current-epoch evidence propagates
// through the current validator set. The harness models epochs as
// "is the cited validator currently in the active/bonded set?";
// validators not in the bonded set produce no neglect.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_64_stale_evidence_does_not_propagate() {
    // Current epoch: v0..v3 are bonded. v4 (stale) is not.
    let mut harness = SlashingTestHarness::new(4, 100);

    // No record exists for v4 (stale offender — not in current
    // bonded set, so no equivocation lands a record).
    assert!(!harness.has_record("v4", 0));

    // v0 publishes a block citing a stale "v4" hash. No record
    // for v4 means no neglect — v0's block is Valid.
    let v0_block = harness.sign_block("v0", 5);
    let _ = harness.dispatch(v0_block);
    assert!(
        !harness.has_record("v0", 4),
        "T-12 epoch filter: stale evidence does not propagate"
    );
}

#[test]
fn uc_64_fresh_evidence_propagates() {
    let mut harness = SlashingTestHarness::new(4, 100);

    // Fresh current-epoch evidence: v0 equivocates.
    let _v0a = harness.sign_block("v0", 5);
    let bad = harness.sign_block_distinct("v0", 5);
    let _ = harness.dispatch(bad);

    // v1 cites v0's bad block without slashing → fresh evidence
    // propagates to v1's record.
    let v1_neg = harness.sign_block_citing("v1", 6, bad);
    let s = harness.dispatch(v1_neg);
    assert_eq!(s, Status::NeglectedEquivocation);
    assert!(harness.has_record("v1", 5));
}
