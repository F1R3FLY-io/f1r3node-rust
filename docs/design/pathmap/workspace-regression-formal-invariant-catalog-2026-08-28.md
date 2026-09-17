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

**FORMAL-FIRST; FIRST NATIVE MIGRATION QUALIFIED, RESOURCE LADDER OPEN.** The old shared fixture constructs a resident
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
UEL-08's geometric native cost measurements and the nested-EPathMap slice remain open; this
qualification does not close task 6444 or the whole-workspace gate.

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
consumers; it does not close UEL-08's resource measurements. J-169 records commands and log hashes.

## Fixed-key singleton-map construction — 2026-09-17, task 6444

**FORMAL-FIRST; NATIVE BRIDGE AND MIGRATION QUALIFIED, RESOURCE LADDER OPEN.** `SingletonMapConstruction.v` has compiled and
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
| SMC-08 | Fixed-size prefix and bounded varints imply linear emitted-byte length. | Geometric phase measurements remain required. This is an output-size bound, not proof of runtime or allocator costs. |

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
The named fixture consumers subsequently passed their migrations as recorded above. SMC-08's
phase-separated measurements and task 6444 remain open.


## Split-carrier header prefilter — 2026-09-17, tasks 6444 and 5834

**FORMAL MODEL AND NATIVE SUCCESSOR QUALIFIED; RESOURCE COMPARISON OPEN.** Profiling the escaped
nested-list fixture identified full expression decoding in `split_carrier_list` just to reject
a non-list expression. The proposed prefilter reads the resident Expr endpoint's borrowed
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
