# Stack-safe PDA, EPathMap, and RSpace Bag proof kernel

This development proves the algorithmic obligations shared by the generated Rust machines.

For the requested-channel RSpace candidate catalog, `RhoPathV1Datum.v` supplies
the executable byte framing, `TwoLevelRSpaceMatchPreparation.v` supplies semantic
construction and collision-safe ordering, and `EncodedRSpaceCandidateCatalog.v`
separates decoder-free construction from later semantic interpretation. The
whole-catalog equivalence requires canonical admission: decoded identity bytes
must re-encode exactly and agree with the stored structure. It does not assume
cryptographic collision-freedom or verify the supplied Blake2b/bincode functions.
The native bindings observe the actual digest preimage, check a historical
known-answer datum, compare production-Par decoding with its derived oracle,
and exercise collisions, malformed framing, lazy decoding, empty catalogs and
unrelated-channel isolation. `scripts/check-stack-safe-pda-proofs.sh
--match-preparation` is the maintained exact-count gate. The detailed invariant
mapping and qualification limits are in [journal entry J-165](../../../docs/design/pathmap/pathmap-engineering-journal-2026-08-23.md#j-165--2026-09-17-encoded-whole-catalog-refinement-and-concrete-codec-bindings).

- `compile_run_equivalence` proves that an explicit post-order instruction/value-stack machine equals the mutually recursive fold for every finite typed tree and every algebra. Clone, equality, hashing, ordering, debug formatting, bincode/protobuf encoding, encoded length, clear, and teardown are instances of that theorem.
- `decoder_machine_equivalent_to_recursive_rebuild` instantiates the same machine with the tree-building algebra and proves exact reconstruction.
- `compiled_program_has_one_instruction_per_node` proves linear work and one completion instruction per node.
- `EPathMap.v` proves neutral-empty mode laws, set lattice laws, exact map-value conflict behavior, and the prefix-restriction subset law.
- `Bag.v` models an exact-key `PathMap<Multiplicity>` Bag by its total count function with zero as absence; the concrete implementation retains only its finite support. It proves commutative-monoid sum, distributive max/min lattice laws, truncated-subtraction conservation, exact-key independence from prefix relationships, canonical-delta construction, and normalized atomic apply/revert. Its cursor-walk refinement proves pointwise max, min, and truncated subtraction for a duplicate-free visit of every occupied key, exact subbag checking for a visit of every left-hand occupied key, preservation of every other key at each cursor step, and preservation of any common endpoint-count bound. The proof assumes the traversal supplies those complete finite-support visits; the focused Rust property and tiny-stack tests check the current PathMap behavior at prefix-related keys, empty endpoints, maximum multiplicities, copy-on-write snapshots, and borrowed-cursor enumeration against the public iterator. Its candidate-projection bridge proves that only stored nonpersistent candidates contribute removals and that selection permutations construct the same normalized delta.
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

## PathMap leaf extension and virtual navigation (2026-09-12)

`PathMapLeafExtension.v` specifies the local repair boundary for PMU-006,
tracked in tasks 6023/6038. Its state separates prefix-closed logical topology
from optional payloads: dangling paths remain observable even without values.
The reference operation unions the old topology with the target's prefixes.
An independent implementation model reads to the first absent edge, then
permits subtree replacement only when the selected existing boundary is a
terminal leaf. It parks that boundary's value, grafts a value-free suffix,
and restores the exact parked value. Insertion subsequently replaces the
target's value. No endpoint sorting or decoded entry projection is modeled.

The original 26 named proof reports cover observational equivalence, existing-path
and repeated-create identities, read selection, preservation of values and
finite-domain counts, parked ownership, and topology-neutral virtual cursor
navigation. Virtual paths comprise an immutable authority prefix and a
relative focus; saturating ascent cannot escape the prefix. Concrete negative
controls show that an unprotected graft loses the root value, grafting below
a nonterminal boundary destroys siblings, and interruption after parking is
not rollback. A separate control distinguishes a missing child below a branch
from a terminal-leaf extension.

The native invariant catalog below is extracted before the Rust repair. Its
exhaustiveness theorem enumerates this model's 21 declared obligations; it is
not a proof that every possible Rust defect is covered. **All native bindings
remain unqualified at this formal-first checkpoint.**

| Model obligation | Required native property or source correspondence |
|---|---|
| `ReadSelectionExact` | Compare the read-selected existing boundary and absent suffix with an independent prefix-set oracle after each original operation. |
| `TerminalChildMask` | Graft only at an existing focus with no children; reject the branching-boundary negative control. |
| `MutableFocusBeforeMissingEdge` | Observe that mutable selection stops before the first absent edge, including before a prefix-rooted callback starts. |
| `CompressedEndpointParentFocus` | Cover endpoints inside compressed paths and parent-held values under sparse and dense branches. |
| `FreshDonorRootAbsent` | The donor root has no value; no synthetic destination value is used to permit mutation. |
| `DonorSuffixExact` | Donor topology is exactly the missing suffix's prefix closure, including deep suffixes. |
| `ParkNoPrune` | Detachment preserves the boundary path and uses non-pruning removal. |
| `ParkCarrierCount` | The guarded endpoint certificate excludes a parked value at every resumable interruption boundary. |
| `BorrowedGraftRetainsDonor` | The donor stays under its existing stack-safe owner throughout grafting and retirement. |
| `RestoreReplacementAbsent` | Restoration returns no unexpected replacement and restores the same payload ownership, not a clone. |
| `SuccessfulEndpointCount` | Create preserves counts; insertion adds exactly one new endpoint or returns exactly one replaced payload. Check before/after each operation. |
| `SiblingValuesAndTopology` | Compare all values and logical edges, including unrelated siblings, against independent oracles. |
| `CowSnapshotPreservation` | Retained snapshots preserve both values and dangling topology through mutation and all relevant drop orders. |
| `ScopedDisposerNonpanicking` | Named production disposal policies are stack-safe and non-panicking; synthetic teardown barriers never enter the real-value sink. |
| `DonorFinalRootTeardown` | Deep donor destruction uses the existing bounded-stack teardown path, including interrupted publication. |
| `CalleeTransferUnwindBoundary` | Audit native remove/set ownership-transfer intervals; caller guards cannot dispose a value already transferred into a failing callee. |
| `EmptyRootNormalizationAfterCursor` | Retired empty roots normalize only after closing the mutable cursor; reopening remains safe. |
| `NoOpPreservesTopology` | Existing creates, missing removals and empty requests preserve their defined topology and return-value semantics. |
| `VirtualNavigationTopologyNeutral` | Missing-path moves, cursor-only closure exit and interruption do not create topology. |
| `VirtualAuthorityPrefix` | Relative reads, moves, child/sibling traversal, ascent and pruning never address outside the logical root. |
| `BranchInsertionPreservesChildren` | A missing edge beneath a nonterminal uses a separately qualified insertion route and preserves existing children. |

### Descendant-supported value removal

The `DescendantRemoval` refinement adds 13 closed reports, bringing that checkpoint to 39.
It models a finite physical-parent abstraction containing value slots and
child slots with relative endpoints. Its independent reference removes only
the focused value. A retained descendant can be a child subtree or a deeper
value inside the same compressed node; either witnesses that removing the
focused value leaves the physical parent nonempty.

The native policy uses `remove_val(true)` only at a non-root value with
descendants. The retained payload makes PathMap's pruning entry return at its
first emptiness guard: at most one guard, zero ancestor-loop iterations,
zero pops and zero detachments. A terminal value uses `remove_val(false)` to
retain its dangling path. Root removal and absent-value requests bypass
pruning. Thus the required contract is **zero logical pruning**, not a fixed
boolean argument to the upstream method.

The model proves exact topology, other-value and endpoint-count preservation
under explicit residual-value uniqueness. Negative controls show that always
using `false` can duplicate child keys in the recorded slot layout, while
unconditionally using `true` can lose a terminal path.

| Additional native binding | Required evidence |
|---|---|
| Descendant support | Bind `child_count() > 0` to a retained parent payload, including deeper compressed values and dangling descendants. |
| Slot removal | Establish that released value-slot removal preserves the supporting payload and physical-parent nonemptiness. |
| Bounded pruning | Inspect the upstream early-return guard and test unchanged focus, topology and snapshots; the model alone does not instrument private native loop counts. |
| Removal ownership | Exact returned payload, one count decrement, named disposal and repeated remove/restore controls. |
| Terminal layout | Check value-plus-same-key-empty-child layouts separately; logical topology equivalence alone does not prove private slot uniqueness. |

### Existing-focus dispatch and graft preparation

The `BoundaryDispatch` refinement adds fifteen reports, bringing that checkpoint to 54. Branch
removal admits a mutable focus only after a successful topology lookup;
value removal requires a present value. A missing value is an exact no-op
even when its dangling path exists. Subtrie removal applies the second gate
to the branch-removal post-state, with an explicit well-formedness premise.
A negative control demonstrates why retaining the original presence witness
would incorrectly reopen a pruned path.

Child materialization dispatches among an existing child, a missing child
below a terminal, and a missing child below a branch. The terminal case
reuses the proved leaf extension; the branch case retains an explicit native
insertion-refinement premise. The resulting child exists, original values
and their finite-domain count are unchanged, and a newly created child is
terminal and valueless. Rooted graft publication absorbs the preparation's
prefix closure, provided the source really publishes a root. This native
publication premise is separate from the model's conceptual navigation root;
it does not authorize grafting an absent source as though it were present.

| Extracted dispatch invariant | Native qualification |
|---|---|
| Absent value/prefix opens no mutable focus | Original-order dense dangling fixtures and generated absent probes; unchanged values, topology, mode, count, cached bytes and retained clones. |
| Present gate invokes the existing operation | Keep native pruning and return semantics; exercise valued parents, descendants and unrelated dangling siblings. |
| Second gate uses post-branch authority | Remove a valueless parent subtree whose branch removal prunes the focus before the value-removal stage. |
| Child preparation preserves values/count and establishes a vacant terminal | Join both operand orders against independent prefix-union and value oracles, including missing terminal and nonterminal children. |
| Rooted graft absorbs preparation | Compare composed set/map grafts at missing, populated and neutral-empty destinations; retain source and destination aliases. |

The `UnitJoin` refinement adds eight reports, for 62 total. Equality-checked
unit-value merging cannot conflict and agrees with the existing value-join
model. Membership is union and the exact count is left plus right minus
shared value-bearing focuses. Common dangling edges contribute topology but
no overlap count. The model checks commutativity, idempotence and associativity
pointwise, and distinguishes an existing logical edge from its optional
cofree payload. A concrete counterexample shows why treating an absent
cofree payload as an absent edge loses shared dangling topology.

The native set specialization now reuses the same synchronized zipper and
borrowed-graft union as map mode. Deterministic set/map cases are separate so
one mode's panic cannot prevent the other from running. Original-order
generated histories compare exact prefix union, associations, counts and
retained snapshots. The complete synchronized traversal, private CoW and
machine-width count arithmetic are not automatically proved by these local
specialization laws.

The existing pointwise idempotence laws also justify returning an unchanged
clone-family root for set union, as already done for map union. The native
binding in `models/tests/epathmap_algebra.rs` checks snapshot-cell identity
before serialization, both cold and warm, plus generated topology histories
and successful mutation invalidation. This observation distinguishes the
constant-time shortcut from a semantically equal reconstructed result. It
does not establish panic-atomic invalidation or private allocator behavior.

These laws qualify the added control flow, not the private implementation of
native pruning, CoW, branch insertion or graft publication. Those remain
explicit source-correspondence and native-test obligations.

The model uses pointwise observations rather than functional extensionality.
The segmentation law concerns logical path concatenation, not PathMap's private
physical layout. The prepublication focus trace specifies the allowed boundary;
the native implementation must still demonstrate that its actual operations
follow that trace. Count laws quantify over a finite observed domain; the
guarded runtime must maintain its exact resident endpoint certificate.

No theorem establishes allocator correctness, private PathMap CoW behavior,
arbitrary exception safety inside upstream methods, automatic Rust extraction,
or a throughput improvement. Parking cleanup and rollback are deliberately
different claims. The maintained `--pathmap-leaf-extension-proofs` route rebuilds
the model prerequisites and checks all 62 reports and kernel acceptance;
`--epathmap-import` and the default gate include the same contract. Passing the
proof-only route does not qualify native acceptance. The combined gate at
`b6e236e81` passes as recorded in engineering journal J-151; the later
caller-dispatch changes require their own native qualification.

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

### Opaque canonical-segment import

`ParConstruction::import_component` keeps an imported component's canonical
score and protobuf body opaque. A stable imported component nevertheless needs
its borrowed trie **segment** before `emit_canonical_key_into` can publish a
key. The previous state permitted `canonical_path_stable = true` with no
segment, which made key emission panic. The fixed import records the stable
segment from the authoritative resident codec. A stable list also records its
different top-level **path**: ordered child segments followed by the terminator,
while its single segment starts with the list tag and child count. Map-mode
EPathMap uses its stored trie snapshot; set-mode EPathMap uses its trie keys in
ascending byte order, without a second resident decode.

[`OpaqueCanonicalSegmentImport.v`](theories/OpaqueCanonicalSegmentImport.v)
models these two key forms, retained opaque score/body bytes, stable and
nonstable metadata rewrap, and a causal mutant that drops a stable segment.
Its ten `Print Assumptions` reports are closed, and `coqchk` validates the
module (source SHA-256
`874ddffddfa0910a142659d4e4745a66f6efda90e98768ca77b1d1cd3484b0ea`;
current maintained-gate receipt
`target/verification/parbag-opaque-8518/current-epathmap-import-gate-r4.log`,
SHA-256 `a9540ec3e012c6e3c1a63eb9d11ac1dec7a742f71b32f33d22f9069556510a3c`).
The model takes the ordered distinct set keys and encoded scalar payloads from
the established trie/codec contracts; it does not re-prove those contracts.
The source-level correspondence is exercised by four fixed regressions and the
three generated properties in
[`opaque_canonical_segment_import_properties.rs`](../../../models/tests/support/opaque_canonical_segment_import_properties.rs):
scalar/private score and wire preservation, list/tuple split-path preservation,
and EPathMap set/map key preservation after copy-on-write. All three generated
properties passed in both debug and release under the current maintained gate,
including the nine stable scalar variants (property source SHA-256
`cf8550421b9c238b581245be9d05bd36fa325fbc8b8603951f419f02acf409ba`;
3/3 in each profile in the `r4` gate receipt above).
The four current-source fixed regressions also passed 1/1 each in serial debug
runs: stable component import (`current-fixed-stable-r2.log`, SHA-256
`40b4b2a696ba19d7249057a9f8c9faf8a97f315c6350d26c826d437249b4c879`),
nested EPathMap copy-on-write on a 512 KiB stack
(`current-fixed-nested-small-stack-r2.log`, SHA-256
`2c1c2a0ebde4565a674edb18a8608d3af936d362e204ec1ed9369575dc1cfad9`),
list segment versus split path (`current-fixed-list-split-r2.log`, SHA-256
`e52c9a99594aa82c85a480eee8d299477974d679ff0560ccc49ce469c388bfcc`),
and resident identity rewrap (`current-fixed-resident-r2.log`, SHA-256
`68286961dd0b3aa3091fe2a220ed6fd32d2cbae5061ed1467a9a8a18d1bd870e`).
All four logs are under `target/verification/parbag-opaque-8518/`.
The production `substitute_drive::plan_expr` NoDescent EPathMap import path is
also exercised by an open-sibling substitution, so the identity fast path
cannot bypass the import; that current-source release test passed 1/1
(`target/verification/parbag-opaque-8518/current-real-caller-release-r2.log`,
log SHA-256 `1ca2b6bb044a8b4da405c9e727eea217937c171520dbc6b2e349c55f2cf87c6a`,
caller source SHA-256 `2c9a08b567601c166355fa8af0f2725dfffccd1c6f98a0572cb331e710950ea8`).
The maintained `--epathmap-import` gate passed with the formal and generated
property checks above; other campaign acceptance remains separate.

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

#### Typed persistent frames and fresh-cell materialization (2026-09-09)

`RPB3Admission.PersistentFrameRealization` connects immutable frame storage to a
logical focused overlay. An overlay keeps historical Region cells unchanged and
computes their current residual lengths from absolute endpoints and a graph stamp.
When a physical graph is required, materialization appends fresh Region cells from
the innermost owner outward. It does not overwrite the historical slots.

The model retains all fields of the six Rust `Frame` variants, splitting Region's
length and active-path phases. `raw_path` retains both enclosing path metadata and
the stack handle; the active Region separately retains its child's metadata and
stack. `Span` is an abstract handle type, not a decoded byte vector.

| Rust storage | Typed model fields retained |
|---|---|
| Tag | Escape permission and next handle. |
| Varint | Role, UTF-8 requirement or obligation kind/source as applicable; accumulator, shift, byte count; next handle. |
| Bytes | Remaining count, optional UTF-8 remaining/min/max state, next handle. |
| Kids | Remaining count and next handle. |
| Region / Length | Remaining count, optional previous span handle, varint state, next handle. |
| Region / Path | Remaining and declared lengths, path residual, start depth, previous span handle, child path metadata/stack, next handle. |
| Obligation | ID, kind, payload length, tag source, payload/awaiting phase including residual/end source, payload start, next handle. |
| InnerPath | Escape permission, status/error, completed segments, first tag/source, stability, stack handle. |

The following catalogue extracts all twelve exported laws. The executable checks
are required bindings for the persistent implementation, not claims that new Rust
property tests already exist. Existing `canonical_frame_storage.rs` and
`canonical_path_opaque_interior.rs` support modules are the intended reuse points.

| ID / exported theorem | Invariant and required executable observation |
|---|---|
| PFR-01 `typed_physical_frames_survive_arbitrary_append` | Every old typed tree remains represented after arbitrary suffix append. Revisit every retained branch, not only the current endpoint. |
| PFR-02 `typed_path_metadata_and_all_payloads_survive_append` | Old raw path handles, metadata and every payload field remain exact. Compare normalized full frames, not validity alone. |
| PFR-03 `typed_overlay_derives_exact_logical_graph` | Inductively valid overlays denote the inside-out owner composition. Independently project the owner chain and compare the full graph. |
| PFR-04 `every_typed_overlay_checkpoint_survives_frame_append` | Arbitrary frame extension preserves old focused checkpoints at their own stamps. Fork and revisit siblings in different orders. |
| PFR-05 `materializing_one_owner_uses_a_fresh_slot_and_preserves_semantics_and_history` | One owner appends one fresh Region, preserves old lookups and exact ID-insensitive semantics. Inspect the new slot and all prior cells. |
| PFR-06 `full_spine_materialization_preserves_typed_semantics_and_appends_one_cell_per_owner` | Full forcing appends exactly one cell per supplied owner and yields a physical graph with identical typed semantics. Count actual cells separately from retained backing capacity. |
| PFR-07 `forced_spine_preserves_every_old_physical_and_focused_checkpoint` | Forcing cannot invalidate any historical physical path, overlay or lookup. Retain shared historical roots during tests. |
| PFR-08 `capturing_a_physical_region_establishes_its_exact_held_owner_and_child` | Capture derives absolute endpoints from current residuals and preserves the actual stored child and continuation. Inspect capture before consuming another byte. |
| PFR-09 `advancing_a_valid_graph_stamp_preserves_held_cells_and_decreases_residuals` | Advancing within both endpoints leaves held cells valid and cannot increase either residual. Exercise both boundaries and equal endpoints. |
| PFR-10 `typed_overlay_private_terminal_is_the_selected_focus_terminal` | Private terminal inspection follows active Region children regardless of path status. Include hidden Payload and Awaiting obligations under Dead paths. |
| PFR-11 `typed_materialization_preserves_exact_private_resolve_and_reject_preflight` | Forcing preserves distinct resolve/reject phase, ID and error-source decisions. Compare exact errors and failed-call nonmutation. |
| PFR-12 `capture_from_a_physical_path_copies_the_actual_enclosing_metadata_and_stack` | Capture copies metadata and stack from the actual enclosing raw path, not caller-invented metadata. Compare every enclosing field independently of child fields. |

**Proof boundary.** `same_path_semantics` erases frame IDs but retains payloads,
span handles, metadata and tree structure. It does not preserve sharing identity;
historical lookup preservation is a separate theorem. Persistent sharing across
checkpoints remains permitted. The model uses mathematical naturals, not Rust
integer widths or allocation limits. Span identity preservation does not establish
span validity, byte observation or backing lifetime. The one-cell-per-owner result
counts one forcing operation, not total parser allocation or runtime complexity.

Before replacing the persistent runtime representation, bind typed owners to the
existing linked-owner scheduler, span observations to the located-byte model, and
fault checkpoints to the selected graph stamp, current byte extension and root
error. Checked arithmetic/capacity and effectful private resolution still require
composition. Private-terminal equality alone does not establish constant-time
inspection without an already available terminal focus. No new parser, production
representation or consensus format is introduced by these twelve laws.

#### Checked typed-owner scheduling projection (2026-09-09)

`PersistentOwnerProjection` connects PFR's typed overlay to the existing linked
owner model without changing either scheduler. `observe_span` is a partial
observation from a retained span handle to located bytes; invalid observation
returns `None`, distinct from a valid absent previous span. `summarize_frame`
supplies scheduling metadata for the older Slots model. It is intentionally not
assumed injective: typed payload preservation remains PFR's separate contract.

`owners_match` relates an inner-first typed owner list to linked entries by
pointwise equality with checked projected owners. It preserves frame slots in
the owner payload; those are not the owner-arena addresses in the entries.
All active Region children are recursively projected with their actual metadata,
and continuation structure retains every frame address. The projection is a
proof/test observation, not a production list or byte-vector allocation.

| Exported theorem | Extracted implementation/property obligation |
|---|---|
| `invalid_previous_span_refuses_owner_projection` | Invalid retained span observation refuses projection; never substitute an empty span or absent previous key. |
| `projected_owner_preserves_exact_scheduler_fields` | Both absolute endpoints, frame slot, enclosing metadata, declared length, start depth, projected continuation and previous located bytes agree exactly. Inspect these independently of arena addresses. |
| `typed_reification_commutes_with_exact_linked_owner_projection` | Projecting the typed inside-out composition equals reference reification over the corresponding owner entries, including their order. Compare independently projected historical and divergent sibling graphs. |
| `projected_paths_preserve_frame_addresses` | Every successful path projection preserves the full ordered frame-address traversal, including repeated addresses from shared subtrees; it is not ID erasure. |
| `typed_overlay_is_the_selected_linked_graph` | A realized typed overlay with a linked matching chain projects to that chain's exact selected graph at its saved stamp. Require the actual focused leaf correspondence. |

These are extracted bindings for the forthcoming persistent runtime, not five
new executed Rust tests. Successful projection checks previous spans on the
active owner/Region path; it does not validate every span inside opaque leaf or
continuation payloads. Exact typed history and backing-lifetime invariants remain
separate. Callback/classification correspondence, selected-fault composition,
Rust arithmetic and resource bounds are not implied by a scheduling projection.

#### Selected persistent fault coherence (2026-09-09)

`PersistentFaultRealization` binds a typed saved focus/owner chain to the linked
checkpoint, graph stamp, located-byte prefix and root error. Coherence requires
physical focus and held-owner realization, exact checked owner/leaf projection,
the stamp equal to saved-prefix length, and current bytes extending that prefix.
This is a selected live-graph checkpoint retained behind a fault, not permission
to reset its stamp to the length of a later dead-byte suffix.

| Exported theorem | Extracted implementation/property obligation |
|---|---|
| `root_error_replacement_commutes_with_checked_projection` | Replacing only root status preserves the projected graph except its exact root error. Do not overwrite child metadata or an obligation's source. |
| `coherent_typed_fault_projects_to_exact_selected_legacy_fault` | The coherent typed fault corresponds to the selected linked graph, not a partially popped candidate or the latest emitted obligation. Check exact graph and source metadata. |
| `appending_dead_bytes_preserves_selected_fault_coherence` | Every dead-byte extension retains saved focus, owner chain, stamp, error and prefix while extending current bytes with exact provenance. Fork divergent suffixes from the same historical handle. |
| `dead_byte_keeps_saved_stamp_and_graph_but_extends_current_provenance` | Dead edges do not advance graph residuals. Compare all saved frame payloads and metadata independently of current byte length. |
| `forcing_a_coherent_fault_preserves_exact_typed_observation_and_all_historical_cells` | Force at the saved stamp, append one cell per owner, retain the exact typed graph modulo frame IDs and current bytes/root error, and leave all old cells unchanged. |
| `forcing_a_coherent_fault_preserves_hidden_terminal_preflight` | Private resolve/reject must agree with the status-ignoring selected terminal, including wrong ID, Payload versus Awaiting and distinct fallback source rules. Failed calls must not publish or force frames. |

The existing `persistent_fault_private_preflight_uses_selected_graph_not_latest_event`
test is the executable reference binding for selected-graph, source, nonmutation
and dead-suffix behavior. It is not yet a test of the optimized persistent forcing
implementation. The forcing theorem assumes coherent selection; it does not prove
the runtime selects that checkpoint on every refusal. Span observation is fixed
through this model; byte-arena extension must separately preserve old observations.
Error projection does not erase physical frame IDs. Forced graphs instead use the
explicit typed ID-insensitive relation established in PFR, with old lookups proved
separately. Shared effectful resolution, checked Rust widths, fault construction
and aggregate resource bounds remain required before persistent runtime cutover.

#### State-threaded linked completion (2026-09-09)

`EffectfulLinkedSettlement` extends the existing dispatch refinement to a
completion callback that takes and returns a logical `World`. Classification
uses the world returned by the preceding callback. The finite positional
reference reuses `PersistentFocusedOwners.positional_command`; the linked
transition relation reuses `linked_command`. This adds no grammar/parser and
does not substitute a second runtime scheduler.

The world denotes effects which both implementations must preserve. Physical
allocation layout and instrumentation counters that intentionally differ are
not implicitly equated. Their refinement remains separate. Classification is
a function of the supplied world and leaf; any semantic classification effect
would need explicit state threading before using this theorem for that effect.

| Exported theorem | Extracted implementation/property obligation |
|---|---|
| `effectful_linked_traversal_terminates_with_exact_world_trace_and_retained_prefix` | Every valid finite linked chain has a terminating run matching the state-threaded positional result, exact callback order, pop/check counts and remaining candidate chain. Compare the logical effect state after every callback, not only the final validity. |
| `effectful_linked_runs_are_deterministic` | The same input world/root/view has one final world, root, observation and check count under the shared deterministic callbacks. Compare repeated runs from independent snapshots. |
| `every_effectful_run_refines_reference_world_and_callback_order` | All permitted linked runs, not just a constructed witness, match the reference world and ordered Region frame-ID call trace. Frame IDs are distinct from owner-arena addresses. |
| `effectful_owner_checks_remain_pop_bounded` | Abstract dispatch checks are at most successful pops plus one even when callbacks change the world. Count callback work and physical storage operations separately. |
| `outer_refusal_retains_inner_and_refusing_callback_effects_control` | A valid two-owner chain increments the logical effect counter twice, calls frames `[2, 1]`, pops only the inner owner and returns the outer refusal. Neither the successful inner effect nor the refusing callback's effect is rewound. |

The reference is the state-threaded positional dispatcher, not a newly proved
state-threaded version of the original eager grammar walk. Its control selection
reuses the existing dispatch correspondence; actual callback/classification
realization and pre-step fault publication must still be composed. The retained
candidate prefix is not authority to publish it after refusal. These five rows
are required implementation/property bindings, not five new Rust tests. The
concrete control is kernel checked. No runtime cost bound for callbacks, model
list lookup or total persistent allocations follows from the dispatch-check bound.

#### Immutable Rust owner-storage binding (2026-09-09)

`PersistentRegionOwners` implements the verified owner-storage operations in
`canonical_path_admission.rs`. Capture borrows an actual immutable active Region,
copies its enclosing `InnerPath`, retains its frame handle, and returns the actual
child. Each new owner stores a backward parent, absolute endpoints, both cached
boundary handles and the outermost handle. `PersistentRegionOwnerId` distinguishes
owner-arena identities from frame identities; the existing `RegionBoundary` is
generic over handle type and remains unchanged for streaming positions.

Materialization follows parent handles inside-out and appends fresh Region cells
to `FrameHistory`. Static Region fields and previous span handles are borrowed
from the retained cell, not reconstructed from bytes. Residual subtraction and
conversion to `u64` are checked against the saved-stamp invariants; the absolute
sum uses `u128` for a native `usize` stamp plus a `u64` residual. Capture never
clears the old child's stack. At the component-only checkpoint these operations
were not connected to the persistent traversal; the following cutover section
records that later binding separately.

| Executed binding target | Formal obligations exercised |
|---|---|
| `persistent_owner_capture_and_forcing_preserve_exact_shared_histories` | PFO-01/02 and PFR-01–09/12: generated owner chains, independent linked-cache scans and opposite tie choices, retained metadata/child handles, divergent owner-arena branches, shared continuations, exact full-payload graph normalization, residual advancement, one fresh frame per owner, and unchanged earlier frames/normalized snapshots. |
| `persistent_owner_endpoints_preserve_full_width_and_opposite_ties` | Capture/materialization at `usize::MAX` with `u64::MAX` residuals; equal endpoints choose the outer all-bound and inner path-bound handles. |
| `persistent_owner_capture_forcing_and_drop_use_a_small_native_stack` | 20,000 captures and materializations, with exact cell-count assertions and destruction on a 256 KiB native stack. This is a concrete depth witness, not a universal time or RSS theorem. |

The existing opaque-interior graph normalizer is reused generically, preserving
alias identity through an explicit address-renaming map and retaining all payload
fields. Production does not use this map or test projection. Generated tests use
nonempty previous spans with distinct physical provenance and check that appending
to the byte arena preserves the old span's exact located-byte observation. They use
all inactive frame variants and active Regions; they deliberately allow shared
untouched continuations and retained historical child graphs. The active frontier
still contains distinct held Region slots. Tests cover the component, not every
real parser transition, fault construction, callback effect, byte-arena lifetime
or optimized traversal allocation bound. The component checkpoint did not switch
the default persistent reducer; the following section records the later cutover.

#### Focused persistent reducer and topology binding (2026-09-09)

The production `CanonicalPathReducer` now stores a physical focus, immutable
linked owner handle, selected graph stamp and optional root-error overlay per
historical checkpoint. Ordinary edges update the focus and settle only owners
whose cached path boundary is due. Successful publication captures newly opened
regions without mutating their old frames. Public observations use the outermost
metadata and focused pending obligation; they do not reconstruct an ancestor chain.

On refusal, the reducer retains the pre-step graph at its own stamp and overlays
the exact error. Current byte history and global obligation IDs are not rewound.
Private preflight follows the selected terminal, even under a dead root. Failed
preflight allocates no state, owner or frame. Rejection changes only the root
error, as proved by `refusing_hidden_leaf_replaces_only_the_original_root_error`.
Successful private resolution of a dead graph materializes fresh cells at the
saved stamp before calling the shared resolver with the current byte history.

The settlement pending check is shallow by a grammar-reachability invariant:
the only newly exposed active Region has a freshly empty child, and successful
completion resumes counted/tag/scalar continuations, not suspended active Regions.
This is not an assumption that arbitrary fabricated frame graphs have no nested
obligations. Other paths retain the general private walk where required.

| Executable binding in `canonical_frame_storage.rs` | Observations tied to the preceding formal catalogue |
|---|---|
| `focused_persistent_generated_bytes_and_historical_forks_match_eager` | Generated bytes, discontinuous provenance, historical restores, exact events, values, pending payloads, global clock, complete alias-preserving graph normalization; correct/wrong IDs, early resolution, rejection, and failed-call nonmutation. |
| `focused_persistent_nested_frames_and_obligations_match_eager` | Nested frames, zero/nonzero opaque lengths, all five supplied resolution classes, region ordering and malformed boundaries, invalid UTF-8, and continued bytes after faults. Private operations are probed even when public pending observation is absent. |
| `focused_persistent_outer_refusal_keeps_inner_completion_effects` | An actual inner completion appends a Region frame before outer truncation. The allocation survives, historical frames/owners stay unchanged, and the selected graph and stamp return to the pre-edge checkpoint with exact final-byte provenance. |
| `focused_persistent_shared_prefix_has_no_repeated_ancestor_work` | Sixteen width/depth cells with four retained sibling endpoints; zero repeated feed/rebuild/pending walks and zero materializations; actual state, frame, owner and backing-capacity counts. |
| `focused_persistent_deep_traversal_and_drop_use_small_native_stack` | Complete 20,000-region input and destruction on a 256 KiB native stack. The fixture itself is constructed without quadratic prefix copying. |
| `focused_persistent_topology_resolution_rejection_and_restore_match_eager` | Production wrapper versus eager wrapper for byte events, zero/nonzero payloads, explicit resolution/rejection, root unwind, finish, underflow/after-root refusal, and frozen payload provenance after both owners are dropped. |

The eager reducer and its control adapter are retained under `tests/support/`,
not as a production alternative. Their shared-prefix baseline remains unchanged.
The runtime gate requires all 19 frame-storage tests, including these six; normal
topology and frozen-payload tests execute the new production default. The shared
byte grammar and completion callbacks are reused, so these are traversal and
representation differentials, not independent proofs of the grammar itself.
The existing theorem premises and raw-parser/checked-width composition limits
remain explicit. No full arbitrary-input Rust equivalence or complete campaign
closure follows solely from these tests. J-109 records measured resource results,
including the shallow checkpoint-storage increase.

#### Compact checkpoint publication model (2026-09-09)

`CompactPersistentCheckpoints` separates directly stored root paths from linked
owner checkpoints. Both arenas are append-only and addressed by one tagged native
index, not a lookup map. The plain arena contains only the original `InnerPath`;
an entirely ownerless workload allocates no linked checkpoint arena. The linked
arena retains the complete focus, owners, stamp and optional fault from J-109.

For an ownerless checkpoint, reification ignores the stamp. Publication folds
the optional fault into the root status, preserving all other metadata and the
physical frame graph. Restoration may therefore use the current byte length as
the unused stamp. This is a quotient of an unobservable field, not preservation
of the old numeric stamp. Linked fault stamps remain exact. Private preflight
still follows the unchanged frame graph even when the root is dead.

| Exported law | Required property binding before acceptance |
|---|---|
| `encoded_addresses_are_injective` | Distinct direct/linked slots cannot alias. Exercise both tags, zero and upper encodable slots. |
| `encoded_address_recovers_slot_and_tag` | Decode returns the exact slot and arena tag; handle size remains native-index plus unchanged byte handle. |
| `tagged_address_fits_existing_nonzero_vector_bound` | Checked index doubling plus tag fits under the existing non-zero-sized Vec allocation bound. Both element sizes must be at least two bytes; no new input/depth cap. |
| `ownerless_stamp_and_fault_compaction_preserve_exact_typed_graph` | For every ownerless state, arbitrary restored stamp and optional overriding error, materialized typed graph and metadata remain exact. |
| `ownerless_compaction_preserves_private_terminal_and_error_replacement` | Hidden terminal and subsequent root-error replacement are unchanged; include already-dead roots and full-width provenance. |
| `publication_returns_exact_graph_at_every_current_byte_length` | Both publication lanes return handles denoting the exact graph, including old graph stamps with later current bytes. |
| `publication_preserves_every_previous_lookup` | Interleave publications to both arenas and revisit every historical handle at multiple current byte lengths. Linked owner aliases and fields remain exact. |
| `plain_publication_allocates_no_linked_checkpoint` | Every plain publication adds exactly one root and no linked entry/capacity; compare shallow backing against the unchanged eager control. |
| `linked_publication_retains_the_exact_saved_stamp_focus_and_fault` | Linked publication leaves direct storage unchanged and returns all saved fields regardless of supplied current byte length. |

The model proves storage/reification correspondence, not Rust parser execution.
Transition binding must additionally compare edge/resolve/reject outcomes, frame
effects, byte provenance and global clocks after normalization. In the shared
Rust resolver, dead roots are never reopened: leaf resolution advances only an
Open leaf; owner reconstruction preserves or replaces status with another error.
Moreover, resolution explicitly establishes the current byte-length stamp before
any new focus capture. Those caller facts must be exercised by a dead-direct
resolve/edge/reject sequence and the existing complete differential suite.
Native layout and resource acceptance require actual measurements after the
implementation; none follows solely from list-based mathematical storage.

The runtime bindings are now implemented in
`models/tests/support/canonical_frame_storage.rs`:

- `compact_checkpoint_addresses_preserve_slot_tag_and_native_width` binds the
  first three laws to both native tags, boundary slots and actual element sizes.
- `compact_checkpoint_publication_preserves_both_historical_arenas` binds the
  remaining storage laws through mixed publication histories, all retained
  handles, three restoration stamps including `usize::MAX`, already-dead roots,
  overriding faults and full-width provenance. Opaque frame/owner IDs in this
  storage-only test are not dereferenced; the graph differentials below supply
  the grammar-reachable binding.
- `compact_checkpoint_dead_direct_private_operations_preserve_exact_graph`
  compares actual edge, rejection and both accepting/refusing resolution calls
  against the unchanged eager implementation after a nested fault. Later bytes
  and accepted evidence cannot reopen the root or allocate linked checkpoints.

All previous exact graph, obligation-clock, historical arena-prefix, production
topology and 20,000-region small-stack controls remain active. J-110 records the
executed gates and full counter matrix, including fixed-header exclusions and
the deep-workload capacity-rounding cost of separate arenas. These measurements
do not establish end-to-end admission throughput or allocator/RSS equivalence.

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
Since 2026-10-17 `with_write_cursor_at` is compiled only for unit tests, because no production
caller remains; `with_write_cursor` is the one live write gateway.

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

### S-expression streaming projection model (2026-09-12)

`CanonicalSexprProjection.v` supplies the observation-level model for removing
repeated typed reconstruction from `ParToSExpr`. A subject carries literal token
chunks and ordered children. An independent recursive renderer defines the
reference output. The continuation machine uses `Visit`, `Siblings` and `Emit`
jobs, processing one sibling at a time. Its execution relation has constructive
completion and deterministic, exact reference output; it is not merely a
conditional assertion about successful executions. The model also connects to
the existing `StackSafePDA` fold theorem.

Render-once/repeat preserves each occurrence and separator. Three-state
classification (`Zero`, `One`, `Many`) agrees with mathematical occurrence
cardinality without requiring a bounded integer sum. Selected family order is
Send, Receive, New, Expr, Bundle. Ignored families affect neither that order nor
the zero/one/many decision, including when they are the only families present.

The unique, exhaustive **modeled observation** catalog has 33 cases. Before
runtime implementation, extract the following complete case binding matrix:

| Model cases | Required native differential/property obligations |
|---|---|
| `ParZero`, `ParOne`, `ParMany` | Exact Nil/unwrapped/wrapped output; selected-family order; mixed and repeated endpoints; category classification near integer boundaries without summing overflowing counts. |
| `SendCase`, `ReceiveCase`, `BindCase`, `NewCase`, `BundleCase` | Correct protobuf fields and ignored metadata; missing children; zero/negative/positive bind counts; empty/repeated data and binds; exact separators and parentheses. |
| `BoolCase`, `IntCase`, `StringCase`, `UriCase`, `BytesCase` | Canonical scalar views equal typed values; signed ZigZag and integer extrema; empty/non-ASCII/quote-containing text remains unescaped; exact byte hex. Scalar conversion is a separate binding, not proved by opaque token strings. |
| `ListCase`, `TupleCase`, `SetCase`, `MapCase`, `PairCase` | Preserve child and pair order; empty-form spacing; missing pair sides become Nil; cursor continuation handles wide and deep children without materializing them. |
| `BoundCase`, `FreeCase`, `WildcardCase`, `MissingVarCase` | Signed indices and exact `_`, `$`, wildcard and `var` distinctions; absent EVar child versus present empty Var. |
| `NegCase`, `NotCase`, `MultCase`, `DivCase`, `PlusCase`, `MinusCase` | Exact operator token, operand order, missing operands and unary/binary punctuation. |
| `MethodCase` | Target, unescaped name and ordered arguments; retain `(Nil. )` for the empty form. |
| `MissingExprCase`, `FallbackExprCase`, `IgnoredFamilyCase`, `MissingParCase` | Distinguish Nil, `(expr)` and ignored families; enumerate every generated unsupported Expr arm, including EPathMap and extended numerics; ignored-only Par is Nil, never an empty rendered occurrence. |

The machine, repeat and family laws additionally require generated nested
combinations, exact token-to-string concatenation, snapshot immutability, and
the unchanged 128 KiB deep test. Preserve the current decoded-PDA formatter as a
shallow test-only oracle: it is not a recursive oracle. Resource controls must
observe zero typed component decoding/rebuilding, bounded immediate-field
visits and one-child-at-a-time pending work; semantic proofs alone establish no
native allocation bound.

**Verification and boundary.** Final compiler and independent kernel checks
pass, with 18 closed-context reports and no axioms or admissions. This proves
the observation-level machine, not the correspondence between actual protobuf
bytes and typed constructor observations. `native_field_view_obligation` names
that remaining proposition without assuming it. Field selection, canonical
ordering, scalar conversion and binder generation still need their refinement
and native bindings before a complete formatter-equivalence claim. No runtime
formatter change has been made at this checkpoint. The model is registered in
`_CoqProject` and the default maintained proof gate; no parser or dependency is
introduced. See journal J-147 for exact artifacts and remaining work.

#### Field-selection refinement and native obligations

The subsequent `FieldViews` extension increases the checked report count to 27.
It compares independent last-write/append decoder observations with first-field
and contiguous-run cursor observations. Singular uniqueness is per semantic
slot (including an entire oneof), not merely per numeric tag. Repeated fields
must form one source-ordered run. These are admitted resident-byte premises,
not permission to accept arbitrary protobuf input without validation.

| Field-view law | Required native binding before acceptance |
|---|---|
| Unique singular selection | Present/absent children and scalar extrema against the frozen decoded formatter; duplicate-field negative control demonstrates why canonical admission matters. |
| Ordered repeated selection | Zero, one and many children, preserving order; split-run negative control; continuation visits only one child at a time. |
| Absent defaults | Missing children yield Nil, absent Var yields `var`, absent scalar/binder fields retain their historical defaults. |
| Metadata insertion neutrality | Vary ignored fields without changing selected field output. |
| Metadata register neutrality | Metadata before/after selected fields cannot replace a child or add an occurrence. |
| Exact family mapping | Mixed Send/Receive/New/Expr/Bundle order, with ignored-only families excluded from wrapper arity. |
| Repeated source-slice order | Non-identical siblings and map pairs compare with the independent decoded reference. |
| Child-window composition | Nested offsets stay within the same callback payload; exact child slices, no owned subtree or copied payload. |
| Invalid-input/nonvacuity controls | Explicit duplicate singular and split repeated runs distinguish the two algorithms outside their premises. |

The native implementation will reuse one offset-frame vector across encoded
endpoint callbacks. Each callback drains its frames before the zipper advances;
offsets refer only to that callback's borrowed payload. Repeated-field frames
advance monotonically and schedule one child, avoiding a width-sized worklist.
This representation is the native binding of modeled windows, not a new parser.
Scalar decoding, Rust dispatch, actual byte framing and allocation complexity
still require native verification; the 27 checks do not prove those by themselves.

Before production changes, four additional baseline tests pass in debug and
release. They cover all 33 observation cases, signed/empty forms, every current
generated fallback arm, and generated shallow terms. The frozen old formatter
and these tests live under `models/tests/support/`. The original four tests keep
their names and depth/stack parameters. These baselines are not yet evidence for
a byte-native implementation.

The subsequent byte-native implementation binds these laws using endpoint-local
offset jobs. Its six additional controls in `models/tests/support/par_to_sexpr.rs`
check non-vacuous duplicate/split-run premises, exact mixed-family mapping and
saturated cardinality classification, multibyte occurrence copies and snapshots,
width-independent pending work, a 4,096-level 128 KiB traversal, and 64 generated
ordered nested cases. The typed-decode observer is scoped to `decode_exact` and
has a positive frozen-oracle control. Existing protobuf-header instrumentation
is reused rather than introducing a second parser. The focused debug inventory
passes all 14 tests; full-package acceptance is tracked separately in J-148.

Independent review adds a fifteenth explicit populated Method, multi-bind and
ignored-metadata fixture. Final focused debug passes all 15; the full release
library passes 502 tests. Exact identity comparison of the earlier 501-test full
debug pass and final focused debug pass establishes aggregate coverage of all
502 release identities. The maintained admission route now checks the 27 proof
reports and exact 15-test formatter counts in both profiles, alongside the
existing tiny-stack admission and compiler controls.

### Compressed teardown navigation refinement (2026-09-11)

`PathMapReversePruneTeardown.NavigationRefinement` independently computes scalar
and unary-compressed rightmost descent over the existing ordered trie model.
Expanding compressed route chunks gives the scalar route and the same selected
leaf and deferred continuation. Reconstructing that continuation gives the exact
original drain trace; composing it with Phase A preserves real-value disposal
order and one-logical-edge pruning. Ghost continuations describe traces and are
not proposed runtime allocations. Route entries are ordered child ordinals, not
native byte labels or physical node offsets.

The complete eight-entry `navigation_catalog` is proved duplicate-free and binds
every constructor. Its required native bindings were extracted below **before
runtime implementation**. The 2026-09-12 native follow-up is recorded after the
table; existing semantic controls remain required rather than being replaced.

| Catalog invariant | Required native/property binding |
|---|---|
| `ExpandedRouteInvariant` | Compare bytewise greatest-child descent against borrowed-read compressed navigation on every generated topology and starting focus; expanded suffixes agree exactly. |
| `TerminalLeafInvariant` | The selected suffix ends at the same existing leaf; after the read borrow closes, direct write descent reaches that leaf without extending topology. Include dangling leaves. |
| `GreatestChildInvariant` | Dense byte sets and asymmetric sibling chains select the lexicographically greatest child, including 0 and 255. |
| `ExactContinuationInvariant` | Resuming the unchanged bubble and sibling traversal matches the scalar reference's complete barrier/prune trace. |
| `TerminalTraceInvariant` | Whole two-phase generated drains agree in real-value removal, topology completion and all observer events. |
| `DisposalAndPruneInvariant` | Preserve exact preorder disposal, no synthetic-value disposal, edge conservation and maximum prune width one; retain CoW sibling and small-stack controls. |
| `CompressedLineWitnessInvariant` | Long unary/value-free lines use a nonzero compressed jump and reach the scalar leaf; instrument navigation separately from bubble work. |
| `GreatestBranchWitnessInvariant` | Unequal sibling chains select the greatest branch and then resume every remaining branch in the reference order. |

All nine newly printed laws are closed; the complete base theory reports 32
closed contexts and independent kernel checking passes. The first three compiler
attempts preserve local proof-script failures before the successful fourth run.
These conclusions do not prove native zipper borrowing, private sentinel/layout
behavior, allocation bounds or performance. The proposed runtime uses a borrowed
read cursor plus one reusable relative-suffix buffer, not an owned interior trie.
The existing one-edge barrier/prune algorithm remains unchanged until its own
contract is separately revised. See journal J-145 for evidence and limitations.

**Native follow-up (2026-09-12).** Three new scalar-differential tests bind the
eight rows above using empty/root-only/dangling/long-line, dense/comb/asymmetric
and generated-history cases. They compare exact routes, terminal values,
continuations, complete disposal/prune traces and shared snapshots. The complete
guarded-map group passes 34 tests in debug and release, with one existing
measurement-only ignore. Concrete borrowed-subtree assertions additionally check
value-pointer identity and the initial local origin's PathMap-0.2.2 bound of
48 bytes, including inside the production helper under `cfg(test)`. This is not
a runtime traversal limit or a formal proof of the released crate's internals.
The long-line witness demonstrates a compressed jump separately from bubble
work; it is not a production jump counter or performance result. J-146 records
exact source/log hashes and the still-open whole-debug/formatter resource gate.

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
| RPO-11 | B: `production_component_admission_is_differentially_equivalent_for_all_ten_families` | Actual production key validator versus retained typed validator, with the original nine families and the subsequently added tenth family, payload-bit mutations, unknown fields and truncation. |
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
cargo test --locked -j4 -p models --lib production_component_admission_is_differentially_equivalent_for_all_ten_families -- --test-threads=2
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

#### Checked ACT line transport: invariant catalogue (2026-09-09, task 6018)

J-112 measures repeated ingestion of opaque nested EPathMap bytes. The first
required refinement is transport across already validated ACT line interiors,
not a replacement grammar or a singleton-map shortcut. The
`CheckedACTLineTransport` module in `RPB3Admission.v` models forward and
reverse spans over an immutable located-byte source. A located byte retains
both its byte value and its physical source/offset pair.

The scalar reference is deliberately partial: `None` means outside the modeled
interior, **not** production EOF or a parser error. For forward transport, the
strict bound excludes the last line byte. That byte, its endpoint value, child
descent and terminal checks remain with the existing scalar cursor. For reverse
transport, the bound excludes the line-owning node's final leave; that node need
not be the root. Each forward byte represents two events, Edge then
AbsentValue; each reverse byte represents one Leave. Both directions preserve
exact physical provenance.

The source function is captured by the offer; commit has no source-substitution
argument. Concrete refinement must bind this function to the cursor-owned
source at `line.start + index`, with a valid bound and stable, available reads.
An arbitrary `IndexedByteSource` can use interior mutability; its trait alone
does not discharge that premise. Existing immutable indexed regions provide
the intended integration boundary.

| Exported invariant | Required executable property before production acceptance |
|---|---|
| `checked_line_expands_to_exact_scalar_events` | At each eligible line in arbitrary branched ACT, compare a bounded commit with exactly twice as many scalar events, including every absent value and its source. |
| `checked_unwind_expands_to_exact_scalar_leaves` | Compare every bounded reverse span with scalar leaves in descending source-offset order, including partial runs and complete line unwind. |
| `stale_offer_refuses_without_a_new_cursor` | A detached stale snapshot cannot change selected state. The Rust capability must make cross-cursor use, cursor mutation, or replay while borrowed impossible. |
| `zero_width_offer_is_refused` | Zero and excessive widths refuse before changing any cursor, counter, reducer store or source descriptor. The excessive-width guard follows from the checked-commit trace and strict bounds. |
| `rewind_invalidates_an_old_offer` | Compile-fail/API tests establish that a live exclusive span prevents rewind and cannot survive cursor replacement. No runtime generation counter is required. |
| `every_forward_prefix_keeps_the_final_byte_scalar` | Enumerate every complete edge/value prefix and compare index, phase, next event and retained boundary against scalar traversal. |
| `every_unwind_prefix_stops_before_the_node_leave` | Enumerate every partial leave run and ensure the node leave, parent stack and sibling transition remain pending. |
| `committed_forward_preserves_other_topology_authority` | Compare validated plan, inactive frames, remaining children, next-record index, leave index, EPM1 value reader/count, pending endpoint and terminal state. |
| `committed_unwind_preserves_other_topology_authority` | Preserve the same fields during unwind, including the completed forward line index. |
| `accepted_commit_has_the_exact_scalar_trace` | For both directions, every successful checked commit has the exact modeled scalar trace and resulting state; failed preflight is nonmutating. |
| `odd_forward_prefix_preserves_the_pending_absent_value` | At every odd event prefix, compare the LineValue phase, unchanged line index, last source, event count and pending absent value. |
| `expanded_event_counts_preserve_logical_work` | Forward spans count twice their width and reverse spans once their width. Measure physical source reads separately; avoiding those reads is part of the optimization. |

The model's context is an abstract encoding of the fields listed above; proving
that this encoding is preserved does not by itself prove the Rust projection.
Likewise, logical generation is a negative-control device, not a proposed native
counter. The selected implementation direction is a private, non-Clone,
non-Copy span capability holding an exclusive mutable borrow of the ACT cursor
and its source. Drop releases the capability without mutation; commit consumes
it after width preflight. An analogous EPM1 capability must retain the enclosing
event/terminal authority. This prevents stale state without adding an artificial
generation overflow limit or cloning an entire cursor.

```text
Algorithm CHECKED-INTERIOR-TRANSPORT(cursor, persistent_path)
1. Borrow a forward-line or reverse-line capability from the validated cursor.
2. Intersect its width with the persistent path's strict payload/owner bound
   or compressed restoration run. Never cross a scalar completion boundary.
3. Preflight source ownership, width arithmetic and both state updates.
4. Commit both already-preflighted updates; count expanded logical events.
5. If no span is eligible, release the capability and perform the existing
   scalar operation with unchanged error and callback ordering.
```

Steps 2–4 still require the persistent-path composition proof before runtime
implementation. Remaining proof obligations within task 6018 are source-backed
append/slice normalization and lifetime preservation; every virtual prefix's
typed graph and shared aliases; exact compressed restoration with fresh global
obligation clocks; and joint refusal/first-error/effect ordering. The source
descriptor must resolve directly to underlying immutable storage, not another
nested descriptor or a newly generated per-byte index. Forward batching without
compressed reverse transport would retain the measured depth multiplier.

Qualification must extend beyond the current singleton opaque timing cells:
branched/shared opaque prefixes, multiple ACT lines and source fragments,
value-free and malformed interior endpoints, zero/one-byte bounds, tied owner
boundaries, wrong obligation IDs, pending/dead states, retained siblings, inner
effects before outer refusal, all restore prefixes, and small-stack destruction.
This catalogue is a list of required bindings, not a claim that the new Rust
span implementation or its properties already exist.

#### Normalized source spans: invariant catalogue (2026-09-09, task 6018)

`NormalizedSourceSpans` extends the existing indexed-window model with
checked coalescing, direct-root subview composition, final scalar-byte sharing,
and append-only backing registration. The model's owner is the identity of an
immutable backing, **not** a physical provenance source number or an equality
hash. Byte/provenance equality alone never authorizes coalescing.

| Exported invariant | Required executable property |
|---|---|
| `coalescing_accepts_exact_identity_adjacency_and_bounds` | Accept consecutive windows of one immutable backing; refuse gaps, overlaps, reversed order, distinct owners and arithmetic overflow before mutation. |
| `coalescing_preserves_exact_located_bytes` | Coalesced bytes and every physical position equal concatenation across all split points, including fragmented provenance and empty spans. |
| `coalescing_preserves_root_validity_and_the_right_end` | The merged range stays inside its root and preserves the right operand's explicit end anchor, including a zero-width right operand. |
| `different_backing_identity_never_coalesces` | Independently allocated equal contents, repeated source IDs and matching physical offsets do not establish backing identity. |
| `coalescing_is_associative_without_intermediate_source_layers` | Three adjacent windows coalesce to one original-root view in either grouping, with the same bounds and final anchor. This law concerns the descriptor operation; actual checked merges also require adjacency. |
| `repeated_subviews_normalize_to_the_original_source` | Nested checked slicing equals direct slicing in owner, offset, width and anchor; increasing view nesting never creates a chain of view descriptors. |
| `normalized_subviews_preserve_each_physical_position` | Compare every byte position against the parent at the translated logical offset; include discontinuous sources and nonzero physical origins. |
| `coalesced_boundary_scalar_remains_in_the_original_backing` | Appending a source-aware one-byte final boundary yields the original backing's extended view, exact scalar byte/provenance and endpoint, without a fresh payload page. |
| `retaining_a_source_preserves_every_existing_backing` | Register new sources and append siblings while retained old windows continue to denote the same immutable backing. |
| `retained_source_slot_denotes_the_original_root` | Newly retained source IDs resolve to the exact original root; exercise independent equal contents and keep source lifetimes valid after ancestor frame release. |

A byte derived from an ACT branch mask is not necessarily the byte physically
stored at its reported source offset. Source-aware scalar append must retain
the logical ACT line offset when the byte is literal. Derived branch bytes
continue to use explicit located-byte storage. Physical provenance must never
be inverted to invent a logical source index.

The storage design retains original borrowed root slices plus checked ranges,
rather than successively shortened slices. Adjacent literal ranges can then
merge without reconstructing pointers or copying the final scalar byte.
Frozen page windows retain their existing immutable backing. Completion
anchors remain separate from payload sharing and must survive empty slices.

This is the terminal-window foundation, not the complete fragmented source
index. General mixed-source composition must share and splice balanced
subtrees, preserve constant-size history handles and source lifetimes, avoid
per-layer flat-index rebuilding, and use stack-safe release. Adding SourceSpan
leaves while retaining today's whole-payload `freeze` rebuild would not meet
task 6018. The current coalescing proofs do not establish balancing, native
allocation bounds, full topology admission, or Rust implementation correctness.

#### Source-window implementation and reverse-entry bindings (2026-09-09)

The terminal-window implementation now retains original borrowed roots and
normalized frozen windows. The ten `normalized_source_spans` tests bind the
preceding catalogue: three generated properties compare located-byte oracles,
adjacency and nested subviews; six semantic controls cover equal-but-distinct
backings, empty/end anchors, final literal bytes, frozen inner/outer offsets,
retained siblings after parent release and overflow; one layout witness checks
the production mirror separately from the test-only backing variants.
The retention controls exercise existing backing lifetime and arena append,
not the future mixed-source registration/composition implementation.

Splitting the optional borrowed end anchor into `Option<u32>` source and `usize`
offset avoids padding in an optional full provenance record. It uses no sentinel
and preserves every source/offset value. The existing 72-byte test-layout guard
is unchanged; the production mirror remains 64 bytes on the measured host.
The full release models library passed 415 tests without failures or ignores.

The ACT cursor has one unobservable reverse-entry transition: a line in the
Children stage with zero pending children becomes Leave before emitting its
first reverse byte. The concrete phase projection maps both such Children
and raw Leave to the model's LineLeave, using `leave_index`. Pending children
remain ineligible. The raw tag is excluded from preserved abstract context;
remaining children, completed forward index, plan, next-record position and
inactive frames remain included. Accepted positive commits must match the
entire raw scalar post-state, not merely this abstract projection.

| Additional exported ACT invariant | Executable binding |
|---|---|
| `pending_children_never_admit_reverse_transport` | Check that Children with pending descent never offers a reverse span, including branched shared-prefix images. |
| `reverse_normalization_changes_only_the_unobservable_phase` | Snapshot every record, frame and cursor field around offer/drop/refusal; accepted reverse entry changes only leave index and raw stage. |
| `accepted_reverse_entry_commit_matches_the_exact_scalar_trace` | Compare every positive reverse width against scalar calls, including the first and only leave of a one-byte line. |
| `reverse_entry_refuses_zero_or_excessive_width_before_normalization` | Zero, oversized and full-width invalid requests preserve raw Children as well as counters and all retained authority. |

The six tests under `checked_line_transport` bind these and the original twelve
ACT transport laws to the cursor-local implementation. They compare every
eligible prefix, odd forward prefixes, complete and partial reverse runs,
exact source offsets, next scalar events and complete raw cursor state.
The compiler fixture `rspace++/tests/compile_fail/act_line_span_borrows.rs`
has a positive released-borrow control and eight negative configurations:
live rewind, live next-event, cursor replacement, double commit, Clone, Copy,
escaping the cursor and substituting another cursor. The verification script
requires the expected Rust error code for each negative control; an unrelated
compiler failure cannot satisfy it. No generation counter or runtime lock is
introduced. An overflowing expanded-event count refuses batching, not input
admission; scalar handling remains available.

These laws require immutable source interpretation. The owned indexed source
cannot be substituted through the capability, but arbitrary interior-mutating
`IndexedByteSource` implementations do not automatically satisfy immutability.
The intended production composition uses immutable `IndexedByteRegion` roots.
The ACT capability is not yet an EPM1/path transaction: enclosing value-reader,
event counters, private effects and restore state require their joint proof.

#### Absolute payload realization: preimplementation invariant catalogue (2026-09-09)

`AbsolutePayloadRealization` retains the typed countdown reference while modeling
a physical payload marker whose remaining length is its absolute end minus the
selected graph stamp. The end is payload start plus declared payload length.
Awaiting resolution retains an explicit physical endpoint; equality with the
payload end does not manufacture the awaiting phase. A saved stamp remains
necessary after failure, even for ownerless states whose fault has been folded
into metadata. Otherwise later ignored bytes change hidden typed residuals.

All fifteen exported invariants below are prefixed `AbsolutePayloadRealization.`.
They are proved model obligations, not claims that the new native representation
or its required properties have been implemented.

| Exported invariant | Required native property |
|---|---|
| `physical_payload_realizes_all_existing_typed_obligation_fields` | Reify exact obligation ID, kind, declared length, tag provenance, payload start, continuation and metadata from the physical marker. |
| `absolute_stamp_advance_changes_exactly_the_typed_payload_and_owner_residuals` | Compare advanced leaf and every held owner with the independent typed countdown; all other fields remain identical. |
| `strict_absolute_window_realizes_a_live_typed_graph` | Intersect the payload, Region and path bounds before batching; refuse crossing any boundary. |
| `every_historical_absolute_prefix_retains_all_strict_typed_boundaries` | Restore every prefix of a shared span and compare all residuals, including zero-width prefixes and strict final-byte exclusion. |
| `strict_absolute_prefix_preserves_physical_continuations_and_held_owners` | Append frames and siblings, then verify original continuation identities and held-owner realization at every retained stamp. |
| `absolute_payload_preserves_the_exact_typed_private_terminal` | Verify exact private phase, obligation ID and tag source for arbitrary held-owner depth. |
| `absolute_payload_private_preflight_keeps_exact_id_phase_and_source` | Wrong-ID and wrong-phase resolve/reject preserve exact errors and preflight state, including partial payloads. |
| `strict_absolute_window_refines_repeated_typed_scalar_payload_steps` | Differentially compare each located byte and callback event with typed scalar payload steps, excluding completion. |
| `live_absolute_checkpoint_uses_the_selected_byte_prefix` | Fault-free live checkpoints select each byte-prefix length without storing a replacement frame per byte. |
| `saved_fault_stamp_preserves_the_entire_typed_graph_under_dead_edges` | Appending bytes after a deferred fault preserves its saved graph, hidden residuals and provenance. |
| `saved_direct_dead_stamp_is_required_even_without_a_fault_field` | Repeat the previous property after failure is represented directly in path metadata with no separate fault field. |
| `publishing_any_checkpoint_preserves_every_historical_prefix_and_shared_alias` | Append arbitrary new checkpoints while every previous ID and byte-prefix alias reifies identically. |
| `explicit_awaiting_phase_keeps_its_original_physical_endpoint` | Advancing unrelated bytes never changes an explicit awaiting endpoint or invents a phase transition. |
| `moving_an_ownerless_dead_byte_stamp_changes_its_hidden_typed_payload` | A negative control that substitutes current byte length for the saved stamp must detect changed hidden payload state. |
| `dropping_the_stamp_after_folding_a_fault_into_direct_metadata_is_not_sound` | The corresponding negative control must fail for direct ownerless Dead metadata, not just separately stored faults. |

The model uses unbounded natural arithmetic. Native widened endpoint arithmetic,
checked address conversion and batch refusal require separate executable binding.
The typed strict-step relation deliberately covers only nonfinal payload bytes;
it is not a proof of the full parser grammar, completion callbacks, compressed
restore runs or failed-preflight/global-obligation-clock composition. Those
remain required before activation. Existing compact-checkpoint proofs are
preserved as history; their ownerless stamp erasure cannot be applied blindly
to the proposed absolute-payload representation.

#### Absolute payload checkpoint and scalar boundary refinement (2026-09-09)

`AbsolutePayloadCheckpointBoundary` supplies the next twenty model invariants.
It distinguishes the current byte-history length from the selected graph stamp:
live checkpoints use the former, whereas failed checkpoints retain the latter.
Direct storage is reserved for ownerless, unfaulted, nondead states. Other states
use immutable linked records; publication preserves every prior lookup.

The native residual is declared length minus consumed payload length, computed
with checked subtraction and conversion. This avoids constructing a widened end
for each payload observation while allowing every `u64` declared length. The
scalar model retains the independent countdown transition: nonfinal native steps
keep one physical frame, while the reference creates a fresh countdown frame.
Equivalence compares semantic graphs, not physical allocation identities.

The following is the exhaustive exported-invariant binding plan for this module.
The regression controls are qualified against the countdown implementation;
production activation and post-change verification must be recorded separately.
Every theorem name below has the prefix `AbsolutePayloadCheckpointBoundary.`.

| Exported invariant | Executable obligation |
|---|---|
| `direct_publication_requires_no_owners_no_fault_and_nondead_metadata` | Generate every owner/fault/status combination and check address class. |
| `retiming_preserves_selected_stamp_typed_graph_and_all_nonstamp_fields` | Vary selected byte lengths; live graphs retime, failed graphs retain saved time and all other fields. |
| `publication_returns_exact_fields_at_the_selected_byte_stamp` | Compare published records with the explicit selected-state interpretation. |
| `publication_preserves_valid_selected_typed_interpretation` | Differentially compare active payloads and held-owner residuals after publication and restore. |
| `every_dead_or_fault_checkpoint_retains_its_saved_stamp_in_linked_storage` | Append ignored bytes after ownerless rejection or nested failure; verify hidden countdown and saved stamp. |
| `publication_preserves_every_old_direct_and_linked_lookup` | Recheck all retained handles after arbitrary subsequent publications. |
| `ownerless_live_publication_does_not_allocate_a_linked_checkpoint` | Assert the linked arena remains unallocated for a sequence of plain live states. |
| `native_consumed_subtraction_equals_the_derived_absolute_end` | Check native residuals against widened arithmetic, including full-width lengths and historical prefixes. |
| `absolute_obligation_creation_refines_the_typed_graph_event_and_fresh_clock` | Compare creation metadata, event, selected graph and machine-global next ID with the eager reference. |
| `zero_length_creation_is_explicitly_pending_at_the_length_source` | Assert immediate pending phase with the exact length-varint endpoint. |
| `positive_length_creation_stays_payload_even_when_the_numeric_end_exceeds_a_native_limit` | Exercise `u64::MAX` lengths without allocating a declared-size buffer or narrowing the length. |
| `nonfinal_scalar_retains_the_physical_leaf_and_refines_the_fresh_typed_frame` | Assert unchanged native leaf allocation and identical reference residual, event and continuation at every prefix. |
| `final_scalar_creates_the_exact_awaiting_frame_at_the_actual_physical_byte` | Assert the final frame's explicit phase, physical source, metadata and continuation. |
| `invalid_stamps_and_explicit_pending_phases_never_feed_a_payload_byte` | Invalid residual calculations refuse; pending input cannot enter scalar payload feed. |
| `explicit_pending_phase_refuses_even_if_the_stamp_looks_like_live_payload` | Phase-only private eligibility and edge refusal must not infer phase from residual arithmetic. |
| `consecutive_births_use_distinct_actual_obligation_ids_without_checkpoint_clock_state` | Historical forks preserve globally fresh obligation IDs rather than restoring an old clock. |
| `selected_checkpoint_private_terminal_preserves_phase_id_and_source_even_when_dead` | Inspect hidden obligations under failed roots and compare exact ID, phase and tag source. |
| `failed_private_preflight_preserves_the_complete_concrete_world` | Wrong-phase/ID operations preserve frame, owner and checkpoint arenas, byte histories, restore handles and clock. |
| `wrong_id_private_operations_return_the_hidden_source_without_mutating_any_world_field` | Both resolve and reject return the hidden tag source without allocation or publication. |
| `matching_payload_resolve_is_nonmutating_while_reject_preflight_passes` | Early resolve refuses without mutation; rejection remains eligible before payload completion. |

The concrete world model covers refusal before private mutation. Its accepted
operation is an explicit parameter, not a proof of successful resolution or
owner settlement. Existing effectful settlement models and differential controls
remain necessary. Compressed restore and the complete ACT/admission transaction
are also outside this module; these results do not establish whole-import linear
complexity. In particular, successful private resolution of an already failed
root does not revive it and must not retime its hidden graph to ignored bytes.

Runtime binding is now implemented and qualified in journal J-117: the eager
storage policy retains the original countdown transitions, native residuals are
checked against widened arithmetic, and tests require unchanged physical leaf
frames across nonfinal bytes and strict bulk interiors. The final 419-test model
library and integrated 116-test admission gate pass, including all 26 frame-storage
tests. This closes the scalar payload representation binding, not compressed
restoration, full ACT transaction composition or whole-import resource bounds.

#### Persistent opaque state reuse: native binding catalogue (2026-09-10)

`PersistentOpaqueStateReuse` composes the checked linked-owner boundary cache
with absolute payload realization. A nonfinal opaque byte does not change the
physical grammar frame, owner chain, metadata or obligation clock. Consequently,
the existing checkpoint address can select the advanced graph through its new
byte handle. The full handle remains significant: two sibling byte histories
can have the same state address, length and obligation ID but different content.

The optimization is restricted to an unfaulted open payload with checked native
arithmetic and an endpoint strictly before both the payload end and every held
Region/path boundary. Equality at any boundary follows the existing scalar
transition, preserving completion, refusal and exact physical provenance. The
cache-to-owner theorem relies on the existing linked-owner projection and open
owner representation invariants; it does not introduce an extra runtime flag.

The seven exported invariants below all have prefix
`PersistentOpaqueStateReuse.`. This catalogue specifies the executable binding
before production activation; qualification is recorded separately.

| Exported invariant | Executable binding |
|---|---|
| `checked_linked_cache_implies_every_typed_owner_is_strict` | Existing generated owner-capture/cache tests, plus malformed nested owner boundaries checked against the independent eager reducer. |
| `checked_native_guard_realizes_the_existing_strict_absolute_window` | Generated native guard tests cover payload length, start/stamp arithmetic, explicit phase, status, fault and native overflow; boundary fixtures exclude equality and overrun at any owner. |
| `guarded_state_reuse_refines_every_located_prefix` | Nested payload prefix and divergent-byte tests compare every retained semantic graph, exact located event and byte history with the independent countdown reference. |
| `guarded_scalar_reuses_the_physical_leaf_with_the_exact_scalar_event` | Nonfinal bytes retain the state address and physical frame/owner counts; final bytes retain the existing explicit pending endpoint and event. |
| `reuse_keeps_the_exact_state_address_and_requires_the_full_byte_handle` | Same-depth siblings share the checkpoint address but retain unequal full handles and unequal contents; all old handles remain readable. |
| `reused_scalar_publishes_only_the_byte_and_exact_restore_predecessor` | Check unchanged checkpoint/frame/owner allocation and obligation clock; existing topology differential tests restore exact prior byte handles and phases after each leave. |
| `every_reused_byte_handle_selects_the_existing_published_state` | Recheck all retained nested prefixes after subsequent appends and forks; existing direct/linked publication properties independently verify lookup and retiming. |

This is a scalar checkpoint-allocation refinement, not a compressed restore
implementation. The byte arena and topology restore list still retain scalar
history. Source-region composition, joint ACT transactions and whole-import
time/space bounds remain separate required work. In particular, proving that a
record need not be copied is not a proof that the entire import is linear.

The native binding is now qualified in journal J-119: 237 assumption checks
close, independent kernel checking passes, all 422 model-library tests pass,
and the integrated gate passes 119 focused model tests and six ACT transport
tests. The preactivation unchanged-state-address regression fails as expected.
The generated guard property, malformed-owner boundary fixture and complete
nested-prefix/fork differential comprise three new tests, bringing the
frame-storage gate to 29. The shared stream/persistent trace helper retains all
semantic comparisons but treats persistent allocations as an upper bound;
accepted interior steps have a separate exact zero-allocation assertion.

#### Compressed scalar undo: preimplementation binding catalogue (2026-09-10)

The planned `CompressedScalarUndo` refinement retains one run descriptor for
consecutive saved predecessors that share a checkpoint address. A descriptor
contains the latest predecessor handle and its first logical depth. The existing
topology protocol guarantees that each earlier predecessor is the exact located
prefix one byte shorter. A successful edge extends or creates the latest run;
one leave returns its latest predecessor and either removes a singleton run or
shortens its byte view once. Private resolution changes the current checkpoint
without changing its bytes or saved undo history.

The ghost expansion is an ordinary scalar undo list of checkpoint addresses and
located-byte sequences. Native code must not construct that expansion. Equal
located-byte prefixes can have different arena descriptor roots across page
boundaries; observation equivalence therefore includes the exact checkpoint
address, logical length and every byte/provenance pair, not raw descriptor
identity. Different sibling contents remain different observations. Window
interpretation is parameterized by stable source roots: preservation when the
concrete arena grows remains a prerequisite supplied by the existing arena
contract and its historical/frozen-source tests, not a new result of this undo
model.

This catalogue is the required executable binding before runtime activation.
All names below have prefix `CompressedScalarUndo.`; final proof qualification
and any additional exported invariants must be recorded before activation.

| Model invariant | Required executable binding |
|---|---|
| `strict_shortening_is_the_existing_checked_source_window` | Page-crossing and frozen-page undo uses checked prefix slicing with exact source provenance and no arena mutation. |
| `zero_prefix_normalization_is_observational` | Final predecessor restores the empty byte sequence, regardless of internal empty-span normalization. |
| `compressed_emptiness_is_exact` | Compare compressed and scalar history emptiness after every edge and leave. |
| `remembering_refines_exact_scalar_predecessor_push` | Independent scalar `Vec<Handle>` oracle records the previous handle; compare maximal run count and all subsequently restored observations. |
| `popping_refines_exact_scalar_head_and_tail` | Every leave returns the scalar oracle's exact state ID and located bytes; remaining history has the corresponding run count. |
| `every_compressed_historical_prefix_is_a_valid_located_handle` | Retain old handles and frozen payloads while appending and branching across page boundaries; all remain readable. |
| `successful_edge_preserves_scalar_topology` | Append through healthy and malformed nested topology, comparing every transition and semantic graph with the independent eager parser. |
| `successful_leave_preserves_scalar_topology` | Partially unwind, append divergent siblings, then unwind again; compare all prefixes and graph phases. |
| `byte_preserving_private_operations_preserve_the_exact_undo_topology` | Successful resolve/reject preserves complete restore records and current bytes while publishing a new checkpoint; later edges and backtracking remain exact. |
| `exact_state_and_located_bytes_preserve_every_selected_graph_and_query` | Compare active, pending, value, error-source, frozen-payload and hidden failed-state observations, not descriptor equality. |
| `compressed_pop_is_the_exact_existing_scalar_world_transition` | Compare every popped predecessor and semantic graph with the eager topology's scalar history. |
| `restoring_never_changes_any_checkpoint_frame_owner_or_obligation_clock` | Check unchanged parser statistics, arena counters and clock at every leave; compare complete stores at representative page boundaries. |
| `compressed_topology_depth_and_empty_root_are_exact` | Unwind exactly the successful-edge count to depth zero, then verify first root leave and repeated underflow. |
| `compression_preserves_root_leave_underflow_and_finish_error_priority` | Preserve pending-obligation error priority, incomplete topology, exact source and root-leave behavior. |
| `returned_edge_error_preserves_its_exact_source_stores_current_and_history` | Pending-edge refusal preserves the complete current handle, stores and restore records; compare the exact error kind and physical source. |
| `successful_edges_publish_exactly_the_previous_not_the_returned_state` | Final payload edges save their interior predecessor even when the returned state is pending; private-operation successor checkpoints delimit later runs. |
| `root_left_refuses_an_edge_without_touching_stores_current_or_history` | Edge-after-root-leave returns the existing source-specific error with unchanged stores, state and history. |
| `a_pop_inspects_one_run_and_performs_exactly_zero_or_one_prefix_slice` | Inspect the native leave implementation for one top-record access and at most one constant-size slice; verify zero arena/checkpoint allocation. |
| `remembering_touches_only_the_latest_run_and_adds_at_most_one_record` | Per-edge compressed record growth is zero or one; no expansion, replay or ancestor walk enters publication. |
| `a_nonsingleton_run_has_a_safe_strict_predecessor` | Every nonterminal run pop uses a nonzero depth and remains above its first depth; native arithmetic cannot underflow. |
| `every_run_is_a_nonempty_block_of_one_exact_state` | Scalar undo oracle groups maximal adjacent equal state IDs; each corresponding native record represents at least one predecessor. |
| `remembering_preserves_maximal_adjacent_state_runs` | After every original or divergent edge, compare actual record count with the scalar oracle's exact adjacent-state group count. |
| `popping_preserves_maximal_adjacent_state_runs` | After every partial or complete unwind, compare the remaining maximal run count and exact restored observations. |

The exact maximal-run invariant requires that nonempty runs contain one
state ID, adjacent runs have distinct IDs, and remember/pop preserve that
property. The independent scalar-history test counts adjacent state-ID groups
directly. Its preactivation failure (four scalar records versus three runs)
demonstrates that it detects the missing compression; it is not passing runtime
evidence. Source-region composition and the complete ACT transaction remain
separate work. Run descriptors add one native depth word compared with a scalar
handle; singleton-heavy histories therefore have a larger per-record constant.
Maximal predecessor runs remove the per-byte factor only where state IDs repeat.
Native vector growth remains amortized allocation, and vector capacity remains
retained during unwind. Retained-storage reporting must use the actual run
descriptor size rather than the old scalar-handle size.

Runtime qualification is now recorded in journal J-120: 260 closed assumption
checks and independent kernel checking precede activation. All 425 model-library
tests pass, and the integrated gate passes 122 focused model tests plus six ACT
tests. The frame-storage subset has 32 tests, including maximal-run, long
private/fault/backtracking and located-span oracle controls. The preactivation
run-count failure and the activated oracle's raw-descriptor mismatch remain in
the evidence record. Cross-arena equality now compares all located payloads,
metadata and normalized graph fields; physical same-arena preservation remains
an exact separate assertion.

## Balanced source concatenation invariant catalogue (2026-09-10; task 6018)

`BalancedSourceComposition` specifies a concrete persistent 2–3 tree. A terminal
leaf is a checked, nonempty window into an original source. Internal nodes have
two or three equal-height children and cached logical length and height. These
are private-constructor invariants, not validation performed by concatenation.
Empty data roots are represented separately; endpoints are not tree payload.

Joining descends only the facing boundary of the taller tree. A result contains
one tree or two equal-height peers. A two-child parent absorbs the result;
a three-child parent can split into two two-child peers. Finalization creates at
most one new root. Untouched children must be retained as immutable references,
not recursively cloned or reconstructed from equal bytes.

Let $`d`$ be the absolute input-height difference and $`S`$ the number of
logical terminal occurrences. The model proves exactly $`d+1`$ boundary visits,
at most $`2d+1`$ new joins including finalization, and
$`2^{\mathrm{height}} \le S`$. These bounds do not count allocator internals,
reference-count synchronization, registry growth, cursor navigation or release.
Unique shared allocations and logical terminal occurrences are different metrics.

### Exhaustive exported invariants and required executable controls

The controls below are required bindings, not claims that the new runtime or
its tests already exist. The existing page-backed implementation remains the
independent located-byte and historical-window oracle.

| Exported invariant | Required executable control |
|---|---|
| `right_spine_rebuilding_keeps_the_exact_other_children` | Append to a taller two-child root; untouched left child retains pointer identity and full metadata. |
| `right_three_spine_rebuilding_keeps_the_exact_other_children` | Append through a three-child boundary; retain both off-spine children exactly, including overflow propagation. |
| `left_spine_rebuilding_keeps_the_exact_other_children` | Prepend through a two-child root; untouched right child retains pointer identity and metadata. |
| `left_three_spine_rebuilding_keeps_the_exact_other_children` | Prepend through a three-child root; retain both off-spine children and preserve their order. |
| `concrete_right_join_preserves_balance_caches_terminals_and_boundary_work` | Generated unequal-height right joins compare complete located output with the scalar oracle, validate every cache/rank and count actual visits/rebuilds. |
| `concrete_left_join_preserves_balance_caches_terminals_and_boundary_work` | Symmetric generated left joins exercise every One/Two absorption and overflow case with the same exact controls. |
| `concrete_concatenation_has_exact_caches_balance_order_and_boundary_cost` | Cover both orientations and equal heights; final root adds at most one level, preserves full order and meets the exact visit/new-join bounds. |
| `checked_concatenation_has_only_the_exact_native_length_preflight` | Use shared doubling DAGs to reach native logical-length boundaries with only word-width-many unique nodes; exercise the actual checked-addition guard, reject before publication and preserve all prior roots. Do not enumerate exponentially many logical bytes. No arbitrary height cap. |
| `checked_concatenation_preserves_all_native_cached_lengths_and_heights` | Inspect every reachable node after generated joins; cached sums/heights agree with the independent oracle and remain representable. |
| `balanced_height_has_a_concrete_exponential_terminal_lower_bound` | Count logical terminal occurrences, including shared occurrences, and check the height bound on long append/prepend and irregular forest traces. |
| `actual_join_preserves_arbitrary_located_bytes_and_valid_original_sources` | Compare byte values, source IDs and offsets for discontinuous sources and ACT-derived logical bytes that differ from the byte at their reported physical offset. |
| `checked_terminal_leaf_retains_the_exact_original_window` | Accept precisely nonempty, source-owned, in-bounds, representable terminal ranges; reject wrong owner, overflowing/out-of-bounds windows and zero-width leaves. Native start/end conversion must also reject reversed ranges before constructing the modeled window. Empty roots use the separate representation. |
| `checked_terminal_leaf_has_exact_bytes_and_representable_metadata` | Compare each accepted leaf's original backing/range, located bytes, length and rank with the existing source-window oracle. Physical provenance validity remains the source constructor's obligation. |
| `concatenation_reuses_exact_input_subtree_references_with_the_counted_new_joins` | Keep both input roots alive, check actual off-spine reference identity and new-node counts, and reject a copying implementation even when bytes agree. |
| `empty_data_roots_are_checked_identities_without_selecting_an_endpoint` | Check empty/empty, empty/nonempty and nonempty/empty data identities independently from right-endpoint selection and absent-versus-present-empty metadata. |
| `publishing_a_composed_root_preserves_every_exact_historical_root` | Retain earlier root handles across publication, source registration and later sibling construction; all select the same original trees and bytes. |
| `root_publication_returns_the_exact_composed_tree_without_flattening` | New root lookup selects the exact assembled root; separately count payload copies, source enumeration and registry allocations. The logical list model is not a constant-time publication proof. |
| `retaining_a_terminal_source_preserves_each_historical_window` | Register more borrowed/frozen sources and release originating arena owners; retained historical windows preserve contents and provenance. |
| `preserved_original_sources_preserve_every_historical_root_denotation` | Compare every retained root before and after source-store growth, including equal-content but distinct source identities and discontinuous provenance. |

### Interpretation and remaining bindings

Native code must use checked addition before allocation/publication: model
natural numbers do not overflow. Concatenation assumes balanced trees with valid
caches and sources; its total-size guard must not be advertised as a validator
for arbitrary forged trees. Terminal window bounds do not independently prove
the representability of physical source origins or offsets.

`reference_assembly` distinguishes retained input subtree values from new join
constructors. Its construction witness must be bound to actual immutable child
references and allocation counters. It is not a Rust pointer-address or allocator
proof. Append-only root/source publication preserves logical selection; its list
specification does not prove the cost of the runtime registry.

Before activation, subsequent models and tests must cover checked range
splitting and splice normalization, explicit seam/end anchors, iterative
continuation-fold correspondence, cursor/zipper seek and traversal, source-aware
append/freeze, and iterative destruction. The runtime must not wrap a composed
source as another terminal leaf, causing hidden ownership/delegation chains.
Literal windows and genuinely generated located bytes remain distinct.

Whole-import acceptance also requires the checked bulk forward/reverse ACT
transport, retained provenance and hash-range cursors, and separate descriptor,
source-read and validation-bitmap counters. These concatenation laws alone do
not prove the J-121 import slope, release safety or production ACT/LMDB parity.

### Proof checkpoint qualification

Full compilation reports 279 closed-context checks (260 existing plus 19 new),
and independent kernel checking succeeds. Compilation took 159.524 seconds
wall time and 29.843 seconds CPU, with a 1G peak; the kernel check took
20.210 seconds wall time and 19.980 seconds CPU, with a 486M peak. Both ran
under MemoryHigh=1G, MemoryMax=2G, MemorySwapMax=0, CPUQuota=100%, TasksMax=64
and a 900-second deadline. Compilation was memory-high throttled during
artifact finalization; early printed proof results were not treated as completion.
No new Rust implementation or runtime test result is claimed by this checkpoint.
See journal J-123 for source/log hashes and retained diagnostic failures.

## Balanced source concat driver invariant catalogue (2026-09-10; task 6018)

`BalancedSourceConcatDriver` refines the previous join into explicit control
states and a LIFO continuation stack. `Right2`/`Right3` retain the untouched
left siblings during right descent; `Left2`/`Left3` retain the untouched
right siblings during left descent. Reaching equal heights starts a return
phase. Each return step pops exactly one frame and applies its matching
absorption rule. The empty return stack terminates the machine.

The recursive `seek` and `resume` definitions are proof specifications.
The separate `step`/execution relation is the intended Rust loop binding;
Rocq structural recursion is not a claim that Rust optimizes tail calls.

| Exported invariant | Required executable control |
|---|---|
| `continuation_pop_applies_exactly_the_selected_absorption` | Construct all four frame variants with One/Two returns; check child order, result metadata and counter increments against the recursive oracle. |
| `right_descent_and_pop_equal_the_recursive_join_with_every_counter` | Generated right-facing descent with nonempty saved outer frames; compare complete output, untouched references and visit/rebuild counts. |
| `left_descent_and_pop_equal_the_recursive_join_with_every_counter` | Symmetric left-facing descent with outer continuations; reject wrong-side absorption and swapped siblings. |
| `explicit_stack_driver_returns_the_exact_recursive_pieces_visits_and_allocations` | Compare the iterative driver with the recursive test-only join over both orientations, equal heights, carry propagation and repeated shared children. Count reconstructed nodes rather than allocator internals. |
| `explicit_stack_driver_preserves_the_exact_native_preflight_and_published_tree` | Exercise representable totals and actual checked-add overflow before publication; old roots and source owners remain unchanged on refusal. |
| `right_descent_stores_exactly_the_facing_height_gap` | Record the actual maximal right-facing frame count and compare it with the cached input-height difference, plus any saved outer frames. |
| `left_descent_stores_exactly_the_facing_height_gap` | Symmetric left-facing stack-depth counter, including equal-height zero-frame joins. |
| `explicit_concat_loop_terminates_without_any_fuel_or_depth_cap` | Execute the exact transition loop to its empty returning stack; require no semantic depth limit and compare its final join result with the oracle. |
| `every_reachable_loop_state_uses_at_most_the_original_height_gap_frames` | Instrument every push/pop transition, including carries and early boundary stops; maximum live frames must not exceed the original height gap. Check all prefixes, not only the final stack. |
| `balanced_concat_loop_finishes_in_exactly_twice_the_height_gap_plus_one_steps` | Count successful control transitions; require exactly twice the input-height difference plus one, with an empty returning stack. Count the terminal None observation separately. |

Native implementation must store immutable child references in frames rather
than structurally cloning subtree values. Node-owned iterative destruction is
a separate obligation: construction correspondence alone does not protect
root, cursor, intermediate or concurrently released child ownership.

### Driver proof qualification

The module adds 10 exported capstones. Full compilation passes 289 closed-context
checks; independent kernel checking succeeds. Compilation took 234.955 seconds
wall and 30.944 seconds CPU, with a 1.1G peak and zero swap. Kernel checking took
21.280 seconds wall and 20.915 seconds CPU, with a 487.4M peak and zero swap.
Both used MemoryHigh=1G, MemoryMax=2G, MemorySwapMax=0, CPUQuota=100%,
TasksMax=64 and RuntimeMaxSec=900. The soft threshold throttled compiler artifact
finalization. The subsequent gate configuration raises only MemoryHigh to 1536M;
the 2G hard ceiling and zero-swap policy remain unchanged. That new threshold was
not used for these recorded measurements. J-124 records exact hashes and failed
diagnostic history. No Rust activation or fresh runtime benchmark is claimed.

## Checked balanced source split invariant catalogue (2026-09-10; task 6018)

`CheckedBalancedSourceSplit` specifies recursive splitting of the same immutable
2–3 trees into optional left and right roots. Zero and full-length cuts retain
the exact original tree on the nonempty side. Strict interior leaf cuts create
two checked windows into the original source. Internal cuts descend through
exactly the child selected by cached prefix lengths, then attach the untouched
siblings in their original order. This is a recursive proof prerequisite, not
an implemented iterative split driver or Rust acceptance result.

The one-sibling helpers `gr1` and `gl1` attach a right or left sibling; an absent
partial root returns that sibling unchanged. With a present partial root they
finalize the corresponding concatenation. The two-sibling helpers `gr2` and
`gl2` return a two-child node for an absent partial root. Otherwise they absorb
the second sibling into the unfinalized concatenation result, then finalize
once. Finishing the intermediate concatenation is a different algorithm and
does not satisfy the proved per-glue work potential.

Let $`H`$ be the input subtree height, $`p`$ the partial-root height (zero for
an absent root), $`h`$ the sibling height, and $`q`$ the glued result height.
The glue precondition is $`p \le h`$; its result satisfies
$`h \le q \le h+1`$, with $`q=h+1`$ for two siblings. Glue visits plus $`p`$
are at most $`q+1`$; rebuilt nodes plus $`2p`$ are at most $`2q+1`$.
For output heights $`L`$ and $`R`$, both at most $`H`$, the recursive proof
telescopes to the following bounds, including two new terminal windows for a
strict interior leaf cut:

```math
\begin{aligned}
\mathrm{visits} &\le 3H+1+L+R \le 5H+1, \\
\mathrm{rebuilt} &\le 2H+2+2(L+R) \le 6H+2.
\end{aligned}
```

These are concrete model-operation counts, not allocator measurements or a
whole-import complexity claim. The existing balance theorem bounds height by
logical terminal occurrences, not by the number of unique shared allocations.

### Exhaustive exported invariants and required executable controls

Every row below is a required native binding, not a report of implemented tests.
Located-byte comparisons include byte values, source identities and offsets.
`legal_window` requires original-source validity, an allowed original owner and
a representable absolute terminal end. `legal_root` additionally requires
balance and valid caches. These private-constructor invariants are preconditions;
the split entry point does not validate arbitrary forged trees.

| Exported invariant | Required executable control |
|---|---|
| `one_right_sibling_glue_preserves_order_balance_caches_and_height_potential` | Exercise absent partial-root identity and present partial roots of equal/lower height; compare located order, caches, result height and per-glue counters, and retain the untouched right sibling's actual reference identity. |
| `one_left_sibling_glue_preserves_order_balance_caches_and_height_potential` | Mirror the one-right controls; reject reversed concatenation or swapped children even when lengths agree. |
| `two_right_siblings_glue_preserves_order_without_finishing_the_intermediate_join` | Exercise absent partial roots and both One/Two concatenation results through `absorb_left2`; retain both siblings in order, check result height and counters, and require an intermediate-finalization mutant to fail the work potential. |
| `two_left_siblings_glue_preserves_order_without_finishing_the_intermediate_join` | Mirror the two-right cases through `absorb_right2`, including carry propagation and exact untouched reference identity; do not finalize the intermediate join. |
| `out_of_range_split_cuts_are_refused_before_descent` | Reject cuts beyond cached size before child descent, window creation or root publication; preserve inputs and stores. Distinguish this range refusal from failure on an invalid terminal descriptor. |
| `zero_and_full_tree_cuts_retain_the_exact_original_root` | Check actual root pointer identity at both endpoints, one visit and no new leaf/join; test endpoint metadata separately from the optional data root. |
| `checked_terminal_construction_and_concatenation_preserve_representable_source_windows` | Check that checked leaf construction and concatenation preserve every absolute terminal end within the native limit, including near-limit offsets and checked-add refusal. Total cached width alone must not substitute for this invariant. |
| `checked_nonempty_subwindow_preserves_legality_and_original_source` | Split strict leaf interiors into positive-width windows with the exact owner, normalized absolute ranges and located bytes; cover repeated slicing and borrowed, frozen and generated-byte sources, including native range checks. |
| `split_node2_interior_equation` | Cut before, at and after the child boundary; descend only the selected child, subtract exactly the first child's size for right descent, select `gr1`/`gl1` correctly and retain the untouched sibling reference. |
| `split_node3_interior_equation` | Cover all three child interiors and both exact boundaries; select the correct one/two-sibling glue and subtract both preceding lengths for last-child descent. Check untouched order and reject intermediate finalization. |
| `concrete_checked_split_is_total_preserves_sources_and_telescopes_both_output_heights` | Exhaust small valid shapes and every cut, then selected cuts of tall shared trees; all in-range cuts must return legal balanced outputs with exact caches and both output-height potentials. Do not weaken the input terminal-bound precondition. |
| `split_reconstruction_is_the_exact_located_firstn_and_skipn` | Compare both outputs with an independent eager located-byte prefix/suffix oracle at every cut; include repeated, reversed and noncontiguous sources and ACT-derived values differing from their reported physical backing bytes. |
| `both_output_height_potentials_give_the_linear_split_work_bound` | Count descent/glue visits and new joins/windows separately against the stated bounds, including two windows for an interior leaf cut. Test tall shared trees without enumerating their logical sequence and reject the intermediate-finalization mutant. |
| `checked_split_outputs_preserve_all_native_lengths_heights_and_terminal_bounds` | Validate every output subtree's cached length/height and every terminal's absolute range under a representable input total. Use valid shared doubling trees for native word-boundary controls, not fabricated caches. |
| `checked_optional_split_is_total_exact_and_linearly_bounded` | Check absent-root cut zero, nonzero refusal, and present-root endpoint/interior cuts; compare exact sizes, located bytes, heights, legal sources, native metadata and work counts. Keep absent data distinct from endpoint metadata. |
| `valid_source_windows_and_terminal_bounds_identify_only_the_original_backings` | Require output owners to come from actual input terminal owners, with exact retained source values. Reject composed-source-as-leaf wrappers and descriptor chains; bind borrowed/frozen source retention separately to native ownership. |
| `valid_cached_metadata_does_not_substitute_for_terminal_end_bounds` | Preserve the negative control: native limit ten, terminal start eleven and width two, backing length thirteen. Source validity and cached length/height fit, yet cut one fails. Add near-native-limit equivalents to reject a missing absolute-end invariant. |
| `publishing_both_optional_split_outputs_preserves_every_exact_historical_root` | Publish either, both or neither optional output while every prior root/range retains exact identity and located observations; cover freeze, reappend and forks. Check actual immutable references separately from the logical publication model. |

### Interpretation and remaining bindings

The proof's terminal and denotation lists are reference semantics, not a native
flattening algorithm. Untouched subtree values must bind to immutable reference
clones, not recursive structural clones. Historical root selection through
append-only publication does not prove constant-time registry publication,
pointer-address identity, source lifetime or iterative release.

Global endpoint anchors remain a separate obligation. The left output endpoint
must be `original.position_at(cut)` and the right endpoint must be
`original.end`; a preceding leaf's endpoint is not a substitute at source seams
or explicit completion overrides. Empty data does not imply absent endpoint
metadata. Checked terminal-subwindow anchor laws do not establish this composed
root binding by themselves.

Iterative split descent/fold correspondence, actual Rust split and ownership
bindings, cursor traversal, release/Drop safety and bulk ACT transport remain
separate prerequisites. This checkpoint neither activates the balanced backend
nor claims importer scaling or production acceptance.

### Split proof qualification

The module adds 18 exported capstones. Full compilation passes 307 closed-context
checks (289 existing plus 18 new); independent kernel checking succeeds.
Compilation took 34.038 seconds wall and 33.692 seconds CPU, with a 1.1G peak
and zero swap. Kernel checking took 21.179 seconds wall and 20.892 seconds CPU,
with a 480.9M peak and zero swap. Both used MemoryHigh=1536M, MemoryMax=2G,
MemorySwapMax=0, CPUQuota=100%, TasksMax=64 and RuntimeMaxSec=900.
Durable logs are `target/verification/epathmap-balanced-split-proof-final.log`
and `target/verification/epathmap-balanced-split-kernel-final.log`; failed and
successful isolated diagnostics remain alongside them. These checks cover the
recursive model, not a native split implementation.

## Canonical source release ownership invariant catalogue (2026-09-10; task 6018)

Run `scripts/check-stack-safe-pda-proofs.sh --canonical-source-release` from the
repository root to check this unit independently. Its placeholder scan, exact
31-report compiler check and independent kernel check passed for the initial
ownership prerequisite. The single-drain checkpoint requires 48 reports: those original
31 exports plus the 17 phased-loop bridge bindings catalogued below. The current
gate requires 68 reports after the 20 concurrent bindings catalogued later. The bridge's
48-report compilation and independent kernel check pass. The gate retains a
512M soft/1G hard memory cap with swap disabled. The full and focused
canonical-key routes invoke the same helper; standalone unit evidence does not
requalify those larger routes.

[`CanonicalSourceRelease`](theories/CanonicalSourceRelease.v) specifies one
phased drain of counted ownership tokens. This is a bounded ownership-model
prerequisite for the balanced source tree, not qualification of a native
destructor. External node owners and external source readers are fixed during
each modeled execution; concurrent clone/drop transitions are not present.
The intended native graph is finite, immutable and acyclic, with no node-ID
reuse, at most three child occurrences per node and strictly descending child
ranks. The ranked-list results take the fanout and rank conditions explicitly;
the subsequent phased-loop bridge connects them to the model's heap and complete
single drain. Native and concurrent ownership bindings remain required.

Node-reference counts include external roots, pending work, children still in
the extracted local shell, detached children awaiting enqueue, and child slots
of live heap nodes. Owned-source counts likewise include external readers and
every owned lease in the heap, local shell or detached payload. Repeated child
IDs and repeated owned-source IDs are allowed and counted by occurrence: two
leases on one backing are not one lease. Borrowed source identifiers do not
constitute owned leases; their native lifetime binding remains separate.

The local phase invariant requires `Ready` to have no shell or staged payload,
`Extracted` to have a full shell but no staged payload, and `Evacuated` to have
no shell. Final extraction clears the heap slot while retaining its complete
payload locally. Detachment empties the shell before child releases can begin.
Child staging transfers one occurrence at a time to a LIFO worklist; source
release records an owned-lease occurrence, not disposal of the entire backing.
An empty-shell drop then returns to `Ready` without changing ownership.

### Initial ownership exports and required executable controls

The following 31 names preserve the initial ownership assumption-check catalogue.
The later bridge adds 17 bindings without replacing these results. Each row
specifies a required native or property-test binding, not an implemented-test
claim. Trace preservation requires the stated initial invariants: unique claims
with cleared slots, valid phases, or positive reference counts naming live heap
slots, as applicable. Finite-trace theorems do not assert termination, eventual
release of every node, or complete reclamation of initially unreachable nodes.

| Exported invariant | Required executable control |
|---|---|
| `clearing_a_node_empties_exactly_its_original_slot` | Final-claim a live node and inspect its exact slot before any child release; reject a mutant clearing a sibling or leaving the payload installed. |
| `clearing_a_node_preserves_every_other_complete_node` | Snapshot every other live node's full payload and identity across a final claim, including shared diamonds and repeated child references. |
| `extraction_consumes_exactly_one_reference_and_preserves_all_other_counts` | From a valid idle state, compare occurrence counts before and after both final and nonfinal extraction. Include duplicate child IDs and held external roots; reject deduplication and double-decrement mutants. |
| `final_extraction_keeps_child_occurrences_in_the_full_local_shell` | Pause immediately after a final claim and check the cleared heap slot, complete retained local payload, empty staged children and `Extracted` phase. No child token may disappear between heap removal and detachment. |
| `nonfinal_extraction_keeps_the_heap_external_owners_and_release_ledgers` | Retain another owner, take the nonfinal path and compare the complete heap, external-owner lists and both ledgers. Permit the consumed pending token to change; do not assert whole-state identity. |
| `detaching_moves_every_owned_child_without_changing_reference_counts` | Starting with empty staging, detach a two/three-child payload and an owned leaf payload; check exact node/source counts and an empty shell. A premature child drop must fail. |
| `enqueueing_transfers_one_occurrence_without_changing_ownership` | Pause after each child transfer and count pending plus staged ownership together, including repeated IDs. Reject copying a token or dropping it before enqueue. |
| `releasing_a_source_preserves_nodes_and_accounts_for_exactly_its_lease` | Test borrowed and owned leaves separately; owned release decreases its live lease count and adds exactly one matching ledger occurrence, while node counts are unchanged. Shared backing IDs must remain distinguishable by multiplicity. |
| `empty_shell_drop_is_work_and_ownership_neutral` | Instrument the evacuated empty-shell return: pending work, heap, ownership counts and ledgers stay unchanged and the phase becomes idle. Populated-shell entry is a separate rejection/control case. |
| `a_surviving_external_reader_keeps_its_source_reference_count_positive` | Keep an external frozen-source reader while draining all releasable tree nodes; the counted source ownership must remain positive. This is a positivity control, not a backing-destructor test. |
| `a_cleared_node_cannot_be_extracted_again` | Construct a deliberately invalid pending token naming a cleared slot and require extraction refusal without another claim; distinguish this local refusal from progress on valid states. |
| `every_successful_step_preserves_the_local_ownership_phase` | Enumerate valid phase states and successful transitions, checking empty staging at `Ready`/`Extracted` and the absent shell at `Evacuated`. Reject stale-local and overlapping-payload mutants. |
| `extraction_preserves_every_original_source_lease` | Pause final and nonfinal extraction of owned leaves and compare every source's live occurrence count, including the retained local shell. Reject omitting local leases from the accounting oracle. |
| `no_backing_disposal_is_admissible_while_an_external_reader_survives` | Check that the model's zero-source-reference disposal guard rejects a backing with an external reader. Bind the guard to an actual backing-disposal event separately before claiming native reader safety. |
| `detaching_preserves_the_combined_local_and_queued_frontier` | Count child occurrences in the local shell, staging and pending work before and after detachment. This theorem controls their total size, not their list order. |
| `enqueueing_preserves_the_combined_local_and_queued_frontier` | Check that each staged-to-pending transfer preserves the combined occurrence count, including intermediate staging states and duplicate children. |
| `completed_child_staging_is_exactly_reverse_children_followed_by_old_work` | With distinct ordered children and existing pending work, finish staging and compare the exact list with reversed children followed by old work. Reject FIFO or accidental double-reversal mutants. |
| `each_ranked_lifo_frontier_has_at_most_one_plus_twice_root_height` | Generate frontiers satisfying the ranked invariant and check the stated size bound, including rank-zero and maximal three-child boundaries. Actual loop high-water measurements require the remaining realized-frontier binding. |
| `final_lifo_extraction_preserves_the_ranked_frontier` | Replace a parent head by its reversed children under fanout at most three and strict rank descent; test repeated children and saved siblings. Nondecreasing ranks and excess fanout must be rejected as invalid inputs. |
| `nonfinal_lifo_extraction_preserves_the_ranked_frontier` | Remove a nonfinal head from valid ranked frontiers and check the unchanged saved-sibling suffix and frontier predicate. |
| `every_final_extraction_strictly_decreases_rank_debt` | For bounded test ranks, compare the mathematical sum of base-four rank weights before and after a final macro replacement, including three children of maximal allowed rank. Do not compute this proof measure with unchecked native arithmetic. |
| `every_nonfinal_extraction_strictly_decreases_rank_debt` | Remove a nonfinal head and check strict debt decrease even at rank zero. This extraction-level control does not establish progress through staging phases. |
| `a_successful_step_only_preserves_storage_or_claims_one_live_slot` | Classify every successful event: heap and claim ledger are unchanged, or exactly one previously live slot is cleared and its ID prepended to the claim ledger. Compare complete storage rather than counts alone. |
| `every_successful_step_preserves_all_previously_cleared_slots` | Carry cleared-slot sentinels through every transition and reject resurrection or ID-reuse mutants, including after unrelated final claims. |
| `every_successful_step_preserves_unique_claims_and_their_empty_slots` | Start with a valid nonempty claim history and check unique claimed IDs plus empty corresponding slots after each step. A duplicate-claim ledger mutant must fail. |
| `every_finite_release_trace_claims_each_node_at_most_once` | Exhaust short valid traces and run longer shared-DAG traces; check the complete claim ledger, including preexisting claims, for uniqueness and cleared slots. Do not require every reachable node to be claimed while external owners remain. |
| `extraction_preserves_all_live_node_references` | From valid idle ownership, exercise final and nonfinal extraction and require every remaining positive node-reference count to name a live slot. Cover shared diamonds and retained external roots. |
| `every_successful_step_preserves_all_live_node_references` | Check the same live-slot implication after each phase transition, with shell and staged tokens included in the independent count oracle. Reject early slot clearing and lost-local-token mutants. |
| `every_finite_release_trace_preserves_live_reference_validity` | Run finite traces from valid phases and live references, checking both final invariants. Snapshot retained live payloads separately; the theorem states reference validity, not a full native observation-equivalence result. |
| `every_successful_step_conserves_owned_source_leases_with_release_occurrences` | For every owner ID and successful phase step, compare live owned leases plus released-ledger occurrences. Include two leaves retaining one backing and a nonempty initial release ledger. |
| `every_finite_release_trace_accounts_for_every_owned_source_lease` | Compare initial and final live-plus-released occurrence totals over finite traces with shared backings, borrowed leaves and retained readers. Reject both deduplicated release ledgers and missing/double lease releases; do not require unique backing IDs in the ledger. |

### Native bindings and the subsequent full-loop refinement

The intended native pattern is node-owned `Drop` with an optional payload:
detach children before draining them through `Arc::into_inner`, then let the
extracted empty shell drop. Ordinary external root/cursor `Arc` tokens need not
each gain a wrapper when every recursively owning node has the protected
destructor. That pattern is not implemented or verified by this catalogue.

The phased-loop bridge below now connects the actual state to the ordered
frontier `rev(local_shell.children) ++ rev(staged_children) ++ pending`, retains
exact original heap and extracted payloads, and proves a decreasing whole-drain
measure and progress on valid states. Its bound covers every modeled state
reachable from an idle single-root start, including intermediate staging; it
does not automatically apply to an arbitrary multi-root initial queue or prove
the native destructor's stack bound.

Ordinary final-root `Arc` drop and final `Arc::into_inner` extraction need
distinct native control bindings, even when they share final-owner election.
The nonfinal extraction path must consume its token without invoking a
populated node destructor. Required native controls include simultaneous final
releases, shared diamonds, retained cursor/root/source owners, empty-shell
reentry, and a causal `try_unwrap(...).ok()` race mutant; substituting that
operation for `into_inner` does not establish the required contract. A model of
finite concurrent drains must account for every drain's local tokens and
permitted environment transfers. No quiescence claim applies to infinite
external work, and no standard-library formal verification is claimed here.

Leaf owners are the original borrowed/frozen sources with nonrecursive backing
ownership, not arbitrary user-defined destructor callbacks. Native lifetime,
backing-disposal, allocation and unwind behavior remain separate bindings.
Normal allocation failure that aborts must not be conflated with arithmetic
panic or unwinding; neither allocator behavior nor panic safety follows from
the ownership conservation lemmas. This catalogue activates no balanced-source
backend and makes no whole-import performance or native Drop-completion claim.

## Canonical source release phased-loop bridge (2026-09-10; task 6018)

The bridge extends the same `CanonicalSourceRelease` state and transition
function. `retained_payloads` requires every live heap slot to contain its exact
original payload and every extracted shell to come from that original heap.
`original_graph_ranked` links each original node's stored rank to its immutable
ID, limits child occurrences to three and requires strict child-rank descent.
`drain_valid` combines retained payloads, valid local phases and the requirement
that every positive node-reference count names a live slot. These are explicit
input invariants, not a validation pass over arbitrary native memory.

The ordered realized frontier is
`rev(local_shell.children) ++ rev(staged_children) ++ pending`. Its length is
exactly `staging_size`, so children held between final extraction and enqueue
cannot escape the bound. Actual final extraction replaces the head with its
reversed child occurrences; nonfinal extraction removes only the head. Every
other successful transition preserves this ordered list and decreases local
phase work. For example, extracting a node with three repeated children leaves
three realized tokens even when the pending worklist is empty.

Let $`F(s)`$ be the realized frontier, $`r(i)`$ the immutable rank of node ID
$`i`$, and $`P(s)`$ the phase work. Phase work is zero in `Ready`, child count
plus two plus a possible source-release step in `Extracted`, and staged-child
count plus one plus a possible source-release step in `Evacuated`. The natural
termination measure is:

```math
D(F) = \sum_{i \in F} 4^{r(i)}, \qquad M(s) = 8D(F(s)) + P(s).
```

The sum counts occurrences, not distinct IDs. Final extraction decreases debt
by at least one while creating at most six units of phase work. Nonfinal
extraction decreases debt, and neutral staging decreases phase work without
changing debt. Thus every successful step decreases the whole measure. A valid
state has no successor exactly when it is idle with empty pending work.
Well-founded induction yields a completed finite execution without a supplied
fuel parameter; every finite trace's length plus its final measure is bounded
by its initial measure.

For any state reachable from a valid idle start with one pending root, all local,
staged and pending child tokens together number at most
$`1 + 2r(\mathrm{root})`$. These are abstract occurrence and proof-measure bounds,
not native call-stack measurements or a practical exponential runtime counter.
Neither termination nor the frame bound asserts reclamation of initially
unreachable heap nodes or nodes retained by external owners.

### Exhaustive bridge exports and executable control bindings

These 17 new exports comprise 13 bridge theorems and four checked concrete model
controls. Native/property controls in the table remain required; the four
explicitly identified model controls are already proved, not native tests.
Together with the preceding historical catalogue, this accounts for all 48
single-drain assumption reports exactly once.

| Exported invariant or model control | Executable control binding |
|---|---|
| `realized_frontier_counts_every_local_staged_and_pending_occurrence` | Independently count shell, staged and pending tokens after each transition, including repeated IDs; reject a pending-only oracle. |
| `every_successful_step_retains_exact_original_heap_and_shell_payloads` | Compare complete retained payloads and original identities after every event; reject slot mutation, source replacement and shell substitution. |
| `original_heap_ranks_and_fanout_apply_to_every_retained_live_node` | Bind actual node IDs and stored ranks to the original immutable graph; check fanout and descending child ranks without deduplicating repeated children. |
| `actual_final_extraction_replaces_the_realized_head_with_exact_reversed_children` | Pause immediately after final extraction and compare the exact reversed child occurrences followed by saved work, before any enqueue. |
| `actual_nonfinal_extraction_removes_only_the_realized_head` | Retain another owner and check that only the selected head token disappears, the suffix remains exact and both phases have zero work. |
| `every_actual_staging_step_preserves_frontier_order_and_decreases_phase_work` | Pause detachment, each enqueue, source release and empty-shell drop; reject FIFO, double reversal and stale staged-token mutants. |
| `every_actual_step_preserves_the_ranked_realized_frontier` | Check every event kind against the ranked frontier invariant, including repeated children, saved siblings and intermediate staging. |
| `every_actual_ranked_step_strictly_decreases_the_whole_drain_measure` | On small bounded-rank fixtures, calculate the mathematical measure independently and require strict decrease after every successful step; do not add an unchecked exponential native counter. |
| `a_valid_state_stops_exactly_when_idle_and_work_is_empty` | Test valid empty and nonempty states in all phases; reject premature completion and a stuck valid state. Keep invalid cleared-slot refusal outside the progress precondition. |
| `every_valid_ranked_drain_reaches_idle_empty_work_without_a_fuel_parameter` | Drain finite valid shared graphs without a user-supplied depth/fuel limit, including retained external owners; completion means idle empty work, not that every heap node was reclaimed. |
| `every_finite_actual_trace_preserves_original_payloads_and_ranked_frontier` | Check complete original-payload retention and ranked frontier after every trace prefix, including mid-detachment and mid-enqueue states. |
| `every_reachable_single_root_state_bounds_all_local_staged_and_pending_frames` | Measure the combined token high-water mark from a valid idle single-root start. Count local shell children even with empty pending work; do not generalize the bound to arbitrary root batches. |
| `every_actual_trace_length_is_bounded_by_the_initial_whole_drain_measure` | Compare trace length plus final measure with initial measure on bounded fixtures, including zero-step traces and prefixes ending during staging. |
| `repeated_child_control_has_valid_original_ranks_and_initial_ownership` | Checked model control: a rank-one root with three references to one owned leaf satisfies the original-rank and initial-ownership invariants. Mirror it natively to keep the shared-graph fixture nonvacuous. |
| `repeated_child_control_executes_every_token_once_and_keeps_the_external_source_reader` | Checked model control: twelve exact transitions include two nonfinal child extractions, one final child extraction and one owned-lease release while the external reader survives. Mirror the exact claims, heap and lease ledger natively. |
| `final_extraction_control_counts_children_before_the_first_enqueue` | Checked model control: final extraction leaves three repeated child tokens in the local shell, a realized frontier of length three and pending length zero. A pending-only bound must fail this control. |
| `cyclic_child_control_cannot_satisfy_the_immutable_rank_precondition` | Checked negative model control: a self-child cannot satisfy strict immutable-rank descent. Reject cyclic or forged-rank native fixtures rather than invoking the termination theorem on them. |

### Bridge qualification and remaining scope

Full unit compilation passes 48 closed-context reports, and independent kernel
checking succeeds. Compilation took 5.821 seconds wall and 5.716 seconds CPU,
with a 465.1M peak and zero swap. Kernel checking took 13.098 seconds wall and
12.812 seconds CPU, with a 277.3M peak and zero swap. Both used
MemoryHigh=512M, MemoryMax=1G, MemorySwapMax=0, CPUQuota=100%, TasksMax=64 and
RuntimeMaxSec=120. Durable logs are
`target/verification/canonical-release-bridge-proof-final.log` and
`target/verification/canonical-release-bridge-kernel-final.log`.

Diagnostics 1 through 14 remain retained. Diagnostic 13's concrete trace proof
used implicit intermediate-state unification and hit the 120-second limit:
120.123 seconds service time, 2.796 seconds CPU and a 558.7M peak, with zero swap.
Diagnostic 14 supplied explicit small intermediate states for the same twelve
transitions and passed in 5.776 seconds wall and 5.701 seconds CPU, with a 469.6M
peak and zero swap. This proof-term change required no algorithm change or cap
increase; the final qualification reran the stable source.

External owners remain fixed during the proved single-drain traces. Ordinary
final-root `Arc` drop, the standard `Arc::into_inner` ownership contract and
finite concurrent drains still need their native/model bindings. Other drains'
held tokens may be accounted for as external ownership when relating an
individual step, but the fixed-external-owner trace theorem cannot simply be
applied across interfering releases or clones. Repeated source IDs remain
legal; source-lease occurrence conservation does not become uniqueness of
backing IDs or a verified physical backing-disposal implementation.

No standard-library formal verification, native destructor completion,
allocator/unwind qualification or importer performance result follows from
this checkpoint. The original single-drain algorithm is unchanged, and the
balanced-source backend is not activated by these proof or documentation edits.

## Representation-derived source frontier capacity (2026-09-10; task 6018)

`BalancedSourceNativeCapacity` is a corollary for the private balanced
2–3 source trees only. Every terminal window is nonempty. Therefore logical
terminal occurrences are no greater than the cached logical byte length,
even when subtrees share physical allocations. Together with the existing
exponential terminal lower bound, this gives:

```math
2^h \le \text{terminal occurrences} \le \text{cached bytes} < 2^w,
\qquad h < w.
```

Here $`h`$ is source-tree height and $`w`$ is the width of the native
length type in bits. A previously proved local frontier bound
$`1+2h`$ consequently fits $`1+2(w-1)`$ slots.

| Exported invariant/control | Required native/property binding |
|---|---|
| `representable_balanced_sources_have_height_below_the_native_word_width` | Bind private constructors' nonempty leaves, balanced children, exact cached lengths and checked overflow to the native word width. Exercise maximally shared repeated-subtree shapes and refuse the first unrepresentable length; do not treat physical allocation count as logical size. |
| `the_proved_local_frontier_fits_the_representation_derived_fixed_capacity` | Combine the actual single-root shell/staged/pending high-water observation with the source node's verified height. Assert every push fits before adopting a fixed buffer; retained/concurrent drains each require their own bound. |
| `native_frontier_capacity_is_127_slots_for_64_bit_lengths` | Verify the capacity expression on a 64-bit target and cover rank-zero, near-boundary shared trees and leaf-only teardown. |
| `native_frontier_capacity_is_63_slots_for_32_bit_lengths` | Verify the same expression on a 32-bit target or checked-width model; do not encode a platform-specific constant in persisted data. |

This is a representation-derived capacity, not a limit on Rholang AST depth,
input traversal or PathMap topology. It does not apply to the old unbalanced
page DAG or to arbitrary ranked release-model heaps. Native binding must
establish that the release node rank is the balanced source-tree height.

A fixed-capacity safe buffer could remove frontier allocation/reallocation.
It does not prove that backing disposal cannot allocate or unwind. Keep the
empty-shell/leaf fast return outside a separate frontier-owning helper:
a branch before array initialization alone does not guarantee the compiler
avoids reserving the array's stack frame. Benchmark initialization and
destruction overhead against the heap-backed candidate before adoption.
No buffer implementation or measured improvement is claimed here.

### Reuse boundaries for the native source controls

The existing test fixtures are not interchangeable ownership proofs. The
implementation must extend their present oracles, retaining their original
backends as differential controls:

| Existing source | Coverage to reuse | Additional composed-source obligation |
|---|---|---|
| `models/tests/support/indexed_span_transport.rs` | Generated anchored windows, nested subviews, physical provenance, byte versus end positions, rejected ranges, and children surviving parent teardown. | Run the same observations against composed roots and retained cursors after parent/sibling release; preserve backing identity without projecting the tree into copied leaves. |
| `models/tests/support/normalized_source_spans.rs` | Exact adjacency and backing-identity coalescing, empty-window anchors, full-width offsets, and byte/reader equivalence. | Add composed backing without weakening the flat borrowed/frozen controls or confusing equal contents with shared identity. |
| `models/tests/support/canonical_frame_storage.rs` | The 20,000-layer `persistent_owner_capture_forcing_and_drop_use_a_small_native_stack` control exercises arena-owned region frames. | This does not test the proposed reference-counted source-node DAG. Add its actual shared-child, simultaneous-release, surviving-reader and empty-shell controls separately. |

On the pinned 64-bit target, the existing layout controls require a 64-byte
mirrored production region header and a 72-byte actual test region (the latter
also includes test-only backing variants). They are not permission to grow
every production descriptor to the test-only maximum. Measure the new native
layout and preserve these distinctions when adding a composed variant.

The installed `nightly-2026-02-09` implementation in
`library/alloc/src/sync.rs` was inspected for the native ownership binding.
`Arc::into_inner` suppresses ordinary `Arc` destruction with `ManuallyDrop`,
atomically decrements the strong count, and returns the moved payload only
for the final decrement. Ordinary final `Arc::drop` instead invokes
`drop_in_place` through `drop_slow`. Consequently an extracted source node
must have its payload removed before its empty shell is destroyed; a final
external root drop must enter the populated node's protected destructor.
`try_unwrap(...).ok()` is not a substitute: failed extraction returns an
ordinary `Arc` whose later drop can become final. This is inspection of the
pinned standard-library contract, not formal verification of its internals
or execution of the still-required native concurrent controls.

## Concurrent canonical source release and native contract (2026-09-10; task 6018)

`ConcurrentCanonicalSourceRelease` adds a shared heap and claim/release ledgers,
fixed external owner lists, and a finite list of local drains. Projecting one
drain treats every other drain's current pending, shell and staged tokens as
external ownership. This reuses the existing single-step transition without
incorrectly applying its fixed-environment trace theorem across interference.
Repeated child and source identifiers are counted by occurrence.

The aggregate invariant preserves local phases, exact original immutable
payloads, positive-reference validity and unique final claims. A successful
step decreases a physical work measure by exactly one. For an idle initial
family, the resulting event bound is:

```math
\text{release events} \le R + 2E + 2N + L.
```

Here $`R`$ counts initial root tokens, $`E`$ counts child-slot occurrences
in physical live nodes, $`N`$ counts physical live nodes and $`L`$ counts
nonempty source-release actions. Shared subtrees are not unfolded. Every finite
trace has exact event count plus residual work equal to initial work; a
constructive schedule reaches idle empty drains without supplied fuel.
Quiescence does not imply that externally retained, orphaned or cyclic
unreclaimed heap entries disappear.

This work measure counts abstract release transitions, not implementation time
spent evaluating the model's list-based bookkeeping or searching for a runnable
drain. A source action is a lease release, not the complete cleanup of a flat
frozen backing's page-pair, leaf and index arrays. Native cost accounting must
include that final metadata traversal separately.

### Exhaustive concurrent exports and native controls

The following 20 exports extend the historical 48-export catalogue to 68.
These rows specify required executable bindings, not completed native tests.

| Exported invariant/control | Required native/property binding |
|---|---|
| `selected_projection_counts_every_other_drain_token_as_external_ownership` | Count every other drain's shell, staged and pending tokens by occurrence at each selected event, including duplicate child and source IDs. |
| `every_global_step_preserves_all_live_node_references` | Check every remaining positive aggregate node count names a live slot, including nodes held in another drain's local shell. |
| `every_global_step_conserves_owned_source_leases_across_all_drains` | Compare each source's live lease occurrences plus released occurrences across every interleaved event; never deduplicate backing IDs. |
| `every_global_step_preserves_unique_claims_and_all_cleared_slots` | Check one final claim per physical allocation and no resurrection of any previously cleared slot. |
| `every_global_step_preserves_the_complete_aggregate_invariant` | Check valid local phases, original payload retention, live references and unique claims together after every controlled event. |
| `each_local_release_step_consumes_exactly_one_unit_of_physical_work` | Calculate the physical heap and local-phase measure independently on small fixtures; each actual event must consume exactly one unit. |
| `every_global_step_consumes_exactly_one_unit_of_total_physical_work` | Interleave paused drains and count the shared heap only once, plus every drain's local work; require exact unit decrease. |
| `every_finite_interleaving_preserves_aggregate_ownership_and_lease_accounting` | Exercise exhaustive short and generated longer closed-family schedules, checking the full invariant and every source's live-plus-released total after each prefix. |
| `finite_interleaving_length_plus_remaining_physical_work_is_exact` | Compare exact event count plus residual work with initial work, including prefixes ending during extraction or staging. |
| `every_finite_closed_family_of_drains_reaches_quiescence_without_fuel` | Drain a finite pre-existing family without a depth/fuel limit until every local queue is idle and empty; externally retained nodes may remain live. |
| `heap_work_counts_physical_nodes_child_occurrences_and_source_actions` | Count physical allocations, child slots by occurrence and nonempty source actions independently, including maximally shared subtrees. |
| `physical_work_bound_for_idle_initial_drains_counts_no_logical_tree_expansion` | Check the root-token/physical-node/edge/source-action bound on shared DAGs without logical expansion; separately count final flat-backing metadata cleanup. |
| `a_global_step_preserves_each_drain_frontier_under_arbitrary_interleaving` | Check each ordered realized frontier, including local and staged tokens, at every event in the closed family. |
| `every_reachable_family_bounds_combined_local_staged_and_pending_storage` | Measure per-drain and aggregate high-water marks; bind ranks to actual balanced-tree heights and do not use one pooled capacity for separate drains. |
| `global_extraction_consumes_exactly_one_token_across_all_drains` | Compare all aggregate counts across extraction; only the selected node loses one token, including when children are repeated or shared. |
| `the_explicit_arc_contract_selects_the_exact_global_extraction_route` | Bind the actual atomic Arc consumption result and resulting count to the selected modeled route; never implement a strong_count check-then-act guard. |
| `ordinary_final_drop_calls_the_node_destructor_but_into_inner_moves_its_payload` | Test ordinary final-root drop separately from final into_inner: populated callback versus returned payload subsequently detached before shell drop. |
| `active_drains_never_reenter_a_populated_child_destructor_under_the_native_contract` | Instrument populated-destructor nesting while using into_inner, detaching payloads and dropping empty shells; cover diamonds, concurrent releases and retained cursors. |
| `two_failed_uniqueness_checks_can_still_end_in_a_nested_populated_callback` | Force two failed try_unwrap checks before dropping returned Arc values; the final ordinary drop must demonstrate the forbidden nested callback. |
| `closed_interleavings_keep_external_owners_and_the_drain_family_fixed` | Check fixed external lists and family size for the modeled schedule; treat external clone/drop and new drain admission as outside this trace theorem. |

### Native assumptions and qualification boundary

The native bridge takes an explicit atomic Arc consumption contract as a
premise; it does not prove standard-library internals. Ordinary final drop
invokes a populated node callback, whereas final `into_inner` returns the
payload. Active drains permit only `into_inner`, and every returned node must
be emptied before shell destruction. The negative control demonstrates why
two failed uniqueness checks followed by ordinary drops are not equivalent.

Safe Arc ownership and borrowed Rust lifetimes provide retention during
external clone/drop activity, but the global trace relation here fixes
external lists and drain-family size. New external transfers and newly started
ordinary-root drains are outside that relation. No arbitrary-interference
termination or whole-program stack bound is inferred. The logical empty-shell
phase completion is not a claim about the exact instant of native allocation
deallocation or destructor return.

The family frontier result applies to rank-valid initial frontiers. Each
native 127/63-slot buffer additionally requires a single representable balanced
root and a rank equal to its actual height; aggregate family storage is not
one pooled buffer. Private constructors, payload extraction, LIFO occurrence
order, source lifetime, native stack, allocation and final backing cleanup
must still be tested on the actual implementation.

Full compilation passes **68 closed-context reports**, and independent kernel
checking completes successfully. Compilation used 16.383 seconds service time,
8.535 seconds CPU and 537.4M peak memory. Kernel checking used 13.889 seconds
service time, 13.052 seconds CPU and 268.5M peak memory. Both used High512M,
Max1G, zero swap, CPU100%, Tasks64 and Runtime120. Source inspection and isolated
diagnostics preceded these final whole-unit checks; no native performance
result follows from them.

### Initial native source implementation and executable controls

`models/src/rust/rholang/canonical_source_tree.rs` now implements the private
balanced two/three-child source representation, checked iterative join/split,
constant-size shared windows, retained byte/located cursors and node-owned
iterative release. Leaves contain original borrowed slices or the existing
flat frozen backing, never another composed-source wrapper. The existing
frozen-byte cursor skip is reused with crate visibility. No importer backing
switch is included in this checkpoint.

The initial executable bindings live in
`models/tests/support/canonical_source_tree.rs`, separate from production:

| Native control | Observed boundary |
|---|---|
| `concatenation_and_every_split_preserve_bytes_origins_and_balance` | 128 generated chunk families, both prepend/append directions, every cut, exact bytes/provenance, balanced cached lengths/heights, cursor observations, invalid-skip atomicity and rejoined equality. |
| `normalized_windows_compose_without_losing_original_sources` | 128 generated retained-window compositions, unchanged original source and original node identity for constant-time subviews. |
| `frozen_windows_keep_page_owners_after_arena_and_parent_release` | Composed windows retain original frozen bytes and provenance after arena and parent teardown. |
| `coalescing_requires_original_owner_and_exact_adjacency` | Same-root adjacent windows merge; overlap and unrelated equal-content owners do not. Equality compares contents independently of provenance. |
| `checked_lengths_refuse_first_unrepresentable_shared_tree_without_expansion` | A binary shared tree reaches the last representable doubling, rejects overflow and exercises split/rejoin near native bounds without enumerating its logical bytes. |
| `concurrent_shared_root_release_retains_surviving_source_on_small_stacks` | Four simultaneous releases retain a surviving view; its final release uses a 64 KiB thread stack. |
| `source_window_is_three_native_words` | The native shared window occupies three machine words. The full indexed-region backing layout is not yet requalified. |
| `retained_leaf_cursors_avoid_frontier_allocation_and_preserve_skip_origins` | Borrowed and flat-frozen leaf cursors have zero pending-vector capacity and retain exact provenance after skipping and source teardown. |
| `subtree_skip_is_constant_work_before_descent_and_failed_copy_is_atomic` | A near-native-limit skip changes a single pending window without descending; invalid copy leaves output unchanged and valid tail copy is exact. |
| `maximally_shared_ternary_teardown_keeps_combined_frontier_bounded` | An independent fixture walk reaches the two-siblings-per-level bound; actual teardown completes on a 64 KiB stack. This is not instrumentation of every native destructor phase. |
| `simultaneous_final_root_releases_reclaim_each_shared_physical_node` | Four final roots race to release a shared graph on 64 KiB stacks; weak probes confirm every source node is reclaimed after joins. |
| `failed_uniqueness_checks_do_not_have_the_into_inner_payload_contract` | Deterministic native Arc control distinguishes callback-producing ordinary drops after failed checks from successful moved-payload extraction. |

These controls do not complete every per-phase native binding in the formal
catalogue. Exact runtime claim/lease ledgers, allocation accounting, all
interleaving schedules, integrated indexed-region anchors and importer
publication/lifetime bindings remain required. Current fragmented copy and
equality paths are linear retained-cursor loops; chunk-copy optimization remains
an explicit next step. No production speedup or whole-campaign completion follows from this
initial module qualification.

The subsequent original-owner coalescer reuses `NormalizedSourceSpans`:
identity/adjacency/bounds, exact located bytes, normalized subviews and
associative composition. Three additional controls bind these laws to native
borrowed and frozen owners; all fail before implementation and all 15 source
controls pass afterward in debug and release. See J-134 for exact logs.

| Native control | Additional binding |
|---|---|
| `original_leaf_borrowed_rejoins_every_cut_without_a_branch` | Every cut of 128 generated borrowed corpora rejoins to one height-zero original-backed leaf, including independently created leaf nodes. |
| `original_leaf_frozen_rejoins_preserve_backing_and_discontinuous_origins` | Both associations retain one flat-frozen owner, exact discontinuous provenance and payload survival after parent/arena release; separately frozen owners refuse coalescing. |
| `original_leaf_coalescing_rejects_identity_gaps_overlap_and_preserves_empty_views` | Reject distinct slice/source/origin, gaps and overlap; preserve valid zero-width views and representable boundary cases. |

These controls cover the current strict full borrowed-source constructor.
Selected-window construction must distinguish logical terminal bounds from
physical provenance and independently retained end anchors before indexed
integration. In particular a final byte at `usize::MAX` with explicit EOF
is not an overflowing logical window. Existing borrowed empty-tail refusal
when its physical start overflows must remain explicit in the compatibility
binding. No importer cutover is claimed by the local coalescing tests.

The selected-source refinement subsequently implements that distinction via
one `SourcePayload::borrowed` validator reused by full/selected construction,
leaf splitting and coalescing. The represented last byte, rather than an
unused physical end, must fit. Independent zero-width source views may
coalesce without manufacturing an end anchor; explicit empty construction
still requires a representable physical start. Four added controls bind:

| Native control | Additional binding |
|---|---|
| `physical_last_byte_at_native_max_keeps_indexed_end_authority_separate` | Existing indexed byte/end observations remain independent; missing EOF stays missing, explicit EOF survives, and endpoint-free empty views coalesce without changing indexed empty-tail refusal. |
| `physical_last_byte_boundary_survives_every_split_and_original_owner_rejoin` | Every cut at the maximum physical-byte boundary preserves bytes, provenance and one-leaf rejoining. |
| `selected_borrowed_range_preserves_identity_without_validating_unused_tail` | Original full-slice identity survives selecting a valid prefix from a source whose unused tail overflows; reversed, out-of-bounds and overflowing explicit empty ranges refuse. |
| `selected_borrowed_ranges_match_checked_physical_byte_oracle` | 128 generated range/base/corpus cases compare acceptance and physical bytes against independent checked arithmetic and exercise every accepted split. |

All 19 source controls pass in debug and release after the initial two
boundary controls fail on the old constructor. J-135 preserves intermediate
failures and resource evidence. Existing logical `legal_window` bounds and
`explicit_completion_anchor_changes_only_the_end` remain unchanged; no
complete native importer correspondence follows before integration.

Retained-chunk refinement reuses the generic ordered fold in
`IncrementalPrepareStreaming` and equal-content prefix cancellation in
`IndexedWindowTransport`. Byte and located cursors share active-leaf
preparation; frozen scalar and chunk reads share cached-leaf location without
constructing chunk slices on the scalar path. The current/next cached leaf
handles local seeks, with indexed fallback for farther jumps. Copy validates
the entire range before writing; equality compares equal-width prefixes of
possibly unequal chunks, independently of provenance. Identical retained
windows take a constant-work identity path.

| Native control | Chunk-state binding |
|---|---|
| `retained_chunks_match_scalar_interleavings_and_atomic_copy` | 128 mixed borrowed/frozen families compare repeated chunk, scalar read, skip and clone observations against original input; verify every bounded copy window and invalid-range nonmutation. |
| `frozen_chunks_preserve_truncated_page_seams_and_cached_seeks` | Compare truncated frozen page seams against original input bytes, retain data after arena/root release, and exercise zero, local and distant seeks. |
| `chunk_equality_handles_unequal_partitions_and_every_mismatch` | Compare distinct fragmentations and borrowed/frozen sources, every mismatch position, unequal lengths and empty roots; near-native-limit shared windows exercise identity without logical expansion. |

The final local run passes 22 source, 17 arena and 32 admission controls in
debug and all 452 model-library tests in release. J-136 records logs and
bounded resource use. The maintained EPathMap gate now includes both-profile
source/arena controls and debug admission controls, and checks the existing
15 closed incremental-preparation results including ordered chunk folding.
The full proof route reuses that check instead of compiling it twice.
The expanded combined route subsequently passes its exact proof/runtime
counts, kernel checks and ACT-span positive/eight negative compiler controls.
J-136 records its transcript and separates driver resources from child scopes.
Native allocator-work instrumentation and importer source integration remain
separate requirements; neither follows from abstract fold equality.

### Source-backed indexed regions (2026-09-10; task 6018)

`IndexedByteBacking::Composed` retains the original `SourceTreeWindow`, outer
start/length and independent root-end provenance. Ordinary views remain
constant-work clones; explicit anchoring narrows the source window without
copying bytes or wrapping a composed root as a new leaf. `source_window`
extracts only bytes/provenance; an indexed conversion additionally needs an
existing `position_at(len)` result. Test-only Contiguous/Arena backings retain
their direct APIs rather than acquiring copying or lifetime-erasing adapters.

The existing anchor-transport and normalized-source property matrices now
include borrowed/frozen composed branches. Five additional native controls
bind the integration to the existing window, coalescing, cursor and chunk laws:

| Control | Required observation |
|---|---|
| `composed_mixed_sources_keep_lifetimes_and_exact_skip_accounting` | Original located bytes survive parent/arena release and sibling mutations; repeated views retain root identity; skips change yielded but not traversed counts, with failed-skip nonmutation. |
| `composed_conversion_requires_independent_end_authority_at_native_max` | A valid last byte does not synthesize EOF; explicit anchored composed tails remain valid while the old borrowed physical-start refusal remains unchanged. |
| `composed_cross_backing_coalescing_preserves_original_owners_and_right_end` | Borrowed/composed and frozen/composed pairs coalesce by original owner and adjacency in both directions, retaining the right selected endpoint. |
| `composed_framing_matches_original_bytes_and_exact_terminal_errors` | Across every source seam, all canonical field payload kinds and malformed inputs retain field boundaries, located values, exact errors, terminal-error caching and cursor accounting. |
| `all_backings_content_equality_preserves_bytes_across_chunk_partitions` | Every backing pair compares content independently of provenance, with unequal-length and every-position mismatch controls; test-only arena fallback is selected before chunk consumption. |

Typed controls preserve production/test region sizes of 64/72 bytes on the
observed 64-bit target. Outer admission frame and canonical-path frame grow
by 32 bytes to 1,304/592; unchanged 64 KiB admission passes, with measured
driver activation 13,248 bytes and unchanged step/entry activations. J-137
corrects the earlier unqualified debugger type-size attribution and separates
type sizes, activation sizes and service memory. Full release qualification
passes 457 tests; the final fixture-expanded maintained gate also passes,
including both profiles of the source/arena/admission/region groups and the
eight exact ACT span compiler-rejection controls. See J-137 for log identity
and the distinction between outer-service and nested-scope resource usage.
Source-backed compact arena handles, borrowed lifetime threading, historical
publication/resource counters and complete importer correspondence remain
required before closing task 6018.

### Cached source-prefix invariant bindings (2026-09-10; task 6018)

Physical-retention qualification adds three test-only controls:
`physical_inventory_deduplicates_shared_roots_without_expanding_logical_bytes`,
`physical_inventory_deduplicates_registry_cache_and_completion_owners`, and
`retained_prefix_physical_inventory_records_geometric_metadata_growth`.
They count unique reachable backing objects across all roots, independently of
descriptor counters, and retain exact located-byte checks after owner release.
The last control distinguishes cache-only, cache-plus-completions and surviving
final-root ownership. Its measured excess historical retention is a negative
resource result, not a failure of the semantic prefix laws below. These laws do
not prove linear allocation or retained-memory bounds; see journal J-144.

`CachedSourcePrefixes` supplies the denotation bridge for retained arena
completions. A descriptor denotes located bytes, and a compact handle selects
a bounded range. Descriptor IDs are append-only, but their mutable tails can
grow: the required relation is prefix-monotone growth, not immutable entries.
A cache denotes an exact prefix; extending it concatenates only the new suffix.
The semantic lists in this model are observations, not a proposed runtime copy.

| Exported theorem | Required native/property observation |
|---|---|
| `bounded_handles_survive_prefix_monotone_descriptor_growth` | Every issued handle, including nonzero-start slices, retains exact located bytes after tail growth and sibling forks. |
| `cached_prefix_survives_descriptor_growth` | Appending beyond an exported prefix does not change its cache or provenance. |
| `cached_delta_extension_has_exact_requested_contents` | Previous cached contents plus the newly exported suffix equal the independent descriptor oracle. |
| `cached_selection_matches_the_descriptor` | Whole, predecessor-only, tail-only and crossing ranges match the independent arena cursor. |
| `extending_a_cache_preserves_all_older_bounded_handles` | Retain earlier completion views while extending cache coverage; every old selection remains unchanged. |
| `refused_requests_preserve_the_exact_cache` | Range/arithmetic refusal precedes cache mutation, source registration and generated-page sealing. |
| `repeated_or_shorter_exports_reuse_the_existing_cache` | Repeated/shorter exports allocate no new cache entry, page index or publication seal. |
| `successful_requests_keep_cache_valid_and_cover_the_requested_prefix` | Every successful export covers its request, respects native length bounds and retains the cache denotation invariant. |

Full RPB3 compilation reports 341 closed contexts; independent kernel checking
passes. These eight new theorems do not prove the future cache implementation,
its allocator bounds, or source-span integration. EOF remains independent
caller authority. Existing balanced concat/split/release laws still govern
native root operations; scheduling preflights remain unchanged.

`SourceTreeWindow::trimmed_subview` reuses the existing checked subview and
normalized split root at an ownership-export boundary. Unlike ordinary O(1)
subviews, it discards excluded composed subtrees. Original borrowed/frozen
leaf ownership is still shared; trimming does not detach a subrange from its
original allocation. The ownership control keeps an ordinary subview as a
negative control, then checks excluded leaf Weak references expire only after
that control is released. Mixed-source properties and the huge shared-prefix
64 KiB test bind exact selection, shape and stack behavior. Native verification
is recorded in J-138 independently of the cache model.

### Retained arena completion binding (2026-09-10; task 6018)

The first native cache stores generated-segment source roots beside existing
compact descriptor IDs. An iterative postorder composes each exact predecessor
span with its local single-page suffix. Existing page uniqueness checks seal
cached generated segments implicitly through retained immutable Arc ownership;
their nodes cannot subsequently extend in place. No cache eviction is used.
Full-node cache coverage specializes `CachedSourcePrefixes` to the descriptor
length; exported handles select checked subranges and trim excluded subtrees.

| Native control | Formal/ownership obligation |
|---|---|
| `retained_cache_exports_every_range_without_rebuilding_segment_indexes` | Exact selection and unchanged segment-index counts across every page-crossing range. |
| `empty_and_invalid_exports_do_not_publish_cache_or_seal_pages` | Refusal/rootless selection before publication and resource mutation. |
| `retained_publication_preserves_old_forks_and_counts_generated_page_seals` | Immutable cached prefixes, correct sliced predecessors, explicit generated publication cost. |
| `trimmed_cache_export_retains_only_intersecting_generated_pages` | Excluded composed-prefix page owners expire after arena drop; selected page owners survive. |
| `retained_source_cache_matches_independent_historical_operation_traces` | Eager located-byte reference for arbitrary append/slice/concat/export histories and retained output survival. |
| `retained_source_cache_expands_and_drops_deep_descriptors_on_a_small_stack` | 100,000 bytes, approximately 1,563 page descriptors, use heap postorder and stack-safe root release on 64 KiB. |
| `old_cache_hits_do_not_allocate_slots_after_unrelated_growth` | Existing-root reuse before resize; cache alone seals published pages, preserving descriptor fields. |

Production topology completion transports the retained source window and exact
existing endpoint anchor to indexed admission. Eager topology differentials
remain frozen reference controls; the old reducer freezer is test-only.
J-139 records verification separately. Arbitrary-history linear metadata,
literal source-span integration and borrowed arena lifetime threading are
not proved by this owned-generated-page implementation.

### Original-source arena span bindings (2026-09-10; task 6018)

The mixed arena extends the retained generated-segment cache with immutable
source descriptors. A descriptor stores a registered root ID and checked local
range, not a byte vector or a physical-offset reconstruction. The registry
retains original borrowed, frozen or composed source roots. Its lifetime passes
through topology completion into indexed admission; compact history handles
remain lifetime-free IDs and bounded ranges. Generated ACT values still need
owned located-byte pages because a mask-derived byte may differ from the byte
at its physical source position.

This implementation reuses the previously checked `NormalizedSourceSpans`
registration/window laws, `CachedSourcePrefixes` bounded-history/cache laws,
and balanced source concat/split/release laws. Source descriptors never extend
in place, so no additional source cache-sealing protocol is assumed. Generated
segments retain their existing Arc-based publication seal. General arena
freezing is now test-only; production exports only one owned segment at a time
and grafts source windows directly into the balanced tree.

| Native control | Existing invariant bound to implementation |
|---|---|
| `source_segments_retain_borrowed_bytes_without_owned_payload_cells` | Registered original owner, exact translated positions, identical borrowed payload pointer after arena drop, zero owned payload/provenance pages. |
| `source_append_refusals_and_empty_ranges_do_not_mutate_storage` | Slot/range/length checks precede mutation; empty append preserves the handle and resources. |
| `source_offsets_subtract_prefix_before_adding_large_root_position` | Local source offset remains bounded even when the predecessor and source start have a sum outside `usize`; repeated shared roots avoid large payload allocation. |
| `mixed_source_histories_match_located_oracle` | 128 generated append/source/slice/concat histories match an independent located-byte oracle; all historical handles and exported views survive subsequent operations. |
| `mixed_original_composed_sources_preserve_every_selected_range` | Original borrowed/frozen/composed owners remain valid across every source/generated seam and after arena teardown. |
| `direct_owned_segment_exports_match_independent_legacy_freezer` | Direct single-segment metadata matches the old test-only descriptor-walking freezer at capacities 32, 64, 128 and 256. |
| `mixed_source_descriptors_export_and_drop_on_an_unchanged_small_stack` | 4,096 source/generated pairs traverse, export and release on 64 KiB with only 4,096 owned payload cells. |

These controls do not establish end-to-end literal ACT ingestion or a universal
metadata bound. Each accepted source append currently creates one immutable
descriptor. Activation must batch authoritative ACT line spans rather than
turn this into a descriptor-per-byte adapter. Completion EOF anchors remain
independent of source contents. J-140 records this checkpoint's results.

### Joint ACT source batching acceptance boundary (2026-09-10; task 6018)

The next runtime integration must compose three already distinct authorities:
the validated ACT line cursor, the enclosing EPM1 value-table cursor, and the
persistent path reducer with its compressed restoration history. Source-span
storage alone does not authorize advancing any of them. The implementation
boundary remains closed until the joint transition and restoration proofs
and their native properties are checked.

The actual scalar path returns immediately from `value(false, source)` without
querying the reducer. A literal ACT line interior yields an Edge followed by
an absent Value event, so its absent events cannot read a map value, increment
endpoint count, request a child, or settle a completed obligation. That does
not authorize skipping the final line byte, a mask-derived edge, a grammar
transition, an owner boundary, or an outstanding child continuation.

| Acceptance obligation | Required runtime observation |
|---|---|
| Strict persistent payload guard | Live, unfaulted Payload phase; positive width strictly below remaining payload and every held owner boundary; exact current state/byte handle. No per-byte typed-state allocation or obligation-clock increment. |
| Maximal safe source width | Intersect the ACT-offered width with payload remaining minus one, owner distance minus one, and native counter capacity. Do not subtract again from the already strict ACT-offered width. Zero capacity declines batching and leaves the scalar path available. |
| Authoritative source identity | Register the ACT cursor's original source once per owning frame. Select logical line ranges from its exclusive capability; never invert physical provenance or infer literalness from byte equality. |
| Forward compressed restoration | At depth `d` and width `w`, retain every predecessor through depth `d+w-1` in a same-state run. Preserve the old first depth when merging; otherwise start at `d`. Compare located bytes/state, not descriptor-owner equality. |
| Reverse compressed restoration | Limit width to the ACT unwind offer and current run population. Restore its exact scalar predecessor sequence without crossing a state change, node leave or root leave. Width one preserves the original handle where scalar pop does. |
| EPM authority preservation | Advance topology count by twice the forward width or once the reverse width. Preserve value-reader position, value count, endpoint count, terminal state/checks and pending-child authority. Set last source from the actual final emitted event. |
| Joint refusal and publication | Validate widths, counters, source ranges and restoration capacity before advancing either cursor or path history. A stale, unavailable or overflowing optimization declines without mutation; it does not become an artificial input-depth limit. |
| Boundary and oracle controls | Differentially compare every prefix, intermediate absent event and reverse prefix against scalar execution, including malformed/valueless branches, completion boundaries, old handles, shared siblings and discontinuous provenance. Retain generated mask bytes as owned data. |

`BulkCompressedUndo` contributes ten closed-context laws to `RPB3Admission.v`
(351 total). The complete theory and its independent kernel check pass. The
laws identify bulk remember with repeated scalar remember, preserve the exact
located predecessor history across different descriptor owners, and identify
run-bounded pop with repeated scalar pop. Positive valid widths constructively
succeed; zero, invalid and excessive widths are refused. Width-one restoration
retains the original handle and anchor rather than manufacturing a new prefix.

`CanonicalActBatchPublication.v` contributes 21 closed-context laws and also
passes compilation and independent kernel checking against that updated base.
It proves exact ACT scalar traces, enclosing value-authority preservation,
expanded event counts, final source provenance, refusal nonmutation and
constructive acceptance. The forward control consumes two interior bytes of
a three-byte line, but refuses all three; reverse consumes the three line
bytes, but refuses to cross their node leave.

Run `scripts/check-stack-safe-pda-proofs.sh --canonical-key-proofs` to rebuild
and kernel-check these prerequisites without running Rust tests. The same
checks remain part of `--epathmap-import` and the full gate. A successful
proof-only invocation does not mean those broader gates pass.

The model's full authority equality is a freshness specification, not a
request to clone or compare native cursor contents. Native exclusive borrowed
capabilities must provide freshness and the fixed cursor-owned source. The
model's reader list likewise represents preserved reader state, not a runtime
byte projection. ACT event counting inside the model is ghost scalar-trace
accounting; the actual enclosing cursor owns the native topology counter.
Native reverse entry from `Children(0)` must use the existing
`accepted_reverse_entry_commit_matches_the_exact_scalar_trace` normalization
law before applying the abstract `LineLeave` result.

These checked laws do not yet prove the joint native batch adapter. Its strict
typed guard, source registration, restoration updates and capacity preflight
must be bound together before activation. Reservations can change allocation
capacity without changing logical authority; refusal nonmutation is not a
claim that failed reservation attempts have zero allocation effects. Existing PMU-006
source-trie construction failures remain a separate required gate, not grounds
to filter the transport corpus. The independent scalar/eager oracles must
remain usable even when a test-only backing has no retained source window.

The runtime test seam already exists; do not introduce a second eager parser
or parallel restoration oracle for this integration:

| Existing control | Reuse in the joint native binding |
|---|---|
| `assert_focused_persistent_matches` | Compares active/completed obligations, exact completion anchor and located bytes, false/true value observations, fresh obligation clock and reified semantic graph. Invoke it after each accepted batch and corresponding scalar prefix. |
| `persistent_opaque_reuse_guard_matches_widened_native_boundaries` | Extend its Open/SplitComplete/Dead, fault, Payload/Awaiting/Countdown and full-width length matrix to span widths; preserve the original scalar guard checks. |
| `topology_restore_runs_preserve_page_crossing_prefixes_and_divergent_continuations` | Compare bulk restore with its existing saved-handle stack, frozen historical views, pending-child preflight and divergent continuations. |
| `topology_restore_runs_preserve_private_effects_faults_and_fresh_births` | Keep resolve/reject and post-Dead effects in the bulk restoration corpus; do not restrict qualification to healthy linear payloads. |
| `checked_line_transport::arbitrary_branched_fragmented_images_match_every_scalar_prefix` | Retain every forward/absent/reverse prefix and final boundary comparison over the full generated topology. Its PMU-006 construction failure remains visible until repaired. |

These existing controls establish scalar/reference behavior. They do not by
themselves verify a bulk adapter that has not yet been implemented.

The native preparation and restoration components now have separate bindings:

| Checked contract | Native implementation and control |
|---|---|
| Exact run-bounded scalar pop | `CanonicalPathTopologyAdmission::try_leave_run` checks both selected and remaining prefixes before mutation. `bulk_restore_matches_scalar_history_across_nested_payloads` compares 64 generated histories against eager scalar execution and saved checkpoints. |
| Located history and refusal preservation | `bulk_restore_all_mixed_source_prefixes_match_scalar_handles` checks every cut of borrowed/generated histories. `bulk_restore_refusals_preserve_runs_root_and_typed_storage` checks invalid/empty/root cases and the maximum logical width without materializing it. Existing private-effect/fault/fresh-ID controls compare bulk and scalar restoration. |
| Prepared ACT publication | `CanonicalCompactTrieLineSpan::prepare` retains an exclusive frame borrow and precomputes all arithmetic. `CanonicalCompactTriePreparedLineSpan::commit` performs assignments only; it cannot fail after a path update. Dropping preparation leaves the cursor unchanged. |
| Preserved enclosing EPM authority | `IndexedCanonicalEpm1LineSpan` additionally borrows the topology counter. Preparation checks its final value and resolves final provenance from the actual source before inner publication. No reader, endpoint, value-record or terminal field is mutated. |
| Scalar EPM trace and exact counters | `epm1_line_batch.rs` compares forward/absent/reverse traces, value bodies, retained authority and final provenance over set/map cursors, borrowed/frozen/composed sources, and discontinuous physical positions. Separate refusal/drop tests check both directions and the exact maximum counter. |

The production frame driver now joins these components through
`try_epathmap_batch`. `new_with_source` registers the ACT arena once;
`forward_capacity` shares `reusable_payload_prefix` with scalar reuse and
intersects strict payload/owner bounds with native counter capacity.
`try_forward_source` reserves history capacity and appends one original-source
descriptor before publishing its same-state checkpoint and compressed history.
The prepared cursor capability then commits without fallible work. Reverse
capacity is limited to the current restoration run. Scalar boundary events and
pending-child checks remain in the existing driver.

`source_forward_batches_preserve_scalar_histories_and_original_payloads`
binds eager/scalar/batched histories and exact source bytes;
`source_forward_capacity_is_maximal_strict_and_counter_bounded` and
`source_forward_refusals_preserve_logical_state_and_registered_source` bind
width and failure boundaries. `joint_epm_transport_*` compares complete native
admission results and requires nontrivial forward/reverse batching. These
bindings do not substitute for passing verification: current small-stack
qualification and the independently failing PMU-006 gate are tracked in J-143,
separately from the unchanged proof-only result.

`scripts/check-stack-safe-pda-proofs.sh --act-line-transport-compile` rebuilds
the release library and independently checks the positive client plus sixteen
expected compiler rejections, including the eight prepared-capability cases.
The full `--act-line-transport` gate still requires all seven runtime tests
before those same compiler controls. A compiler-only pass does not waive the
PMU-006 runtime failure or establish joint native activation.

## Balanced split endpoint composition binding (2026-09-10; task 6018)

`BalancedSplitRegionPositions` adds no endpoint algorithm. Its sole exported
capstone composes exact split-driver observations with the existing
`IndexedWindowTransport` anchored/rebased-window theorem. Preconditions are a
legal source root and representable cut, exact located-byte equality between
the original region and that root, and a successful terminal split execution.
No `source_refines` predicate is added: located ACT-derived logical values
remain permitted.

| Exported invariant | Required native/property control |
|---|---|
| `iterative_split_outputs_preserve_every_existing_anchored_region_position` | Feed composed outputs into the existing full-position oracles in `indexed_span_transport` and `normalized_source_spans`. Compare every local offset's physical source/offset and Byte/End kind, including beyond-end refusal. Cover zero/full cuts, empty input, interior discontinuous provenance, a preceding leaf with a conflicting anchor, and an original explicit completion override. |

Capture both boundaries from the original view before changing data roots:
left end is `original.position_at(cut).physical`; right end is
`original.end_position().physical`. Reconstruct each output's local logical
offset and position kind; do not copy the original position record wholesale.
At cut zero in nonempty data, the empty left endpoint is the first byte's
provenance. At full length the empty right retains the original end. Empty
input gives both outputs that original end. An absent data root must not
erase its endpoint metadata.

The proof's `Region` lists are semantic observations, not a native flattening
implementation. Original-region owner equality is not a prerequisite for
position equivalence; actual backing/root identity is covered separately.
Existing property oracles remain reusable, but their current native subjects
do not cover an unimplemented composed backend.

Full RPB3 compilation passes 323 closed-context reports and independent
kernel checking passes under High1536M/Max2G, zero swap, CPU100%, Tasks64
and Runtime900. J-128 records exact source/log hashes and the initial
isolated/full implicit-argument mismatch. No combined runtime gate,
performance or native implementation result follows from this capstone.

## Balanced source split driver invariant catalogue (2026-09-10; task 6018)

`BalancedSourceSplitDriver` refines the checked recursive split into explicit
descent and return states. Its five continuation variants retain the untouched
siblings for first/last descent in a two-child node and first/middle/last descent
in a three-child node. The current cut is rebased only when descending past a
cached child prefix. Returning success rebuilds one saved context; returning
failure pops that context without running its glue. An empty returning stack
terminates the machine.

The transition relation preserves the exact recursive interpretation at every
prefix, including refusal, optional output roots, located bytes, reconstruction
counts and visit counts. Termination is proved for every finite input tree,
without fuel or a depth cap; successful source/native contracts additionally
require the recursive model's legal-window, balance and representability
preconditions. A malformed tree's finite execution need not succeed.

### Exhaustive exported invariants and required executable controls

These 15 bindings are required native controls, not implemented-test or runtime
acceptance claims. The recursive model remains the independent specification;
complete located observations include byte value, source identity and offset.

| Exported invariant | Required executable control |
|---|---|
| `split_glue_uses_the_verified_concat_driver_without_intermediate_finalization` | Bind all four glue helpers to the iterative concat subroutine and compare complete results/counters with recursive glue. Two-sibling cases must absorb before finalization; reject the intermediate-finalization mutant. |
| `every_split_frame_rebuilds_the_exact_recursive_outputs_and_counters` | Exercise all five frames with absent/present partial roots and concat carries; compare output roots, child order, caches and counters, and assert actual untouched sibling reference identity. |
| `failed_split_results_propagate_without_any_glue_or_publication` | Inject a checked-window failure below nonempty saved contexts; pop the complete stack without invoking glue or publishing roots. Assert native source/root stores remain unchanged. |
| `each_explicit_split_step_preserves_the_exact_recursive_interpretation` | At every descent/pop prefix, compare the active state plus saved contexts with recursive split interpretation; cover both failures and successful partial results. |
| `explicit_split_loop_terminates_without_fuel_or_a_depth_cap` | Run the explicit loop to an empty returning stack on deep valid trees and finite malformed controls, including range and window refusals; do not introduce a semantic depth limit or recursive split fallback. |
| `every_terminal_split_execution_has_the_exact_recursive_result_and_counters` | Differential-test complete terminal success/failure results against recursive split, including optional root identity, located outputs, visits and reconstruction counts. |
| `every_reachable_split_state_retains_at_most_the_input_height_frames` | Record split-frame count at every transition on balanced inputs, including early stops and failed return paths. Count concat-subroutine frames separately rather than attributing the split-only bound to combined storage. |
| `out_of_range_machine_cuts_return_failure_without_pushing_a_frame` | Reject cuts beyond cached size in the actual first transition; retain the saved stack and require no child descent, glue, window construction or store publication. |
| `empty_optional_data_roots_preserve_the_exact_zero_cut_guard` | Accept absent-root cut zero and reject nonzero cuts with the exact result/counters and unchanged frames; preserve any independent endpoint metadata. |
| `zero_and_full_machine_cuts_keep_the_exact_original_data_root` | Assert actual original-root reference identity for zero/full cuts, one visit and no reconstruction; cover saved outer frames and keep endpoint selection separate. |
| `interior_leaf_machine_steps_keep_both_exact_checked_subwindow_results` | Compare both checked original-source ranges for every strict interior cut, including absolute-end overflow failures, owner/provenance preservation and the successful two-window count. |
| `node2_machine_descent_selects_and_rebases_the_exact_child` | Test cuts before, at and after the child boundary; select the exact child/frame, subtract the first cached length only for last-child descent and retain the untouched sibling reference. |
| `node3_machine_descent_keeps_both_boundaries_and_full_last_child_rebase` | Exercise all child interiors and both boundaries; subtract both preceding lengths for the last child and save exactly the correct ordered siblings. Reject one-prefix-only subtraction and wrong-frame mutants. |
| `successful_tree_executions_inherit_the_full_recursive_split_facts` | From legal private trees, check successful outputs against all recursive facts: source legality, balance, exact caches, cut bounds, located reconstruction, output heights and both work potentials. |
| `legal_optional_inputs_reach_the_exact_successful_recursive_result` | Exhaust valid optional roots and cuts against an eager located-byte oracle; require successful termination, exact sizes/prefix/suffix, output-height bounds, native metadata, absolute terminal bounds and established visit/rebuild limits. |

### Subroutine, frame and endpoint scope

A split pop is a glue-subroutine transition, not a constant-time operation.
Its zero, one or two concatenations use `BalancedSourceConcatDriver`, whose
separate loop correspondence and termination results supply the native
subroutine contract. The current split execution relation does not interleave
individual concat transitions with split transitions. Native code must call
the explicit concat loop rather than translate its recursive proof specification
into runtime recursion.

For balanced input height $`H`$, every reachable split state has at most $`H`$
saved split frames. This is not a bound on simultaneous split-plus-concat
frames, allocator storage or ownership-release work. The recursive split's
visit/reconstruction bounds transfer through exact result equality, but those
counters are not counts of split control transitions or native wall time.

Optional roots describe data, not global endpoint anchors. The left endpoint
still requires `original.position_at(cut)` and the right endpoint requires
`original.end`; absent data, seam positions and completion overrides need their
separate bindings. Immutable reference cloning, source lifetimes, publication
costs, the ownership model's remaining complete-drain obligations and native
Drop/unwind safety are not proved by split-machine correspondence. No balanced
runtime backend, cursor or bulk ACT transport is activated by this checkpoint.

### Split driver proof qualification

The module adds 15 exported capstones. Full compilation passes 322 closed-context
checks (307 existing plus 15 new); independent kernel checking succeeds.
Compilation took 36.070 seconds wall and 35.585 seconds CPU, with a 1.1G peak
and zero swap. Kernel checking took 20.479 seconds wall and 20.241 seconds CPU,
with a 480M peak and zero swap. Both used MemoryHigh=1536M, MemoryMax=2G,
MemorySwapMax=0, CPUQuota=100%, TasksMax=64 and RuntimeMaxSec=900.
Durable logs are `target/verification/epathmap-split-driver-proof-final.log`
and `target/verification/epathmap-split-driver-kernel-final.log`; isolated
diagnostics local-1 through local-6 remain retained. These standalone checks
do not requalify the unified full/focused gate or establish native acceptance.

## Checked constructor scheduling (task 8413)

`CheckedConstructorScheduling` in `RPB3Admission.v` verifies the scheduling
boundary needed to remove oversized admission-frame return values. A
`FrameRequest` retains the region and constructor metadata; it does not contain
an initialized parser frame. The driver realizes that request immediately after
the step returns, before another parser transition can execute. The existing
`dispatch_exact`, `suspended_cursor_and_ordered_read_are_preserved` and
`child_failure_is_terminal` results cover request transport and suspension.
They do not model fallible native constructors or checked counter publication;
the new six-result extension supplies that boundary.

The model separates three ordered operations. **Preparation** performs the
existing checked obligation-counter updates. **Construction** calls the same
fallible frame constructor with the exact request. **Publication** performs the
existing checked frame-kind counter updates and makes the child available to
the driver. Each operation may return an error. Its attempt is recorded, and
the first error terminates processing without attempting later operations.
Successful observations contain the exact waiting parent, constructed child
and published statistics. Root requests use the same construction/publication
ordering, without an enclosing parent continuation.

The equivalence deliberately uses a **terminal-result abstraction**. On failure,
the public result contains the error, not the discarded parent or statistics.
For obligation dispatch, the deferred implementation may install the private
wait field before a constructor fails, where the former implementation installed
it only after successful construction. The theorem does not assert equal
discarded parent fields, failure-state statistics, allocator capacity or timing.
In particular, it does not assert rollback of a successful first obligation
counter increment when the second increment overflows. Native controls retain
and check that ordering explicitly. No failed request may be resumed, published
or allowed to consume a later parent suffix.

### Invariants and native bindings

The following bindings are recorded before implementation verification. All
native tests reside in `models/tests/support/standalone_path_admission.rs`.

| Closed report | Required native evidence |
|---|---|
| `deferred_construction_preserves_terminal_observations_and_attempt_order` | `deferred_epathmap_constructor_matches_direct_constructor`: 128 generated byte cases compare the existing direct constructor with request materialization across source backings; the four-variant control supplies a valid EPM case. Preserve exact constructor arguments and error provenance for all four request variants. |
| `checked_preparation_errors_precede_construction_and_publication` | `deferred_obligation_preparation_overflow_precedes_constructor_failure`: obligation-count overflow wins before payload-count update or invalid-EPM construction; payload-count overflow preserves the preceding successful obligation increment and wins before construction. |
| `constructor_errors_precede_publication_even_if_publication_would_fail` | `deferred_request_constructor_errors_precede_publication`: malformed EPM and invalid schema requests retain their constructor errors even when the corresponding publication counter is already maximal. No child is pushed and no frame-kind count changes. |
| `publication_errors_follow_exactly_one_successful_constructor` | The valid Message request in `deferred_request_constructor_errors_precede_publication` has an overflowing kind counter: construction succeeds, publication returns the exact overflow error, and no child is pushed. Preserve terminal refusal without another parser transition. |
| `successful_deferred_dispatch_preserves_exact_parent_child_and_published_counts` | `deferred_request_variants_publish_exactly_once`: Message, Map, EPathMap and CanonicalPath requests materialize the intended variant, preserve input and metadata, update only the intended counters, and enter the frame stack exactly once. Existing suspension/resolution controls retain exact parent continuation behavior. |
| `rejected_construction_or_publication_never_resumes_a_parent_or_child` | Preserve `child_error_precedes_bad_parent_suffix_and_preserves_physical_anchor` and `pending_parent_cannot_advance_or_accept_wrong_summary`; exercise the unchanged deep tuple admission/teardown test on its original small stack. Reject advancing a failed or waiting request. |

`deferred_request_transport_is_smaller_than_frames` separately checks the native
representation improvement. The formal attempt trace is not a machine-stack
measurement, an allocation count or proof of the native constructor internals.
Constructor and publisher behavior remain explicit parameters; their Rust
bindings and the unchanged grammar differential corpus remain necessary.

The joint-source activation additionally binds this ordering to
`FrameRequest::push_into`: variant-specific helpers construct, check their kind
counter, and publish directly into the existing worklist. EPM construction uses
`with_epm1_region` with a fallible publication continuation, so ACT parsing
returns before the publisher's large enum temporaries become active. The direct
returning constructor exists only in the test module. Completed frames are
dropped in place. These changes add no allocation or semantic transition;
their debug-stack benefit requires the unchanged small-stack controls, not an
assumption that the compiler performs tail-call optimization.

`Frame::step` and `Frame::accept` additionally separate dispatch from result
consumption. The driver constructs a requested child only after `step` returns,
and removes a completed frame before invoking parent acceptance. EPM diagnostic
observation remains after its step on both success and error. The expanded
64 KiB control retains both 8192-byte opaque fixtures and enumerates all nine
component families with precomputed typed body/score oracles. This binds the
same scheduling laws while testing the smaller phase-specific activations;
it does not extend the theorem to arbitrary native stack footprints.

### Proof evidence

Full compilation passes 333 closed-context checks (327 existing plus six new),
and independent kernel checking succeeds before the native scheduling rewrite.
Compilation took 37.475 seconds service wall time and 36.951 seconds CPU, with
a 1.1G peak and zero swap. Kernel checking took 21.216 seconds service wall time
and 20.408 seconds CPU, with a 473.4M peak and zero swap. Both used
MemoryHigh=1536M, MemoryMax=2G, MemorySwapMax=0, CPUQuota=100%, TasksMax=64 and
RuntimeMaxSec=900. Logs are
`target/verification/canonical-constructor-scheduling-proof-r3.log` and
`target/verification/canonical-constructor-scheduling-kernel.log`.

The initial service attempt failed before compilation because its working
directory was absent. The second attempt reached the new proof and failed on
an opaque `realize` expression; explicit unfolding repaired that proof script.
Those diagnostic logs remain retained. The shared formal gate now requires
333 reports, and the standalone admission gate expects its five new controls
in addition to the previous 27 tests. Updating these expected counts does not
claim that the native tests or the combined gate have passed.

The subsequent native rewrite passes all 442 models release library tests.
After the final independent-review assertions, all 32 standalone admission
controls pass in both debug and release, including the unchanged 64 KiB
20,000-level tuple fixture. The complete debug library run remains pending.
J-133 in the PathMap engineering journal records source/log hashes and the
debug activation reduction; these are bounded native observations, not a
proof of Rust compiler stack layout or of the entire campaign.

The complete debug run later reaches its 900-second time limit with 441
passing tests and one unfinished Par-to-S-expression fixture. A separate
unchanged-fixture run with a longer time allowance is pending. This timeout
does not invalidate the 32 passing admission controls, but it cannot be
reported as a successful complete debug suite.

The maintained `--epathmap-import` combined route subsequently passes its
formal, runtime and ACT-span compiler controls. The separate unchanged deep
debug conversion test subsequently passes separately in 1,103.72 seconds,
with 115.7M peak and zero swap. Exact test-name comparison confirms the union
of the 441 prior debug successes and this one success equals the full release
inventory of 442, with no omissions or overlap. This is aggregate qualification
across two invocations, not a successful single full-debug run. The original
900-second timeout is retained. See J-133 for hashes and resource scope.

### Interior ACT source-read observations (2026-09-12; task 6023)

`CheckedACTLineTransport` now separates immutable provenance calculation from
byte fetching. Its independently defined `observed_scalar_next` and
`observed_scalar_run` erase to the existing scalar transition and exact event
stream. A successful interior `LineEdge` emits one source-read observation;
`LineValue` and `LineLeave` use provenance only. Metadata transitions are
independent of the supplied byte-fetch function.

| Checked invariant | Native binding |
|---|---|
| Erasure preserves cursor state, values, event order and provenance | Compare production events with the independent legacy topology oracle in `pathmap_codec_validated_plan.rs`. |
| Width `w` emits reads at exactly the ordered logical suffix indices `start..start+w` | `check_observed_event` observes real `IndexedByteSource` calls and checks one suffix read, rather than trusting production counters. |
| Metadata and reverse line leaves perform no source reads | `independent_source_witness_checks_empty_sparse_dense_and_compressed_rewinds` covers retained metadata, branch edges, final bytes, pops and exhaustion as native controls. |
| Every finite sequence of rewinds has an empty read trace, increments generation once per rewind, and preserves identity, record and event count | `check_observed_rewind` checks actual source accesses and cursor replay; finite prefix rewinds are exercised independently of the model. |
| Metadata reads and duplicate suffix reads are detectable | `independent_source_witness_rejects_metadata_and_duplicate_suffix_reads` supplies causal witness mutations. |

The formal forward theorem retains the original strict interior bound and does
not cover the final line byte, value-bearing node events, cached branch edges,
node pops or the full concrete parser. In particular, a native branch `Edge`
can be emitted from retained plan data without a source read; the model's
interior event classifier must not be generalized to every native `Edge`.
Provenance mapping may be discontinuous or alias physical locations: the proved
sequence is one read per logical suffix index, not a blanket claim that physical
addresses are globally unique. The source function is total in this model;
native source failures and byte-slice ownership remain separate obligations.

These six added reports bring `RPB3Admission.v` to 357 closed contexts. The full
module compiled and passed independent kernel checking under MemoryHigh=1536M,
MemoryMax=2G, no swap, one CPU quota, TasksMax=64 and a 900-second limit. Logs are
`target/verification/act-source-reads-formal-r3.log` and
`act-source-reads-kernel-r3.log`; the two earlier failed rewind-proof attempts
remain retained. The maintained `--canonical-key-proofs` route enforces the new
count and rebuilds dependent proof modules. This formal qualification does not
declare the combined native acceptance gate complete.
