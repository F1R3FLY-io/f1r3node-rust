# Stack-safe PDA, EPathMap, and RSpace Bag proof kernel

This development proves the algorithmic obligations shared by the generated Rust machines.

- `compile_run_equivalence` proves that an explicit post-order instruction/value-stack machine equals the mutually recursive fold for every finite typed tree and every algebra. Clone, equality, hashing, ordering, debug formatting, bincode/protobuf encoding, encoded length, clear, and teardown are instances of that theorem.
- `decoder_machine_equivalent_to_recursive_rebuild` instantiates the same machine with the tree-building algebra and proves exact reconstruction.
- `compiled_program_has_one_instruction_per_node` proves linear work and one completion instruction per node.
- `EPathMap.v` proves neutral-empty mode laws, set lattice laws, exact map-value conflict behavior, and the prefix-restriction subset law.
- `Bag.v` models an exact-key `PathMap<Multiplicity>` Bag by its total count function with zero as absence; the concrete implementation retains only its finite support. It proves commutative-monoid sum, distributive max/min lattice laws, truncated-subtraction conservation, exact-key independence from prefix relationships, canonical-delta construction, and normalized atomic apply/revert. Its candidate-projection bridge proves that only stored nonpersistent candidates contribute removals and that selection permutations construct the same normalized delta.
- `RSpaceCommit.v` models the accepted-match transaction above that Bag boundary. Continuation freshness is an opaque equality-only epoch capability: the model assumes no integer width, ordering, increment, or wraparound law. The development proves deterministic acceptance, exact rejected-state identity, stale-epoch and missing-witness refusal, exact accepted datum/continuation/join publication, COMM publication only after a complete accepted state, unrelated-group preservation, and equal play/replay outcomes. It proves the transition relation used to review the Rust implementation; it does not verify `Arc` allocation, pointer identity, mutex implementation, or the Rust compiler.
- `GenerationCheckpointPublication.v` is the formal-first contract for fresh PathMap-DAG generation publication. For arbitrary canonical-object and semantic-state types, its 13 closed capstones prove exact duplicate-rejecting merge of the retention-manifest singleton into the strictly ordered retained-union inventory, exact current-closure (`L`), retained-union (`R`), and checkpoint-DATA (`D`) descriptor inventories, the revised wire-layout equations, affine and continuous writer authority, exact prepare/index membership, recovery only after the post-catalog directory barrier, acknowledgement only after restart-grade physical recovery and unchanged semantic readmission, and zero semantic execution charge for physical checkpoint maintenance. `RSpaceGenerationCheckpointPublication.tla` checks the finite crash/refinement state machine through old, torn, complete-new, and damaged-complete slot images; it preserves the original publication outcome across later restart recovery and never silently rolls a selected damaged generation back to the old one. The proof gate rejects 18 independently targeted mutants and requires five reachable non-vacuity witnesses: acknowledgement, old recovery, new recovery, published failure, and indeterminate publication. These proofs specify the runtime changes before implementation; actual `fsync`, directory-handle security, checksums, bounded parsers, partial-write behavior, and semantic admission remain executable obligations in the model-derived property catalog.
- `formal/verus/bag_checked_delta.rs` binds those delta equations to the executable `u64` acceptance boundary: underflow and overflow return `None`, every accepted cell has the mathematical result, and swapping a normalized successful delta restores the original count. Verus runs with `--no-cheating`.
- `formal/tlaplus/stack_safe_pda/RSpaceBagCommit.tla` exhaustively checks finite play/replay schedules for exact selected-removal and unconsumed-arrival deltas, persistent presence witnesses, opaque continuation freshness and metadata, rejected-state immutability, complete datum/continuation/join publication, COMM logging only after acceptance, and play/replay result equality. The checked model has 710,528 distinct states and reaches depth 5.
- `formal/mcrl2/stack_safe_pda/` proves the validation/private-construction/complete-publication process branching-bisimilar to its atomic reference contract. Independent modal properties exclude partial publication, pre-commit COMM logging, and a commit after stale-ticket selection. Internal `tau` steps deliberately do not resolve a later visible play/replay choice.
- `EPM1.v` models the actual envelope rather than an opaque suffix: canonical base-128 length fields, the length-framed ACTree03 arena, the ordered count-delimited generated-PDA value stream, exact end-of-input checks, mode well-formedness, and positional endpoint/body association. ACT values are zero occurrence sentinels rather than body indexes. It proves framing round trips, malformed/truncated/trailing rejection, exact sentinel cardinality, positional body coverage, and equality of generated-PDA versus recursive value bodies. `formal/rocq/actree03_completeness/Epm1PositionalAssociationV6.v` joins that envelope law to the concrete ACTree03 writer/parser, CoW/address independence, owned-zipper publication, count agreement, and error precedence. `formal/verus/epm1_positional_association.rs` checks platform-width arithmetic and one-step lockstep, while `formal/tlaplus/stack_safe_pda/Epm1PositionalAssociation.tla` checks the finite suspension/publication lifecycle and twelve causal mutants.
- `formal/verus/epathmap_canonical_worklist.rs` binds the canonical-key implementation shape to machine-checked Rust arithmetic: relative snapshot subranges cannot overflow or escape their enclosing region, and cursor-driven worklist validation is equivalent to recursive validation of the same result sequence. Verus runs with `--no-cheating`.
- `RMP2.v` models the derived in-memory Rho Match Path v2 (RMP2) boundary for an opaque `ParBag`. It proves that the marker, two big-endian `u64` counts, and 32-byte digest form exactly one 49-byte token; exact queries compare that whole token, a dynamic nested interior becomes a whole-token wildcard, and unsupported shells fail closed. Dynamic-shell classification is independent of the summary, so it cannot force eager hashing of every nested suffix; summary congruence is used only when an opaque token is emitted. The no-false-negative theorem covers every successfully compiled exact or dynamic shell. Its formal-assurance bridge separately defines the common semantic-field observation produced by the resident generated cursor and the provenance-admitted canonical-region cursor, proves identical field projection under explicit semantic correspondence, defines adjacent-only occurrence coalescing and proves exact cardinality preservation, and proves that nested `Par` projection observes only the opaque summary and two root metadata fields. A separate theorem proves that canonical candidate comparison depends only on the canonical identity hash and full identity bytes, never on derived RMP2 bytes. `formal/verus/rmp2_par_bag_transcript.rs` proves the same boundaries with `--no-cheating`: semantic field correspondence, the production pending-run coalescer step and final flush, nested-`Par` opacity, transcript equality, count arithmetic, and body-length arithmetic. Neither development claims to verify BLAKE2b-256, arbitrary protobuf parsing, generated Rust, allocator behavior, or the whole runtime matcher. Focused Rust gates compare actual semantic event streams, exact adjacent-run multiplicities, and the opaque nested-`Par` cutover; broader corpus, malformed-input, query-superset, and forced identity-hash-collision tests discharge the remaining executable boundaries.
- `SpatialMatcher.v` instantiates the PDA theorem with the matcher's state-transformer algebra. It proves equivalence to recursive matching for every finite matcher tree and initial `FreeMap`, retry/disjunction and negation snapshot isolation, consistent/conflicting binding behavior, and semantic preservation of the singleton PathMap owned-zipper move optimization. Its retained-query section factors the production soundness boundary into exact-prefix and residual-query superset obligations, then proves that their conjunction cannot remove a spatial success and preserves the first spatially admissible candidate in canonical list order. The fresh-or-resting extension proves that unioning the arriving-datum candidates with terminals reached by the retained-automaton × authoritative-Bag product cannot hide a spatial success; the executable product remains responsible for establishing its prefix-terminal premise. Its generated-compiler section proves that exact, hole, and ordered RMP2 query compilation preserves every spatial success and that unsupported patterns have no compilation derivation. Its encoded-Expr refinement proves that only independently proven unequal outer tags may reject early; same or unproved evidence delegates exactly, preserving accepted `FreeMap` values and invalid/invariant outcomes. The bridge theorem is explicitly conditional on tag soundness, different decoded variants rejecting, and delegation agreeing with the owned authority. It does not claim to verify protobuf parsing, canonicality, or panic behavior; generated-table differentials and malformed-input subprocess gates discharge those executable boundaries.
- `FreeMapJournal.v` refines the matcher PDA's drive-local undo journal to the snapshot equations in `SpatialMatcher.v`. It proves write/rollback, nested checkpoint, exact delta extraction/replay, validate-before-apply atomicity, AND/OR/NOT isolation, root refusal and abort restoration, and the universal linear bound in completed uncommitted writes. The measured nested matcher has a separately checked constant write bound per level, which yields its conditional linear-in-depth corollary; the theorem does not claim that arbitrary width or retry schedules are bounded by depth alone.
- `formal/verus/free_map_transaction.rs` checks an executable one-key projection of the journal with `--no-cheating`, including reverse rollback, final-value delta extraction, finish/abort, and journal/delta length bounds. `formal/tlaplus/stack_safe_pda/FreeMapJournal.tla` checks the bounded journal/snapshot lockstep, nested control, atomic delta application, Drop restoration, action non-vacuity, and rollback/isolation mutants. The corresponding mCRL2 implementation is branching-bisimilar to its atomic snapshot contract; explicit OR-binding and partial-apply mutants are not bisimilar and violate the no-leak property.
- The same matcher development models the byte-native authoritative Connective PDA separately from protobuf recognition. AND accumulates bindings sequentially and rolls the whole fold back on failure; every OR arm starts from and restores the same snapshot, including a successful arm, because Rholang disjunction binds nothing; NOT always restores before inverting its verdict. The encoded-to-resident bridge is conditional on canonical admission, exact ordered child ranges, nested-singleton routing soundness, and generated-decoder delegation when the cursor has no proof. A singleton Connective envelope enters encoded recursion only when its enclosing child `Par` also proves `connective_used = true`; false or unproved summaries delegate to concrete resident matching. Root metadata alone is not enough to bypass the owned matcher's `ParCount`/`sub_pars` candidate selection: direct encoded matching additionally requires proof that both routes receive the same target domain, and an active but domain-unsafe root delegates to the candidate-selecting authority. Candidate compilation is only a conservative filter, so its independent root gate remains `connective_used`; a false root cannot compile a Connective candidate plan. Consequently parser refusal is not semantic rejection and malformed bytes never enter the encoded job. The exact-scalar bridge additionally states the executable recognizer's proof boundary: once both scalar expression bodies are canonically validated, byte equality is decoded equality, so unequal admitted bytes are a conclusive mismatch; an unproved operand delegates. TLC exhaustively checks a finite nested AND/OR/NOT corpus, cursor order, child/root metadata and target-domain dispatch, malformed/unproved routes, predicate-family routing, exact-scalar routing, and explicit leak/skip/duplicate/reversal/collapse mutants. These results do not claim a proof of the Rust protobuf parser for arbitrary bytes; canonical parser differentials, recursive-oracle comparisons, and malformed admission tests remain the executable obligations.
- `AccountingDemand.v` proves total termination and exact trace refinement of the explicit accounting worklist for every finite recognized process tree. It also proves occurrence-major whole-trace multiplicity replay, saturating COMM counts, sticky unknown demand, and preservation for every stateful channel callback. `formal/verus/accounting_demand.rs` checks the executable cursor-advance and `i64` saturation arithmetic without cheating. `formal/tlaplus/stack_safe_pda/AccountingDemand.tla` exhaustively checks bounded trace/callback/summary prefixes and weak-fair completion, with explicit grouped-multiplicity, family/branch reversal, all-receive-sources, missing-COMM, missing-callback, and dropped-unknown mutants. This layer begins after canonical protobuf recognition: the generated-schema conformance gate, arbitrary-byte cursor totality tests, and frozen recursive-oracle differential discharge the Rust recognition boundary; the theorem does not claim to verify arbitrary-byte protobuf parsing.
- `ChannelImage.v` models the storage-independent RCI2 endpoint language: escaped RPB3-relative keys, positive fixed-width multiplicities, strict endpoint order, exact distinct count, root `connective_used`, prefix parsing, and exact parsing. It proves encoder/parser left inverses, uniqueness exactly on the modeled `Par` equality quotient, exclusion of root `locally_free`, connective separation, multiplicity preservation, canonicality of every admitted endpoint sequence, and rejection at the duplicate/permutation/noncanonical/zero-multiplicity boundaries. The RPB3 key validator is an explicit universally quantified Boolean boundary rather than an axiom; executable Rust delegates that boundary to the production RPB3 validator. `formal/verus/channel_image_cursor.rs` checks the production-shaped cursor, bounded arithmetic, byte unescaping, exact big-endian words, strict counts, and encoder/decoder refinement with `--no-cheating`. Neither proof claims to verify generated protobuf semantics, PathMap internals, BLAKE2b, allocation, or Rust slice-memory semantics; the complete schema corpus, hostile-byte property tests, and tiny-stack codec differentials discharge those executable boundaries.

The RPB3 traversal kernel, scalar Verus helpers and generated-table differential now exist.
They are not a proof of the full generated protobuf semantic reducer: `RPB3Admission.v`
compares Boolean folds of an already-built tree, and its `local_valid` parameter does not
model raw protobuf, descriptors or scores. The full-summary and concrete-cursor obligations
remain required under tasks 6743 and 6744, respectively, within task 6020. The dated
[observation correspondence](#rpb3-observation-correspondence-2026-09-07) below distinguishes
what is proved, what is tested, and what remains unproved.
- `PathMapReversePruneTeardown.v` is the formal-first contract for stack-safe destruction of campaign-owned `PathMap<()>`, `PathMap<Par>`, and Bag tries. It proves exact reverse-postorder payload detachment, one logical edge per immediate-parent-barrier prune, topology-only line/dense/comb coverage without depth limits, exact Par-PDA work transfer, root handling, rejected partial writer/iterator closure, destructive subtree closure, publication/error preservation, and absence of any nonempty unique raw-PathMap Drop at the integration boundary. Its primary ownership refinement is `Arc<AnchorRoot<PathMap<V>>>`: immutable wrapper clones never clone a raw PathMap header or root value, standard Arc finalization elects exactly one anchor teardown, and `Arc::make_mut` is the only operation that creates a fresh registered anchor/header for semantic mutation. Cross-anchor internal PathMap subtrie sharing is modeled separately and preserves the sibling image while the final anchor drains through public zipper CoW. `PathMapReversePruneTeardownMutants.v` kills twenty-seven causal traversal/ownership/publication controls for the twenty-five-invariant catalog, including snapshot-only `Arc::strong_count`, per-wrapper raw headers, manual family counters, unregistered `make_mut`, topology-blind walking, recursive multi-edge prune, early owned-writer Drop, and live detached-subtree release. `formal/tlaplus/stack_safe_pda/PathMapAnchorRootTeardown.tla` exhaustively explores clone, nonlast/final Drop, `make_mut`, abort, and both final-drop orders; four configurations reject the ownership race mutants. `formal/verus/pathmap_reverse_prune_teardown.rs` checks the executable one-edge/root steps and proves edge/payload conservation for the abstract reverse-terminal state machine with `--no-cheating`. These artifacts specify the runtime work before implementation; they do not verify the upstream PathMap node layout, Rust standard-library Arc internals, allocator behavior, panic code generation, or a future Rust integration that has not yet been written.

The theorem kernel is schema-parametric. The generated Rust equivalence manifest binds each concrete schema surface to a theorem and an independent recursive-oracle differential. The manifest test is the coverage proof: a new generated traversal cannot remain converted without a theorem/differential row. PathMap still owns the internal ACTree03 byte grammar and parser. `EPM1.v` preserves that arena byte-for-byte and proves the surrounding framing and positional association law parametrically over the executable zero-marker extractor; the V6 physical refinement binds that law to the canonical writer/parser. Executable PathMap behavior remains checked by the Rust differential and malformed-input suites.

The development contains no axioms, admitted goals, opaque assumptions, Verus `assume`, or external proof bodies.

## Resolved authority scope contract (2026-09-07)

Task 8283 extends `RSpaceCallerAuthority.v` with exact independent database-name
resolution, checked namespace framing, physical-slot isolation, rejection before
opening, and scoped migration-source selection. Its twenty-entry invariant type
is exhaustively bound to checked propositions. The
[extracted executable catalog](../../../docs/design/pathmap/generation-checkpoint-invariant-catalog-2026-08-28.md#resolved-lmdb-scope-isolation-2026-09-07)
records the remaining Rust obligations. The caller and mutant files compile with
36 and 43 closed assumption reports, respectively, and pass independent kernel
checking. The 23 new controls include positive admission, dropped overrides,
missing/duplicate mappings, malformed names, physical aliases, omitted frames or
fields, ambient migration sources and foreign namespace markers. Existing caller
proofs and controls remain unchanged.

The namespace proof imports the existing `ChannelImage.v` fixed-width codec;
the proof runner compiles that dependency first. Physical environment IDs stand
for resolved paths, not string inequality. The model does not prove filesystem
identity, actual LMDB behavior, process-restart durability, or the Rust adapter.
Those boundaries need the model-derived executable properties; unique namespace
bytes alone cannot isolate overlapping physical databases.

## Captured replay archive contract (2026-09-07)

Task 5636 adds a **current-epoch test fixture**, not a reconstruction of the missing
historical deployment archive. `ReplayArchive.v` defines acceptance as the conjunction
of exact archive admission, complete context, a codec fixed point, matching request,
disabled replay shortcuts, actual successful replay and the expected final root.
Its twenty capstones prove the consequences and refusal conditions of that contract.
They do not prove the Rust runtime, protobuf codec, genesis constructor or cryptographic
hash implementation. Those are explicit realization premises, not inferred properties
of an opaque success Boolean. `ReplayArchiveMutants.v` supplies a concrete accepted
example and omission controls, including a permutation-insensitive toy runtime whose
accepted archive must nevertheless preserve the captured byte order.

The implementation belongs under `casper/tests/util/rholang/`, reusing the existing
seven-program corpus. Capture is an explicitly selected ignored test, writes new files
only, and is separate from ordinary read-only verification. A checked-in BLAKE2b-256
checksum binds the whole archive; each protobuf payload also records its byte length
and digest. Review of the fixture/checksum pair and collision resistance are trust
premises. A checksum is not a mathematical proof of injectivity or authenticity.

The complete model-derived obligation catalog, specified **before Rust implementation**,
is as follows. The witness names are implementation requirements until their results
are recorded in the integration report.

| ID | Formal obligation | Required executable witness |
|---|---|---|
| RA01 | Admission loads exactly the expected archive; absence and alteration refuse. | `replay_archive_requires_present_unchanged_files`; `replay_archive_byte_mutations_are_rejected`. Missing paths are read-only failures; altered bytes are checked against the unchanged trusted checksum. |
| RA02 | Integrity and completeness are independent premises. | Full-file digest, exact schema epoch, all seven unique corpus names, mandatory fields and unknown-field refusal in `replay_archive_context_and_corpus_are_complete`. |
| RA03 | The complete archived context realizes the literal pre-root. | Exhaustive `Genesis`/`ProofOfStake` field projection, ordered public validator/vault keys, equality with the selected test genesis configuration and exact actual pre-root in `archived_deploys_replay_without_play_or_caches`. Private keys are never archived. |
| RA04 | The decoded payload re-encodes to exactly the original bytes. | Per-record length/digest, `ProcessedDeploy::from_proto` and typed `to_proto` fixed point in `replay_archive_payloads_preserve_all_fields_and_order`; malformed/non-fixed-point controls. All signed deploy metadata and all event variants are included. |
| RA05 | Every supplied byte position equals the corresponding archived position. | `replay_archive_event_edits_cannot_reuse_original_seal` checks event drop, duplication and guaranteed-distinct permutation against the original file seal. No sorting or COMM-only fingerprint substitutes for the full payload. |
| RA06 | Requested context and actual pre-root equal the archived values. | Decode and pass all `BlockData`, system deploys, invalid-block context, flags and client allocations; reject a changed genesis descriptor or actual pre-root. |
| RA07 | Both caches are disabled, and a cache shortcut is never successful actual replay. | Explicitly clear both runtime cache options before verification and reject either enabled option at the execution boundary; `replay_archive_rejects_cache_shortcuts_and_wrong_roots`. |
| RA08 | Actual replay succeeds and returns the literal archived final root. | `archived_deploys_replay_without_play_or_caches` executes the archived deploys without first playing their source; capture and verification run in different operating-system processes. Wrong-final-root control. |
| RA09 | Failed or absent runtime execution cannot establish acceptance. | `replay_archive_runtime_rejects_cost_status_and_comm_mutations` exercises actual uncached replay with changed cost/status and controlled COMM identity/multiplicity. A parse-only success cannot satisfy this row. |
| RA10 | Complete evidence admits a nonempty valid fixture. | Explicit seven-program floor and successful untouched archive replay. The formal toy fixture is not a replacement for this runtime witness. |
| RA11 | Captured order is not a total execution schedule. | Reuse `rig_is_order_insensitive` (5,040 permutations), `rig_distinguishes_a_different_multiset` and `a_produces_channel_hash_is_not_part_of_its_replay_identity`; do not require harmless permutations or ignored metadata to fail actual replay. |

Verification first checks the immutable archive seal and context, then decodes the full
ordered payload and checks its fixed point. It reconstructs the pinned genesis, disables
both replay caches, invokes the existing runtime with the archived inputs, and compares
the resulting root. This checks a particular captured execution's replay obligations;
it does not prove that parallel re-execution emits the same total event order or count of
standalone IO events. The historical experiment-provenance gap remains separately open.

### Bootstrap replay composition (v2)

The v1 execution disproved its configuration-only genesis-realization assumption:
the same descriptor did not reproduce the captured starting root. Its sealed bytes
and failed qualification remain evidence; they are not silently replaced. V2 must
capture the complete genesis block's processed-deploy list and replay it from a
fresh registry bootstrap before replaying the workloads. This is an archive-format
revision, not a consensus-epoch approval or deterministic-genesis proof.

`ReplayArchive.v` preserves v1 and adds fourteen composition capstones, for thirty-four
printed closed checks. `ReplayArchiveMutants.v` now has twenty-one printed checks,
including composed acceptance without the original workload-genesis premise and six
new omission controls. Both modules and `coqchk` passed under a 2 GiB/zero-swap cap.
The derived realization replaces only the workload starting-state premise; it still
requires actual accepted genesis replay, exact root stitching and compatible runtime
contexts. Concrete codec, integrity, runtime and bootstrap realization remain premises.

The complete new obligation catalog precedes its Rust implementation:

| ID | Composition obligation | Required executable witness |
|---|---|---|
| BR01 | Genesis start is established, not assumed from configuration. | A fresh scoped runtime materializes registry bootstrap with `RuntimeOps::empty_state_hash`; compare actual, archived and existing pinned bootstrap roots. |
| BR02 | Bootstrap, genesis final, workload pre and workload final are separately checked. | `replay_archive_genesis_context_and_bridge_are_complete` and actual replay assert every link; mutate each root independently. |
| BR03 | Both archives, contexts and ordered payloads remain exact. | Mandatory `genesis_replay`, full payload fixed points, exact default signed genesis deploy list/order, all context fields and whole-file seal checks. |
| BR04 | Both stages execute successfully without either cache. | Fresh-process positive control and `replay_archive_genesis_rejects_cache_roots_and_failed_execution`. |
| BR05 | Original starting-state realization is required only for bootstrap. | Verification constructs no genesis block and never plays workload sources; it uses the same runtime/history and fixed services/tags for both stages. |
| BR06 | Unaccepted genesis cannot authorize workloads. | Restoration returns an error before workload dispatch; unchanged control must first succeed. |
| BR07 | Missing genesis refuses. | Missing-section and empty/truncated/duplicated genesis-list controls in archive admission. |
| BR08 | A wrong trusted bootstrap refuses. | Independently mutated archived pre-root and actual-root argument; no reset to an unchecked root. |
| BR09 | A mismatched genesis/workload bridge refuses. | Mutate every workload's pre-root; validation rejects before execution. |
| BR10 | Incompatible runtime contexts refuse. | Exact expected genesis and workload context checks, including flags, sender, timestamp, system deploys and allocations. |
| BR11 | A genesis cache shortcut refuses. | Independently enable each runtime cache before genesis replay and require rejection. |
| BR12 | Failed genesis replay refuses. | Change a captured genesis deploy's cost; require actual runtime replay failure, not a parse failure. |
| BR13 | Enabled genesis caches refuse even if a root could agree. | The shared execution boundary checks both options before replay; the cache controls use otherwise valid input. |
| BR14 | Complete composed evidence admits the nonempty fixture. | Explicit capture self-check and fresh-process genesis-plus-seven-program replay; preserve v1 independently. |

Genesis event logs are already canonicalized by `Genesis::create_processed_deploy`.
The new archive preserves those exact block payloads, not an invented raw emission
order. Workload records preserve their captured order. Existing RA01–RA11 workload
refusal obligations apply unchanged under the derived realization. Successful v2
replay cannot discharge task 5304's independent genesis-construction discrepancy.

## Live PathMap write-gateway correspondence (2026-09-07)

Task 6446 qualifies the existing ownership-disjoint structural repair, not the rejected
universal in-place removal proposal. Structural removal publishes a freshly constructed
filtered trie; value-only updates retain a guarded cursor. `with_write_cursor` and
`with_write_cursor_at` both close/reset their cursor, normalize a truly empty **whole map**,
and then return the original result or resume the original panic. Normalization is not rollback.

The separate live-write subsection of `PathMapPhysicalOwnership.v` adds four printed
capstones; it does not alter the existing eleven-entry ownership catalog. Its state records
the physical anchor, actual whole-map emptiness observation, endpoint count and cursor status.
`live_write_well_formed` is an explicit emptiness-consistency premise: empty observations have
no logical endpoints and count zero, and a sentinel is observed empty. It is not a proof of
initial count exactness, allocator correctness or complete physical ownership. In particular,
zero values do **not** imply empty topology.

The complete obligations extracted from the new capstones are:

| ID | Model obligation | Implementation evidence |
|---|---|---|
| LW01 | Open cursors cannot pass `normalize_closed_live_write`. | Rust mutable-borrow boundary; the model rejects the open-cursor control. |
| LW02 | Finishing a write closes the cursor. | Both gateway scopes and `GuardedWriteCursor::drop`; normal/unwind tests below. |
| LW03 | The returned image is the checked normalization of the closed state. | Both gateway calls to `normalize_live_empty` occur after the closure-owned cursor is dropped. |
| LW04 | A completed well-formed image is write-ready. | Reinsertion after each tested normal/unwind completion. |
| LW05 | Normalization preserves logical endpoints. | Empty iteration, surviving sibling and retained snapshot checks. |
| LW06 | Normalization preserves the supplied endpoint count. | Zero after final removal; exact surviving count under a nonempty prefix. |
| LW07 | Normalization preserves anchor identity. | Unique-anchor identity before/after completion. |
| LW08 | Normalization preserves the supplied raw-header count. | Normalizer replaces the map within its existing anchor; no owner creation. |
| LW09 | Normalization preserves the supplied wrapper count. | Normalizer creates no wrapper; aliases retain their exact observations. |
| LW10 | The result or unwind payload is unchanged. | `Arc::ptr_eq` checks the exact returned/resumed payload. |
| LW11 | A truly empty image becomes the fresh-root representation. | Exact fresh-empty ACT bytes and subsequent writes. ACT equality is an executable check, not a byte-encoding theorem. |
| LW12 | A nonempty image is unchanged by normalization. | Zero-value topology and nonempty-prefix sibling preservation. |
| LW13 | Normalization is idempotent. | Repeated root/prefix no-op gateways preserve the canonical image. |

The three `live_write_` tests in
[`two_phase_teardown_properties.rs`](../../../rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs)
exercise both gateway entrypoints, normal and unwind outcomes, unique/shared ownership,
empty and nonempty paths, a sole endpoint below a nonempty prefix, and an unaffected sibling.
The generated property uses 32 cases with paths of zero through 256 bytes. Existing Bag and
guard regressions supply the opposite alias-drop order, actual encoded Rholang paths, exact
multiplicities, failure atomicity and deeper stack witnesses; they are not duplicated here.

The model-level negative controls distinguish omitted normalization, normalization with a
live cursor, success-only cleanup, zero-count-as-empty, and prefix-empty-as-whole-map-empty.
The sentinel fixture is proved well-formed and is write-ready only after normalization.
These are abstract controls, **not mutations of the compiled Rust implementation**. Attempts
to reproduce a sentinel panic by omitting normalization after a one-byte guarded removal,
and after raw `remove_val(false)` plus `prune_ascend`, both allowed reinsertion on the current
dependency. Those hypotheses were rejected; no executable sentinel-panic control is claimed.
Likewise, earlier real-key failures affected coherent as well as split cursor protocols, so
close/reopen alone is not asserted to be the historical sibling-loss cause.

Rust reset/Drop completing before normalization and exposing the modeled image, the actual
emptiness observation, `PathMap::new()` behavior, and existing CoW isolation are explicit
realization premises. Header/wrapper/count laws preserve their supplied values; they do not
prove those values were initially correct. The runner checks twelve ownership/live-write
closed-context reports, three control reports, kernel acceptance, and exactly three executed
Rust gateway tests. The manifest test checks inventory linkage only.

### Recorded qualification

Six current modules were compiled from source and kernel-checked together: physical ownership
and controls (12/3 closed-context reports), reverse teardown and controls (23/13), and channel
replacement and controls (5/2), for 58 reports. No orphaned compiled module is used as source
evidence. These are scoped checks, not a run of the entire multi-model verification script.

| Release selection | Passed |
|---|---:|
| `cargo test -p rspace_plus_plus --lib guarded_pathmap` | 31; one measurement-only case intentionally ignored |
| `cargo test -p rspace_plus_plus --lib cow_root_publication` | 3 |
| `cargo test -p rspace_plus_plus --test bag_spec` | 33 |
| `cargo test -p rspace_plus_plus --lib channel_prefix_replacement` | 8 |
| `cargo test -p models --test formal_equivalence_manifest` | 23 |
| Rholang `mod`: both guard arrival-order regressions and `bounded_generated_terms_have_deterministic_play_replay_cost` | 3 |

Every Cargo selection above used `--release -j4` under a systemd scope capped at 8 GiB,
with swap disabled and `PROPTEST_RNG_SEED=41316446`. This is 101 selected test cases,
not full-workspace CI. The actual live-write runner block passed its three tests in debug;
the new manifest test also passed in debug. Nextest passed five executions of the three
Rholang scenarios because the two guard cases are exposed by both `mod` and `reduce_spec`
targets; this duplication is not additional semantic coverage. A channel-prefix invocation
against `--test mod` selected zero tests and is excluded; the correct `--lib` selection above
was subsequently run. The actual count-guard block rejected real zero-test and one-test binary
outputs instead of accepting their successful process exit statuses.

Formal checks used a 2 GiB hard cap with no swap. These configured limits are not peak-RSS
measurements. No benchmark, golden repin, default cutover or broader campaign completion is
implied by this qualification.

## RPB3 observation correspondence (2026-09-07)

**SCOPE.** Task 7966 connects existing proof boundaries and executable observations; it does
not replace the separate full-admission developments in tasks 6743/6744. RPB3 is the canonical
component-key representation stored in `ParBag`; ACT is its compact trie image. Root
`locally_free` and `connective_used` remain outside that component image and are supplied
unchanged when constructing `Par::from_parts`. A successful compact-image admission is not
permission to reconstruct, discard or recompute those root fields.

### Formal boundary inventory

| Boundary | Actual theorem or checked contract | What it does not establish |
|---|---|---|
| Boolean traversal | `RPB3Admission.v`: `rpb3_pda_result_exact`, `rpb3_pda_equivalent_to_recursive_validation`, termination, instruction-count and deterministic-result theorems. The algebra is `local_valid label && forallb children`. | Raw-wire parsing, descriptor interpretation, ordered score/body summaries, default/presence rules, complete/incomplete dispatch and specialized EPathMap/EZipper semantics. Boolean conjunction cannot distinguish child permutations. These remain task 6743. |
| Existing traversal controls | `RPB3AdmissionMutants.v`: omitted local validity is refuted; a standalone exact-input predicate distinguishes `[42]` from `[42;7]`. | The latter is not a mutation of the real protobuf machine. Neither control proves score/order/default/child-loss coverage. Task 6743 still owns those original requirements. |
| Checked scalar steps | `rpb3_admission_cursor.rs`: `checked_region`, `checked_advance`, `worklist_step`, `checked_next_slot`, `child_frame_step`, `root_consumption_authorized`, `rejects_unbalanced_root`, `rejects_duplicate_slot`. | A simulation of production frames, varints, oneof tracking, worklist/arena ownership and full summaries. Root authorization receives completeness and child validity as supplied Booleans; it does not prove their calculation. The last two predicates are positive contracts, not deliberately failing implementations. Task 6744 remains required. |
| Channel and path composition | `ChannelImage.v` quantifies over an arbitrary RPB3 endpoint decider; `RhoPathV2.v` frames opaque payload bytes; the lifecycle model relates abstract images and roots. | None silently discharges the generated admission reducer. String-typed lifecycle witnesses are not proof of Rholang-value correspondence. |

The existing RPB3 model and controls were compiled and kernel-checked before adding the
new observation witnesses. They printed one and two closed capstones, respectively; Verus
with `--no-cheating` checked eight helper contracts with zero errors. This verifies the
named narrow artifacts, not the missing full-summary model. Task 6020 and its cursor child
6744 were returned to `in_progress` after their completion claims failed this source audit;
6743 was already open. No duplicate proof task or new parser was introduced.

### Executable observation map

In the table, **A** is the existing test module in
[`protobuf_admission.rs`](../../../models/src/rust/rholang/protobuf_admission.rs),
**B** is [`par_bag.rs`](../../../models/src/rust/par_bag.rs), and **C** is the integration suite
[`tests/par_bag.rs`](../../../models/tests/par_bag.rs). The production admission endpoints
are `validate_canonical_protobuf_inner`, `validate_canonical_component_protobuf_indexed`
and `ParBag::from_compact_bytes`. Test helpers expose observations without adding a
production decoded projection. The table covers all 22 existing tests in A, plus the
production-validator differential and the three new C witnesses.

| ID | Observation and exact executable witnesses | Boundary being exercised |
|---|---|---|
| RPO-01 | A: `borrowed_and_owned_roots_have_identical_admission_and_error_semantics` | Same success/error for owned and indexed regions; shifted source provenance remains visible. |
| RPO-02 | A: `every_descriptor_message_field_and_oneof_arm_matches_the_recursive_oracle`; `generated_rpb3_table_differential_is_bound_to_admission_runner` | 56 ordinary messages plus two externs (`Par`, `EPathMap`), five oneofs/54 arms; actual generated registry identity; protobuf fixed points against Prost and generated bincode against derived serde. This enumerates descriptors, not every possible input. |
| RPO-03 | A: `every_generated_rpb3_family_fixture_returns_the_authoritative_body_and_score`; `every_component_family_matches_the_authoritative_rpb3_score`; `every_incomplete_component_shape_matches_the_nonsemantic_rpb3_score` | All nine family tags, exact body/score, complete/incomplete and stability summaries; corrupted claimed scores are rejected. Full-summary proof remains 6743. |
| RPO-04 | A: `raw_par_score_preserves_multiplicity_and_quotients_only_root_locally_free`; `raw_par_default_equality_ignores_only_root_locally_free` | Multiplicity and connective metadata remain observable; only the defined root-LF equality quotient is applied. Equality is not an exact serialization oracle. |
| RPO-05 | A: `eset_and_emap_admission_bind_identity_representatives_and_value_winners`; `complete_lane_rejects_sorter_normalization_aliases` | Ordered canonical scores, collection representatives and Map value winners agree with the typed authority; normalization aliases are not admitted. |
| RPO-06 | A: `pathmap_backed_externs_keep_specialized_wire_and_admission_paths`; `epathmap_snapshot_and_every_nested_map_value_are_validated`; `every_epm1_framing_layer_and_homogeneous_mode_is_admitted_once` | Specialized Par/EPathMap/EZipper routes, nested map values, complete EPM1 framing and homogeneous modes, without replacing the trie by an entry list. |
| RPO-07 | A: `descriptor_wide_hostile_wires_match_decode_plus_canonical_reencode`; `every_descriptor_scalar_field_rejects_explicit_default_presence`; `scalar_aliases_defaults_unknowns_and_duplicate_oneofs_are_rejected` | Mutation corpus, minimal representations, unknown/duplicate fields and presence rules; a present default-valued oneof remains valid while an ordinary explicit scalar default is rejected. |
| RPO-08 | A: `map_entries_follow_btree_order_and_prost_default_omission`; `admission_rejects_native_or_mismatched_schema_tables_without_panicking` | Correct map framing/order/default omission and explicit refusal of inappropriate schema programs. |
| RPO-09 | A: `canonical_encoder_images_validate_without_constructing_an_ast`; `every_forbidden_cutover_fallback_makes_the_verdict_red` | Successful admission has zero typed decodes, reconstructed keys and endpoint replays, and exactly one byte-native validation; each forbidden counter independently invalidates acceptance. |
| RPO-10 | A: `depth_20000_validation_uses_the_heap_worklist_on_a_small_native_stack`; `width_4096_validation_uses_counted_occurrences_on_a_small_native_stack` | Concrete fixed-stack depth/width witnesses; not a universal resource theorem. |
| RPO-11 | B: `production_component_admission_is_differentially_equivalent_for_all_nine_families` | Actual production key validator versus retained typed validator, with nine families, payload-bit mutations, unknown fields and truncation. |
| RPO-12 | C: `admitted_compact_rpb3_preserves_codec_event_and_history_observations` | Actual compact admission followed by root-metadata-preserving Par reconstruction, exact protobuf/bincode, three event-hash preimages, complete Produce/Consume bytes, DATA/CONT/JOIN bytes, selected legacy history root and cold read-back. Explicit nine-family and nonempty-corpus floors prevent scalar-only success after corpus shrinkage. |
| RPO-13 | C: `admitted_observation_witness_detects_lost_multiplicity_and_root_metadata` | Deliberately losing occurrences changes every measured byte/root observation. Losing root LF can preserve Par equality yet change protobuf, refuting equality-only acceptance. |
| RPO-14 | C: `admitted_compact_observations_preserve_generated_occurrences` | 32 property cases with bounded duplicate scalar occurrences and root metadata; not arbitrary recursive AST generation. |

The composed C verdict records expected observations **before** compact admission. Both
sides independently check generated bincode/event preimages against derived serde, construct
production datum/continuation records, checkpoint through the actual legacy repository and
read the three record kinds back. It is a preservation differential, not a second implementation
of hashing. Existing `event_hash_encoder_differential`, `event_hash_bytes_differential`,
`epathmap_canonical_fixtures`, `cold_store_records` and `serializer_par_byte_goldens` retain their
separate independent-oracle and pinned-identity roles. No golden was regenerated.

### Runner binding and exclusions

[`check-stack-safe-pda-proofs.sh`](../../../scripts/check-stack-safe-pda-proofs.sh) now
kernel-checks both RPB3 modules, invokes the existing Verus model with `--no-cheating`, runs
the complete A module rather than only its generic registry test, runs the production B
differential, and runs all `admitted_` tests in C. The runner requires exact passing counts
of 22, one and three, with zero ignored tests, so a missing filter or ignored witness cannot
silently pass. The formal manifest checks those runner entries. Manifest strings establish
inventory binding, not successful execution or semantic
equivalence; actual command results must be recorded separately.

**MEASURED VERIFICATION.** The release selections passed 123 tests: 22 admission, one
production-validator differential, nine cold-record, thirteen EPathMap fixture, eleven
event-byte, five event-encoder, twenty-two formal-manifest, thirty-three ParBag and seven
serializer-golden tests. Focused debug selections passed 28 tests (22 + 1 + 3 + 2);
nextest separately passed all three new observation tests. Counts of reruns are not added
to the distinct release-test count. The final manifest change passed again in debug and release.
All selected tests passed without ignores; existing workspace warnings remain.

The exact runner helper was also sourced from the checked script and exercised through
its normal memory-bounded launcher: 22/1/3 passing selections were accepted, a real zero-test
selection was rejected, and a three-test success presented with expected count four was
rejected. The controls checked both Cargo's successful footer and the guard's diagnostic,
so an unrelated compiler failure cannot satisfy the intended-failure verdict.

Rust ran with an 8 GiB hard/6 GiB high memory cap, swap disabled, four build jobs and bounded
test concurrency. Formal checks used a separate 2 GiB hard cap with swap disabled. Property
reruns used seed `41317966`. These are configured limits, not measured peak memory or benchmark
results. Representative focused commands, run within those scopes, are:

```sh
cargo test --locked -j4 -p models --lib protobuf_admission::tests -- --test-threads=2
cargo test --locked -j4 -p models --lib production_component_admission_is_differentially_equivalent_for_all_nine_families -- --test-threads=2
cargo test --locked -j4 -p models --test par_bag admitted_ -- --test-threads=2
cargo test --locked -j4 -p models --test formal_equivalence_manifest rpb3 -- --test-threads=2
cargo test --release --locked -j4 -p models --test par_bag --test event_hash_encoder_differential --test event_hash_bytes_differential --test epathmap_canonical_fixtures --test cold_store_records --test formal_equivalence_manifest --test serializer_par_byte_goldens -- --test-threads=2
```

The admission and production-validator selections were repeated with `--release` as well.
This is the scoped binding gate, not a claim that the entire multi-model formal script,
full workspace CI or all remaining campaign obligations passed.

No production codec, default/presence rule, hashing algorithm, backend or charge rule changes
in this slice. The new composition tests do not execute COMMs, replay deploys, terminate a
writer, prove arbitrary-input parser equivalence, resume a durable import session or activate
the PathMap DAG backend. Those remain their existing required tasks. The absence of a new
consensus format is not evidence that the entire campaign is consensus-qualified.
