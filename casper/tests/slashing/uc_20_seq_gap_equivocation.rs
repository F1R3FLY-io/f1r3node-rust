// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-20 — Equivocation detected across a sequence-number gap.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-20.
// Theorems: T-1 (detection sound), T-9.7 (seq-num density).
// Reference: formal/rocq/slashing/theories/BugFixSeqNumDensity.v.
//
// Scenario: v0 publishes a block at seq=0, then at seq=2 (skipping 1).
// A distinct block at seq=2 is then offered. Even with the missing seq=1
// block in the chain, the detector must classify the second seq=2 block
// as IgnorableEquivocation and the dispatcher must mint a record. Pre-fix
// detection assumed contiguous sequence numbers and could miss this case.

use super::harness::SlashingTestHarness;
use super::types::Status;

#[test]
fn uc_20_seq_gap_equivocation_is_detected() {
    let mut harness = SlashingTestHarness::new(2, 100);

    let _ = harness.sign_block("v0", 0);
    let _ = harness.sign_block("v0", 2);
    let bad = harness.sign_block_distinct("v0", 2);
    let status = harness.dispatch(bad);

    assert_eq!(status, Status::IgnorableEquivocation);
    assert!(harness.has_record("v0", 1));
}
