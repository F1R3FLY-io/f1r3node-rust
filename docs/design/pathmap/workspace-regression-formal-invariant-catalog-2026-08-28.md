# Workspace-regression formal invariant catalog — 2026-08-28

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

This report does not weaken exact `(root, generation)` compare-and-swap authority, make panics part
of a public API contract, or claim that `catch_unwind` contains abort-mode panics, native stack
overflow, out-of-memory abort, or foreign faults.

## Formal evidence before executable changes

| Model | Control result | Required negative controls |
|---|---|---|
| `formal/tlaplus/stack_safe_pda/RSpaceRecordReplayAuthorityLifecycle.tla` | SANY pass; TLC 7 generated / 6 distinct states, depth 6 | skip-reset acceptance violates `ReplayCommitRequiresReactivation` |
| `formal/tlaplus/stack_safe_pda/RSpaceStateImportFfiBoundary.tla` | SANY pass; TLC 6,586 generated / 4,661 distinct states, depth 20; `EveryStartedCallReturns` passes under phase fairness | early capability exposure violates `CapabilityOutputRequiresEncodedSuccess`; escaped unwind violates `NoOrdinaryUnwindEscapesBoundary`; publication misclassification violates `PublicationResponseIsTruthful`; usable staged-open violates `StageEncodingFailurePoisonsMutatedOpen`; duplicate in-call publication violates `PublicationIsMonotoneAndUnique`; removing fairness violates `EveryStartedCallReturns`; a separate witness reaches two legal sequential publications |

`scripts/check-stack-safe-pda-proofs.sh` runs both controls and requires each mutant to fail for its
assigned invariant. A control-only result is not sufficient evidence.

## Record/replay authority model

### Transition contract

The bounded lifecycle is deliberately narrow and complete:

```text
B@g0 --play prepare/commit--> T@g1
T@g1 --skip-reset replay attempt--> reject with no authority mutation
T@g1 --activate known B--> B@g2
B@g2 --fresh replay prepare/commit--> T@g3
```

Target-root equality is not authority. A plan prepared at `B@g0` cannot be reused at `B@g2`, and a
replay history still positioned at `B@g0` cannot commit while the shared authority is at `T@g1`.

### Exhaustive invariant and property mapping

| ID | Formal invariant/property | Executable property family | Required observation | Status before test correction |
|---|---|---|---|---|
| RR-01 | `TypeInvariant` | `memory_checkpoint_publishes_all_namespaces_together`; `target_equal_replay_requires_reactivation_and_reuses_exact_object_closure` | roots, generations, object namespaces, and lifecycle states remain typed | PASS |
| RR-02 | `CurrentRootIsVerified` | `memory_checkpoint_publishes_all_namespaces_together`; LMDB success/reopen cases | every selected current root has verified membership | PASS |
| RR-03 | `VerifiedRootsHaveCompleteClosure` | `missing_or_malformed_cold_leaf_leaves_exact_pre_state`; reopen family | verified roots have complete history and typed cold closure | PASS |
| RR-04 | `ContentKeysAreImmutable` | `immutable_collision_leaves_exact_pre_state`; idempotent same-value case | same key cannot acquire a different value; same key/value is idempotent | PASS |
| RR-05 | `NoUnpublishedCheckpointObjects` | success, collision, missing-leaf, and stale-conflict snapshots | failed or partial checkpoint objects are not externally visible | PASS |
| RR-06 | `GenerationAdvancesExactlyOnActivation` | no-op, success, reset/reactivation, and ABA cases | generation advances once per activation and never for prepare/no-op/refusal | PASS |
| RR-07 | `SkipResetIsRejectedWithoutAuthorityMutation` | `target_equal_replay_requires_reactivation_and_reuses_exact_object_closure`; `same_generation_plans_conflict_without_partial_visibility` | target-equal stale replay is rejected and current root, generation, activation count, verified-root catalog, history objects, and cold objects remain exact | PASS |
| RR-08 | `ReplayCommitRequiresReactivation` | `target_equal_replay_requires_reactivation_and_reuses_exact_object_closure`; `epathmap_heavy_record_then_replay_reproduces_the_log` | replay commit is preceded by reset/reactivation and never by accepted skip-reset | PASS |
| RR-09 | `ReplayPreparationUsesFreshExactBase` | `target_equal_replay_requires_reactivation_and_reuses_exact_object_closure`; `aba_root_reactivation_does_not_revive_an_old_plan` | replay prepares only after reset and binds the new `(B, generation)` pair | PASS |
| RR-10 | `ReplayPreparationLeavesAuthorityUnchanged` | `target_equal_replay_requires_reactivation_and_reuses_exact_object_closure`; `preparing_checkpoint_leaves_authority_byte_for_byte_unchanged` | preparation alone changes no current root, generation, activation count, verified-root catalog, history namespace, or cold namespace | PASS |
| RR-11 | `ReplayTargetMatchesPlay` | EPathMap replay root equality | replay publishes the same target root as play | PASS |
| RR-12 | `ReplayReusesAuthenticatedPlayClosure` | `target_equal_replay_requires_reactivation_and_reuses_exact_object_closure` plus EPathMap event-log exhaustion | after removing only the expected current-root/generation record, replay has byte-identical history, cold, and verified-root namespaces and introduces no unreachable object | PASS |
| RR-13 | `ReplayLifecycleEventuallyCompletes` | EPathMap test reaches replay checkpoint and final equality assertions | valid reset/fresh-prepare replay is live, not merely safe | PASS |

The executable correction must therefore capture the initial root, checkpoint play, reactivate the
known initial root through `reset`, rig and evaluate replay, checkpoint replay, and compare it with
the saved play root. Permitting a stale or idempotent target-root commit is forbidden.

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
| PT-01 | checkpoint descriptor | arbitrary 0--1,023-byte input |
| PT-02 | generation PREPARE preflight | arbitrary 0--1,023-byte input |
| PT-03 | generation PREPARE view | arbitrary 0--1,023-byte input |
| PT-04 | generation COMMIT | arbitrary 0--1,023-byte input |
| PT-05 | object index | arbitrary 0--1,023-byte input |
| PT-06 | pack header | arbitrary 0--1,023-byte input |
| PT-07 | retention manifest | arbitrary 0--1,023-byte input |
| PT-08 | two-slot catalog selection | every arbitrary exact 672-byte catalog image |
| PT-09 | aggregate WAL-05/RES-01 compatibility family | all eight calls in one generated case |

Every parser is total over bytes by returning success or a typed error. None uses panicking as an
accepted outcome.

## Completion gates

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
