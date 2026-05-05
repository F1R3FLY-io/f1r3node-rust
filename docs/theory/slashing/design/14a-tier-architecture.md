# 14a · Tier Architecture for Slashing Tests

This document is the principled long-term architecture for
verifying the slashing subsystem at runtime. It complements the
test catalogue in [§14](./14-test-plan.md) with the *structural*
question: how do the harness, the oracle, and the production code
relate, and how do tests pin that relationship?

## 1. Motivation

The original §14 prescribed a two-layer architecture:

* **Harness** — a state machine that projects the LTS from spec
  §3-§8 into in-memory Rust types.
* **Oracle** — pure functions in `oracle.rs` that mirror the
  Rocq definitions of `EquivocationDetector.detect`,
  `PoSContract.slash`, and `TwoLevelSlashing.neglect`.

Property tests compare harness post-states against oracle
post-states (§14.5), and §14 implicitly assumes the harness is
a faithful projection of the production code: every `T-N`
theorem proven against the harness is taken as evidence that the
production code satisfies it too.

That assumption is **load-bearing but unverified**. If the
production `MultiParentCasperImpl::handle_invalid_block` regresses
to a lock-free RMW tomorrow, the harness's projected `dispatch`
operation would not notice — the harness has its own,
parallel implementation of the dispatcher. UC-39 verifies the
harness matches the *Rocq* oracle; nothing verifies the harness
matches the *production*. This is the **harness-faithfulness
gap**.

The principled closure is a **third tier** — the production
adapter — and **triple-bisimilarity property tests** that
exercise all three tiers in lock-step.

## 2. The Three Tiers

```
┌─────────────────────────────────────────────────────────────┐
│  Tier 1: Production                                         │
│  • SlashingProductionAdapter                                │
│  • Wraps TestNode + BlockDagKeyValueStorage + Rholang       │
│  • Real LMDB, real RSpace, real signing                     │
│  • Source of truth                                          │
│  • Slow — ~seconds per operation                            │
└─────────────────────────────────────────────────────────────┘
                          ▲
                          │ refinement (every spec property)
                          │
┌─────────────────────────────────────────────────────────────┐
│  Tier 2: Oracle                                             │
│  • RocqOracleAdapter                                        │
│  • Wraps oracle.rs (hand-translated from Rocq)              │
│  • Pure functions over (DagState, EqRecordSet, PoSState)    │
│  • Mathematical projection of formal mechanization          │
│  • Fast — ~µs per operation                                 │
└─────────────────────────────────────────────────────────────┘
                          ▲
                          │ refinement (every spec property)
                          │
┌─────────────────────────────────────────────────────────────┐
│  Tier 3: Harness                                            │
│  • SlashingTestHarness                                      │
│  • In-memory state machine projecting the LTS               │
│  • Refinement of (2), adapter API for (1)                   │
│  • Fastest — ~ns per operation; supports 10,000-case proptests │
│  • Failure shrinking works (the canonical proptest property) │
└─────────────────────────────────────────────────────────────┘
```

All three implement the same `SlashingObserver` trait:

```rust
pub trait SlashingObserver {
    fn bond(&self, validator: &str) -> i64;
    fn coop_vault(&self) -> i64;
    fn is_active(&self, validator: &str) -> bool;
    fn has_record(&self, validator: &str, base_seq: SeqNum) -> bool;
    fn record_witnesses(&self, validator: &str, base_seq: SeqNum) -> BTreeSet<BlockHash>;
    fn fork_choice(&self) -> Vec<ValidatorId>;
}
```

The trait surface is *read-only* because mutating operations are
tier-specific (the production tier takes a real `BlockMessage`;
the harness/oracle tiers take a synthetic `BlockHash`). Cross-
tier driver code in the triple-bisim tests handles the conversion.

## 3. The Triple-Bisimilarity Test Pattern

```rust
proptest! {
    #[test]
    fn t_15_triple_bisim_dispatch(events in gen_event_sequence()) {
        let mut harness     = SlashingTestHarness::new(...);
        let mut oracle      = RocqOracleAdapter::new(...);
        let mut production  = SlashingProductionAdapter::snapshot(...);

        for event in events {
            // Apply the event to all three tiers.
            apply_to_tier3(&mut harness, &event);
            apply_to_tier2(&mut oracle, &event);
            apply_to_tier1(&mut production, &event);

            // After every step, every observable must agree
            // across all three tiers.
            for v in &validators {
                prop_assert_eq!(harness.bond(v), oracle.bond(v));
                prop_assert_eq!(oracle.bond(v),  production.bond(v));
                prop_assert_eq!(harness.has_record(v, 0), oracle.has_record(v, 0));
                // ...etc for every SlashingObserver method
            }
        }
    }
}
```

When the test fails, the disagreement narrows the fault to
exactly one tier:

| Tiers in agreement | Outlier | Diagnosis |
|--------------------|---------|-----------|
| Harness ≠ Oracle = Production | Harness | Harness drift away from spec |
| Harness = Oracle ≠ Production | Production | Production regression away from spec |
| Harness = Production ≠ Oracle | Oracle | oracle.rs translation is stale relative to Rocq |
| All three disagree | Multiple | Investigate each pair |

This is the principled closure of the harness-faithfulness gap:
it does not require the harness to *be* the production, only to
*agree with* the production on observable state under arbitrary
event sequences.

## 4. Why Not Coq Extraction?

The academically pure way to eliminate oracle drift would be Coq
Extraction: replace `oracle.rs` (hand-translated) with code
auto-extracted from the Rocq theories. The triple-bisim tests
would then verify production agreement with the *machine-
extracted* oracle — a stronger guarantee than agreement with a
hand-translation.

This was rejected for three reasons:

1. **OCaml runtime in the test build.** Coq's standard extraction
   target is OCaml; the resulting code is famously unidiomatic
   (booleans become a Coq inductive with two arms, polymorphic
   functions lose their generality). Linking the OCaml output
   into the Rust test build doubles CI surface and adds an FFI
   layer.

2. **Project ethos.** The repo's CLAUDE.md states *"No Nix, no
   SBT, no Scala — this repo builds with standard Rust tooling
   (cargo + system deps)."* Coq Extraction belongs to the same
   "auxiliary toolchain" category as the rejected build systems.

3. **Drift mitigation works without it.** When `formal/rocq/
   slashing/theories/*.v` changes, `oracle.rs` must be updated
   in the same PR; this is enforced by code review (see
   `CODEOWNERS` for the slashing area) and detected by CI
   (the Rocq build job rebuilds the theories on every commit;
   a mismatch between Rocq and oracle.rs surfaces as a
   triple-bisim failure on the next nightly run).

## 5. Why Not Harness-As-Production?

A tempting alternative architecture: re-implement the harness as
a wrapper around `BlockDagKeyValueStorage` and `MultiParentCasperImpl`,
projecting their state into harness-style accessors. This
"harness-as-projection" approach would eliminate the harness-
faithfulness gap *by construction*: every harness operation IS
a production operation, observed through the projection.

This was rejected because the harness's value comes precisely
from *not* being the production:

* The harness backs proptests at 10,000 cases per execution;
  production-tier tests cap at ~25 cases per execution.
* Proptest shrinking only works when test cases are cheap. Each
  shrinking step on a production-tier test takes seconds; on a
  harness-tier test, microseconds.
* The harness's `dispatch_with_status` lets tests inject a
  classifier verdict directly; production-tier tests must
  synthesize a block whose validator pipeline lands in the
  desired classification — much more work, and tightly coupled
  to validation rule ordering.

The right closure is *cross-execution agreement* (this
document's triple-bisim), not *structural identity*.

## 6. CI Architecture

Per [§14.9](./14-test-plan.md#149-ci-integration), the slashing
test suite runs as eight jobs in
`.github/workflows/slashing-tests.yml`:

| Job                        | Tier  | Trigger    | Time    |
|----------------------------|-------|------------|---------|
| example-based              | 3     | PR-gate    | <30 min |
| property-based             | 2 + 3 | PR-gate    | <30 min |
| pre-fix-regressions (×9)   | 3     | PR-gate    | <15 min |
| loom-interleavings         | 3     | PR-gate    | <30 min |
| tla-model-check            | (TLA+) | PR-gate   | <30 min |
| rocq-build                 | (Rocq) | PR-gate   | <60 min |
| **production-integration** | 1 + 2 + 3 | PR-gate (Track 2) | <30 min |
| **mutation-coverage**      | 3     | Nightly    | <240 min |
| **nightly-extended-proptest** | 2 + 3 | Nightly | <240 min |

A PR cannot merge unless all PR-gate jobs pass. Nightly jobs
surface coverage gaps and high-confidence proptest results that
would consume too much time on every PR.

## 7. Failure-Mode Catalogue

| Symptom | Likely tier outlier | Diagnostic next step |
|---------|---------------------|----------------------|
| `prop_t_15_triple_bisim_dispatch` fails on bond observable | Production | Compare `compute_bonds` output between two consecutive commits |
| `prop_t_15_triple_bisim_dispatch` fails on has_record | Harness | Check `dispatch` arm coverage; harness may have skipped a slashable variant |
| `prop_t_13a_bonds_bisim` fails (no production tier) | Oracle | Check `oracle.rs` against `formal/rocq/slashing/theories/PoSContract.v` |
| Surviving mutant in `equivocation_detector.rs` | Coverage gap | Add a UC test or proptest exercising the mutated code path |
| `loom-interleavings` job times out | Loom budget | Decrease `LOOM_MAX_PREEMPTIONS` or remove a thread from the schedule |

## 8. Constraint Compliance

The principled architecture is verified against the constraints:

* **No feature flags.** ✓ `SlashingObserver` dispatch is type-
  level; no `cfg` flags or build-time variant selection.
* **No `cfg` shims for variant gating.** ✓ The loom test's
  shadow uses `loom::sync::*` types directly; no `cfg(loom)` flag
  is needed in either production or test code.
* **No destructive changes without approval.** ✓ Track 5's trait
  extraction in `block-storage` was flagged for approval before
  implementation; the sequential-shadow fallback is non-
  destructive.
* **No fabricated tests.** ✓ Every test traces to a §14 line
  or a Rocq theorem in `formal/rocq/slashing/theories/`.
* **Every assertion traceable.** ✓ Each test file's lead
  comment cites the §14 anchor and Rocq theorem name.

## 9. Companion Documents

* [§14 Test Plan](./14-test-plan.md) — the test catalogue and
  the §14.2.4 tier-model summary.
* [§09 Bug Fixes and Rationale](./09-bug-fixes-and-rationale.md)
  — the 9 production bug fixes the tests are pinning.
* [§10 Bisimilarity](./10-bisimilarity.md) — the formal
  bisimulation theorem the triple-bisim tests are runtime-
  checking.
* [`formal/rocq/slashing/theories/Bisimulation.v`](../../../formal/rocq/slashing/theories/Bisimulation.v)
  — the Rocq mechanization.

---

**Authoritative status.** The architecture in this document is
normative for the slashing test suite as of commit fa29d33+ (post-
Track-9 documentation update). Any deviation from the tier model
or the triple-bisim closure pattern requires updating this
document in the same PR.
