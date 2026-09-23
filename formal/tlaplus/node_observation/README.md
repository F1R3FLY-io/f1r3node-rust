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

The [Rocq project](../../rocq/node_observation/README.md) exports 14 kernel-checked theorems. Six cover challenge allocation and cross-incarnation tokens, and eight cover capture consistency, budgets, length prefixes, guard order, and detachment.

Every theorem reports `Closed under the global context`. The formal gate counts 14 closed assumption sets for the `NodeObservation.MainTheorem` module.

The capture theorems use a time-indexed transaction clock over an unbounded environment set. Equal observations at open and validation imply that no commit occurred in the observed interval, given monotone transaction identifiers.

The generation theorem is the same result over the insertion generation. The budget theorems cover any finite sequence of checked charges, and the prefix theorems cover any payload length.

The guard-order and detachment theorems hold for every reachable state of the capture protocol over any finite action sequence, including opening any number of transactions.

`Begin` projects to `Hello`, and a successful `Reply` projects to `Response`. `Tick` and `Close` project to `Rejected`. Capture actions project to the `capture_step` transitions in the same order as the TLA+ phases.

These correspondences are documented source arguments, not machine-checked refinement theorems. Rust UUID formatting, decimal formatting, string equality, checked `u64` arithmetic, and the LMDB transaction rule remain stated assumptions.

The binding tier has three forms on this branch. The capture oracle test `capture_outcomes_match_the_bounded_capture_oracle` runs production capture against a hand-translated `BoundedCapture` oracle over 14 scenarios and requires equal outcomes.

The session oracle test `session_sequences_match_an_independent_event_oracle` checks event sequences across response, replay, and disconnect. The retained pre-fix regressions cover the lock deadline, short decompression, nested collection bounds, canonical work, duration identity, body counts, and repeated entropy.

Eight Kani harnesses under `#[cfg(kani)]` in the shared reader and the block store cover the length prefix, the limit comparison, checked totals, atomic charging, and decode-limit validation. The prefix harnesses call the same `split_length_prefixed` predicate that production decoding uses. Their execution status is recorded in the evidence package.

[bindings.json](bindings.json) maps all 23 required properties to invariants, theorems, harnesses, and tests. It is provisional and does not establish complete property coverage.

## Applicability per property

`U` means construction over arbitrary permitted histories or states is required. `F` proposes a bounded-by-design classification for maintainer review. No `F` proposal is accepted, and every property keeps required Rust binding evidence.

A resource limit, timeout, frame size, or test fixture does not make a property bounded by design. Each `F` proposal names the finite domain that verification covers.

| Property | Class | Refutation | Construction | Binding | Decision |
| --- | --- | --- | --- | --- | --- |
| A1: disabled startup | F proposed | None | Not applicable proposed. The domain is the absent configuration section and the absent option. | Disabled-startup test. | Pending maintainer review. |
| A2: activation and limits | F proposed | None | Not applicable proposed. The domain is the documented integer ranges and required fields. | Configuration rejection tests. | Pending maintainer review. |
| A3: directory safety | U | None | Pending. Filesystem states are unbounded and unmodeled. | Directory, link, and duplicate-socket tests. | Pending. |
| A4: peer identity | U | `BoundIdentity` | Pending. Kernel credentials are a trust boundary. | Peer identity and cross-process tests. | Pending. |
| A5: request identity | U | `FreshChallenge`, `BoundIdentity`, `FreshChallenges`, `ReplayRefused` | `observer_replay_refused`, `observer_qualified_replay_refused`, `observer_cross_incarnation_distinct` under the distinct-incarnation assumption. | Session oracle, repeated-entropy, and identity tests. | Construction recorded, acceptance pending. |
| A6: request count and freshness | U | `FreshChallenge`, `OneRequest`, `FreshChallenges`, `ReplayRefused` | `observer_challenges_unique`, `observer_replay_refused`. | Replay and session oracle tests. | Construction recorded, acceptance pending. |
| A7: frames and deadline | U | `BoundFrame`, `BoundDeadline`, `SessionBudget` | `observer_counter_exhaustion_refused`, `observer_checked_allocation_valid` for the budget counter. The deadline remains pending. | Frame, deadline, and budget tests. | Partial construction, acceptance pending. |
| A8: capabilities and effects | F proposed | None | Not applicable proposed. The domain is the fixed capability list and the single operation. | Capability and fault-command tests. | Pending maintainer review. |
| A9: cleanup and shutdown | U | None | Pending. No complete shutdown model. | Source-order regression and cleanup tests. | Pending. |
| A10: public configuration | F proposed | None | Not applicable proposed. The domain is the fixed allowlist. | Configuration digest test. | Pending maintainer review. |
| B1: input limits | U | `ValidAdmission`, `BoundBytes` | `capture_budget_bounded`, `capture_overflow_fails_limit`. | Limit tests, capture oracle, and five Kani harnesses. | Construction recorded, acceptance pending. |
| B2: bounded locks | U | `BoundLockWait`, `GuardOrder` | `capture_guard_order`. The deadline bound relies on the lock library and remains pending. | Deadline and guard tests. | Partial construction, acceptance pending. |
| B3: environment partition | U | `OpenIdentity` | `capture_no_interference` over any participant set. | Separate-environment tests. | Construction recorded, acceptance pending. |
| B4: identity at open | U | `OpenIdentity`, `ValidatedIdentity` | `capture_no_interference`. | Open and validation tests. | Construction recorded, acceptance pending. |
| B5: allocation limits | U | `BoundBytes` | `capture_prefix_roundtrip`, `capture_prefix_sound`, `capture_budget_bounded`. | Length, decode, and nested-bound tests, and three Kani harnesses. | Construction recorded, acceptance pending. |
| B6: copied state and effects | U | `GuardOrder`, `ReadOnly` | `capture_guard_order`, `capture_detached`. | Unchanged-bytes tests. | Construction recorded, acceptance pending. |
| B7: environment validation | U | `ValidatedIdentity` | `capture_no_interference`, including restored values under monotone identifiers. | Interference tests and capture oracle. | Construction recorded, acceptance pending. |
| B8: generation validation | U | `GenerationStable` | `capture_generation_stable`. | Partial. No deterministic generation-rejection test on this branch. | Binding gap recorded. |
| B9: incomplete rows | U | `CompleteRows` | Pending. The row model is a Boolean predicate. | Missing-row tests and capture oracle. | Pending. |
| B10: resource release | U | `Detached`, `ReadOnly` | `capture_detached`. | Release and unchanged-bytes tests. | Construction recorded, acceptance pending. |
| B11: canonical identity | U | None | Pending. No model or theorem. | Digest, duration, and byte-bound tests. | Pending. |
| B12: scratch independence | U | None | Pending. No model or theorem. | Scratch independence tests. | Pending. |
| B13: unsupported backends | F proposed | `ValidAdmission` | Not applicable proposed. The domain is the backend downcast result. | Unsupported-backend tests. | Pending maintainer review. |

Every classification and correspondence remains pending named maintainer review. A recorded theorem does not change a claim status until acceptance.

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
