# CbC Evidence: casper/src/rust/finality/deploy_lifecycle.rs

- **Status:** discharged
- **Adapter:** agentic
- **Commit:** c9aff7732 (landed; review-remediation refinements follow on the same branch)
- **Verified:** 2026-08-27T23:01:30Z

Claim:

> CLAIM-FINALITY-001 (docs/claims/settled-effect-probe-equivalence.md): the batched settled-effect walk (`settled_sigs_of_lineage`) is extensionally equal to the per-sig reference walk (`effect_in_state_of`) on complete lineage segments, and refuses (typed `BlockNotHeld`) rather than answers on incomplete ones.

```json
{
  "artifact": {
    "path": "casper/src/rust/finality/deploy_lifecycle.rs",
    "commit": "c9aff7732",
    "id": "casper-src-rust-finality-deploy-lifecycle-rs"
  },
  "claim": "CLAIM-FINALITY-001: settled_sigs_of_lineage is extensionally equal to the effect_in_state_of reference walk on complete segments; incomplete segments refuse with typed BlockNotHeld (fail-closed availability strengthening).",
  "adapter": "agentic",
  "status": "discharged",
  "evidence": {
    "kind": "test+mechanization",
    "ref": "casper::rust::finality::deploy_lifecycle::tests::{batched_walk_matches_the_reference_walk_on_generated_lineages, batched_walk_is_fail_closed_on_a_gapped_segment, floor_probe_matches_the_reference_floor_loop, floor_probe_short_circuit_skips_unavailable_later_floors}; formal/rocq/finalized_floor/theories/SettledEffectProbe.v (walk_collect_equiv, walk_segment_composition, walk_memo_false_stable, walk_true_stable; gate scripts/check-finalized-floor-ALL.sh, 18/18 axiom-free)",
    "counterexample": null,
    "detail": "The reference walk is kept untouched as the specification oracle (discharge plan item 1). The generative test compares batched membership against the reference on 12 seeded lineages covering fresh sigs (failed and non-failed), applied_from_scope sigs, decoy sigs on non-base parents, absent sigs, and low/mid/above-tip bounds, with a second pass through the lineage-step cache (item 2). The per-block lineage-step cache stores content-addressed per-block facts only, never answers, and every hit is revalidated against the caller's store with a raw key-existence check, so the walk stays a function of the supplied store; the documented within-segment fail-closed divergence is pinned by its own test, and FloorSettledProbe preserves the reference loop's per-floor short-circuit (its two tests pin equivalence and gap behavior)."
  },
  "verified_at": "2026-08-27T23:01:30Z"
}
```
