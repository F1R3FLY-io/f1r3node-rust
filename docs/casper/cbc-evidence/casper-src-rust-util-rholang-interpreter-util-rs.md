# CbC Evidence: casper/src/rust/util/rholang/interpreter_util.rs

- **Status:** waived
- **Adapter:** waiver
- **Commit:** eac5665a0
- **Verified:** 2026-08-22T03:12:00Z

Claim:

> Consensus-visible rejection counts use complete authenticated history or return `BlockNotHeld`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "PR299-REJECTION-COUNTS",
  "artifact": {
    "path": "casper/src/rust/util/rholang/interpreter_util.rs",
    "commit": "eac5665a0",
    "id": "casper-src-rust-util-rholang-interpreter-util-rs"
  },
  "claim": "Consensus-visible rejection counts use complete authenticated history or return BlockNotHeld.",
  "adapter": "waiver",
  "status": "waived",
  "evidence": {
    "kind": "waiver",
    "ref": null,
    "counterexample": null,
    "detail": "explicit waiver"
  },
  "waiver": {
    "reason": "PR #299 records phase-1 implementation evidence. PR #311 will discharge the formal Rust-conformance claims.",
    "by": "maintainer",
    "at": "2026-08-22T03:12:00Z"
  },
  "verified_at": "2026-08-22T03:12:00Z"
}
```

---

## 2026-08-27 — CLAIM-FINALITY-001 (settled-effect probe batching)

- **Status:** discharged
- **Adapter:** agentic
- **Commit:** c9aff7732 (landed; review-remediation refinements follow on the same branch)
- **Verified:** 2026-08-27T23:01:30Z

Claim:

> CLAIM-FINALITY-001 (docs/claims/settled-effect-probe-equivalence.md): the merge's `sig_settled_in_base` and `sig_settled_in_floor` probes answer by membership in lazily built applied-sig sets whose construction is the reference walk batched (C2), so every probe verdict is unchanged.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-FINALITY-001",
  "artifact": {
    "path": "casper/src/rust/util/rholang/interpreter_util.rs",
    "commit": "c9aff7732",
    "id": "casper-src-rust-util-rholang-interpreter-util-rs"
  },
  "claim": "CLAIM-FINALITY-001: the settled-sig probe closures answer by membership in sets built by settled_sigs_of_lineage (one walk on first probe; floor probe = FloorSettledProbe with the reference loop's in-order per-floor short-circuit), preserving every reference verdict.",
  "adapter": "agentic",
  "status": "discharged",
  "evidence": {
    "kind": "test+mechanization",
    "ref": "casper::rust::finality::deploy_lifecycle::tests::batched_walk_matches_the_reference_walk_on_generated_lineages; formal/rocq/finalized_floor/theories/SettledEffectProbe.v (walk_collect_equiv for the base probe; walk_segment_composition for the per-floor form; floor_probe_matches_the_reference_floor_loop and floor_probe_short_circuit_skips_unavailable_later_floors pin the probe)",
    "counterexample": null,
    "detail": "Closure bodies changed from per-sig effect_in_state_of walks to set membership; bounds are byte-identical to the previous closures (floor_block_number - deploy_lifespan for the base; per-floor saturating bound for the floors). Laziness preserves no-probe-no-walk behavior. The settled-probe wrapper counters keep running and now time membership lookups - the before/after telemetry for issue #24. End-to-end acceptance per the claim's discharge plan item 4 (InvalidRejectedDeploy equality) rides the existing casper merge/validation suites plus the next soak preflight."
  },
  "verified_at": "2026-08-27T23:01:30Z"
}
```

---

## 2026-11-25 — CLAIM-FINALITY-001 (base probe follows the merge base)

- **Status:** discharged
- **Adapter:** agentic
- **Commit:** be4b3bec2
- **Verified:** 2026-11-25T01:23:39Z

Claim:

> CLAIM-FINALITY-001 (docs/claims/settled-effect-probe-equivalence.md): `sig_settled_in_base` and the name-seed scan walk the state lineage of the merge base, the block whose state the merge builds on, so a merge that falls back to the floor drops no work of the main parent's lineage without a rejection record.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-FINALITY-001",
  "artifact": {
    "path": "casper/src/rust/util/rholang/interpreter_util.rs",
    "commit": "be4b3bec2",
    "id": "casper-src-rust-util-rholang-interpreter-util-rs"
  },
  "claim": "CLAIM-FINALITY-001: sig_settled_in_base and merge_name_seeds take the scope anchor (the merge base), not the main parent, so the settled set describes the state the merge applies scope chains to.",
  "adapter": "agentic",
  "status": "discharged",
  "evidence": {
    "kind": "test",
    "ref": "casper merging::floor_fallback_spec::a_merge_re_based_on_the_floor_accounts_for_the_main_parents_work",
    "counterexample": null,
    "detail": "The test drives compute_parents_post_state with real blocks onto the floor fallback path. With the main parent as the probe base, deploys of the main parent's lineage above the floor reach neither the merged state nor a rejection record, and the test fails. With the merge base, each either reaches the state or is recorded, and the test passes. The reference walk and the batched set (C2) are unchanged; only the probe argument moved."
  },
  "verified_at": "2026-11-25T01:23:39Z"
}
```

---

## 2026-11-25 — CLAIM-FINALITY-001 (the name-seed scan follows the merge base)

- **Status:** discharged
- **Adapter:** agentic
- **Commit:** 1f1a21eac
- **Verified:** 2026-11-25T16:00:19Z

Claim:

> CLAIM-FINALITY-001 (docs/claims/settled-effect-probe-equivalence.md): `merge_name_seeds` walks the merge base, so when the base falls back to a floor outside the main parent's ancestry, a seed the floor already holds keeps its holder and no second deploy with that seed is applied.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-FINALITY-001",
  "artifact": {
    "path": "casper/src/rust/util/rholang/interpreter_util.rs",
    "commit": "1f1a21eac",
    "id": "casper-src-rust-util-rholang-interpreter-util-rs"
  },
  "claim": "CLAIM-FINALITY-001, name-seed half: merge_name_seeds takes the scope anchor, so the seeds held by the merge base include those of a floor the main parent's ancestry does not reach.",
  "adapter": "agentic",
  "status": "discharged",
  "evidence": {
    "kind": "test",
    "ref": "casper merging::floor_fallback_spec::a_merge_re_based_on_the_floor_keeps_the_floors_holder_of_a_name_seed",
    "counterexample": null,
    "detail": "X and Y share a name seed; X lands in the floor F and Y in a sibling S of F. Merging S as the main parent with a child of F re-bases on F. With the main parent as the scan base, the scan misses F's X, the merge applies Y on top of it, and the test fails; with the merge base, the merge keeps X as the seed's holder and drops Y, and the test passes. The settled-sig half is covered by a_merge_re_based_on_the_floor_accounts_for_the_main_parents_work."
  },
  "verified_at": "2026-11-25T16:00:19Z"
}
```
