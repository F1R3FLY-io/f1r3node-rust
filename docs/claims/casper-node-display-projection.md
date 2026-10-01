# Casper Node Display Projection

This claim registers the pending Batch E obligations. It contains no acceptance or discharge evidence.

```yaml
claim_id: CLAIM-CASPER-NODE-OBSERVATION-005
status: pending
adapter: null
scope: batch-e-detached-display-projection
implementation_baseline: 3ab092cc58fb30f4da39e6c8b28b8d25206c661b
registered_by: pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa
registered_at: 2026-09-30T17:42:17.769798+00:00
artifacts:
  - block-storage/src/rust/dag/soak_equivocations.rs
  - block-storage/src/rust/dag/mod.rs
  - block-storage/src/rust/dag/soak_snapshot.rs
  - block-storage/src/rust/dag/block_dag_key_value_storage.rs
  - block-storage/tests/soak_snapshot.rs
  - casper/src/rust/safety/initial_fault.rs
  - casper/src/rust/safety/mod.rs
  - casper/src/rust/engine/multi_parent_casper/dispatch.rs
  - casper/src/rust/api/block_api.rs
  - casper/src/rust/soak_observer/display.rs
  - casper/src/rust/soak_observer.rs
  - casper/src/rust/soak_observer/evaluation.rs
  - casper/tests/soak_observer.rs
  - node/tests/soak_observer.rs
  - formal/tlaplus/node_observation/DisplayProjection.tla
  - formal/tlaplus/node_observation/MC_DisplayProjection.tla
  - formal/tlaplus/node_observation/MC_DisplayProjection.cfg
  - formal/tlaplus/node_observation/MC_DisplayProjection_fabricated_unsafe.tla
  - formal/tlaplus/node_observation/MC_DisplayProjection_fabricated_unsafe.cfg
  - formal/tlaplus/node_observation/MC_DisplayProjection_source_unsafe.tla
  - formal/tlaplus/node_observation/MC_DisplayProjection_source_unsafe.cfg
  - formal/tlaplus/node_observation/MC_DisplayProjection_interval_unsafe.tla
  - formal/tlaplus/node_observation/MC_DisplayProjection_interval_unsafe.cfg
  - formal/tlaplus/node_observation/verification-plan.json
  - formal/tlaplus/node_observation/bindings.json
  - formal/tlaplus/node_observation/README.md
  - formal/rocq/node_authority/theories/DisplayProjection.v
  - formal/rocq/node_authority/theories/MainTheorem.v
  - formal/rocq/node_authority/_CoqProject
  - formal/rocq/node_authority/README.md
  - scripts/ci/check-tla-invariants.sh
  - scripts/ci/test-check-tla-invariants.sh
  - scripts/ci/check-formal-invariants.sh
refutation: pending
construction: pending
binding: pending
soak: pending
```

The artifact list follows the approved 72-file scope. An additional implementation path requires a scope amendment.

## Authorization and scope

The user approved the scope and implementation on 2026-09-30.
Agent A transferred the final Batch D files after step 13.
Agent B verified all seven handoff digests against commit `3ab092cc58fb30f4da39e6c8b28b8d25206c661b`.
This registration precedes the first Batch E code change.

The [approved plan](../plans/casper-node-observation-batch-e.md) defines the implementation scope.
The source base is the verified step-13 handoff revision.

This claim covers the bounded capture of the equivocation inputs and the detached display calculation.

Batch E changes artifacts of claims 001 through 004.
Historical acceptance remains specific to the recorded revisions.
Claim 004 remains pending.

Agent A owns the 28 `batch-d-registration` records and the step-15 binding-driver refresh.
Batch E waits for that committed refresh before it creates shared-file successor records with `previous_record`.
Batch E does not change the Batch B2 digest test or the fork-choice digest contract.

## Required properties

1. A request with no display option gives the current response. The display projection stays unavailable, with the reason `equivocation_snapshot_unavailable`.
2. A display request requires an explicit positive tracker-row limit of at most 4,096.
   The observer rejects invalid limits and explicit null before store access.
3. The capture reads the tracker rows in the guards and in the DAG read transaction of the detached capture. It makes one capture for each request.
4. The capture checks each raw row length before the copy. It checks each count field before the typed decode.
5. A tracker commit in the capture interval changes the transaction identity and rejects the capture. An equal insertion generation does not replace that check.
6. The capture reads the tracker store and does not write it. The production store bytes are equal before and after each capture.
7. The canonical encoding of the equivocation inputs sorts the rows and covers each retained field. The digest is SHA-256 of that encoding.
8. The authority digest of a display request binds the snapshot digest and the equivocation digest. The display projection uses the target input digest.
9. The display calculation reads captured data only. It does not call the Casper instance, the live tracker, or the public block API.
10. The base value is the persisted value for a finalized target. The base value is the original oracle result for other targets.
11. The live and detached paths share initial-fault and final-subtraction helpers.
    Equal accepted inputs give equal display bits.
    The calculation has one term for each tracker record.
12. The observer refuses a weight sum that is larger than the `u64` range. It does not report a wrapped value.
13. Each refusal gives an unavailable or failed result with a reason. A refusal has no fabricated value and no zero value.
14. The display inputs, the display bits, the original bits, and the persisted bits have separate fields and separate availability states.
15. The snapshot wire schema does not change. The schema version stays 2, and the snapshot has no new tag.
16. The helper extraction preserves public block API values and ordinary unchecked sum behavior.
17. The display calculation charges its work before each bounded operation. The response contains counts and digests, and no tracker rows.

Properties 15 and 16 apply to the approved guarded capture and shared arithmetic.

## What this claim does not cover

- The numeric mapping of `f32` bits to rational values. TASK-017-12 owns that mapping.
- The paired fork-choice observation. Batch D owns that work.
- Equality between a detached value and a public API response. The live value is not atomic.
- The correctness of the live initial-fault rule, which includes the rule of one term for each record.
- The correctness of the equivocation detector or of the tracker write path.
- A wire proof for the canonical encoding of the equivocation inputs.
- The floating-point behavior of a target that the tests do not run on.
- The qualification of a live authority profile.
- Tracker rows of a storage backend that is not LMDB.
- A DAG that is larger than the explicit capture limits.

## Trust and evidence boundaries

The pinned LMDB library and its transaction identity counter stay trusted inputs.

The `bincode` layout of a tracker row is a trusted input of the row parser. The tests must show that layout with rows that production code writes.

The `f32` division and subtraction follow the IEEE 754 rules of the Rust target. No model in this claim shows that arithmetic.

SHA-256 collision resistance is an assumption of the digest binding.

## Refutation evidence obligations

The registered bounded model is `DisplayProjection`, with three negative controls.
The formal gate requires a clean positive run and exact invariant violations from all three controls.

| Bounded invariant | Negative control | Property |
|----------------------|-----------------------------|----------|
| `NoFabrication`: an available value requires a captured tracker and an available base value | `MC_DisplayProjection_fabricated_unsafe` | 1, 13 |
| `BaseSource`: a finalized target uses the persisted value | `MC_DisplayProjection_source_unsafe` | 10 |
| `OneInterval`: the tracker rows and the snapshot have one transaction identity | `MC_DisplayProjection_interval_unsafe` | 3, 5 |

Properties 3, 5, and 6 also extend the accepted `BoundedCapture` model. The tracker store is in the DAG environment of that model.

The model uses Boolean values for the arithmetic result. It does not show `f32` bits.

The user approved the new bounded model with three controls.

## Construction evidence obligations

The registered Rocq theory is `DisplayProjection.v` in the `NodeAuthority` project.
The theory exports seven results.
The formal gate checks all twenty-two NodeAuthority assumption sets.

| Theorem | Statement | Property |
|--------------------|-----------|----------|
| `display_weight_sum_checked` | A checked sum gives a result only when the exact sum is in the range. | 12 |
| `display_matched_weight_bounded` | With no repeated validator in the records, the matched weight is not larger than the total weight. | 11 |
| `display_record_multiplicity` | The matched weight has one term for each record, for each record order. | 11 |
| `display_duplicate_records_add_duplicate_terms` | Repeated records add repeated weight terms. | 11 |
| `display_overflow_refuses` | An excessive exact sum has no checked result. | 12 |
| `display_refusal_has_no_value` | A refused calculation gives no value. | 13 |
| `display_requires_equal_digest` | A display value and its inputs have one input digest. | 8, 14 |

Properties 3 and 5 inherit the accepted capture theorems `capture_no_interference` and `capture_generation_stable`.

Properties 7 and 15 keep pending construction. The approved scope includes no tracker wire proof in this batch.

Property 11 keeps pending construction for the `f32` operations. The theorems cover the integer inputs only.

Properties 1, 14, and 17 can use a proposed bounded-by-design classification. The domain is the fixed response schema.

## Binding evidence obligations

The [binding manifest](../../formal/tlaplus/node_observation/bindings.json) maps all seventeen properties to twenty-six named tests.
The [applicability review](../../formal/tlaplus/node_observation/README.md#batch-e-applicability-review) records the evidence and limits for each property.
The map includes frozen arithmetic, parser controls, consistency failures, both finalized sources, typed refusals, and resource limits.

A malformed node request closes its connection.
An invalid numeric limit returns a typed unavailable response before capture.
Both outcomes refuse admission without changing the existing protocol.

The display helper also refuses mixed capture generations or transaction identities.
That defensive check does not replace the guarded capture interval.

A Kani harness remains optional.
A named-test map is not a refinement proof.
Test results do not discharge this claim.

## Registration history

Claim 005 was registered before implementation.
Twelve source records were first registered after their source implementation commits.
Those records retain explicit registration gaps.
The maintainer must review these gaps with the source-bound evidence.

## Verification requirements

Retain the failing controls for each refusal condition and for each negative model.

Verify that the production store bytes are equal before and after accepted and rejected captures.

Verify that the source inventory and the dependency inventory contain no unrelated change.

Record compact evidence under `docs/cbc-evidence/`. Keep bulk evidence outside Git.

Source-bound verification and named maintainer acceptance are necessary before the status can change from `pending`.
