# Casper Node Paired Fork-Choice Observation

This claim was registered as pending on 2026-09-30, before the implementation. It contains no acceptance evidence.

```yaml
claim_id: CLAIM-CASPER-NODE-OBSERVATION-004
status: pending
adapter: null
scope: batch-d-paired-fork-choice
artifacts:
  - casper/src/rust/soak_observer.rs
  - casper/src/rust/soak_observer/evaluation.rs
  - casper/src/rust/soak_observer/fork_choice.rs
  - casper/src/rust/estimator.rs
  - casper/src/rust/util/dag_operations.rs
  - casper/src/rust/util/proto_util.rs
  - casper/src/rust/finality/floor.rs
  - shared/src/rust/dag/observation_work.rs
  - node/src/rust/soak_observer.rs
  - node/tests/soak_observer.rs
  - casper/tests/soak_observer.rs
  - formal/tlaplus/node_observation/PairedForkChoice.tla
  - formal/tlaplus/node_observation/MC_PairedForkChoice.tla
  - formal/tlaplus/node_observation/MC_PairedForkChoice.cfg
  - formal/tlaplus/node_observation/MC_PairedForkChoice_digest_unsafe.tla
  - formal/tlaplus/node_observation/MC_PairedForkChoice_digest_unsafe.cfg
  - formal/tlaplus/node_observation/MC_PairedForkChoice_floor_unsafe.tla
  - formal/tlaplus/node_observation/MC_PairedForkChoice_floor_unsafe.cfg
  - formal/tlaplus/node_observation/MC_PairedForkChoice_absent_unsafe.tla
  - formal/tlaplus/node_observation/MC_PairedForkChoice_absent_unsafe.cfg
  - formal/tlaplus/node_observation/MC_PairedForkChoice_compare_unsafe.tla
  - formal/tlaplus/node_observation/MC_PairedForkChoice_compare_unsafe.cfg
  - formal/tlaplus/node_observation/MC_PairedForkChoice_budget_unsafe.tla
  - formal/tlaplus/node_observation/MC_PairedForkChoice_budget_unsafe.cfg
  - formal/tlaplus/node_observation/README.md
  - formal/tlaplus/node_observation/verification-plan.json
  - formal/tlaplus/node_observation/bindings.json
  - scripts/ci/check-tla-invariants.sh
  - scripts/ci/test-check-tla-invariants.sh
refutation: pending
construction: pending
binding: pending
soak: pending
```

## Authorization and scope

The user confirmed the 34-file scope and accepted the 12 decisions of [the Batch D plan](../plans/casper-node-observation-batch-d.md) on 2026-09-30. The implementation starts from `feature/casper-node-observation` at `e90e4cffa`.

The named maintainer for the acceptance is `@jltatbeach`. The 12 decisions came from the user. The maintainer can change a decision at the acceptance review.

Claims 001, 002, and 003 are accepted at their recorded revisions. That acceptance does not cover the Batch D changes.

Batch D changes 8 files that claim 003 lists as artifacts. The accepted digests of those files become out of date.

## Required properties

Each property is a statement that the verification must show. This registration does not state that the design has these properties.

1. One request makes one capture. The 2 fork-choice evaluations read that capture and no live store.
2. The fork-choice input digest covers the capture digest, the authority inputs, the fork-choice inputs, the latest message scope, and the 2 lower bound rules.
3. The adopted parent limits and the approved block identity supply the fork-choice inputs. Startup values cannot replace them.
4. The `bounded` evaluation uses the production fork-choice functions with the checked meter, on its own scratch view.
5. The `bounded` evaluation applies the 2 latest message filters of the production caller.
6. The `reference` evaluation reads immutable captured data. It calls no production estimator, common ancestor, floor, or traversal function.
7. Each result names its mode, its lower bound, its lower bound rule, and its input digest.
8. A comparison uses 2 available results with equal input digests. A different condition gives an unavailable comparison.
9. A head field contains only a head that its evaluation selected. A floor, an oracle result, or a default value never fills it.
10. A missing input, a limit, or an error gives an unavailable or failed state with a reason.
11. The 2 evaluations use the shared budget and the request deadline. Each charge occurs before its operation.
12. The work report has separate counts for the 2 fork-choice paths. The aggregate contains them.
13. A metered production function with `NoopWork` gives the same result and the same error as the function before the change.
14. A request with no fork-choice selection gives the same Batch B2 results and the same authority digest as before the change.
15. The observation changes no production store, no production cache, no consensus limit, and no proposal.
16. The response keeps `live_profile_qualified` equal to `false`.

## Tier plan for each property

| Property | Class | Refutation plan | Construction plan | Binding plan |
|----------|-------|-----------------|-------------------|--------------|
| 1: one capture | U | New model, property `OneCapture`, control `digest_unsafe` | Inherit the accepted capture theorems. The scratch construction stays pending. | Test of one capture call and of store bytes |
| 2: digest coverage | U | Model property `OneCapture` | Pending. Digest coverage of each input stays a hash assumption. | Test that changes each input and requires a different digest |
| 3: adopted inputs | U | None | Pending. The adoption routes are not modeled. | Test with different startup and adopted values |
| 4: measured path | U | None | Pending. The call structure is not mechanized. | Test with call doubles, and the scratch view test |
| 5: caller filters | U | None | Pending. The filter in the observer is a copy of the caller logic (decision 3). | Differential test against the first parent of a production snapshot |
| 6: independent reference | U | None | Pending. The reference semantics are not mechanized. | 6 reference controls, each with a required mismatch |
| 7: result identity | F proposed | None | Not applicable proposed. The domain is the fixed response schema. | Schema test of each result |
| 8: comparison rule | U | Model property `CompareSameInput`, control `compare_unsafe` | Extend `authority_comparison_requires_equal_digest`, if the maintainer requires it | Test with 2 different captures |
| 9: no substitute head | U | Model property `HeadNotFloor`, control `floor_unsafe` | Pending | Schema test and a test with a floor above the approved block |
| 10: explicit absence | U | Model property `NoFabricatedHead`, control `absent_unsafe` | Pending. Refusal completeness is not modeled. | One test for each row of the failure table in the plan |
| 11: work bounds | U | Model property `SharedBudget`, control `budget_unsafe` | Inherit the 5 budget theorems of `NodeAuthority`. Charge placement stays pending. | Limit tests for each work kind, and a deadline test |
| 12: separate counts | F proposed | None | Not applicable proposed. The domain is the fixed number of paths. | Test that the aggregate is the sum of the 6 paths |
| 13: unchanged production result | U | None | Pending | The existing fork-choice and floor suites, and one differential test for each changed function |
| 14: unchanged Batch B2 result | U | None | Pending | Byte comparison of the response and of the digest for a request with no selection |
| 15: read-only behavior | U | None | Pending. Effect confinement is not modeled. | Production byte comparisons and the ordinary regression tests |
| 16: no live qualification | F proposed | None | Not applicable proposed. The value is a constant. | Response test |

Class U means that the property covers each permitted input. Class F means that the property has a fixed finite domain.

## Evidence and acceptance

The tests must cover each property and each control of the plan. A bounded test instance does not show the complete property.

The bounded model explores a finite domain. It does not show the properties for each permitted DAG or request history.

The binding tier maps each property to named tests in the bindings manifest. The manifest has no machine-checked refinement.

The compact evidence package goes in `docs/cbc-evidence/runs/`. The ledger records go in `docs/cbc-evidence/`.

The claim stays pending until the named maintainer accepts the source-bound evidence.

## What the claim does not cover

| Item | Cause |
|------|-------|
| That the floor bound never changes the selected head | The batch reports each comparison. Decision D-03 records that this proof is open. |
| That the production fork choice conforms to its specification | The reference comparison gives evidence for the observed captures only. |
| That the reference evaluation is correct for each DAG | The reference semantics are not mechanized. |
| The display projection and the equivocation inputs | Batch E |
| The selected chain of the public `showMainChain` API | The batch does not change or observe that API. |
| The fork choice that a live proposal used | The observation is detached. It starts from a later capture. |
| A DAG above the capture limits | The capture refuses it. |
| Qualification of a live authority profile | The harness and TASK-017-12 own that qualification. |
| The harness mapping of the new fields | TASK-017-12 on the soak branch |
| Process memory and operating system latency | The allocation charges cover accounted data only. |
| Collision resistance of SHA-256 | Assumption |
| The soak tier | It stays pending until a qualified campaign runs. |
