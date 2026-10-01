# Node observation verification

The node branch owns this canonical model set for TASK-019-4. Both node claims were accepted on 2026-09-23 at revision `4c0c0dbe7` in PR #447 review 5294038948.

This reconciliation combines the node freshness model with the broader session and capture models from downstream revision `8a379f05a07974ae9af6b450e6cea5fa6da80e0f`.

The reconciliation starts from node revision `10e7b8452824e12a1fe2743dca7989b79fce2133`. The downstream revision identifies imported inputs, not the evidence base.

## Canonical inventory

[verification-plan.json](verification-plan.json) lists three positive configurations and 22 negative controls. Each configuration has one matching entry module.

The entry modules extend `ObserverSession`, `BoundedCapture`, or `PairedForkChoice`. They do not contain separate state machines.

The gate registers the union of the downstream 16 controls, the node freshness control, and the 5 Batch D controls. It rejects disagreement with the plan or configuration inventory.

| Family | Positive configuration | Negative controls |
| --- | --- | --- |
| Session | `MC_ObserverSession` | `freshness_pre_fix`, `challenge_unsafe`, `identity_unsafe`, `frame_unsafe`, `deadline_unsafe`, `repeat_unsafe`, `budget_unsafe`. |
| Capture | `MC_BoundedCapture` | `admission_unsafe`, `deadline_unsafe`, `order_unsafe`, `open_unsafe`, `validation_unsafe`, `generation_unsafe`, `incomplete_unsafe`, `bytes_unsafe`, `release_unsafe`, `write_unsafe`. |
| Paired fork choice | `MC_PairedForkChoice` | `digest_unsafe`, `floor_unsafe`, `absent_unsafe`, `compare_unsafe`, `budget_unsafe`. |

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

## Paired fork-choice model

`PairedForkChoice` models the Batch D observation of [CLAIM-CASPER-NODE-OBSERVATION-004](../../../docs/claims/casper-node-fork-choice-observation.md). Its domain has two captures, two candidate heads, and a budget of three work units.

One request makes one capture. The `bounded` evaluation runs first and the `reference` evaluation second. Each evaluation reads a capture digest, selects a head or refuses with a reason, and charges work units to the shared budget.

An evaluation that would exceed the budget stops with the reason `limit` and charges the remaining units only. The comparison runs last. It is available only when both results are available and both input digests are equal.

The model does not contain the fork-choice rules, the DAG, or the weights. A selected head is an arbitrary candidate. The model states which values a result can carry and when a comparison is permitted, not which head is correct.

| Invariants | Rust boundary | Limit |
| --- | --- | --- |
| `OneCapture` | The input digest of each evaluation in `evaluation.rs` and the capture binding in `soak_observer.rs`. | The digest coverage of each input is a hash assumption. |
| `HeadNotFloor` | The head fields of `ForkChoiceResult`. | The model has one floor value. It does not model the oracle results. |
| `NoFabricatedHead` | The state and reason fields of each result. | Refusal completeness is not modeled. The refusal reasons are a sample. |
| `CompareSameInput` | The comparison in `evaluation.rs`. | The model compares heads only, not tips or scores. |
| `SharedBudget` | The two new work paths in `observation_work.rs` and the charge sites. | Charge placement in the Rust code is a correspondence obligation. |

Each negative control sets the `Bug` constant to one value and lists `TypeOK` and its named invariant. The `compare_unsafe` control also lets the reference read a different capture, because a comparison of equal inputs cannot show the missing digest check.

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

Seven Kani harnesses under `#[cfg(kani)]` in the shared reader and the block store cover the length prefix, the limit comparison, atomic charging, and decode-limit validation. The charging harnesses drive the checked-total arithmetic with fully symbolic inputs. The prefix harnesses call the same `split_length_prefixed` predicate that production decoding uses. Their execution status is recorded in the evidence package.

[bindings.json](bindings.json) maps all 23 required properties to invariants, theorems, harnesses, and tests. It is provisional and does not establish complete property coverage.

## Applicability per property

`U` means construction over arbitrary permitted histories or states is required. `F` proposes a bounded-by-design classification for maintainer review. The five `F` proposals were accepted as bounded by design on 2026-09-23, and every property keeps required Rust binding evidence.

A resource limit, timeout, frame size, or test fixture does not make a property bounded by design. Each `F` proposal names the finite domain that verification covers.

| Property | Class | Refutation | Construction | Binding | Decision |
| --- | --- | --- | --- | --- | --- |
| A1: disabled startup | F proposed | None | Not applicable proposed. The domain is the absent configuration section and the absent option. | Disabled-startup test. | Accepted as bounded by design. |
| A2: activation and limits | F proposed | None | Not applicable proposed. The domain is the documented integer ranges and required fields. | Configuration rejection tests. | Accepted as bounded by design. |
| A3: directory safety | U | None | Pending. Filesystem states are unbounded and unmodeled. | Directory, link, and duplicate-socket tests. | Construction gap accepted as recorded. |
| A4: peer identity | U | `BoundIdentity` | Pending. Kernel credentials are a trust boundary. | Peer identity and cross-process tests. | Construction gap accepted as recorded. |
| A5: request identity | U | `FreshChallenge`, `BoundIdentity`, `FreshChallenges`, `ReplayRefused` | `observer_replay_refused`, `observer_qualified_replay_refused`, `observer_cross_incarnation_distinct` under the distinct-incarnation assumption. | Session oracle, repeated-entropy, and identity tests. | Construction recorded, accepted. |
| A6: request count and freshness | U | `FreshChallenge`, `OneRequest`, `FreshChallenges`, `ReplayRefused` | `observer_challenges_unique`, `observer_replay_refused`. | Replay and session oracle tests. | Construction recorded, accepted. |
| A7: frames and deadline | U | `BoundFrame`, `BoundDeadline`, `SessionBudget` | `observer_counter_exhaustion_refused`, `observer_checked_allocation_valid` for the budget counter. The deadline remains pending. | Frame, deadline, and budget tests. | Partial construction, deadline gap accepted as recorded. |
| A8: capabilities and effects | F proposed | None | Not applicable proposed. The domain is the fixed capability list and the single operation. | Capability and fault-command tests. | Accepted as bounded by design. |
| A9: cleanup and shutdown | U | None | Pending. No complete shutdown model. | Source-order regression and cleanup tests. | Construction gap accepted as recorded. |
| A10: public configuration | F proposed | None | Not applicable proposed. The domain is the fixed allowlist. | Configuration digest test. | Accepted as bounded by design. |
| B1: input limits | U | `ValidAdmission`, `BoundBytes` | `capture_budget_bounded`, `capture_overflow_fails_limit`. | Limit tests, capture oracle, and four Kani harnesses. | Construction recorded, accepted. |
| B2: bounded locks | U | `BoundLockWait`, `GuardOrder` | `capture_guard_order`. The deadline bound relies on the lock library and remains pending. | Deadline and guard tests. | Partial construction, deadline gap accepted as recorded. |
| B3: environment partition | U | `OpenIdentity` | `capture_no_interference` over any participant set. | Separate-environment tests. | Construction recorded, accepted. |
| B4: identity at open | U | `OpenIdentity`, `ValidatedIdentity` | `capture_no_interference`. | Open and validation tests. | Construction recorded, accepted. |
| B5: allocation limits | U | `BoundBytes` | `capture_prefix_roundtrip`, `capture_prefix_sound`, `capture_budget_bounded`. | Length, decode, and nested-bound tests, and three Kani harnesses. | Construction recorded, accepted. |
| B6: copied state and effects | U | `GuardOrder`, `ReadOnly` | `capture_guard_order`, `capture_detached`. | Unchanged-bytes tests. | Construction recorded, accepted. |
| B7: environment validation | U | `ValidatedIdentity` | `capture_no_interference`, including restored values under monotone identifiers. | Interference tests and capture oracle. | Construction recorded, accepted. |
| B8: generation validation | U | `GenerationStable` | `capture_generation_stable`. | Generation-rejection test at the validated phase, unchanged-generation writer tests, and capture oracle. | Construction recorded, accepted. |
| B9: incomplete rows | U | `CompleteRows` | Pending. The row model is a Boolean predicate. | Missing-row tests and capture oracle. | Construction gap accepted as recorded. |
| B10: resource release | U | `Detached`, `ReadOnly` | `capture_detached`. | Release and unchanged-bytes tests. | Construction recorded, accepted. |
| B11: canonical identity | U | None | Pending. No model or theorem. | Digest, duration, and byte-bound tests. | Construction gap accepted as recorded. |
| B12: scratch independence | U | None | Pending. No model or theorem. | Scratch independence tests. | Construction gap accepted as recorded. |
| B13: unsupported backends | F proposed | `ValidAdmission` | Not applicable proposed. The domain is the backend downcast result. | Unsupported-backend tests. | Accepted as bounded by design. |

The named maintainer reviewed and accepted every classification on 2026-09-23. A recorded theorem changed the claim status only through that acceptance.

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

Named maintainer acceptance was recorded on 2026-09-23 at revision `4c0c0dbe7` on the package `casper-node-claim-gate-78d696ea6-01`. The two gate-claim records in the union inventory stay pending under their own claim. B2 planning is unblocked.


## Batch B2 applicability review

[CLAIM-CASPER-NODE-OBSERVATION-003](../../../docs/claims/casper-node-authority-evaluation.md) covers the detached authority observer.
The earlier acceptance applies to claims 001 and 002 at their recorded revision.
It does not accept this extension.

B2 accepts positive request limits below the ceilings in the [batch plan](../../../docs/plans/casper-node-observation-batch-b.md#work-bounds).
The ceilings bound each request.
They do not establish correctness for every permitted DAG or installation history.

The named maintainer accepted every classification below on 2026-09-23 in PR #447 review 5294038948 at revision `237e43d72`.
The `F` proposals name a finite domain. The `U` rows require construction over every permitted history, and the construction column names the kernel-checked theorem where one exists.

Batch B2 adds no bounded TLA+ model. Its refutation tier is inherited from the accepted session and capture models where a property extends them, and is otherwise absent.
The construction theorems live in the [`NodeAuthority` project](../../rocq/node_authority/README.md). Its 15 exported results have closed assumption sets and are registered in the formal gate.

| Property | Class | Refutation | Construction | Binding | Decision |
| --- | --- | --- | --- | --- | --- |
| C1: disabled startup | F proposed | None | Not applicable proposed. The domain is the fixed set of constructor routes and the default configuration. | Default configuration and disabled installation tests. | Accepted as bounded by design. |
| C2: narrow attachment | U | None | Pending. Capability exclusion is not modeled. | Endpoint construction and forbidden-call test doubles. | Construction gap accepted as recorded. |
| C3: one attachment | U | None | `authority_instance_attaches_at_most_once`, `authority_first_attachment_wins`. | Once-only attachment and replacement tests. | Construction recorded, accepted. |
| C4: instance coverage | U | None | `authority_replaced_binding_refused`, `authority_shutdown_refuses_all`, `authority_installation_never_wraps`. Concurrent replacement during response publication remains pending. | Replacement, busy, cancellation, and closed tests. | Partial construction, remaining part accepted as recorded. |
| C5: nonblocking records | U | None | `authority_queue_never_exceeds_capacity`, `authority_ledger_accounts_for_every_attempt`, `authority_complete_coverage_delivers_every_attempt`, `authority_sequence_exhaustion_refused`. The lost counter bound is trusted to the same checked pattern. | Queue loss, closed receiver, malformed hash, and overflow tests. | Construction recorded, accepted. |
| C6: event meaning | U | None | Pending. Finalizer hook placement is not modeled. | Event variant and effect failure tests. | Construction gap accepted as recorded. |
| C7: request envelope | U | Inherited `MC_ObserverSession` | Inherited accepted session theorems. The authority operation adds no challenge allocation. | Identity test and the session suite. | Inherited from claim 001, extension accepted. |
| C8: detached capture | U | Inherited `MC_BoundedCapture` | Inherited accepted capture theorems. Scratch reconstruction remains pending. | One capture call, scratch independence, and byte comparisons. | Inherited from claim 002, extension accepted. |
| C9: result identity | U | None | `authority_comparison_requires_equal_digest`. Digest coverage of every input remains a hash assumption. | Exact and strict comparison tests. | Partial construction, remaining part accepted as recorded. |
| C10: adopted parameters | U | None | Pending. Adoption routes are not modeled. | Conflicting threshold test. | Construction gap accepted as recorded. |
| C11: independent reference | U | None | Pending. Reference semantics are not mechanized. | Exhaustive five-vertex cliques and five reference controls. | Construction gap accepted as recorded. |
| C12: work bounds | U | None | `authority_shared_budget_bounded`, `authority_budget_failure_sticky`, `authority_budget_failure_keeps_usage`, `authority_paths_share_one_budget`, `authority_checked_overflow_is_limit_failure`. Charge placement at every operation remains pending. | Shared path, clique, traversal, cancellation, and partial counter tests. | Partial construction, remaining part accepted as recorded. |
| C13: distinct values | F proposed | None | Not applicable proposed. The domain is the fixed response schema. | Typed availability and unavailable display tests. | Accepted as bounded by design. |
| C14: unavailable inputs | U | None | Pending. Refusal completeness is not modeled. | Missing target, body, and restore provenance tests. | Construction gap accepted as recorded. |
| C15: read-only behavior | U | None | Pending. Effect confinement is not modeled. | Production byte comparisons and ordinary regressions. | Construction gap accepted as recorded. |

Five properties have complete or partial construction theorems. Eight properties keep pending construction with Rust tests only, and two of those inherit accepted theorems from claims 001 and 002. Two properties propose a bounded-by-design classification.

The independent reference reads immutable snapshot data.
It does not call the production floor, oracle, clique, or traversal helpers.
Both paths use the same bounded block decoder and captured source data.
Those shared inputs and the decoder remain trusted dependencies of the comparison.

The original oracle receives a separate scratch view.
The reference ignores captured optimization caches only when captured ancestry reaches genesis.
Truncated ancestry reports `restore_seed_provenance_unavailable`.
The display projection reports `equivocation_snapshot_unavailable` because B1 excludes the equivocation tracker.

Work counters describe instrumented algorithm operations.
Preparation accounts for scratch capacity separately from measured decision counters.
Allocation charges bound accounted payloads and collection capacity, not process memory or allocator latency.
The shared budget theorems prove the arithmetic of one budget across all paths.
They do not prove that B2 charges every required operation.

Live coverage starts with finalizer contexts created after attachment.
Earlier contexts have no binding and fall outside this scope.
A successful effect return does not establish persisted metadata.
A separate capture must observe persisted metadata.

B2 does not claim a live authority profile.
The [work log](../../../docs/work-logs/task-019-3-node-authority-evaluation.md) records the tests, the construction cycle, and the source-bound evidence packages.

## Batch D applicability review

[CLAIM-CASPER-NODE-OBSERVATION-004](../../../docs/claims/casper-node-fork-choice-observation.md) covers the paired fork-choice observation.
The earlier acceptances apply to claims 001, 002, and 003 at their recorded revisions.
They do not accept this extension.

Batch D adds the bounded model `PairedForkChoice` with 5 invariants and 5 negative controls.
The model has no construction project. Its refutation tier covers the value states, the capture binding, and the shared budget.
It does not cover the fork-choice rules. The Rust tests bind those rules to the production estimator and to the [specification](../../../docs/casper/theory/fork-choice/fork-choice-specification.md).

The named maintainer has not reviewed the classifications below. The decision column records the proposal only.

| Property | Class | Refutation | Construction | Binding | Decision |
| --- | --- | --- | --- | --- | --- |
| D1: one capture | U | `OneCapture`, control `digest_unsafe` | Inherited capture theorems. The scratch construction stays pending. | Reference on the capture and the scratch view test. | Pending maintainer review. |
| D2: digest coverage | U | `OneCapture` | Pending. Digest coverage of each input is a hash assumption. | Selection and capture change tests. | Pending maintainer review. |
| D3: adopted inputs | U | None | Pending. The adoption routes are not modeled. | Endpoint value test and the zero parent limit test. | Pending maintainer review. |
| D4: measured path | U | None | Pending. The call structure is not mechanized. | Production estimator parity, scratch view, and 12 random DAGs. | Pending maintainer review. |
| D5: caller filters | U | None | Pending. The filter is a copy of the caller logic. | Estimator parity and the count parity of the 2 evaluations. | Pending maintainer review. |
| D6: independent reference | U | None | Pending. The reference semantics are not mechanized. | 5 reference controls with a required head mismatch. | Pending maintainer review. |
| D7: result identity | F proposed | None | Not applicable proposed. The domain is the fixed response schema. | Result field tests and the schema check test. | Pending maintainer review. |
| D8: comparison rule | U | `CompareSameInput`, control `compare_unsafe` | Pending. The extension of the authority theorem awaits the maintainer. | Different captures and unavailable results. | Pending maintainer review. |
| D9: no substitute head | U | `HeadNotFloor`, control `floor_unsafe` | Pending. | Zero parent limit, schema check, and heads in tips on 12 random DAGs. | Pending maintainer review. |
| D10: explicit absence | U | `NoFabricatedHead`, control `absent_unsafe` | Pending. Refusal completeness is not modeled. | 7 refusal tests, with 2 rows untested. | Pending maintainer review. |
| D11: work bounds | U | `SharedBudget`, control `budget_unsafe` | Inherited budget theorems through the shared meter, with charge placement pending. | Limit tests of the 4 metered functions and of the reference. | Pending maintainer review. |
| D12: separate counts | F proposed | None | Not applicable proposed for the fixed number of paths. | Sum of the 6 paths and the response work fields. | Pending maintainer review. |
| D13: unchanged production result | U | None | Pending. | 4 differential tests and the ordinary estimator and floor suites. | Pending maintainer review. |
| D14: unchanged Batch B2 result | U | None | Pending. | Byte and digest comparison without a selection. | Pending maintainer review. |
| D15: read-only behavior | U | None | Pending without a model of effect confinement. | Scratch view test and the ordinary regressions. | Pending maintainer review. |
| D16: no live qualification | F proposed | None | Not applicable proposed for the constant value. | Node capability and admission tests. | Pending maintainer review. |

Three properties propose a bounded-by-design classification. Thirteen properties keep pending construction with Rust tests, and 2 of those inherit accepted theorems through the capture and the shared meter.

The work fields of a fork-choice result have these meanings.
`visited_blocks` is the metadata count of the evaluation path.
`examined_edges` is the traversal count of the evaluation path.

The `bounded` path counts the reads of the production functions under the checked meter.
The `reference` path counts the reads of the captured maps. The 2 counts are not comparable.

The `bounded` and `reference` results agree in head and tips on every fixture.
Their score maps do not agree in extent.

The production estimator credits the main parent of a block at the common ancestor height.
The reference stops at the common ancestor, as R-SCORE states. The comparison does not compare scores.

A production error in the `bounded` evaluation gives the failed state with an error class, such as `production_error:missing_block`.
A work limit gives the unavailable state with the limit reason.

The reference gives `history_incomplete` when the captured history does not reach the lower bound.

The testimony filter has no negative control. The DAG storage records a latest message under its sender only, so a foreign message is not constructible through the capture.
The count parity of the 2 evaluations covers that filter.

Batch D does not claim a live fork-choice profile.
The [work log](../../../docs/work-logs/task-019-9-paired-fork-choice.md) records the steps, the findings, and the test results.


## Batch E applicability review

Claim 005 keeps the snapshot schema at version 2.
A display request adds a bounded tracker capture and separate display inputs.
The property-to-test map covers all seventeen properties with twenty-six named tests.
The map is not a machine-checked refinement proof.

| Property | Evidence | Limit |
| --- | --- | --- |
| E1 | Legacy request and digest tests | Fixed schema review |
| E2 | Admission and socket tests | Typed limits refusals |
| E3 | Capture and mixed-interval tests | LMDB identity trusted |
| E4 | Parser and bounded-read refusals | Producer encoding tested |
| E5 | Tracker write interference | Cooperative transaction model |
| E6 | Before-and-after store bytes | Captured fixture stores |
| E7 | Canonical digest field controls | Wire construction pending |
| E8 | Input digest and value tests | SHA-256 trusted |
| E9 | Casper methods panic on invocation | Detached fixture boundary |
| E10 | Both finalized sources and typed missing history | Frozen source rule |
| E11 | Multiplicity and arithmetic boundaries | IEEE-754 construction pending |
| E12 | Total and matched overflow refusals | Integer construction |
| E13 | Typed unavailable results | No fabricated value |
| E14 | Separate fields and input identities | Fixed schema review |
| E15 | Schema 2 and legacy output | Wire construction pending |
| E16 | Frozen shared-helper tests | No atomic live equivalence |
| E17 | Work, allocation, deadline, cancellation | Fixed response review |

The positive model checks `NoFabrication`, `BaseSource`, and `OneInterval`.
Each negative control must violate its named invariant with exit 12 and a trace.
The capture model remains an inherited bounded abstraction.

The integer construction has seven theorems in `DisplayProjection.v`.
Matched weight is bounded by total weight only when validator records do not repeat.
Repeated records deliberately contribute repeated terms.
The model does not prove floating-point arithmetic or tracker wire encoding.

The display helper checks capture generation and transaction identities before calculation.
Work charges precede identity comparison, hash-table allocation, key processing, checked sums, and shared-helper scans.
Hash-table charges include spare capacity and control storage.
The existing six work paths share their aggregate budget.

The tracker rows are not response fields.
The response contains counts and digests only.
The node closes malformed requests and returns typed refusals for invalid numeric limits.
A typed refusal does not require a closed connection.

E1, E14, and E17 require named maintainer review of their bounded-by-design classifications.
The twelve late source-record registrations remain explicit evidence gaps.
Claim 005 stays pending until the maintainer accepts the source-bound evidence and the stated limits.
