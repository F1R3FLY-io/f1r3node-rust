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

## Singleton EPathMap import prefilter (2026-09-08)

`CanonicalEPathMapImport.v` refines the existing singleton import admission
decision with a negative EPM1 mode/count hint. It does not replace ACT validation
or the importer. The hint reads only framing; every hinted singleton still passes
through the original inspector. The proof's named `inspector_refines_epm1_framing`
premise requires a successful inspection to agree with a complete EPM1 parse.
The prefilter section's twelve closed reports cover six general contracts and
six synthetic controls. The binary Var section below adds seven closed reports,
and the concrete envelope section adds ten. Carrier, nested-chain and signed-GInt
sections add nineteen, for forty-eight closed reports in this module.

| Formal contract | Executable correspondence in `models/tests/support/epathmap_fast_import.rs` |
|---|---|
| `decoded_epm1_prefix_determines_route_hint` | `singleton_prefilter_full_inspection_refines_hint_for_generated_and_mutated_images`: 128 generated snapshots, each with a replacement/truncation/append variant; full inspection implies the same hint mode and value count. Neutral empty inputs remain covered. |
| `singleton_prefilter_preserves_result` and `singleton_prefilter_preserves_fallback_observation` | Positive singleton, forged-count and same-hint-corruption controls; the focused gate also reruns seven existing Var, metadata, import and owned-component witnesses. This is not exhaustive Rust equivalence. |
| `prefilter_machine_has_exact_execution` | Positive and negative examples exercise both phases, rather than only proving properties of an unreachable machine. |
| `width_rejection_never_invokes_full_inspection` | Wide-map tests require zero inspector entries and zero expanded-key copies; an actual unfiltered-inspector control must copy shared prefixes. The nested case requires three, not four, inspections. |
| `hinted_singleton_still_invokes_full_inspection` | Forged count and corrupted ACT retain one inspector entry and are rejected. A hint-only admission implementation fails these controls. |

The Rust counters are test-only and thread-local. Their scope is inspector calls
and owned endpoint-key copying, not total allocation, memory residency or runtime.
The formal controls use synthetic topology and do not establish actual ACT parser
correctness. The carrier and chain models below now cover outer Par metadata,
nested composition and the exact stability fold. The shared byte-native
key-validation replacement and its physical-resource qualification remain task
6018; neither a symbolic inspector nor a passing bounded differential discharges
that implementation boundary.

Run `bash scripts/check-stack-safe-pda-proofs.sh --epathmap-import` from the
repository root. It rebuilds the imports, checks the exact registered capstone
counts and independent kernel validation, and runs thirty-four Rust tests with property seed
`41315976` by default. Proof/Rust cgroup ceilings are 2/6 GiB with swap disabled.
Both stages are also registered in the aggregate gate; this is an internal
admission optimization, not a new generated schema traversal/manifest entry.
The [journal](../../../docs/design/pathmap/pathmap-engineering-journal-2026-08-23.md#j-084--2026-09-08-reject-ineligible-singleton-imports-before-expanded-key-inspection)
retains causal evidence and the distinction between formal premises and runtime tests.

### Exact canonical Var grammar (CEI-01)

`BinaryVar` models the concrete `RhoTypes.proto` oneof: empty bytes mean no selected
arm; keys `08` and `10` select Bound and Free, respectively; `1a 00` is the empty
Wildcard message. Scalar payloads are the unique minimal base-128 encoding of a
zigzag-encoded signed 32-bit integer. Explicit Bound/Free zero remains distinct
from an absent oneof. Payload arithmetic uses binary `N`, and signed values use
`Z`; the existing EPM1 parser correspondence is symbolic, avoiding evaluation of
large unary naturals.

The seven new closed reports establish the EPM1 parser link, exact u32 encoder
image, signed-i32/u32 zigzag bijection, exact Var encoder image, and three grouped
positive/refusal/presence controls. The main theorem is an iff for all inputs and
values, not a property conditional on randomly sampled acceptance. An auxiliary
lemma proves that accepted u32 payloads occupy at most five bytes. The encoder's
five-group budget follows from this integer bound; it is not a limit on term,
trie or PDA traversal depth.

`models/tests/support/canonical_var_admission.rs` binds these invariants to the
existing generated protobuf encoder and production cursor. Three properties run
1,024 cases each: all four forms round-trip; accepted bytes are exact encoder
images with four guaranteed accepted controls in every case; and generated
Bound/Free messages reject truncated, nonminimal-key/payload, duplicate, trailing,
unknown-field and overflowing variants. The signed generators range over the
full i32 domain; the existing deterministic image matrix retains extremal and
Wildcard witnesses. These are executable implementation correspondences, not a
claim that Rocq has verified the Rust compiler or generated machine code. No
production Var encoding, admission rule or dependency changes.

### Concrete protobuf envelope grammar (CEI-02–03 local boundary)

The `Envelope` module proves its normalized four-phase cursor accepts exactly
the modeled ordered protobuf grammar: optional nonempty locally-free field 3,
optional true connective field 4, optional canonical Var field 5, and mandatory
snapshot field 9. The snapshot is opaque and may be empty or invalid ACT at this
stage. This is not the complete generated EPathMap writer image. Literal keys
are related to the existing canonical EPM1 varint reader; frame reconstruction
and the checked Var image theorem supply concrete byte correspondence without
assuming an envelope recognizer.

Rocq compilation and kernel checking preceded the Rust properties in
`models/tests/support/epathmap_envelope_admission.rs`. The test reference uses
four ordered phases, prost framing and generated Var decoding plus
exact re-encoding. It does not call the production guarded-loop parser.

| Checked capstone | Executable binding |
|---|---|
| `envelope_frame_exact_bytes`, `ordered_envelope_cursor_exact_grammar` | `canonical_epathmap_envelope_proof_ordered_cursor_correspondence`: independent normalized cursor versus production loop on arbitrary bytes, a guaranteed canonical input and generated duplicate/order/nonminimal-key/truncation mutations. |
| `envelope_observation_exact`, `successful_envelope_remainder_is_canonical`, `envelope_remainder_presence_is_distinct`, `envelope_positive_metadata_controls` | `canonical_epathmap_envelope_proof_images_and_presence`: all twenty locally-free/connective/remainder-presence combinations per case, including default/Bound/Free/Wildcard Var images and full-domain signed values. |
| `later_bad_remainder_rejects_independently` | `canonical_epathmap_envelope_proof_late_remainder_refusal`: all four preceding metadata combinations with malformed remainder families and generated suffixes. |
| `snapshot_bytes_remain_opaque` | `canonical_epathmap_envelope_proof_opaque_snapshot_and_borrowing`: exact snapshot/remainder bytes and pointers into the original input; no snapshot interpretation. |
| `envelope_order_and_duplicate_controls`, `envelope_framing_and_scalar_refusal_controls` | `canonical_epathmap_envelope_proof_refusal_controls`: all sixteen formal refusal examples, absence versus present-empty, and opaque inputs accepted locally but refused by root-route inspection. |

The four generated properties run 1,024 cases each. These bounded executable
bindings do not prove Rust or allocator correctness. Outer Expr/Par framing,
nested-chain composition, ACT admission, checked-size arithmetic and full-import
observations remain separate obligations. The local envelope cursor does not
expose locally-free payload bytes; only its nonempty/presence flag is observed.

### Import contract coverage (task 5976)

The prefilter result must not be mistaken for the complete fast-import proof.
The following contracts are requirements. CEI-01 and CEI-07 and the local
CEI-02–03 envelope boundary have the checked model sections described above.
The carrier and chain sections now prove their explicit models, including
CEI-04–06. The score section supplies concrete CEI-08 byte and length laws.
These model-to-implementation boundaries remain bound by the independent
canonical encoder/semantic references and the explicit premises below.

| Contract | Exact obligation | Required executable binding |
|---|---|---|
| CEI-01: Var bytes | Recognize exactly absent, Bound/Free signed-i32 and empty Wildcard encoder images. Minimal keys and payload varints, complete consumption, the u32 zigzag bound, and present zero versus absence are distinct obligations. | Generate signed values across the full i32 domain with guaranteed valid Bound and Free examples; compare both recognition and re-encoding. Retain extrema, truncation, redundant varints, unknown/duplicate/trailing fields, overflow and nonempty-Wildcard controls. Conditional random-byte acceptance alone is insufficient. |
| CEI-02: remainder presence | Every present remainder has canonical Var bytes. An absent remainder differs from a present empty/default Var; both presence and bytes survive envelope recognition. | Absent/default/bound/free/wildcard cases at outer and nested levels; malformed remainders remain rejected after an earlier unstable layer. |
| CEI-03: envelope metadata | Recognize the actual ordered EPathMap fields, reject duplicates and preserve map locally-free/connective/remainder information separately from outer Par metadata. Parsing a snapshot field does not validate its opaque ACT contents. | Canonical writer images and independent mutations of each framing, order, presence and metadata axis. |
| CEI-04: chain traversal | The iterative chain helper agrees with a recursive reference that either descends through an eligible nested envelope or returns an opaque terminal. Successful helper output does not imply that terminal is a valid imported term. | Compare depth, terminal bytes and folded metadata; require later nested validation even after an earlier instability. |
| CEI-05: stability fold | Key admission must succeed and classify keys as stable. Component stability includes map metadata/remainder presence and nested value-Par metadata, but excludes the outer Par metadata. An unstable admitted value is not a rejected key. | Independent semantic stability comparisons, one-axis-at-a-time metadata changes, and invalid-key controls. |
| CEI-06: terminal filter | The full importer accepts only a metadata-free, exact singleton GInt terminal. Prove the complete importer validates every remainder, rather than falsely assigning this guarantee to the intermediate chain helper. | Noninteger, malformed nested-envelope and trailing-metadata terminals must decline the fast import; valid integer boundaries must pass. |
| CEI-08: imported observation | The direct score corresponds to the EPathMap score tree; exact Expr bytes, outer Par metadata and multiplicity one are preserved. Fast-path decline delegates to the existing generated decoder. | Byte/term/score/hash differentials against independently constructed terms and the generated codec; preserve fallback behavior. |

CEI-07 is the separately checked prefilter contract above. The Var development
uses binary unsigned/signed arithmetic rather than evaluating large values as
unary naturals. Reuse the existing EPM1 framing and shared admission machine;
do not introduce another general protobuf parser to prove these local contracts.
Task 6018 owns the remaining standalone byte-native key-admission entry and its
resource qualification. Its existing worklist already handles message, EPM1 and
opaque nested-path obligations; a shared entry must preserve rejected-key versus
admitted-stability distinctions without wrapping keys in fabricated snapshots.

### Composed import refinement boundaries (2026-09-08)

The task 5976 source review fixes the following proof and executable-oracle
interfaces. The checked models and executable bindings below distinguish
completed local contracts from remaining implementation-wide obligations.
The implementation under review is
[`par_bag.rs`](../../../models/src/rust/par_bag.rs), specifically
`canonical_singleton_epathmap_chain_proof` composed with
`import_canonical_singleton_epathmap_par`.

| Stage | Observable contract | Discriminating witness |
|---|---|---|
| EPathMap envelope | Required field 9 retains its exact borrowed bytes, even when empty or not a valid ACT image. Field 5 distinguishes absence from a present empty Var. Locally-free bytes are a reconstruction witness; the helper exposes only whether that field is absent. | Helper acceptance of an opaque snapshot must not imply root-route or full-import acceptance. Late malformed Var bytes must fail even after locally-free/connective metadata makes the component unstable. |
| Chain transition | State consists of the current snapshot, depth and accumulated stability. Every descent still performs snapshot inspection and key admission; a false stability accumulator does not terminate validation. | Compare depth, terminal bytes and each metadata axis against a recursive reference over the same inspected slices. Keep ACT/key-parser correspondence as explicit premises. |
| Opaque terminal | A nested-envelope decline returns a terminal from the chain helper. The composed importer subsequently requires an exact metadata-free singleton GInt image. | Construct a malformed nested EPathMap for which the helper returns a terminal but full import declines. Prove this shape cannot satisfy the GInt grammar. `RhoTypes.proto` declares GInt as `sint64`: apply signed-64 zigzag before the u64 varint, rather than Var's signed-32 bound; include minimum, maximum, minus one and zero. |
| Stability | Invalid or unstable keys decline the specialization. Valid unstable value metadata instead yields an admitted component with false stability. Component stability excludes the outermost Par metadata; whole-Par stability includes it. | Use explicit independent expected booleans for every metadata axis. Comparing two cached stability values alone is insufficient. |
| Imported endpoint | Preserve the RPB3 score, exact Expr payload, big-endian u64 score-length trailer, multiplicity one and outer Par metadata. | Independently encode the semantic score tree and inspect the complete endpoint key, in addition to term/protobuf/hash comparisons. |

The score authority for this importer is
[`canonical_semantics::rpb3_pathmap_score`](../../../models/src/rust/rholang/canonical_semantics.rs):
`Node(EPATHMAP, Bytes(snapshot), Int(map_connective))`. Bind it through the
independent tree encoder in
[`score_tree.rs`](../../../models/src/rust/rholang/sorter/score_tree.rs).
The generic expression sorter uses a different collection-score route and is
not the oracle for this RPB3 endpoint. `CanonicalEPathMapScore.v` now proves the
concrete fixed-shape score bytes independently of the semantic metadata fold.

Completeness claims must include the implementation's checked size arithmetic
and depth representability premises. Successful-import soundness alone does not
prove acceptance for every structurally eligible input. A strictly shrinking
nested snapshot proves termination, not linear work in the original physical
input size. Allocation failure, the full ACT/key validator and fallback decoder
correspondence remain explicit boundaries until their own evidence is supplied.

### Carrier, chain and signed terminal bindings (CEI-04–06)

The `Carrier` development recognizes the exact ordered Par fields: one Expr
field 5, optional nonempty locally-free field 9, and optional true connective
field 10. Nested EPathMap recognition composes that carrier with Expr field 32
and the checked envelope grammar. The complete GInt image is a separate,
metadata-free singleton carrier. Its `sint64` payload uses the bijection between
signed 64-bit integers and minimal unsigned-64 zigzag varints.

The `Chain` machine agrees with its recursive reference in both directions,
including refusal, subject to checked depth representability. Every descent
strictly reduces the current snapshot length. Proof induction is not a runtime
fuel or traversal limit. Invalid or unstable keys decline the specialization;
valid unstable value metadata does not. A false accumulated stability bit never
skips later snapshot or key validation. A helper terminal is opaque until the
full importer's separate GInt check succeeds.

| Checked invariant group | Executable binding |
|---|---|
| Exact carrier grammar, nested composition and remainder image | `canonical_epathmap_carrier_proof_ordered_images`, `nested_metadata_images`: generated valid images, independent ordered cursor and malformed-field controls. |
| Exact raw-u64 and signed-zigzag GInt images; map/GInt disjointness | Carrier `exact_int64_terminal`, `map_shape_excludes_integer`, `boundary_controls`: generated protobuf differential across full-domain i64 values, extrema, nonminimal/overflow/trailing refusals and malformed map shapes. |
| Inspector/key authority and validation after false stability | `singleton_chain_proof_invalid_and_unstable_keys_decline`, `false_stability_does_not_skip_later_act`: real ACT images, isolated key/ACT mutations and inspector-call witnesses. |
| Machine/reference equivalence and exact metadata fold | Chain `recursive_reference_and_metadata`, `every_metadata_axis_and_integer_extremum`: 128 generated chains and independent one-axis changes at three nested levels, including outer metadata exclusion from component stability. |
| Opaque terminal versus complete admission | `singleton_chain_proof_opaque_malformed_terminal_is_not_full_import`: malformed nested remainder accepted only as an opaque helper terminal; the complete importer refuses it. |

Four carrier properties use 1,024 generated cases each. The recursive chain
reference is test-only and shares the real ACT inspector/key validator as an
explicit boundary, but independently frames, descends and folds metadata.
These tests therefore do not independently verify those shared validators.

The initial signed test oracle mistakenly used an i64-to-u64 cast. The schema
and generated encoder exposed that error at minus one and the signed extrema.
The corrected oracle uses zigzag and retains the minimized minus-one regression
seed. No runtime GInt codec or accepted-byte language changed in this correction.

### Concrete score and endpoint bindings (CEI-08)

`CanonicalEPathMapScore.v` proves the specialized integer atoms, zero escaping,
fixed-shape semantic serialization, exact endpoint framing, score-length trailer,
checked reserve arithmetic, parser round-trip and prefix-free score ordering.
Let $`n`$ be snapshot bytes, $`z`$ its zero-byte count and $`p`$ the exact Expr
payload length. Score length is $`26+n+z`$, endpoint length is $`39+n+z+p`$,
and the current checked reserve is $`69+2n+p`$. The reserve bounds the emitted
endpoint; it does not guarantee allocator success. Equal-score endpoint ordering
uses an explicit non-prefix payload premise; unequal-score ordering does not
depend on the appended payload or trailer.

`models/tests/support/epathmap_score_admission.rs` checks direct score emission
against both the semantic tree encoder and the canonical-score arena cursor.
Three 1,024-case properties cover arbitrary snapshot bytes, complete imported
keys/root metadata/multiplicity-one observations and ordering. Fixed controls
independently mutate constructor, escaped zero, connective, Expr payload and
trailer, and retain an equal-score/different-payload ordering witness. Arbitrary
snapshot bytes are legitimate score-encoding inputs, not claimed ACT images;
full-import properties separately construct valid EPM1 snapshots.

The module's seventeen checked capstones include three composition results:
`concrete_checked_import_sizes_exact` instantiates the import model's header with
five bytes and its score-length parameter with the concrete encoder;
`full_import_has_concrete_rpb3_endpoint` derives exact endpoint splitting,
semantic score, capacity fit and multiplicity one from the full-import contract;
`concrete_full_import_positive_control` witnesses a successful composition.
The proof retains the ACT/key implementation premises rather than replacing them
with a claim about a synthetic inspector. Platform representability and allocator
success remain separate implementation obligations.

### Standalone canonical-key root: implementation contract (tasks 5976/6018)

The internal standalone admission entry reuses the existing protobuf admission worklist and
shared canonical-path grammar. It is not a second key parser or a fabricated EPM1
snapshot. Initially its frame used the persistent reducer handle and page-backed
arena. The refinements below replace those with `CanonicalPathStreamReducer`,
reusable grammar frames and checked indexed spans. The current standalone frame
retains its indexed region, located cursor, stream reducer and optional pending
obligation; the persistent ACT facade still owns its original arena policy.
Raw-Par escapes dispatch the existing Message frame; nested EPM1 dispatches
the existing EPathMap frame. The parent remains suspended on the same explicit
heap stack. The Message-root wrapper retains its current postchecks and error
precedence. The broader typed metadata API is not replaced by this narrower
validity/stability result.

The following matrix is the required formal-to-executable contract. The entry
is implemented but not activated in the importer. `CanonicalKeyProtocol` in
`RPB3Admission.v` proves nine protocol capstones and two controls before the Rust
change; the preserved generic fold supplies the twelfth closed report. Independent
kernel checking passes. The concrete read, payload-extraction and terminal-semantic
premises remain explicit in `parser_refinement`; neither that interface nor the
generic Boolean fold is a proof of the concrete byte parser.

| Invariant | Required discriminating executable witness |
|---|---|
| Exact root completion | Empty, truncated, complete and trailing paths; no success with a pending child or unconsumed suffix. |
| Three distinct verdicts | Guaranteed stable keys, valid unstable escapes and malformed keys; errors must not become valid instability. |
| Correct child routing | Raw-Par versus nested-EPM obligations, mismatched summary kinds and pending IDs; zero-length obligations dispatch immediately. |
| Canonical resolution | An escape containing a stable Par refuses; nested EPM requires Map mode and stable entries. Installing a returned handle is mandatory even when it carries deferred rejection. |
| Suspension and resumption | A mixed split key has an obligation followed by a discriminating suffix; no skipped, duplicated or prematurely consumed bytes. |
| Child failure authority | Malformed child plus malformed suffix retains child-first refusal and cannot authorize the parent endpoint. |
| Exact byte ownership and origin | Borrowed and frozen fragmented inputs agree; discontinuous byte origins and empty-payload EOF retain their supplied anchors. |
| Importer selection | New stable-key success agrees with the existing typed validator over generated valid keys and causal mutations; invalid and valid-unstable keys both decline only at the importer. |
| Actual operation accounting | Observe cursor reads, reducer states/frames, arena payload/provenance/pages, driver capacity and child dispatch/resolution; do not mistake zero-initialized legacy counters for allocation measurements. |

`models/tests/support/standalone_path_admission.rs` binds the catalogue to the
existing typed validator and to causal controls. CKP-01 is
`standalone_acceptance_inherits_explicit_parser_authority`; CKP-02 is
`completion_requires_full_consumption_without_pending_child`; CKP-03 is
`saved_parent_id_kind_and_resolution_are_exact`; CKP-04 is
`canonical_resolution_rules_preserve_valid_unstable`; CKP-05 is
`suspended_cursor_and_ordered_read_are_preserved`; CKP-06 is
`child_failure_is_terminal`; CKP-07 is
`child_payload_retains_owner_bytes_and_provenance`; CKP-08 is
`stable_only_caller_preserves_three_outcomes`; CKP-09 is
`shared_loop_has_no_forbidden_transport_effect`.

The tests include a stale ID taken from a previous child, a returned handle
carrying deferred semantic rejection, an empty EPM child whose error must retain
the length-byte anchor, a sixteen-level protobuf/EPM chain, and depth-20,000 tuple
admission and teardown on a 64-KiB stack. Located-cursor and arena counters are
actual implementation observations, but the transport effect theorem does not
prove allocator costs or child-parser work. The previous and extended driver
enum layouts are compared directly in the test build; this is not a portable
ABI promise or a substitute for measuring production allocations.

The standalone entry must remain internal until flat string/byte-array and
nested-obligation qualification justifies caller activation. The existing
reducer retains state and byte/provenance storage per edge and has no no-fork
mode. A standalone contiguous key does not need topology restoration history;
reusing the grammar does not imply that retaining its trie-walk history is
efficient. No advantage is inferred merely from avoiding AST decoding.

Task 6018 also retains the pipeline-wide requirements: avoid repeated traversal
of nested opaque payloads, repeated active-region rebuilding and keys already
reconstructed by the local ACT inspector. Page-backed ownership already exists
and is not a missing implementation to recreate. The new root protocol alone
cannot prove linear work in physical input bytes or close these requirements.

The source-backed resource plan separates exclusive standalone storage from
persistent ACT topology history while retaining the same grammar. A non-forkable
current-state facade can remove historical `InnerPath` slots, but that alone
does not remove historical parser frames or copied arena payloads. The required
endpoint is a live exclusive continuation stack and indexed input spans for the
standalone root, alongside the existing persistent storage for ACT forks.
`IndexedByteRegion` already provides the subviews, comparison and located access
needed by standalone region-key and child-payload spans. Child-region adaptation
must preserve the reducer's supplied completion anchor exactly.

The storage sequence's pre-implementation formal obligations are:

1. Exclusive state cannot export or restore historical reducer handles.
2. Persistent and exclusive paths have identical validity, stability, error,
   provenance and child-obligation observations.
3. Replacing/reclaiming frames preserves all reachable continuations and
   previous-key spans without changing retained topology checkpoints.
4. Indexed spans equal arena slices, including comparison and EOF anchors.
5. Suspended child spans survive parent advancement and teardown.
6. Checked bulk consumption of opaque payloads equals repeated feeding,
   including enclosing bounds, pending IDs and child-first error order.
7. Retained states, live/allocated frames, copied payload/provenance and physical
   byte visits have separately stated bounds.
8. The CKP completion, resolution, failure and three-outcome contracts remain
   unchanged.

Opaque payload skipping cannot skip local UTF-8 or grammar checks. Replacing
ancestor-by-ancestor region updates with checked absolute endpoints, and removing
repeated work inside nested ACT traversal, remain part of the complete physical
work requirement. The current-state-only step must not be mistaken for that
endpoint. The existing flat-width, deep-tuple, region-key ordering, mixed-child
and fragmented-source corpora are the executable qualification basis; allocator
and per-origin work observations must exclude fixture construction and report
teardown separately. These are planned storage refinements, not claims proved
by `CanonicalKeyProtocol`.

### Exclusive current-state storage refinement (tasks 5976/6018)

`ExclusiveCurrentState` in `RPB3Admission.v` proves the first storage refinement,
not the entire resource plan above. A shared checked transition consumes the
current parser state, semantic core world and next input. Both success and
failure return their resulting world; failure retains the previous current
state and terminates the input trace. This explicitly avoids assuming rollback
of frame/arena effects. Allocator capacity is outside the semantic-world model.

The persistent storage policy retains successful states as history; the
exclusive policy has exactly one current state and no historical restore
operation. The full observation includes current state, final world, attempted
inputs, successful observations and the optional error. Nine closed reports
are added after the twelve prior reports, and independent kernel checking
passes before the Rust storage change.

| ID | Closed report | Executable binding |
|---|---|---|
| ECS-01 | `historical_storage_has_exact_nonempty_latest_projection` | Persistent root/checkpoint API retains prior handles; a pending old handle remains unresolved after its successor resolves. |
| ECS-02 | `exclusive_and_history_have_identical_full_observations` | Byte-by-byte and obligation-resolution differential; compare transitions, endpoint/stability verdicts and exact errors. |
| ECS-03 | `checked_failure_stops_both_drivers_at_the_same_state` | Pending-child edge refusal, stale ID, malformed grammar and terminal child failure; no swallowed error. |
| ECS-04 | `both_storage_policies_inherit_the_explicit_grammar_premise` | Both facades invoke the same extracted `CanonicalPathMachine`; grammar methods are unchanged apart from formatting. |
| ECS-05 | `exclusive_storage_retains_exactly_one_state` | One private `InnerPath`, no history vector or exported checkpoint handle; flat/deep standalone tests and direct retained-state bytes. |
| ECS-06 | `historical_growth_counts_only_successful_transitions` | Count successful edges/resolutions against persistent retained states after every step. |
| ECS-07 | `shared_world_effects_and_invariants_are_preserved` | Equal obligation IDs, payload bytes/provenance, frame counts and arena work; concrete invariant preservation remains an explicit premise. |
| ECS-08 | `exclusive_storage_positive_controls` | Stable, unstable and resolved sequences; persistent growth versus exclusive one-state storage. |
| ECS-09 | `exclusive_storage_causal_refusal_controls` | Wrong oldest-state selection, reset core world, swallowed refusal and erroneous failure-world rollback differ from the reference. |

`CanonicalPathReducer` remains the persistent facade for ACT topology;
`CanonicalPathStreamReducer` supplies the exclusive facade to the private
standalone admission frame. Both delegate to the same core. The existing
`standalone_path_admission.rs` corpus adds a 96-case storage-trace property and
explicit ascending/duplicate/reversed region-key controls. This is not a second
parser or a new input grammar.

The initial ECS checkpoint removes historical **root-state** retention only. Historical
parser frames, arena payload copies, enclosing-region rebuilding and repeated
nested visits remain required work. The resource controls intentionally verify
that these costs have not disappeared merely because the state count is one.
Importer activation remains gated on their full qualification. The later reusable
frame and indexed-span sections record the subsequent removals; their tests
replace the obsolete arena-work equality with exact consumed-prefix correspondence.

### Reusable frame ownership (tasks 5976/6018)

`ReusableFrameOwnership` in `RPB3Admission.v` adds nineteen closed reports after
the twenty-one prior reports. The complete theory compiles and passes independent
kernel checking before production frame storage changes. A slot is an internal
frame index, not a serialized identifier. The concrete pool partitions all slots
into occupied cells and a free list. Cells record their frame kind, grammar and
obligation metadata, outer continuation and optional Region child. A Region with
an empty child remains distinct from an ordinary frame.

The edit algebra covers keeping, pushing, popping and replacing ordinary frames;
opening, rebuilding and completing Regions; selecting saved Region owners;
returned errors; and composition. Concrete edits require the exact stored cells,
from which final graph realization is derived. The allocation and edit journals
may enumerate the same slots in different orders; their relationship is a
permutation, not an accidental list equality.

The following catalogue was recorded before the runtime refinement. The binding
checkpoint below distinguishes the now-executed storage properties from the
remaining physical-byte and integration obligations.

| ID | Closed report | Required executable invariant/control |
|---|---|---|
| RFO-01 | `link_construction_refines_concrete_slot_contents` | A fresh ordinary frame contains exactly the selected metadata and continuation. |
| RFO-02 | `region_construction_preserves_disjoint_child_and_outer` | Region child and outer continuation own disjoint slots, including empty-child creation. |
| RFO-03 | `reservation_journal_retains_original_cells` | In-flight allocation never overwrites any saved owner cell. |
| RFO-04 | `reserved_slots_are_neither_live_nor_in_flight` | Every reserved slot is absent from both original live cells and earlier reservations. |
| RFO-05 | `local_owner_journals_have_exact_slot_conservation` | Original plus allocated slots equal selected graph plus retirees, with multiplicity preserved. |
| RFO-06 | `concrete_edit_derives_both_realization_and_exact_local_journal` | Each grammar storage action binds an actual frame cell to the corresponding edit constructor; include Region opening/completion. |
| RFO-07 | `local_commit_journal_is_exact_and_never_retires_a_live_frame` | No duplicate retirements, omitted consumed slots or live-frame retirement. |
| RFO-08 | `physical_reservation_freshness_discharges_edit_ownership` | Fresh allocation maintains unique ownership across the original graph and new slots. |
| RFO-09 | `commit_preserves_the_new_concrete_graph_and_removes_exact_consumed_slots` | After commit, occupied slots are exactly the selected graph and all its metadata/links are readable. |
| RFO-10 | `returned_error_restores_exact_owner_and_original_obligation_cells` | Wrong-ID and other returned errors preserve the current owner and original cells; semantic core effects are not assumed rolled back. |
| RFO-11 | `held_root_identity_selects_exact_graph_independently_of_dead_status` | Compare retained root IDs while original cells remain immutable; never select rollback from `Dead` status alone. |
| RFO-12 | `fresh_rebuild_and_owned_pop_cannot_alias_the_held_parent_root` | A fresh replacement or owned tail cannot equal the consumed ancestor's root. |
| RFO-13 | `deferred_dead_retains_the_selected_local_graph_and_metadata` | Shallow refusal may retain a new Tag; a saved Region owner may retain its old graph; preserve source, stability and obligation-clock metadata. |
| RFO-14 | `region_dead_discards_child_replacements_without_retiring_original_children` | Saved-owner selection cancels child retirement choices and retires child allocations instead. |
| RFO-15 | `physical_capacity_is_bounded_by_peak_live_plus_in_flight_not_transition_count` | Slot high-water follows peak occupancy, not cumulative bytes; measure actual vector capacity separately. |
| RFO-16 | `reservation_occupancy_is_exact_live_originals_plus_in_flight_replacements` | During a transaction, count retained originals and all fresh replacements, including ones awaiting retirement. |
| RFO-17 | `reusable_slot_positive_transaction_controls` | Allocate, commit and reuse a freed slot without increasing slot capacity. |
| RFO-18 | `reusable_slot_causal_ownership_and_dead_controls` | Detect eager overwrite, aliased child/outer graphs, clearing a failed stack and incorrect saved-owner selection. |
| RFO-19 | `ordered_multi_edit_region_completion_and_reuse_controls` | Compose multiple allocations with reversed physical journal order; open/complete a Region; reject omitted child retirement and status-based rollback. |

The implementation retains separate, fixed persistent and exclusive
storage policies behind the shared grammar. Persistent ACT checkpoint handles
must never reach an exclusive reusable store. The exclusive transaction proceeds
as follows:

1. Reserve new slots without releasing original or in-flight cells.
2. Record allocation/retirement lengths at each owner scope.
3. If the selected owner root is unchanged, restore that scope's retirement
   prefix and append its allocations as retirees. Keep the allocation journal.
4. Otherwise retain the child edit journal and retire the replaced owner.
5. At outer commit only, release the final retirement journal and clear the
   transaction journals. A returned error selects the original root graph.

Observation methods only inspect frames. Root identity is usable because both
graphs are realized by the same immutable in-flight cells. No per-byte scan of
all reachable frames is needed to derive the retirement set. Physical release
inside a nested scope would invalidate the reserve-only transaction argument
and is therefore not the implementation described by these proofs.

The pool capacity theorem is conditional on peak occupied slots; its occupancy
identity counts original live frames plus in-flight replacements. It does not
prove a parser-specific depth bound, `Vec` byte capacity, allocation timing or
RSS. Parser-branch-to-edit correspondence, frozen payload/provenance preservation
in Rust, concrete pool property tests, live-frame bounds and integration require
the separate executable evidence described below. The model neither proves the
complete parser grammar nor assumes that errors are absorbing. Arena payload
reclamation and physical-byte traversal work remain separate obligations in the
existing resource plan.

#### Rust binding checkpoint (2026-09-08)

`CanonicalPathMachine<FrameHistory>` retains immutable ACT branch checkpoints;
`CanonicalPathMachine<FrameStorage>` holds only the standalone validator's live
graph and reusable slots. Static dispatch shares the grammar without storing
unused reclamation journals in the persistent policy. `feed_frame`, Region
unwinding and obligation resolution select owner-local journals; `bubble` records
consumed Kids frames. `advance` and `resolved_state` alone finalize reclamation.
No nested owner releases physical slots and no production byte step scans the
complete frame graph.

The external `models/tests/support/canonical_frame_storage.rs` module binds the
catalogue with eight tests, including two 128-case properties:

| Executable test family | RFO bindings |
|---|---|
| All six frame variants and generated ordinary edits | 01, 03–10, 12, 15–17 |
| Generated nested Region selections | 02–09, 11–14, 16, 19 |
| Dead-state root selection and explicit Region opening/completion | 02, 06, 10–14, 19 |
| Rejected eager reuse, aliasing and omitted/duplicate retirement | 03–09, 18–19 |
| Persistent history and repeated committed reuse | 03–04, 09, 15–17 |

Each transaction checks exact original-plus-allocated occupancy, unchanged held
cells, disjoint selected/retired sets and complete retirement accounting. Each
commit checks that occupancy equals the reachable selected graph. The standalone
semantic suite additionally compares persistent and reusable storage after byte
and resolution steps, including wrong-ID recovery and deferred refusal. These
are executable refinement witnesses, not a mechanically extracted Rust parser.

All **360** models unit tests pass with seed `41315976`, four test threads and a
6-GiB systemd memory limit. In the flat byte-array matrix, frame backing capacity
is **768 bytes / two slots** at every width from 16 through 8,192. Payload arena
growth is unchanged. The independent persistent-policy layout assertion requires
exactly the size of `Vec<Frame>`; the outer protobuf admission enum measures
1,136 bytes. These are layout/capacity measurements, not allocation counts or RSS.
See journal J-092 and stack-safety §5.33.11 for raw evidence and remaining limits.

The focused gate additionally passes **57** Rust tests, 48 import / 17 score /
40 RPB3 closed reports and three independent kernel checks. Broader integration
passes **23** manifest and **36** ParBag cases, with depth 4,096 unchanged.
After relocating the graph checker under `tests/` and clarifying the layout
probe's nonhistorical label, the complete models-library rerun passes 360/360.

### Indexed standalone transport: refinement contract (task 6018)

This is the next required resource refinement after reusable frames, not a new
validator or a change to ACT traversal. The existing standalone input and located
cursor already own or borrow every input byte. Copying the same stream into a
second arena is unnecessary if the grammar's byte-dependent operations can use
checked ranges into that immutable input. The persistent ACT policy still needs
its existing branch handles and arena; those public signatures remain intact.

The shared grammar will parameterize its previous-key span representation.
An arena span remains `CanonicalBytesHandle`; an indexed span is
`CanonicalByteRange`. `InnerPath` and frame IDs remain lifetime-free. The read
policy supplies consumed length, checked slices, span length, byte comparison
and physical provenance. Indexed slices must stay within the **consumed prefix**,
not merely within the full input. Pending-child rejection remains before any
append or consumed-offset advance. A region-taking child constructor will serve
both the frozen ACT adapter and the indexed standalone adapter.

#### Exact child completion anchors

The existing `payload_end` is a completion anchor: the final physical payload
byte, or the length-field byte for an empty payload. It is not necessarily the
next physical byte or an arithmetically incremented offset. The current frozen
adapter installs this value as the child view's end position. A plain input
subview would instead expose the following byte's provenance and could change a
child framing error without changing the payload bytes.

The planned anchored-subview operation changes only the end position. Frozen
backing uses its existing constant-size subview and `root_end`, rebased to the
exact child window. Borrowed backing carries an optional end override inside
that variant, not an additional field on every region. An interior subview uses
the next byte's provenance; one ending at the anchored boundary inherits the
anchor. Empty views use the supplied anchor without inventing a payload byte.

The following implementation bindings must follow verified span/anchor laws:

| ID | Required invariant and discriminating witness |
|---|---|
| IST-01 | Consumed indexed prefix equals the independently accumulated located-byte reference, including all source IDs and offsets. |
| IST-02 | Checked slices agree for empty, nested and overflow cases; reject a range inside full input but beyond consumed bytes. |
| IST-03 | Anchoring preserves bytes and byte locations while replacing only the end position; test borrowed/frozen, empty/nonempty, interior/equal-end nested windows and discontinuous sources. |
| IST-04 | Ordering depends only on byte values. A mismatch reports the current key's source; equal and proper-prefix cases retain the supplied endpoint fallback. |
| IST-05 | Suspended child spans retain their denotation after parent advancement or teardown through the external borrowed lifetime or shared frozen backing. |
| IST-06 | Validity, stability, errors, provenance, pending IDs/kinds and completion anchors match the retained arena reference after each byte/resolution, including wrong-ID and post-Dead traces. |
| IST-07 | Reusable-frame ownership preserves span denotation when slots are retired/reused; no selected span borrows from a retired frame. |
| IST-08 | Backing identity and actual transport activity demonstrate absence of payload/provenance copies; report view/frame layouts, retained backing and byte visits separately. |

#### Verified model and executable correspondence

`RPB3Admission.IndexedWindowTransport` adds twenty-two closed capstones. The
complete theory now prints sixty-two closed reports and passes independent
kernel checking. Its located-byte reference accumulates the consumed prefix
independently; it does not define correctness by calling the indexed algorithm.
The following inventory is exhaustive for the new reported capstones. Supporting
lemmas remain in the same module.

| Contract | Closed capstones | Executable binding and remaining obligation |
|---|---|---|
| IST-01 | `accumulated_located_prefix_is_exact`; `indexed_slice_equals_independently_accumulated_prefix`; `indexed_and_append_reference_transport_observations_are_identical` | Full consumed-prefix/arena trace differential must accompany the standalone transport cutover; it is not discharged by subview tests alone. |
| IST-02 | `checked_window_accepts_exact_consumed_and_integer_bounds`; `checked_window_has_exact_length_and_representable_absolute_end`; `nested_windows_preserve_owner_bounds_and_exact_slice`; `checked_empty_windows_do_not_read_a_following_byte` | Window properties cover nested, reversed, outside-input and offset-overflow ranges. Future-byte refusal against the consumed prefix remains a transport binding. The empty-window theorem proves empty denotation, not measured read count. |
| IST-03 | `explicit_completion_anchor_changes_only_the_end`; `earlier_and_equal_end_subviews_select_the_correct_boundary`; `indexed_byte_positions_preserve_exact_physical_provenance`; `rebased_frozen_and_inline_anchored_windows_have_identical_positions`; `completion_anchor_comes_from_the_final_consumed_byte`; `indexed_and_rebased_windows_preserve_source_refinement` | Anchored-window tests exercise all four test backings, empty/interior/equal-end windows, discontinuous sources and replacement anchors. Existing payload/empty-obligation controls must also pass through the new child adapter. |
| IST-04 | `located_comparison_refines_content_only_lexicographic_order`; `first_content_mismatch_reports_the_current_physical_byte`; `equal_and_proper_prefix_comparisons_preserve_endpoint_fallback` | Content equality/order survives anchoring. Exact mismatch and proper-prefix fallback must additionally bind to the new reducer read policy. |
| IST-05 | `suspended_window_retention_survives_parent_release` | Parent teardown, original arena mutation and shifted contiguous-owner teardown preserve child bytes/locations. Rust ownership and shared backing implement retention; the model is not a proof of `Arc` internals. |
| IST-06 | `pending_preflight_refuses_before_state_world_or_consumed_offset_advance`; `shared_grammar_and_span_correspondence_preserve_full_protocol_observations` | Existing wrong-ID, pending-child and post-Dead trace controls remain mandatory after cutover. The theorem assumes correspondence to the shared grammar; it is not an extracted-parser equivalence proof. |
| IST-07 | `captured_descriptor_survives_recycling_its_frame_cell` | New span descriptors must be copied values independent of frame-cell identity. Existing reusable-frame ownership controls remain required with the new span type. |
| Causal controls | `concrete_window_anchor_lifetime_and_ordering_positive_controls`; `concrete_transport_causal_controls_reject_future_bytes_wrong_anchors_and_borrowed_frames` | Distinguish full-input bounds from consumed-prefix bounds, next-byte positions from completion anchors, retained backing from released backing, and copied spans from recycled-cell lookups. |

Physical source offsets additionally require the separate checked-origin premise
in `borrowed_physical_offsets_require_a_separate_checked_origin`; logical window
bounds alone do not establish physical offset representability. IST-08 is an
executable resource obligation, not an allocation theorem in this model.

The helper binding is
[`indexed_span_transport.rs`](../../../models/tests/support/indexed_span_transport.rs):
`anchored_windows_preserve_bytes_locations_bounds_and_shared_backing` (128 generated
cases); `nonterminal_and_empty_windows_preserve_supplied_completion_anchors`;
`anchored_windows_reject_bad_ranges_and_checked_offset_overflow`;
`anchored_children_survive_parent_teardown_and_original_arena_mutation`;
`anchored_contiguous_children_preserve_shifted_origins_after_owner_drop`; and
`anchored_metadata_fits_the_original_indexed_region_layout`.
The gate requires all six, with no ignored tests. Nonempty owned windows share
their original allocation; an empty `Bytes` slice may use the library's empty
representation because it has no payload to retain. Frozen windows share the
immutable backing index, not a newly constructed page index.

#### Baseline and acceptance boundary

The existing flat-key layout probe, extended only with two measurements, passes
before any indexed transport implementation. On the current test build,
`IndexedByteRegion` is **72 bytes**, `CanonicalPathFrame` **664 bytes**, and the
outer admission `Frame` **1,136 bytes**. The indexed region and outer frame must
not grow; removing the standalone arena should reduce its own frame footprint.
These are current-toolchain layout observations, not a portable ABI contract.
The capped baseline log SHA-256 is
`7ddff6de7b8dde5515e81f2e92aa05149e30ca8b3c95dcb21dc3ae2b5e4c8d48`
(pgmcp artifact 534). Its only source difference from `b4d5a212a` is additional
test output; no production transport is changed at this checkpoint.

Reuse the existing frozen-payload, empty-anchor, indexed subview/provenance and
standalone trace/recovery tests. Extend their discriminating cases rather than
creating a second semantic oracle. Allocation evidence must come from backing
identity or executed storage activity, not a counter hardcoded to zero. Fixture
construction is outside the measured operation and teardown is reported
separately.

This refinement alone does not eliminate repeated payload traversal or enclosing
Region reconstruction. Checked opaque-payload bulk consumption and absolute
region endpoints remain subsequent required steps. Their proofs must preserve
enclosing bounds and child-first error order, and may not skip local UTF-8 or
grammar validation. No importer or persistence backend is activated by this plan.

#### Indexed read-policy runtime binding

The standalone reducer now specializes the shared grammar with
`FrameStorage<CanonicalByteRange>` and retains a checked `consumed: usize`.
`ArenaPathBytes` preserves persistent ACT handles, append and freeze behavior;
`IndexedPathBytes` borrows the already-retained input during each operation.
Neither policy owns another parser. `InnerPath` and frame IDs remain independent
of byte-buffer lifetimes, and a completed child receives an anchored input
subview through the same region-taking constructor as the frozen ACT adapter.

The new executable bindings in
[`canonical_path_byte_policy.rs`](../../../models/tests/support/canonical_path_byte_policy.rs)
are:

| Tests | Model obligations exercised |
|---|---|
| `indexed_byte_policy_matches_accumulated_consumed_prefix` | IST-01/02/03: 128 generated prefix/range cases against an independently built arena prefix; future-but-present input is rejected, and byte provenance cannot substitute the consumed end boundary. |
| `indexed_byte_policy_rejects_invalid_bounds_without_endpoint_substitution` | IST-02: reversed, future, maximum-offset and over-consumed spans; physical-origin overflow; checked consumed-increment failure leaves counters unchanged. |
| `indexed_byte_policy_orders_content_and_reports_current_mismatch_or_endpoint` | IST-04: both strict orders, equality and both proper-prefix directions; current-key mismatch source and endpoint fallback agree with the arena reference. |
| `indexed_byte_policy_captured_range_survives_source_owner_and_frame_replacement` | IST-05/07: the actual reusable frame slot is replaced while a retained anchored child keeps its bytes and provenance after source-owner teardown. |
| `indexed_byte_policy_pending_preflight_preserves_consumption_but_dead_edges_advance` | IST-06: pending refusal precedes consumption/statistics changes; accepted post-Dead edges consume input; exceeding input refuses without advancement. |

The existing fifteen standalone tests retain their semantic witnesses. Their
byte/resolution trace adapters prebuild immutable input, including post-Dead
suffixes, and compare consumed prefixes and completed payload locations with the
persistent arena reference. Wrong-ID/pending attempts additionally assert no
consumed-offset change. A sixteenth test,
`standalone_children_share_borrowed_payloads_and_survive_parent_drop`, checks the
actual RawPar and nested-EPM child constructor for direct payload-pointer
identity, completion anchors, unconsumed suffix and parent teardown. The layout
probe checks the original borrowed input's pointer and actual cursor counts,
rather than reporting a constant zero-copy counter.

The focused runner requires sixteen standalone, five byte-policy, eight
frame-storage and six anchored-window tests. These bindings do not remove
per-byte nested Region reconstruction or duplicate visits by separately required
child validators; whole-pipeline physical linearity remains an independent gate.

### Strict opaque-payload interiors: next refinement (task 6018)

The private standalone driver ignores non-final `ObligationByte` events but
currently visits every opaque payload byte before dispatching the authoritative
child validator. Under a nested Region, each such visit also reconstructs the
enclosing owner frames. The optimization must preserve the public scalar API:
batching is an internal driver operation, not an equivalent public event stream.

An interior batch decrements the payload remainder and every enclosing Region
and framed-path remainder by the same width. Choose that width as one less than
the minimum of those remainders and available input, saturating at zero. The
minimum must be bounded by available input before converting a declared `u64`
length to `usize`; oversized declarations must retain their existing eventual
truncation behavior. Keeping the last byte outside the batch leaves completion,
truncation, child dispatch, and exact physical anchors in the scalar grammar.

```text
Algorithm STRICT-OPAQUE-INTERIOR(input, current)
1. Refuse batching unless the selected path and all its owners are live,
   with a nonempty opaque payload rather than a pending child.
2. Find the minimum payload/owner/path remainder and available input.
3. Keep its final byte for the scalar grammar; if none remain, do nothing.
4. Decrement each participating remainder once by the interior width.
5. Advance the existing indexed cursor and logical consumed count equally.
6. Process the next byte through the unchanged scalar boundary rules.
```

The following catalogue records required model and executable obligations
before any runtime batching change. Model completion is not claimed here.

| ID | Required invariant and distinguishing witness |
|---|---|
| SOI-01 | The chosen width is strictly below every participating remainder and available input; zero/one-byte and empty cases do not batch. |
| SOI-02 | One bulk decrement equals repeated scalar interior decrements for every owner and payload; metadata, IDs, stability and sibling spans remain unchanged. |
| SOI-03 | No completion/error boundary is crossed. Test a child ending before, at and after its enclosing boundary; the existing scalar boundary transition remains authoritative. |
| SOI-04 | Cursor position and logical consumption advance equally. The next byte and its discontinuous physical provenance are unchanged across borrowed and frozen backing and checked seeks. |
| SOI-05 | Only ignored non-final payload events are omitted. The scalar API, final event, child dispatch, exact payload window and last-consumed completion anchor remain unchanged. |
| SOI-06 | Dead, pending, nonopaque and stale states do not batch or mutate; wrong-ID and post-Dead scalar resolution/consumption behavior remains covered by the existing trace oracle. |
| SOI-07 | Arithmetic stays bounded by actual input without eagerly adding declared lengths to absolute offsets; test `u64::MAX` declarations against short input. |
| SOI-08 | Frame retirement preserves the selected owner graph and retained child spans; bulk skips do not introduce payload copies, endpoint replay or stack recursion. |
| SOI-09 | Record actual byte visits, batch count and owner work separately from consumed bytes, frame capacity, RSS and throughput. Width/depth controls must expose omitted enclosing bounds or batching the boundary byte. |

The pre-change `opaque_payload_cursor_resource_matrix_preserves_completion`
probe covers borrowed/frozen backing, payload widths 16 through 8,192 and
enclosing depths 0, 1, 8 and 32. It checks the child boundary and pending-parent
nonadvance while reporting actual cursor visits and retained frame capacity.
Fixture construction is outside the measured operation. This is a deterministic
resource probe, not a timing benchmark or a whole-pipeline linearity claim.

Absolute endpoints are a subsequent, distinct refinement: merely replacing
countdowns still reconstructs the embedded child/owner spine. Additionally,
post-Dead input consumption can diverge from saved-graph accounting. A later
focused-owner design must explicitly model that distinction; this interior
batch does not silently reinterpret saved remainders as end minus input position.

#### Verified strict-interior model and required bindings

`StrictOpaqueInterior` now has fourteen closed capstones. The complete RPB3
theory compiles with 76 closed reports and passes independent kernel checking
before a runtime batching implementation. Its scalar reference independently
uses predecessor/successor updates; it does not invoke the bulk decrement.

| Capstone | Invariant binding |
|---|---|
| `planner_returns_the_exact_positive_strict_minimum` | SOI-01: every owner/path/payload/input bound participates. |
| `a_fresh_plan_is_accepted_without_rechecking_payload_bytes` | SOI-01/07: a current checked plan needs no byte inspection. |
| `strict_bulk_equals_repeated_scalar_countdown` | SOI-02: independently repeated scalar steps produce the same residual state. |
| `successful_bulk_preserves_all_metadata_and_positive_residuals` | SOI-02/03: IDs, kind, source, start, anchor, stability and saved owner values are unchanged. |
| `only_ignored_nonfinal_payload_events_are_erased` | SOI-05: projection discards interior events, not final events or boundary observations. |
| `bulk_cursor_consumption_preserves_exact_next_located_byte` | SOI-04: consumed prefix and next physical byte remain exact. |
| `accepted_bulk_refines_the_actual_checked_input_prefix` | SOI-02/04: the reference runs on the actual selected input bytes. |
| `every_intermediate_prefix_stays_before_all_completion_boundaries` | SOI-03: every intermediate state, not only the final state, remains strictly interior. |
| `the_following_scalar_boundary_uses_the_same_state_and_physical_byte` | SOI-03/05: conditional on using the same boundary transition. |
| `pending_dead_and_nonopaque_modes_do_not_plan_or_advance` | SOI-06: ineligible modes do not produce an updated state. |
| `stale_origin_or_obligation_id_refuses_without_an_updated_state` | SOI-06: stale origin and ID refuse without mutation. |
| `declared_lengths_larger_than_the_machine_limit_are_bounded_before_conversion` | SOI-07: available input bounds the batch independently of large declarations. |
| `strict_bulk_positive_boundary_and_projection_controls` | SOI-01–07: concrete successful controls. |
| `strict_bulk_causal_controls_reject_missing_bounds_boundary_crossing_and_stale_requests` | SOI-01/03/06: omitted-owner, included-boundary and stale-request controls. |

Actual Rust grammar correspondence, selected frame ownership (SOI-08), allocator
and global obligation-clock behavior, and measured cursor/owner work (SOI-09)
remain executable binding obligations. The boundary theorem is conditional, not
a proof of a different parser. The model does not establish absolute endpoints,
whole-pipeline linearity or throughput. Journal J-095 records the pre-change
resource baseline and the independently checked proof artifacts.

#### Rust strict-interior binding

The stream reducer exposes a checked plan containing consumed origin, obligation
ID and positive width. Application recomputes eligibility and bounds, permitting
a narrower width but refusing stale or oversized requests without mutation.
The selected graph must be Open at every enclosing owner and at its payload.
One shared `feed_path` owner loop performs either a scalar byte step or the
verified interior decrement; its existing selection/retirement journals remain
responsible for frame ownership. The batch creates no obligation and preserves
the global obligation clock. The public persistent and standalone `edge` APIs
continue to emit every original scalar event.

The private driver plans only when it receives `ObligationStarted`, avoiding
planning walks on ordinary grammar bytes. It applies the strict interior,
advances the existing checked cursor and leaves the next byte scalar. Borrowed
and frozen cursors reuse their existing fast-forward implementations; the
test-only borrowed-arena cursor retains its scalar compatibility traversal.
`IndexedByteReader::skip_exact` uses the same checked cursor operation. A rejected
skip preserves position and counters, including overflow-sized requests.

| Executable binding | SOI obligations |
|---|---|
| `opaque_interior_plans_match_independent_all_owner_minimum` | 01/07: generated lengths and owners versus an independent widened minimum. |
| `opaque_interior_matches_independent_scalar_steps_and_public_events` | 02–05/08: separate pools and normalized full graphs, scalar events, next-byte transition and exact child descriptors. |
| `opaque_interior_narrower_chunks_compose_with_pool_reuse` | 02/04/05/08: multiple narrower requests, exact completion anchors and stable slot occupancy. |
| `opaque_interior_stale_and_invalid_plans_are_exact_noops` | 01/06/07: origin/ID/width/input refusal snapshots include physical slots, journals, counters and clock. |
| `opaque_interior_mode_guards_cover_every_owner_and_leaf` | 06/08: every owner depth, Dead/SplitComplete, pending and nonopaque leaf states. |
| `opaque_interior_boundary_matrix_retains_scalar_refusal_and_final_anchor` | 03–06: independent Region/path/payload/input boundaries at depths 0/1/2/8, including subsequent scalar steps. |
| `opaque_interior_huge_lengths_are_clipped_before_conversion_or_addition` | 01/03/07: `u64::MAX` residuals, short input and near-maximum consumed positions. |
| `opaque_interior_keeps_persistent_forks_and_public_scalar_events_unchanged` | 02/05/08: independently retained ACT fork, original public events, exact payload, resolution and global clock. |
| `checked_opaque_cursor_skips_preserve_next_bytes_and_refuse_without_mutation` | 04/06/07: 128 generated cases across borrowed, protobuf-contiguous, arena and frozen backings. |
| `opaque_batching_keeps_enclosing_truncation_before_child_dispatch` | 03/05: actual encoded overlong child fails at its enclosing boundary before child dispatch. |
| `opaque_batching_seeks_without_traversing_payload_interiors` | 04/05/09: actual RawPar child validation, exact anchor, scalar suffix and measured compatibility fallback. |
| `opaque_payload_cursor_resource_matrix_preserves_completion` | 02–05/08/09: 32 encoded width/depth/backing cells with paired scalar descents and exact pending-child observations. |

The two generated graph tests and cursor property each use 128 cases. Synthetic
graphs deliberately exercise every local bound even where a complete encoded
path would establish additional relations; encoded driver fixtures supply the
reachable-path evidence. Previous scalar wrong-ID/post-Dead traces remain
unchanged. `edges` and consumed bytes remain logical counts. Test-only
`bulk_operations`, `bulk_bytes` and `region_descents` measure batches, skipped
bytes and descents in `feed_path`; the last does **not** include the two planner
walks. Those walks occur once per plan/application, not per skipped byte.

The focused runner now requires twenty standalone and eight interior tests in
addition to the existing frame-storage, span and byte-policy tests. No claim of
whole-pipeline linearity follows from this binding: ordinary nested Region
reconstruction and persistent ACT payload handling retain their separate
required acceptance boundaries.

### Focused Region owners: representation and accounting contract (task 6018)

The strict-interior implementation does not eliminate ancestor walks for ordinary
grammar bytes, queries or child resolution. A **focused path** is the currently
active leaf parser. Its **owner spine** holds the suspended enclosing Region
metadata and continuations, innermost last in the planned Rust vector. Detaching
the leaf permits ordinary byte transitions and obligation queries without
traversing or rewriting the spine. This is a representation refinement of the
existing grammar, not a new canonical codec or parser language.

`FocusedRegionOwners` models exact path metadata, Region lengths, previous
located-byte spans and reusable-frame continuation graphs. Its model lists are
innermost first; Rust vector orientation must be bound explicitly. **Reification**
means rebuilding the original nested graph from the focus and suspended owners.
The model keeps a separate **graph stamp**, the consumed position to which saved
Region residuals belong. The public input-consumption clock may advance after a
Dead state while the selected graph remains unchanged. Subtracting the public
clock from every saved endpoint would therefore change observable behavior.

Absolute endpoints require arithmetic wide enough for the sum of a machine
position and a declared `u64` remainder. The natural-number proof establishes
the bound, not Rust overflow safety by itself. Runtime must bind that bound to
its chosen representation and preserve short-input handling for `u64::MAX`
declarations. This is not a change to multiplicity or language integer types.

```text
Algorithm FOCUSED-REGION-STEP(input, focus, owners, graph_stamp)
1. Check the focused obligation and cached enclosing minimum before consuming.
2. Retain the pre-step focus and any owners touched by this transaction.
3. For an ordinary interior byte, run the existing shallow grammar exactly once;
   advance the graph stamp without rewriting suspended owners.
4. Settle the deepest due path using the existing local Region completion;
   suspend a pending child at zero. A due outer path with unfinished inner
   owners must still report truncation. After successful settlement, detach
   newly entered owners at the post-byte stamp.
5. Commit the selected graph and retire only unreachable slots. An outer refusal
   selects the exact pre-step graph, while retaining the grammar's global clock.
6. Reify a selected fault graph once and continue through the same shared fault
   protocol. Do not treat Dead as absorbing or replay healthy input prefixes.
```

The following complete binding catalogue governs focused-runtime qualification.
The original scalar controls establish the reference; the implemented binding
and its independent differential checks are described after the scheduler laws.

| ID | Required invariant and executable witness |
|---|---|
| FRO-01 | Decompose/reify preserves every path status, tag/source, stability bit, segment count, Region/path remainder, declared length, start, previous span and continuation. Compare independently allocated normalized graphs, including empty and deep spines. |
| FRO-02 | Detached owner slots, focused frames and continuation graphs are disjoint; free slots do not overlap any selected root. Check push/pop, local replacement, failed transactions and retained payload spans against the actual slot pool. |
| FRO-03 | Widened endpoints reconstruct exact residuals at the graph stamp. Advance the stamp only when the selected graph advances; test near machine limits, `u64::MAX` declarations, short input and post-Dead consumption. |
| FRO-04 | A push stores exact prefix caches and a pop restores them in constant time. The all-bound cache includes Region and path endpoints with outermost tie-breaking for pre-byte refusal. A separate path-only cache uses deepest tie-breaking for post-step completion. Test equal endpoints and Region-only expiration. |
| FRO-05 | The ordinary operation invokes the identical shallow grammar and preserves exact public events, first-tag effects and the global obligation clock. Prove pointwise action correspondence and compare scalar traces; assert zero owner rewrites on strict ordinary interiors. |
| FRO-06 | Successful equal-boundary completion removes only the consumed innermost suffix. Pending children at zero preserve all owners. A due outer path with unfinished inner owners truncates at the current byte; a shallow error at that byte takes precedence. New owners are charged only after their opening length byte. Bind actual sibling ordering, malformed ancestors and completion provenance to `finish_region_path`, not only to suffix arithmetic. |
| FRO-07 | Wrong-ID, wrong-phase and pending-edge refusal preserve parser state and selected physical slots. Compare exact snapshots and subsequent valid operations. |
| FRO-08 | Returned errors preserve the old state; outer deferred refusals select the immediately preceding graph, not the graph at focus entry, while preserving permitted global-clock advancement. Shallow fresh-frame failure retains its distinct selection rule. |
| FRO-09 | Reified faults use the existing shared protocol. Later scalar consumption and private resolution retain nonabsorbing Dead behavior, with no second grammar and no repeated healthy-prefix reification. |
| FRO-10 | Borrowed/frozen cursors, checked opaque skips, child spans, exact physical anchors and pending dispatch are unchanged. Repeat existing cursor, boundary and provenance properties through the focused driver. |
| FRO-11 | Count all owner work: active/completed queries, planning, resolution, feed descent, rebuild, detachment and reification. Width/depth, repeated-child and cascading-completion controls must expose hidden ancestor scans. Report capacity, bytes, visits and time separately. |
| FRO-12 | Public persistent ACT forks and the scalar event protocol remain unchanged. Retain independent fork/restore witnesses and tiny-stack admission plus teardown, without depth limits or new byte arenas. |

The model's ordinary-action equivalence is conditional on the shared shallow
operation. The initial suffix theorem proves exactly which owners are removed
and the pending suspension rule. The subsequent finite scheduler refinement
below covers deepest-due settlement and malformed-ancestor observations.
Physical slot mutation, Rust-width arithmetic, the real grammar binding and
whole-pipeline resource slopes remain explicit implementation obligations. No
full focused parser correctness or production activation follows from this
model alone.

#### Pre-change all-owner-work baseline

The new module compiles with eighteen closed capstones (94 reports in the full
RPB3 theory) and passes independent kernel checking before focused runtime work.

| Capstone | Binding |
|---|---|
| `concrete_decomposition_reifies_every_path_and_region_field` | FRO-01: exact normalized graph reconstruction. |
| `cached_minimum_push_and_pop_preserve_the_exact_prefix` | FRO-04: exact cached prefix after push/pop. |
| `detached_spine_preserves_exact_frame_membership_and_count` | FRO-01/02: no logical frame is lost or added. |
| `detached_region_child_and_outer_continuation_are_disjoint` | FRO-02: selected child and held owner/continuation do not alias under distinct graph slots. |
| `widened_endpoints_recover_exact_residuals_without_early_overflow` | FRO-03: mathematical sum bound and exact subtraction. |
| `advancing_graph_stamp_decrements_every_owner_without_rewriting_spine` | FRO-03/05: reified residuals equal eager decrements. |
| `cached_minimum_checks_all_enclosing_region_and_path_endpoints` | FRO-04: no enclosing bound is omitted. |
| `zero_boundary_selection_is_outer_first_with_no_skipped_predecessor` | FRO-04/06: first expired owner in outer-first order. |
| `ordinary_shared_shallow_action_refines_eager_owners_and_preserves_exact_events` | FRO-05: exact event/clock and unchanged spine. |
| `explicit_shared_grammar_binding_transfers_ordinary_observations` | FRO-05: pointwise leaf-action agreement implies owner-fold agreement by induction. |
| `incremental_equal_boundary_completion_only_removes_the_consumed_suffix` | FRO-06: exact popped suffix and count, conditional on local completion. |
| `pending_child_at_zero_boundary_preserves_all_owners_without_completion` | FRO-06: pending suspension precedes completion. |
| `wrong_obligation_id_refuses_without_a_candidate_or_owner_change` | FRO-07: wrong-ID resolution produces no candidate. |
| `transaction_selection_uses_the_pre_step_graph_for_outer_refusal_but_keeps_global_clock` | FRO-08: exact pre-step graph and independent clock selection. |
| `shallow_fresh_frame_selection_and_returned_error_are_distinct` | FRO-08: shallow candidate versus returned-error rollback. |
| `reified_fault_operations_use_the_exact_same_shared_protocol_without_reaccounting` | FRO-09: shared post-fault behavior without residual reinterpretation. |
| `focused_representation_accounting_completion_and_selection_positive_controls` | FRO-01/03–08: concrete admissible controls. |
| `focused_causal_controls_reject_wrong_stamps_wrong_boundary_order_and_dead_absorption` | FRO-03/04/07–09: distinguish wrong stamp, wrong order, wrong ID, stale graph and absorbing-fault mutations. |

`ordinary_region_work_matrix_tracks_all_owner_walks` drives complete encoded
byte arrays and tuples of nested map obligations through borrowed and frozen
inputs: widths 1/16/128/1,024 and depths 0/1/8/32, for 64 cells. It validates all
dispatched children, exact final summaries, logical consumption, pending counts
and frame ownership. Ordinary-byte expected walks are independently calculated
as the sum of encoded child lengths before each enclosing Region is added.
Both backings must produce identical work, visit and capacity observations.

`CanonicalPathOwnerWork` is test-only observation state, separate from semantic
parser state: reads increment query counters without changing the parser graph.
It adds the planner, query, resolution and rebuild walks omitted from the earlier
feed-descent-only counter. The probe counts the parent, not fixture construction
or independent child-validator work, and is not a timing benchmark. J-097 records
the observations and final qualification artifacts.

#### Verified deepest-due scheduler and encoded reference controls

Ten further capstones complete the finite scheduler refinement inside
`FocusedRegionOwners`: 28 capstones in that module and 104 reports in the full
RPB3 theory. Both compilation and independent kernel checking pass before the
focused runtime changes. The eager reference examines owners inside-out;
the optimized model reads stored prefix caches and visits only successful
completions plus at most one stopping owner. Both return the same finite
Dead/Pending/Unfinished/Completed observation, completion-call trace and pop count.

| Capstone | Binding |
|---|---|
| `both_prefix_caches_preserve_push_pop_and_opposite_tie_rules` | FRO-04: exact cached selection with outermost all-bound ties and deepest path-bound ties. |
| `deepest_due_settlement_equals_eager_unwind_under_exact_local_completion` | FRO-06: equality of finite observations, local call order and pop count. |
| `settlement_refinement_needs_only_pointwise_local_completion_authority` | FRO-05/06: local completion agreement transfers to the entire owner fold. |
| `pending_zero_boundaries_and_existing_dead_errors_do_not_invoke_completion` | FRO-06/09: preserve pending and prior errors before boundary handling. |
| `skipped_unfinished_inner_owners_refuse_at_the_current_endpoint` | FRO-06: an outer due path cannot be missed behind unfinished inner owners. |
| `focused_settlement_checks_are_bounded_by_successful_pops_plus_one` | FRO-11: finite scheduler-check bound, not an allocation or whole-pipeline bound. |
| `settlement_preserves_exact_returned_error_public_event_clock_and_selected_world` | FRO-07/08: scheduler equivalence transfers through the shared transaction selector. |
| `refusal_after_successful_pops_restores_immediate_pre_step_graph_and_keeps_event_clock` | FRO-08: failed completion after one or more pops retains the correct pre-step graph. |
| `boundary_scheduler_equal_endpoints_pending_and_rollback_positive_controls` | FRO-04/06/08: two inner successes followed by an outer refusal, and pending-at-zero. |
| `boundary_scheduler_causal_controls_reject_wrong_ties_gap_completion_and_region_only_expiry` | FRO-04/06: distinguish opposite ties, unfinished gaps, shallow-error precedence and Region-only expiration. |

The correspondence premises are valid stored caches, non-overdue path endpoints,
live owner representation, accurate leaf classification, pointwise agreement
with the existing completion helper, and correct candidate-graph/transaction
construction. The model does not prove physical slot materialization or hide
fault reification inside its finite scheduler-work bound.

Three executable reference controls were added before changing leaf resolution:

| Test | Discriminating observations |
|---|---|
| `focused_owner_opening_byte_is_not_charged_to_the_new_child` | Valid `[0x0d,3,2,0x03,0]`, then every wrapping depth through 32, against scalar history and the independent canonical validator. |
| `focused_owner_outer_expiry_keeps_shallow_error_precedence_and_provenance` | `[0x0d,5,4,0x0d,5,4,0x03,0]` truncates at index 6; replacing that index with `0xff` produces `ReservedTag(255)`, with exact borrowed/frozen provenance and unchanged error through later input. |
| `focused_owner_duplicate_refusal_follows_nested_completion_and_pending_resolution` | Duplicate nested paths at depths 1/2/8/32 fail after inner completion; actual nested-map child admission/resolution is exercised. Post-Dead re-resolution changes the error to `NestedEpmNotMap`, distinguishing a wrongly absorbing fault. |

`resolve_leaf_obligation` now owns the original matched leaf rules, wrong-ID and
wrong-phase checks, stability propagation and frame-selection checkpoint. The
existing owner traversal delegates to it without changing input consumption,
obligation IDs or owner unwinding. The planned focused traversal will call this
same helper. No second resolution grammar or focused runtime activation is
introduced by extracting the helper. The combined runner requires 24 standalone
tests alongside the existing groups. J-098 records exact qualification artifacts.

#### Implemented focused-owner binding (2026-09-09)

The stream implementation now uses an outer-to-inner owner vector, widened
Region/path endpoints, opposite-tie prefix caches and a separate graph stamp.
Reserved Region slots have detached child links. Completion and resolution share
the original helpers; selected faults reconstruct the immediate pre-step graph
and resume the existing fault protocol. J-099 records exact scope and measurements.

| Catalogue | Runtime witness |
|---|---|
| FRO-01/03/05/08/09 | Independent projected graphs, canonical slot renaming and eager byte/resolution reference after each step; generated nested mutations, boundary controls and post-Dead suffixes retain metadata, clock and provenance comparisons. |
| FRO-02/04 | Physical selected/free partition, detached-child nonaliasing and independently computed cache minima at every normalized observation; existing synthetic boundary and machine-width controls. |
| FRO-06/07 | Existing opening-byte, expiry, duplicate-refusal, pending-edge and wrong-ID recovery tests now exercise focused owners; returned-error and no-op checks remain. |
| FRO-10/12 | Opaque-interior plan/no-op properties, borrowed/frozen transport, retained child spans, persistent scalar-history comparison and tiny-stack tests. |
| FRO-11 | Eager ordinary-byte control with independent length-sum formula; 64-cell parent resource matrix includes all queries, descents, detachments, reifications, settlement checks and owner-vector capacity. |

The combined runner now requires 25 standalone tests. No extra theorem is
claimed for this Rust binding: the same 104-report RPB3 theory predates the
implementation. The properties connect its premises to the actual shared grammar
and physical representation; they are not exhaustive verification of all Rust
executions or proof of whole-pipeline linearity.

#### Persistent focused-owner refinement requirements (2026-09-09)

J-100 establishes the persistent shared-prefix baseline. The following are new
required proof/implementation bindings, not completed theorems. Reuse the FRO
model through a concrete linked-owner refinement; do not substitute latest-state
equivalence for arbitrary historical-handle preservation.

| ID | Required law and executable witness |
|---|---|
| PFO-01 | Appending state, owner and frame cells preserves every old checkpoint's selected graph and byte/provenance handle. Check old endpoints after every divergent sibling, including frames referenced by several states. |
| PFO-02 | Backward-linked owner nodes and cached all-bound/path-bound/outermost handles refine the existing owner list with exact opposite tie rules. Check links and independent projection after pushes/pops; no mutable shared child detachment. |
| PFO-03 | Ordinary leaf transitions allocate no ancestor replacement cells. Owner push, pop, root observation and Leave use bounded handle operations. Compare complete eager traces and count owner reads and allocations separately. |
| PFO-04 | Restoring any historical checkpoint selects its own graph stamp, bytes, metadata and pending obligation without rewinding the global obligation allocator. Fork distinct physical sources and resolve both children in different orders. |
| PFO-05 | Refusal after successful inner pops selects the immediate pre-step graph and preserves permitted global-clock effects. Require exact graph and error-source comparison, not only final validity. |
| PFO-06 | A focused fault denotes the exact reified graph with its root error. Edge/value/active/completed observations refine existing early-Dead behavior; checked materialization for private resolve/reject preserves nonabsorbing faults and returned-error nonmutation. Include many malformed value-free siblings, wrong IDs and wrong phases. |
| PFO-07 | Resource accounting includes historical states, new owner nodes, leaf frames, actual completion work, refusal handling and forced materialization. The bound must not hide depth-many work under ordinary edges, Leave or malformed siblings. Retain the independently predicted eager baseline and integrate actual EPM1 traversal/tiny-stack teardown. |

The chosen representation is an append-only linked owner arena with fixed-size
focused historical states. `FrameHistory` and existing byte handles remain
immutable. The outermost-owner cache is required for constant-depth `value()`;
the fault observation law is required before claiming malformed-topology
resource closure. Arena freeze page/leaf indexing remains outside this bound.
The runtime is not implemented at this checkpoint. The combined runner now
expects nine frame-storage tests, including the persistent baseline.

#### Persistent owner and fault foundations (2026-09-09)

`PersistentFocusedOwners` supplies sixteen new closed-report obligations before
runtime changes. Its append-only linked arena has concrete parent and cached
boundary/outermost handles. A `linked` witness includes exact lookup, strictly
backward parent references and list-selection correspondence. The constructor
theorem establishes those premises for an appended owner; extension preserves
every existing witness. The owner-list projection is used only for reification:
its unused FRO cached-minimum field is `None`, so no `F.cache_valid` theorem or
linked scheduler refinement is inferred from that projection.

| New law | Binding and scope |
|---|---|
| `immutable_extension_preserves_every_linked_checkpoint` | PFO-01: old linked observations survive arbitrary appended owner cells. |
| `cached_linked_reads_refine_exact_list_selection` | PFO-02: all-bound, path-bound and outermost cached handles agree with the reference selection on a valid linked chain. |
| `append_owner_constructs_valid_backward_links_and_exact_caches` | PFO-01/02: fresh owner construction establishes backward links and both caches, rather than assuming its validity. |
| `extension_preserves_exact_reified_graph_and_old_frame_lookups` | PFO-01: owner-graph observations and old frame lookups survive append. These are separate facts, not yet a concrete frame-ID realization relation. |
| `pop_restores_the_exact_preexisting_prefix` | PFO-03: selecting the parent restores the old chain and cached observations. No whole-parser cost bound follows. |
| `publishing_a_sibling_preserves_every_old_stamp_focus_and_byte_history` | PFO-04: every previously published state retains all its fields after a sibling is appended. |
| `historical_restore_never_rewinds_the_global_obligation_clock` | PFO-04: state selection is separate from the global allocator; subsequent IDs remain increasing in the mathematical model. |
| `equal_endpoint_caches_use_opposite_owner_handles` | PFO-02: exact opposite tie behavior for equal endpoints. |
| `refusing_hidden_leaf_replaces_only_the_original_root_error` | PFO-06: the shared child-Dead unwind preserves original child graphs while propagating the error to the root. |
| `root_error_replacement_preserves_all_slots_and_last_error` | PFO-06: error replacement retains graph slots and is last-error-wins. |
| `resolve_and_reject_preflight_preserve_their_distinct_phase_and_source_rules` | PFO-06: resolve requires AwaitingResolution; reject accepts Payload; no-stack errors use different sources. |
| `wrong_id_returns_the_hidden_tag_source_for_both_operations` | PFO-06: ID mismatch returns the retained hidden tag location. |
| `focused_fault_observations_match_the_exact_root_dead_branches` | PFO-06: queries and value observations use the exact stored root error independently of the live-grammar branch. |
| `dead_edge_preserves_selected_graph_stamp_and_clock_while_appending_current_bytes` | PFO-04/06: a successful Dead edge appends current bytes and history while retaining the saved graph/stamp and allocator. |
| `fault_forcing_uses_saved_graph_stamp_but_current_suffix_bytes` | PFO-06: required forcing inputs; this is a denotation pair, not executable checked materialization. |
| `fault_preflight_causal_controls_distinguish_payload_and_rolled_back_events` | PFO-06: pending phase and latest-event shortcuts produce distinguishable errors. |

The Rust reference test
`persistent_fault_private_preflight_uses_selected_graph_not_latest_event` pins
two discriminating traces. `[0x0d,4,3,0x10,2,0xaa]` retains the pre-step payload
remainder 2 after outer truncation: matching-ID resolve refuses at the tag while
reject succeeds at the caller source without new frames. `[0x0d,3,2,0x10,2]`
emits a fresh obligation but restores the old length-varint frame: resolve and
reject must not use the emitted obligation as their hidden terminal. Exact old
states, frames, counters, allocator and error sources are checked.

Before persistent runtime implementation, complete linked-to-scheduler and
frame-realization refinement, hidden-terminal correspondence, checked forcing
and their full resource accounting. These foundation laws and reference traces
do not complete PFO-01–07 or prove the Rust implementation. The combined runner
requires 120 total RPB3 reports and ten frame-storage tests. J-101 records the
qualification and precise remaining premises.

#### Persistent cache-to-scheduler correspondence (2026-09-09)

Seven further `PersistentFocusedOwners` laws connect arena addresses to the
existing positional scheduler. `cache_matches` relates equal cached endpoints
and a successful `nth_error` lookup from an owner-chain position to an arena
address. Arena addresses, chain positions and Region frame slots are different
identities; the proof neither equates them nor assumes contiguous addresses.

| Law | Extracted invariant and required implementation property |
|---|---|
| `address_caches_refine_positional_selection_without_address_arithmetic` | Both preferences preserve the selected endpoint and position-to-address mapping. Generate arbitrary address gaps, endpoint orders and ties; compare against the independent positional fold. |
| `linked_cached_reads_refine_the_existing_scheduled_owner_caches` | Reads from every valid historical root match `schedule_owners` caches, including after forks and append-only extensions. |
| `due_address_is_exactly_the_translated_scheduler_position` | Equal-stamp filtering preserves `None` and exact selected addresses; no default-valued lookup may substitute for an absent position. |
| `selected_cached_address_is_a_reachable_historical_owner` | The selected position resolves to the same cell in both the chain and arena. Test non-current roots and intervening unrelated allocations. |
| `selected_root_address_is_exactly_position_zero` | Strictly backward links exclude aliasing the root with a deeper chain position. Bind head-versus-ancestor scheduler branches to this identity. |
| `cached_address_due_refines_exact_scan_under_no_overdue_boundary` | With every endpoint at least the current stamp, cached due selection equals the exact reference scan. Include expired-boundary negative controls; this premise cannot be omitted. |
| `concrete_cached_target_has_exact_endpoint_and_arena_identity` | The actual selected cell has the cached endpoint, not merely a matching arena address. Bind both all-bound and path-bound projections. |

The pure correspondence needs no stamp bound. The scan refinement does: at
stamp 5, endpoints `[4,5]` have cached minimum 4 and cannot use that minimum to
select the due endpoint 5. Equal all-bound endpoints retain the outer owner;
equal path endpoints select the inner owner. `projected_owners` still carries
unused `None` FRO minimum fields: `schedule_owners` constructs its own valid
scheduler caches, so no unsupported `F.cache_valid` claim is required.

This table is the complete implementation-property catalogue for these seven
new laws, not evidence that the persistent Rust representation implements them
yet. Existing stream cache assertions and persistent fork baselines remain
reference controls. Before production changes, bind linked completion/pop
transitions, concrete frame realization, selected-fault/hidden-terminal coherence,
checked forcing, shared clock effects and resource accounting. A cached lookup
in this list-based mathematical model does not prove Rust constant-time cost.
The focused runner now requires 127 RPB3 reports; runtime filters are unchanged.

#### Linked completion machine and historical frame binding (2026-09-09)

`linked_command` performs one address-based dispatch: existing Dead/Pending and
empty-root states stop without owner checks; a live root reads one owner and its
cached due address. No due address stops unfinished, a non-head due address
refuses as truncated, and a completed child at the head invokes the existing
local completion authority. An invalid arena root returns `None`; valid linked
roots are proved to produce the exact positional command.

`linked_settlement` specifies repeated dispatch without a fuel parameter or depth
cap. A successful completion follows the stored parent; refusal and stop retain
the current candidate root. Completion calls record the Region **frame slot**,
not its arena address. Termination is proved by induction over the finite valid
owner chain. Determinism then strengthens existence of a matching run into an
all-runs correctness theorem.

| Law | Extracted invariant and implementation-property obligation |
|---|---|
| `linked_dispatch_refines_exact_positional_command` | Every valid root chooses the same stop/complete action, parent, owner metadata and child as the positional model. Generate empty/live/Dead/Pending states, arbitrary address gaps, no-due/head/ancestor cases and equal endpoints. |
| `linked_settlement_terminates_with_exact_observations_checks_and_retained_prefix` | The run terminates with exact result, call order, successful-pop count and check count; the final candidate root denotes exactly the unconsumed chain. Generate multi-pop success, inner success followed by outer refusal, and historical sibling restores. |
| `linked_settlement_has_eager_semantics_and_pop_bounded_owner_work` | Under no-overdue endpoints, exact eager observations hold and owner checks are at most successful pops plus one. Count dispatch separately from local completion and physical storage costs. |
| `linked_settlement_is_deterministic` | Fixed arena, input view and completion/classification authorities admit one final root/result/check triple. Repeated runs against immutable historical roots must agree. |
| `every_linked_run_preserves_eager_result_prefix_and_pop_bounded_checks` | Every permitted linked run has the eager result, work bound and exact retained candidate prefix; this is not only an existential trace claim. |

The prefix is the private **candidate** after successful pops. On a later refusal,
transaction selection must still choose the immediate pre-step graph and preserve
the specified global effects; it must not publish the partially popped candidate.
This model treats local completion/classification as shared pure authorities.
Their Rust frame allocations, byte comparisons, error sources and global-clock
effects remain separate realization obligations. The check count does not count
list-based `nth_error` cost, completion-body work, parsing or physical allocations.
It therefore is not a whole-runtime complexity theorem.

**Concrete realization requirements from source review.** A logical focused Region
has new residuals and a new child while its historical slot is immutable. Ordinary
`Slots.realizes` at that same slot cannot describe both graphs; its root-uniqueness
theorem rules this out. Use an overlay realization built from exact physical leaf
and continuation frames, a held historical Region snapshot and checked logical
child/residual overrides. Bind all six Rust frame variants, not just an opaque
`grammar_fields` list. A held owner retains exact previous span, declared length,
start depth and continuation; an existential birth stamp establishes absolute
endpoints from the stored residuals. Saved enclosing `InnerPath` metadata is not
stored in the Region frame and must be retained separately.

Checked materialization must append fresh Region cells and preserve old lookups.
Prove semantic equivalence under explicit frame-ID relocation (or a separately
defined ID-insensitive equivalence), not literal equality to a reified graph with
old Region slot IDs. Fault coherence additionally binds the selected owner root,
saved graph stamp and located-byte prefix extension. Hidden-terminal inspection
follows Region children regardless of status. After graph-dependent private
resolution, retain materialized form unless a new focused decomposition is proved.
These requirements remain within task 6018; no new grammar or dependency is needed.

The five new laws raise the focused runner to 132 RPB3 reports. Existing reference
runtime controls remain unchanged; the persistent optimized representation and its
new property bindings are not yet implemented.

## Exact matcher count intervals (2026-09-07)

Task 5855's required formal child 8339 adds `ParCountIntervals` to
`SpatialMatcher.v`. Existing matcher theorems remain unchanged. The count model
uses natural-number mathematics and an explicit finite/unbounded domain; the
implementation refinement selects the cap $`2^{128}-1`$ for exact aggregate
counts while retaining `Multiplicity(u64)` at each PathMap endpoint. A finite
value equal to the cap is not top. Values above the cap and top have identical
lower/upper comparisons against every representable finite target; the checked
arithmetic quotient preserves addition, minimum, maximum, scaling and the existing
interval folds. In particular, top as a lower bound refuses finite targets, and
zero times top is finite zero.

For exact available mass $`N`$, selection lower/upper bounds $`L_s,U_s`$ and
remainder bounds $`L_r,U_r`$, let $`c_N(U)=\min(N,U)`$ for finite $`U`$ and
$`c_N(\infty)=N`$. Reject unbounded lower bounds or $`L_r>N`$, then compute:

```math
\ell=\max(L_s,N-c_N(U_r)),\qquad
u=\min(c_N(U_s),N-L_r).
```

The range is empty if $`\ell>u`$. Otherwise membership is equivalent to satisfying
both selection and complement constraints, not just the selected cardinality.
The complete ten-capstone invariant catalogue below was recorded before Rust changes
in child 8340. Child 8341 owns execution and independent qualification. The binding
checkpoint following the table distinguishes executed checks from remaining work.

| Capstone | Implementation invariant | Required executable binding |
|---|---|---|
| `exact_family_total_is_endpoint_mass` | Family totals are exact endpoint sums and additive over disjoint list segments. | Exact cached-total accessor versus independent endpoint sums, all eight partitioned families, single-maximum and two-endpoint maximum-plus-one carriers. |
| `finite_cap_quotient_preserves_target_constraints` | Overflow normalization preserves comparisons to finite targets; zero scaling produces zero. | Finite `u128::MAX` distinct from top, checked overflow in both lower/upper positions, and zero-times-top controls. |
| `bounded_interval_fold_preserves_existing_equations` | Bounded min/max/add/scale and parallel/AND/OR folds refine the mathematical equations. | Independent small arithmetic/fold oracle and overflow-boundary cases through production `ParCount`. |
| `concrete_expression_minimum_is_exact_nonfree_mass` | Free-expression mass cannot erase a concrete sibling's minimum. | Maximum free/wildcard multiplicity plus one concrete Expr retains minimum one; no projected-total subtraction. |
| `exact_feasible_range_preserves_both_constraints` | Every selected count satisfies both its own and its complementary interval. | Complete small count-vector/interval oracle, including contradictory ranges and exact-one complement above `u64::MAX`. |
| `impossible_lower_bounds_refuse_finite_targets` | Top lower bounds, excessive remainder minima and overflowed minima cannot invent a candidate. | Direct impossible-lower witnesses and a control that incorrectly clamps top lower to availability. |
| `zero_complement_requires_the_entire_exact_target` | A zero complement requires selecting the exact complete target. | Top-derived selection at 1,001 for every family; maximum-plus-one whole-target split; old-1,000 and projected-availability controls. |
| `endpointwise_partitions_conserve_exact_mass` | Valid counted selections preserve each endpoint and the exact sum. | Per-key selected-plus-complement equality, nonoverlap of occurrence ownership and retained CoW-root observations. |
| `feasible_filter_preserves_reference_order_and_uniqueness` | Filtering a supplied duplicate-free ordered reference retains order, uniqueness and exactly the feasible members. | Compare complete small candidate sequences against independent lawful positional enumeration with first-occurrence deduplication, never a sorted-set comparison. |
| `partition_scratch_size_depends_only_on_distinct_endpoints` | The abstract counter vector has one slot per endpoint independently of multiplicity. | Narrow prefix-consumption checks at maximum counts, distinct-endpoint-controlled allocation observations and existing tiny-stack/rollback gates. |

Five additional proved examples discriminate the discarded finite-top sentinel,
projected-availability complement error, projected-free-subtraction error,
finite-maximum/top conflation, and lower-top clamping. The proof runner requires
exactly ten closed-context reports and a whole-module kernel check, and rejects
proof placeholders within the new module. The existing historical prose is not
treated as a proof directive.

**Refinement boundary.** The model does not prove the Rust cached metrics, byte
codec, PathMap internals, physical allocations, or the existing `FamilyPartitions`
skip/advance cursor. Its filtering theorem assumes an ordered reference; its scratch
theorem counts abstract counter slots, not retained key bytes. Production parity
against the recursive matcher cannot close these gaps because that oracle imports
the same `ParCount` and `sub_pars`. The independent properties above bind the
implementation to the model. Formal-check status and immutable evidence are recorded
in the task and stack-safety report §5.29.9; no current full-workspace pass follows.

### Executable binding checkpoint

`scripts/check-stack-safe-pda-proofs.sh --matcher-counts` executes the prerequisite
model, ten interval and 21 byte-native closed-report guards and both whole-module
kernel checks. It requires exactly 13, 3, 16, 8 and 57 nonignored tests in the
count-interval, count-fold, connective-isolation, conditional-family and
PDA-equivalence suites, then 11 byte-native count tests, three generated Expr
classifier tests and four models-local physical byte-read tests, followed by the
explicitly selected original depth-4,096/128-KiB probe (116 total).
The focused mode and full runner share the same runtime functions. The historical
September 7–8 run passed the original 92 selected Rust tests; that result does not
establish the subsequently extended gate or include unrelated workspace tests.

The subsequent byte-native implementation run passes the extended 110-test gate,
31 closed reports and both kernel checks. Its log SHA-256 is
`f0b61715a64f1c0fc3238ce9007a7cf00f9f440a9fac330594a9a3541f93a936`;
the stack-safety report §5.29.9 records the command, bounds and remaining resource
qualification. Private test support resides in the respective `tests/support`
directories; production builds contain no measurement counters.

Resource qualification subsequently adds five allocation/fixture/stack tests to
the count-interval suite and registers the original probe without reducing its
depth. The full 116-test gate passes with the same 31 closed reports and both kernel
checks; log SHA-256 is
`2287e52514951bff979ff20aad156acc3c7c96afcf0d5515f2b9093e5f11657b`.
The actual repeated-suffix decoder control violates both allocation-growth bounds
while the native route preserves the same interval and passes. This is the executable
resource binding, not a proof of the allocator, PathMap internals or whole-workspace CI.

The frozen local reproduction/acceptance criterion for pgmcp bug 5855 is run from
the repository root:

```sh
env ALLOW_UNBOUNDED_TLC=0 RUST_BACKTRACE=1 PROPTEST_RNG_SEED=41315855 \
  bash scripts/check-stack-safe-pda-proofs.sh --matcher-counts
```

Success requires exit zero, 31 closed reports, both kernel checks and the exact
116-test inventory, including `PROBE OK depth=4096 stack=131072`. The three
historical source-mutation controls reject finite-top 1,000, saturated complementary
availability and lower-top clamping for their intended assertions; their restored
source and log digests are preserved in report §5.29.9. This criterion makes the
local repair reproducible. An agent's `claimed_done` submission is not independent
CI verification, and the campaign's current full-workspace release gate remains
required separately.

The final local acceptance repetition against committed `3035fb970` passes this
criterion in full. Log SHA-256:
`3cbbe9e1f3a95fae7b87703fa2a4e2b5f8e4b1046c0405d2f7dd819686aba3a8`.
Task 8341's acceptance boundary links the independent ordered-reference controls,
formal contracts, byte-native implementation and resource qualification; it does
not widen the verification claim to unexecuted workspace tests.

| Invariant group | Executed implementation binding |
|---|---|
| Exact family totals; zero complement; counted conservation | `every_partitioned_par_family_admits_compressed_multiplicity_above_the_legacy_ceiling` now covers all eight families at 1,001, maximum and two-endpoint maximum-plus-one, with independent endpoint sums, per-key conservation and retained CoW roots; both `parcount_saturated_target_respects_*_complement` tests cover the complementary constraints. |
| Finite cap quotient; impossible lower bounds | `finite_maximum_is_distinct_from_top_and_zero_annihilates_top`, `overflowing_or_impossible_lower_bounds_never_invent_a_partition` and `finite_count_arithmetic_matches_unbounded_integer_reference` cover explicit top, finite maximum, overflow and 256 arbitrary-precision arithmetic cases. The actual lower-top-clamping mutation fails the impossible-lower-bound assertion. |
| Interval fold equations | All three `matcher_count_folds` tests compare the actual owned/borrowed PDA entries and all eight fields with an independently evaluated specification tree: direct/empty connective equations, six composed fixtures and 64 bounded generated trees. |
| Concrete expression minimum | `parcount_saturated_free_mass_preserves_concrete_expression_minimum` covers both free-variable and wildcard maximum mass plus one concrete sibling. |
| Exact feasible intervals; ordered filtering; uniqueness | `complete_small_partition_sequences_match_independent_positional_reference` covers 7,461 interval cases and 3,532 ordered candidates; the generated companion adds 256 cases with up to three distinct endpoints. Expected sequences use lawful positional masks and first-occurrence deduplication, not production interval normalization or a sorted-set comparison. |
| Multiplicity-independent scratch | `fixed_endpoint_partition_scratch_does_not_grow_with_multiplicity` observes equal allocation calls/bytes for fixed endpoint count at multiplicities 1, 1,001 and maximum; `partition_allocations_scale_with_distinct_endpoints` checks the 32-to-128-endpoint growth envelope. These measure cumulative allocations, not peak RSS. |

Actual temporary source mutations restoring top 1,000, projected availability and
lower-top clamping were rejected by the expected assertions. The production files
were restored exactly to `5ff659702c` before the registered gate passed again.
The final gate log SHA-256 is
`d55c4dc36e9b99574198eddd2d91fee576ae4117f00113e60a3464081e3cd442`.

The supplementary depth-4,096 count probe did not pass: its 300-second scope
expired. Stage attribution then separated construction, borrowed count and root
drop on the same fixture. At depth 256 those phases took 0.444 s, 38.323 s and
0.0034 s respectively on a 128-KiB stack. This identifies a count-path resource
gap, not merely fixture setup. These are individual diagnostic observations, not
powered benchmarks or a proof of asymptotic complexity. Required tasks 8343–8345
under 8342 specify formal-first byte-region projection/fold refinement, runtime
implementation and resource qualification before 8341 can close. The original
depth-4,096 obligation remains; smaller successful probes do not replace it.
Stack-safety report §5.29.9 records the raw phase ladder and evidence digests.

## Byte-native matcher count refinement (2026-09-08)

Required task 8343 follows the measured count-path gap in §5.29.9 of the
stack-safety report. This section specifies the implementation obligations before
the runtime change in 8344. **The registered formal gate passes; runtime realization
and resource qualification remain required.**
The existing `ParCountIntervals` arithmetic proofs and the 92-test qualification
checkpoint above remain independently applicable; neither proves the new byte
projection or its resource bounds.

The input authority has two forms. A resident `Par` supplies exact cached family
totals and distinct canonical PathMap endpoints. Its Expr endpoints can be classified
from their framing; each Connective endpoint can be copied once into an immutable
owner before the lending zipper advances. An encoded child `Par` instead supplies
repeated protobuf occurrences, not resident multiplicity counters. The existing
`CanonicalParRegionIndex` identifies generated-schema family runs. Its child regions
retain the same byte owner, and only Connective structure needs recursive semantic
inspection for counts. A suspended cursor and interval accumulator replace decoded
child collections. Per-endpoint multiplicity scales the completed interval once.
The assembly relation must connect those two layers: each resident Connective
endpoint supplies exactly one child with its own multiplicity scale, in inventory
order. Merely proving a generic fold and a separate scale law would not rule out
an assembler that forgets a scale or duplicates a child. Encoded repeated occurrences
use unit multiplicity at that boundary.
Projection admission retains the enclosing family tag along with each body: an
empty Send, Receive, Expr and Connective can all have identical zero-length bodies.
A payload-only decoder relation would not represent this valid mixed-family input.
The model therefore needs a concrete mixed-empty-body admission witness as well as
the conditional projection theorem.

Public owned and borrowed `Connective` inputs remain structural entry points. Their
child `Par` values may contain compressed maximum multiplicities, so serializing an
entire public input to select the byte route would defeat the representation. NOT
returns its interval without descending into its child; scalar predicates contribute
one expression irrespective of their Boolean payload. Derived `connective_used`
metadata is not a gate for these count equations.

The current cursors may inspect direct framing twice: once to validate/index a run
and once to advance it. The intended resource theorem therefore bounds a constant
number of header inspections per relevant encoded field, not exactly one. It must
account separately for root endpoint copies, retained owner bytes, continuation
storage and fallback decoding. It does not prove PathMap topology, the complete
protobuf parser, Rust memory safety, allocator behavior or unconditional linearity
for a route whose framing proof fails.

### Pre-implementation invariant and executable-test catalogue

The following identifiers name required bindings, not already-passing new tests.
Each must be linked to the new theory's capstones before production implementation.
Tests must use independent expected values or counterexamples; reusing production
count arithmetic on both sides is insufficient.

| ID | Invariant | Required executable binding and discrimination control |
|---|---|---|
| PCBN-01 | Resident exact family projection equals endpoint mass for all eight fields. | Reuse maximum and maximum-plus-one carriers from `matcher_connective_isolation`; compare exact independent sums with the new route, preserving retained CoW roots. |
| PCBN-02 | Encoded family projection counts repeated occurrences, including equal bytes, exactly and preserves enclosing family context. | Compare every generated Par family with independent repeated-field counts; duplicate fields must count twice rather than disappear through set deduplication. Include empty Par and mixed zero-length Send/Receive/Expr/Connective bodies distinguished only by their source field tags. |
| PCBN-03 | Only a FreeVar or Wildcard expression removes concrete Expr mass and sets the unbounded upper interval. | Cover every signed `i32` FreeVar boundary, Wildcard, BoundVar, absent Expr oneof, absent EVar child and absent Var oneof. Reject a classifier that conflates unproved framing with concrete. |
| PCBN-04 | Every admitted current Expr schema alternative receives a proven byte classification. | Derive the coverage inventory from the generated schema; include nested and omitted-field forms. Assert zero whole-expression decoding and zero fallback for the admitted corpus, not just recognized scalar examples. |
| PCBN-05 | Empty AND, empty OR, NOT, scalar predicates, VarRef and absent Connective preserve structural equations. | Extend the independent `matcher_count_folds` specification for both predicate Boolean payloads and all signed VarRef fields; NOT must not visit its child. |
| PCBN-06 | Suspended cursor traversal consumes each child in order and preserves its accumulator across nested return. | Independent nested AND/OR/parallel fold corpus, multi-child and deep unary cases, with ordered visit traces and work counters. Reject skip, duplicate, reversal, wrong-seed and lost-accumulator controls: final count equality alone cannot detect duplicate or reversed AND/OR visits because those folds are idempotent and commutative. |
| PCBN-07 | Exactly one child is assembled per Connective endpoint, with its own scale; evaluation followed by scaling equals occurrence-wise contribution. | Duplicate-coalesced resident Connectives at 1, 1,001 and maximum; independent endpoint-to-child multiplicity trace, forgotten-scale/duplicate-child controls, zero scaling and finite/top overflow; fixed traversal/copy counters while multiplicity varies. |
| PCBN-08 | Derived metadata does not suppress count traversal. | Identical components with both root and child `connective_used` values and empty/nonempty `locally_free` metadata yield identical count intervals; metadata-gated mutation is rejected. |
| PCBN-09 | Structural public Connective entries do not serialize child Pars to obtain counts. | Owned/borrowed parity with child family totals maximum and maximum-plus-one, exact independent expected intervals and zero serialization/expansion counters. |
| PCBN-10 | Unproved framing delegates to the existing decoding authority; refusal is not an invented semantic interval. | Noncanonical/malformed framing controls compare decoder outcome and failure authority. Keep admission assumptions explicit; valid-schema fallback remains forbidden by PCBN-04. |
| PCBN-11 | Every suspended child range retains its exact immutable byte owner and stays inside its parent range. | Existing owner-bound cursor and `ProtobufRegion` range tests plus nested resume/drop and CoW source-retention cases; mismatched-owner and out-of-range controls must refuse. |
| PCBN-12 | Header work and copied endpoint bytes do not grow with repeatedly decoded suffixes. | Geometric deep/wide deterministic counters, at most one retained payload copy per visited resident Connective endpoint, borrowed Expr classification with zero retained payload copies, zero nested payload copies and zero whole-suffix decodes; an actual repeated-decoding control must violate the gate. Separate header work from zipper path reconstruction. |
| PCBN-13 | Count-machine native stack is independent of semantic depth and its live continuations follow the proved machine bound. | Restore the original depth-4,096/128-KiB probe, retain separate construction/count/drop timing, and qualify wide/empty/mixed controls under systemd memory limits. A smaller passing depth does not close this obligation. |
| PCBN-14 | Integration preserves ordered partition, rollback and matcher behavior after the byte refinement. | Rerun the registered ten-capstone / 92-test gate and new bound tests; preserve independent ordered oracle and actual finite-top/projected-availability/clamping rejection evidence. Full-workspace acceptance remains a separate required gate. |

Resource qualification in 8345 must publish raw observations, command/environment
pins and the distinction between deterministic work bounds, single diagnostic
samples and controlled timing populations. Throughput claims require a suitable
idle-core window. None of these obligations authorizes a PathMap fork, a new codec,
a parser dependency or a change to the Rholang count semantics.

Test-only counters must distinguish cursor operations from actual low-level header
reads. Rholang unit tests do not compile their `models` dependency with `cfg(test)`;
models-local cursor instrumentation therefore qualifies the per-operation header
bound, while the Rholang machine instrumentation records operation counts, copied
bytes, decoder calls/input bytes, live frames and ordered visits. Compose those
observations explicitly rather than relabeling a cursor-call count as a measured
header count. Private support modules under the respective `tests/` directories
can hold thread-local measurement scopes without production counters or a new
runtime dependency. Existing depth histogram hooks do not measure these costs.

### Formal bindings and qualification boundary

`ParCountByteNative.v` imports the existing interval algebra instead of redefining
finite/top arithmetic. Its projection admission passes the enclosing source field
to both the semantic decoder and the byte view. Its cursor machine has explicit
Visit, Scan and Return states, with Resume and Scale continuations; a decreasing
potential establishes finite termination. The same transitions emit an ordered
inspection trace, so a value-only equivalence cannot hide duplicate work.

| Capstone | Catalogue bindings |
|---|---|
| `encoded_projection_preserves_all_eight_fields_and_hole_mass` | PCBN-01, PCBN-02, PCBN-03: exact projection under source-tagged admission. |
| `duplicate_occurrences_and_compressed_endpoints_have_equal_projection` | PCBN-01, PCBN-02, PCBN-07: equal counts for repeated unit occurrences and counted endpoints. |
| `tri_state_refinement_preserves_decoder_results_and_failure_authority` | PCBN-03, PCBN-10: proven classification or exact decoder delegation, including errors. |
| `admitted_expression_forms_have_complete_zero_fallback_classification` | PCBN-03, PCBN-04: complete classification over abstract expression forms; generated-schema coverage is the concrete binding. |
| `structural_connective_equations_ignore_metadata_but_not_children` | PCBN-05, PCBN-08: empty/control/scalar equations and metadata independence. |
| `suspended_cursor_step_preserves_the_structural_fold` | PCBN-06: each transition preserves the denotation of active work and suspended continuations. |
| `suspended_cursor_terminates_with_exact_all_field_results` | PCBN-06, PCBN-13: every finite modeled input terminates with the structural result. |
| `suspended_cursor_execution_preserves_exact_ordered_visits` | PCBN-06, PCBN-12: completed traces contain exactly the structural visits in order. |
| `live_continuations_are_bounded_without_occurrence_expansion` | PCBN-07, PCBN-13: live frames are bounded by structural potential independently of multiplicity. This is not a native-stack or depth-specific allocation theorem. |
| `compressed_endpoint_scaling_refines_occurrence_fold_and_cap` | PCBN-07: one completed result scales like repeated addition through the checked finite/top quotient. |
| `ordered_endpoint_assembly_preserves_exact_multiplicity_and_visits` | PCBN-07: ordered child assembly preserves the admitted Connective multiplicity inventory. The byte-to-child semantic correspondence remains an executable admission obligation. |
| `checked_child_ranges_preserve_retained_owner_and_bounds` | PCBN-11: checked relative ranges preserve owner identity and bounds. |
| `suspending_and_resuming_preserve_every_retained_range` | PCBN-11: both active child and suspended parent remain within retained owners. |
| `inspected_headers_have_a_derived_constant_pass_byte_budget` | PCBN-12: header work is bounded under the explicit admitted header catalogs and pass multiplicities; it does not prove the Rust parser's correspondence to those catalogs. |
| `outer_endpoint_copying_is_exact_and_descents_add_no_payload_copy` | PCBN-12: modeled endpoint-copy and shared-child actions account separately for copied bytes. Concrete copy calls require instrumentation. |
| `public_connective_entry_remains_structural_for_arbitrary_mass` | PCBN-09: completed structural-entry inspection traces contain no whole-Par serialization effect; runtime effect correspondence must be tested. |

The positive mixed-empty-body witness prevents loss of source-family context from
making projection admission vacuous. A separate inventory witness admits a scalar
Connective of multiplicity two and rejects assembly with multiplicity one. The
other controls distinguish incorrect unproved-to-concrete classification, absent
options, metadata gating and zero-times-top handling. The registered proof-only
mode is `scripts/check-stack-safe-pda-proofs.sh --matcher-count-byte-native-proofs`:
it first rebuilds/checks the ten-capstone interval prerequisite, then requires 21
closed reports (16 main capstones and five selected examples) and checks the whole
new module. It shares the same bounded helpers with the full runner.

**FORMAL GATE PASSED — 2026-09-08.** The registered proof-only command completed
with exit zero, ten prerequisite plus 21 new closed reports, and two successful
whole-module kernel checks. Every compiler/checker invocation used systemd
`MemoryHigh=1G`, `MemoryMax=2G`, `MemorySwapMax=0`, `CPUQuota=200%`,
`TasksMax=128` and a 900-second watchdog. Final source SHA-256:
`2d57c973908e3c8f88dcf6e10536a13353c6ad294c2a14817cb2e41f71870c9e`.
Registered log `/tmp/rspace-8343-registered-proof-gate-20260908.log`, SHA-256:
`0990fb97b6545b8fa70ade22acf499ff61d37abb0f1bd572c4a0e154d8adb832`.

Review corrected source-family context, endpoint-to-scale assembly, and a header
budget premise that originally required complete catalog passes in order. The
qualified header model instead allows arbitrary interleaving of consume-next
cursors and derives the work bound from their remaining slot mass. Those corrections
precede all new Rust changes. The proof does not discharge the explicit parser,
child-correspondence, schema-completeness, owner, arithmetic or concrete operation
bindings listed above. **None of the new runtime bindings is yet implemented or
passed; tasks 8344 and 8345 remain required before 8341/5855 can close.**

## Exact scalar projection contract (2026-09-07)

Task 5853 adds the standalone `ScalarProjection.v` admission contract, not a
new evaluator algorithm. The model computes distinct-family purity from a
finite endpoint list and its family filter; it does not infer purity from
saturating logical cardinalities. For any cap greater than one, projected
cardinality one is exact. Positive endpoint multiplicities then establish
that a pure unit-mass carrier contains exactly one matching endpoint with
multiplicity one. The family predicate is generic, so the theorem does not
enumerate and accidentally omit a foreign constructor family.

The eight contract capstones bind the following executable obligations.
The named acceptance extensions are realization targets, not tests proved or
executed by Rocq.

| Capstone | Executable obligation |
| --- | --- |
| `exact_distinct_family_purity` | Existing `models/tests/par_bag.rs::all_components_are_is_exact_across_families_and_saturated_cardinality`; all eight foreign families and masks in `scalar_projection_acceptance_properties`. |
| `saturated_singleton_count_is_exact` | Counts zero, one, two, maximum and arbitrary positive `u64` values in `scalar_projection_acceptance_properties`. |
| `pure_unit_mass_is_exactly_one_selected_endpoint` | Pure singleton acceptance and compressed-repeat refusal in `scalar_projection_acceptance_matrix_preserves_refusal_trace`. |
| `saturated_cardinality_equality_does_not_prove_purity` | Saturated Expr plus a foreign endpoint remains a foreign-family refusal, even when projected total and family counts agree. |
| `foreign_refusal_precedes_arity_without_effects` | The matrix and properties preserve the exact foreign error, initial diagnostic trace and snapshot while a child that would otherwise evaluate is present. |
| `arity_refusal_has_no_decode_or_child_effects` | Pure empty and nonunit carriers preserve the same observations and the request-specific arity error. |
| `singleton_delegation_preserves_the_complete_observation` | The matrix preserves a bound-variable child's success and an unbound-variable child's full error, state and ordered diagnostic trace. |
| `fusion_decline_preserves_fallback_and_accepted_execution` | `scalar_projection_recognizers_decline_without_effects` exercises resident, encoded-chain and encoded-reentry gates; `fused_differentials::scalar_projection_fused_unfused_carriers_preserve_observations` compares the complete fallback observation and nonvacuous positive case. |

Structural refusal records zero decoder/delegate calls, an empty added trace
and unchanged abstract state. Successful admission delegates exactly once and
preserves the delegate's complete result, final state and trace, including a
delegate failure. Recognizer `None` leaves fallback intact; it is distinct
from an error after accepted fused execution, which must not be retried as
fallback. Trace equality is dynamic diagnostic preservation, not a new
consensus-cost golden. The three expression requests retain their supplied
foreign and arity errors; the theorem does not merge those error payloads.

The file also checks a successful fixture and causal controls for swapped
error precedence, empty carriers, repeated endpoints, premature child work,
delegate failure and swallowed fallback errors. It has ten closed assumption
reports and passes independent kernel checking. The proof runner requires
that exact count, rejects placeholders and kernel-checks the module.

The endpoint list must refine the actual distinct resident index; the family
predicate must refine constructor tags, and stored multiplicities must be
positive. Concrete cached metrics, canonical payload decoding, Rust evaluation
and observable-state correspondence remain executable obligations. Decoder
and child audit lists describe the abstract dispatch contract, not a proof
of Rust allocation, codec execution or call counts. The generic
`StackSafePDA.v` fold theorem does not itself establish this admission boundary.

The named Rust extensions are now executed: the complete evaluator module passes
18 tests, ParBag passes 33, and the feature-enabled fusion integration target passes
seven, including 22 literal/bound-variable carrier cases under actual enabled and
force-disabled production evaluation. The first integration fixture omitted a required
Bundle body; the retained failure preceded scalar evaluation, and the replacement
uses complete inert components without changing expected errors. Scalar observations
prove child-effect refusal for the selected witnesses, not zero concrete decoder calls.

The proof runner separately binds four `scalar_projection` library tests and the one
named feature-enabled integration test, with exact nonignored passing counts. Both
selections pass at seed `41315853`. Its real zero-selection and incorrect-count
controls fail for the intended guard reason. The runner uses a 6 GiB hard memory cap,
zero swap and four jobs for these Rust checks, distinct from the 2 GiB proof scope.
Artifact 506 and stack-safety report §5.29.8 preserve commands, failure attribution,
results and exclusions. This is local acceptance, not full-workspace or independent
CI verification.

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

RA09's causal-error recognition is refined before the test repair: the existing
`failed_runtime_refuses` theorem excludes `RuntimeRefused` from acceptance regardless
of diagnostic wrapping. The observed current runtime wraps RSpace's capitalized
unused-COMM diagnostic in `ReplayFailure::InternalError`; the legacy wrapper checks
lowercase `unused`. For this successful-evaluation mutation, both represent refusal,
not successful replay. This does not establish the separate failed-evaluation
suppression behavior, which is unchanged.

The test-only recognizer must retain strict cost/status checks and the existing
COMM-cost/unused-event cases. Its additional internal-error case must match the
complete anchored wrapper and diagnostic, a canonical positive `usize` count, and
the exact suffix. Required controls accept the observed count and generated positive
counts; reject zero, signed/whitespace/leading-zero/overflowed counts, wrong wrappers,
extra prefixes/suffixes, and unrelated internal errors. The mutation must explicitly
differ from the archived identity. The successful untouched replay and `expect_err`
remain mandatory. These executable controls qualify the Rust diagnostic recognizer;
the abstract theorem does not prove a string parser or concrete runtime.

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
