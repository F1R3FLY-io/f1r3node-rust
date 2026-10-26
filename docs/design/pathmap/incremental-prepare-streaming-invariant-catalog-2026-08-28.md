# Incremental PREPARE streaming invariant catalog

Date: 2026-08-28

Status: formal model, exhaustive invariant extraction, executable refinement, bounded production implementation, and package integration verified; workspace integration remains a separate gate

Scope: bounded, stack-safe recovery of ordinary kind-1 RSpace WAL PREPARE/COMMIT records

## Purpose and mandatory refinement order

This is the traceability authority for replacing the record-sized kind-1 recovery buffer with a bounded streaming PDA. The V1 wire image and the contiguous parser remain the frozen semantic oracle. The production implementation may begin only after this document has been translated into executable property tests and the intended memory defect has been observed with a deliberate red control.

The enforced order is:

1. verify the TLA+ protocol, mutants, and reachability witnesses;
2. prove the universal Rocq refinement theorems and Verus arithmetic obligations without admitted results or cheating;
3. extract the complete invariant set into this catalog;
4. bind every applicable row to an executable property and run deliberate red controls against the pre-change implementation;
5. implement the bounded streaming PDA;
6. require formal, property, differential, malformed-input, small-stack, resource, package, and workspace gates to pass.

The first executable traceability audit found that the initial 89-row manifest proved identifier
set equality but did not prove semantic test coverage: most rows named pre-existing contiguous
codec tests instead of the new streaming machines. Any production source drafted while that gap is
being repaired is explicitly non-authoritative. It cannot be integrated or used as completion
evidence until the 35 concrete property families below are compiled, non-ignored, and have first
failed for the intended reason where they are red controls.

That acceptance boundary is now closed. The 89 stable invariant IDs below map exhaustively to the
35 named production properties; their five implementation-facing modules execute 53 non-ignored
tests. The deliberate whole-record and incomplete-source-replay controls failed for their intended
reasons before the bounded implementation replaced them. On 2026-08-28 all 53 refinement tests
passed, including the unchanged 128/256 KiB native-stack subprocess matrix and the geometric
decoder-owned-heap ladder. The integrated formal gate subsequently passed Rocq, Verus
`--no-cheating`, Z3, both positive TLA+ models and their named mutation-kill configurations, and
mCRL2. This closes the catalog's implementation-acceptance rule without promoting the dormant
persistence backend into an active RSpace or consensus route. The enclosing
`rspace_plus_plus --lib` regression gate passes **452/452** in **26.35 s** under a 12 GiB hard
memory cap, zero swap, and a ten-core quota. The complete package gate passes **877/877** across
all library and integration binaries under the same resource scope.

## Executable property families

The 89 invariants refine into these 35 executable property families. This list is exhaustive over
the protocol and byte-level TLA+ invariants, printed Rocq capstones, public Verus obligations, and
the trusted-boundary obligations identified by the formal models.

| Family | Exact executable property | Invariants |
|---|---|---|
| P01 | `kind1_v1_wire_images_are_frozen_and_stream_identical` | K1-WIR-01 |
| P02 | `kind1_discriminator_and_adjacent_pair_matrix_is_exact` | K1-WIR-02, K1-COM-01 |
| P03 | `primary_read_trace_covers_authenticated_extent_once_in_order_for_every_partition` | K1-CUR-01..02 |
| P04 | `first_semantic_error_is_retained_while_declared_extent_is_raw_drained` | K1-CUR-03 |
| P05 | `every_stream_transition_strictly_decreases_formal_progress_measure` | K1-CUR-04 |
| P06 | `all_kind1_extent_count_and_sum_arithmetic_fails_closed_at_boundaries` | K1-ARITH-01 |
| P07 | `kind1_variable_work_is_silent_until_header_pair_and_aggregate_preadmission` | K1-PF-01..02 |
| P08 | `every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work` | K1-LIM-01..08 |
| P09 | `recovery_read_work_separates_authorized_primary_prior_key_and_layout_bytes_exactly` | K1-ACC-01..02 |
| P10 | `recovery_physical_work_is_semantic_comm_neutral` | K1-ACC-03 |
| P11 | `prepare_header_checksum_encoding_and_232_byte_domain_are_exact` | K1-CRC-01..02 |
| P12 | `prepare_record_checksum_encoding_and_prefix_payload_domain_are_exact` | K1-CRC-03..04 |
| P13 | `kind1_error_precedence_is_io_then_crc_then_first_semantic` | K1-CRC-05 |
| P14 | `kind1_physical_digest_is_exact_chunk_fold_over_crc_domain` | K1-DIG-01 |
| P15 | `computed_prepare_digest_has_no_authority_until_exact_commit_binding` | K1-DIG-02 |
| P16 | `rho_path_streaming_matches_contiguous_oracle_for_all_namespaces_partitions_and_prefixes` | K1-MUT-07, K1-EQV-01..03 |
| P17 | `streamed_mutation_envelope_generation_and_section_order_match_contiguous_oracle` | K1-MUT-01..03 |
| P18 | `streamed_mutation_aggregate_counts_and_key_bytes_are_exactly_admitted` | K1-MUT-04..06 |
| P19 | `streamed_prior_key_reread_proves_full_lexicographic_order_or_rejects_source_change` | K1-MUT-08..09, K1-SRC-01 |
| P20 | `streamed_mutation_transition_presence_and_empty_rules_match_oracle` | K1-MUT-10..12 |
| P21 | `streamed_mutation_consumes_exact_declared_extent_and_preserves_precedence` | K1-MUT-13 |
| P22 | `streamed_mutation_identity_matches_receipt_or_rejects_each_binding` | K1-MUT-14..15 |
| P23 | `kind1_object_layout_parser_enforces_count_disposition_reference_order_and_body_shape` | K1-OBJ-01..07 |
| P24 | `all_five_canonical_object_stream_pdas_match_contiguous_oracles_for_generated_valid_and_malformed_images` | K1-OBJ-08..13 |
| P25 | `declared_object_reference_must_equal_streamed_canonical_digest_and_length` | K1-OBJ-14 |
| P26 | `object_byte_sum_includes_data_and_reuse_and_objects_end_at_trailer` | K1-OBJ-15..16 |
| P27 | `incremental_commit_fixed_image_crc_domain_width_and_all_truncations_are_exact` | K1-COM-02 |
| P28 | `incremental_commit_binding_rejects_each_of_six_identity_fields_before_replay` | K1-BIND-01..06 |
| P29 | `outer_chain_binding_rejects_each_of_nine_fields_before_replay` | K1-CHAIN-01..09 |
| P30 | `authenticated_layout_replay_emits_exact_private_index_and_enforces_reuse_rules` | K1-IDX-01..03 |
| P31 | `layout_transcript_and_replay_accept_iff_structurally_identical` | K1-TR-01..03, K1-SRC-01 |
| P32 | `no_validation_failure_exposes_recovery_authority_or_mutates_durable_index` | K1-AUTH-01..02 |
| P33 | `kind1_streaming_recovery_is_a_complete_structured_differential_of_contiguous_oracle` | K1-EQV-01..03 |
| P34 | `kind1_decoder_owned_heap_has_zero_input_slope` | K1-MEM-01 |
| P35 | `kind1_native_stack_high_water_has_zero_input_slope` | K1-MEM-02 |

The formal models abstract cryptographic collision resistance, the concrete canonical-object parsers, filesystem durability, and Rust allocation behavior. Those boundaries are identified below as executable refinement obligations rather than represented as proved facts.

## Frozen design decisions

- Ordinary kind-1 PREPARE and COMMIT bytes remain byte-for-byte V1 compatible.
- The primary pass uses at most 64 KiB of reusable scratch and retains no record-, mutation-, key-, or object-sized byte buffer.
- I/O failure precedes record CRC; record CRC precedes a deferred semantic error. The first semantic error is retained while the remaining authenticated extent is drained.
- COMMIT identity and the complete outer chain/publication authority are checked before private index replay. This deliberately removes pre-chain-validation callback/index effects. Any change in multiply-malformed diagnostic order is consensus-reviewed separately; single-fault behavior remains oracle-equivalent.
- After chain binding, a source-authentication replay rereads the complete PREPARE CRC domain, including the fixed prefix, mutation bytes, every object header, and every DATA body. No private index state may be created until that replay is byte-identical to the primary authenticated transcript. A subsequent bounded layout-application pass rereads only object headers needed to derive the private index and requires the exact first-pass structural transcript, count, order, offsets, and final extent.
- A candidate index may be constructed privately only after chain binding and complete source replay, but the candidate, recovery authority, and observer effects remain unobservable until exact catalog verification.
- Authorized logical pack extent and actual physical reread work are separate quantities. Neither is a semantic COMM charge.
- Cryptographic primitives and the production read-only file lifetime are trusted boundaries with executable byte and source-stability tests, not hidden formal axioms.

## Formal evidence

- TLA+: `formal/tlaplus/stack_safe_pda/RSpaceIncrementalPrepareStreaming.tla`
- Byte-level TLA+: `formal/tlaplus/stack_safe_pda/RSpaceStreamingCodecPDA.tla`
- Rocq: `formal/rocq/stack_safe_pda/theories/IncrementalPrepareStreaming.v`
- Byte-level Rocq: `formal/rocq/stack_safe_pda/theories/StreamingCodecPDA.v`
- Verus: `formal/verus/incremental_prepare_streaming.rs`
- Byte-level Verus: `formal/verus/streaming_codec_pda.rs`
- Integrated gate: `scripts/check-stack-safe-pda-proofs.sh`
- Ignored raw evidence: `scratch/k1-formal/` and `scratch/kind1-streaming-tlc-base-final.log`

The protocol TLA+ model explores 131 distinct states to depth 17 without an invariant failure, kills 19 independent mutants by their named invariant, reaches 12 named terminal witnesses, and reaches the private-candidate-awaiting-catalog intermediate boundary. The byte-level codec TLA+ refinement explores 122 distinct states to depth 6 without an invariant failure, kills 19 independent mutants, and reaches 17 named codec and rejection witnesses. Rocq closes 15 protocol capstones and 20 byte-level refinement capstones under the global context. Verus verifies 23 protocol obligations (15 for the prepare protocol and 8 ledger-trial reservation obligations added by task 7921) and 16 byte-level arithmetic/state obligations with `--no-cheating`.

### Byte-level formal invariant extraction

The detailed codec model closes the abstraction gap for RhoPath framing, mutation-key ordering and positional reread, canonical-object dispatch, error precedence, and bounded retained memory. The following map is exhaustive over the base TLA+ invariants, Rocq capstones, and Verus executable/proof obligations in the three byte-level artifacts.

The protocol-level Verus refinement contributes `checked_extent_advance`, `next_stream_read_width`, `resource_admitted`, `exact_resource_boundary_is_accepted`, `required_minus_one_is_rejected`, `checked_layout_advance`, `checked_add_physical_work`, `full_source_replay_exact`, `admitted_stream_read_width`, `auxiliary_memory_has_zero_input_slope`, `validation_accepted`, `catalog_mismatch_exposes_no_public_state`, and `replay_complete_candidate_remains_private_until_catalog_verification`. These bind K1-CUR-01..04, K1-ARITH-01, K1-PF-01..02, K1-LIM-01..08, K1-ACC-01..02, K1-TR-01..03, K1-SRC-01, K1-AUTH-01..02, and K1-MEM-01. The executable refinement must exercise the concrete Rust call sites rather than treating these arithmetic helpers as sufficient evidence by themselves.

Task 7921 adds the ledger-trial reservation code to the same Verus file: `checked_trial_reserve`, `checked_trial_peak`, `checked_trial_release`, `reserve_trial_step`, `exact_trial_limit_is_accepted`, `one_below_trial_limit_is_refused`, `trial_overflow_is_a_limit_refusal`, and `released_scratch_is_reusable_under_its_peak`. They bind no K1 invariant. They are the checked counters of the [failure-atomic bounded private ledger trials](generation-checkpoint-invariant-catalog-2026-08-28.md#failure-atomic-bounded-private-ledger-trials-2026-10-25) (rows `LT-02`, `LT-07` and `LT-09` of that catalog). The integrated gate requires the file's three reservation mutants, `partial_commit`, `unchecked_limit` and `scratch_conflation`, to fail at their named postconditions.

| Formal obligation(s) | Extracted executable invariant IDs |
|---|---|
| TLA+ `TypeInvariant`, `CursorIsFiniteAndMonotone` | K1-CUR-01..04, K1-ARITH-01 |
| TLA+ `ScratchIsBounded`, `NoWholeFieldRetention` | K1-MEM-01..02 |
| TLA+ `VariableReadsFollowAdmission` | K1-PF-01..02, K1-LIM-01..08 |
| TLA+ `OrderingFollowsKeyValidation` | K1-MUT-07..10, K1-EQV-03 |
| TLA+ `RereadStabilityPrecedesOrderAuthority` | K1-MUT-08..09, K1-SRC-01 |
| TLA+ `FullSourceReplayPrecedesPrivateIndex`, `StableSourceRequiredForSuccess` | K1-SRC-01, K1-AUTH-01..02 |
| TLA+ `CatalogVerificationIsExact`, `NoPublicStateBeforeCatalogVerification`, `NoRejectedCandidatePublishes`, `SuccessPublishesExactCandidate` | K1-AUTH-01..02 |
| TLA+ `AggregatePrecedesFieldValidation` | K1-MUT-04..07, K1-OBJ-09, K1-OBJ-12..13, K1-EQV-03 |
| TLA+ `EveryObjectKindIsDispatched` | K1-OBJ-08..13 |
| TLA+ `OutcomeRefinesCodecOracle` | K1-MUT-01..15, K1-OBJ-08..14, K1-EQV-01..03 |
| Rocq `chunked_frame_pda_refines_contiguous_frame_parser`, `dangling_escape_is_truncated`, `group_marker_inside_frame_is_not_a_terminator`, `escaped_zero_continues_the_frame` | K1-MUT-07, K1-EQV-01..03 |
| Rocq `datum_identity_suffix_is_unframed_and_may_be_empty`, `continuation_requires_nonempty_group_identity_and_no_trailing_bytes`, `join_requires_one_channel_and_nonempty_unframed_suffix` | K1-MUT-07, K1-EQV-01..03 |
| Rocq `equal_keys_are_rejected_by_strict_order`, `proper_prefix_is_lexicographically_smaller` | K1-MUT-08, K1-EQV-03 |
| Rocq `unstable_reread_cannot_authorize_key_order`, `stable_chunk_reassembly_uses_exact_byte_order` | K1-MUT-08..09, K1-SRC-01 |
| Rocq `exact_source_replay_accepts_iff_identical` | K1-SRC-01, K1-TR-02..03, K1-AUTH-01..02 |
| Rocq `catalog_mismatch_exposes_no_authority_index_or_observer`, `replay_complete_candidate_remains_private_until_catalog_verification` | K1-AUTH-01..02 |
| Rocq `malformed_key_precedes_order_and_transition_errors`, `key_order_precedes_noop_transition`, `trailing_bytes_precede_empty_batch` | K1-MUT-07..13, K1-EQV-03 |
| Rocq `aggregate_resource_admission_precedes_variable_grammar` | K1-PF-01..02, K1-LIM-02, K1-LIM-06..07 |
| Rocq `all_five_canonical_object_kinds_are_dispatched_exactly` | K1-OBJ-08..13 |
| Rocq `envelope_extent_mismatch_precedes_every_body_field`, `unsupported_envelope_version_precedes_extent_and_fields` | K1-MUT-01, K1-MUT-13, K1-OBJ-06, K1-OBJ-08..13, K1-EQV-03 |
| Rocq `aggregate_extent_precedes_accelerator_and_retention_fields` | K1-OBJ-12..13, K1-EQV-03 |
| Rocq `streaming_codec_auxiliary_memory_has_zero_input_slope` | K1-MEM-01..02 |
| Verus `checked_codec_extent`, `next_codec_chunk`, `split_codec_scratch` | K1-CUR-01..04, K1-ARITH-01, K1-MEM-01 |
| Verus `frame_transition`, `group_marker_inside_frame_is_invalid`, `escaped_zero_preserves_open_frame` | K1-MUT-07, K1-EQV-01..03 |
| Verus `datum_suffix_may_be_empty`, `empty_continuation_group_is_rejected`, `empty_join_suffix_is_rejected` | K1-MUT-07, K1-EQV-01..03 |
| Verus `unstable_reread_has_no_order_authority` | K1-MUT-09, K1-SRC-01 |
| Verus `full_source_replay_exact` | K1-SRC-01, K1-TR-02..03, K1-AUTH-01..02 |
| Verus `catalog_mismatch_exposes_no_public_state`, `replay_complete_candidate_remains_private_until_catalog_verification` | K1-AUTH-01..02 |
| Verus `mutation_error_precedence_is_fixed` | K1-MUT-07..13, K1-EQV-03 |
| Verus `exactly_five_canonical_object_kinds_are_dispatched` | K1-OBJ-08..13 |
| Verus `envelope_extent_precedes_body_fields` | K1-MUT-01, K1-MUT-13, K1-OBJ-06, K1-OBJ-08..13, K1-EQV-03 |
| Verus `codec_auxiliary_memory_has_zero_input_slope` | K1-MEM-01..02 |

Each of the 19 byte-level TLA+ mutants is bound to the invariant it violates in the integrated proof gate. The 17 reachability configurations witness all nine successful codecs plus frame, group, resource, key, order, unstable-source, aggregate, and field rejection. No formal symbol above may be removed or renamed without breaking the executable traceability test.

## Exhaustive executable invariant catalog

The intended Rust property target is `rspace++/tests/persistence/incremental_prepare_streaming_properties.rs`. The independent contiguous oracle is `TransactionPrepareRecordViewV1::parse`; it must never become a production fallback that restores the whole-record allocation.

### Wire, cursor, admission, and accounting

| ID | Required property | Property generator and oracle |
|---|---|---|
| K1-WIR-01 | V1 incremental PREPARE and COMMIT bytes are unchanged. | Frozen goldens plus randomized encode/parse/stream differentials. |
| K1-WIR-02 | The 32-byte discriminator selects only the exact adjacent kind pairing. | Generate all kinds, truncation at every discriminator byte, and nonadjacent pairs. |
| K1-CUR-01 | The primary cursor is monotone and never exceeds the authenticated PREPARE extent. | Arbitrary chunk schedules including one-byte and boundary-crossing reads. |
| K1-CUR-02 | An accepted primary scan covers every authenticated byte exactly once. | Instrument byte offsets and compare their multiset/order with `0..prepare_len`. |
| K1-CUR-03 | After the first semantic error, recovery raw-drains the declared record; it never resynchronizes within it. | Inject a semantic defect followed by valid record-like suffixes and inspect reads/outcome. |
| K1-CUR-04 | Every nonterminal transition consumes bytes or decreases a finite counter. | Model-derived transition trace with a strict lexicographic progress measure. |
| K1-ARITH-01 | Every offset, length, count, ordinal, and byte-sum operation is checked. | Values biased around zero, exact end, `u64::MAX`, and `usize::MAX`. |
| K1-PF-01 | No variable-size read occurs before fixed-prefix authentication and all relevant resource admission. | Instrumented reader plus independently invalid header/resource fields. |
| K1-PF-02 | No mutation/object body is parsed before the declared PREPARE extent and adjacent COMMIT extent are admitted. | Truncated and oversized paired records with parser callbacks instrumented. |
| K1-LIM-01 | `max_record_bytes` accepts exact demand and rejects required-minus-one before variable read. | Exact/minus-one dependent pair. |
| K1-LIM-02 | `max_mutation_bytes` has the same exact boundary. | Exact/minus-one dependent pair. |
| K1-LIM-03 | `max_objects` has the same exact boundary. | Exact/minus-one dependent pair. |
| K1-LIM-04 | `max_object_bytes` has the same exact boundary. | Exact/minus-one dependent pair. |
| K1-LIM-05 | `max_single_object_bytes` applies equally to DATA and REUSE. | Generate both dispositions at exact/minus-one limits. |
| K1-LIM-06 | `max_mutation_entries` has the same exact boundary. | Generate the three section counts coherently. |
| K1-LIM-07 | `max_mutation_key_bytes` has the same exact boundary. | Generate escaped RhoPath keys around the boundary. |
| K1-LIM-08 | Durable transaction/object/index/scan limits precede their corresponding work. | One typed resource refusal at a time with read/callback trace. |
| K1-ACC-01 | Authorized pack extent, primary authenticated reads, prior-key rereads, complete authenticated-source replay, header-only layout-application replay, and their physical sum are reported separately and exactly. | Counting reader over a DATA-bearing PREPARE; independently derive the full PREPARE CRC domain and the object-header-only replay extent. |
| K1-ACC-02 | Prior-key, authenticated-source, and layout-application rereads are admitted and counted before issuance. The source replay includes the fixed prefix, mutations, object headers, and every DATA body; the later layout replay includes object headers only. | Exact and underprovisioned physical-read budgets for each replay class. |
| K1-ACC-03 | Recovery physical accounting never changes semantic COMM accounting. | Seed nonzero runtime accounting and compare the complete before/after snapshot. |

### CRC and physical digest

| ID | Required property | Property generator and oracle |
|---|---|---|
| K1-CRC-01 | Header checksum is a zero-extended little-endian `u32`. | Set each upper checksum bit independently. |
| K1-CRC-02 | Header CRC covers exactly the 232 authenticated prefix bytes. | Omit, duplicate, reorder, and substitute each boundary chunk. |
| K1-CRC-03 | Trailer checksum is a zero-extended little-endian `u32`. | Set each upper checksum bit independently. |
| K1-CRC-04 | Record CRC covers the complete prefix and payload and excludes the eight-byte trailer. | Independent reference CRC over randomized chunk schedules. |
| K1-CRC-05 | Record CRC failure precedes every deferred semantic error. | Paired corruption with invalid CRC, then the same semantic defect with repaired CRC. |
| K1-DIG-01 | The physical digest covers exactly the record-CRC domain. | Independent BLAKE2b fold over the frozen contiguous bytes. |
| K1-DIG-02 | A computed digest is not authoritative until the parsed COMMIT binds it exactly. | Mutate COMMIT digest and every earlier identity field independently. |

### Canonical mutation grammar

| ID | Required property | Property generator and oracle |
|---|---|---|
| K1-MUT-01 | Envelope domain, version, and body extent are exact. | Canonical mutations plus single-field/truncation/extension mutations. |
| K1-MUT-02 | Schema and base/target generations are canonical. | Boundary-biased generation and byte mutation. |
| K1-MUT-03 | Exactly three sections occur in the frozen order. | Omit, duplicate, and permute section tags. |
| K1-MUT-04 | Every section count equals the number of parsed entries. | Count-minus/plus-one and premature end. |
| K1-MUT-05 | Aggregate entry count is exact and admitted. | Independently sum all three sections. |
| K1-MUT-06 | Aggregate key-byte count is exact and admitted. | Independently sum encoded key extents. |
| K1-MUT-07 | Every key is a canonical `RhoPathV1` in the correct namespace. | Existing arbitrary canonical/malformed RhoPath corpus. |
| K1-MUT-08 | Adjacent keys are strictly increasing. | Duplicate, swap, prefix-related, and first-differing-byte cases. |
| K1-MUT-09 | Chunkwise prior-key reread reproduces the saved length and digest. | Stable reader and a reader that substitutes prior-key chunks between passes. |
| K1-MUT-10 | Count transitions cannot encode no-ops. | Equal before/after multiplicities and zero deltas. |
| K1-MUT-11 | Content before/after presence tags and references are exact. | All four presence combinations plus malformed tags/references. |
| K1-MUT-12 | The mutation batch is nonempty. | Zero counts with otherwise canonical envelope. |
| K1-MUT-13 | Mutation parsing consumes exactly `mutation_len`. | Truncation and trailing bytes at every field boundary. |
| K1-MUT-14 | Streamed mutation digest equals the sealed receipt digest. | Independent contiguous digest and one-byte substitutions. |
| K1-MUT-15 | Streamed base and target generations equal the sealed receipt. | Mutate each binding independently. |

### Canonical object stream

| ID | Required property | Property generator and oracle |
|---|---|---|
| K1-OBJ-01 | Exactly `object_count` object records occur. | Zero, exact, minus-one, plus-one, and premature trailer. |
| K1-OBJ-02 | Disposition accepts only DATA or REUSE. | Exhaust every other tag byte. |
| K1-OBJ-03 | Each object reference is exactly 40 canonical bytes. | Truncation and malformed digest/length framing. |
| K1-OBJ-04 | Reference digests are strictly increasing. | Duplicate, swap, and equal-digest/different-length aliases. |
| K1-OBJ-05 | The single-object limit applies equally to DATA and REUSE declarations. | Both dispositions at exact/minus-one limits. |
| K1-OBJ-06 | DATA consumes exactly its declared canonical body. | Truncation, extension, and next-record overlap. |
| K1-OBJ-07 | REUSE consumes no body. | Insert body-shaped bytes after a REUSE header. |
| K1-OBJ-08 | All five canonical object domains receive the same complete validation as the contiguous oracle. | One generator/differential per domain. |
| K1-OBJ-09 | DAG endpoints, branch classes, edge order, bitmap/slot rules, and empty/unary rejection match the oracle. | Existing canonical DAG arbitrary/malformed corpus. |
| K1-OBJ-10 | Blob inner domain, version, kind, and length are exact. | Existing blob corpus plus field mutations. |
| K1-OBJ-11 | Semantic-manifest fixed roots, order, and presence match the oracle. | Existing manifest corpus plus field mutations. |
| K1-OBJ-12 | Accelerator-manifest count, order, and artifact kinds match the oracle. | Existing manifest corpus plus field mutations. |
| K1-OBJ-13 | Retention current/history count, order, and exclusion rules match the oracle. | Existing retention corpus plus field mutations. |
| K1-OBJ-14 | Computed canonical object reference equals the declared digest and length. | Mutate digest and length independently. |
| K1-OBJ-15 | `object_bytes` sums declared lengths for both DATA and REUSE references. | Mixed-disposition records with an independent checked sum. |
| K1-OBJ-16 | Object parsing ends exactly at the PREPARE trailer. | Omission and trailing payload at every object boundary. |

### COMMIT, chain, replay, index, source, and authority

| ID | Required property | Property generator and oracle |
|---|---|---|
| K1-COM-01 | Incremental PREPARE pairs only with the adjacent incremental COMMIT. | All record kinds, gaps, and truncations. |
| K1-COM-02 | COMMIT fixed length, CRC domain, and checksum encoding are exact. | Frozen bytes plus exhaustive boundary truncation and checksum mutations. |
| K1-BIND-01 | COMMIT sequence equals PREPARE sequence. | Single-field mutation. |
| K1-BIND-02 | COMMIT store identity equals PREPARE store identity. | Single-field mutation. |
| K1-BIND-03 | COMMIT physical digest equals the complete PREPARE digest. | Single-field mutation. |
| K1-BIND-04 | COMMIT address length equals PREPARE length. | Single-field mutation. |
| K1-BIND-05 | COMMIT receipt equals PREPARE receipt. | Single-field mutation. |
| K1-BIND-06 | COMMIT address offset equals the admitted PREPARE offset. | Single-field mutation. |
| K1-CHAIN-01 | Outer expected sequence binds exactly. | Single-field mutation. |
| K1-CHAIN-02 | Outer store identity binds exactly. | Single-field mutation. |
| K1-CHAIN-03 | Outer profile binds exactly. | Single-field mutation. |
| K1-CHAIN-04 | State base generation binds exactly. | Single-field mutation. |
| K1-CHAIN-05 | Previous COMMIT binds exactly. | Single-field mutation. |
| K1-CHAIN-06 | PREPARE address binds exactly. | Single-field mutation. |
| K1-CHAIN-07 | Publication first-live pack binds exactly. | Single-field mutation. |
| K1-CHAIN-08 | Index generation binds exactly. | Single-field mutation. |
| K1-CHAIN-09 | Index tail binds exactly. | Single-field mutation. |
| K1-IDX-01 | Each DATA object emits exactly one private location with exact pack, offset, sequence, and reference. | Compare replay output with the contiguous oracle. |
| K1-IDX-02 | REUSE emits no location and resolves to a strictly prior exact reference. | Missing, mismatched, same-generation, and valid prior references. |
| K1-IDX-03 | Generation-start incremental rules reject forbidden reuse. | First-generation and successor dependent cases. |
| K1-TR-01 | The structural transcript is domain-separated and covers ordinal, disposition, reference, relative offset, and final extent. | Independent transcript implementation over generated layout traces. |
| K1-TR-02 | Replay count, order, offsets, and final extent equal the first pass. | Arbitrary DATA/REUSE layouts and chunk schedules. |
| K1-TR-03 | Substitution, omission, duplication, reordering, or offset shift rejects. | Typed `ReplayMutation` enumeration. |
| K1-SRC-01 | Source instability during the reader lifetime is excluded by the production contract and detected before private index construction. The authenticated replay covers the entire PREPARE CRC domain, including DATA bodies, rather than only structural headers. | Deterministic prefix-, mutation-, object-header-, and DATA-body-only mutations between primary fold, binding, source replay, and layout application. |
| K1-AUTH-01 | Recovery authority, public index state, and observer effects are absent until exact catalog verification. | Instrument every validation failure, the private-candidate boundary, and the catalog boundary. |
| K1-AUTH-02 | No rejected candidate can publish its derived index or observer effects; a catalog mismatch leaves all public state absent. | Seed authoritative evidence and fail every gate, including exact catalog-image and terminal binding. |
| K1-EQV-01 | Streaming accepts if and only if the contiguous oracle accepts. | Proptest over canonical and single-/multi-malformed records and arbitrary chunk schedules. |
| K1-EQV-02 | On success, identity, receipt, object observations, private index, and terminal summary equal the oracle. | Complete structured result differential. |
| K1-EQV-03 | Every single-fault error class and its ordering equal the oracle except the explicitly reviewed chain-before-index side-effect refinement. | Typed fault matrix, never an unstructured random byte alone. |
| K1-MEM-01 | Decoder-owned auxiliary heap is fixed scratch plus constant state, independent of record, mutation, key, and object sizes. | Phase-resetting allocator at a geometric size ladder and deliberate whole-record control. |
| K1-MEM-02 | Native stack high-water is independent of record, key, object, and object-count scale. | Isolated 128/256 KiB stack subprocesses and geometric depth/size ladder. |

## Formal-to-property derivation matrix

Classification: **D** is directly proved at the model abstraction; **P** has a proved abstract core plus concrete executable refinement; **E** is necessarily executable because it binds bytes, cryptographic implementations, allocation, or platform behavior. Every E row remains mandatory.

| Property range | Formal symbol(s) | Class | Executable qualification |
|---|---|---|---|
| K1-WIR-01..02 | `TypeInvariant`, `OutcomeRefinesContiguousOracle`, `chunk_fold_equals_contiguous_fold` | P | Frozen byte layout and concrete kind tags are executable. |
| K1-CUR-01..04 | `CursorProgressIsExact`, `AuthenticationTranscriptIsExact`, `SemanticTranscriptIsAPrefix`; `chunk_fold_equals_contiguous_fold`; Verus `checked_extent_advance`, `next_stream_read_width` | P | Concrete read schedules and raw drain are executable. |
| K1-ARITH-01 | Verus `checked_extent_advance`, `checked_layout_advance`, `checked_add_physical_work` | D/P | Production call-site completeness is executable. |
| K1-PF-01..02 | `VariableReadsFollowAdmission`, `MinusOneIsRejectedBeforeVariableRead`; Verus `admitted_stream_read_width` | P | Header/COMMIT byte authentication is executable. |
| K1-LIM-01..08 | `MinusOneIsRejectedBeforeVariableRead`, `ExactBoundaryIsNotRejected`; Rocq `exact_resource_boundary_is_admitted`, `required_minus_one_is_rejected`; Verus resource theorems | P | Seven concrete limits and durable aggregate limits are executable. |
| K1-ACC-01..03 | Verus `checked_add_physical_work`; existing checkpoint accounting separation proof | P | Reader counters and complete runtime snapshots are executable. |
| K1-CRC-01..05 | `CrcDominatesSemanticError`; Rocq `observation_fold_retains_exact_first_error`, `deferred_semantic_error_refines_contiguous_oracle`, `crc_failure_dominates_every_semantic_error`, `chunk_fold_equals_contiguous_fold` | P | CRC32C domains/encodings are executable. |
| K1-DIG-01..02 | Rocq `chunk_fold_equals_contiguous_fold`; `NoPrivateIndexBeforeCommitBinding` | P | BLAKE2b implementation and exact COMMIT bytes are executable. |
| K1-MUT-01..15 | `SemanticTranscriptIsAPrefix`, `OutcomeRefinesContiguousOracle`; Rocq first-error and chunk-fold theorems | P/E | The complete concrete mutation grammar is differential-tested. |
| K1-OBJ-01..16 | `ReplayTranscriptMatchesFirstPass`, `OutcomeRefinesContiguousOracle`; Rocq chunk-fold/replay theorems; Verus `checked_layout_advance` | P/E | All five concrete canonical grammars and references are differential-tested. |
| K1-COM-01..02 | `NoPrivateIndexBeforeCommitBinding`, `OutcomeRefinesContiguousOracle` | P | Concrete COMMIT bytes, CRC, and adjacency are executable. |
| K1-BIND-01..06 | `CommitBindsExactIdentity`, `AcceptedIdentityIsExact`; Rocq `commit_binding_accepts_exactly_the_prepare_identity` | D/P | Concrete field extraction is executable. |
| K1-CHAIN-01..09 | `NoPrivateIndexBeforeChainBinding`, `OutcomeRefinesContiguousOracle`; Rocq authority conjunction | P | Nine concrete outer bindings are executable. |
| K1-IDX-01..03 | `NoPrivateIndexBeforeCommitBinding`, `NoPrivateIndexBeforeChainBinding`, `FullSourceReplayPrecedesPrivateIndex`, `ReplayTranscriptMatchesFirstPass`, `NoPublicStateBeforeCatalogVerification` | P | PathMap index operations and REUSE lookup are executable. |
| K1-TR-01..03 | `AuthenticationTranscriptIsExact`, `ReplayTranscriptMatchesFirstPass`; Rocq `exact_layout_replay_accepts_iff_identical`, `exact_source_replay_accepts_iff_identical`; Verus `checked_layout_advance`, `full_source_replay_exact` | P | Concrete domain-separated source and layout transcript hashes are executable. |
| K1-SRC-01 | `FullSourceReplayPrecedesPrivateIndex`, `StableSourceRequiredForSuccess`; Rocq `exact_source_replay_accepts_iff_identical` and authority conjunction; Verus `full_source_replay_exact` | P | Complete CRC-domain replay, file-lifetime contract, and prefix/mutation/header/DATA-body unstable-reader rejection are executable. |
| K1-AUTH-01..02 | `CatalogVerificationIsExact`, `NoPublicStateBeforeCatalogVerification`, `NoRejectedCandidatePublishes`, `SuccessPublishesExactCandidate`; Rocq `authority_exists_iff_every_validation_gate_passes`, `any_failed_gate_exposes_no_authority`, `index_is_not_visible_before_full_authentication`, `catalog_mismatch_exposes_no_authority_index_or_observer`, `replay_complete_candidate_remains_private_until_catalog_verification`; Verus `catalog_mismatch_exposes_no_public_state`, `replay_complete_candidate_remains_private_until_catalog_verification` | D/P | Exact catalog verification, private-candidate isolation, observer silence, and durable publication are executable. |
| K1-EQV-01..03 | `OutcomeRefinesContiguousOracle`; Rocq chunk/error/replay/binding capstones | P | Full structured differential supplies concrete semantics. |
| K1-MEM-01..02 | `ScratchIsBounded`, `NoWholeRecordBuffer`; Rocq `retained_memory_is_independent_of_record_extent`; Verus `auxiliary_memory_has_zero_input_slope` | P/E | Allocation, RSS, and native stack are measured executable obligations. |

## Negative controls

Every formal mutant is independently required to fail by the named invariant. Runtime controls are temporary test implementations or instrumented old behavior; no control may remain reachable in production.

| Mutant/control | Formal rejection | Executable property IDs |
|---|---|---|
| Whole-record buffer | `NoWholeRecordBuffer` | K1-MEM-01 |
| Record-sized scratch | `ScratchIsBounded` | K1-MEM-01 |
| Variable read before admission | `VariableReadsFollowAdmission` | K1-PF-01, K1-LIM-01..08 |
| Accept required-minus-one | `MinusOneIsRejectedBeforeVariableRead` | K1-LIM-01..08 |
| Reject exact boundary | `ExactBoundaryIsNotRejected` | K1-LIM-01..08 |
| Cursor off by one | `CursorProgressIsExact` | K1-CUR-01..04 |
| Incomplete primary transcript | `AuthenticationTranscriptIsExact` | K1-CUR-02, K1-TR-01 |
| Semantic error before CRC | `CrcDominatesSemanticError` | K1-CRC-05 |
| Dropped semantic error | `OutcomeRefinesContiguousOracle` | K1-EQV-01, K1-EQV-03 |
| Wrong accepted identity | `AcceptedIdentityIsExact` | K1-BIND-01..06, K1-EQV-02 |
| Skip COMMIT binding | `NoPrivateIndexBeforeCommitBinding` | K1-BIND-01..06, K1-IDX-01 |
| Skip outer-chain binding | `NoPrivateIndexBeforeChainBinding` | K1-CHAIN-01..09, K1-IDX-01 |
| Swap replay layouts | `ReplayTranscriptMatchesFirstPass` | K1-TR-02..03 |
| Skip replay transcript item | `ReplayTranscriptMatchesFirstPass` | K1-TR-02..03 |
| Accept unstable source or construct a private index before complete source replay | `FullSourceReplayPrecedesPrivateIndex`, `StableSourceRequiredForSuccess` | K1-SRC-01, K1-AUTH-01..02 |
| Expose index or observer effects before catalog verification | `NoPublicStateBeforeCatalogVerification` | K1-AUTH-01..02 |
| Accept a catalog mismatch | `CatalogVerificationIsExact` | K1-AUTH-01..02 |
| Publish a catalog-rejected candidate | `NoRejectedCandidatePublishes` | K1-AUTH-01..02 |
| Existing buffered recovery on a large valid record | Deliberate executable red control | K1-MEM-01; must observe a prepare-sized allocation/read before correction. |
| Header-only replay against a reader whose DATA body changes | Deliberate executable red control | K1-SRC-01, K1-TR-03. |
| DATA-only object-byte sum | Deliberate executable red control | K1-OBJ-15. |
| Domain-plus-digest-only object validator | Deliberate executable red control | K1-OBJ-08..14. |

## Non-vacuity witnesses

The TLA+ gate reaches success plus each modeled rejection: header, resource, pair, read, CRC, mutation, object, COMMIT, chain, unstable source, and catalog mismatch. It separately reaches a fully replayed private candidate awaiting catalog verification with no public authority, index, or observer state. Executable properties additionally require witnesses for every streaming phase, every mutation namespace, every content-presence combination, all five canonical object kinds, every DAG branch/end-point class, DATA-only, REUSE-only, mixed layouts, exact/minus-one limits, and lengths `0`, `1`, `scratch−1`, `scratch`, `scratch+1`, and multiple scratch chunks.

## Completion rule

This catalog becomes implementation-ready only when an executable manifest contains exactly the 89 `K1-*` IDs above, every ID names at least one non-ignored property, the formal/property ID equality gate passes, and the deliberate old-implementation memory control has failed for the intended reason. It becomes complete only after the bounded implementation makes every production property green, the whole-buffer control remains test-only, and the capped resource evidence demonstrates constant decoder-owned scratch and stack.

The 2026-08-28 implementation coordinate satisfies that rule: the manifest has exact 89-ID set
equality, all 35 property families are non-ignored, the 53 implementation-facing refinement tests
are green, the whole-buffer path is test-only, retained scratch is fixed at 64 KiB plus constant
state, and the 128/256 KiB subprocess matrix observes zero input-dependent native-stack growth.
