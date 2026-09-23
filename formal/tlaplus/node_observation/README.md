# Node observation verification

The node branch owns this canonical model set for TASK-019-4. Both node claims remain pending.

This reconciliation combines the node freshness model with the broader session and capture models from downstream revision `8a379f05a07974ae9af6b450e6cea5fa6da80e0f`.

The reconciliation starts from node revision `10e7b8452824e12a1fe2743dca7989b79fce2133`. The downstream revision identifies imported inputs, not the evidence base.

## Canonical inventory

[verification-plan.json](verification-plan.json) lists two positive configurations and 17 negative controls. Each configuration has one matching entry module.

The entry modules extend either `ObserverSession` or `BoundedCapture`. They do not contain separate state machines.

The gate registers the union of the downstream 16 controls and the node freshness control. It rejects disagreement with the plan or configuration inventory.

| Family | Positive configuration | Negative controls |
| --- | --- | --- |
| Session | `MC_ObserverSession` | `freshness_pre_fix`, `challenge_unsafe`, `identity_unsafe`, `frame_unsafe`, `deadline_unsafe`, `repeat_unsafe`, `budget_unsafe`. |
| Capture | `MC_BoundedCapture` | `admission_unsafe`, `deadline_unsafe`, `order_unsafe`, `open_unsafe`, `validation_unsafe`, `generation_unsafe`, `incomplete_unsafe`, `bytes_unsafe`, `release_unsafe`, `write_unsafe`. |

A clean check must exit zero with the completed-search marker and no error. A negative check must exit 12 with its named invariant and a trace.

Parser errors, timeouts, unrelated violations, duplicate violations, contradictory output, and incomplete output fail the gate. Fixture tests are not model executions.

## Session model

The configured domain has three sessions, two random values, two logical time units, and normalized frame sizes of zero, two, and three.

The frame limit is two. Bug controls can admit one extra session or response to expose the corresponding boundary violation.

Repeated random values are permitted. Challenge allocation combines the random value with the next hello-event sequence.

Responses also consume event sequences. Rejected requests and disconnected sessions do not reset the sequence.

`FreshChallenges` checks uniqueness of issued tokens. `FreshChallenge` checks that a response used the current complete token.

`ReplayRefused` checks the request occurrence, not only token equality. The freshness control removes the sequence component without disabling request comparison.

The challenge control separately disables request comparison. Neither control assumes random UUID uniqueness.

| Model transition or invariant | Rust boundary | Limit |
| --- | --- | --- |
| `Begin`, `sequence`, `FreshChallenges` | Hello allocation in `Observer::session` and `Observer::event`. | One observer lifetime only. |
| `Reply`, `FreshChallenge`, `ReplayRefused` | Complete challenge equality and response event allocation. | Other identity fields use Boolean abstractions. |
| `Close` | Request refusal, disconnect, or cancellation. | No complete node-shutdown proof. |
| `BoundIdentity` | Peer and request identity checks. | Kernel credentials and parsing remain outside the model. |
| `BoundFrame`, `BoundDeadline` | Framing and complete-session timeout. | Admission predicates, not native scheduling guarantees. |
| `OneRequest`, `SessionBudget` | Serial session handling and connection budget. | Accepted connections consume budget even without replies. |

The model can allocate a challenge before rejecting a false peer predicate. Production checks peer credentials before hello allocation.

This abstraction permits extra histories. It checks response admission, not handshake disclosure or the complete peer-authentication property.

The model uses natural-number event sequences. The unchanged Rocq refusal theorem and Rust exhaustion test cover the checked-counter boundary separately.

Cross-incarnation freshness remains unresolved. Filesystem safety, configuration, capability effects, serialization, and shutdown still need their separate evidence.

## Capture model

`BoundedCapture` retains the downstream transition system. Its domain has two environments, two commits per environment, one generation change, and three guards.

The model samples each environment before open, at open, and during validation. Writers can commit between these observations.

Transaction identifiers increase even when application bytes return to an earlier value. This model assumes the LMDB transaction rule and does not model application payload restoration.

Separate validation observations do not prove atomic publication across environments. Generation equality does not replace transaction checks or exclude every writer history.

The lock deadline can expire before or between acquisitions. A contended acquisition cannot succeed after expiry.

An uncontended acquisition can succeed after expiry, consistent with the lock implementation. An earlier acquired guard need not disappear when the deadline expires.

Copied sizes are one, two, or three units, with a limit of two. These values do not cover every Rust allocation or decoder input.

| Invariants | Rust boundary | Limit |
| --- | --- | --- |
| `ValidAdmission`, `BoundLockWait`, `GuardOrder` | Limit validation and the three capture locks. | Native lock behavior remains a correspondence obligation. |
| `OpenIdentity`, `ValidatedIdentity` | `BoundedLmdbReader::open` and `validate`. | LMDB transaction semantics remain trusted. |
| `BoundBytes`, `CompleteRows` | Pre-allocation budgets and required-row checks. | Normalized sizes, not complete decoder verification. |
| `GenerationStable` | The final insertion-generation comparison. | Injected changes can include interference that normal guard ownership prevents. |
| `Detached`, `ReadOnly` | Reader consumption, guard release, and sealing. | Effect abstraction, not a complete store-write proof. |

Both models permit stuttering and assume no fairness. They establish bounded safety results, not eventual completion or universal timing bounds.

## Construction and binding

The [Rocq project](../../rocq/node_observation/README.md) retains four challenge-allocation theorems over arbitrary finite event histories.

`Begin` projects to `Hello`, and a successful `Reply` projects to `Response`. `Tick` and `Close` project to `Rejected`.

The projection reverses the TLA+ challenge history because Rocq stores the newest token first. Extra admission guards restrict transitions without changing allocation arithmetic.

This correspondence is a source argument, not a machine-checked refinement theorem. The Rocq results do not prove the additional session or capture invariants.

The production repeated-entropy test supplies `Uuid::nil` twice. The integration oracle checks event sequences across response, replay, and disconnect.

Rust UUID formatting, decimal formatting, exact string equality, and checked arithmetic still require their stated correspondence assumptions.

[bindings.json](bindings.json) maps all 23 required properties to supporting tests and invariants. It is provisional and does not establish complete property coverage.

The map distinguishes supplemental node unit tests from interface tests. B8 explicitly records the missing deterministic generation-rejection test on this node revision.

The downstream-only generation test is not silently imported. The existing shutdown test checks source ordering, not complete production shutdown.

The [applicability map](../../../docs/work-logs/task-019-4-node-claim-verification.md#property-coverage-plan) remains provisional. No property has an accepted construction exemption.

## Applicability per property

`U` proposes construction over arbitrary permitted histories or states. `F/U` identifies a finite predicate within a larger property, not a construction exemption.

Every classification and Rust correspondence remains pending named maintainer review. The linked applicability map retains the domains, assumptions, and evidence gaps.

| Property | Proposed class | Current model evidence |
| --- | --- | --- |
| A1: disabled startup | U | No model coverage. |
| A2: activation and limits | F/U | No model coverage. |
| A3: directory safety | U | No model coverage. |
| A4: peer identity | U | Boolean response-admission predicate only. |
| A5: request identity | U | Complete challenge comparison and Boolean identity predicates. |
| A6: request count and freshness | U | One request and within-lifetime challenge allocation. |
| A7: frames and deadline | F/U | Normalized frame, clock, and session-budget predicates. |
| A8: capabilities and effects | F/U | No model coverage. |
| A9: cleanup and shutdown | U | No complete shutdown model. |
| A10: public configuration | F/U | No model coverage. |
| B1: input limits | U | Abstract admission and byte bounds. |
| B2: bounded locks | U | Guard order and contended deadline. |
| B3: environment partition | U | Two fixed participants only. |
| B4: identity at open | U | Abstract transaction comparisons. |
| B5: allocation limits | U | Normalized copied sizes only. |
| B6: copied state and effects | U | Abstract guards and effects. |
| B7: environment validation | U | Monotonic transaction observations. |
| B8: generation validation | U | Abstract generation comparison. |
| B9: incomplete rows | U | Boolean completeness predicate. |
| B10: resource release | U | Abstract handles and effects. |
| B11: canonical identity | U | No model coverage. |
| B12: scratch independence | U | No model coverage. |
| B13: unsupported backends | F/U | Boolean backend predicate. |

## Reproduction and integration

1. Run `bash scripts/ci/check-tla-invariants.sh --soak-pr` with the pinned TLC jar.
2. Run `bash scripts/ci/test-check-tla-invariants.sh` with Bash 4 or later.
3. Run `bash scripts/ci/check-node-observation-bindings.sh /path/to/new-output` in the isolated Rust build environment.
4. Rebuild the Rocq project and check all four exported assumption sets.

The binding driver runs node tests, not the complete capture suite. It does not accept a named-test map as a refinement proof.

This branch uses the existing gate and binding driver. It adds no standalone checker crate, workspace, or lockfile.

Downstream owns `scripts/node-observation` and must remove it or include it in its supply-chain audit before acceptance.

That downstream binding parser also assumes 18 interface tests and two storage unit tests. Those assumptions do not match this node revision.

Downstream integration must take these canonical model files and combine gate registrations. It must preserve the Rocq registrations and the node binding driver.

The node package records gate registration evidence separately. The harness claim keeps its primary record under `docs/casper/cbc-evidence/` without node claim digests.

The default legacy gate record also retains its own claim and historical source identity. Model reconciliation does not renew either governance claim.

Named maintainer acceptance, complete required verification, and the strict claim gate remain necessary. B2 planning remains blocked.
