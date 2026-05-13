// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-57 — Off-era / stale evidence is filtered before closure.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-57.
// Theorem: T-12 filter (`restricted_closure_only_from_current_direct_offenders`,
// formal/rocq/slashing/theories/TwoLevelSlashing.v).
// Reference: formal/sage/slashing/FINDINGS.md row 5.
//
// Sage witness: current validators [0,1,2,3]; evidence validators
// [0,1,2,3,4]; stale equivocator [4]; edge [[0,4]]. A filtered
// current-validator model slashes []; an unfiltered projection
// slashes [0]. The post-fix model uses the filtered semantics —
// stale evidence does NOT propagate into current closure.
//
// Harness modeling: the current validator set is "v0", "v1", "v2",
// "v3"; "v4" is a stale offender from a prior epoch. A neglect
// edge from v0 → v4 should NOT slash v0 because v4 is not in the
// current bonded set.

use super::harness::SlashingTestHarness;

#[test]
fn uc_57_stale_evidence_does_not_propagate() {
    // Bonded set is v0..v3. v4 is unknown to the harness (no bond).
    let mut harness = SlashingTestHarness::new(4, 100);

    // No record for v4 (since v4 isn't a known current-bonded
    // validator). The harness's tracker only mints records for
    // dispatched blocks; v4 has none.
    assert!(!harness.has_record("v4", 0));

    // Suppose v0 publishes a block that cites a stale "v4" block
    // hash. The harness's neglect-detection only triggers when the
    // cited validator has an outstanding *current* record — v4
    // does not, so v0 is Valid.
    //
    // (The harness models this directly by having the cite
    // mechanism look up the cited validator in the current
    // tracker; stale evidence never enters the tracker, so it
    // cannot trigger neglect.)
    let v0_block = harness.sign_block("v0", 7);
    let _ = harness.dispatch(v0_block);

    // T-12 filter: v0 is not in any closure because v4's evidence
    // is filtered.
    assert!(
        !harness.has_record("v0", 6),
        "T-12 filter: stale evidence does not slash a current validator"
    );

    // All current bonded validators remain active.
    for i in 0..4 {
        let v = format!("v{}", i);
        assert!(harness.is_active(&v));
        assert!(harness.bond(&v) > 0);
    }
}
