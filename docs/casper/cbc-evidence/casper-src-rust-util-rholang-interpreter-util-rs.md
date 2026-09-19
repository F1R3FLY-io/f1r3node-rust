# CbC Evidence: casper/src/rust/util/rholang/interpreter_util.rs

- **Status:** waived
- **Adapter:** waiver
- **Commit:** eac5665a0
- **Verified:** 2026-08-22T03:12:00Z

Claim:

> Consensus-visible rejection counts use complete authenticated history or return `BlockNotHeld`.

```json
{
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
