# Workspace-regression formal invariant catalog — 2026-08-28


## Binder-family substitution schedules (SBS, task 5916)

`SubstitutionSchedule.v` has 21 closed obligations, kernel-checked before
native witness changes. It composes offset reconstruction maps over semantic
child lists and instantiates Receive, Match and New. EMethod's target-last
constructor convention has a separate round-trip. Context arithmetic uses mathematical
integers: native correspondence requires the relevant signed-32-bit calculations
to remain representable. This is not an overflow proof. Descendant evaluation is
an abstract function; its own semantic refinement is not assumed proven by this model.

| ID | Closed declaration | Native binding obligation |
|---|---|---|
| SBS-01 | `binder_shift_composes` | Repeated ctx.shifted vs mathematical shift composition |
| SBS-02 | `pattern_depth_and_binder_shift_commute` | Actual per-slot depth/shift in Receive/Match |
| SBS-03 | `borrowed_environment_key_equals_owned_shift` | Owned Env shifts vs borrowed EnvView representation guard |
| SBS-04 | `identity_order_restores_every_value` | Identity-order families and New result placement |
| SBS-05 | `append_plan_preserves_wire_association` | Multiple unequal Receive bind blocks and Match blocks |
| SBS-06 | `source_first_restores_patterns_then_source` | Source-first Receive jobs reconstructed patterns-first |
| SBS-07 | `case_source_first_restores_pattern_source_guard` | Match source/pattern/guard job and result order |
| SBS-08 | `appended_blocks_preserve_each_wire_association` | Repeated blocks retain indices across differing arities |
| SBS-09 | `restoration_preserves_result_mapping` | Distinct child-result labels retain their associations |
| SBS-10 | `method_rotation_restores_target_and_arguments` | EMethod target/arguments checked after actual combine |
| SBS-11 | `receive_plan_restores_each_bind_and_shifted_tail` | Generated Receive masks, contexts, work order and wire map |
| SBS-12 | `match_case_plan_restores_only_retained_cases` | All eight Match presence masks in both modes |
| SBS-13 | `match_plan_restores_target_and_retained_case_slots` | Target and retained cases retain association |
| SBS-14 | `new_plan_preserves_injections_without_substituting_them` | New body shifts and lexical injections remain Preserve |
| SBS-15 | `accumulated_evaluation_preserves_results_and_first_error` | Child evaluator first-error rule; native recursive oracle |
| SBS-16 | `scheduled_evaluation_equals_recursive_job_evaluation` | Actual work sequence and existing recursive-oracle equivalence |
| SBS-17 | `first_job_error_prevents_suffix_evaluation` | First semantic failure compared with recursive oracle |
| SBS-18 | `incomplete_preservation_keeps_exact_body` | Original protobuf and compact RPB3 images remain exact |
| SBS-19 | `incomplete_preservation_keeps_family_count_and_metadata` | Cross-family injection matrix with count three and root metadata |
| SBS-20 | `incomplete_preservation_has_no_required_child_jobs` | Actual incomplete-preserve dispatch emits a result without child jobs |
| SBS-21 | `incomplete_preservation_uses_admission_nonsemantic_score` | Compact-image comparison with typed key compilation |

The native extension inspects jobs and continuations from actual
`descend_region_component`, then supply distinctly labeled child results to
actual `combine` for successful slots. Incomplete preservation instead requires
an immediate byte-exact result with no scheduled children. Those labels isolate result placement;
they are not claimed to implement recursive substitution. Existing recursive
oracle tests retain authority for terminal behavior and ordered charge traces.
Missing jobs are not manufactured into successful results. All 70 final selected
checks pass; [J-189](pathmap-engineering-journal-2026-08-23.md#j-189--2026-09-18-bind-substitution-schedules-and-preserve-incomplete-injections)
records the capped evidence. Formal identity orders are
explicit index lists; native empty `wire_order` denotes identity. Pure evaluator
theorems do not independently establish side effects or charges. Opaque-preservation
theorems assume admission, faithful shallow classification and the nonsemantic
score; native typed admission/compact-image checks bind those assumptions.

## Dispatch and retained-slot refinement (DSR, task 5916)

This extension of `SubstitutionRegionDecomposition.v::DispatchAndSlots` reuses the
existing field-source certificate; it does not introduce another parser. All thirteen
declarations were kernel-checked before adding native bindings. Context and planned
empty identifiers are opaque. Binder transformation, schema generation, arbitrary
malformed-byte acceptance and final Par construction are not proved here. All native
bindings below pass; [J-188](pathmap-engineering-journal-2026-08-23.md#j-188--2026-09-18-verify-dispatch-actions-and-retained-slot-selection)
records the formal-first and capped native receipts, including their scope limits.

The formal `permits` predicate constrains piece category and retention, not the
child schema identifier stored in a native frame. The native publication oracle
separately checks that identifier. Descriptor resolution, slot assignment and
updates to the supplied `seen` function are native bindings, not extracted Rocq
implementations or consequences of the action-to-piece theorem.

| ID | Closed declaration | Required native observation |
|---|---|---|
| DSR-01 | `unknown_field_borrows_without_singular_check` | Unknown tag borrows without a singular check |
| DSR-02 | `wrong_wire_precedes_slot_and_shape_checks` | Wrong wire precedes duplicate-slot refusal |
| DSR-03 | `duplicate_slot_precedes_shape_check` | Duplicate singular and oneof slot refusal |
| DSR-04 | `accepted_dispatch_has_exact_generated_action` | Generated row/arm selects exact piece, child schema and retention |
| DSR-05 | `recursive_dispatch_cannot_be_replaced_by_borrowing` | Byte-identical Borrowed replacement is rejected |
| DSR-06 | `optional_and_oneof_children_share_presence` | Optional and message-arm children retain empty shells |
| DSR-07 | `nonpar_child_presence_is_exact` | Retention-bit mutant is rejected despite unchanged original bytes |
| DSR-08 | `dispatched_piece_preserves_source_and_action` | Actual publication binds action and original field bytes together |
| DSR-09 | `selection_consumes_exact_prefix` | Generated cursor requests consume exact prefixes with shared region identity |
| DSR-10 | `selection_job_count_includes_synthetic_excludes_dropped` | Missing/Empty emit without consuming; Drop consumes without emitting |
| DSR-11 | `finished_selection_exhausts_cuts` | Exact exhaustion succeeds; under/over-consumption refuses |
| DSR-12 | `reverse_push_executes_children_before_continuation` | Actual work stack pops semantic jobs before continuation |
| DSR-13 | `selection_is_not_a_permutation_of_cuts` | Drop/Empty/Preserve witness is not a permutation of cuts |

DSR-01 is bound by `repeated_unknown_fields_do_not_claim_a_known_singular_slot`;
DSR-02/03 by `dispatch_refusal_precedence_and_shared_oneof_slots_never_return_a_root`;
DSR-04/06/08 by `every_generated_field_and_oneof_arm_has_exact_dispatch_and_presence`
and `verify_node_field_certificates`; DSR-05/07 by
`dispatch_certificate_rejects_byte_identical_borrow_and_retention_mutants`. These
live in `models/tests/support/substitution_region_emission.rs`.

`rholang/tests/support/substitution_region_slots.rs` binds DSR-09/10/12 with
`generated_slot_selection_preserves_prefix_identity_and_semantic_order`, DSR-11
with `region_cut_cursor_rejects_unconsumed_and_exhausted_plans`, and DSR-13 with
`dropped_and_synthetic_slots_are_not_a_permutation_of_retained_cuts`. The mandatory
proof runner executes both native modules, and the manifest checks those commands.

The descriptor-selected wire check precedes singular-slot checking and shape dispatch.
All arms of one oneof share a slot. Unknown fields bypass the singular-slot check.
Child type determines cut-at-Par versus descent. A map entry inherits its parent
path; its value edge uses the outer tag/ordinal despite wire tag 2. Existing provenance
tests retain authority for these path facts.

Cut selection is not a bijection: missing required and optional empty slots synthesize
jobs; incomplete Match cases can discard present cuts. Reconstruction order differs
from semantic evaluation order. This abstract relation does not yet establish the
family-specific result-index map or first semantic error. Prior private child
publications may exist after structural failure; no completed root may be returned.


## Status and scope

This is a living engineering report for the two failures exposed by the exact release-workspace
gate after commit `2df6626d2`. It applies a strict formal-first sequence:

1. model the intended transition system;
2. model-check the control and required negative controls;
3. enumerate every model invariant and liveness obligation;
4. bind each obligation to named executable property families;
5. change executable tests or production code only after steps 1--4 pass.

The two failures have different dispositions. The EPathMap replay failure is a test protocol that
attempted to checkpoint a stale replay history against a shared atomic authority. The state-import
FFI finding is a boundary-hardening obligation: ordinary Rust unwinds must not cross the C ABI,
and an unwind while encoding a result must not expose an affine capability or lie about an
authoritative root-catalog publication. Parser totality is included because the workspace
panic-policy gate found an ordinary
property test using `catch_unwind` as its assertion mechanism.

This report does not weaken the checkpoint authority, which since 2026-11-12 publishes the successor
of a checkpoint's own verified base in one atomic publication, whatever root is current, and whose
generation counts publications; nor does it make panics part of a public API contract, or claim that
`catch_unwind` contains abort-mode panics, native stack overflow, out-of-memory abort, or foreign
faults.

## Formal evidence before executable changes

| Model | Control result | Required negative controls |
|---|---|---|
| `formal/tlaplus/stack_safe_pda/RSpaceRecordReplayAuthorityLifecycle.tla` | SANY pass; TLC 18 generated / 14 distinct states, depth 6; the action properties `ReplayResetLeavesAuthorityUnchanged` and `ReplayPreparationLeavesAuthorityUnchanged` hold on every step; `ReplayLifecycleEventuallyCompletes` passes under weak fairness | skipping the reset and preparing from the current root violates `ReplayPreparationUsesRecordedBase`; a reset that moves the current root violates the action property `ReplayResetLeavesAuthorityUnchanged`; separate witnesses reach a foreign commit while the replayer is reset (`NeverForeignCommitWhileReplayReset`) and while it is prepared (`NeverForeignCommitWhileReplayPrepared`), and a replay commit made with `F` current after a reset made with `T` current (`NeverReplayCommitAcrossForeignCommit`) |
| `formal/tlaplus/stack_safe_pda/RSpaceStateImportFfiBoundary.tla` | SANY pass; TLC 6,586 generated / 4,661 distinct states, depth 20; `EveryStartedCallReturns` passes under phase fairness | early capability exposure violates `CapabilityOutputRequiresEncodedSuccess`; escaped unwind violates `NoOrdinaryUnwindEscapesBoundary`; publication misclassification violates `PublicationResponseIsTruthful`; usable staged-open violates `StageEncodingFailurePoisonsMutatedOpen`; duplicate in-call publication violates `PublicationIsMonotoneAndUnique`; removing fairness violates `EveryStartedCallReturns`; a separate witness reaches two legal sequential publications |

`scripts/check-stack-safe-pda-proofs.sh` runs both controls and requires each mutant to fail for its
assigned invariant or property, and each witness to be violated. A control-only result is not
sufficient evidence.

## Record/replay authority model

### Transition contract

The bounded lifecycle is deliberately narrow and complete:

```text
B@g0 --play prepare/commit--> T@g1
T@g1 --optional foreign commit, at most once--> F@g2
     (before the replay reset, while the replayer is reset, or while it is prepared)
replay reset step: the replay view attaches to the recorded base B; authority unchanged
replay prepare step: B -> T, from the recorded base B; authority unchanged
T@g1 --replay commit of T from base B--> T@g2
F@g2 --replay commit of T from base B--> T@g3
```

Reset is local to the replaying view: it attaches the view to the recorded base `B` and writes
nothing to the shared authority. The replay names its own verified base `B` and publishes its
successor `T` at the current generation plus one, whichever of `T` and `F` is current; it does not
reactivate `B` first and is not refused because the play already made `T` current. A foreign
commit may move the shared authority while the replayer is reset or prepared, so
`ReplayResetLeavesAuthorityUnchanged` and `ReplayPreparationLeavesAuthorityUnchanged` are action
properties of the replayer's own reset and preparation steps (the steps that set its reset and
preparation flags), not comparisons with a snapshot held for the whole phase.

### Exhaustive invariant and property mapping

| ID | Formal invariant/property | Executable property family | Required observation | Status before test correction |
|---|---|---|---|---|
| RR-01 | `TypeInvariant` | `memory_checkpoint_publishes_all_namespaces_together`; `target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure` | roots, generations, object namespaces, and lifecycle states remain typed | PASS |
| RR-02 | `CurrentRootIsVerified` | `memory_checkpoint_publishes_all_namespaces_together`; LMDB success/reopen cases | every selected current root has verified membership | PASS |
| RR-03 | `VerifiedRootsHaveCompleteClosure` | `missing_or_malformed_cold_leaf_leaves_exact_pre_state`; reopen family | verified roots have complete history and typed cold closure | PASS |
| RR-04 | `ContentKeysAreImmutable` | `immutable_collision_leaves_exact_pre_state`; idempotent same-value case | same key cannot acquire a different value; same key/value is idempotent | PASS |
| RR-05 | `NoUnpublishedCheckpointObjects` | `plans_from_one_base_both_publish_without_partial_visibility`; success, collision, and missing-leaf snapshots | failed or partial checkpoint objects are not externally visible | PASS |
| RR-06 | `GenerationAdvancesExactlyOnActivation` | `no_op_writes_nothing_and_does_not_advance_generation`; success, two-plans-from-one-base, replay, and ABA cases | generation advances once per publication (play, foreign commit, replay) and never for reset, preparation, no-op, or refusal | PASS |
| RR-07 | `ReplayResetLeavesAuthorityUnchanged` | `target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure`; `spawn_attaches_the_source_view_root_without_writing_the_shared_authority` | action property: the replay reset step attaches the replay view to the recorded base and leaves current root, generation, activation count, verified-root catalog, history objects, and cold objects exact; a foreign commit while the replayer is reset may still move them | PASS |
| RR-08 | `ReplayCommitPublishesSuccessorOfRecordedBase` | `target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure`; `epathmap_heavy_record_then_replay_reproduces_the_log`; `views_reset_to_different_roots_both_checkpoint` | after its reset, the replay commit publishes the play's target from the recorded base at the current generation plus one, whether the play's target or a foreign root is current, without reactivating the base | PASS |
| RR-09 | `ReplayPreparationUsesRecordedBase` | `target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure`; `a_plan_publishes_the_successor_of_its_own_base_after_root_aba` | replay prepares only after its reset, from the recorded base `B`, not from the current root | PASS |
| RR-10 | `ReplayPreparationLeavesAuthorityUnchanged` | `target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure`; `preparing_checkpoint_leaves_authority_byte_for_byte_unchanged` | action property: the replay preparation step changes no current root, generation, activation count, verified-root catalog, history namespace, or cold namespace; a foreign commit while the replayer is prepared may still change them | PASS |
| RR-11 | `ReplayTargetMatchesPlay` | EPathMap replay root equality | replay publishes the same target root as play | PASS |
| RR-12 | `ReplayReusesAuthenticatedPlayClosure` | `target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure` plus EPathMap event-log exhaustion | the replay adds no history or cold object: after removing only the expected current-root/generation record, replay has byte-identical history, cold, and verified-root namespaces and introduces no unreachable object | PASS |
| RR-13 | `ReplayLifecycleEventuallyCompletes` | EPathMap test reaches replay checkpoint and final equality assertions | valid reset/prepare/replay is live, not merely safe | PASS |

The executable test therefore captures the initial root, checkpoints play, resets the replay view
to the captured root (which writes nothing to the shared authority), rigs and evaluates replay,
checkpoints replay, and compares it with the saved play root. The replay checkpoint publishes the
play's target from the recorded base at the next generation, even though play already made that
target current.

Three reachability witnesses show that the model covers a foreign commit between the replay reset
and the replay commit; each is an invariant that the proof runner requires TLC to report violated.
`NeverForeignCommitWhileReplayReset` and `NeverForeignCommitWhileReplayPrepared` reach a foreign
commit while the replayer is reset and while it is prepared, and
`NeverReplayCommitAcrossForeignCommit` reaches a replay commit made with `F` current after a reset
made with `T` current. The executable counterparts are
`views_reset_to_different_roots_both_checkpoint`, in which a view reset to its base checkpoints the
successor of that base after another view published, and
`plans_from_one_base_both_publish_without_partial_visibility` and
`a_plan_publishes_the_successor_of_its_own_base_after_root_aba`, in which a prepared plan publishes
the successor of its own base after the current root moved.

## State-import FFI boundary model

### Transition contract

Each of the six operations—begin, stage page, seal, publish, abort open, and abort sealed—has three
separate injectable boundary phases: before operation, before primary result encoding, and before
fallback panic-result encoding. Boundary-entry failure preserves the caller's input capability;
an operation-body unwind follows the operation's own consume-or-poison rule. Begin and
seal capabilities remain private until a success result is encoded. A stage operation that mutates
an open session but cannot encode its receipt poisons that session. Root-catalog publication may
become authoritative before result encoding; a later unwind therefore returns null so authoritative
state, rather than an invented response, decides the fact. Here "authoritative" means the
root-catalog effect has occurred; filesystem durability is a separate persistence-backend
obligation. It must never be reported as a definite unpublished failure.

### Exhaustive invariant and property mapping

| ID | Formal invariant/property | Executable property family | Required observation | Strengthened-gate status |
|---|---|---|---|---|
| FFI-01 | `TypeInvariant` | `all_six_operations_satisfy_the_exhaustive_boundary_fault_matrix` | all 18 operation × boundary-fault cells produce only modeled capabilities, outputs, effects, and responses | PASS |
| FFI-02 | `AtMostOneAffineCapability` | 18-cell matrix; `open_page_seal_publish_is_affine_and_returns_typed_receipts` | a capability is private or caller-visible, never both | PASS |
| FFI-03 | `PreparedCapabilityIsPrivate` | begin/seal primary-encoding cells | unencoded open/sealed capabilities are dropped privately and never written to caller output | PASS |
| FFI-04 | `CapabilityOutputRequiresEncodedSuccess` | begin/seal output cells across all three phases | non-null capability output implies a successfully encoded success response | PASS |
| FFI-05 | `FailureLeavesNoNewCapabilityOutput` | all 18 cells; `primary_and_fallback_encoding_failure_returns_null_without_consuming_operation` | error, panic response, or null creates no new caller capability | PASS |
| FFI-06 | `NoOrdinaryUnwindEscapesBoundary` | all 18 cells | every injected ordinary unwind returns a framed result or null and never crosses the ABI | PASS for the modeled ordinary-unwind domain; abort-mode panic, native stack overflow, OOM abort, and foreign faults remain explicitly outside this model |
| FFI-07 | `PublicationIsMonotoneAndUnique` | `two_sequential_publications_are_distinct_and_each_exactly_once`; publish cells | two independent imports may publish, but each affine publish call crosses authority at most once | PASS |
| FFI-08 | `PublicationResponseIsTruthful` | publish primary-encoding and post-effect-panic properties | authoritative publication yields success or null, never an unpublished panic error | PASS |
| FFI-09 | `PrePublicationPanicIsStructured` | `pre_publication_operation_panic_is_structured_and_does_not_invent_publication` | pre-effect panic is structured and cannot invent an authority effect | PASS |
| FFI-10 | `PublishSuccessRequiresPublication` | affine success and sequential-publication properties | every publish success corresponds to exactly one observed authoritative root-catalog effect | PASS |
| FFI-11 | `StageEncodingFailurePoisonsMutatedOpen` | stage primary-encoding cell; named focused property | one unacknowledged staged mutation makes the open session unusable | PASS |
| FFI-12 | `AbortConsumesWithoutPublishing` | abort-open/abort-sealed cells with drop counters | after boundary entry, abort consumes exactly once and never publishes; a pre-operation boundary failure preserves the input | PASS |
| FFI-13 | `EveryStartedCallReturns` | all 18 finite fault cells; no-fairness formal mutant | every started ordinary-unwind call reaches framed completion or null | PASS |

The original five red properties below authorized the first production repair. The strengthened
model then exposed the vacuous global-publication bound and the distinction between a boundary-entry
fault and an operation-body unwind. Its 4,661-state control, duplicate-publication mutant,
no-fairness mutant, and two-publication reachability witness passed before the 18-cell executable
matrix or related implementation terminology was added.

### Captured implementation red control

Before any production repair, the capped release package run
`cargo nextest run --release --no-fail-fast -p rspace_plus_plus_rhotypes` executed all 18 tests:
13 passed and exactly five model-derived properties failed. The failing witnesses were:

- `begin_primary_encoding_failure_exposes_no_open_capability` (FFI-03/04);
- `seal_primary_encoding_failure_exposes_no_sealed_capability` (FFI-02/03/04);
- `stage_primary_encoding_failure_poisons_the_mutated_open_session` (FFI-11);
- `publish_primary_encoding_failure_returns_null_after_authority_effect` (FFI-07/08);
- `post_publication_operation_panic_returns_null_not_unpublished_error` (FFI-08/09).

The pre-publication panic control, primary-plus-fallback encoding failure control, affine success,
ordinary validation failure, abort-open, and abort-sealed controls all passed. This
is the immutable before-repair baseline: the production change is permitted only to turn these
five named witnesses green without regressing the thirteen controls.

### Strengthened post-repair evidence

The first audit after repair rejected three overclaims in this catalog and one vacuous formal
bound. The repaired FFI control now permits the minimum six calls needed for two complete
begin→seal→publish cycles, separates total publications from the per-call 0-or-1 effect, separates
boundary-entry faults from operation-body faults, and proves fair completion. TLC explores 6,586
generated / 4,661 distinct states at depth 20. Five safety mutants fail their named invariants, the
no-fairness mutant fails `EveryStartedCallReturns`, and `NeverTwoPublications` fails on a legal
second publication. The focused executable run passes 11/11, including all 18 finite fault cells,
two distinct exactly-once publications, exact replay object-closure equality, and the mechanical
26-row formal-to-property registry.

The enclosing release gates are also green. The complete rhotypes plus Casper LFS boundary gate
passes 78/78, and the exact command
`RUST_BACKTRACE=1 cargo nextest run --release --no-fail-fast --workspace` passes 5,122/5,122 in
279.181 seconds, with 53 tests intentionally skipped. Both ran in `systemd-run` scopes with hard
resident-memory limits and swap disabled.

## Arbitrary-wire parser totality

The aggregate property `arbitrary_checkpoint_wire_bytes_are_total` remains bound to WAL-05 and
RES-01. It must invoke each parser directly: any panic already fails a proptest case, so wrapping
the calls in `catch_unwind` weakens diagnostics and violates repository panic-test policy. Separate
properties cover these grammars so one parser cannot hide behind the aggregate:

| ID | Parser/property family | Domain |
|---|---|---|
| PT-01 | checkpoint descriptor — `arbitrary_checkpoint_descriptor_bytes_are_total` | arbitrary 0--1,023-byte input |
| PT-02 | generation PREPARE preflight — `arbitrary_generation_prepare_preflight_bytes_are_total` | arbitrary 0--1,023-byte input |
| PT-03 | generation PREPARE view — `arbitrary_generation_prepare_bytes_are_total` | arbitrary 0--1,023-byte input |
| PT-04 | generation COMMIT — `arbitrary_generation_commit_bytes_are_total` | arbitrary 0--1,023-byte input |
| PT-05 | object index — `arbitrary_object_index_bytes_are_total` | arbitrary 0--1,023-byte input |
| PT-06 | pack header — `arbitrary_pack_header_bytes_are_total` | arbitrary 0--1,023-byte input |
| PT-07 | retention manifest — `arbitrary_retention_manifest_bytes_are_total` | arbitrary 0--1,023-byte input |
| PT-08 | two-slot catalog selection — `arbitrary_exact_catalog_images_are_total` | every arbitrary exact 672-byte catalog image |
| PT-09 | aggregate WAL-05/RES-01 compatibility family — `arbitrary_checkpoint_wire_bytes_are_total` | all eight calls in one generated case |

Every parser is total over bytes by returning success or a typed error. None uses panicking as an
accepted outcome.

## Final-root unary-EList fixture refinement — 2026-09-17, task 6444

**FORMAL-FIRST; NATIVE MIGRATION AND FINITE RESOURCE LADDER QUALIFIED.** The old shared fixture constructs a resident
parent at every level, copying every suffix's protobuf body and canonical score. The existing
`Par::wrap_unary_elist` prepares length frames and emits only the final authoritative endpoint.
`UnaryEListConstruction.v` now compares these two concrete byte computations. It is not an
extracted Rust program or a proof of arbitrary terminal serialization.

For a child Par body of length $`p`$, a frame has list length
$`1+v(p)+p`$, expression length $`2+v(l)+l`$, and enclosing Par length $`1+v(e)+e`$,
where $`v`$ is canonical varint length and $`l,e`$ are the preceding lengths. Its tags are
`0a`, `a2 01`, and `2a`. Inner-to-outer planning followed by outer-to-inner header emission copies
the terminal body once into the outer expression; the final key copies that expression once.
The canonical score uses EList constructor 6, Par constructor 999, remainder -1 and exact closing
flags. At positive depth $`d`$, its length is terminal component-score length plus $`54d`$.

The model deliberately retains the current initial reservation formula $`44d+64`$, not a fictitious
exact allocation. Native vectors may grow. Its checked constructor composes frame-length and
reservation checks: all summands are nonnegative, so a representable final sum also bounds each
intermediate addition/multiplication. Arithmetic refusal produces no endpoint. Successful
allocation and representable native lengths are explicit correspondence conditions; the model does
not claim graceful allocation failure, because the native frame vector is allocated before these
checks. The trailer theorem separately requires a score length representable in `u64`.

The emitted-byte bound assumes a fixed bound on the three frame varints. A separate explicit cost
model counts frame/score-loop visits and two body copies; it excludes terminal preparation,
allocator behavior and PathMap insertion. This is not a measured wall-time bound or a proof of all
Rust execution costs. Native differential and geometric phase measurements remain mandatory.

| ID | Extracted formal obligation | Native binding required before fixture replacement |
|---|---|---|
| UEL-01 | Zero depth preserves terminal bytes and all root metadata. | `bulk_unary_fixture_matches_frozen_resident_oracle` and `bulk_unary_fixture_preserves_arbitrary_terminal_metadata` include depth zero and vary metadata/multiplicity. |
| UEL-02 | Positive depth has one Expr occurrence and empty/false outer metadata. | Both bounded differentials check root cardinality, family count and metadata. |
| UEL-03 | Each planned length equals independent protobuf framing; emitted Expr omits only the outer Par header. | Exact generated protobuf and bincode comparisons with frozen repeated wrapping; framing-boundary depths. |
| UEL-04 | Planned emission equals repeated resident wrapping, with exact frame count. | Exact depth/terminal recognizer; unchanged depth-20,000 gate; no suffix decoding in the witness. |
| UEL-05 | Score prefix/suffix loops equal recursive score serialization; exact positive-depth length is $`s+54d`$. | Component key/snapshot equality with the frozen oracle; scalar extremes, metadata-bearing terminal and varint boundaries. |
| UEL-06 | RPB3 family/domain, body and score trailer form the exact same endpoint. | Compact snapshot plus Eq/Ord/Hash comparison and a concrete `GInt(0)` body golden. |
| UEL-07 | Successful checked planning refines mathematical planning; frame or reservation refusal cannot publish. | Formal positive witness and overflow controls; existing native checked arithmetic is inspected, not tested by attempting unbounded allocations. |
| UEL-08 | Fixed-width varint premise yields linear emitted bytes and modeled loop/copy cost. | Separate construction, operation and teardown measurements at unchanged geometric depths; allocator costs reported separately. |
| UEL-09 | Representable eight-byte score-length trailer round-trips. | Exact endpoint/snapshot differential; native allocated-score length must fit `u64`. |

All public capstones are covered by these nine rows; helper lemmas establish frame lengths,
prefix concatenation and score lengths. Causal controls change a tag, child length, leaf occurrence,
remainder/flag/close, trailer, depth or zero-depth metadata. Composed frame and reservation overflow
controls require refusal, and a successful positive construction prevents vacuity. These controls
do not replace executable correspondence.

The adjacent wide-fixture change uses existing checked `insert_component_many`, not a different
semantics. Its bounded frozen-vector oracle must preserve exact bytes, multiplicity, distinct count,
other families and root metadata. Overflow must leave both components and metadata unchanged.
Nested EPathMap construction is a subsequent slice using existing planned arena handles; it is not
covered by the unary-EList theorem.

The `gint_zero_qualified_endpoint` theorem specializes the symbolic construction model; the native
`[42, 2, 16, 0]` protobuf golden and frozen-oracle comparisons supply the separate terminal binding.

The 2026-09-17 qualification compiled both theories, checked 22 main and 14 control assumption
reports, and independently kernel-checked both modules. Before changing either fixture, all four
new native witnesses and all 25 formal-manifest tests passed. After migration, all 49 tests passed
across `match_path_structure`, `match_path_cursor`, `match_schema_cursor`,
`bincode_decoder_differential` and `protobuf_decoder_differential`. The 20,000-level depths and
small-stack assertions are unchanged. See engineering journal J-169 for commands and log hashes.
At that checkpoint UEL-08's measurements and the nested-EPathMap slice remained open.
The subsequent SMC qualification below covers the nested fixture. Experiment 345 now
supplies phase-separated constructor measurements through depth 4096 and operation
measurements through 1024 with explicit deadline/not-run outcomes; see J-174 and the
[phase report](../stack-safety/measurements/final-root-fixture-phase-attribution-2026-09-17.md).
These finite observations do not prove every Rust operation linear or close the workspace gate.

The native bridge additionally covers empty-Par terminals for both encoder-space fixtures and
`planned_escaped_fixture_preserves_outer_set_and_exact_representation` for the EPM1 escaped fixture.
The outer ESet constructor is unchanged; composition uses the exact unary byte equivalence, with
a bounded frozen witness checking the enclosing EPathMap snapshot as well. Both prerequisite
witnesses passed before replacement; all 16 encoder-space tests and 17 EPM1 tests then passed.
The distinct nested-map term-operation gate was excluded from that invocation; the later SMC
qualification below includes and passes it.

The replay/output-value terminal correspondence is additionally bound by
`final_root_output_value_fixture_matches_frozen_resident_wrapping`, which passed before the two
Rholang fixture helpers changed. Their three complete release test targets then passed all seven
tests with no exclusions, including the depth-4,096 replay case. This extends UEL-01–06/09 to those
consumers; resource evidence is supplied separately by experiment 345, not inferred from
test duration. J-169 records commands and log hashes.

## Fixed-key singleton-map construction — 2026-09-17, task 6444

**FORMAL-FIRST; NATIVE BRIDGE, MIGRATION AND FINITE RESOURCE LADDER QUALIFIED.** `SingletonMapConstruction.v` has compiled and
passed independent kernel checking with 18 closed assumption reports. It models the fixed-key
singleton-map fixture, not arbitrary ACT encoding or every arena constructor. A supplied common
prefix contains the EPM1 magic/version/mode, PathMap-produced topology and one-value count. Each
snapshot appends the canonical varint body length and child Par bytes. Wrapping uses protobuf field
9 (`4a`), Expr field 32 (`82 02`) and Par field 5 (`2a`). The native topology-prefix equality is a
separate mandatory correspondence obligation, supported by the maintained V6 positional-layout
authority and checked against the frozen resident constructor before migration.

| ID | Formal obligation | Native binding and limitation |
|---|---|---|
| SMC-01 | Four frame lengths and framing tags are exact. | Exact snapshot/protobuf/bincode comparisons at depth one and framing boundaries. |
| SMC-02 | Planned final emission equals repeated resident wrapping; one frame per inner level. | Frozen builder differential with unchanged key, terminal and depth. Frame count is not total arena-record/allocation count. |
| SMC-03 | One outer map encloses exactly the requested inner depth; depth one retains the terminal. | Explicit Map mode, singleton key/value, outer snapshot and terminal/depth controls. Empty/false/None metadata stays unchanged. |
| SMC-04 | Snapshot zero-count summaries and escaped-score lengths are exact. | Exact carrier component snapshot and canonical-key comparison against resident serialization, including zero-bearing leaves. |
| SMC-05 | Final score and endpoint equal maintained canonical score serialization. | Bounded carrier Eq/Ord/Hash, protobuf/bincode and complete RPB3 snapshot differential. The fixture itself returns EPathMap, so direct snapshot equality is also mandatory. |
| SMC-06 | Checked frame success preserves lengths; composed checked-plan success refines the mathematical plan; refusal cannot publish. | Inspect native checked sums and keep finite model refusal/positive controls; no enormous allocations as an overflow test. Allocation success and addressable sizes are explicit premises. |
| SMC-07 | Successful endpoint reservation uses exact capacity and a representable score trailer. | Native capacity is $`39+s+z+e`$, with snapshot length $`s`$, zero count $`z`$ and expression length $`e`$. Symbolic score bound specializes to $`2^{64}`$; native conversion follows private allocation/emission, so the model does not claim identical internal refusal cost/order. |
| SMC-08 | Fixed-size prefix and bounded varints imply linear emitted-byte length. | Experiment 345 measures final-root construction through depth 4096, separately from decoding/cursors/teardown. This is an output-size bound plus finite native observations, not proof of all runtime or allocator costs. |

The small `[7, 0, 9]` prefix used by model controls is deliberately abstract, not a canonical EPM1
example. Actual writer-prefix equality must be checked natively. A failed proof-development run
expanded the numeric trailer bound and reached its 180-second capped watchdog; the successful
model keeps that bound symbolic and uses a finite bound only for executable controls. No theorem
was weakened to a bounded-depth statement.

Before migration, `planned_map_fixture_matches_frozen_resident_snapshots_and_keys`,
`planned_map_fixture_preserves_arbitrary_leaf` and
`planned_map_fixture_controls_reject_wrong_depth_key_and_value` all passed, along with 26 formal
manifest checks. They bind SMC-01–05 to the native writer, compare canonical paths and topology
prefixes through each bounded level, and reject missing depth, changed key/leaf and swapped
key/value. The helper then switched to integer arena handles, one inner finish, and the unchanged
outer `EPathMap::new_map`. The complete EPM1 snapshot suite passed 21 tests with none excluded,
including depth-4,096 Eq/Ord/Hash and snapshot/codec gates on 256 KiB stacks. See J-170 for evidence.
The named fixture consumers subsequently passed their migrations as recorded above.
SMC-08's seven-size constructor ladder has measured time/byte exponents 1.0139/1.0049;
the initial 512/1024 operation ladder is fully accounted for in J-174, including censored
and explicitly unrun cells. Separate map validation and decoded-descent costs remain
required production work under 5834. Task 6444 still requires evidence acceptance.


## Split-carrier header prefilter — 2026-09-17, tasks 6444 and 5834

**FORMAL MODEL, NATIVE SUCCESSOR AND TARGETED RESOURCE COMPARISON QUALIFIED.** Profiling the escaped
nested-list fixture identified full expression decoding in `split_carrier_list` just to reject
a non-list expression. The implemented prefilter reads the resident Expr endpoint's borrowed
protobuf field header. Only canonical resident oneof bytes are eligible for this shortcut:
the first field of arbitrary input protobuf is not an authority because a later oneof field
can override it. No new untrusted-input admission path is introduced.

`SplitCarrierPrefilter.v` proves equivalence conditional on a sound negative hint. Its
`canonical_hint` is a semantic abstraction, not an implementation of the byte cursor.
The field-20 header theorem establishes the constant `a2 01`, not a complete schema-to-parser
refinement. Those native bridge obligations are independently tested below. Ordered opaque
child identifiers model exact content, order and duplicates; they are not a standalone proof
of Rust Par semantics. The abstract event trace models entry into semantic decoding, not
allocation counts or actual execution time.

| ID | Model invariant and theorem | Required executable binding |
|---|---|---|
| SCP-01 | `outer_false`, `outer_guards_not_bypassed`: other families, arity and root metadata retain their guards. | `split_prefilter_preserves_all_outer_guards_and_multiplicity`; all eight other families, zero/two Expr occurrences and root metadata. |
| SCP-02 | `list_header`, `header_list`, `header_nonlist`, `canonical_hint_sound`: a negative hint excludes a list. | `resident_expr_headers_match_every_generated_variant`; every generated Expr arm, absent Expr and default scalar oneofs, field number versus schema protobuf tag (not serde index). |
| SCP-03 | `reference_success_is_list`, `conclusive_rejection_sound`, `result_equivalence`, `canonical_result_equivalence`: classification is unchanged. | Frozen prefilter-free classifier versus successor; deterministic controls and 256 generated cases. |
| SCP-04 | `observations_equivalence`, `returned_children_are_exact`: any observation of the returned list is unchanged. | Exact EList protobuf bytes and complete canonical path/stability differential; ordered duplicates and unstable children. The path oracle retains production segment encoding and independently freezes top-level selection. |
| SCP-05 | `list_hint_retains_metadata_checks`: a list hint does not bypass list metadata. | `split_prefilter_retains_list_metadata_and_ordered_duplicate_children`; remainder, locally-free bytes, connective flag and empty list. |
| SCP-06 | `inconclusive_preserves_reference`: unknown hints preserve the old path. | Malformed-framing controls directly call the production private hint helper and require fallthrough. Field 20 with an unexpected wire type also falls through. Malformed bytes are not imported as resident Par values. |
| SCP-07 | `rejection_has_no_decode`: conclusive rejection has no semantic decode event. | Thread-local counter immediately before decoding; baseline `Par::first::<Expr>` and successor `entry.decode()` share `decode_exact::<Expr>`. Baseline non-list count one, successor count zero; genuine list count one in both. This does not assert zero zipper allocations. |
| SCP-08 | `wire_header_controls`, `outer_guard_mutant_controls`, `list_metadata_mutant_controls`, `hint_mutant_controls`, `decode_order_mutant_control`: controls distinguish changed tags, guards, metadata, hints and decode ordering. | Positive list witnesses, negative shape/metadata witnesses, malformed framing and exact decode counts; checked proof manifest. |

The runner requires all 20 assumption reports to be closed and independently checks the module
with the Rocq kernel. The model does not prove Rust compilation, schema correspondence, allocator
complexity or full campaign completion. Native pre-repair witnesses must pass before the runtime
prefilter changes; successor witnesses and resource measurements remain mandatory afterward.
The separate legacy decoder construction and map-value validation costs remain open under 5834.

Before changing runtime classification, the five native classifier tests, exhaustive generated
variant test and manifest passed under a 6 GiB, zero-swap cap. The baseline counter observed one
semantic decode for the depth-64 non-list; the genuine list also observed one. Retained log
`target/verification/split-prefilter-baseline-native-r1.log` has SHA-256
`2e1f89672b6948d39bbb8ee6bd5b585b9c79671498a202e71802ce362e1add97`.
The final 20-report proof and kernel-check log is `split-prefilter-6444-proof-r3.log`, SHA-256
`9142003af0ddfc667b3be918398301beda2b58ef86b659f73c468d6f1ff51d73`.
The successor must observe zero decodes for that non-list, still one for the genuine list, and
retain all semantic witnesses. The manifest also binds the test module inclusion and counter
placement so disconnected test source is not sufficient evidence.

The successor uses a single encoded-endpoint visitor for header inspection and any required
decode, rather than walking the same trie key a second time. The unit-cardinality guard plus
positive multiplicities ensures that the callback sees exactly one endpoint. Its decoded Expr
is owned; payload borrowing ends with the callback. Both this visitor's `entry.decode()` and
the frozen classifier's `Par::first::<Expr>` call the same `decode_exact::<Expr>` implementation,
and both map a decoding error to no split. This is a source-reviewed native bridge, not an
additional theorem about the Rust decoder.

The final single-visitor release run passes 35 canonical-path tests (one opt-in timing probe
ignored), all 27 manifest tests, all seven schema-cursor tests including depth 20,000, and all
25 EPM1 snapshot/meter tests (one opt-in phase probe ignored). No correctness test is excluded;
the original depth-4,096 cases remain. The EPM1 target takes 271.52 seconds under the 6 GiB,
zero-swap cap. Build/test overlap means this elapsed time is qualification evidence, not a
benchmark. The log `target/verification/split-prefilter-single-visitor-native-r1.log` has SHA-256
`3bfab381fc3abe3b3a185fdcd16fd0a6293aaa04208a5411cf502c956428eacc`.

Experiment 345 artifact 681 adds the successor resource comparison: the non-list classification
repair removes the targeted escaped outer-construction slope through depth 4,096. The genuine
empty-list case retains an observed 15.48 ns overhead, while legacy reconstruction and map
validation retain their independent slopes. See J-173 for scope and raw evidence; this focused
qualification does not close tasks 6444/5834 or the campaign.

## Substitution immutable-region foundation (SRR, task 5916)

`SubstitutionRegionRanges.v` models a region by an abstract backing-owner identity
and half-open absolute bounds. A child retains that owner and adds relative offsets;
a successful range check must stay inside the parent and the machine-word limit.
Cut registration appends one region and returns its ordinal, including empty regions.
An EPathMap value group selects a contiguous, ordered range of those ordinals.

The following inventory covers all 18 printed proof obligations. Native witness names
refer to `models/tests/substitution_region_ranges.rs`; the mechanical manifest checks
their registration, theorem declarations, runner count and kernel-check command.

| ID | Formal obligations | Native witness and scope |
|---|---|---|
| SRR-01 | `subregion_success_exact`, `subregion_complete`, `successful_child_is_valid` | `checked_regions_match_native_slice_and_preserve_backing`: arbitrary backing bytes and bounds compared with Rust slice admission. |
| SRR-02 | `successful_child_retains_owner` | Same property checks pointer offsets and retained bytes after releasing the root handle; cut tests also release the root before reading retained cuts. |
| SRR-03 | `reversed_range_refused`, `beyond_parent_refused`, `absolute_end_overflow_refused` | `impossible_and_reversed_ranges_refuse_without_mutation` and SRR-01 compare refusal and unchanged input. Actual absolute-add overflow is unreachable through valid public regions; its arithmetic obligation is formal-only. |
| SRR-04 | `empty_at_end_is_valid`, `nested_child_composes`, `nested_local_bounds_compose` | SRR-01 compares nested and direct slices, absolute ranges, pointers and terminal empty regions. |
| SRR-05 | `registered_cut_has_exact_ordinal`, `registration_preserves_previous_ordinals` | `ordered_cut_ordinals_preserve_empty_and_duplicate_children`: optional integer children include empty Pars and duplicates; every cut is compared with independent child encoding and its schema-path occurrence. |
| SRR-06 | `value_group_has_exact_order`, `selected_group_preserves_owner` | `epathmap_groups_preserve_value_order_with_surrounding_cuts`: neutral, set and two map groups follow two Send cuts and precede a Bundle cut. A typed trie zipper supplies expected value bytes independently of cut grouping. One map value is empty. |
| SRR-07 | `wrong_owner_is_observable`, `absolute_offset_mutant_is_observable` | Formal observability examples; SRR-01's pointer/range assertions check the corresponding native effects. These are not complete cutter mutation proofs. |
| SRR-08 | `dropped_empty_cut_is_observable`, `swapped_value_ordinals_are_observable` | Formal observability examples; SRR-05/06 check native empty-cut retention and ordered distinct values. |

**Boundary.** The limit denotes inclusive `usize::MAX`, not the number of possible
machine-word values. The parent-validity premise is essential: natural-number
subtraction alone does not describe an invalid Rust range. Nested composition is an
algebraic equality; successful checked composition additionally uses the validity and
completeness obligations. The abstract owner does not prove `Bytes` reference-counting,
lifetime implementation or allocation behavior. Group-order equivalence requires the
specified valid prefix/value/suffix decomposition; the model's truncated list selection
does not model Rust's panic on an invalid indexing range.

Path metadata, occurrence overflow, output-length arithmetic, the three shell/reassembly
emit modes, EPathMap key/value association, binder shifts, early errors, charge order and
full substitution equivalence are not proved by this foundation. Native checks provide
bounded executable evidence for some of these boundaries, not a promoted formal claim.
Task 5916 remains open for composition/refinement, operational counters, unchanged
high-rung gates, scaling confidence bounds, profiles and lifecycle evidence.

**Qualification (2026-09-17).** Rocq reports 18 closed obligations and explicit `coqchk`
success. The focused release run passes 56 tests: 28 manifest, 14 cursor, eight region
differentials (including depth 4096), four new range/group tests and two test-registration
checks, with none ignored or filtered. See J-175 for capped-run receipts and hashes.

## Conditional substitution-region emission (SRE, task 5916)

`SubstitutionRegionEmission.v` models the existing indexed-node and piece worklist,
not a new serializer. `Original` emits complete cut bodies; `Shallow` emits empty Par
placeholders; `Compact` omits Par fields and only elides a child envelope if cutting
emptied a previously nonempty, non-presence-sensitive child. Both shell modes preserve
the EPathMap prefix and emit one zero-length value frame per retained map-value ordinal.
The framing equation uses `EPM1.encode_varint` for the concrete protobuf key
$`8t+2`$ and payload length, where $`t`$ is a valid field tag.

The model separately computes planned lengths with inclusive machine-word and `u64`
limits. A successful plan has exact per-mode lengths; a failed mode cannot publish
the new length record. The ordered worklist invariant equates emitted bytes followed
by pending bytes before and after every transition. Its termination theorem uses a
decreasing natural-number work measure, without an execution fuel argument.

| ID | Printed obligations (35 total) | Native evidence |
|---|---|---|
| SRE-01 | `emit_step_preserves_ordered_pending_bytes`, `execution_preserves_ordered_pending_bytes`, `worklist_emission_equals_mode_reference`, `reverse_push_visits_pieces_in_source_order` | `all_emission_modes_match_independent_typed_map_shells`, exact region reconstruction corpus and distinct ordered child witnesses. |
| SRE-02 | `every_emit_step_decreases_rank`, `ranked_emission_terminates_without_fuel` | Existing depth-4096 region walk on a 256 KiB stack; this is bounded native evidence, not a universal runtime complexity theorem. |
| SRE-03 | `compact_preserves_required_empty_envelopes`, `compact_retains_genuinely_empty_repeated_messages`, `compact_omits_only_cut_emptied_unrequired_envelopes`, `original_par_keeps_present_empty_header` | Typed-map-shell property guarantees absent/present fields and genuine empty entries; existing compact-shell and present-empty regression tests remain bound. |
| SRE-04 | `epm_shallow_and_compact_preserve_value_count`, `epm_original_value_order` | `epathmap_shell_modes_preserve_keys_and_every_empty_value_slot` covers neutral/set/map storage and three map values, one empty; SRR's surrounding-cut witness checks group ranges. |
| SRE-05 | `checked_add_success_exact`, `checked_add_refuses_overflow`, `checked_total_success_exact`, `checked_total_complete`, `checked_total_refuses_unrepresentable_nonempty` | `checked_add_matches_machine_word_arithmetic` and fixed `usize::MAX` boundaries; the list-total theorem is supporting mathematics, not a separately called Rust routine. |
| SRE-06 | `checked_delimited_success_exact`, `checked_delimited_refuses_u64_overflow`, `checked_frames_success_exact` | `delimited_length_matches_widened_arithmetic`; `all_modes_preserve_unknown_bytes_across_varint_boundaries` uses 30 tag/length combinations through the maximum valid tag and payload length 16384. The separate `u64` overflow branch is formal-only on this 64-bit host. |
| SRE-07 | `planned_piece_success_has_exact_mode_length`, `finished_node_success_has_exact_mode_length`, `finished_node_success_is_representable`, `failed_mode_does_not_publish` | Exact three-mode byte/length comparisons and `overflow_in_each_cached_mode_refuses_finished_node_publication`; the latter supplies synthetic lengths without allocating enormous buffers. |
| SRE-08 | `dropping_genuine_empty_envelope_is_observable`, `ignoring_presence_flag_is_observable`, `reversed_value_order_is_observable`, `missing_empty_value_is_observable`, `unchecked_wrap_is_observable`, `reversed_neighbor_pieces_are_observable`, `stale_child_cache_is_observable` | Concrete formal negative witnesses; native presence, ordering, empty-value and arithmetic assertions detect the corresponding effects. These are not generated mutations of the complete Rust implementation. |
| SRE-09 | `fixture_reference_is_sound`, `fixture_ranks_are_valid`, `fixture_emits_all_modes_without_fuel`, `fixture_planner_accepts_all_modes` | Concrete formal nonvacuity arena contains borrowed bytes, a nested child, present-empty Par and two framed values. Its opaque prefix is an emission fixture, not a proved valid ACT/EPM1 snapshot. Native EPathMap tests use actual admitted trie snapshots. |

**Premises and remaining bridge.** The reference satisfies the arena's piece equations;
child caches agree with that reference; the supplied work measure decreases on expansion.
A concrete finite arena satisfies these premises, but the proof that the Rust builder
establishes them for every accepted input remains required. Total lookup functions do
not themselves establish index validity, contiguous group membership, valid region
provenance or physical byte/tag ranges. Those belong to the postorder builder and SRR
composition proof. Rank existence is not inferred merely from tests, and termination
does not establish a linear allocation or runtime bound. A DAG could revisit a subtree.

The model plans one entire mode at a time; Rust checks the three modes per piece.
The current result covers successful lengths and all-or-nothing refusal, not the first
failing arithmetic operation. All modeled arithmetic failures map to `SizeOverflow`.
The emitter theorem's reference lengths correspond to Rust caches only under the cache
premise. Most importantly, internal reference consistency and equal reconstructed lengths
do not prove equality with the original input. A canonical decomposition certificate is
still needed before claiming full cut/reassembly correctness or substitution equivalence.

All 35 declarations are bound to the compile count and explicit kernel check in the
manifest; native witnesses live under `models/tests/`. See J-176 for qualification.

The unknown-field test proves unchanged borrowed fields across its named tag/payload
sizes; it does not prove regenerated key-width transitions. The separate deterministic
`delimited_length_covers_key_and_payload_width_transitions` adds 80 arithmetic cases,
including tags 15/16, 2047/2048, 262143/262144 and 33554431/33554432, where the encoded
key actually changes width. It uses the same widened-arithmetic oracle as SRE-06 and
allocates no payload buffers for its near-`usize::MAX` cases.

## Checked region publication and constructed witnesses (SRP, task 5916)

`SubstitutionRegionPublication.v` constructs SRE's reference, cache and rank witnesses
through finite append-only publication. The model represents the arena by a count and
indexed tables; unpublished slots have empty programs, whose invariant equations force
empty views, zero cached lengths and unit node rank. A new node's view and rank are
computed from earlier children; its caches come from the checked planner. Strict backward
references keep existing views and ranks unchanged when the next index is filled.
Source bytes and computed views are ghost specification data, not new runtime storage.

The source certificate describes individual fields: an unchanged borrowed field; a
canonical header and retained Par payload; a header and an earlier child's recorded
payload; or an EPathMap header, prefix and ordered value frames. A constructed publication
also requires that the supplied actual frame bytes equal the concatenation of those
field bytes. This is a local decomposition premise, not the conclusion that emitted
bytes equal input. The theorem derives that conclusion for the certified source.

| ID | Printed obligations (21 total) | Native binding |
|---|---|---|
| SRP-01 | `empty_is_valid`, `publication_preserves_old_indices`, `publication_gives_exact_new_index`, `prior_piece_denotation_stable`, `publication_increases_count_once` | `every_published_node_preserves_source_caches_indices_and_prior_meaning`: captures contiguous publication indices and outputs at publication; compares them after later nodes are appended and after truncating the node arena back to the earlier prefix. |
| SRP-02 | `append_establishes_all_arena_invariants`, `successful_checked_publication_establishes_invariants`, `constructed_arena_is_valid` | Same witness checks backward child references, existing cut/group indices, bounded group intervals, earlier path parents, and exact per-mode caches. Cut/group/path validity is native evidence here, not a formal consequence of the fixed external tables. |
| SRP-03 | `certified_fields_reconstruct_original`, `valid_published_root_emits_its_original_input`, `constructed_published_root_reconstructs_certified_source` | Captured `Frame.region` bytes, not bytes reconstructed from pieces, are the oracle for each node's Original output. `generated_publications_preserve_actual_frame_bytes` adds property-generated EMap inputs. |
| SRP-04 | `valid_arena_supplies_node_rank`, `valid_arena_supplies_piece_rank`, `constructed_root_emits_all_modes_with_exact_cached_length` | The native witness constructs a finite work count from already-published child counts and checks all three output lengths. Formal termination instantiates SRE with constructed ranks; no arena-size linearity claim follows. |
| SRP-05 | `publication_checks_original_length`, `length_mismatch_refuses_publication`, `arithmetic_failure_refuses_publication` | Existing direct arithmetic-refusal tests remain bound. `late_framing_failure_never_publishes_the_root` observes an already-completed child followed by an error and no completed root; it does not claim rollback of earlier internal child publications. Original-length mismatch remains a formal defensive-branch obligation. |
| SRP-06 | `two_node_publication_is_constructed`, `same_length_wrong_source_passes_only_the_length_check`, `self_reference_is_not_an_admissible_publication`, `unrepresentable_publication_is_refused` | Formal success/nonvacuity and negative witnesses. Wrong same-length source demonstrates that the numeric guard alone cannot prove source equality; the backward-reference premise excludes a self-reference that numeric planning alone need not reject. |

**Actual-code observation.** The `cfg(test)` observer runs immediately after
`builder.nodes.push(node)`, receiving the still-retained `frame.region` and the current
builder. Only when a test explicitly enables capture does it retain source regions and
emit the newly completed node in all modes. The test checks later output against these
earlier outputs and independently checks Original output against the captured region.
Every captured source, cut, borrowed field and EPathMap prefix is checked against the
original backing slice and pointer. The existing shared exhaustive corpus is reused
through a test-only crate alias. These capture allocations are test instrumentation,
excluded from production and performance evidence.

**Remaining boundary.** Construction is conditional on the field certificate and actual
frame partition. The universal proof that the canonical cursor and EPM1 inspector supply
that certificate—including gap-free source ranges, finite cut/group membership, physical
tag/byte validity and provenance—remains required. The fixed external tables in this
module do not establish those facts. Native corpus/properties give direct bounded
evidence, not a proof over all accepted inputs. Parser errors, allocation failure and
the exact order of arithmetic refusals are not modeled by this publication theorem.
Full substitution semantics, work counters and scaling/lifecycle acceptance remain open.

## Canonical framing and ordered source partitions (SRF, task 5916)

`SubstitutionRegionFraming.v` reuses EPM1's canonical varint/frame definitions.
It proves the previously missing converse: successful decoding determines the
exact consumed encoding, rather than only proving that encoded inputs decode.
The ten-group scanner model adds physical-byte, final-group and minimal-terminal
guards, then derives that its successful results refine the existing canonical
decoder. No input-is-already-canonical premise is used for that direction.

| ID | Printed obligations (19 total) | Native binding and boundary |
|---|---|---|
| SRF-01 | `successful_varint_reencodes_consumed_prefix`, `successful_frame_reencodes_consumed_prefix`, `successful_frames_reencode_consumed_prefix`, `epm_value_frames_reconstruct_complete_suffix` | `check_field_partition` reconstructs successful real cursor fields with the independent protobuf writer. `verify_epathmap_value_frame_partition` reconstructs retained EPM1 prefix/value frames against captured source bytes. EPM1 topology remains the existing inspector's authority, not a consequence of this framing proof. |
| SRF-02 | `adjacent_fields_bounds`, `interval_then_suffix`, `successful_fields_partition_consumed_suffix`, `eof_completes_source_partition`, `successful_fields_make_bounded_progress` | Generated valid fields and arbitrary bytes check adjacent nonempty intervals, payload containment, source exhaustion on EOF and progress in both numeric and producer-slot order. The formal relation describes successful interval transitions; proving the concrete cursor always establishes it is a remaining refinement obligation. |
| SRF-03 | `successful_groups_refine_decoder`, `successful_bounded_varint_reencodes_consumed_prefix`, `successful_groups_consume_at_most_budget` | Deterministic native cases exercise every power-of-two value boundary through `u64::MAX`, six tag-width positions and both ordering modes. The model composes radix groups mathematically; a universal proof relating Rust's shift/OR accumulator and machine arithmetic to it remains required. |
| SRF-04 | `framed_empty_values_are_not_omitted`, `wrong_same_length_value_is_not_reconstruction`, `truncated_frame_is_not_a_partition`, `bounded_scanner_accepts_multibyte_value`, `bounded_scanner_rejects_redundant_terminal`, `bounded_scanner_rejects_tenth_byte_overflow`, `bounded_scanner_rejects_eleventh_group` | Formal nonvacuity/negative controls and native malformed fields cover empty values, redundant groups, truncation and overflow. Owned-cursor retries verify unchanged failure state; borrowed-cursor failure atomicity is deliberately not claimed. |

The EPM1 primitive checks minimality by comparing consumed width to the writer's
encoded width; the protobuf primitive rejects a redundant zero terminal group.
The mathematical scanner models the latter. Equivalence of those concrete checks,
native checked-offset conversion, and the parent-frame suspension/certificate
bridge remain explicit obligations. Neither this module nor its bounded native
properties constitute a universal proof of the Rust parser or full substitution.
No new parser is introduced into production, and no generated schema, ACT
topology, field order or consensus encoding changes.

## Field decomposition and suspended-parent certificates (SRD, task 5916)

`SubstitutionRegionDecomposition.v` composes SRF framing with SRP publication.
Its successful field scanner handles the four supported wire kinds, bounds tags
by a supplied format limit, rejects unsupported kinds and truncated bodies, and
derives exact encoded-prefix and length-delimited-header equations from scanning.
It does not assume the complete source is the concatenation it seeks to prove.

A `Ready` state carries completed field/piece pairs and remaining bytes. A
`Suspended` state additionally carries the field whose child is being visited.
The invariant says that completed pieces have their source certificates and that
completed bytes, the suspended field when present, and remaining bytes partition
the input in order. Child publication preserves earlier certificates under the
backward-reference condition; returning the child closes its suspended field.

| ID | Printed obligations (21 total) | Native binding |
|---|---|---|
| SRD-01 | `taken_body_splits_source`, `successful_body_scan_reconstructs_source`, `successful_field_scan_reconstructs_source`, `successful_delimited_field_has_exact_header` | Existing exact cursor reconstruction and new `verify_node_field_certificates` check original encoded fields, tags, payloads, adjacent intervals and final exhaustion. |
| SRD-02 | `initial_frame_has_source_invariant`, `borrowed_field_establishes_certificate`, `par_field_establishes_certificate`, `ready_field_consumption_preserves_source` | Every actual stored piece is paired with its captured source field; Borrowed and Par pieces require exact absolute ranges as well as bytes. |
| SRD-03 | `child_descent_preserves_source`, `published_child_closes_suspended_field`, `prior_piece_source_survives_publication`, `prior_certificate_survives_publication`, `suspended_parent_survives_child_publication` | Child sources come from actual publication capture and require exact payload ranges and backward node indices. Earlier-node emission remains stable after later appends. |
| SRD-04 | `original_value_frames_are_encoded_frames`, `epathmap_field_establishes_certificate` | EPathMap piece bodies reconstruct the complete captured field payload from the unchanged prefix and ordered value frames. The exact-three-value map fixture makes this nonvacuous. |
| SRD-05 | `completed_frame_supplies_publication_certificate`, `completed_frame_reconstructs_original`, `completed_frame_publishes_valid_arena` | All fields must be exhausted after exactly one piece per field, with exact source length. The shared corpus asserts positive counts for all four piece kinds. |
| SRD-06 | `empty_delimited_field_is_present`, `truncated_delimited_field_is_refused`, `unknown_wire_kind_is_refused` | Formal success/refusal controls, native malformed/presence cases, and `field_certificate_rejects_swapped_same_tag_par_cuts`, which first checks the original node then requires the swapped association to fail. |

**Boundary.** The model uses natural-number lengths and SRF's mathematical group
scanner. It does not establish concrete shift/OR accumulation, `usize` conversion,
error diagnostics or schema/field-order acceptance. Correct schema action and
presence-flag selection remain external; cuts and groups remain fixed tables
whose construction and provenance need their own refinement. Child return uses
the child-source equality supplied by the composed publication boundary. Native
rescanning uses the same real cursor, so it independently checks piece/source
association but is not a second parser oracle. Full substitution semantics and
operational-resource acceptance remain open.

## Native varint accumulator refinement (SVA, task 5916)

`SubstitutionVarintAccumulator.v` connects the successful SRF radix scanner to
the two concrete scanner algorithms: protobuf's nonzero-final-digit check and
EPM1's encoded-width check. The accumulator contains only bits below the current
seven-bit group. Consequently the shifted digit is disjoint from those bits,
and bitwise OR equals radix addition. The tenth group is checked before
accumulation, restricting its payload to one bit. The model derives the width
formula used by locked prost 0.14.3 for the entire unsigned 64-bit domain.

| ID | Printed obligations (20 total) | Native binding |
|---|---|---|
| SVA-01 | `low_accumulator_and_shifted_digit_are_disjoint`, `masked_shift_or_equals_radix_accumulation`, `forward_step_preserves_accumulator_invariant` | Independent reversed radix-128 accumulation in `u128`, compared with both private production scanners for arbitrary input and generated canonical values. |
| SVA-02 | `forward_scanner_refines_group_decomposition`, `whole_forward_scanner_equals_group_scanner`, `successful_forward_scan_reencodes_consumed_bytes` | `both_varint_scanners_match_widened_reference_on_arbitrary_bytes` and `both_varint_scanners_preserve_generated_values_and_offsets` check acceptance, value, consumed extent and preserved suffix at valid initial offsets. |
| SVA-03 | `all_native_shifts_are_below_word_width`, `tenth_group_guard_preserves_u64_bound`, `terminal_width_equals_nonzero_high_digit` | `both_varint_scanners_cover_every_terminal_byte_and_group_position` enumerates all 256 candidate bytes at ten positions, low/high lower digits and two offsets. |
| SVA-04 | `forward_scanner_accepts_full_u64`, `forward_scanner_refuses_tenth_byte_two`, `forward_scanner_refuses_redundant_terminal` | `both_varint_scanners_cover_u64_boundaries_without_consuming_suffix` includes maximum u64 and both sides of every positive power-of-two boundary; named malformed cases include tenth-byte overflow and redundant zero. |
| SVA-05 | `width_formula_for_all_u64_logarithms`, `prost_width_is_logarithmic_radix_width`, `radix_quotient_interval`, `prost_width_characterizes_canonical_range`, `terminal_prost_width_equals_nonzero_high_digit` | Both scanners are compared against canonical prost re-encoding; boundary fixtures independently stress every change in encoded width. |
| SVA-06 | `width_checked_loop_equals_terminal_checked_loop`, `both_scanners_return_same_value_and_suffix`, `successful_epm_forward_scan_reencodes_consumed_bytes` | All five native witnesses exercise both actual private scanners, including named errors and exact consumption positions in `malformed_varints_preserve_each_scanners_error_class_and_position`. |

**Boundary.** The zero-initialized ten-group specialization is the native
algorithm correspondence; generalized starting accumulators and shifts are
induction machinery, not an arbitrary native entry contract. The model uses
unbounded binary naturals with proved accumulator/shift bounds, not extracted
Rust or a compiler proof. It erases error distinctions into `None` and therefore
does not prove native error messages, error offsets, `usize` address arithmetic
or owned-cursor failure atomicity. Native named malformed cases check categories
and consumed positions separately; arbitrary malformed properties check
acceptance and cursor agreement, not every diagnostic. Initial positions are
valid positions including EOF. The independent decoding oracle uses prost for
canonical re-encoding, so it is not independent of that encoder.

The manifest binds all 20 printed obligations, the runner's exact closed count,
explicit kernel checking, five native witnesses and both test-only source
module mounts. No production parser, representation, dependency or consensus
format is changed. Full substitution semantics, schema/table construction and
resource/lifecycle acceptance remain separate obligations.


## Append-only table and publication invariants (SRT, task 5916)

`SubstitutionRegionTables.v` replaces the emission/publication models' fixed
byte-table assumption with finite append-only cut and group tables. A group
stores its retained prefix and a contiguous cut interval. A well-formed table
bounds every group interval. A supported piece refers only to existing cuts or
groups; Child references remain governed by SRP's backward-node invariant.

The combined `table_arena_valid` predicate requires well-formed tables,
supported pieces in every node and SRP's original arena invariant. Empty
initialization, cut registration, group registration and successful checked
publication preserve this package. A separate fold theorem equates sequential
cut appends followed by group publication with the batch specification.

| ID | Printed obligations (30 total) | Native binding |
|---|---|---|
| SRT-01 | `extension_preserves_cut_lookup`, `extension_preserves_group_lookup`, `extension_preserves_support`, `group_range_bounded` | Snapshots compare all earlier cut/group entries; bounded group intervals and path references are checked at each direct registration. |
| SRT-02 | `extension_preserves_group_body`, `extension_preserves_piece_bytes`, `extension_preserves_source_piece`, `extension_preserves_field_certificates`, `extension_preserves_arena_validity` | Publication-time tables remain exact prefixes of final tables; original/shallow/compact outputs and cached lengths remain unchanged. |
| SRT-03 | `cut_registration_extends`, `cut_registration_preserves_well_formed`, `cut_registration_has_exact_new_ordinal`, `group_registration_extends`, `group_registration_preserves_well_formed`, `group_registration_has_exact_descriptor` | Real `add_child` and `add_epathmap` calls check returned piece indices, exact table length increments and group start/end. |
| SRT-04 | `suffix_indices_select_values`, `group_registration_preserves_exact_value_order`, `group_registration_reconstructs_original_payload`, `registration_establishes_piece_support`, `cut_registration_establishes_field_certificate`, `group_registration_establishes_field_certificate` | Typed PathMap traversal supplies expected values in trie order; exact encoded frames, empty/duplicate values, offsets and backing pointers must match. |
| SRT-05 | `empty_tables_are_well_formed`, `sequential_cuts_equal_batch`, `publication_preserves_piece_support` | The fixture begins with `Builder::new`, interleaves ordinary cuts with map/set/empty groups, and checks every old group after each new one. |
| SRT-06 | `sequential_group_publication_equals_registration`, `empty_table_arena_is_valid`, `extension_transports_table_arena`, `cut_registration_transports_table_arena`, `group_registration_transports_table_arena`, `checked_publication_preserves_table_arena` | The existing real-node publication corpus checks source fields and all emission modes while retaining every publication-time table snapshot. |

The native transition fixture has two map groups separated by ordinary Par cuts
and set/neutral-empty groups. Its deterministic case includes empty and duplicate
value bodies; the generated case varies optional integer values. It starts each
snapshot at a nonzero backing offset. The three negative controls perturb a
group's start, exchange value ordinals and remove a value's group parent, and each
must fail its corresponding native assertion. These are native controls, not
formal path-provenance theorems.

**Boundary.** The finite model retains bytes, not backing identities or region
offsets, path metadata, storage modes or machine-width ordinals. SRR establishes
region arithmetic separately; a universal composition of path/region
registration with this byte-table model remains required. Actual-field
certificates still come from SRD's successful framing equations. The extension
relation alone does not validate new groups; the concrete registration lemmas
supply that obligation. Native snapshot/path checks are bounded observations,
not a proof of every Rust execution. Private builder mutation before an error
is not claimed to be rolled back; unsuccessful cutting returns no plan.

The existing `u32` occurrence/ordinal representation also requires resolution:
an input-length bound alone does not establish `u32` representability on a
64-bit host. No new limit or waiver is introduced here. Full schema/action,
substitution, charge/error-order and operational-resource gates remain open.


## Input-bounded native ordinals (SRO, task 5916)

`SubstitutionRegionOrdinals.v` models the real contiguous-tag-run counter:
the first positive tag receives ordinal zero, equal adjacent tags increment,
and a changed tag resets the ordinal. After a positive number of successful
fields, the ordinal is strictly below that field count. Successful field
framing has a positive tag and consumes at least one byte, so another successful
field keeps the next ordinal within the native input-length bound.

EPM1's length prefix is nonempty even for an empty value. Its framed byte count
therefore bounds value count and every enumeration index. The machine limit is
a parameter: selecting the host's `usize::MAX` aligns both counters with the
resident slice/arena domain instead of assuming every input fits a u32 counter.

| ID | Printed obligations (16 total) | Native binding |
|---|---|---|
| SRO-01 | `initial_state_is_counted`, `next_ordinal_is_bounded_by_seen_fields`, `advance_preserves_counted_state`, `input_bounded_step_cannot_overflow`, `tag_change_resets_ordinal`, `positive_tag_run_preserves_count` | Real `Frame::next_occurrence` properties compare independent contiguous tag-run lengths; a seeded 64-bit counter crosses the old u32 boundary, and tag changes reset it. |
| SRO-02 | `encoded_varint_is_nonempty`, `successful_field_has_positive_tag_and_extent`, `successful_field_trace_bounds_count_and_tags`, `successful_prefix_makes_next_ordinal_machine_representable` | Existing canonical cursor/region reconstruction tests supply positive field progress and exact source consumption; the capstone composes modeled successful scans with the counter invariant. |
| SRO-03 | `epm_framed_value_count_is_bounded_by_bytes`, `epm_enumeration_ordinal_is_machine_representable`, `narrow_counter_is_not_justified_by_wider_input` | Existing set/map/empty EPM registration tests preserve every framed value and ordinal. The new path test retains full machine-sized occurrences without allocating a huge input. |
| SRO-04 | `widening_preserves_successful_steps`, `paired_cut_indices_are_in_bounds`, `method_argument_index_is_in_bounds` | Receive-address/pairing tests cover wide equality, ordering, surplus skipping and non-aliasing; ordinary printer tests and a source-binding guard cover removal of narrowing in all three sequence helpers. |

**Scope and controls.** The field-count capstone composes the successful
mathematical scanner and reconstruction lemmas; it is not an extracted-Rust or
compiler-correctness theorem. Widening preserves numeric successful transitions,
not by itself every matcher or printer execution. Counter/path/address boundary
fixtures deliberately seed otherwise tiny native objects with large ordinals.
They expose arithmetic and narrowing behavior without pretending to process
billions of fields. The old-boundary tests are gated to 64-bit hosts; ordinary
transition and maximum-native-index path/address tests also run on narrower
hosts. A deliberately invalid synthetic maximum counter retains checked-overflow
behavior, while changing its tag still resets successfully.

No arbitrary input-size or traversal-depth ceiling is added. Machine-index
representability does not establish allocation feasibility or an acceptable
runtime-resource envelope. Field tags remain u32; protobuf, bincode and EPM1
formats are unchanged. The public Rust `ProtobufRegionStep::occurrence` field
changes type, and previously over-u32 native paths can change disposition. See
CBR-044's dated ordinal qualification; unconditional consensus neutrality is
not claimed. Region/path provenance, full substitution semantics and resource/
lifecycle acceptance remain separate requirements.


## Region-backed registration provenance (SRV, task 5916)

`SubstitutionRegionProvenance.v` interprets each SRR region through an immutable
owner-to-bytes store. A path records its native parent and step plus a **ghost**
payload region used only in the proof. Cuts link to those payloads; groups link
their whole payload and contained prefix to their anchor. Erasing region
references to bytes yields exactly SRT's tables. No production allocation of
ghost payloads or decoded Par collections is introduced.

Path-transparent protobuf map entries need special care: a map-entry frame
inherits its parent's path and has a contained payload, whereas an ordinary
descended message has a dedicated path. Containment composes through SRR;
equating every frame with its path payload would be incorrect. MapValue edges
retain the outer map tag/occurrence even though their protobuf value wire tag is
2. Named model kinds denote semantics, not Rust enum discriminants.

| ID | Printed obligations (42 total) | Native binding / scope |
|---|---|---|
| SRV-01 | `containment_is_transitive`, `containment_preserves_region_validity`, `checked_subregion_is_contained`, `subregion_bytes_are_parent_interval` | Checked subregions and captured source/cut/prefix pointer and absolute-range assertions; same-backing slices compose. |
| SRV-02 | `path_append_preserves_old_lookup`, `appended_path_has_exact_index`, `admitted_parent_is_backward`, `path_append_preserves_admission`, `path_chain_has_valid_regions`, `inherited_frame_composes_admission`, `admitted_region_is_valid` | Actual publication captures retain frame schema and path leaf. Ordinary descent checks its dedicated parent; map entries check inherited paths and exact contained field payloads. |
| SRV-03 | `path_registration_erases_to_identity`, `par_registration_erases_to_byte_registration`, `value_registration_erases_to_byte_registration`, `value_fold_erases_to_sequential_cuts`, `value_fold_keeps_groups`, `epathmap_registration_erases_to_byte_registration` | Existing direct registration fixtures compare append-only paths/cuts/groups, empty and repeated values, and reconstructed bytes; these synthetic fixtures assert local registration only. |
| SRV-04 | `path_registration_preserves_cut_links`, `par_registration_preserves_cut_links`, `par_registration_preserves_path_chain`, `group_anchor_is_admitted`, `value_fold_preserves_provenance`, `epathmap_registration_preserves_provenance` | Corpus publication certificates bind every Par cut to its enclosing captured frame and every EPM value to its group; generated injection cases exercise transparent map-entry envelopes. |
| SRV-05 | `value_fold_has_exact_paths_and_cuts`, `value_path_count_is_exact`, `value_cut_count_is_exact`, `value_path_lookup_is_exact`, `value_cut_lookup_is_exact`, `epathmap_has_exact_group_and_value_records` | Exact registration indices, contiguous value ranges, schema IDs, field tags, ordinals, named Snapshot/Value kinds and ordered value bytes; shifted-range/ordinal/wrong-parent negative controls. |
| SRV-06 | `par_registration_transports_published_arena`, `epathmap_registration_transports_published_arena`, `epm_region_framing_establishes_new_field_certificate`, `local_value_ranges_interpret_as_parent_slices`, `source_framing_derives_region_field_certificate`, `region_backed_registration_preserves_provenance_and_publication`, `decoded_epm_suffix_establishes_region_field_certificate` | Existing publication captures preserve prior tables and three-mode meaning. Source framing composes with the existing EPM frame decoder; actual physical range selection is bound by native framing/pointer checks. |
| SRV-07 | `epm_prefix_retains_backing_and_bounds`, `path_registration_preserves_group_links`, `value_fold_preserves_group_links`, `epathmap_registration_preserves_group_links`, `linked_group_prefixes_are_valid`, `region_registration_preserves_all_retained_provenance` | Group anchors retain whole payloads, prefixes remain contained in those payloads, and all retained prefixes are valid under immutable backing/root validity. Native EPM certificates check exact prefix starts and backing. |

**Executable controls.** The maintained manifest lists every printed obligation
and checks the runner's count/kernel check and test bindings.
`captured_frame_provenance_covers_dedicated_and_inherited_paths` requires
ordinary descent, inherited map-entry frames and MapValue edges all to occur.
`captured_frame_provenance_rejects_another_valid_earlier_parent` changes a cut's
parent to another existing backward index; index bounds alone therefore cannot
pass the negative control. `generated_injection_entries_preserve_inherited_frame_provenance`
adds generated map-entry coverage to the existing publication properties.
The direct registration and seeded ordinal fixtures use synthetic owner frames:
they do not establish complete reachable-frame containment and are not presented
as doing so.

**Assumptions and residuals.** The universal model takes immutable backing,
valid root/admitted subregions and selected schema actions. Its decoder wrapper
derives the EPM reconstruction equation from successful existing
`decode_frames` with empty remainder and prefix interval starting at zero;
correspondence of the selected physical regions to the actual inspector remains
a native binding, not extracted-Rust equivalence. Group bounds alone do not
establish canonical encoding. Successful registration preserves old records;
`add_child` may append a path before a later schema lookup fails, so failure
atomicity of the entire builder is not claimed. The capstone transports the
existing byte-arena/publication invariant without proving universal schema
dispatch, all substitution semantics, allocator correctness or resource bounds.
All these latter campaign obligations remain explicit. See
[J-183](pathmap-engineering-journal-2026-08-23.md#j-183--2026-09-18-region-backed-registration-and-frame-provenance)
for evidence and remaining boundaries.

## Physical EPM value-range selection (SVI, task 5916)

`SubstitutionValueRegions.v` models the value-framing loop after a known EPM1
prefix boundary. It uses the existing canonical ten-group length scanner;
the consumed suffix determines header advancement. Each range begins after that
header and ends after its declared payload. The loop keeps the original input
and an absolute cursor, records every range, and requires EOF after the declared
count. The count is a serialized cardinality, not an artificial traversal fuel.

| ID | Printed obligations (22 total) | Executable binding |
|---|---|---|
| SVI-01 | `suffix_split_gives_exact_offset`, `single_frame_selects_exact_physical_range`, `span_body_length`, `partition_has_exact_physical_positions` | Independent append-time oracle checks exact starts/ends at body lengths 0, 1, 127, 128, 16383 and 16384; duplicate and empty bodies cannot be substituted merely because their bytes agree. |
| SVI-02 | `successful_loop_refines_decoder_and_partition`, `partition_has_exact_count_bound`, `partition_retains_ordered_in_bounds_spans`, `successful_inspection_decodes_complete_suffix`, `completed_loop_satisfies_admission_guard`, `selected_headers_and_bodies_reconstruct_whole_input` | Generated bounded arbitrary-body suffixes compare all recorded positions, count and EOF; headers plus bodies reconstruct the consumed suffix. Existing provenance tests retain the resulting fields. |
| SVI-03 | `successful_selection_is_machine_representable`, `unrepresentable_payload_cannot_be_selected` | Canonical u64::MAX header with no body tests checked endpoint overflow on 64-bit or length-conversion refusal on 32-bit, without allocating the claimed payload; successful intervals remain within the actual source. |
| SVI-04 | `trailing_bytes_refuse_publication`, `impossible_count_refuses_before_loop`, `failed_frame_cannot_publish_range_list`, `truncated_payload_does_not_publish`, `inspected_prefix_is_in_bounds` | Noncanonical/truncated headers, short payloads, count mismatch, impossible count and trailing bytes must return errors rather than completed partial range lists. |
| SVI-05 | `duplicate_empty_bodies_have_distinct_positions` | Two empty frames retain distinct offsets; repeated nonempty payloads at different positions retain different ranges. |
| SVI-06 | `region_interpretation_has_exact_length`, `successful_selection_lifts_owned_regions`, `selected_physical_ranges_establish_region_field_certificate`, `selected_physical_ranges_preserve_all_retained_provenance` | Selected local intervals are lifted through a nonzero-offset shared backing with exact owner/range/pointer checks; formal composition derives SRV field certificates and retained provenance. |

For zero-based ordinal $`i`$, the physical-position theorem establishes:

```math
\begin{aligned}
\operatorname{start}_i &=
p + \sum_{j<i}\lvert\operatorname{frame}(b_j)\rvert
  + \lvert\operatorname{varint}(\lvert b_i\rvert)\rvert,\\
\operatorname{end}_i &= \operatorname{start}_i + \lvert b_i\rvert .
\end{aligned}
```

Here $`p`$ is the already identified value-table boundary and $`b_i`$
is the body at ordinal $`i`$. Every header is nonempty, including for an
empty body, so successive occurrences remain physically distinct.

**Independent oracle and limits.** Native expected offsets and prefix end are
recorded while constructing bytes, never read from an inspector. The test uses
PathMap's maintained ACT writer with zero markers and a count-matching topology;
zero-count cases explicitly cover neutral empty, a set and a nonempty value-free
map. Arbitrary body bytes test framing, not their validity as serialized Par.
A separate control passes framing and then fails full-inspector topology
validation, preserving the distinction between those acceptance stages.

The model proves successful-selection refinement to the existing frame decoder,
not unrestricted acceptance equivalence with its unbounded mathematical form.
The machine-bound theorem assumes resident input length fits the host limit;
diagnostic strings/cursor side effects and allocation failure are not modeled.
EPM magic/version/mode/arena-prefix admission, ACT topology and schema dispatch
remain separate refinement boundaries. See
[J-184](pathmap-engineering-journal-2026-08-23.md#j-184--2026-09-18-exact-physical-epm-value-selection)
for receipts and remaining scope.

## EPM mode-model regressions (EMR, task 5916)

The structural EPM1 envelope and positional-marker predicates must cover
value-free map topology and set membership endpoints. They do not themselves
validate ACT bytes or equate physical arena byte length with logical emptiness.

| ID | Printed obligations (seven total) | Native binding |
|---|---|---|
| EMR-01 | `structurally_nonempty_map_may_have_zero_values`, `zero_count_map_framed_round_trip`, `zero_count_map_association_requires_no_valued_endpoints` | Existing three-mode zero-count fixture and the new mode matrix admit a nonempty value-free map but reject a map whose valued endpoint count differs from its body count. |
| EMR-02 | `associated_set_markers_are_zero_without_a_value_table`, `set_association_accepts_any_number_of_zero_markers`, `set_association_rejects_nonzero_markers` | Deterministic and generated endpoint inventories admit all-zero set markers with no value bodies and reject nonzero or mixed markers. |
| EMR-03 | `structurally_empty_typed_arena_is_rejected` | Empty-byte controls reject typed modes; the broader native matrix separately rejects a valid encoded but logically empty ACT, which the structural predicate alone cannot establish. |

The native bindings are `value_region_zero_count_preserves_empty_set_and_value_free_map_modes`,
`mode_admission_uses_logical_topology_and_zero_endpoint_markers` and
`generated_mode_admission_matches_independent_endpoint_inventory`, under
`models/tests/support/substitution_value_regions.rs`. The manifest requires
all seven declarations, these witnesses, the exact closed-count runner and its
kernel check. Generated cases explicitly include all-zero variants, preserving
positive coverage. Full ACT admission, prefix parsing, schema dispatch and
substitution semantics are separate obligations. See
[J-185](pathmap-engineering-journal-2026-08-23.md#j-185--2026-09-18-correct-epm-mode-model-drift)
for provenance, measured receipts and limits.

## EPM prefix and framing composition (SSP, task 5916)

`SubstitutionSnapshotPrefix.v` derives the value-table boundary from fixed
header bytes, an arena frame and a count header, then composes SVI selection.
All twelve printed declarations are required by the manifest and proof runner.

| ID | Printed obligations | Native binding |
|---|---|---|
| SSP-01 | `fixed_header_has_six_bytes`, `successful_mode_selects_exact_fixed_header` | Literal neutral-empty golden bytes, numeric 0/1/2 mode assertions, fixed-header truncation/corruption controls. |
| SSP-02 | `successful_count_selects_exact_boundary`, `successful_prefix_has_exact_source_certificate`, `prefix_offsets_and_count_fit_resident_input` | Independent append-time arena/count offsets and borrowed arena pointer; arena/count widths at 127/128 and 16383/16384; canonical oversized length refusal without allocation. |
| SSP-03 | `prefix_reconstructs_exact_envelope_without_value_decode`, `successful_framing_retains_prefix_and_value_selections`, `complete_framing_reconstructs_exact_epm1_bytes` | Deterministic/generated opaque-arena prefix fixtures and exact empty-frame ranges; existing SVI arbitrary-body and maintained-ACT controls supply value and full-inspector coverage. |
| SSP-04 | `malformed_prefix_never_publishes_framing`, `malformed_value_suffix_never_publishes_framing`, `impossible_count_is_rejected_before_framing` | Malformed magic/version/mode, arena/count varints, truncated arena, count lower bound and native paired-fault precedence checks. |
| SSP-05 | `route_prefix_does_not_validate_topology_or_value_suffix` | Route hint succeeds while the full inspector rejects a truncated value header, malformed ACT or trailing bytes. |

The four new witnesses in `models/tests/support/substitution_value_regions.rs`
are `prefix_offsets_follow_append_time_oracle_across_varint_widths`,
`malformed_prefixes_fail_before_value_range_publication`,
`route_hints_do_not_bypass_framing_or_topology_validation` and
`generated_prefix_offsets_match_independent_construction`. Prefix-only fixtures
may contain opaque invalid ACT bytes: success is not topology admission.
The successful-selection proof bounds native lengths by resident input length;
it does not prove all native error strings, allocation behavior or compiler
correctness. See [J-186](pathmap-engineering-journal-2026-08-23.md#j-186--2026-09-18-compose-epm-prefix-and-physical-value-selection)
for receipts and the remaining ACT-summary/schema/semantic/resource boundaries.

## ACT cached value summary (AVS, task 5916)

The sixteen printed declarations in `actree03_completeness/theories/Epm1ValueSummary.v`
connect the concrete accumulator to independent value inventories, existing
source certificates and typed EPM mode decisions.

| ID | Printed obligations | Native binding |
|---|---|---|
| AVS-01 | `accumulated_count_equals_valued_observation_count`, `value_inventory_is_node_bounded`, `bindings_append`, `certified_summary_count_fits_resident_image` | Independent legacy semantic-tree preorder inventory; valueless nodes are omitted, root and descendant values are retained. Count fits any host bound covering the resident image. |
| AVS-02 | `first_nonzero_matches_independent_find`, `first_nonsequential_matches_value_ordinal_reference`, `concrete_summary_refines_independent_inventory` | Expected full tuples come from independent find/enumeration; a wrapped first offender requires the wrapper ID rather than node ID, and sequential checks use value ordinal rather than node index. |
| AVS-03 | `latched_nonzero_diagnostic_is_retained`, `latched_nonsequential_diagnostic_is_retained`, `absent_nonzero_is_exactly_all_zero_markers` | Multiple offenders retain the first; sequential, all-zero and mixed-marker fixtures exercise positive and negative diagnostics. |
| AVS-04 | `semantic_preorder_markers_match_writer_bindings`, `certified_summary_markers_are_source_writer_markers`, `admitted_canonical_image_summary_uses_forward_preorder` | Existing generated writer images plus independent parser inventory bind forward preorder; deterministic inputs differ from physical-offset order. V5's stored reversed list is explicitly reversed in the model. |
| AVS-05 | `canonical_zero_writer_summary_has_exact_count_and_zero_markers`, `zero_summary_decides_exactly_all_zero_markers`, `typed_mode_decision_uses_derived_count_and_zero_markers` | Maintained ACT zero-marker fixtures and the existing deterministic/generated EPM mode matrices compare endpoint count, all-zero markers, logical emptiness and declared body count. |

The strengthened native witnesses are
`cached_value_summary_matches_independent_preorder_offsets_and_latches`,
`arbitrary_pathmap_writer_images_are_accepted_with_exact_preorder_plans`
and the existing `value_summary_retains_preorder_ordinal_diagnostics_without_an_event_prepass`
under `rspace++/tests/support/pathmap_codec_validated_plan.rs`.
Repeated getters additionally preserve the summary across traversal/rewind,
perform no observed source reads and leave work counters unchanged. Those
getter resource properties are native evidence, not a formal allocation theorem.
The formal source bridge assumes the existing canonical-writer certificate
domain; logical emptiness and universal Rust/native admission are not inferred
from count alone. See [J-187](pathmap-engineering-journal-2026-08-23.md#j-187--2026-09-18-bind-cached-act-summaries-to-source-preorder).

## Substitution cost-event accounting (SCA, task 5916)

### Scoped allocator extension (task 8516, formal-first)

`ScopedHeapCost` in the same theory models successful allocation/reallocation
requests as `(allocated, freed)` events. Signed outstanding bytes are requested
minus freed bytes; the peak is the maximum sampled outstanding balance, not
allocator usable size, RSS or transient realloc internals. Native bindings must
retain input owners outside the measured window, mark arithmetic overflow as
invalid and distinguish output retention from subsequent drop. These are
measurement-instrument rules, not proofs of decoder complexity.

| Statement | Required native witness before measurement |
|---|---|
| `step_preserves_signed_balance` | Generated event traces agree with an independent signed reference fold |
| `peak_dominates_new_balance` | Every step's peak bounds its observed balance |
| `peak_never_decreases` | Peak is monotone across each generated trace |
| `concatenated_windows_compose` | Split trace accumulation equals one complete trace |
| `allocation_then_drop_balances` | Known-size live allocation and full lifecycle checkpoints |
| `reallocation_replaces_old_request` | Explicit grow/shrink realloc test and final drop |
| `external_drop_remains_negative` | Dropping preexisting ownership remains a negative balance, not saturation to zero |
| `no_event_preserves_meter` | Empty capture changes no counters; failed allocator events are not recorded |

Additional Rust obligations not proved by the arithmetic model: panic-safe scope
retirement, nested-capture rejection without losing the outer scope, thread
isolation, failed allocation behavior and checked machine-width counters.

The extended theory has 24 closed declarations and passes independent kernel
checking before the recorder change. Instrument qualification passes 31/31
focused tests, including generated trace arithmetic, real allocation/reallocation,
negative external frees, thread/unwind/nesting and overflow controls. The
workspace manifest binds all eight new statements and both fixture/routing
tests. J-219 records exact receipts and the still-unmeasured resource protocol.

The original `SubstitutionCost` module counts explicit events, not elapsed time or
allocator behavior. Natural-number counters require checked native addition.
A source partition, a complete native event trace and a linear local budget
are premises to the resource bound, not conclusions about arbitrary Rust input.
In particular, overlapping retained region lengths are not bytes copied.
Endpoint multiplicity invariance concerns distinct top-level input traversal,
not mandatory protobuf repetition of nested repeated fields. Each row below
is an obligation inventory established before its native counter implementation.

| ID | Closed model declaration | Native binding / executable obligation |
|---|---|---|
| SCA-01 | accumulation_refines_reference | `scoped_cost_fold_matches_event_reference`: compare every counter with an independent event fold |
| SCA-02 | concatenation_adds_costs | Same property splits arbitrary event sequences and adds the separate totals |
| SCA-03 | prefix_cost_never_exceeds_complete_trace | Same property checks every component is monotone over appended events |
| SCA-04 | shared_references_copy_no_body_bytes | `shared_regions_record_handles_without_body_copies` and `actual_shared_import_records_no_body_copy`: verify actual operation hooks, pointer sharing and zero copied/emitted/decoded bytes |
| SCA-05 | disjoint_root_partition_copies_exact_source | `nonidentity_costs_track_distinct_endpoints_and_single_finish`: copied bytes equal admitted endpoint payload bytes |
| SCA-06 | one_root_copy_per_endpoint | Unary probe plus `measured_par_substitution` independently census distinct encoded root bodies across the recursive oracle corpus; `region_cost_census_exercises_all_nine_nonidentity_root_families` requires non-vacuous nine-family coverage |
| SCA-07 | multiplicity_change_preserves_input_work | `root_multiplicity_does_not_repeat_region_traversal`: vary only root multiplicity and compare work/copy/emission counters |
| SCA-08 | no_finish_trace_has_zero_counters | `scoped_cost_fold_matches_event_reference` excludes arena/finish events and checks zeros |
| SCA-09 | successful_machine_has_one_arena_and_finish | Native non-identity probe observes actual arena creation and finish attempt/success, not wrapper calls |
| SCA-10 | prefinish_error_has_no_finish_attempt | `substitution_error_records_partial_work_without_finish`: semantic error retains partial work and no root finish |
| SCA-11 | identity_bypass_has_no_arena_or_finish | `identity_substitution_has_no_construction_events`: ground identity probe |
| SCA-12 | local_budget_implies_total_budget | Proof-only additive inequality; native complete-hook coverage remains required |
| SCA-13 | declared_linear_partition_bounds_observed_cost | Conditional proof-only bound; experiment 346 is finite-workload scaling evidence, not premise discharge |
| SCA-14 | shallow_cuts_emit_no_descendant_payload | `actual_emission_modes_and_planning_counts_match_returned_bytes` and the feature-enabled all-family region differential compare actual Shallow bytes; existing SRE byte differentials remain binding |
| SCA-15 | compact_cuts_emit_no_descendant_payload | Same real-emitter tests plus existing SRE compact byte differentials; separate mode counters must not merge modes |
| SCA-16 | original_cut_payload_cost_is_exact | Same real-emitter tests plus SRE reconstruction equality bind actual Original byte lengths |

Additional executable-only recorder requirements: thread-local isolation,
rejected nesting, panic cleanup, checked overflow, repeated finish visibility
and no events outside capture. Hooks and collector are compiled only with the
existing `protobuf-projection-experiment` feature; the collector lives under
`models/tests/support/`. Certificate-publication observation must not be active
inside a cost capture because its deliberate reconstruction would contaminate
emission counters. Feature builds are attribution instruments, not the latency
population. No proof of cryptographic, allocator, OS scheduling or Rust compiler
behavior is implied.

`RootBytes` records every `ProtobufRegion::copy_from_slice` request inside the
scope; it is not restricted to the designated root call sites. Unexpected
additional region copies therefore remain observable. `ResidentBytes` covers
entry bytes to `par_bag::decode_exact`, not every generated decoder; `WorkSteps`
counts outer substitution work-item pops, not all constructor, comparison or
allocator operations. `RootKeyBytes` counts complete emitted RPB3 keys including
score and trailer, not protobuf length. Actual output endpoint slices provide
an independent byte-length expectation. Successful unsorted substitution and
prefinish error counts are tested separately; the sorted public API may have
additional legitimate construction stages. These scope distinctions are part
of the binding, not universal source-wide complexity claims.

### Heterogeneous endpoint binding obligations

The next native binding reuses SCA-05/06/09/10/11 for the existing recursive
substitution corpus, with capture restricted to public unsorted `Par`
substitution. Before capture, an independent nine-family encoded endpoint
census excludes resident opaque zipper entries, which do not use region copies.
For a non-identity input, copied endpoint count and bytes must equal that census;
the root enumeration occurs before any descendant semantic error. A successful
run must create one arena and finish once, with emitted key count and bytes
equal to an independent census of the returned resident Bag. The planned
occurrence tree merges equal complete component keys before finalization, so
distinct variables resolving to the same value must yield one emitted key with
combined multiplicity. Identity bypass must record zero events. Prefinish
semantic errors must not record a finish. All oracle work, construction,
inspection and serialization stay outside capture.

Required non-vacuity witnesses are actual non-identity visits covering all nine
root families, including an unforgeable beside a variable, and two distinct
variables resolving to one integer. Scaling that collision input's root
multiplicities must preserve all work counters and scale only output counts.
The corpus's simple replacement environment permits zero resident decoder
bytes; arbitrary replacement environments do not: singleton Bundle/Connective
imports inspect their merge/flatten shape. Original reconstruction emission
must remain absent on the tested region paths. These bindings do not cover
all generated decoders, all arena fragment copies, elapsed time, or source-wide
local budgets, and do not turn SCA-13's conditional bound into a universal one.

The final J-192 gate passes all 20 feature-enabled oracle tests, including
`region_cost_collision_merges_keys_without_multiplicity_work`,
`region_cost_terminal_splices_preserve_unresolved_children`, and the existing
`error_paths_agree` with scoped counters. All 43 manifest bindings and a
default-build corpus rerun pass. The terminal-splice control observes positive
resident-decoder bytes while retaining unresolved child variables exactly;
this prevents treating all environment imports as the simple unary case.

## Collection collision-elision obligations (task 5915)

`CollectionCollisionElision.v` formalizes retaining the input handles of empty
and singleton collections without constructing a collision index. The model's
reference retains the first set representative and the first map key, selecting
a replacement map value only when it is strictly greater. Its ordering model
does not claim a general refinement of Rust's sorting implementation: ordering
zero or one element is the identity, and the multi-entry implementation remains
unchanged. No equality or ordering axioms are needed for the dispatch proof.

The 20 statements below compile with closed assumptions and pass independent
kernel checking (`target/verification/collection-5915-proof-r5.log`). The Rust
bindings were specified before the production optimization. Proof checking
alone is not evidence that native tests or the scaling gate pass.

Native bindings in `models/tests/support/collection_collision_elision.rs`:
**E** = `empty_singleton_observables_and_zero_identity_work`;
**M** = `multi_entry_identity_work_and_observables_unchanged`;
**S** = `singleton_all_schema_families_preserve_score_and_bytes`;
**N** = `nested_singletons_elide_positive_identity_work`.
All observation checks compare the complete component key (including score),
protobuf body and final compact snapshot against the existing typed canonical
sorter. Existing all-permutation collision tests additionally bind representative
selection, root-only locally-free quotienting and strict full-wire map winners.

| Obligation | Formal statement | Native binding |
|---|---|---|
| CCE-01 | `set_empty_exact` | E, set empty |
| CCE-02 | `set_singleton_exact` | E and S, set singleton |
| CCE-03 | `set_dispatch_refines` | E, M and S |
| CCE-04 | `set_multi_entry_unchanged` | M and existing permutation test |
| CCE-05 | `set_observables_unchanged` | E, M and S, complete byte observations |
| CCE-06 | `set_first_representative` | M and existing permutation test |
| CCE-07 | `map_empty_exact` | E, map empty |
| CCE-08 | `map_singleton_exact` | E and S, map singleton |
| CCE-09 | `map_dispatch_refines` | E, M and S |
| CCE-10 | `map_multi_entry_unchanged` | M and existing permutation test |
| CCE-11 | `map_observables_unchanged` | E, M and S, complete byte observations |
| CCE-12 | `map_first_key_and_strict_winner` | M and existing permutation test |
| CCE-13 | `empty_identity_work_zero` | E, zero cursor openings |
| CCE-14 | `singleton_identity_work_zero` | E, S and N, zero cursor openings |
| CCE-15 | `singleton_removed_work_exact` | N, positive independent identity length; exact byte count is source-level correspondence |
| CCE-16 | `multi_entry_identity_work_unchanged` | M and N, one cursor per input key; unchanged exhaustion loop is source-level correspondence |
| CCE-17 | `identity_work_never_increases` | E and M across dispatch cases |
| CCE-18 | `nested_singleton_identity_work_zero` | N, 256 nested constructors |
| CCE-19 | `rejecting_duplicate_set_dispatch_mutant` | Existing duplicate-set collision witness |
| CCE-20 | `rejecting_map_winner_dispatch_mutant` | Existing strict-winner collision witness |

For maps, modeled identity size is **key** identity size, not key/value pair
size. The counter observes actual `ParSemanticIdentity` cursor construction,
scoped only around collection planning after child import. Zero cursor openings
implies zero bytes streamed through that identity path; it does not claim zero
serialization, zero allocation or linear total runtime. The nested test measures
a positive identity outside capture and requires two-entry indexing as a positive
control. `arity_errors_precede_identity_work` binds validation before dispatch;
`identity_counter_is_nested_panic_safe_and_thread_local` qualifies the probe.
Whole construction/witness scaling is separately measured by experiment 347.

Qualification at `f0647928f` passes all six named native tests, 19 existing
construction tests and 44 manifest checks. Before optimization, the native
suite fails exactly its three zero-identity-work witnesses. The full treatment
at runner commit `5b2a5eca4` also passes experiment 347: all 18 family/arm upper
95% time exponents are at most 1.012855, below 1.20. J-194 in the PathMap journal
retains negative controls, raw measurement digests and the boundary between
model claims, native observations and resource evidence.

## Depth-probe termination classification (DPC, task 5917)

`DepthProbeClassification.v` was compiled and independently kernel-checked before
the classifier correction. Its eight Boolean inputs abstract timeout, successful
exit, panic evidence, runtime abort, native stack exception, stack diagnostic,
allocation diagnostic and signal termination. It proves classification, not OS
signal authenticity, stderr completeness, process cleanup or search correctness.
The separate DPS and DPL models below cover search and capture/cleanup; this
classifier model alone does not establish those obligations.

Native bindings reside in `shared/tests/support/test_process.rs`: **M** is
`classification_matches_termination_evidence_exhaustively` (96 Unix status/diagnostic
combinations); **C** is `child_process_outcomes_are_classified_distinctly`; **P** is
`panic_with_stack_text_is_not_stack_exhaustion`; **W** is
`windows_exception_status_requires_compatible_diagnostics`. M, C and P passed
on Linux; W is present but has not been executed on Windows in this qualification.

| ID | Closed declaration | Binding and evidence boundary |
|---|---|---|
| DPC-01 | `timeout_dominates` | M, C; watchdog overrides every diagnostic |
| DPC-02 | `clean_success_ignores_diagnostics` | M; successful child is not failed by text |
| DPC-03 | `panic_dominates_stack_text` | M, P; includes signal plus panic text |
| DPC-04 | `native_stack_status_qualifies` | W; native Windows execution outstanding |
| DPC-05 | `abort_and_stack_text_qualify` | M, C; actual small-stack abort |
| DPC-06 | `unqualified_stack_text_is_not_a_ceiling` | M; ordinary exits, SIGSEGV and SIGKILL |
| DPC-07 | `abort_without_diagnostics_is_not_a_ceiling` | M, C; bare abort is not stack evidence |
| DPC-08 | `allocation_failure_is_not_a_ceiling` | M, C; simulated allocator abort, not a real OOM |
| DPC-09 | `ambiguous_signal_remains_signal` | M; SIGKILL does not prove OOM |
| DPC-10 | `ordinary_failure_remains_failure` | M; unqualified allocation/stack text |
| DPC-11 | `stack_classification_exact` | M and W; Boolean theorem exhaustive, platform adapter coverage distinct |
| DPC-12 | `passed_classification_exact` | M; success requires no timeout |
| DPC-13 | `deceptive_panic_rejects_text_first_mutant` | P, M; pre-correction matrix fails |
| DPC-14 | `timeout_rejects_false_ceiling_mutant` | M, C; no timeout-to-ceiling conversion |

The original cap-first search tests remain intact. Test sources moved out of
production `src/` without changing their module names. J-195 records commands,
resource observations, negative controls and artifact digests. No full-task,
whole-workspace or cross-platform completion follows from this classifier increment.

### DPC extension: typed stack-survival observation

The classifier model adds five declarations for the stack gate's Boolean adapter.
The original 14 declarations remain unchanged. Native bindings below were written
before implementation; all 36 focused shared probe tests pass with 1,024 seeded
property cases. The gate's actual unknown-subject regression fails before the
adapter and passes after it. J-198 records the separate full-gate decoder timeout,
which remains a failure rather than a stack reading. Search geometry
and the existing instrument-floor semantics are not changed by this extension.

| ID | Closed declaration | Native binding |
|---|---|---|
| DPC-15 | `survival_true_exact` | `stack_survival_rejects_all_nonstack_failures_without_losing_evidence` |
| DPC-16 | `survival_false_exact` | same full outcome table |
| DPC-17 | `survival_error_preserved` | same table, including original diagnostic allocation identity |
| DPC-18 | `panicked_exit_rejects_boolean_status_mutant` | same panic row; actual gate unknown-subject regression |
| DPC-19 | `survival_transport_error_preserved` | same test, original owned transport-error identity |

`watchdog_expiry_cannot_satisfy_an_expected_stack_failure` binds the real timeout
child to this adapter. These properties validate interpretation, not the eventual
whole-subject watchdog duration; that calibration remains a separate obligation.

## Typed depth-probe search (DPS, task 5917)

`DepthProbeSearch.v` models base, inclusive cap, bracket and terminal phases.
It proves termination with a strictly decreasing natural-number rank, not an
execution-fuel limit. Its boundary theorem assumes a monotone threshold for
successful and stack-failing observations; arbitrary transport and other failures
are terminal error observations and impose no threshold assumption. It does not
prove that actual stack requirements are monotone or that OS calls terminate.
The Rust bisection is unchanged; the callback now returns `Result`, with the
original error moved into `ProbeFailureCause::Transport` and its depth retained.

Native test abbreviations (all under `shared/tests/support/test_process.rs`):
**B** = `bisection_trace_preserves_bracket_and_first_failing_witness`;
**X** = `maximum_machine_caps_preserve_bounds_and_terminate`;
**T** = `transport_errors_preserve_identity_and_stop_at_every_search_phase`;
**N** = `every_nonstack_outcome_stops_without_a_false_boundary`;
**S** = `cap_first_search_matches_the_monotone_ceiling_spec_exhaustively`;
**O** = `cap_first_matches_frozen_exponential_search_on_power_of_two_caps`;
**P** = `arbitrary_machine_caps_match_threshold_spec`.

| ID | Closed declaration | Native binding |
|---|---|---|
| DPS-01 | `midpoint_strictly_inside` | B |
| DPS-02 | `midpoint_bounded_by_machine_max` | X, P |
| DPS-03 | `either_update_strictly_shrinks` | B |
| DPS-04 | `close_bracket_well_formed` | B, S |
| DPS-05 | `close_bracket_rank` | B; rank is ghost state, width is observed |
| DPS-06 | `step_preserves_well_formed` | B, X |
| DPS-07 | `nonterminal_step_decreases_rank` | B, X, P |
| DPS-08 | `close_bracket_preserves_boundary` | B, S |
| DPS-09 | `step_preserves_exact_boundary` | B, S, P |
| DPS-10 | `terminal_is_absorbing` | T, N; no callback after error |
| DPS-11 | `base_transport_preserves_error` | T, depth one and original allocation identity |
| DPS-12 | `cap_transport_preserves_error` | T, inclusive cap |
| DPS-13 | `midpoint_transport_preserves_error` | T, midpoint |
| DPS-14 | `invalid_midpoint_is_not_a_ceiling` | N, all five non-stack kinds |
| DPS-15 | `broken_base_is_not_a_ceiling` | `a_broken_base_probe_is_not_called_a_ceiling`, N |
| DPS-16 | `passing_unit_cap_uses_base` | `a_unit_cap_is_both_the_base_and_cap_witness` |
| DPS-17 | `passing_cap_needs_two_probes` | `cap_success_needs_only_the_anti_vacuity_and_cap_probes`, X |
| DPS-18 | `finished_exact_has_correct_survivor` | B, S, O, P |
| DPS-19 | `finished_cap_is_only_a_lower_bound` | S, X, P |
| DPS-20 | `search_terminates_without_fuel` | B, X, P; finite native witnesses, universal proof in Rocq |
| DPS-21 | `complete_run_preserves_exact_boundary` | S, O, P |

The model abstracts failure payloads; B additionally checks that the native result
retains the nearest failing boundary's diagnostic. T uses an owned non-Copy value
and checks allocation identity after propagation. Real subprocess transport and
the minimum-stack search caller remain separate lifecycle/integration obligations.

## Bounded depth-probe lifecycle (DPL, task 5917)

`DepthProbeLifecycle.v` has 29 closed declarations, independently kernel-checked
before runner implementation. The model covers sticky evidence, a nonrenewable
cleanup start time, status-before-timeout observation, classification versus
harness-error completion, bounded read attempts and exact chunk-independent
raw-byte suffix retention. Expired cleanup cannot poll again. Liveness depends
on clock and OS-call progress; this is not a kernel scheduling bound or a promise
of successful reaping after arbitrary OS failures. Error completion may close
the pipe without EOF, but classified completion may not. Completion observed
at the cleanup boundary takes priority over expiry. Zero execution timeout and
unrepresentable deadlines are rejected before spawning in the intended binding.

Native bindings under `shared/tests/support/test_process.rs`: **D** =
`lifecycle_decisions_match_complete_capture_and_cleanup_contract`; **O** =
`lifecycle_observations_latch_evidence_without_renewing_cleanup`; **T** =
`diagnostic_tail_is_exact_across_chunk_boundaries` and
`diagnostic_tail_matches_suffix_spec`; **Q** =
`interrupted_reads_consume_the_quantum_and_pending_stops_it`.
The initial model publication preceded runner implementation. The final Linux
focused acceptance passes all 34 selected tests with 1,024 seeded property cases;
J-197 records the artifacts. **F** below denotes tests in the `lifecycle_faults`
module; all other names are in the surrounding support module. Scripted adapter
failures exercise the driver's error path, not every possible kernel failure.
Windows-native adapter execution remains outstanding.

| ID | Closed declaration | Binding/required evidence |
|---|---|---|
| DPL-01 | `initial_polls` | D |
| DPL-02 | `first_error_preserved` | F: `original_io_error_allocation_survives_secondary_failures` |
| DPL-03 | `failed_capture_disables_reads` | D; F: `first_error_survives_cleanup_and_capture_is_disabled` |
| DPL-04 | `eof_disables_reads` | D; F: `observed_exit_wins_at_deadline_and_eof_is_not_read_again` |
| DPL-05 | `reaped_is_sticky` | O |
| DPL-06 | `eof_is_sticky` | O |
| DPL-07 | `failure_is_sticky` | O |
| DPL-08 | `timeout_is_sticky` | O; F: `timeout_survives_failed_kill_racing_with_successful_exit` |
| DPL-09 | `cleanup_is_never_renewed` | O; F: `output_never_renews_cleanup_and_reaped_status_is_not_polled_again` |
| DPL-10 | `exit_starts_cleanup` | O |
| DPL-11 | `failure_starts_cleanup` | O; F: `first_error_survives_cleanup_and_capture_is_disabled`, `wait_failure_preserves_original_error_after_reaping` |
| DPL-12 | `live_deadline_latches_timeout` | O; `continuous_output_cannot_starve_the_execution_deadline` |
| DPL-13 | `live_deadline_starts_cleanup` | O |
| DPL-14 | `observed_exit_precedes_timeout` | O; F: `observed_exit_wins_at_deadline_and_eof_is_not_read_again` |
| DPL-15 | `classify_requires_complete_capture` | D; `inherited_stderr_writers_cannot_hold_cleanup_open` |
| DPL-16 | `complete_capture_classifies` | D; `finite_output_larger_than_pipe_capacity_has_exact_raw_tail` |
| DPL-17 | `reaped_error_needs_no_eof` | D; F: `first_error_survives_cleanup_and_capture_is_disabled` |
| DPL-18 | `eof_alone_cannot_classify` | D; `stderr_eof_does_not_mean_the_child_exited` |
| DPL-19 | `expired_cleanup_cannot_poll` | D; F: `slow_termination_cannot_start_a_read_after_cleanup_expiry`, `failed_termination_reports_unresolved_child_with_bounded_work` |
| DPL-20 | `failed_capture_never_classifies` | D; F: `first_error_survives_cleanup_and_capture_is_disabled` |
| DPL-21 | `inherited_writer_expiry_is_error` | D; `inherited_stderr_writers_cannot_hold_cleanup_open` |
| DPL-22 | `retained_length` | T |
| DPL-23 | `retained_suffix` | T |
| DPL-24 | `short_capture_is_unchanged` | T |
| DPL-25 | `retain_is_idempotent` | T |
| DPL-26 | `retention_is_chunk_independent` | T |
| DPL-27 | `attempts_bounded` | Q; continuous readable input |
| DPL-28 | `interruptions_consume_quota` | Q |
| DPL-29 | `pending_stops_quantum` | Q |

`invalid_deadlines_are_rejected_before_spawn` additionally binds the native
positive/representable deadline precondition. Q covers both an interrupted reader
and an always-readable reader: each uses exactly sixteen attempts, each successful
read bounded to 4,096 bytes. Lossy UTF-8 expansion is witnessed by the large-output
native test and is deliberately outside the raw-byte retention bound.

## Checked expected-probe core policy (DCP, task 5917)

`DepthProbeCorePolicy` in `DepthProbeClassification.v` adds nine closed declarations,
kernel-checked before implementation. It models selected-child dispatch, an explicit
opt-in, a lazy installer result, and permission to execute the probe body. It does
not prove OS syscall semantics or suppression on unsupported systems. The existing
classifier/adapter contributes another 19 declarations in the same source.

All native bindings are in `shared/tests/support/test_process.rs`. **R** denotes
`core_policy_preserves_installed_unsupported_and_original_errors`; **E** denotes
`failed_core_policy_exits_before_probe_body`; **L** denotes
`core_policy_is_child_scoped_and_explicit_opt_in_preserves_dumpability` (Linux).

| ID | Closed declaration | Native binding |
|---|---|---|
| DCP-01 | `explicit_allow_skips_installer` | `core_policy_opt_in_skips_installation`; L |
| DCP-02 | `suppression_installed_exact` | R; L verifies `PR_GET_DUMPABLE == 0` |
| DCP-03 | `unsupported_preserved` | R; `unsupported_core_policy_reports_without_claiming_suppression` |
| DCP-04 | `installation_error_preserved` | R checks original allocation identity and OS error code |
| DCP-05 | `installation_error_prevents_body` | E, real child exits 2 before the sentinel |
| DCP-06 | `explicit_allow_permits_body` | L checks successful opt-in child and unchanged dumpability |
| DCP-07 | `unsupported_not_installed` | R; unsupported diagnostic child |
| DCP-08 | `failed_policy_is_not_stack_evidence` | E, typed `ExitFailure`, not `StackExhausted` |
| DCP-09 | `unselected_child_does_not_apply_policy` | `classifier_child_without_selector_is_a_noop`; caller source guards |

`core_policy_requires_exact_opt_in` additionally binds the Boolean model input to
the exact environment value `1`, with 1,024 seeded generated cases. L supplies a
deterministic positive opt-in witness and checks parent dumpability after each
child. `automated_classifier_children_remove_inherited_core_opt_in` checks command
construction for the deliberately aborting fixtures. Direct selected-child runs
retain the developer opt-in; automated parent commands remove inherited opt-in.

The focused suite passes 44/44 tests; the complete non-ignored shared-library suite
passes 82/82 (three ignored tests excluded). J-200 records the initial diagnostic
capture failure, its correction and verification receipts. This does not close
the separate task-5834 decoder timeout or whole-gate watchdog qualification.

## Decoded incomplete-component construction (DCC, task 5834)

The original `DecodedComponentConstruction.v` kernel has 23 closed declarations, compiled and independently
kernel-checked before native tests and implementation. It reuses `StackSafePDA`'s
fold-machine equivalence and `EPM1`'s varint framing. It models ordered Par holes in
a finite generated shell, planned handles and resident substitution, and final
component observations. It does **not** prove the full protobuf parser/merge machine,
native integer addressability, cryptographic primitives or the concrete score codec.

The native correspondence assumes valid handles from the same construction arena
and a correct generated shell program. Rust derives slot ordinals by visiting actual
schema fields; it does not accept an arbitrary caller-supplied ordinal program or
dynamically validate handle provenance. Model rejection of permuted ordinal programs
therefore specifies the schema contract; native endpoint comparisons detect schema
order mistakes. The model's arena is prior Par denotations, not all private scratch
allocations: failed attempts may append unreachable protobuf/score fragments.

Bindings are in `models/tests/support/incomplete_component_preservation.rs`:
**T** = `planned_incomplete_top_level_families_match_resident_endpoints`;
**E** = `planned_incomplete_expr_presence_matrix_matches_resident_endpoints`;
**N** = `incomplete_nested_shells_preserve_guards_patterns_and_injections`;
**P** = `incomplete_plans_preserve_arbitrary_ordered_children`;
**M** = `incomplete_slot_order_and_presence_mutants_change_bytes`;
**D** = `incomplete_mixed_family_construction_keeps_handles_on_a_fixed_stack`;
**C** = `incomplete_collection_keeps_duplicate_pairs_and_order`;
**F** = `incomplete_planning_has_no_resident_finish_events` (existing experiment feature).
Their shared `check_shell` compares resident construction with planned construction,
including exact compact endpoints, protobuf, bincode, metadata, multiplicity, order
and hash; it also checks complete-shell decline, missing/excess slots, no failed
component publication and prior-handle preservation.

| ID | Closed declaration | Native binding / boundary |
|---|---|---|
| DCC-01 | `ordered_hole_substitution_is_exact` | T, E, N, P; exact component body |
| DCC-02 | `worklist_emission_refines_resident_substitution` | T, E, N, P, D |
| DCC-03 | `emitted_program_has_one_instruction_per_shell_node` | modeled shell-program bound; D checks bounded retained fragments |
| DCC-04 | `successful_plan_has_exact_ordered_slots` | T, E, N; generated-schema premise |
| DCC-05 | `successful_plan_consumes_exactly_all_children` | shared missing/excess checks |
| DCC-06 | `missing_or_extra_children_are_rejected` | shared typed errors; no component publication |
| DCC-07 | `permuted_or_repeated_slots_are_rejected` | model schema-negative witnesses; M detects wrong child order/duplication natively |
| DCC-08 | `accepted_handles_refer_to_existing_children` | same-arena handle premise; tests allocate all handles in their arena |
| DCC-09 | `complete_shell_declines_without_arena_change` | complete mask cases check all seven arena lengths |
| DCC-10 | `every_prepare_preserves_prior_arena` | prior bytes unchanged after successful and failed construction |
| DCC-11 | `prior_denotations_survive_arena_extension` | shared prior/root replay after both arity failures |
| DCC-12 | `modeled_plan_never_sorts_or_materializes` | C preserves pairs/order; F counts zero resident finishes during planning |
| DCC-13 | `modeled_plan_retains_each_handle_once` | D counts exact Par references and bounded inline bytes |
| DCC-14 | `absent_and_present_empty_are_distinct` | M; T/E presence matrices |
| DCC-15 | `final_endpoint_equals_resident` | exact compact endpoint, protobuf, bincode, metadata, count/order/hash comparisons |
| DCC-16 | `successful_plan_uses_supplied_nonsemantic_score` | compact endpoint equality against existing resident key compiler |
| DCC-17 | `nested_field_length_prefix_round_trips` | exact protobuf comparisons, P includes metadata across varint-size boundaries |
| DCC-18 | `ordered_distinct_handles_are_accepted` | T, N, M |
| DCC-19 | `misordered_slot_mutant_is_rejected` | formal schema mutant; M has distinct child order witnesses |
| DCC-20 | `missing_slot_mutant_is_rejected` | shared missing/excess arity checks; M dropped child differs |
| DCC-21 | `duplicated_slot_mutant_is_rejected` | formal schema mutant; M duplicate child differs |
| DCC-22 | `reordered_or_duplicated_children_change_observation` | M |
| DCC-23 | `bad_framing_mutant_changes_body_extent` | formal wrong-length witness; native full encoded-byte equality |

The final helper is a shared construction primitive, not yet a replacement decoder.
Task 5834 still requires planned generated decode/merge frames, malformed/error-state
refinement and the unchanged full depth-gate rerun. The independent legacy decoder
execution reference is qualified by the following DOR boundary and J-203.

## Independent decoder execution reference (DOR, task 5834)

`DecoderOracleRouting.v` proves ten routing statements. Its model selects a frozen
or successor implementation, or refuses successor execution under poison. It does
not prove either parser correct. Native routing must cover every decoder entry and
indirect re-entry; that correspondence premise is challenged by independent body
guards and executable missing-public-guard controls, not assumed from equal output.
The model treats observations as arbitrary values, so preservation includes errors,
remaining input and partially merged state without redefining their contents.

The test-only sources live under `models/tests/support/`. **S** denotes
`routing_scope_restores_nested_state_and_unwind`; **P** denotes
`successor_poison_is_live_and_legacy_route_bypasses_it`; **D** denotes the descriptor
corpus; **E** denotes EPathMap map values, legacy fields and mode conflicts;
**O** denotes root/partial-merge observations and arbitrary-buffer properties;
**M** denotes the two missing-public-guard controls inside the generated decoder.

| ID | Closed declaration | Native witness / limitation |
|---|---|---|
| DOR-01 | `legacy_routes_frozen` | P, D, E; decoder entry counters |
| DOR-02 | `poisoned_legacy_routes_frozen` | P, D, E under successor poison |
| DOR-03 | `poison_detects_successor` | P's treatment negative control |
| DOR-04 | `ordinary_routes_successor` | P; zero frozen entries outside legacy scope |
| DOR-05 | `scope_restores_exact_state` | S; normal and unwind RAII restoration |
| DOR-06 | `nested_scope_restores_legacy` | S; nested saved state |
| DOR-07 | `routing_preserves_observation` | O; exact errors, bytes and input extent |
| DOR-08 | `closed_reentry_stays_frozen` | D, E plus independent successor-body assertions; coverage remains a native premise |
| DOR-09 | `disjoint_thread_state` | S; fresh thread does not inherit active legacy scope |
| DOR-10 | `missing_guard_poison_witness` | M deliberately bypasses public guards before fast import and machine execution |

The frozen reference shares unchanged Bag canonicalization, encoding, hashing,
PathMap and EPM primitives. Independence applies to decoder execution, not to those
shared algorithms. Tests retain legacy scope through observations that may decode
resident components. A borrowed `&mut dyn Buf` bounds frozen-machine monomorphization
without flattening input or hiding `advance`/`remaining` behavior. The frozen source
is checked against a pinned digest; it is not regenerated from the treatment schema.
J-203 records provenance, native qualification and remaining decoder work.

## Ordered protobuf Par accumulation (PPA, task 5834)

`ProtobufParAccumulation.v` proves 13 closed statements before the accumulator's
implementation. Events denote completed scalar parsing or component preparation,
including bytes already consumed and an explicit error. They are not a new wire
parser. The model's immutable root list denotes logical occurrence histories;
the Rust implementation reuses the existing persistent occurrence AVL dictionary.
Canonical key comparison, AVL balance, scalar parsing and concrete addressability
remain correspondence premises. The maximum multiplicity is parameterized, not an
artificial traversal-depth bound. The proof does not establish allocator rollback,
generated-schema completeness, or an unconditional linear-time bound.

Native witnesses in `models/tests/support/protobuf_par_accumulation.rs`: **F** is
`all_component_families_preserve_explicit_root_metadata`; **E** is
`malformed_scalar_updates_keep_exact_state_error_and_consumption`; **A** is
`failed_occurrence_insert_preserves_builder_and_prior_roots`; **M** is
`move_reentry_and_insert_do_not_copy_metadata_or_finalize_pars`; **D** is
`mixed_nested_accumulation_is_stack_safe_and_finalizes_each_par_once`; **P** is
`ordered_field_events_match_frozen_committed_observations`; **C** is the existing
experiment-feature counter witness `accumulation_emits_no_resident_root_until_requested`.

| ID | Closed declaration | Native witness / exact boundary |
|---|---|---|
| PPA-01 | `each_field_refines` | F, E, P; component preparation is a shared prerequisite |
| PPA-02 | `ordered_fields_refine` | P compares frozen committed observations after every event |
| PPA-03 | `earlier_arena_roots_survive` | A compares prior planned bytes after failure and later insertion |
| PPA-04 | `insertion_preserves_metadata` | F, M, P; no summary union from inserted components |
| PPA-05 | `metadata_update_preserves_authority` | E compares the occurrence root before/after each scalar update |
| PPA-06 | `connective_update_preserves_authority` | E, P |
| PPA-07 | `explicit_empty_locally_free_replaces` | F resets nonempty LF to empty |
| PPA-08 | `explicit_false_connective_replaces` | F resets true to false before another connective-bearing component |
| PPA-09 | `insertion_failure_preserves_authority` | A injects early and post-node-append length-aggregate failures with nondefault root metadata; checked multiplicity refusal remains the existing occurrence primitive's premise |
| PPA-10 | `refusal_preserves_committed_state` | E compares exact scalar errors, consumed position and surviving committed bytes |
| PPA-11 | `refusal_does_not_visit_suffix` | E checks remaining bytes; first-error event-loop termination is modeled, with generated routing qualification still required |
| PPA-12 | `derived_metadata_mutant_changes_observation` | F inserts components carrying metadata after explicit root defaults |
| PPA-13 | `union_metadata_mutant_changes_explicit_reset` | F and P retain last-write rather than union semantics |

M verifies pointer/capacity preservation through 4,096 owned slot moves and duplicate
insertions after a 64 KiB LF payload. No Par node is finalized and no additional score
or protobuf nodes are built during that accumulation; consuming freeze retains the
metadata allocation. D verifies depths 256/1,024/4,096/8,192 on a 256 KiB stack, one
planned Par per completed level, one occurrence node per level and a bounded linear
protobuf-node count. C distinguishes planned freeze from resident emission: internal
planning must emit no resident root, and the requested final root emits once.
These resource witnesses are not formal allocation bounds or decoder benchmarks.

## EPM payload representation (EPR, task 5834)

`EpmPayloadRepresentation.v` is a narrow value-representation proof, kernel checked
before production genericization. Logical endpoint identities are natural numbers;
the binding list models point updates, not PathMap's physical nodes or traversal
order. A denotation maps each supplied handle to its completed value. Valid same-arena
handles and unchanged endpoint/control decisions are premises. Topology is a retained
ghost observation: the model proves that payload edits do not change it, not that the
ACT parser constructs it correctly. Existing V2/V6 positional authorities retain
their documented byte/control refinement boundaries. This addition is not a complete
parser proof or a proof of Rust ownership, PathMap internals, or resource bounds.
Model `Refuse` denotes latched semantic rejection and discarded private storage,
not every returned API error. Double-yield, push-without-yield and excess-push
misuse errors leave the pending reader recoverable; U exercises that distinction.

Native witnesses in `models/tests/support/epm_payload_representation.rs`: **T** is
`opaque_payloads_preserve_modes_topology_and_remove_scaffolds`; **K** is
`equal_payload_handles_do_not_collapse_endpoints`; **E** is
`malformed_frames_and_deferred_precedence_are_pinned`; **U** is
`pending_protocol_misuse_never_publishes_incomplete_values`; **L** is
`scaffold_factory_remains_lazy_and_matches_nonroot_value_free_events`; **H** is
`planned_handles_keep_exact_associations_without_resident_emission`; **D** is
`generic_topology_replay_and_drop_are_stack_safe`; **P** is
`arbitrary_prefix_maps_preserve_payload_association`.

| ID | Closed declaration | Native binding and scope |
|---|---|---|
| EPR-01 | `erase_commutes_with_denotation` | T, P compare exact topology and final bindings after scaffold removal |
| EPR-02 | `install_commutes_with_denotation` | T, H, P compare independently inserted source endpoints with generic replay |
| EPR-03 | `each_writer_step_commutes` | T and E bind updates and latched semantic rejection; U separately checks recoverable API guards; framing decisions remain shared prerequisites |
| EPR-04 | `writer_trace_commutes` | P varies prefix maps and dangling topology; it does not quantify arbitrary malformed ACT traces |
| EPR-05 | `publication_commutes` | H compares exact same-arena handles without materializing values |
| EPR-06 | `operations_preserve_topology` | T, P compare exact ACT layout prefix, including root values and dangling branches |
| EPR-07 | `refusal_cannot_publish` | E checks semantic rejection prevents publication; U independently checks incomplete finish and recoverable protocol errors, not absorbing Refuse for every error |
| EPR-08 | `scaffold_erasure_preserves_real_bindings` | T, L bind absent-focus and strict-descendant premises to actual topology events; removal retains paths |
| EPR-09 | `equal_payloads_preserve_distinct_keys` | K retains root and two distinct endpoints carrying one equal token |
| EPR-10 | `scaffold_leak_mutant_is_observable` | T, P use a sentinel scaffold outside the fixture's value domain and compare all endpoints |

L additionally guards allocation timing: the legacy default factory is not called
at reader creation, for Empty/Set/root-only Map, or after rejected topology. H uses
the existing experiment counters to require zero resident finishes, bytes, or root
keys. D covers 8,192-byte topology depth and teardown on a 256 KiB stack. These native
resource witnesses are not consequences of the representation lemmas.

The frozen protobuf decoder shares the pending EPM core. Its EPM differential cannot
independently certify this refactor. E instead pins expected framing/canonicality/
protocol messages, and obtains child-error wording from the isolated frozen child
decoder, rather than from another instantiation of the modified pending core.

## Protobuf owned slots (POS, task 5834)

The original `formal/rocq/stack_safe_pda/theories/ProtobufOwnedSlots.v` kernel proves
44 exported statements about payload-parametric container operations and ranked ownership.
Kernel checking preceded the generated private-record implementation. The native
bindings live in `models/tests/support/protobuf_owned_slots.rs`; they qualify the
projection layer and pin existing decoder contracts before production routing.

Witness abbreviations: D = descriptor-complete defaults/fields/oneof projections;
O = optional take/reopen; P = randomized repeated children and keyed replacements;
K = protobuf map-entry defaults, repeated values, and failed-entry nonpublication;
A = same/different oneof arms in both owned records and the current decoder;
F = injected projection failure; R = complete schema-edge rank validation and the
separate native Var remainder family; T = TaggedContinuation body-before-guard
projection trace. P runs with the recorded property-test seed and case count.

| Obligations | Formal statements | Native binding and limit |
|---|---|---|
| Optional presence | `absent_slot_commutes`, `present_slot_commutes`, `absent_and_present_default_remain_distinct` | D, O preserve None versus Some(empty) |
| Take/default/reopen | `take_slot_commutes`, `reopen_commutes`, `take_reopen_commutes`, `reopen_present_preserves_child` | O moves a nonempty builder, adds another component, then projects both |
| Repeated children | `repeated_default_commutes`, `repeated_append_commutes`, `repeated_append_preserves_order` | D, P retain encounter order and duplicate values |
| Entry assembly | `default_entry_commutes`, `entry_key_update_commutes`, `entry_value_update_commutes` | K pins omitted key/value and repeated value-field merging in the existing decoder; mirror routing remains pending |
| Map replacement | `keyed_replacement_commutes`, `keyed_lookup_commutes`, `successful_keyed_replacement_is_last_wins`, `successful_keyed_replacement_preserves_other_keys` | P compares all ordered BTreeMap bindings after repeated keys; K pins successful empty-entry replacement |
| Key erasure helpers | `erase_key_commutes`, `erase_preserves_other_lookup` | P supplies the corresponding observations through successful last-wins replacement; these two helper lemmas are included in kernel checking |
| Oneof selection | `oneof_selection_commutes`, `same_oneof_arm_reopens_existing_child`, `different_oneof_arm_starts_default` | A uses distinct nonempty children and compares owned take/select/attach with the frozen decoder |
| Oneof attachment | `successful_oneof_attach_commutes`, `successful_oneof_attach_selects_new_arm` | A and D preserve constructor/payload; scalar arms use nondefault witnesses |
| Successful commit | `child_attachment_commutes`, `optional_attachment_commutes`, `repeated_attachment_commutes` | D, O, P establish generated structural projection, not the future PDA attachment implementation |
| Failed commit | `failed_attachment_preserves_committed_parent` | K pins existing parser behavior; F verifies projection cannot return a partial parent |
| Recoverable guards | `failed_attachment_allows_later_success`, `rejected_api_guard_preserves_parent`, `guarded_attachment_commutes` | EPR's recoverable protocol tests cover the existing EPM guard boundary; generated owned-PDA attachment binding remains required at cutover |
| Negative controls | `repeated_prepend_mutant_changes_order`, `absent_collapsed_to_default_mutant_changes_presence` | P and O contain distinguishing nonempty/order/presence witnesses |
| Schema ownership | `ownership_path_rank_budget`, `ownership_path_length_is_schema_bounded`, `strict_schema_rank_excludes_positive_cycles` | R establishes decreasing edges over the same resolved schema graph; T independently pins protobuf traversal order |
| Component frontier | `component_frontier_selects_incomplete`, `component_frontier_selects_complete`, `component_frontier_commutes` | `all_component_families_match_frozen_attachment_for_complete_and_incomplete_shells` and `ordered_and_commutative_collections_match_frozen_attachment` compare frozen-decoder component identities; equivalence of each selected family constructor is a premise, not proved by this Boolean dispatch lemma |
| Method child order | `method_child_order_commutes`, `method_child_order_preserves_cardinality`, `method_child_order_retains_target_last`, `method_unrotated_mutant_changes_order` | `method_order_and_incomplete_lane_match_frozen_keys` uses nonempty target/arguments and compares protobuf and RPB3 keys; target moves from first wire slot to last planner slot exactly once |
| Optional semantic children | `semantic_optional_commutes`, `semantic_optional_absent_retains_absence`, `semantic_optional_present_retains_presence` | `guard_and_condition_completion_preserve_frontier_semantics` covers absent, present-empty, LF-only, connective-only, and nonempty conditions/guards; synthesized empty handles do not change shell presence and are added only in the complete lane |

The ten component-frontier statements were compiled and kernel checked before
adapter implementation (`protobuf-attach-8516-r2`, 13.520 seconds, 397.7 MiB peak,
zero swap, 2 GiB cap). Their native bindings, preregistered before implementation,
are in `models/tests/support/protobuf_construction.rs` and pass with 1,024 property
cases and seed 5834. `captured_epm_bounds_and_complete_shape_are_certified` ties the
descriptor-derived EPM cut bound and route checks to semantic completeness.
`epm_and_ezipper_attachment_preserve_topology_payloads_and_distinct_metadata`
binds retained mode, value-free topology, payloads, remainder and inner/outer metadata.
`deep_mixed_attachment_uses_flat_plans_without_resident_children` builds 8,192
mixed levels on 256 KiB and, with instrumentation, observes zero intermediate
finish calls. These are adapter tests, not production decoder routing tests.
The first run found a missing Boolean type annotation; no Rust adapter existed yet.
The existing DCC ordered-hole substitution and PPA accumulator models cover their
respective boundaries. Neither proves the semantic family constructors themselves.

**Premises and exclusions.** Defaults and successful commits must correspond under
the selected payload denotation. Failure preserves the saved parent after any prior
take, not a rollback of that take. The keyed formal model establishes lookup
observations, not BTreeMap physical layout or iteration order; P supplies native
ordered observations. R bounds only the residual generated message graph.
Oneof wrappers subdivide a message edge at most once: rank r bounds expanded
message/oneof nesting by 2r+1 edges, not an exact native-frame count. Par/EPM
cut implementations, including branching PathMap destruction and panic/error
cleanup, need their own guarantees; this model does not prove them. The EPM remainder
is the existing native Var family, separately checked to reach neither Par nor EPM.
No byte parser, Rust memory/resource behavior, complete decoder refinement, or
production performance theorem is claimed. Component planning is implemented and
qualified separately from public projection; the general decoder still needs routing
through these adapters. The public EPM materialization helper is qualified by
J-215 below but is not yet wired into the general decoder. D's empty-EPM callback
remains a test double, not a production implementation.

### Guarded consuming cursor extension (task 8516)

Preregistered before Rust changes: the consuming cursor observes the existing
trie's ordered bindings. Each successful step moves one binding; early termination
leaves the exact suffix owned by the guard. Natural-number endpoint identities
stand for distinct byte paths, with an optional root binding first. Topology is
again a ghost observation. The model does not prove PathMap's physical navigation
or destruction; native tests must independently check these refinement boundaries.
The count in `cursor_take` specifies how many observations a consumer requests;
it is not a production traversal fuel or depth limit.

| Closed declaration | Required native witness in `epm_owned_cursor.rs` |
|---|---|
| `cursor_next_preserves_topology` | Prefix-valued and dangling-path fixtures retain every original path after each removal |
| `cursor_next_conserves_bindings` | Yielded item plus remaining borrowed trie entries equal the pre-step sequence |
| `cursor_take_is_ordered_partition` | Generated byte-key maps and sets match independently sorted expected bindings at arbitrary stopping positions |
| `cursor_take_conserves_bindings` | Yielded prefix plus guarded suffix equals the input, including duplicate payloads at distinct keys |
| `cursor_take_preserves_topology` | Every retained path remains visible throughout generated removal sequences |
| `cursor_exhaustion_is_fused` | Repeated `next` after exhaustion returns absence without restarting navigation |
| `cursor_early_retirement_owns_exact_suffix` | Tracked payloads balance on partial drop and callback unwind; deep branching retirement on a small stack |
| `cursor_root_skip_mutant_is_observable` | Root, prefix, descendant and sibling values appear exactly once in trie order |

The cursor must remain guarded before copying keys or invoking consumer callbacks.
It uses the shared path-preserving value-removal policy, not unconditional pruning
or unconditional non-pruning. Existing EOR quarantine obligations govern retirement.
This extension does not establish resident clone-family ownership or make native
algebra temporaries safe; those are later boundaries of the same decoder task.
The five native witnesses and their manifest pass in J-213's default 35-test
selection. The generated test preserves the original input domain and saved seed;
its builder uses `NativePathCursor` after a raw `create_path` fixture failure.
The topology observer enumerates every logical byte path through child masks,
including absence of added paths. A separate uninterrupted consuming traversal
is compared with an independently built ordered map. Retirement counts refer to
tracked test payloads, not a Rust lifetime proof. J-213 retains all negative runs.

### Fallible payload mapping and compressed spans (task 8516)

Preregistered after `b031e0a3e`, before production mapping changes. Extend EPR
with an ordered fallible reference and an accumulator machine. Map values are
converted once in root-first lexicographic order; failure returns the first error,
without publishing a partial destination. Set and Empty transfer without invoking
the callback. Typed-empty modes and value-free topology remain observable.

| Closed declaration | Required native witness |
|---|---|
| `mapping_loop_refines_reference`, `mapping_empty_accumulators_are_equivalent` | Generated binding/trace differential against an independent ordered reference |
| `mapping_success_preserves_keys_and_trace`, `mapping_success_exact_callback_count` | Exact key/value correspondence, repeated payloads at distinct keys, root/prefix and sibling order |
| `mapping_first_error_stops`, `mapping_failure_cannot_publish`, `mapping_first_error_mutant_is_observable` | Error injection at every callback ordinal; no later callback or partial public result |
| `mapping_preserves_shape_and_mode` | Neutral and typed empties, dangling topology, both specializations, unchanged shared source |
| `relative_spans_compose_exact_path` | Relative compressed-span traversal equals independent complete byte-path observations |
| `scaffold_count_is_bounded_by_event_points`, `changing_span_lengths_preserves_scaffold_work` | Long unary stems do not create a scaffold/frame per byte; compare against branching fixtures |

The span model assumes correct segmentation at values, branches and dangling
endpoints; the native witness must establish this binding. It does not prove
physical PathMap allocation behavior. Compression requires separate structural or
allocation evidence, not merely identical ACT bytes. Existing scaffold-erasure and
guarded-ownership obligations apply to partial destination and source cleanup;
native panic, small-stack and rejection tests remain required. Public materialization
must not be introduced at every nested component attachment.

J-215 binds these obligations to five `payload_mapping_tests` witnesses and
`public_materialization_preserves_topology_values_and_metadata`. J-215 qualified
29 closed EPR declarations. Compilation and independent kernel checking precede Rust
implementation. The mapper transfers Set/Empty ownership directly; Map uses guarded
source/destination zippers and compressed relative spans. The generated differential
observes every logical byte path independently, not just stored keys. Error and panic
injection check exact callback prefixes; existing retirement witnesses support the
cleanup boundary but do not constitute a Rust leak-freedom proof. The public witness
checks typed modes, metadata set before admission, exact EPM1 snapshot bytes, deep
payloads and topology on 256 KiB, with one finish per Map value and none for Set/Empty.
Retained-destination measurements are not peak-allocation or throughput evidence.
General decoder routing and gate 5917 remain separate required work.

### Planned snapshot admission (task 8516)

Preregistered after `91879d500`, before changing the admission implementation.
`snapshot` contains mode, topology and ordered bindings. Key identities are abstract
natural numbers, and validation is an opaque function returning the existing error
code or success. The model assumes that resident and planned payloads expose identical
ordered keys; it does not prove canonical-path validation, codec completeness or the
whole protobuf parser. Admission checks mode first, then keys, and commits only after
success. Outer metadata is not part of the replaced snapshot.

| Closed declaration | Required native admission witness |
|---|---|
| `key_validation_ignores_payload_representation`, `snapshot_admission_commutes` | Generated resident/planned acceptance, exact errors, modes and snapshot equivalence under strict and deferred validation |
| `snapshot_rejection_preserves_parent` | Invalid keys and incompatible modes leave prior topology/bindings and metadata unchanged |
| `snapshot_success_installs_exact_image` | Exact topology and typed-empty mode replacement; outer metadata/remainder unchanged |
| `mode_conflict_precedes_key_validation`, `key_validation_before_mode_mutant_is_observable` | Conflicting incoming mode with malformed keys reports mode error, not key error |
| `neutral_mode_accepts_every_mode` | All nine current/incoming mode combinations, including neutral reset |
| `first_key_failure_stops_validation` | Multiple bad keys select the same first error in zipper order |

Native tests must additionally exercise guarded failure retirement on a small stack.
Canonical-value framing errors still belong to the pending decoder and occur before
this boundary. Validation-before-later-field precedence requires generated decoder
integration tests after routing; these helper tests cannot establish that routing.
Legacy field-1/field-8 union and general parser transitions remain separate obligations.

J-216 raises the exported EPR count to 37, compiled and independently kernel-checked
before implementation. `epm_snapshot_admission.rs` binds these statements with four
witnesses: `planned_and_resident_admission_match_generated_keys`,
`all_modes_preserve_typed_empties_and_replace_instead_of_union`,
`root_and_first_key_errors_preserve_mode_precedence_and_deferred_policy`, and
`dangling_topology_and_rejected_deep_snapshots_retire_on_small_stack`.
The shared-policy differential is supplemented by fixed diagnostic strings and an
independent nine-cell mode truth table. Fresh-arena adoption checks the retained
barrier; deep rejection retains a shared parent and checks its topology before final
cleanup. Existing planned-owner tests supply independent unique retirement coverage.
The feature-enabled 25-test selection passes with 1,024 property cases, seed 5834.
Full decoder and parser-gate qualification are not inferred from this selection.

### Legacy ingress staging and normalization (task 8516)

Preregistered after `da67180c4`, before legacy ingress implementation. A concrete
accepted bare-escape list is normalized by the existing field-8 reader: bytes
`0f 07 2a 05 a2 01 02 0a 00` become `0f 00 00`. Therefore validation alone does
not authorize copying every accepted path unchanged. The new route must preserve
the old normalized key, not merely its decoded value. This is a legacy ingress
requirement, not authority to change snapshot key identity or the accepted language.

The formal model treats framing as ordered key/error events and normalization as a
fallible key-to-key function. A reversed private accumulator refines the recursive
reference. Published set membership is an extensional union; its list notation is
a specification, not a proposed production Vec representation. Byte-codec equivalence,
topology-prefix closure and physical trie ownership are separate native obligations.

| Closed declaration | Required native witness |
|---|---|
| `legacy_stage_refines_reference`, `legacy_staging_ignores_representation` | Existing decoder/re-encoder versus planned ingress on generated framed keys and every canonical family; exact success/error and normalization |
| `legacy_field_failure_is_atomic`, `legacy_first_frame_error_stops` | Malformed key/header after valid prefixes; prior parent and metadata unchanged; first error retained |
| `legacy_map_mode_precedes_fields` | Typed-empty and populated Map rejection precedes malformed field bytes |
| `legacy_empty_field_preserves_owner`, `legacy_merge_preserves_metadata` | Empty field retains neutral or typed Set mode, topology, enclosing metadata and remainder |
| `legacy_success_is_membership_union`, `legacy_duplicate_keys_are_idempotent` | Overlap, repeated normalized aliases and insertion order produce the exact old set keys |
| `legacy_normalization_is_observable` | Accepted bare-escape list alias is refiled under its normalized split key; raw-copy mutant differs |
| `legacy_split_metadata_is_outer`, `legacy_leading_escape_dispatch_mutant_is_observable` | Compare declared and validated metadata for leading escaped and mixed split segments; the first segment's metadata is not the outer list's metadata |

The normalization oracle remains the preexisting semantic decoder followed by its
encoder. The implementation reuses the validator's already-decoded bare-escape root,
after full validation completion and before publication. No claim of complete
Rust/codec refinement follows from the abstract normalization premise.

The two metadata dispatch statements were kernel-checked before correcting the
declared metadata scanner (`legacy-formal-r3`, 49 closed declarations). They model
the distinction between bare and split paths; they do not prove the byte classifier.
Native differential witnesses bind that classifier to accepted path bytes.

J-217 binds this extension to seven `epm_legacy_ingress.rs` witnesses and records
17/17 instrumented and 26/26 default passing selections. The depth-8,192 helper
uses an independent exact-byte oracle on a 256 KiB stack, not the old general
decoder, whose repeated semantic-oracle run timed out. Full end-to-end decoding
at that depth and unchanged gate 5917 remain explicit cutover obligations.
The opaque-import segment-program defect discovered by the first tag-1 fixture
is recorded as campaign bug 8518; the decoder fixture uses the actual owned
expression planner. No import defect or general decoder acceptance is discharged
by these helper passes.

### Generated decoder control composition (task 8516)

Preregistered after helper checkpoint `8b5c7d9c7`, before owned-frame routing.
`ProtobufDecoderComposition.v` has 26 closed declarations, independently kernel
checked before implementation (12.793 seconds service, 350.9 MiB peak, zero swap,
2 GiB hard cap). Its first invocation failed because the service PATH did not
contain coqc; the passing run uses the installed absolute paths. This is an
abstract continuation/control model, not a new protobuf parser.

| Closed declarations | Required implementation and native binding |
|---|---|
| `mapped_frame_preserves_control`, `mapped_frames_preserve_context_order`, `selected_error_and_contexts_are_preserved` | Owned Node/Attach/EPM records preserve kind, absolute remaining-byte limit and nearest-first pending attachment locations; compare frozen reader's exact errors |
| `completed_body_restores_exact_parent`, `unfinished_body_cannot_restore`, `absent_parent_cannot_restore`, `enter_preserves_admission_depth`, `nested_inputs_restore_lifo` | Nested EPM value input is saved/restored LIFO only after full consumption; malformed nested bodies and split buffers retain consumption/error observations |
| `snapshot_policy_uses_preincrement_depth`, `successful_admission_balances_depth`, `failed_admission_has_no_continuation`, `admission_without_snapshot_is_rejected` | Preserve validation policy at snapshot start, decrement only after successful admission, never resume parent after refusal; strict/deferred and nested map controls |
| `routing_composes_payload_refinement`, `first_error_leaves_suffix_unvisited` | Frozen/schema/property differential compares result, selected error, consumed bytes and unvisited suffix; POS/PPA/DCC/EPR supply payload premises, not assumed byte-parser equivalence |
| `reopenable_child_is_not_frozen`, `freeze_only_at_consuming_boundary`, `freezing_preserves_denotation`, `reopen_preserves_existing_builder` | Duplicate singular, same-oneof and repeated map-entry values retain builders; freeze only at completed component, EPM value, legacy member or public output consumption |
| `guards_preserve_live_arena`, `projection_and_retirement_use_live_arena`, `final_projection_failure_is_not_published`, `projection_failure_trace_has_no_publication`, `success_retires_guards_and_arena_before_publication`, `every_guard_has_one_retirement_per_owned_slot` | Machine arena outlives projection and guarded frames; successful and rejected deep inputs retire without publishing a partial result; instrumented zero-intermediate-finish and small-stack checks |
| `reversed_context_mutant_is_observable`, `premature_freeze_mutant_loses_reopenable_form` | Distinguishing nested error contexts and nonempty duplicate-field fixtures must fail under the wrong order or early-freeze route |

The one-step payload correspondence and freeze denotation are explicit premises.
Input identities and error codes are abstract observations, not parsed bytes.
The final retirement trace is a specified protocol whose Rust binding remains
to be tested; no Rust lifetime, allocation or whole-decoder proof is inferred.
The unchanged 5917 depth gate and explicit 8,192-deep ENot end-to-end check remain
required, alongside shallow/wide resource comparisons and full acceptance.

The production routing is now implemented and J-218 records 43/43 instrumented
focused passes. Five `protobuf_decoder_control.rs` tests bind general depth-8,192
decoding, deep malformed-frame cleanup, unique partially populated branching Map
retirement, nested input restoration, exhaustive current Var alternatives and a
literal nearest-first diagnostic. The existing thirteen frozen-oracle tests bind
descriptor/merge/oneof/metadata/error/consumption behavior; owned-slot and family
adapter tests supply the corresponding payload premises. A contiguous-input
fixture failed to exercise the general machine and is retained as negative
evidence; its fragmented replacement retains depth and expected bytes.
The legacy top-level canonical fast path's borrowed-Buf observation is explicitly
unchanged, not covered by a new universal consumption claim. Full acceptance and
resource comparisons remain open; the protocol model is not a Rust lifetime proof.

## EPM stateful ownership retirement (EOR, task 5834)

The initial `EpmOwnedRetirement.v` ownership layer has 24 exported closed statements
and two helper lemmas. The resident-cache extension below raises the exported count to 32.
Compilation and independent kernel checking completed before the shared provider
implementation. It extends the existing reverse-prune trace model; it does not
replace that algorithm or establish a Rust/compiler refinement theorem.

Native witnesses in `rspace++/tests/guarded_pathmap/stateful_retirement.rs`:
P = generated static/stateful trace erasure and independently ordered real values;
L = lazy factory and uninstalled barrier disposal; C = CoW sibling preservation;
D = 8,192-level branching and topology-only cleanup on a 256 KiB stack;
F = subprocess fail-stop controls for manufacture, retirement and disposal panics.
The shared facade consumes a raw root under quarantine. Q denotes the seven
pending-decoder witnesses in `models/tests/support/epm_owned_retirement.rs`: deep
Map abandonment before/after attachment, Set/topology-only abandonment, final
rejection, transfer/replacement, frame unwind, guard layout, and generated
abandonment/rejection schedules. Q explicitly checks retained writers and deep
scaffold state before dropping, so premature Discard cannot satisfy the depth gate.
R denotes the seven `planned_epm_ownership_tests` witnesses: arena-barrier validity
and reuse, branching owner retirement, lazy raw-ingress bootstrap and public
normalization, builder unwind, returning length refusal, generated-mirror replacement
and projection rejection, and construction-arena disposal. R's synthetic invalid
handle is a test-only unwind injection, not an accepted production handle. The
length refusal is a separate ordinary-error witness. Native qualification is
recorded in J-211; the binding itself is not a test-pass claim.

| Obligation | Formal statements | Native binding and scope |
|---|---|---|
| Arena-backed barrier | `arena_barrier_reuses_nonempty`, `arena_barrier_bootstraps_empty`, `arena_barrier_returns_valid`, `arena_barrier_preserves_existing`, `arena_barrier_repeated_requests_reuse`, `arena_extension_preserves_barrier` | R checks empty/nonempty bootstrap, repeated manufacture, preservation under arena extension, topology-only cleanup, and no per-map neutral allocation. The count model assumes same-arena input handles and append-only arena growth; the bounded empty-Par score source audit is separate from the count proof. No Rust refinement is claimed. |
| Provider erasure and validity | `stateful_provider_preserves_every_event`, `stateful_provider_preserves_valid_same_arena_values`, `stateful_terminal_drain_preserves_existing_semantics`, `repeat_scaffold_needs_only_validity` | P compares observer events with the static adapter, checks one-byte prune width and exact real values. L checks absence of factory calls for empty/root-only maps and disposal before installation. Stateful test values have no Default and carry one arena identity. |
| Affine settlement | `every_settlement_consumes_source`, `live_source_has_exactly_one_terminal_event`, `consumed_source_cannot_retire_or_transfer_again`, `retirement_and_transfer_are_exclusive` | Q binds consuming pending-map/writer slots with balanced payload creation/drop counts. R exercises planned-node and generated-mirror cleanup on a small stack; it does not count every real destructor. Successful public materialization remains separate. |
| Zipper adoption | `zipper_adoption_preserves_subject_and_authority` | Q exercises guarded map/writer handoff. Pinned PathMap 0.3.4 source review establishes the no-callback empty-root conversion premise; the abstract theorem does not prove upstream code. |
| Replacement | `replacement_retires_old_before_adopting_new`, `replacing_with_absence_retires_without_publication` | Q exercises owned replacement and latched Discard. R replaces the generated EPM field and rejects its projection with retained Map/Set topology; full generated PDA oneof routing remains pending. |
| Frame/arena disposal | `scope_disposal_consumes_each_owned_slot_once`, `scope_disposal_preserves_order_and_drain_traces` | D, Q and R exercise branching drain, partial pending frames, builder unwind and planned-node/arena teardown. Native small-stack observations are not a Rust lifetime proof. |
| Returning errors | `returning_failure_preserves_selected_error`, `refusal_and_projection_failure_do_not_publish` | Q pins deferred noncanonical rejection and first-semantic-error precedence. R covers planned snapshot-length overflow and generated EPM projection rejection; full generated PDA projection remains pending. |
| Terminal cleanup failures | `cleanup_panic_is_terminal`, `cleanup_allocation_failure_is_terminal`, `retirement_reuses_existing_raw_drop_quarantine` | F requires SIGABRT at all three callback sites, not merely test failure. C checks sibling survival. Allocation failure is inherited from the existing quarantine model; no new allocator-failure injection is claimed. |

Premises: provider state remains valid and every barrier belongs to the same live
arena; value manufacture, retirement and real-value disposal must be bounded and
non-panicking in production. Test callbacks deliberately violate the latter premise
to exercise quarantine. Affine model slots must not be duplicated, and transfer
recipients must already own a cleanup guard. Existing terminal-root authority and
reverse-prune topology assumptions remain prerequisites. The abstract model does
not prove arena lifetime, private PathMap callees, arbitrary user-defined payload
Drop, serialized bytes, full parser equivalence or production performance.
Q's tracked payload uses ordinary Drop counters: this proves balanced test ownership,
not dispatch through the named disposal policy. The sealed production policy admits
only unit, resident Par and planned handles. The layout witness compares retained
writer size against the existing writer/provider tuple for these types; it is not
a throughput or allocator benchmark. The short-lived map owner has an explicit
ownership discriminator (eight additional stack bytes for unit on the tested target),
not a per-PDA-frame or heap allocation. Successful transfer into raw public EPathMapRepr remains the
pre-existing, separately audited frontier, not a guarded-recipient theorem binding.

### Resident storage and cache coherence extension (task 8516)

Preregistered after `8db4a76f5`, before the resident-storage Rust migration.
Reuse `PathMapReversePruneTeardown.v`'s immutable wrapper clone, nonfinal release,
shared `make_mut`, aborted detach, cross-anchor sharing and final-only retirement
facts, and EOR's guarded transfer facts. Do not duplicate those models. The new
EOR layer labels each optional cache with the image it describes. It proves
cache/image coherence and preflight no-op preservation, not the serialization
function, Arc internals, allocator behavior or arbitrary metadata transactions.

| New closed declaration | Required witness in `epm_resident_ownership.rs` |
|---|---|
| `clone_owner_shares_image_identity_and_caches` | Cold and warm clone families share one storage identity and both cells |
| `mutation_begin_clears_both_caches` | Both cells are empty before the mutable representation is exposed |
| `mutation_begin_preserves_image_and_metadata` | A test interruption at mutation entry leaves values and maintained metadata unchanged |
| `interrupted_mutation_begin_has_coherent_caches` | Interrupted detached owner remains cold while its sibling retains the original image/caches |
| `prepared_image_change_has_coherent_caches` | Recomputed snapshot/layout agree with the mutated representation, not the old image |
| `shared_mutation_preserves_sibling_and_cache_coherence` | Generated Set/Map alias mutations preserve the other sibling's bytes and metadata |
| `preflight_noop_preserves_exact_owner` | Missing removals, existing-path creation, empty extension and absent greatest values retain identity, metadata and caches |
| `uncleared_cache_mutant_is_incoherent` | Warm-cache mutation controls compare encoded bytes before/after a genuine change |

Additional native ownership bindings must cover unique/shared Set, Map and
topology-only retirement on a small stack, rejected incoming representations,
replacement, algebraic root swaps and consuming transfers. The claim excludes
transactional restoration of metadata after arbitrary mid-mutation panics; the
mutation-entry witness interrupts before image or metadata changes.

The six witnesses are bound by the maintained runner and manifest. The no-op
claim applies to read-only preflights that establish absence, not every native
operation returning false. In particular, a dangling PathMap leaf can contain an
empty-child sentinel with zero logical children that native branch removal still
prunes. Both removal APIs therefore gate only on path existence. Direct native
parity checks cover the result, surviving topology and shared sibling for both
specializations. A false native result after detach may leave a cold cache; that
is not a semantic change or a violation of the restricted preflight theorem.

The admission witness also exercises the raw-input wrapper with depth-8,192
topology and a depth-8,192 Par payload. The algebra witness checks typed-empty Set
selection from neutral Empty and exact consumed key/value association. Qualification
and residual proof boundaries are recorded in J-214 of the engineering journal.

## Decoder resource refinements (task 8516, J-222 through J-227)

The affected executable proof inventories are now 63 statements in
`DecodedComponentConstruction.v`, 51 in `ProtobufOwnedSlots.v`, and 365 in
`RPB3Admission.v`. These totals include their older kernels; they are not counts
of new proofs. Qualification checks both the runner counts and named native
test groups. The following additions preserve the earlier semantic obligations.

| Model module and exhaustive added statements | Native binding and proof boundary |
|---|---|
| `FragmentComparison`: `borrowed_is_exact`, `comparison_refines`, `component_order_refines`, `equal_score_keeps_protobuf_tiebreak`, `literal_comparison_has_no_cursor`, `graph_representation_retains_fallback`, `ignoring_tiebreak_mutant_is_detected`, `ignoring_fragment_suffix_mutant_is_detected` | Four tests in `planned_fragment_comparison.rs`: literal/graph differential, all ordering branches, allocation-free literal comparison, and retained arena state. Formal byte sequences abstract the allocator. |
| `GroundLeafConstruction`: `direct_score_refines_tree`, `direct_wire_refines_generated_program`, `complete_component_refines`, `restricted_dispatch_refines`, `recursive_empty_children_still_require_general`, `ground_extra_children_rejected`, `owned_fragment_preserves_prefix`, `owned_fragment_has_exact_payload`, `flattened_fixed_scale_mutant_is_detected` | Four tests in `planned_ground_leaves.rs`: independent sorter/full encoder, scalar boundaries, arity errors and recursive fallback, allocation volume. Generated schema and scalar encoders remain native bindings. |
| `FragmentStoragePublication`: `pack_refines`, `append_refines`, `take_preserves_order_and_clears_owner`, `emission_refines`, `aggregate_refines`, `key_is_injective`, `reusable_buffer_does_not_depend_on_prior`, `unique_insert_preserves_prior_and_payload`, `duplicate_rejects`, `successful_stream_keeps_input_order`, `failed_publication_has_no_partial_result`, `sorting_by_body_then_appending_a_trailer_can_reverse_order` | Five tests in `planned_fragment_publication.rs` plus Bag unique-writer tests: reusable key buffers, composite programs, prior-publication differential, family/cache coverage, and semantic-order versus key-order counterexample. No new sorting premise is introduced. |
| `PooledConstructionStorage`: `appended_span_is_exact`, `append_preserves_admission`, `append_preserves_earlier_span`, `rewind_restores_prefix`, `nested_rewind_preserves_outer`, `pooled_owned_emission_refines`, `abandoned_offsets_must_not_escape`, `link_roundtrip`, `link_encoding_injective`, `addressable_link_fits`, `cold_indirection_preserves_view` | Five tests in `planned_pooled_storage.rs`: reallocations, owned/shared identity, nested rewind, stale-offset reuse, checked links/layout, canonical leaf independence, and warmed allocation counts. The formal start/extent span binds to native start/end via subtraction. |
| `RetainedAdmissionHistory`: `latest_append`, `empty_consistent`, `cached_success_preserves_consistency`, `exact_step_error_and_history`, `exact_trace_first_error_and_accepted_history`, `rejected_candidate_preserves_retained_history`, `unrelated_slot_is_unchanged`, `retaining_first_instead_of_last_is_detected` | Six tests in `admission_retained_history.rs`: independent cached acceptance traces, strict maps versus duplicate components, body ties, consumed continuations, forged history and incoming-error precedence, and exactly two slot-table allocations. Formal equivalence assumes well-typed retained occurrences; rejection's actual unchanged history is a native snapshot observation. |
| `IndirectSnapshotContinuation`: `relocated_transition_preserves_payload_or_exact_error`, `successful_resume_reuses_same_identity`, `arbitrary_suspend_resume_trace_is_exact`, `arbitrary_successful_trace_keeps_one_owner`, `boxed_worklist_observes_same_flat_payload_order`, `moving_to_continuation_and_back_needs_no_second_identity`, `completion_observes_original_retirement_payload` | Three tests in `protobuf_snapshot_continuation.rs`: common/rare frame layouts, allocation-free box transport, and actual multi-value decoder identity/input restoration. Existing decoder-control tests cover nested truncation, exact errors/remaining input, and small-stack pending-map teardown. The abstract cell model does not prove the allocator or Rust destructor. |

The broader qualification also exposes a census assumption: an inferred
`vec![0; length]` can construct an ordinal table, not just a byte bitset.
`bit_vector_canonicity` retains every raw hit, separately requires the two byte
producers, and attributes the third only when the initializer, enclosing
`freeze_owned_segment` function, `PageFrozenData` struct and `Box<[usize]>`
field agree. Mutating that field to bytes, changing its owner/constructor, or
adding another zero-fill site must not silently inherit the attribution.
This changes test classification, not bitset or ordinal production behavior.

## Original workspace-regression completion gates

This checkpoint is complete only when all of the following hold:

- both TLA+ controls pass; the replay mutant, five FFI safety mutants, and FFI no-fairness mutant
  fail for their named obligations; and the second legal publication witness is reachable;
- every RR, FFI, and PT row is backed by a passing named executable property;
- all RR and FFI formal rows appear exactly once in the mechanical binding registry and name an
  existing executable property;
- every PT row names a direct, non-unwind-based parser-totality property;
- the panic-policy gate contains only the narrowly justified production FFI interceptor entry;
- the focused replay and parser suites pass under zero-swap resource caps;
- the exact release workspace gate passes;
- the living stack-safety report, engineering journal, consensus register, and pgmcp graph record
  the evidence and any remaining red FFI obligations without claiming premature completion.
