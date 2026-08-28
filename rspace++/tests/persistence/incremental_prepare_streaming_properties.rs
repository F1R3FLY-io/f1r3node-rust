use std::collections::{BTreeMap, BTreeSet};

const K1_PROPERTY_BINDINGS: &[(&str, &str)] = &[
    ("K1-WIR-01", "canonical_v1_prepare_and_commit_images_are_frozen"),
    ("K1-WIR-02", "prepare_discriminator_routes_only_exact_known_fixed_headers"),
    ("K1-CUR-01", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-CUR-02", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-CUR-03", "prepare_rejects_every_truncation_without_panicking"),
    ("K1-CUR-04", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-ARITH-01", "index_identity_counters_and_object_extent_overflow_are_checked"),
    ("K1-PF-01", "prepare_prefix_is_authenticated_and_bounded_before_variable_size_reads"),
    ("K1-PF-02", "range_and_object_work_are_admitted_before_open_or_variable_body_read"),
    ("K1-LIM-01", "declared_resource_use_is_bounded_before_payload_parsing"),
    ("K1-LIM-02", "declared_resource_use_is_bounded_before_payload_parsing"),
    ("K1-LIM-03", "declared_resource_use_is_bounded_before_payload_parsing"),
    ("K1-LIM-04", "declared_resource_use_is_bounded_before_payload_parsing"),
    ("K1-LIM-05", "single_object_limit_applies_to_reuse_references_as_well_as_inline_data"),
    ("K1-LIM-06", "decoder_enforces_total_resources_without_a_depth_limit"),
    ("K1-LIM-07", "decoder_enforces_total_resources_without_a_depth_limit"),
    ("K1-LIM-08", "range_and_object_work_are_admitted_before_open_or_variable_body_read"),
    ("K1-ACC-01", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-ACC-02", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-ACC-03", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-CRC-01", "persisted_checksums_reject_nonzero_high_u64_bits"),
    ("K1-CRC-02", "checksums_reject_header_and_payload_corruption"),
    ("K1-CRC-03", "persisted_checksums_reject_nonzero_high_u64_bits"),
    ("K1-CRC-04", "checksums_reject_header_and_payload_corruption"),
    ("K1-CRC-05", "checksums_reject_header_and_payload_corruption"),
    ("K1-DIG-01", "prepare_stream_round_trips_without_projecting_objects"),
    ("K1-DIG-02", "valid_commit_images_cannot_rebind_prepare_fields"),
    ("K1-MUT-01", "decoder_rejects_header_reference_and_presence_corruption"),
    ("K1-MUT-02", "decoder_rejects_noncontiguous_generation_and_wrong_section_order"),
    ("K1-MUT-03", "decoder_rejects_noncontiguous_generation_and_wrong_section_order"),
    ("K1-MUT-04", "decoder_enforces_total_resources_without_a_depth_limit"),
    ("K1-MUT-05", "decoder_enforces_total_resources_without_a_depth_limit"),
    ("K1-MUT-06", "decoder_enforces_total_resources_without_a_depth_limit"),
    ("K1-MUT-07", "decoder_rejects_noop_unsorted_and_malformed_keys"),
    ("K1-MUT-08", "decoder_rejects_noop_unsorted_and_malformed_keys"),
    ("K1-MUT-09", "hundred_thousand_byte_key_round_trips_on_a_small_native_stack"),
    ("K1-MUT-10", "decoder_rejects_noop_unsorted_and_malformed_keys"),
    ("K1-MUT-11", "decoder_rejects_header_reference_and_presence_corruption"),
    ("K1-MUT-12", "decoder_rejects_noop_unsorted_and_malformed_keys"),
    ("K1-MUT-13", "generation_overflow_and_trailing_bytes_fail_closed"),
    ("K1-MUT-14", "streaming_plan_propagates_sink_failure_and_has_a_payload_digest"),
    ("K1-MUT-15", "normalized_batch_round_trips_all_three_semantic_sections"),
    ("K1-OBJ-01", "prepared_layouts_are_the_single_source_for_parsed_object_offsets"),
    ("K1-OBJ-02", "prepare_rejects_every_truncation_without_panicking"),
    ("K1-OBJ-03", "prepared_layouts_are_the_single_source_for_parsed_object_offsets"),
    ("K1-OBJ-04", "prepared_layouts_are_the_single_source_for_parsed_object_offsets"),
    ("K1-OBJ-05", "single_object_limit_applies_to_reuse_references_as_well_as_inline_data"),
    ("K1-OBJ-06", "prepared_layouts_are_the_single_source_for_parsed_object_offsets"),
    ("K1-OBJ-07", "warm_store_reuse_omits_only_the_equal_body_and_collision_prevents_preflight"),
    ("K1-OBJ-08", "malformed_envelopes_endpoints_and_branches_fail_closed"),
    ("K1-OBJ-09", "malformed_child_tables_and_noncompressed_nodes_fail_closed"),
    ("K1-OBJ-10", "prefix_and_blob_payload_views_borrow_the_original_bytes"),
    ("K1-OBJ-11", "arbitrary_checkpoint_wire_bytes_are_total"),
    ("K1-OBJ-12", "arbitrary_checkpoint_wire_bytes_are_total"),
    ("K1-OBJ-13", "arbitrary_checkpoint_wire_bytes_are_total"),
    ("K1-OBJ-14", "warm_store_reuse_omits_only_the_equal_body_and_collision_prevents_preflight"),
    ("K1-OBJ-15", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-OBJ-16", "prepare_rejects_every_truncation_without_panicking"),
    ("K1-COM-01", "incremental_parser_reports_checkpoint_kind_without_trusting_its_extents"),
    ("K1-COM-02", "commit_has_a_fixed_portable_image_and_rejects_corruption"),
    ("K1-BIND-01", "valid_commit_images_cannot_rebind_prepare_fields"),
    ("K1-BIND-02", "valid_commit_images_cannot_rebind_prepare_fields"),
    ("K1-BIND-03", "valid_commit_images_cannot_rebind_prepare_fields"),
    ("K1-BIND-04", "valid_commit_images_cannot_rebind_prepare_fields"),
    ("K1-BIND-05", "valid_commit_images_cannot_rebind_prepare_fields"),
    ("K1-BIND-06", "valid_commit_images_cannot_rebind_prepare_fields"),
    ("K1-CHAIN-01", "cross_pack_sequence_state_and_previous_commit_discontinuities_fail_closed"),
    ("K1-CHAIN-02", "terminal_publication_digest_and_address_mismatches_fail_before_authority"),
    ("K1-CHAIN-03", "cross_pack_sequence_state_and_previous_commit_discontinuities_fail_closed"),
    ("K1-CHAIN-04", "cross_pack_sequence_state_and_previous_commit_discontinuities_fail_closed"),
    ("K1-CHAIN-05", "cross_pack_sequence_state_and_previous_commit_discontinuities_fail_closed"),
    ("K1-CHAIN-06", "terminal_publication_digest_and_address_mismatches_fail_before_authority"),
    ("K1-CHAIN-07", "terminal_publication_digest_and_address_mismatches_fail_before_authority"),
    ("K1-CHAIN-08", "terminal_publication_digest_and_address_mismatches_fail_before_authority"),
    ("K1-CHAIN-09", "terminal_publication_digest_and_address_mismatches_fail_before_authority"),
    ("K1-IDX-01", "prepared_data_layouts_create_exact_locations_while_reuse_creates_none"),
    ("K1-IDX-02", "warm_store_reuse_omits_only_the_equal_body_and_collision_prevents_preflight"),
    ("K1-IDX-03", "generation_checkpoint_reuse_and_short_physical_pack_fail_closed"),
    ("K1-TR-01", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-TR-02", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-TR-03", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-SRC-01", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-AUTH-01", "exact_catalog_image_is_consumed_before_recovered_authority_is_exposed"),
    ("K1-AUTH-02", "range_and_object_work_are_admitted_before_open_or_variable_body_read"),
    ("K1-EQV-01", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-EQV-02", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-EQV-03", "incremental_prepare_recovery_is_bounded_and_exact"),
    ("K1-MEM-01", "incremental_prepare_recovery_never_requests_a_record_sized_buffer"),
    ("K1-MEM-02", "hundred_thousand_byte_transition_streams_and_validates_on_a_small_stack"),
];

fn k1_id(token: &str) -> bool {
    let mut parts = token.split('-');
    matches!(parts.next(), Some("K1")) &&
        matches!(
            parts.next(),
            Some(
                "WIR" | "CUR" | "ARITH" | "PF" | "LIM" | "ACC" | "CRC" | "DIG" |
                    "MUT" | "OBJ" | "COM" | "BIND" | "CHAIN" | "IDX" | "TR" | "SRC" |
                    "AUTH" | "EQV" | "MEM"
            )
        ) &&
        parts
            .next()
            .is_some_and(|index| index.len() == 2 && index.bytes().all(|byte| byte.is_ascii_digit())) &&
        parts.next().is_none()
}

#[test]
fn kind1_streaming_property_manifest_is_unique_exhaustive_and_formally_derived() {
    let bindings = K1_PROPERTY_BINDINGS.iter().copied().collect::<BTreeMap<_, _>>();
    assert_eq!(bindings.len(), 89, "the formal catalog contains exactly 89 K1 invariants");
    assert_eq!(bindings.len(), K1_PROPERTY_BINDINGS.len(), "duplicate K1 property ID");

    let catalog = include_str!(
        "../../../docs/design/pathmap/incremental-prepare-streaming-invariant-catalog-2026-08-28.md"
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
        for invariant in
            tla_config.lines().filter_map(|line| line.trim().strip_prefix("INVARIANT "))
        {
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
        include_str!("incremental_prepare_streaming_properties.rs"),
        include_str!("durable_recovery.rs"),
        include_str!("generation_checkpoint_properties.rs"),
        include_str!("index_codec.rs"),
        include_str!("mutation_codec.rs"),
        include_str!("object_codec.rs"),
        include_str!("wal_codec.rs"),
    ]
    .concat();
    for (id, target) in K1_PROPERTY_BINDINGS {
        assert!(
            executable_sources.contains(&format!("fn {target}(")),
            "K1 property {id} names missing executable target {target}",
        );
    }
}
