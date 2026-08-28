use std::collections::{BTreeMap, BTreeSet};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use proptest::prelude::*;

use crate::rspace::persistence::durable::IncrementalLayoutTranscriptV1;
use crate::rspace::persistence::{LogPackObjectDispositionV1, ObjectDigest, ObjectRefV1};

const WIRE: &str = "kind1_wire_protocol_properties.rs";
const RHO_PATH: &str = "rho_path_streaming_properties.rs";
const MUTATION: &str = "mutation_streaming_properties.rs";
const OBJECT: &str = "canonical_object_streaming_properties.rs";
const RECOVERY: &str = "kind1_recovery_protocol_properties.rs";

const K1_PROPERTY_BINDINGS: &[(&str, &str, &str)] = &[
    ("K1-WIR-01", WIRE, "kind1_v1_wire_images_are_frozen_and_stream_identical"),
    ("K1-WIR-02", WIRE, "kind1_discriminator_and_adjacent_pair_matrix_is_exact"),
    ("K1-CUR-01", WIRE, "primary_read_trace_covers_authenticated_extent_once_in_order_for_every_partition"),
    ("K1-CUR-02", WIRE, "primary_read_trace_covers_authenticated_extent_once_in_order_for_every_partition"),
    ("K1-CUR-03", WIRE, "first_semantic_error_is_retained_while_declared_extent_is_raw_drained"),
    ("K1-CUR-04", WIRE, "every_stream_transition_strictly_decreases_formal_progress_measure"),
    ("K1-ARITH-01", WIRE, "all_kind1_extent_count_and_sum_arithmetic_fails_closed_at_boundaries"),
    ("K1-PF-01", WIRE, "kind1_variable_work_is_silent_until_header_pair_and_aggregate_preadmission"),
    ("K1-PF-02", WIRE, "kind1_variable_work_is_silent_until_header_pair_and_aggregate_preadmission"),
    ("K1-LIM-01", WIRE, "every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work"),
    ("K1-LIM-02", WIRE, "every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work"),
    ("K1-LIM-03", WIRE, "every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work"),
    ("K1-LIM-04", WIRE, "every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work"),
    ("K1-LIM-05", WIRE, "every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work"),
    ("K1-LIM-06", WIRE, "every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work"),
    ("K1-LIM-07", WIRE, "every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work"),
    ("K1-LIM-08", WIRE, "every_kind1_limit_accepts_exact_and_rejects_minus_one_before_corresponding_work"),
    ("K1-ACC-01", WIRE, "recovery_read_work_separates_authorized_primary_prior_key_and_layout_bytes_exactly"),
    ("K1-ACC-02", WIRE, "recovery_read_work_separates_authorized_primary_prior_key_and_layout_bytes_exactly"),
    ("K1-ACC-03", WIRE, "recovery_physical_work_is_semantic_comm_neutral"),
    ("K1-CRC-01", WIRE, "prepare_header_checksum_encoding_and_232_byte_domain_are_exact"),
    ("K1-CRC-02", WIRE, "prepare_header_checksum_encoding_and_232_byte_domain_are_exact"),
    ("K1-CRC-03", WIRE, "prepare_record_checksum_encoding_and_prefix_payload_domain_are_exact"),
    ("K1-CRC-04", WIRE, "prepare_record_checksum_encoding_and_prefix_payload_domain_are_exact"),
    ("K1-CRC-05", WIRE, "kind1_error_precedence_is_io_then_crc_then_first_semantic"),
    ("K1-DIG-01", WIRE, "kind1_physical_digest_is_exact_chunk_fold_over_crc_domain"),
    ("K1-DIG-02", WIRE, "computed_prepare_digest_has_no_authority_until_exact_commit_binding"),
    ("K1-MUT-01", MUTATION, "streamed_mutation_envelope_generation_and_section_order_match_contiguous_oracle"),
    ("K1-MUT-02", MUTATION, "streamed_mutation_envelope_generation_and_section_order_match_contiguous_oracle"),
    ("K1-MUT-03", MUTATION, "streamed_mutation_envelope_generation_and_section_order_match_contiguous_oracle"),
    ("K1-MUT-04", MUTATION, "streamed_mutation_aggregate_counts_and_key_bytes_are_exactly_admitted"),
    ("K1-MUT-05", MUTATION, "streamed_mutation_aggregate_counts_and_key_bytes_are_exactly_admitted"),
    ("K1-MUT-06", MUTATION, "streamed_mutation_aggregate_counts_and_key_bytes_are_exactly_admitted"),
    ("K1-MUT-07", RHO_PATH, "rho_path_streaming_matches_contiguous_oracle_for_all_namespaces_partitions_and_prefixes"),
    ("K1-MUT-08", MUTATION, "streamed_prior_key_reread_proves_full_lexicographic_order_or_rejects_source_change"),
    ("K1-MUT-09", MUTATION, "streamed_prior_key_reread_proves_full_lexicographic_order_or_rejects_source_change"),
    ("K1-MUT-10", MUTATION, "streamed_mutation_transition_presence_and_empty_rules_match_oracle"),
    ("K1-MUT-11", MUTATION, "streamed_mutation_transition_presence_and_empty_rules_match_oracle"),
    ("K1-MUT-12", MUTATION, "streamed_mutation_transition_presence_and_empty_rules_match_oracle"),
    ("K1-MUT-13", MUTATION, "streamed_mutation_consumes_exact_declared_extent_and_preserves_precedence"),
    ("K1-MUT-14", RECOVERY, "streamed_mutation_identity_matches_receipt_or_rejects_each_binding"),
    ("K1-MUT-15", RECOVERY, "streamed_mutation_identity_matches_receipt_or_rejects_each_binding"),
    ("K1-OBJ-01", RECOVERY, "kind1_object_layout_parser_enforces_count_disposition_reference_order_and_body_shape"),
    ("K1-OBJ-02", RECOVERY, "kind1_object_layout_parser_enforces_count_disposition_reference_order_and_body_shape"),
    ("K1-OBJ-03", RECOVERY, "kind1_object_layout_parser_enforces_count_disposition_reference_order_and_body_shape"),
    ("K1-OBJ-04", RECOVERY, "kind1_object_layout_parser_enforces_count_disposition_reference_order_and_body_shape"),
    ("K1-OBJ-05", RECOVERY, "kind1_object_layout_parser_enforces_count_disposition_reference_order_and_body_shape"),
    ("K1-OBJ-06", RECOVERY, "kind1_object_layout_parser_enforces_count_disposition_reference_order_and_body_shape"),
    ("K1-OBJ-07", RECOVERY, "kind1_object_layout_parser_enforces_count_disposition_reference_order_and_body_shape"),
    ("K1-OBJ-08", OBJECT, "all_five_canonical_object_stream_pdas_match_contiguous_oracles_for_generated_valid_and_malformed_images"),
    ("K1-OBJ-09", OBJECT, "all_five_canonical_object_stream_pdas_match_contiguous_oracles_for_generated_valid_and_malformed_images"),
    ("K1-OBJ-10", OBJECT, "all_five_canonical_object_stream_pdas_match_contiguous_oracles_for_generated_valid_and_malformed_images"),
    ("K1-OBJ-11", OBJECT, "all_five_canonical_object_stream_pdas_match_contiguous_oracles_for_generated_valid_and_malformed_images"),
    ("K1-OBJ-12", OBJECT, "all_five_canonical_object_stream_pdas_match_contiguous_oracles_for_generated_valid_and_malformed_images"),
    ("K1-OBJ-13", OBJECT, "all_five_canonical_object_stream_pdas_match_contiguous_oracles_for_generated_valid_and_malformed_images"),
    ("K1-OBJ-14", RECOVERY, "declared_object_reference_must_equal_streamed_canonical_digest_and_length"),
    ("K1-OBJ-15", RECOVERY, "object_byte_sum_includes_data_and_reuse_and_objects_end_at_trailer"),
    ("K1-OBJ-16", RECOVERY, "object_byte_sum_includes_data_and_reuse_and_objects_end_at_trailer"),
    ("K1-COM-01", WIRE, "kind1_discriminator_and_adjacent_pair_matrix_is_exact"),
    ("K1-COM-02", RECOVERY, "incremental_commit_fixed_image_crc_domain_width_and_all_truncations_are_exact"),
    ("K1-BIND-01", RECOVERY, "incremental_commit_binding_rejects_each_of_six_identity_fields_before_replay"),
    ("K1-BIND-02", RECOVERY, "incremental_commit_binding_rejects_each_of_six_identity_fields_before_replay"),
    ("K1-BIND-03", RECOVERY, "incremental_commit_binding_rejects_each_of_six_identity_fields_before_replay"),
    ("K1-BIND-04", RECOVERY, "incremental_commit_binding_rejects_each_of_six_identity_fields_before_replay"),
    ("K1-BIND-05", RECOVERY, "incremental_commit_binding_rejects_each_of_six_identity_fields_before_replay"),
    ("K1-BIND-06", RECOVERY, "incremental_commit_binding_rejects_each_of_six_identity_fields_before_replay"),
    ("K1-CHAIN-01", RECOVERY, "outer_chain_binding_rejects_each_of_nine_fields_before_replay"),
    ("K1-CHAIN-02", RECOVERY, "outer_chain_binding_rejects_each_of_nine_fields_before_replay"),
    ("K1-CHAIN-03", RECOVERY, "outer_chain_binding_rejects_each_of_nine_fields_before_replay"),
    ("K1-CHAIN-04", RECOVERY, "outer_chain_binding_rejects_each_of_nine_fields_before_replay"),
    ("K1-CHAIN-05", RECOVERY, "outer_chain_binding_rejects_each_of_nine_fields_before_replay"),
    ("K1-CHAIN-06", RECOVERY, "outer_chain_binding_rejects_each_of_nine_fields_before_replay"),
    ("K1-CHAIN-07", RECOVERY, "outer_chain_binding_rejects_each_of_nine_fields_before_replay"),
    ("K1-CHAIN-08", RECOVERY, "outer_chain_binding_rejects_each_of_nine_fields_before_replay"),
    ("K1-CHAIN-09", RECOVERY, "outer_chain_binding_rejects_each_of_nine_fields_before_replay"),
    ("K1-IDX-01", RECOVERY, "authenticated_layout_replay_emits_exact_private_index_and_enforces_reuse_rules"),
    ("K1-IDX-02", RECOVERY, "authenticated_layout_replay_emits_exact_private_index_and_enforces_reuse_rules"),
    ("K1-IDX-03", RECOVERY, "authenticated_layout_replay_emits_exact_private_index_and_enforces_reuse_rules"),
    ("K1-TR-01", RECOVERY, "layout_transcript_and_replay_accept_iff_structurally_identical"),
    ("K1-TR-02", RECOVERY, "layout_transcript_and_replay_accept_iff_structurally_identical"),
    ("K1-TR-03", RECOVERY, "layout_transcript_and_replay_accept_iff_structurally_identical"),
    ("K1-SRC-01", RECOVERY, "layout_transcript_and_replay_accept_iff_structurally_identical"),
    ("K1-AUTH-01", RECOVERY, "no_validation_failure_exposes_recovery_authority_or_mutates_durable_index"),
    ("K1-AUTH-02", RECOVERY, "no_validation_failure_exposes_recovery_authority_or_mutates_durable_index"),
    ("K1-EQV-01", RECOVERY, "kind1_streaming_recovery_is_a_complete_structured_differential_of_contiguous_oracle"),
    ("K1-EQV-02", RECOVERY, "kind1_streaming_recovery_is_a_complete_structured_differential_of_contiguous_oracle"),
    ("K1-EQV-03", RECOVERY, "kind1_streaming_recovery_is_a_complete_structured_differential_of_contiguous_oracle"),
    ("K1-MEM-01", RECOVERY, "kind1_decoder_owned_heap_has_zero_input_slope"),
    ("K1-MEM-02", RECOVERY, "kind1_native_stack_high_water_has_zero_input_slope"),
];

fn k1_id(token: &str) -> bool {
    let mut parts = token.split('-');
    matches!(parts.next(), Some("K1")) &&
        matches!(
            parts.next(),
            Some(
                "WIR" |
                    "CUR" |
                    "ARITH" |
                    "PF" |
                    "LIM" |
                    "ACC" |
                    "CRC" |
                    "DIG" |
                    "MUT" |
                    "OBJ" |
                    "COM" |
                    "BIND" |
                    "CHAIN" |
                    "IDX" |
                    "TR" |
                    "SRC" |
                    "AUTH" |
                    "EQV" |
                    "MEM"
            )
        ) &&
        parts.next().is_some_and(|index| {
            index.len() == 2 && index.bytes().all(|byte| byte.is_ascii_digit())
        }) &&
        parts.next().is_none()
}

#[test]
fn kind1_streaming_property_manifest_is_unique_exhaustive_and_formally_derived() {
    let bindings = K1_PROPERTY_BINDINGS
        .iter()
        .map(|(id, source, target)| (*id, (*source, *target)))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(bindings.len(), 89, "the formal catalog contains exactly 89 K1 invariants");
    assert_eq!(bindings.len(), K1_PROPERTY_BINDINGS.len(), "duplicate K1 property ID");
    assert_eq!(
        bindings.values().copied().collect::<BTreeSet<_>>().len(),
        35,
        "the 89 formal invariants refine into exactly 35 executable property families",
    );

    let catalog = include_str!(
        "../../../docs/design/pathmap/incremental-prepare-streaming-invariant-catalog-2026-08-28.\
         md"
    );
    let catalog_ids = catalog
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
        .filter(|token| k1_id(token))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        bindings.keys().copied().collect::<BTreeSet<_>>(),
        catalog_ids,
        "formal catalog and executable K1 property IDs diverged",
    );

    let tla_artifacts = [
        (
            include_str!(
                "../../../formal/tlaplus/stack_safe_pda/RSpaceIncrementalPrepareStreaming.tla"
            ),
            include_str!(
                "../../../formal/tlaplus/stack_safe_pda/RSpaceIncrementalPrepareStreaming.cfg"
            ),
        ),
        (
            include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceStreamingCodecPDA.tla"),
            include_str!("../../../formal/tlaplus/stack_safe_pda/RSpaceStreamingCodecPDA.cfg"),
        ),
    ];
    for (tla_source, tla_config) in tla_artifacts {
        let mut in_invariants = false;
        let invariants = tla_config
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line == "INVARIANTS" {
                    in_invariants = true;
                    return None;
                }
                if in_invariants && line.is_empty() {
                    in_invariants = false;
                    return None;
                }
                line.strip_prefix("INVARIANT ").or_else(|| {
                    in_invariants
                        .then_some(line)
                        .filter(|line| !line.is_empty())
                })
            })
            .collect::<Vec<_>>();
        assert!(!invariants.is_empty(), "TLA+ base config contains no invariants");
        for invariant in invariants {
            assert!(
                tla_source.contains(&format!("{invariant} ==")),
                "base config cites missing TLA+ invariant {invariant}",
            );
            assert!(
                catalog.contains(&format!("`{invariant}`")),
                "TLA+ invariant {invariant} is absent from the extracted catalog",
            );
        }
    }

    let rocq_sources = [
        include_str!("../../../formal/rocq/stack_safe_pda/theories/IncrementalPrepareStreaming.v"),
        include_str!("../../../formal/rocq/stack_safe_pda/theories/StreamingCodecPDA.v"),
    ];
    for rocq_source in rocq_sources {
        for capstone in rocq_source.lines().filter_map(|line| {
            line.trim()
                .strip_prefix("Print Assumptions ")
                .map(|symbol| symbol.trim_end_matches('.'))
        }) {
            assert!(
                catalog.contains(&format!("`{capstone}`")),
                "Rocq capstone {capstone} is absent from the extracted catalog",
            );
        }
    }

    let verus_sources = [
        include_str!("../../../formal/verus/incremental_prepare_streaming.rs"),
        include_str!("../../../formal/verus/streaming_codec_pda.rs"),
    ];
    for verus_source in verus_sources {
        for obligation in verus_source.lines().filter_map(|line| {
            line.trim()
                .strip_prefix("pub proof fn ")
                .or_else(|| line.trim().strip_prefix("pub fn "))
                .and_then(|tail| tail.split('(').next())
        }) {
            assert!(
                catalog.contains(&format!("`{obligation}`")),
                "Verus obligation {obligation} is absent from the extracted catalog",
            );
        }
    }

    let formal_gate = include_str!("../../../scripts/check-stack-safe-pda-proofs.sh");
    assert!(formal_gate.contains("theories/IncrementalPrepareStreaming.v"));
    assert!(formal_gate.contains("theories/StreamingCodecPDA.v"));
    assert!(formal_gate.contains("formal/verus/incremental_prepare_streaming.rs"));
    assert!(formal_gate.contains("formal/verus/streaming_codec_pda.rs"));
    assert!(formal_gate.contains("RSpaceIncrementalPrepareStreaming.cfg"));
    assert!(formal_gate.contains("RSpaceStreamingCodecPDA.cfg"));

    let executable_sources = [
        (WIRE, include_str!("kind1_wire_protocol_properties.rs")),
        (RHO_PATH, include_str!("rho_path_streaming_properties.rs")),
        (MUTATION, include_str!("mutation_streaming_properties.rs")),
        (OBJECT, include_str!("canonical_object_streaming_properties.rs")),
        (RECOVERY, include_str!("kind1_recovery_protocol_properties.rs")),
    ]
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    let registered_modules = include_str!("mod.rs");
    for source in executable_sources.keys() {
        let module = source.strip_suffix(".rs").unwrap();
        assert!(
            registered_modules.contains(&format!("mod {module};")),
            "formal-derived property module {source} is not compiled",
        );
    }
    for (id, source, target) in K1_PROPERTY_BINDINGS {
        let source_text = executable_sources
            .get(source)
            .unwrap_or_else(|| panic!("K1 property {id} names unknown source {source}"));
        let marker = format!("fn {target}(");
        let function_offset = source_text
            .find(&marker)
            .unwrap_or_else(|| panic!("K1 property {id} names missing executable target {target}"));
        let attribute_offset = source_text[..function_offset]
            .rfind("#[test]")
            .unwrap_or_else(|| panic!("K1 property {id} target {target} is not a test"));
        assert!(
            !source_text[attribute_offset..function_offset].contains("#[ignore]"),
            "K1 property {id} target {target} is ignored",
        );
    }
}

fn independent_layout_digest(
    entries: &[(LogPackObjectDispositionV1, ObjectRefV1, u64, Option<u64>, u64)],
    final_extent: u64,
) -> [u8; 32] {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(b"RSpaceK1LayoutTranscriptV1");
    let mut data_count = 0u64;
    let mut logical_object_bytes = 0u64;
    for (ordinal, (disposition, reference, record, canonical, end)) in entries.iter().enumerate() {
        hasher.update((ordinal as u64).to_le_bytes());
        hasher.update(disposition.kind().to_le_bytes());
        hasher.update(reference.digest().as_bytes());
        hasher.update(reference.encoded_len().to_le_bytes());
        hasher.update(record.to_le_bytes());
        match canonical {
            Some(offset) => {
                hasher.update([1]);
                hasher.update(offset.to_le_bytes());
                data_count += 1;
            }
            None => {
                hasher.update([0]);
                hasher.update(0u64.to_le_bytes());
            }
        }
        hasher.update(end.to_le_bytes());
        logical_object_bytes += reference.encoded_len();
    }
    hasher.update((entries.len() as u64).to_le_bytes());
    hasher.update(data_count.to_le_bytes());
    hasher.update(logical_object_bytes.to_le_bytes());
    hasher.update(final_extent.to_le_bytes());
    hasher.finalize().into()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn layout_transcript_matches_an_independent_domain_separated_fold(
        dispositions_and_lengths in proptest::collection::vec((any::<bool>(), 32_u64..8192), 0..64),
        seed in any::<u8>(),
    ) {
        let mut transcript = IncrementalLayoutTranscriptV1::new();
        let mut entries = Vec::with_capacity(dispositions_and_lengths.len());
        let mut cursor = 0u64;
        let mut data_count = 0u64;
        let mut logical_object_bytes = 0u64;
        for (ordinal, (is_data, encoded_len)) in
            dispositions_and_lengths.into_iter().enumerate()
        {
            let mut digest = [seed; 32];
            digest[..8].copy_from_slice(&(ordinal as u64).to_le_bytes());
            let reference = ObjectRefV1::new(ObjectDigest::from_array(digest), encoded_len).unwrap();
            let disposition = if is_data {
                LogPackObjectDispositionV1::Data
            } else {
                LogPackObjectDispositionV1::Reuse
            };
            let record_offset = cursor;
            cursor += 48;
            let canonical_offset = is_data.then_some(cursor);
            if is_data {
                cursor += encoded_len;
                data_count += 1;
            }
            transcript
                .push(disposition, reference, record_offset, canonical_offset, cursor)
                .unwrap();
            entries.push((disposition, reference, record_offset, canonical_offset, cursor));
            logical_object_bytes += encoded_len;
        }
        let summary = transcript.finish(cursor);
        prop_assert_eq!(summary.object_count, entries.len() as u64);
        prop_assert_eq!(summary.data_count, data_count);
        prop_assert_eq!(summary.logical_object_bytes, logical_object_bytes);
        prop_assert_eq!(summary.relative_final_extent, cursor);
        prop_assert_eq!(
            *summary.digest.as_bytes(),
            independent_layout_digest(&entries, cursor),
        );
    }
}

#[test]
fn layout_transcript_rejects_every_typed_layout_substitution_by_identity() {
    let first = ObjectRefV1::new(ObjectDigest::from_array([1; 32]), 64).unwrap();
    let second = ObjectRefV1::new(ObjectDigest::from_array([2; 32]), 96).unwrap();
    let fold = |entries: &[(LogPackObjectDispositionV1, ObjectRefV1, u64, Option<u64>, u64)],
                final_extent| {
        let mut transcript = IncrementalLayoutTranscriptV1::new();
        for entry in entries {
            transcript
                .push(entry.0, entry.1, entry.2, entry.3, entry.4)
                .unwrap();
        }
        transcript.finish(final_extent).digest
    };
    let base = [
        (LogPackObjectDispositionV1::Data, first, 0, Some(48), 112),
        (LogPackObjectDispositionV1::Reuse, second, 112, None, 160),
    ];
    let expected = fold(&base, 160);
    let variants = [
        [base[1], base[0]],
        [(LogPackObjectDispositionV1::Reuse, first, 0, None, 48), base[1]],
        [base[0], (base[1].0, first, base[1].2, base[1].3, base[1].4)],
        [base[0], (base[1].0, base[1].1, 113, base[1].3, base[1].4)],
        [base[0], (base[1].0, base[1].1, base[1].2, base[1].3, 161)],
    ];
    for variant in variants {
        assert_ne!(fold(&variant, 160), expected);
    }
    assert_ne!(fold(&base, 161), expected);
}
