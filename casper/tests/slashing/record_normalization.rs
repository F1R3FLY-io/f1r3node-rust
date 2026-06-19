// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// UC-65 — Equivocation-record normalization.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-65.
// Theorem: T-5 record equivalence (`hashes_equiv_*`,
// formal/rocq/slashing/theories/EquivocationRecord.v).
// Reference: formal/sage/slashing/FINDINGS.md row 16 — "record
// normalization is order- and duplicate-insensitive. All
// permutations of duplicate records normalize to the same
// key-to-hash-set meaning, and duplicate hashes are idempotent."
//
// Property: the equivocation tracker stores witness hashes as a
// BTreeSet — set semantics make insertion order irrelevant and
// duplicates idempotent.

use super::harness::SlashingTestHarness;

#[test]
fn uc_65_witness_set_is_order_insensitive() {
    let mut harness_a = SlashingTestHarness::new(2, 100);
    let mut harness_b = SlashingTestHarness::new(2, 100);

    // Insert witnesses in different orders.
    harness_a.record_equivocation("v0", 0, 100);
    harness_a.record_equivocation("v0", 0, 200);
    harness_a.record_equivocation("v0", 0, 300);

    harness_b.record_equivocation("v0", 0, 300);
    harness_b.record_equivocation("v0", 0, 100);
    harness_b.record_equivocation("v0", 0, 200);

    // Order-insensitive normalization: both harnesses produce the
    // same witness set.
    assert_eq!(
        harness_a.record_witnesses("v0", 0),
        harness_b.record_witnesses("v0", 0),
        "T-5: witness sets are order-insensitive"
    );
    assert_eq!(harness_a.record_witnesses("v0", 0).len(), 3);
}

#[test]
fn uc_65_duplicate_hashes_are_idempotent() {
    let mut harness = SlashingTestHarness::new(2, 100);

    // Insert the same witness hash multiple times.
    harness.record_equivocation("v0", 0, 100);
    harness.record_equivocation("v0", 0, 100);
    harness.record_equivocation("v0", 0, 100);

    let witnesses = harness.record_witnesses("v0", 0);
    assert_eq!(witnesses.len(), 1, "T-5: duplicate hashes are idempotent");
    assert!(witnesses.contains(&100));
}
