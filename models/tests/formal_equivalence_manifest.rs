//! Mechanical closure gate for the generated PDA equivalence argument.
//!
//! The schema generator owns the complete traversal registry. The hand-written
//! evidence table owns the theorem and executable-oracle binding. This test
//! compares those independent inventories in both directions and then verifies
//! that every named file, theorem, and oracle marker exists.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{TokenStream, TokenTree};

#[test]
fn act_cached_summary_has_source_preorder_and_independent_inventory_bindings() {
    let theory =
        read_repository_file("formal/rocq/actree03_completeness/theories/Epm1ValueSummary.v");
    for theorem in [
        "zero_summary_decides_exactly_all_zero_markers",
        "typed_mode_decision_uses_derived_count_and_zero_markers",
        "accumulated_count_equals_valued_observation_count",
        "first_nonzero_matches_independent_find",
        "first_nonsequential_matches_value_ordinal_reference",
        "concrete_summary_refines_independent_inventory",
        "latched_nonzero_diagnostic_is_retained",
        "latched_nonsequential_diagnostic_is_retained",
        "absent_nonzero_is_exactly_all_zero_markers",
        "value_inventory_is_node_bounded",
        "bindings_append",
        "semantic_preorder_markers_match_writer_bindings",
        "certified_summary_markers_are_source_writer_markers",
        "certified_summary_count_fits_resident_image",
        "canonical_zero_writer_summary_has_exact_count_and_zero_markers",
        "admitted_canonical_image_summary_uses_forward_preorder",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    for forbidden in ["Admitted.", "admit.", "Axiom ", "Parameter "] {
        assert!(!theory.contains(forbidden));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/Epm1ValueSummary.v",
        "16",
    ]));
    assert!(
        runner.contains("rocq_check_modules_at '' \"ACT cached value summary\" Epm1ValueSummary")
    );
    assert!(
        read_repository_file("formal/rocq/actree03_completeness/_CoqProject")
            .lines()
            .any(|line| line == "theories/Epm1ValueSummary.v")
    );
    let native = read_repository_file("rspace++/tests/support/pathmap_codec_validated_plan.rs");
    for witness in [
        "cached_value_summary_matches_independent_preorder_offsets_and_latches",
        "arbitrary_pathmap_writer_images_are_accepted_with_exact_preorder_plans",
        "value_summary_retains_preorder_ordinal_diagnostics_without_an_event_prepass",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    for binding in [
        "node.wrapper.unwrap_or(id)",
        "assert_eq!(actual.first_nonzero(), first_nonzero)",
        "assert_eq!(actual.first_nonsequential(), first_nonsequential)",
        "assert!(cursor.source().take_accesses().is_empty())",
        "assert_eq!(cursor.work_counters(), before)",
    ] {
        assert!(native.contains(binding));
    }
    let modes = read_repository_file("models/tests/support/substitution_value_regions.rs");
    assert!(modes.contains("fn mode_admission_uses_logical_topology_and_zero_endpoint_markers("));
    assert!(modes.contains("fn generated_mode_admission_matches_independent_endpoint_inventory("));
}

#[test]
fn substitution_snapshot_prefix_has_exact_offset_and_admission_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionSnapshotPrefix.v");
    for theorem in [
        "fixed_header_has_six_bytes",
        "successful_mode_selects_exact_fixed_header",
        "successful_count_selects_exact_boundary",
        "successful_prefix_has_exact_source_certificate",
        "prefix_reconstructs_exact_envelope_without_value_decode",
        "prefix_offsets_and_count_fit_resident_input",
        "successful_framing_retains_prefix_and_value_selections",
        "complete_framing_reconstructs_exact_epm1_bytes",
        "malformed_prefix_never_publishes_framing",
        "malformed_value_suffix_never_publishes_framing",
        "route_prefix_does_not_validate_topology_or_value_suffix",
        "impossible_count_is_rejected_before_framing",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    for forbidden in ["Admitted.", "admit.", "Axiom ", "Parameter "] {
        assert!(!theory.contains(forbidden));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionSnapshotPrefix.v",
        "12",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionSnapshotPrefix"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionSnapshotPrefix.v")
    );
    let native = read_repository_file("models/tests/support/substitution_value_regions.rs");
    for witness in [
        "prefix_offsets_follow_append_time_oracle_across_varint_widths",
        "malformed_prefixes_fail_before_value_range_publication",
        "route_hints_do_not_bypass_framing_or_topology_validation",
        "generated_prefix_offsets_match_independent_construction",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn epm_mode_model_has_value_free_map_and_set_marker_regressions() {
    let theory = read_repository_file("formal/rocq/stack_safe_pda/theories/EPM1.v");
    for theorem in [
        "associated_set_markers_are_zero_without_a_value_table",
        "zero_count_map_association_requires_no_valued_endpoints",
        "set_association_accepts_any_number_of_zero_markers",
        "set_association_rejects_nonzero_markers",
        "structurally_nonempty_map_may_have_zero_values",
        "structurally_empty_typed_arena_is_rejected",
        "zero_count_map_framed_round_trip",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/EPM1.v",
        "7",
    ]));
    assert!(has_runner_kernel_check(&runner, &["StackSafePDA.EPM1"]));
    let native = read_repository_file("models/tests/support/substitution_value_regions.rs");
    for witness in [
        "value_region_zero_count_preserves_empty_set_and_value_free_map_modes",
        "mode_admission_uses_logical_topology_and_zero_endpoint_markers",
        "generated_mode_admission_matches_independent_endpoint_inventory",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn substitution_value_regions_have_physical_position_and_refusal_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionValueRegions.v");
    for theorem in [
        "suffix_split_gives_exact_offset",
        "single_frame_selects_exact_physical_range",
        "successful_loop_refines_decoder_and_partition",
        "partition_has_exact_count_bound",
        "partition_retains_ordered_in_bounds_spans",
        "span_body_length",
        "partition_has_exact_physical_positions",
        "successful_inspection_decodes_complete_suffix",
        "successful_selection_is_machine_representable",
        "trailing_bytes_refuse_publication",
        "impossible_count_refuses_before_loop",
        "failed_frame_cannot_publish_range_list",
        "duplicate_empty_bodies_have_distinct_positions",
        "truncated_payload_does_not_publish",
        "inspected_prefix_is_in_bounds",
        "completed_loop_satisfies_admission_guard",
        "region_interpretation_has_exact_length",
        "successful_selection_lifts_owned_regions",
        "selected_physical_ranges_establish_region_field_certificate",
        "selected_physical_ranges_preserve_all_retained_provenance",
        "unrepresentable_payload_cannot_be_selected",
        "selected_headers_and_bodies_reconstruct_whole_input",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionValueRegions.v",
        "22",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionValueRegions"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionValueRegions.v")
    );
    let native = read_repository_file("models/tests/support/substitution_value_regions.rs");
    for witness in [
        "value_region_positions_match_construction_across_header_widths_and_duplicates",
        "value_region_zero_count_preserves_empty_set_and_value_free_map_modes",
        "malformed_value_framing_never_publishes_a_partial_range_list",
        "topology_validation_remains_a_separate_post_framing_gate",
        "generated_value_regions_equal_independently_recorded_positions",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    assert!(native.contains("expected.push(start..bytes.len())"));
    assert!(native.contains("assert_eq!(inspection.value_bodies, expected)"));
    let codec = read_repository_file("models/src/rust/epathmap_trie_codec.rs");
    assert!(codec.contains("#[path = \"../../tests/support/substitution_value_regions.rs\"]"));
}

#[test]
fn substitution_region_provenance_has_region_path_and_decoder_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionRegionProvenance.v");
    for theorem in [
        "containment_is_transitive",
        "containment_preserves_region_validity",
        "checked_subregion_is_contained",
        "subregion_bytes_are_parent_interval",
        "path_append_preserves_old_lookup",
        "appended_path_has_exact_index",
        "admitted_parent_is_backward",
        "path_append_preserves_admission",
        "path_chain_has_valid_regions",
        "path_registration_erases_to_identity",
        "par_registration_erases_to_byte_registration",
        "value_registration_erases_to_byte_registration",
        "value_fold_erases_to_sequential_cuts",
        "value_fold_keeps_groups",
        "epathmap_registration_erases_to_byte_registration",
        "path_registration_preserves_cut_links",
        "par_registration_preserves_cut_links",
        "par_registration_preserves_path_chain",
        "group_anchor_is_admitted",
        "value_fold_preserves_provenance",
        "epathmap_registration_preserves_provenance",
        "par_registration_transports_published_arena",
        "epathmap_registration_transports_published_arena",
        "epm_region_framing_establishes_new_field_certificate",
        "inherited_frame_composes_admission",
        "value_fold_has_exact_paths_and_cuts",
        "value_path_count_is_exact",
        "value_cut_count_is_exact",
        "value_path_lookup_is_exact",
        "value_cut_lookup_is_exact",
        "epathmap_has_exact_group_and_value_records",
        "admitted_region_is_valid",
        "epm_prefix_retains_backing_and_bounds",
        "local_value_ranges_interpret_as_parent_slices",
        "source_framing_derives_region_field_certificate",
        "region_backed_registration_preserves_provenance_and_publication",
        "path_registration_preserves_group_links",
        "value_fold_preserves_group_links",
        "epathmap_registration_preserves_group_links",
        "linked_group_prefixes_are_valid",
        "region_registration_preserves_all_retained_provenance",
        "decoded_epm_suffix_establishes_region_field_certificate",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionRegionProvenance.v",
        "42",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionRegionProvenance"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionRegionProvenance.v")
    );
    let native = read_repository_file("models/tests/support/substitution_region_emission.rs");
    for witness in [
        "captured_frame_provenance_covers_dedicated_and_inherited_paths",
        "captured_frame_provenance_rejects_another_valid_earlier_parent",
        "generated_injection_entries_preserve_inherited_frame_provenance",
        "registration_preserves_tables_between_map_set_empty_and_ordinary_cuts",
        "generated_registration_retains_every_value_and_prior_group",
        "registration_controls_reject_shifted_ranges_and_wrong_value_paths",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    assert!(native.contains("assert_eq!(observations[*node].path_leaf, parent.path_leaf)"));
    assert!(native.contains("assert_eq!(path.parent, parent.path_leaf)"));
    assert!(native.contains("assert_eq!(path.parent, Some(group.path_leaf))"));
    assert!(theory.contains("EPM1.decode_frames count (skipn prefix_end (region_bytes body))"));
}

#[test]
fn substitution_region_ordinals_have_machine_bound_and_consumer_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionRegionOrdinals.v");
    for theorem in [
        "initial_state_is_counted",
        "next_ordinal_is_bounded_by_seen_fields",
        "advance_preserves_counted_state",
        "input_bounded_step_cannot_overflow",
        "tag_change_resets_ordinal",
        "positive_tag_run_preserves_count",
        "encoded_varint_is_nonempty",
        "successful_field_has_positive_tag_and_extent",
        "successful_field_trace_bounds_count_and_tags",
        "successful_prefix_makes_next_ordinal_machine_representable",
        "epm_framed_value_count_is_bounded_by_bytes",
        "epm_enumeration_ordinal_is_machine_representable",
        "narrow_counter_is_not_justified_by_wider_input",
        "widening_preserves_successful_steps",
        "paired_cut_indices_are_in_bounds",
        "method_argument_index_is_in_bounds",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionRegionOrdinals.v",
        "16",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionRegionOrdinals"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionRegionOrdinals.v")
    );
    let native = read_repository_file("models/tests/support/substitution_region_emission.rs");
    for witness in [
        "native_occurrence_counter_crosses_u32_boundary_and_resets",
        "synthetic_invalid_counter_retains_checked_overflow_behavior",
        "native_par_cut_paths_preserve_machine_sized_occurrences",
        "occurrence_transition_matches_independent_tag_run_length",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    let region = read_repository_file("models/src/rust/rholang/protobuf_region.rs");
    assert!(region.contains("pub occurrence: usize"));
    assert!(!region.contains("occurrence: u32"));
    assert!(!region.contains("u32::try_from(ordinal)"));
    let printer = read_repository_file("rholang/src/rust/interpreter/pretty_printer.rs");
    let sequences = printer
        .split("fn direct_cut_sequence(")
        .nth(1)
        .unwrap()
        .split("fn descend_encoded_expr")
        .next()
        .unwrap();
    assert!(!sequences.contains("u32::try_from"));
    let matcher =
        read_repository_file("rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs");
    assert!(matcher.contains("bind: usize"));
    assert!(matcher.contains("occurrence: usize"));
    assert!(matcher.contains("#[path = \"../../../../tests/support/region_occurrence.rs\"]"));
    let tests = read_repository_file("rholang/tests/support/region_occurrence.rs");
    for witness in [
        "receive_cut_addresses_retain_full_native_ordinals",
        "receive_pairing_does_not_alias_ordinals_at_u32_boundary",
    ] {
        assert!(tests.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn substitution_region_tables_have_registration_and_publication_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionRegionTables.v");
    for theorem in [
        "extension_preserves_cut_lookup",
        "extension_preserves_group_lookup",
        "extension_preserves_support",
        "group_range_bounded",
        "extension_preserves_group_body",
        "extension_preserves_piece_bytes",
        "extension_preserves_source_piece",
        "extension_preserves_field_certificates",
        "extension_preserves_arena_validity",
        "cut_registration_extends",
        "cut_registration_preserves_well_formed",
        "cut_registration_has_exact_new_ordinal",
        "group_registration_extends",
        "group_registration_preserves_well_formed",
        "group_registration_has_exact_descriptor",
        "suffix_indices_select_values",
        "group_registration_preserves_exact_value_order",
        "group_registration_reconstructs_original_payload",
        "registration_establishes_piece_support",
        "cut_registration_establishes_field_certificate",
        "group_registration_establishes_field_certificate",
        "empty_tables_are_well_formed",
        "sequential_cuts_equal_batch",
        "publication_preserves_piece_support",
        "sequential_group_publication_equals_registration",
        "empty_table_arena_is_valid",
        "extension_transports_table_arena",
        "cut_registration_transports_table_arena",
        "group_registration_transports_table_arena",
        "checked_publication_preserves_table_arena",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionRegionTables.v",
        "30",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionRegionTables"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionRegionTables.v")
    );
    let native = read_repository_file("models/tests/support/substitution_region_emission.rs");
    for witness in [
        "registration_preserves_tables_between_map_set_empty_and_ordinary_cuts",
        "registration_controls_reject_shifted_ranges_and_wrong_value_paths",
        "generated_registration_retains_every_value_and_prior_group",
        "every_published_node_preserves_source_caches_indices_and_prior_meaning",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    assert!(native.contains("tables: RegisteredTables::capture(builder)"));
    assert!(contains_rust_token_sequence(
        &native,
        "observed.tables.assert_preserved(&plan.paths, &plan.cuts, &plan.epathmaps)"
    ));
}

#[test]
fn substitution_varint_accumulator_has_bitwise_proof_and_native_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionVarintAccumulator.v");
    for theorem in [
        "low_accumulator_and_shifted_digit_are_disjoint",
        "masked_shift_or_equals_radix_accumulation",
        "forward_step_preserves_accumulator_invariant",
        "forward_scanner_refines_group_decomposition",
        "whole_forward_scanner_equals_group_scanner",
        "successful_forward_scan_reencodes_consumed_bytes",
        "all_native_shifts_are_below_word_width",
        "tenth_group_guard_preserves_u64_bound",
        "terminal_width_equals_nonzero_high_digit",
        "forward_scanner_accepts_full_u64",
        "forward_scanner_refuses_tenth_byte_two",
        "forward_scanner_refuses_redundant_terminal",
        "width_formula_for_all_u64_logarithms",
        "prost_width_is_logarithmic_radix_width",
        "radix_quotient_interval",
        "prost_width_characterizes_canonical_range",
        "terminal_prost_width_equals_nonzero_high_digit",
        "width_checked_loop_equals_terminal_checked_loop",
        "both_scanners_return_same_value_and_suffix",
        "successful_epm_forward_scan_reencodes_consumed_bytes",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionVarintAccumulator.v",
        "20",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionVarintAccumulator"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionVarintAccumulator.v")
    );
    let native = read_repository_file("models/tests/support/substitution_varint.rs");
    for witness in [
        "both_varint_scanners_cover_every_terminal_byte_and_group_position",
        "both_varint_scanners_cover_u64_boundaries_without_consuming_suffix",
        "malformed_varints_preserve_each_scanners_error_class_and_position",
        "both_varint_scanners_match_widened_reference_on_arbitrary_bytes",
        "both_varint_scanners_preserve_generated_values_and_offsets",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    assert!(read_repository_file("models/src/rust/epathmap_trie_codec.rs")
        .contains("#[cfg(test)]\n#[path = \"../../tests/support/substitution_varint.rs\"]\nmod substitution_varint_tests;"));
    assert!(read_repository_file("models/src/rust/rholang/protobuf_cursor.rs")
        .contains("#[cfg(test)]\n#[path = \"../../../tests/support/protobuf_varint_access.rs\"]\npub(crate) mod varint_test_access;"));
}

#[test]
fn substitution_region_decomposition_has_source_certificate_bindings() {
    let slots = read_repository_file("rholang/tests/support/substitution_region_slots.rs");
    for witness in [
        "dropped_and_synthetic_slots_are_not_a_permutation_of_retained_cuts",
        "region_cut_cursor_rejects_unconsumed_and_exhausted_plans",
        "generated_slot_selection_preserves_prefix_identity_and_semantic_order",
    ] {
        assert!(slots.contains(&format!("fn {witness}(")));
    }
    assert!(read_repository_file("rholang/src/rust/interpreter/substitute_drive.rs")
        .contains("#[cfg(test)]\n#[path = \"../../../tests/support/substitution_region_slots.rs\"]\nmod region_slot_tests;"));
    let theory = read_repository_file(
        "formal/rocq/stack_safe_pda/theories/SubstitutionRegionDecomposition.v",
    );
    for theorem in [
        "taken_body_splits_source",
        "successful_body_scan_reconstructs_source",
        "successful_field_scan_reconstructs_source",
        "successful_delimited_field_has_exact_header",
        "initial_frame_has_source_invariant",
        "borrowed_field_establishes_certificate",
        "par_field_establishes_certificate",
        "ready_field_consumption_preserves_source",
        "child_descent_preserves_source",
        "published_child_closes_suspended_field",
        "completed_frame_supplies_publication_certificate",
        "completed_frame_reconstructs_original",
        "original_value_frames_are_encoded_frames",
        "epathmap_field_establishes_certificate",
        "prior_piece_source_survives_publication",
        "prior_certificate_survives_publication",
        "suspended_parent_survives_child_publication",
        "completed_frame_publishes_valid_arena",
        "empty_delimited_field_is_present",
        "truncated_delimited_field_is_refused",
        "unknown_wire_kind_is_refused",
        "unknown_field_borrows_without_singular_check",
        "wrong_wire_precedes_slot_and_shape_checks",
        "duplicate_slot_precedes_shape_check",
        "accepted_dispatch_has_exact_generated_action",
        "recursive_dispatch_cannot_be_replaced_by_borrowing",
        "optional_and_oneof_children_share_presence",
        "nonpar_child_presence_is_exact",
        "dispatched_piece_preserves_source_and_action",
        "selection_consumes_exact_prefix",
        "selection_job_count_includes_synthetic_excludes_dropped",
        "finished_selection_exhausts_cuts",
        "reverse_push_executes_children_before_continuation",
        "selection_is_not_a_permutation_of_cuts",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionRegionDecomposition.v",
        "34",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionRegionDecomposition"
    ]));
    assert!(has_runner_command(&runner, &[
        "TLC_MEMORY_HIGH=4G",
        "TLC_RSS=6G",
        "TLC_CPU_QUOTA=400%",
        "TLC_TASKS_MAX=512",
        "rpb3_run_tests",
        "23",
        "--release",
        "-j4",
        "--lib",
        "emission_tests",
    ]));
    assert!(has_runner_command(&runner, &[
        "rholang_run_tests",
        "9",
        "--lib",
        "region_slot_tests",
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionRegionDecomposition.v")
    );
    let native = read_repository_file("models/tests/support/substitution_region_emission.rs");
    for witness in [
        "verify_node_field_certificates",
        "every_published_node_preserves_source_caches_indices_and_prior_meaning",
        "epathmap_publication_frames_include_empty_and_distinct_map_values",
        "field_certificate_rejects_swapped_same_tag_par_cuts",
        "every_generated_field_and_oneof_arm_has_exact_dispatch_and_presence",
        "dispatch_certificate_rejects_byte_identical_borrow_and_retention_mutants",
        "dispatch_refusal_precedence_and_shared_oneof_slots_never_return_a_root",
        "repeated_unknown_fields_do_not_claim_a_known_singular_slot",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn substitution_region_framing_has_reconstruction_proof_and_native_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionRegionFraming.v");
    for theorem in [
        "successful_varint_reencodes_consumed_prefix",
        "successful_frame_reencodes_consumed_prefix",
        "successful_frames_reencode_consumed_prefix",
        "epm_value_frames_reconstruct_complete_suffix",
        "adjacent_fields_bounds",
        "interval_then_suffix",
        "successful_fields_partition_consumed_suffix",
        "eof_completes_source_partition",
        "successful_fields_make_bounded_progress",
        "framed_empty_values_are_not_omitted",
        "wrong_same_length_value_is_not_reconstruction",
        "truncated_frame_is_not_a_partition",
        "successful_groups_refine_decoder",
        "successful_bounded_varint_reencodes_consumed_prefix",
        "successful_groups_consume_at_most_budget",
        "bounded_scanner_accepts_multibyte_value",
        "bounded_scanner_rejects_redundant_terminal",
        "bounded_scanner_rejects_tenth_byte_overflow",
        "bounded_scanner_rejects_eleventh_group",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionRegionFraming.v",
        "19",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionRegionFraming"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionRegionFraming.v")
    );
    let native = read_repository_file("models/tests/protobuf_cursor.rs");
    for witness in [
        "canonical_varint_boundaries_reconstruct_exact_field_bytes",
        "owned_cursor_refuses_and_retries_malformed_fields_without_consuming_them",
        "arbitrary_successful_fields_partition_and_reencode_source",
        "generated_fields_partition_source_in_numeric_and_producer_order",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    assert!(
        read_repository_file("models/tests/support/substitution_region_emission.rs")
            .contains("fn verify_epathmap_value_frame_partition(")
    );
    assert!(
        read_repository_file("models/tests/support/substitution_region_emission.rs")
            .contains("fn epathmap_publication_frames_include_empty_and_distinct_map_values(")
    );
}

#[test]
fn substitution_region_publication_has_derived_invariants_and_native_capture() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionRegionPublication.v");
    for theorem in [
        "empty_is_valid",
        "publication_preserves_old_indices",
        "publication_gives_exact_new_index",
        "prior_piece_denotation_stable",
        "certified_fields_reconstruct_original",
        "append_establishes_all_arena_invariants",
        "successful_checked_publication_establishes_invariants",
        "publication_checks_original_length",
        "length_mismatch_refuses_publication",
        "arithmetic_failure_refuses_publication",
        "valid_arena_supplies_node_rank",
        "valid_arena_supplies_piece_rank",
        "valid_published_root_emits_its_original_input",
        "constructed_arena_is_valid",
        "publication_increases_count_once",
        "constructed_root_emits_all_modes_with_exact_cached_length",
        "constructed_published_root_reconstructs_certified_source",
        "two_node_publication_is_constructed",
        "same_length_wrong_source_passes_only_the_length_check",
        "self_reference_is_not_an_admissible_publication",
        "unrepresentable_publication_is_refused",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionRegionPublication.v",
        "21",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionRegionPublication"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionRegionPublication.v")
    );
    let native = read_repository_file("models/tests/support/substitution_region_emission.rs");
    for witness in [
        "every_published_node_preserves_source_caches_indices_and_prior_meaning",
        "generated_publications_preserve_actual_frame_bytes",
        "late_framing_failure_never_publishes_the_root",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    let source = read_repository_file("models/src/rust/rholang/protobuf_region.rs");
    assert!(contains_rust_token_sequence(
        &source,
        "builder.nodes.push(node); #[cfg(test)] emission_tests::record_publication(node_index, &frame.region, frame.schema, frame.path_leaf, &builder);"
    ));
    assert!(read_repository_file("models/src/lib.rs")
        .contains("#[cfg(test)]\nextern crate self as models;"));
}

#[test]
fn substitution_region_emission_has_conditional_proof_and_native_bindings() {
    let arithmetic = read_repository_file("models/tests/support/substitution_region_emission.rs");
    for witness in [
        "checked_add_matches_machine_word_arithmetic",
        "delimited_length_matches_widened_arithmetic",
        "delimited_length_covers_key_and_payload_width_transitions",
        "overflow_in_each_cached_mode_refuses_finished_node_publication",
    ] {
        assert!(arithmetic.contains(&format!("fn {witness}(")));
    }
    assert!(read_repository_file("models/src/rust/rholang/protobuf_region.rs").contains(
        "#[cfg(test)]\n#[path = \"../../../tests/support/substitution_region_emission.rs\"]\nmod emission_tests;"
    ));
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionRegionEmission.v");
    for theorem in [
        "emit_step_preserves_ordered_pending_bytes",
        "execution_preserves_ordered_pending_bytes",
        "worklist_emission_equals_mode_reference",
        "reverse_push_visits_pieces_in_source_order",
        "every_emit_step_decreases_rank",
        "ranked_emission_terminates_without_fuel",
        "compact_preserves_required_empty_envelopes",
        "compact_retains_genuinely_empty_repeated_messages",
        "compact_omits_only_cut_emptied_unrequired_envelopes",
        "original_par_keeps_present_empty_header",
        "epm_shallow_and_compact_preserve_value_count",
        "epm_original_value_order",
        "checked_add_success_exact",
        "checked_add_refuses_overflow",
        "checked_total_success_exact",
        "checked_total_complete",
        "checked_total_refuses_unrepresentable_nonempty",
        "dropping_genuine_empty_envelope_is_observable",
        "ignoring_presence_flag_is_observable",
        "reversed_value_order_is_observable",
        "missing_empty_value_is_observable",
        "unchecked_wrap_is_observable",
        "checked_delimited_success_exact",
        "checked_delimited_refuses_u64_overflow",
        "checked_frames_success_exact",
        "planned_piece_success_has_exact_mode_length",
        "finished_node_success_has_exact_mode_length",
        "finished_node_success_is_representable",
        "failed_mode_does_not_publish",
        "fixture_reference_is_sound",
        "fixture_ranks_are_valid",
        "fixture_emits_all_modes_without_fuel",
        "fixture_planner_accepts_all_modes",
        "reversed_neighbor_pieces_are_observable",
        "stale_child_cache_is_observable",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionRegionEmission.v",
        "35",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionRegionEmission"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionRegionEmission.v")
    );
    let native = read_repository_file("models/tests/substitution_region_ranges.rs");
    for witness in [
        "all_emission_modes_match_independent_typed_map_shells",
        "all_modes_preserve_unknown_bytes_across_varint_boundaries",
        "epathmap_shell_modes_preserve_keys_and_every_empty_value_slot",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    let existing = read_repository_file("models/tests/protobuf_region_differential.rs");
    for witness in [
        "compact_shell_retains_genuinely_empty_repeated_messages",
        "compact_shell_elides_repeated_cut_only_message_envelopes",
        "present_empty_par_fields_remain_distinct_from_absence",
        "depth_4096_walk_reuses_one_backing_and_keeps_machine_depth_bounded",
    ] {
        assert!(existing.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn substitution_region_ranges_have_proof_and_native_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionRegionRanges.v");
    for theorem in [
        "subregion_success_exact",
        "subregion_complete",
        "successful_child_is_valid",
        "successful_child_retains_owner",
        "reversed_range_refused",
        "beyond_parent_refused",
        "absolute_end_overflow_refused",
        "empty_at_end_is_valid",
        "nested_child_composes",
        "nested_local_bounds_compose",
        "registered_cut_has_exact_ordinal",
        "registration_preserves_previous_ordinals",
        "value_group_has_exact_order",
        "selected_group_preserves_owner",
        "wrong_owner_is_observable",
        "absolute_offset_mutant_is_observable",
        "dropped_empty_cut_is_observable",
        "swapped_value_ordinals_are_observable",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionRegionRanges.v",
        "18",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionRegionRanges"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionRegionRanges.v")
    );
    let native = read_repository_file("models/tests/substitution_region_ranges.rs");
    for witness in [
        "checked_regions_match_native_slice_and_preserve_backing",
        "ordered_cut_ordinals_preserve_empty_and_duplicate_children",
        "impossible_and_reversed_ranges_refuse_without_mutation",
        "epathmap_groups_preserve_value_order_with_surrounding_cuts",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn split_carrier_prefilter_has_conditional_proof_and_native_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SplitCarrierPrefilter.v");
    for capstone in [
        "list_header",
        "outer_false",
        "header_list",
        "header_nonlist",
        "reference_success_is_list",
        "conclusive_rejection_sound",
        "result_equivalence",
        "observations_equivalence",
        "rejection_has_no_decode",
        "inconclusive_preserves_reference",
        "list_hint_retains_metadata_checks",
        "outer_guards_not_bypassed",
        "returned_children_are_exact",
        "canonical_hint_sound",
        "canonical_result_equivalence",
        "wire_header_controls",
        "outer_guard_mutant_controls",
        "list_metadata_mutant_controls",
        "hint_mutant_controls",
        "decode_order_mutant_control",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {capstone}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SplitCarrierPrefilter.v",
        "20",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SplitCarrierPrefilter"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SplitCarrierPrefilter.v")
    );
    let native = read_repository_file("models/tests/support/split_carrier_prefilter.rs");
    for witness in [
        "split_prefilter_preserves_all_outer_guards_and_multiplicity",
        "split_prefilter_retains_list_metadata_and_ordered_duplicate_children",
        "split_prefilter_matches_frozen_classifier_and_path",
        "split_prefilter_decode_boundary_control",
        "split_prefilter_inconclusive_framing_controls",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    assert!(read_repository_file("models/tests/match_schema_cursor.rs")
        .contains("fn resident_expr_headers_match_every_generated_variant("));
    let source = read_repository_file("models/src/rust/canonical_path.rs");
    assert!(source.contains(
        "#[cfg(test)]\n#[path = \"../../tests/support/split_carrier_prefilter.rs\"]\nmod split_prefilter_tests;"
    ));
    assert!(source.contains(
        "#[cfg(test)]\n            split_prefilter_tests::record_decode();\n            decoded = entry.decode().ok().map(|expr| (expr, multiplicity));"
    ));
}

#[test]
fn rmp2_region_summary_differential_has_an_executable_formal_bridge_marker() {
    const MARKER: &str = "resident_and_region_par_bag_summaries_match_for_every_canonical_family";
    assert!(
        include_str!("match_path_structure.rs").contains(&format!("fn {MARKER}")),
        "the RMP2 resident/region equivalence marker must remain executable"
    );
}
use std::path::{Path, PathBuf};

use models::rust::rholang::schema_meta::{
    BoundaryEquivalenceEvidence, Disposition, EquivalenceEvidence,
    BOUNDARY_PDA_EQUIVALENCE_EVIDENCE, EPATHMAP_FORMAL_EVIDENCE, PDA_EQUIVALENCE_EVIDENCE,
};
use models::rust::rholang::schema_meta_tables::{
    DERIVE_DISPOSITION_REGISTRY, HAND_WRITTEN_TRAVERSALS,
};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("models has a repository parent")
        .to_path_buf()
}

#[test]
fn substitution_family_schedules_bind_actual_dispatch_and_combine() {
    let theory = read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionSchedule.v");
    for theorem in [
        "binder_shift_composes",
        "pattern_depth_and_binder_shift_commute",
        "borrowed_environment_key_equals_owned_shift",
        "identity_order_restores_every_value",
        "append_plan_preserves_wire_association",
        "source_first_restores_patterns_then_source",
        "case_source_first_restores_pattern_source_guard",
        "appended_blocks_preserve_each_wire_association",
        "restoration_preserves_result_mapping",
        "method_rotation_restores_target_and_arguments",
        "receive_plan_restores_each_bind_and_shifted_tail",
        "match_case_plan_restores_only_retained_cases",
        "match_plan_restores_target_and_retained_case_slots",
        "new_plan_preserves_injections_without_substituting_them",
        "accumulated_evaluation_preserves_results_and_first_error",
        "scheduled_evaluation_equals_recursive_job_evaluation",
        "first_job_error_prevents_suffix_evaluation",
        "incomplete_preservation_keeps_exact_body",
        "incomplete_preservation_keeps_family_count_and_metadata",
        "incomplete_preservation_has_no_required_child_jobs",
        "incomplete_preservation_uses_admission_nonsemantic_score",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionSchedule.v",
        "21",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionSchedule"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionSchedule.v")
    );
    let native = read_repository_file("rholang/tests/support/substitution_region_slots.rs");
    for witness in [
        "binder_family_dispatch_and_combine_preserve_semantic_and_wire_orders",
        "method_constructor_rotation_preserves_target_and_each_argument",
        "map_and_pattern_slots_preserve_pairing_and_pattern_depth",
        "identity_order_families_preserve_each_positional_result",
        "generated_receive_schedules_bind_contexts_and_result_indices",
        "generated_match_schedules_bind_dropped_cases_and_result_indices",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn collection_collision_elision_binds_model_and_native_observations() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/CollectionCollisionElision.v");
    for theorem in [
        "set_empty_exact",
        "set_singleton_exact",
        "set_dispatch_refines",
        "set_multi_entry_unchanged",
        "set_observables_unchanged",
        "set_first_representative",
        "map_empty_exact",
        "map_singleton_exact",
        "map_dispatch_refines",
        "map_multi_entry_unchanged",
        "map_observables_unchanged",
        "map_first_key_and_strict_winner",
        "empty_identity_work_zero",
        "singleton_identity_work_zero",
        "singleton_removed_work_exact",
        "multi_entry_identity_work_unchanged",
        "identity_work_never_increases",
        "nested_singleton_identity_work_zero",
        "rejecting_duplicate_set_dispatch_mutant",
        "rejecting_map_winner_dispatch_mutant",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    assert_eq!(theory.matches("Print Assumptions ").count(), 20);
    for forbidden in ["Admitted.", "admit.", "Axiom ", "Parameter "] {
        assert!(!theory.contains(forbidden));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/CollectionCollisionElision.v",
        "20"
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.CollectionCollisionElision"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/CollectionCollisionElision.v")
    );
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "6",
        "--release",
        "-j4",
        "--lib",
        "collection_collision_elision"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "2",
        "--release",
        "-j4",
        "--lib",
        "par_construction::tests::ac_"
    ]));
    let native = read_repository_file("models/tests/support/collection_collision_elision.rs");
    for witness in [
        "empty_singleton_observables_and_zero_identity_work",
        "multi_entry_identity_work_and_observables_unchanged",
        "singleton_all_schema_families_preserve_score_and_bytes",
        "nested_singletons_elide_positive_identity_work",
        "arity_errors_precede_identity_work",
        "identity_counter_is_nested_panic_safe_and_thread_local",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    let catalog = read_repository_file(
        "docs/design/pathmap/workspace-regression-formal-invariant-catalog-2026-08-28.md",
    );
    for index in 1..=20 {
        assert_eq!(catalog.matches(&format!("| CCE-{index:02} |")).count(), 1);
    }
}

#[test]
fn substitution_cost_model_binds_scoped_native_events() {
    let theory = read_repository_file("formal/rocq/stack_safe_pda/theories/SubstitutionCost.v");
    for theorem in [
        "accumulation_refines_reference",
        "concatenation_adds_costs",
        "prefix_cost_never_exceeds_complete_trace",
        "shared_references_copy_no_body_bytes",
        "disjoint_root_partition_copies_exact_source",
        "one_root_copy_per_endpoint",
        "multiplicity_change_preserves_input_work",
        "no_finish_trace_has_zero_counters",
        "successful_machine_has_one_arena_and_finish",
        "prefinish_error_has_no_finish_attempt",
        "identity_bypass_has_no_arena_or_finish",
        "local_budget_implies_total_budget",
        "declared_linear_partition_bounds_observed_cost",
        "shallow_cuts_emit_no_descendant_payload",
        "compact_cuts_emit_no_descendant_payload",
        "original_cut_payload_cost_is_exact",
        "step_preserves_signed_balance",
        "peak_dominates_new_balance",
        "peak_never_decreases",
        "concatenated_windows_compose",
        "allocation_then_drop_balances",
        "reallocation_replaces_old_request",
        "external_drop_remains_negative",
        "no_event_preserves_meter",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    assert_eq!(theory.matches("Print Assumptions ").count(), 24);
    for forbidden in ["Admitted.", "admit.", "Axiom ", "Parameter "] {
        assert!(!theory.contains(forbidden));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SubstitutionCost.v",
        "24"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "10",
        "--lib",
        "payload_mapping_tests"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "2",
        "--release",
        "-j4",
        "--features",
        "protobuf-projection-experiment",
        "--lib",
        "protobuf_decoder_oracle::measurement::measurement_"
    ]));
    let measurement = read_repository_file("models/tests/support/protobuf_decoder_measurement.rs");
    for witness in [
        "measurement_fixtures_match_independent_resident_oracles",
        "measurement_entries_preserve_full_observations_and_arm_routing",
    ] {
        assert!(measurement.contains(&format!("fn {witness}(")));
    }
    let meter = read_repository_file("models/tests/support/epm_payload_mapping.rs");
    for witness in [
        "allocation_trace_matches_signed_reference",
        "allocation_meter_tracks_lifecycle_and_reallocation",
        "allocation_meter_scopes_restore_after_unwind_and_reject_nesting",
        "allocation_meter_is_thread_local_and_observes_external_frees",
        "allocation_meter_overflow_invalidates_instead_of_wrapping",
    ] {
        assert!(meter.contains(&format!("fn {witness}(")));
    }
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SubstitutionCost"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/SubstitutionCost.v")
    );
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "7",
        "--release",
        "-j4",
        "--features",
        "protobuf-projection-experiment",
        "--lib",
        "substitution_cost"
    ]));
    assert!(has_runner_command(&runner, &[
        "rholang_run_tests",
        "4",
        "--features",
        "protobuf-projection-experiment",
        "--test",
        "stack_depth_gate",
        "substitution_scaling::costs"
    ]));
    let model_tests = read_repository_file("models/tests/support/substitution_cost.rs");
    for witness in [
        "scoped_cost_fold_matches_event_reference",
        "cost_scopes_are_isolated_panic_safe_and_checked",
        "shared_regions_record_handles_without_body_copies",
        "repeated_finish_calls_are_observable",
        "actual_shared_import_records_no_body_copy",
        "actual_resident_decode_records_input_bytes",
        "actual_emission_modes_and_planning_counts_match_returned_bytes",
    ] {
        assert!(model_tests.contains(&format!("fn {witness}(")));
    }
    let runtime_tests = read_repository_file("rholang/tests/support/substitution_scaling.rs");
    let oracle = read_repository_file("rholang/tests/support/substitute_oracle.rs");
    assert!(has_runner_command(&runner, &[
        "rholang_run_tests",
        "1",
        "--features",
        "protobuf-projection-experiment",
        "--lib",
        "every_corpus_case_agrees_under_both_environments"
    ]));
    assert!(has_runner_command(&runner, &[
        "rholang_run_tests",
        "3",
        "--features",
        "protobuf-projection-experiment",
        "--lib",
        "region_cost_"
    ]));
    assert!(has_runner_command(&runner, &[
        "rholang_run_tests",
        "1",
        "--features",
        "protobuf-projection-experiment",
        "--lib",
        "substitute_oracle::differential_substitute_worklist::error_paths_agree"
    ]));
    for witness in [
        "measured_par_substitution",
        "encoded_cost_census",
        "region_cost_census_exercises_all_nine_nonidentity_root_families",
        "region_cost_collision_merges_keys_without_multiplicity_work",
        "region_cost_terminal_splices_preserve_unresolved_children",
    ] {
        assert!(oracle.contains(&format!("fn {witness}(")));
    }
    for witness in [
        "nonidentity_costs_track_distinct_endpoints_and_single_finish",
        "root_multiplicity_does_not_repeat_region_traversal",
        "substitution_error_records_partial_work_without_finish",
        "identity_substitution_has_no_construction_events",
    ] {
        assert!(runtime_tests.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn admitted_incomplete_preservation_binds_shared_bytes_and_recursive_oracle() {
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    let oracle = read_repository_file("rholang/tests/support/substitute_oracle.rs");
    assert!(oracle.contains("fn new_injection_preserves_admitted_incomplete_match_cases("));
    assert!(oracle.contains("fn new_injection_preserves_all_admitted_incomplete_families("));
    assert!(has_runner_command(&runner, &[
        "rholang_run_tests",
        "3",
        "--lib",
        "new_injection_preserves_"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "9",
        "--lib",
        "incomplete_preservation_tests"
    ]));
    let native = read_repository_file("models/tests/support/incomplete_component_preservation.rs");
    assert!(native.contains("fn incomplete_import_shares_body_and_preserves_canonical_image("));
    assert!(native.contains("fn complete_shell_declines_opaque_import_without_arena_mutation("));
}

#[test]
fn decoded_component_construction_has_proof_and_native_witnesses() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/DecodedComponentConstruction.v");
    for theorem in [
        "ordered_hole_substitution_is_exact",
        "worklist_emission_refines_resident_substitution",
        "successful_plan_consumes_exactly_all_children",
        "missing_or_extra_children_are_rejected",
        "complete_shell_declines_without_arena_change",
        "prior_denotations_survive_arena_extension",
        "modeled_plan_never_sorts_or_materializes",
        "absent_and_present_empty_are_distinct",
        "final_endpoint_equals_resident",
        "nested_field_length_prefix_round_trips",
        "misordered_slot_mutant_is_rejected",
        "missing_slot_mutant_is_rejected",
        "duplicated_slot_mutant_is_rejected",
        "bad_framing_mutant_changes_body_extent",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/DecodedComponentConstruction.v",
        "73"
    ]));
    assert_eq!(theory.matches("Print Assumptions ").count(), 73);
    for (count, filter) in [
        ("4", "planned_fragment_comparison_tests::"),
        ("4", "planned_ground_leaf_tests::"),
        ("5", "planned_fragment_publication_tests::"),
        ("5", "planned_pooled_storage_tests::"),
        ("9", "root_stream_"),
    ] {
        assert!(has_runner_command(&runner, &[
            "rpb3_run_tests",
            count,
            "--lib",
            filter
        ]));
    }
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.DecodedComponentConstruction"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "9",
        "--lib",
        "incomplete_preservation_tests"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/DecodedComponentConstruction.v")
    );
    let native = read_repository_file("models/tests/support/incomplete_component_preservation.rs");
    for witness in [
        "planned_incomplete_top_level_families_match_resident_endpoints",
        "planned_incomplete_expr_presence_matrix_matches_resident_endpoints",
        "incomplete_collection_keeps_duplicate_pairs_and_order",
        "incomplete_nested_shells_preserve_guards_patterns_and_injections",
        "incomplete_slot_order_and_presence_mutants_change_bytes",
        "incomplete_plans_preserve_arbitrary_ordered_children",
        "incomplete_mixed_family_construction_keeps_handles_on_a_fixed_stack",
        "incomplete_planning_has_no_resident_finish_events",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    let production = read_repository_file("models/src/rust/par_construction.rs");
    for theorem in [
        "positive_partition_fits",
        "every_intermediate_partition_fits",
        "accepted_guard_bounds_every_summary",
        "aggregate_guard_does_not_prove_key_addressability",
        "guarded_step_refines_reference",
        "declined_guard_preserves_reference_error",
        "later_parse_error_precedes_deferred_emission",
        "reweight_preserves_every_key",
        "reweight_uses_exact_recorded_count",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let native = read_repository_file("models/tests/support/protobuf_construction_resources.rs");
    for witness in [
        "root_stream_preserves_unsorted_duplicates_and_metadata",
        "root_stream_compact_counts_cross_byte_boundary",
        "root_stream_late_changes_preserve_count_and_replay_topology",
        "root_stream_decline_replays_first_insertion_topology",
        "root_stream_key_overflow_is_deferred_without_publication",
        "root_stream_retains_only_fixed_records_after_rewind",
        "root_stream_replay_preserves_the_original_checked_failure",
        "root_stream_teardown_keeps_deep_keys_on_a_small_stack",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    let bag = read_repository_file("rspace++/src/rspace/bag.rs");
    for witness in [
        "reweight_preserves_keys_snapshot_and_exact_cardinality",
        "rejected_partial_reweight_publishes_no_result_or_snapshot_change",
        "reweight_resumes_across_bounded_chunks_in_key_order",
    ] {
        assert!(bag.contains(&format!("fn {witness}(")));
    }
    assert!(has_runner_command(&runner, &[
        "rspace_preparation_run_tests",
        "3",
        "--lib",
        "reweight_tests::"
    ]));
    assert!(production.contains("pub fn incomplete_component<T: ParBagComponent>"));
    assert!(production.contains("self.component(shell, wire_children, score, false).map(Some)"));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "1",
        "--release",
        "-j4",
        "--features",
        "protobuf-projection-experiment",
        "--lib",
        "incomplete_planning_has_no_resident_finish_events"
    ]));
}

#[test]
fn frozen_decoder_oracle_has_provenance_proof_and_native_bindings() {
    use blake2::{Blake2b512, Digest};
    let baseline = include_bytes!("support/protobuf_decoder_frozen.rs");
    assert_eq!(format!("{:x}", Blake2b512::digest(baseline)),
        "866fdb5daf9c1ef96eae0a0a2641b54d5629588337a4e57d0004c78ab743242dd7cdb6c6512ae521d06dbe106d46f0869488e51a93b8c3e2832d50946644634b");
    let theory = read_repository_file("formal/rocq/stack_safe_pda/theories/DecoderOracleRouting.v");
    for theorem in [
        "legacy_routes_frozen",
        "poisoned_legacy_routes_frozen",
        "poison_detects_successor",
        "ordinary_routes_successor",
        "scope_restores_exact_state",
        "nested_scope_restores_legacy",
        "routing_preserves_observation",
        "closed_reentry_stays_frozen",
        "disjoint_thread_state",
        "missing_guard_poison_witness",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/DecoderOracleRouting.v",
        "10"
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.DecoderOracleRouting"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "14",
        "--lib",
        "protobuf_decoder_oracle::"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "2",
        "--lib",
        "oracle_body_guards::"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/DecoderOracleRouting.v")
    );
    let controller = read_repository_file("models/tests/support/protobuf_decoder_oracle.rs");
    for witness in [
        "routing_scope_restores_nested_state_and_unwind",
        "successor_poison_is_live_and_legacy_route_bypasses_it",
        "descriptor_corpus_routes_all_generated_decoders_independently",
        "root_and_partial_merge_observations_match_frozen_reference",
        "late_failure_preserves_committed_fields_and_exact_error",
        "epathmap_adapter_and_indirect_value_decoders_are_isolated",
        "arbitrary_buffers_preserve_root_merge_and_consumption",
        "canonical_fast_import_retains_baseline_borrowed_input_observation",
        "fixed_numeric_root_stream_matches_fragmented_frozen_reference",
        "repeated_fields_oneof_replacement_and_groups_preserve_observations",
        "public_new_keeps_uri_order_until_attachment_to_par",
        "public_collection_expr_keeps_order_and_duplicates_until_attachment_to_par",
        "public_receive_keeps_explicit_empty_condition_until_attachment_to_par",
        "public_match_keeps_explicit_empty_guard_until_attachment_to_par",
    ] {
        assert!(controller.contains(&format!("fn {witness}(")));
    }
    let guards = read_repository_file("models/tests/support/protobuf_decoder_body_guards.rs");
    assert!(guards.contains("fn missing_public_guard_is_detected_before_canonical_fast_import("));
    assert!(guards.contains("fn missing_public_guard_is_detected_before_machine_execution("));
}

#[test]
fn protobuf_par_accumulation_has_proof_and_native_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/ProtobufParAccumulation.v");
    for theorem in [
        "each_field_refines",
        "ordered_fields_refine",
        "earlier_arena_roots_survive",
        "insertion_preserves_metadata",
        "metadata_update_preserves_authority",
        "connective_update_preserves_authority",
        "explicit_empty_locally_free_replaces",
        "explicit_false_connective_replaces",
        "insertion_failure_preserves_authority",
        "refusal_preserves_committed_state",
        "refusal_does_not_visit_suffix",
        "derived_metadata_mutant_changes_observation",
        "union_metadata_mutant_changes_explicit_reset",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/ProtobufParAccumulation.v",
        "41"
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.ProtobufParAccumulation"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "6",
        "--lib",
        "protobuf_par_accumulation_tests"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "1",
        "--release",
        "-j4",
        "--features",
        "protobuf-projection-experiment",
        "--lib",
        "accumulation_emits_no_resident_root_until_requested"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/ProtobufParAccumulation.v")
    );
    let native = read_repository_file("models/tests/support/protobuf_par_accumulation.rs");
    for witness in [
        "all_component_families_preserve_explicit_root_metadata",
        "malformed_scalar_updates_keep_exact_state_error_and_consumption",
        "failed_occurrence_insert_preserves_builder_and_prior_roots",
        "move_reentry_and_insert_do_not_copy_metadata_or_finalize_pars",
        "mixed_nested_accumulation_is_stack_safe_and_finalizes_each_par_once",
        "ordered_field_events_match_frozen_committed_observations",
        "accumulation_emits_no_resident_root_until_requested",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn epm_payload_representation_has_proof_and_native_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/EpmPayloadRepresentation.v");
    for theorem in [
        "erase_commutes_with_denotation",
        "install_commutes_with_denotation",
        "each_writer_step_commutes",
        "writer_trace_commutes",
        "publication_commutes",
        "operations_preserve_topology",
        "refusal_cannot_publish",
        "scaffold_erasure_preserves_real_bindings",
        "equal_payloads_preserve_distinct_keys",
        "scaffold_leak_mutant_is_observable",
        "cursor_next_preserves_topology",
        "cursor_next_conserves_bindings",
        "cursor_take_is_ordered_partition",
        "cursor_take_conserves_bindings",
        "cursor_take_preserves_topology",
        "cursor_exhaustion_is_fused",
        "cursor_early_retirement_owns_exact_suffix",
        "cursor_root_skip_mutant_is_observable",
        "mapping_loop_refines_reference",
        "mapping_empty_accumulators_are_equivalent",
        "mapping_success_preserves_keys_and_trace",
        "mapping_first_error_stops",
        "mapping_failure_cannot_publish",
        "mapping_success_exact_callback_count",
        "mapping_preserves_shape_and_mode",
        "mapping_first_error_mutant_is_observable",
        "relative_spans_compose_exact_path",
        "scaffold_count_is_bounded_by_event_points",
        "changing_span_lengths_preserves_scaffold_work",
        "key_validation_ignores_payload_representation",
        "snapshot_admission_commutes",
        "snapshot_rejection_preserves_parent",
        "snapshot_success_installs_exact_image",
        "mode_conflict_precedes_key_validation",
        "neutral_mode_accepts_every_mode",
        "first_key_failure_stops_validation",
        "key_validation_before_mode_mutant_is_observable",
        "legacy_stage_refines_reference",
        "legacy_staging_ignores_representation",
        "legacy_field_failure_is_atomic",
        "legacy_map_mode_precedes_fields",
        "legacy_empty_field_preserves_owner",
        "legacy_merge_preserves_metadata",
        "legacy_success_is_membership_union",
        "legacy_duplicate_keys_are_idempotent",
        "legacy_first_frame_error_stops",
        "legacy_normalization_is_observable",
        "legacy_split_metadata_is_outer",
        "legacy_leading_escape_dispatch_mutant_is_observable",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert_eq!(theory.matches("Print Assumptions ").count(), 49);
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/EpmPayloadRepresentation.v",
        "49"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "7",
        "--lib",
        "legacy_ingress_tests"
    ]));
    let legacy = read_repository_file("models/tests/support/epm_legacy_ingress.rs");
    for witness in [
        "accepted_legacy_escape_alias_is_refiled_under_its_normalized_key",
        "normalization_matches_every_scalar_collection_and_escape_boundary",
        "framed_ingress_matches_frozen_reader_and_key_normalization",
        "legacy_fields_are_atomic_mode_checked_and_metadata_preserving",
        "planned_tag_one_and_public_projection_preserve_deep_values_without_internal_finishes",
        "split_metadata_belongs_to_outer_list_not_first_segment",
        "tag_one_preserves_modes_duplicates_and_outer_metadata",
    ] {
        assert!(legacy.contains(&format!("fn {witness}(")));
    }
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "4",
        "--lib",
        "snapshot_admission_tests"
    ]));
    let admission = read_repository_file("models/tests/support/epm_snapshot_admission.rs");
    for witness in [
        "planned_and_resident_admission_match_generated_keys",
        "all_modes_preserve_typed_empties_and_replace_instead_of_union",
        "root_and_first_key_errors_preserve_mode_precedence_and_deferred_policy",
        "dangling_topology_and_rejected_deep_snapshots_retire_on_small_stack",
    ] {
        assert!(admission.contains(&format!("fn {witness}(")));
    }
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.EpmPayloadRepresentation"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "8",
        "--lib",
        "payload_representation_tests"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "5",
        "--lib",
        "owned_cursor_tests"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "10",
        "--lib",
        "payload_mapping_tests"
    ]));
    let mapping = read_repository_file("models/tests/support/epm_payload_mapping.rs");
    for witness in [
        "ordered_mapping_preserves_all_bindings_and_topology",
        "modes_root_prefix_dense_siblings_and_repeated_values_are_exact",
        "first_error_and_panic_stop_callbacks_without_publishing",
        "compressed_stems_have_bounded_frames_scaffolds_and_retained_heap",
        "deep_branching_success_error_and_unwind_are_stack_safe",
    ] {
        assert!(mapping.contains(&format!("fn {witness}(")));
    }
    let cursor = read_repository_file("models/tests/support/epm_owned_cursor.rs");
    for witness in [
        "root_prefix_siblings_and_dangling_paths_are_preserved",
        "arbitrary_prefixes_partition_the_source_and_preserve_shared_roots",
        "partial_drop_and_callback_unwind_balance_payloads",
        "deep_branching_partial_consumption_is_stack_safe",
        "public_consuming_boundaries_keep_modes_and_guard_callback_unwind",
    ] {
        assert!(cursor.contains(&format!("fn {witness}(")));
    }
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "1",
        "--release",
        "-j4",
        "--features",
        "protobuf-projection-experiment",
        "--lib",
        "planned_handles_keep_exact_associations_without_resident_emission"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/EpmPayloadRepresentation.v")
    );
    let native = read_repository_file("models/tests/support/epm_payload_representation.rs");
    for witness in [
        "opaque_payloads_preserve_modes_topology_and_remove_scaffolds",
        "equal_payload_handles_do_not_collapse_endpoints",
        "malformed_frames_and_deferred_precedence_are_pinned",
        "pending_protocol_misuse_never_publishes_incomplete_values",
        "scaffold_factory_remains_lazy_and_matches_nonroot_value_free_events",
        "planned_handles_keep_exact_associations_without_resident_emission",
        "generic_topology_replay_and_drop_are_stack_safe",
        "arbitrary_prefix_maps_preserve_payload_association",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn protobuf_owned_slots_have_proof_and_native_bindings() {
    let theory = read_repository_file("formal/rocq/stack_safe_pda/theories/ProtobufOwnedSlots.v");
    let names = [
        "absent_slot_commutes",
        "present_slot_commutes",
        "absent_and_present_default_remain_distinct",
        "take_slot_commutes",
        "reopen_commutes",
        "take_reopen_commutes",
        "reopen_present_preserves_child",
        "repeated_default_commutes",
        "repeated_append_commutes",
        "repeated_append_preserves_order",
        "default_entry_commutes",
        "entry_key_update_commutes",
        "entry_value_update_commutes",
        "keyed_replacement_commutes",
        "keyed_lookup_commutes",
        "successful_keyed_replacement_is_last_wins",
        "successful_keyed_replacement_preserves_other_keys",
        "oneof_selection_commutes",
        "same_oneof_arm_reopens_existing_child",
        "different_oneof_arm_starts_default",
        "successful_oneof_attach_commutes",
        "successful_oneof_attach_selects_new_arm",
        "child_attachment_commutes",
        "failed_attachment_preserves_committed_parent",
        "failed_attachment_allows_later_success",
        "rejected_api_guard_preserves_parent",
        "guarded_attachment_commutes",
        "optional_attachment_commutes",
        "repeated_attachment_commutes",
        "repeated_prepend_mutant_changes_order",
        "absent_collapsed_to_default_mutant_changes_presence",
        "ownership_path_rank_budget",
        "ownership_path_length_is_schema_bounded",
        "strict_schema_rank_excludes_positive_cycles",
        "method_child_order_commutes",
        "method_child_order_preserves_cardinality",
        "method_child_order_retains_target_last",
        "semantic_optional_commutes",
        "semantic_optional_absent_retains_absence",
        "semantic_optional_present_retains_presence",
        "component_frontier_selects_incomplete",
        "component_frontier_selects_complete",
        "component_frontier_commutes",
        "method_unrotated_mutant_changes_order",
        "relocated_transition_preserves_payload_or_exact_error",
        "successful_resume_reuses_same_identity",
        "arbitrary_suspend_resume_trace_is_exact",
        "arbitrary_successful_trace_keeps_one_owner",
        "boxed_worklist_observes_same_flat_payload_order",
        "moving_to_continuation_and_back_needs_no_second_identity",
        "completion_observes_original_retirement_payload",
    ];
    assert_eq!(names.len(), 51);
    assert_eq!(theory.matches("Print Assumptions ").count(), names.len());
    let catalog = read_repository_file(
        "docs/design/pathmap/workspace-regression-formal-invariant-catalog-2026-08-28.md",
    );
    for name in names {
        assert!(theory.contains(&format!("Print Assumptions {name}.")));
        assert!(catalog.contains(&format!("`{name}`")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/ProtobufOwnedSlots.v",
        "51"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "3",
        "--lib",
        "snapshot_continuation_tests::"
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.ProtobufOwnedSlots"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "8",
        "--lib",
        "protobuf_owned_slots_tests"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "7",
        "--lib",
        "protobuf_construction_tests"
    ]));
    let attachment = read_repository_file("models/tests/support/protobuf_construction.rs");
    for witness in [
        "all_component_families_match_frozen_attachment_for_complete_and_incomplete_shells",
        "method_order_and_incomplete_lane_match_frozen_keys",
        "ordered_and_commutative_collections_match_frozen_attachment",
        "guard_and_condition_completion_preserve_frontier_semantics",
        "captured_epm_bounds_and_complete_shape_are_certified",
        "epm_and_ezipper_attachment_preserve_topology_payloads_and_distinct_metadata",
        "deep_mixed_attachment_uses_flat_plans_without_resident_children",
    ] {
        assert!(attachment.contains(&format!("fn {witness}(")));
    }
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/ProtobufOwnedSlots.v")
    );
    let native = read_repository_file("models/tests/support/protobuf_owned_slots.rs");
    for witness in [
        "every_descriptor_field_and_oneof_arm_projects_without_canonicalizing_its_owner",
        "owned_rank_witness_covers_every_schema_edge_and_the_epm_native_remainder",
        "optional_take_and_reopen_preserve_presence_and_accumulated_children",
        "projection_failure_stops_before_later_children_and_does_not_publish_a_result",
        "message_oneof_reopens_same_arm_but_discards_a_different_arm",
        "tagged_continuation_projects_body_before_guard_in_protobuf_order",
        "keyed_entry_defaults_and_failure_keep_the_existing_parent_binding",
        "repeated_children_and_keyed_replacements_preserve_values_and_order",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn protobuf_decoder_control_composition_has_proof_and_native_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/ProtobufDecoderComposition.v");
    for theorem in [
        "mapped_frame_preserves_control",
        "mapped_frames_preserve_context_order",
        "selected_error_and_contexts_are_preserved",
        "completed_body_restores_exact_parent",
        "unfinished_body_cannot_restore",
        "absent_parent_cannot_restore",
        "enter_preserves_admission_depth",
        "nested_inputs_restore_lifo",
        "snapshot_policy_uses_preincrement_depth",
        "successful_admission_balances_depth",
        "failed_admission_has_no_continuation",
        "admission_without_snapshot_is_rejected",
        "routing_composes_payload_refinement",
        "first_error_leaves_suffix_unvisited",
        "reopenable_child_is_not_frozen",
        "freeze_only_at_consuming_boundary",
        "freezing_preserves_denotation",
        "reopen_preserves_existing_builder",
        "guards_preserve_live_arena",
        "projection_and_retirement_use_live_arena",
        "final_projection_failure_is_not_published",
        "projection_failure_trace_has_no_publication",
        "success_retires_guards_and_arena_before_publication",
        "every_guard_has_one_retirement_per_owned_slot",
        "reversed_context_mutant_is_observable",
        "premature_freeze_mutant_loses_reopenable_form",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {theorem}.")));
    }
    assert_eq!(theory.matches("Print Assumptions ").count(), 26);
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/ProtobufDecoderComposition.v",
        "26"
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.ProtobufDecoderComposition"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "5",
        "--lib",
        "decoder_control_tests::"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "1",
        "--release",
        "-j4",
        "--features",
        "protobuf-projection-experiment",
        "--lib",
        "deep_general_decoder_materializes_once_and_retires_failed_frames_on_small_stack"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/ProtobufDecoderComposition.v")
    );
    let native = read_repository_file("models/tests/support/protobuf_decoder_control.rs");
    for witness in [
        "deep_general_decoder_materializes_once_and_retires_failed_frames_on_small_stack",
        "nested_snapshot_inputs_restore_parent_and_preserve_failure_contexts",
        "epm_remainder_reopens_all_native_var_variants_without_losing_mode",
        "nested_error_locations_have_an_independent_nearest_first_witness",
        "partial_map_failure_retires_unique_deep_pending_topology_before_arena",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn epm_owned_retirement_has_explicit_proof_boundaries_and_native_bindings() {
    let theory = read_repository_file("formal/rocq/stack_safe_pda/theories/EpmOwnedRetirement.v");
    let names = [
        "stateful_provider_preserves_every_event",
        "stateful_provider_preserves_valid_same_arena_values",
        "stateful_terminal_drain_preserves_existing_semantics",
        "repeat_scaffold_needs_only_validity",
        "arena_barrier_reuses_nonempty",
        "arena_barrier_bootstraps_empty",
        "arena_barrier_returns_valid",
        "arena_barrier_preserves_existing",
        "arena_barrier_repeated_requests_reuse",
        "arena_extension_preserves_barrier",
        "every_settlement_consumes_source",
        "live_source_has_exactly_one_terminal_event",
        "consumed_source_cannot_retire_or_transfer_again",
        "retirement_and_transfer_are_exclusive",
        "zipper_adoption_preserves_subject_and_authority",
        "replacement_retires_old_before_adopting_new",
        "replacing_with_absence_retires_without_publication",
        "scope_disposal_consumes_each_owned_slot_once",
        "scope_disposal_preserves_order_and_drain_traces",
        "returning_failure_preserves_selected_error",
        "refusal_and_projection_failure_do_not_publish",
        "cleanup_panic_is_terminal",
        "cleanup_allocation_failure_is_terminal",
        "retirement_reuses_existing_raw_drop_quarantine",
        "clone_owner_shares_image_identity_and_caches",
        "mutation_begin_clears_both_caches",
        "mutation_begin_preserves_image_and_metadata",
        "interrupted_mutation_begin_has_coherent_caches",
        "prepared_image_change_has_coherent_caches",
        "shared_mutation_preserves_sibling_and_cache_coherence",
        "preflight_noop_preserves_exact_owner",
        "uncleared_cache_mutant_is_incoherent",
    ];
    assert_eq!(names.len(), 32);
    assert_eq!(theory.matches("Print Assumptions ").count(), names.len());
    let catalog = read_repository_file(
        "docs/design/pathmap/workspace-regression-formal-invariant-catalog-2026-08-28.md",
    );
    for name in names {
        assert!(theory.contains(&format!("Print Assumptions {name}.")));
        assert!(catalog.contains(&format!("`{name}`")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/EpmOwnedRetirement.v",
        "32"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "6",
        "--lib",
        "resident_ownership_tests"
    ]));
    let resident = read_repository_file("models/tests/support/epm_resident_ownership.rs");
    for witness in [
        "cold_and_warm_aliases_share_storage_until_mutation",
        "mutation_clears_both_caches_before_exposing_repr",
        "no_op_mutations_preserve_storage_metadata_and_caches",
        "resident_owners_retire_unique_shared_and_topology_only_on_small_stack",
        "guarded_admission_rejection_and_replacement_preserve_previous_state",
        "algebra_and_consuming_transfers_preserve_values_and_metadata",
    ] {
        assert!(resident.contains(&format!("fn {witness}(")));
    }
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.EpmOwnedRetirement"
    ]));
    assert!(has_runner_command(&runner, &[
        "rspace_preparation_run_tests",
        "5",
        "--lib",
        "stateful_retirement"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "7",
        "--lib",
        "owned_retirement_tests"
    ]));
    assert!(has_runner_command(&runner, &[
        "rpb3_run_tests",
        "8",
        "--lib",
        "planned_epm_ownership_tests"
    ]));
    assert!(
        read_repository_file("formal/rocq/stack_safe_pda/_CoqProject")
            .lines()
            .any(|line| line == "theories/EpmOwnedRetirement.v")
    );
    let native = read_repository_file("rspace++/tests/guarded_pathmap/stateful_retirement.rs");
    for witness in [
        "stateful_factory_is_lazy_and_uninstalled_barriers_are_retired",
        "stateful_retirement_preserves_cow_sibling_and_value_free_topology",
        "stateful_deep_branching_and_value_free_retirement_use_256k_stack",
        "stateful_retirement_panics_fail_stop_without_raw_unwind",
        "stateful_provider_erases_to_static_drain_with_exact_real_values",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
    let pending = read_repository_file("models/tests/support/epm_owned_retirement.rs");
    for witness in [
        "branching_pending_map_abandonment_retires_all_suspension_states",
        "branching_set_and_topology_only_pending_frames_retire_without_publication",
        "branching_final_refusal_preserves_selected_error_and_retires_values",
        "map_writer_transfer_and_replacement_keep_exact_payload_ownership",
        "unwinding_a_partially_built_frame_retires_its_owned_map",
        "writer_guard_reuses_existing_root_and_provider_storage",
        "generated_abandonment_and_rejection_retire_every_created_payload",
    ] {
        assert!(pending.contains(&format!("fn {witness}(")));
    }
    let planned = read_repository_file("models/tests/support/planned_epm_ownership.rs");
    for witness in [
        "arena_barriers_preserve_validity_and_reuse",
        "planned_owners_retire_branching_sets_maps_and_topology",
        "raw_topology_ingress_preserves_topology_and_bootstraps_valid_barrier",
        "builder_unwind_retires_partial_branching_keys",
        "planned_length_refusal_preserves_error_and_arena_reuse",
        "generated_mirror_replacement_and_projection_failure_retire_owned_maps",
        "arena_disposal_retires_planned_trie_nodes_on_small_stack",
        "public_materialization_preserves_topology_values_and_metadata",
    ] {
        assert!(planned.contains(&format!("fn {witness}(")));
    }
}

fn read_repository_file(relative: &str) -> String {
    let path = repository_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn contains_rust_token_sequence(source: &str, expected: &str) -> bool {
    let source: TokenStream = source.parse().expect("Rust source must tokenize");
    let expected: Vec<TokenTree> = expected
        .parse::<TokenStream>()
        .expect("Rust binding must tokenize")
        .into_iter()
        .collect();
    assert!(!expected.is_empty(), "Rust binding must contain tokens");

    fn same_token(actual: &TokenTree, wanted: &TokenTree) -> bool {
        std::mem::discriminant(actual) == std::mem::discriminant(wanted)
            && actual.to_string() == wanted.to_string()
    }

    fn same_call_arguments(actual: &TokenTree, wanted: &TokenTree) -> bool {
        let (TokenTree::Group(actual), TokenTree::Group(wanted)) = (actual, wanted) else {
            return false;
        };
        if actual.delimiter() != proc_macro2::Delimiter::Parenthesis
            || wanted.delimiter() != proc_macro2::Delimiter::Parenthesis
        {
            return false;
        }
        let mut actual: Vec<TokenTree> = actual.stream().into_iter().collect();
        let mut wanted: Vec<TokenTree> = wanted.stream().into_iter().collect();
        for arguments in [&mut actual, &mut wanted] {
            if matches!(arguments.last(), Some(TokenTree::Punct(comma)) if comma.as_char() == ',') {
                arguments.pop();
            }
        }
        actual.len() == wanted.len()
            && actual
                .iter()
                .zip(&wanted)
                .all(|(left, right)| same_token(left, right))
    }

    fn contains(stream: TokenStream, expected: &[TokenTree]) -> bool {
        let tokens: Vec<TokenTree> = stream.into_iter().collect();
        tokens.windows(expected.len()).any(|window| {
            window
                .iter()
                .zip(expected)
                .enumerate()
                .all(|(index, (actual, wanted))| {
                    same_token(actual, wanted)
                        || (index > 0
                            && matches!(
                                (&window[index - 1], &expected[index - 1]),
                                (TokenTree::Ident(_), TokenTree::Ident(_))
                            )
                            && same_call_arguments(actual, wanted))
                })
        }) || tokens.into_iter().any(|token| match token {
            TokenTree::Group(group) => contains(group.stream(), expected),
            _ => false,
        })
    }

    contains(source, &expected)
}

#[test]
fn rust_binding_match_ignores_layout_but_rejects_noncode_and_reordering() {
    let expected = "builder.nodes.push(node); #[cfg(test)] emission_tests::record_publication(node_index, &builder);";
    let formatted = "fn witness() { builder.nodes.push(node);\n#[cfg(test)]\nemission_tests::record_publication(\nnode_index,\n&builder,\n); }";
    assert!(contains_rust_token_sequence(formatted, expected));
    assert!(!contains_rust_token_sequence(
        &format!("fn decoy() {{ let text = {expected:?}; }}"),
        expected
    ));
    assert!(!contains_rust_token_sequence(
        &format!("fn decoy() {{ /* {expected} */ }}"),
        expected
    ));
    assert!(!contains_rust_token_sequence(
        "fn reversed() { #[cfg(test)] emission_tests::record_publication(node_index, &builder); builder.nodes.push(node); }",
        expected
    ));
    assert!(!contains_rust_token_sequence(
        "fn distinct_tuple() { let value = (node,); }",
        "let value = (node);"
    ));
}

fn has_runner_command(runner: &str, expected: &[&str]) -> bool {
    runner.replace("\\\n", " ").lines().any(|line| {
        line.split_whitespace()
            .take(expected.len())
            .eq(expected.iter().copied())
    })
}

fn has_runner_kernel_check(runner: &str, modules: &[&str]) -> bool {
    runner.replace("\\\n", " ").lines().any(|line| {
        let tokens: Vec<_> = line.split_whitespace().collect();
        tokens.first() == Some(&"rocq_check_modules") && tokens.ends_with(modules)
    })
}

#[test]
fn proof_runner_binding_checks_commands_not_descriptions_or_comments() {
    let command = ["rocq_compile_with_closed_count", "theories/Example.v", "32"];
    assert!(has_runner_command(
        "  rocq_compile_with_closed_count \\\n  theories/Example.v 32 \\\n  \"a changed description\"\n",
        &command,
    ));
    for invalid in [
        "# rocq_compile_with_closed_count theories/Example.v 32",
        "echo rocq_compile_with_closed_count theories/Example.v 32",
        "rocq_compile_with_closed_count theories/Example.v 320",
        "rocq_compile_with_closed_count theories/Example.v 31",
        "rocq_compile_with_closed_count theories/Other.v 32",
        "rocq_compile_with_closed_count\ntheories/Example.v 32",
    ] {
        assert!(
            !has_runner_command(invalid, &command),
            "accepted {invalid:?}"
        );
    }
    let modules = ["StackSafePDA.Main", "StackSafePDA.Mutants"];
    assert!(has_runner_kernel_check(
        "rocq_check_modules \"an arbitrary description\" StackSafePDA.Main StackSafePDA.Mutants",
        &modules,
    ));
    for invalid in [
        "# rocq_check_modules \"description\" StackSafePDA.Main StackSafePDA.Mutants",
        "echo rocq_check_modules \"description\" StackSafePDA.Main StackSafePDA.Mutants",
        "rocq_check_modules \"description\" StackSafePDA.Main",
        "rocq_check_modules \"description\" StackSafePDA.Main StackSafePDA.Other",
    ] {
        assert!(
            !has_runner_kernel_check(invalid, &modules),
            "accepted {invalid:?}"
        );
    }
}

#[derive(Clone, Copy)]
struct Actree03Correspondence {
    id: &'static str,
    theorems: &'static [&'static str],
    production_file: &'static str,
    production_marker: &'static str,
    executable_file: &'static str,
    executable_marker: &'static str,
    generator: &'static str,
    oracle: &'static str,
}

#[test]
fn unary_elist_fixture_construction_has_concrete_proof_and_native_bindings() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/UnaryEListConstruction.v");
    for capstone in [
        "planned_emission_equals_repeated_resident_wrapping",
        "planned_expression_is_exact",
        "direct_score_equals_recursive_semantic_score",
        "positive_depth_score_length_is_exact",
        "checked_endpoint_success_refines_recursive_specification",
        "frame_refusal_cannot_publish_endpoint",
        "reservation_refusal_cannot_publish_endpoint",
        "modeled_loop_and_copy_cost_is_linear",
        "representable_score_trailer_round_trips",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {capstone}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for (source, count) in [
        ("theories/UnaryEListConstruction.v", "22"),
        ("theories/UnaryEListConstructionMutants.v", "14"),
    ] {
        assert!(has_runner_command(&runner, &[
            "rocq_compile_with_closed_count",
            source,
            count
        ]));
    }
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.UnaryEListConstruction",
        "StackSafePDA.UnaryEListConstructionMutants",
    ]));
    let native = read_repository_file("models/tests/match_path_structure.rs");
    for witness in [
        "bulk_unary_fixture_matches_frozen_resident_oracle",
        "bulk_unary_fixture_preserves_arbitrary_terminal_metadata",
        "wide_fixture_multiplicity_matches_frozen_occurrence_oracle",
        "wide_fixture_overflow_preserves_components_and_metadata",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

#[test]
fn singleton_map_fixture_has_checked_proof_and_native_correspondence() {
    let theory =
        read_repository_file("formal/rocq/stack_safe_pda/theories/SingletonMapConstruction.v");
    for capstone in [
        "planned_emission_refines_resident_wrapping",
        "one_outer_map_and_exact_inner_depth",
        "snapshot_zero_summary_is_exact",
        "final_component_score_and_key_are_exact",
        "checked_endpoint_success_refines_resident",
        "checked_plan_refusal_cannot_publish",
        "endpoint_success_has_exact_capacity_and_representable_trailer",
        "emitted_bytes_bound",
    ] {
        assert!(theory.contains(&format!("Print Assumptions {capstone}.")));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/SingletonMapConstruction.v",
        "18",
    ]));
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.SingletonMapConstruction"
    ]));
    let native = read_repository_file("models/tests/epathmap_epm1_snapshot.rs");
    for witness in [
        "planned_map_fixture_matches_frozen_resident_snapshots_and_keys",
        "planned_map_fixture_preserves_arbitrary_leaf",
        "planned_map_fixture_controls_reject_wrong_depth_key_and_value",
    ] {
        assert!(native.contains(&format!("fn {witness}(")));
    }
}

const ACTREE03_GENERATORS: &[&str] = &[
    "G-ACT03-WRITER",
    "G-ACT03-MUTATION",
    "G-ACT03-LINE-CACHE",
    "G-EPM1-WRITER",
    "G-ACT03-DEPTH-WIDTH",
];

const ACTREE03_ORACLES: &[&str] = &[
    "O-ACT03-LEGACY",
    "O-ACT03-RECONSTRUCT",
    "O-ACT03-PATHMAP",
    "O-ACT03-SOURCE",
    "O-EPM1-GOLDEN",
    "O-ROCQ-ACT03",
];

const ACTREE03_CORRESPONDENCE: &[Actree03Correspondence] = &[
    Actree03Correspondence {
        id: "ACT03-CAN-001",
        theorems: &["validate_accepts_every_canonical_writer_output"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "fn validate_direct_canonical_act",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker: "arbitrary_pathmap_writer_images_are_accepted_with_exact_preorder_plans",
        generator: "G-ACT03-WRITER",
        oracle: "O-ACT03-LEGACY",
    },
    Actree03Correspondence {
        id: "ACT03-FRM-001",
        theorems: &["semantic_certificate_image_boundaries"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "ACTree03 canonical varint trailer is not zero-filled",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker:
            "new_and_independent_oracle_agree_for_all_truncations_and_single_bit_mutations",
        generator: "G-ACT03-MUTATION",
        oracle: "O-ACT03-LEGACY",
    },
    Actree03Correspondence {
        id: "ACT03-ROOT-001",
        theorems: &[
            "semantic_certificate_root_closes_arena",
            "canonical_semantic_certificate_header_and_root_bounds",
        ],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "ACTree03 arena contains gaps, overlaps, or unreachable objects",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker:
            "new_and_independent_oracle_agree_for_all_truncations_and_single_bit_mutations",
        generator: "G-ACT03-MUTATION",
        oracle: "O-ROCQ-ACT03",
    },
    Actree03Correspondence {
        id: "ACT03-DISC-001",
        theorems: &["canonical_writer_semantic_certificate_discovery_exact"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "records.push(validated_topology_record",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker:
            "validated_plan_preserves_old_topology_endpoints_rewind_and_fragmented_provenance",
        generator: "G-ACT03-WRITER",
        oracle: "O-ACT03-PATHMAP",
    },
    Actree03Correspondence {
        id: "ACT03-ID-001",
        theorems: &["semantic_certificate_preorder_physical_ids_nodup"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "register_direct_semantic_node(&mut seen",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker: "assert_forward_preorder_plan_is_exact",
        generator: "G-ACT03-WRITER",
        oracle: "O-ROCQ-ACT03",
    },
    Actree03Correspondence {
        id: "ACT03-ORDER-001",
        theorems: &["validate_accepts_semantic_certificate"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "ACTree03 child roots are not in producer order",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker:
            "new_and_independent_oracle_agree_for_all_truncations_and_single_bit_mutations",
        generator: "G-ACT03-MUTATION",
        oracle: "O-ACT03-LEGACY",
    },
    Actree03Correspondence {
        id: "ACT03-INDEX-001",
        theorems: &["reversed_source_nodes_index"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "next_record: usize",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker: "assert_forward_preorder_plan_is_exact",
        generator: "G-ACT03-WRITER",
        oracle: "O-ROCQ-ACT03",
    },
    Actree03Correspondence {
        id: "ACT03-CHILD-001",
        theorems: &[
            "direct_children_encoded_len_source_forest_exact",
            "direct_children_encoded_len_source_exact",
        ],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "direct_children_encoded_len = frame",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker: "unequal_children_wrappers_and_reused_lines_preserve_exact_plan_layout",
        generator: "G-ACT03-WRITER",
        oracle: "O-ACT03-PATHMAP",
    },
    Actree03Correspondence {
        id: "ACT03-LINE-001",
        theorems: &["replay_canonical_line_matches_add_or_reuse"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "struct DirectCanonicalLineReplay",
        executable_file: "rspace++/src/rspace/pathmap_codec.rs",
        executable_marker:
            "forced_line_hash_collision_preserves_range_equality_and_last_writer_semantics",
        generator: "G-ACT03-LINE-CACHE",
        oracle: "O-ROCQ-ACT03",
    },
    Actree03Correspondence {
        id: "ACT03-LINE-002",
        theorems: &["replay_canonical_line_matches_add_or_reuse"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "ranges_equal",
        executable_file: "rspace++/src/rspace/pathmap_codec.rs",
        executable_marker: "equal ranges may reuse the latest physical line",
        generator: "G-ACT03-LINE-CACHE",
        oracle: "O-ACT03-RECONSTRUCT",
    },
    Actree03Correspondence {
        id: "ACT03-LINE-003",
        theorems: &["replay_canonical_line_matches_add_or_reuse"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "ranges_equal",
        executable_file: "rspace++/src/rspace/pathmap_codec.rs",
        executable_marker: "unequal second line must replace the first in the hash bucket",
        generator: "G-ACT03-LINE-CACHE",
        oracle: "O-ACT03-RECONSTRUCT",
    },
    Actree03Correspondence {
        id: "ACT03-LINE-004",
        theorems: &["replay_canonical_line_matches_add_or_reuse"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "line reuse differs from the canonical writer",
        executable_file: "rspace++/src/rspace/pathmap_codec.rs",
        executable_marker:
            "forced_line_hash_collision_preserves_range_equality_and_last_writer_semantics",
        generator: "G-ACT03-LINE-CACHE",
        oracle: "O-ROCQ-ACT03",
    },
    Actree03Correspondence {
        id: "ACT03-REPLAY-001",
        theorems: &[
            "certified_source_writer_recursive_replay_exact",
            "semantic_certificate_recursive_replay_exact",
        ],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "validate_direct_producer_node(",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker: "unequal_children_wrappers_and_reused_lines_preserve_exact_plan_layout",
        generator: "G-ACT03-WRITER",
        oracle: "O-ACT03-LEGACY",
    },
    Actree03Correspondence {
        id: "ACT03-PDA-001",
        theorems: &[
            "replay_plan_executes_source_semantics",
            "semantic_certificate_pda_replay_exact",
        ],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "struct DirectPlanValidationFrame",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker: "deeply_nested_value_prefixes_are_stack_safe_on_a_small_native_stack",
        generator: "G-ACT03-DEPTH-WIDTH",
        oracle: "O-ROCQ-ACT03",
    },
    Actree03Correspondence {
        id: "ACT03-FUEL-001",
        theorems: &["semantic_certificate_pda_replay_exact"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "while let Some(frame) = frames.last_mut()",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker: "semantic_node_parses, counters.plan_records",
        generator: "G-ACT03-DEPTH-WIDTH",
        oracle: "O-ROCQ-ACT03",
    },
    Actree03Correspondence {
        id: "ACT03-EPM1-001",
        theorems: &["validate_accepts_every_canonical_writer_output"],
        production_file: "models/src/rust/epathmap_trie_codec.rs",
        production_marker: "ArenaCompactTree::from_zipper",
        executable_file: "models/tests/epathmap_epm1_snapshot.rs",
        executable_marker:
            "map_snapshot_preserves_values_across_compact_line_branch_and_dense_shapes",
        generator: "G-EPM1-WRITER",
        oracle: "O-EPM1-GOLDEN",
    },
    Actree03Correspondence {
        id: "ACT03-ORACLE-001",
        theorems: &["validate_accepts_semantic_certificate"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "validate_direct_canonical_act",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker: "arbitrary_pathmap_writer_images_are_accepted_with_exact_preorder_plans",
        generator: "G-ACT03-MUTATION",
        oracle: "O-ACT03-RECONSTRUCT",
    },
    Actree03Correspondence {
        id: "ACT03-SOURCE-001",
        theorems: &["semantic_certificate_image_boundaries"],
        production_file: "rspace++/src/rspace/pathmap_codec.rs",
        production_marker: "pub trait IndexedByteSource",
        executable_file: "rspace++/tests/support/pathmap_codec_validated_plan.rs",
        executable_marker: "malformed_errors_match_between_contiguous_and_fragmented_sources",
        generator: "G-ACT03-MUTATION",
        oracle: "O-ACT03-SOURCE",
    },
];

#[derive(Clone, Copy)]
struct Epm1PositionalCorrespondence {
    id: &'static str,
    theorem: &'static str,
    production_marker: &'static str,
    executable_marker: &'static str,
}

const EPM1_POSITIONAL_CORRESPONDENCE: &[Epm1PositionalCorrespondence] = &[
    Epm1PositionalCorrespondence {
        id: "EPM1-POS-001",
        theorem: "canonical_zero_writer_has_one_zero_sentinel_per_logical_endpoint",
        production_marker: "ArenaCompactTree::from_zipper(map.read_zipper(), |_| 0)",
        executable_marker: "epm1_map_layout_writes_zero_marker_for_every_value_occurrence",
    },
    Epm1PositionalCorrespondence {
        id: "EPM1-POS-002",
        theorem: "canonical_writer_and_image_are_address_independent",
        production_marker: "ArenaCompactTree::from_zipper(map.read_zipper(), |_| 0)",
        executable_marker: "positional_occurrences_ignore_shared_cow_par_addresses",
    },
    Epm1PositionalCorrespondence {
        id: "EPM1-POS-003",
        theorem: "canonical_zero_writer_is_accepted_by_concrete_act_parser",
        production_marker: "CanonicalCompactTrieTopologyCursor::new(&bytes[framing.arena.clone()])",
        executable_marker: "positional_replay_preserves_root_prefix_dense_and_topology_only_nodes",
    },
    Epm1PositionalCorrespondence {
        id: "EPM1-POS-004",
        theorem: "admitted_positional_run_publishes_exactly_once",
        production_marker: "pub(crate) fn finish(mut self)",
        executable_marker:
            "pending_decoder_publishes_only_from_successful_finish_without_partial_result",
    },
    Epm1PositionalCorrespondence {
        id: "EPM1-POS-005",
        theorem: "positional_entry_count_and_count_free_machine_agree",
        production_marker: "layout_with_certified_value_count",
        executable_marker: "certified_and_traversed_layout_counts_are_identical_for_every_mode",
    },
    Epm1PositionalCorrespondence {
        id: "EPM1-POS-006",
        theorem: "positional_error_precedence_is_total",
        production_marker: "first_semantic_error",
        executable_marker: "child_errors_precede_deferred_act_and_noncanonical_body_errors",
    },
    Epm1PositionalCorrespondence {
        id: "EPM1-POS-007",
        theorem: "epm1_positional_act_body_association_capstone",
        production_marker: "fn advance_to_map_value(&mut self)",
        executable_marker: "synchronous_decode_and_generated_pending_core_are_byte_identical",
    },
];

#[test]
fn every_actree03_proof_obligation_has_a_mechanical_executable_binding() {
    const EXPECTED_IDS: &[&str] = &[
        "ACT03-CAN-001",
        "ACT03-CHILD-001",
        "ACT03-DISC-001",
        "ACT03-EPM1-001",
        "ACT03-FRM-001",
        "ACT03-FUEL-001",
        "ACT03-ID-001",
        "ACT03-INDEX-001",
        "ACT03-LINE-001",
        "ACT03-LINE-002",
        "ACT03-LINE-003",
        "ACT03-LINE-004",
        "ACT03-ORACLE-001",
        "ACT03-ORDER-001",
        "ACT03-PDA-001",
        "ACT03-REPLAY-001",
        "ACT03-ROOT-001",
        "ACT03-SOURCE-001",
    ];
    const PROOF_FILE: &str = "formal/rocq/actree03_completeness/theories/Epm1ActConcreteV5.v";

    let proof = read_repository_file(PROOF_FILE);
    let generator_ids = ACTREE03_GENERATORS.iter().copied().collect::<BTreeSet<_>>();
    let oracle_ids = ACTREE03_ORACLES.iter().copied().collect::<BTreeSet<_>>();
    let mut ids = BTreeSet::new();
    for binding in ACTREE03_CORRESPONDENCE {
        assert!(
            ids.insert(binding.id),
            "duplicate ACTree03 property {}",
            binding.id
        );
        assert!(
            generator_ids.contains(binding.generator),
            "{} has unknown generator",
            binding.id
        );
        assert!(
            oracle_ids.contains(binding.oracle),
            "{} has unknown oracle",
            binding.id
        );
        for theorem in binding.theorems {
            assert!(
                proof.contains(&format!("Theorem {theorem}"))
                    || proof.contains(&format!("Corollary {theorem}")),
                "{} names absent theorem `{theorem}`",
                binding.id,
            );
            assert!(
                proof.contains(&format!("Print Assumptions {theorem}.")),
                "{} does not expose `{theorem}` to the kernel-assumption gate",
                binding.id,
            );
        }
        let production = read_repository_file(binding.production_file);
        assert!(
            production.contains(binding.production_marker),
            "{} names absent production marker `{}`",
            binding.id,
            binding.production_marker,
        );
        let executable = read_repository_file(binding.executable_file);
        assert!(
            executable.contains(binding.executable_marker),
            "{} names absent executable marker `{}`",
            binding.id,
            binding.executable_marker,
        );
    }
    assert_eq!(
        ids,
        EXPECTED_IDS.iter().copied().collect(),
        "the ACTree03 proof-to-executable property inventory changed",
    );
}

#[test]
fn every_epm1_positional_obligation_has_a_mechanical_executable_binding() {
    const EXPECTED_IDS: &[&str] = &[
        "EPM1-POS-001",
        "EPM1-POS-002",
        "EPM1-POS-003",
        "EPM1-POS-004",
        "EPM1-POS-005",
        "EPM1-POS-006",
        "EPM1-POS-007",
    ];
    const PROOF_FILE: &str =
        "formal/rocq/actree03_completeness/theories/Epm1PositionalAssociationV6.v";
    const PRODUCTION_FILE: &str = "models/src/rust/epathmap_trie_codec.rs";

    let proof = read_repository_file(PROOF_FILE);
    let production = read_repository_file(PRODUCTION_FILE);
    let mut ids = BTreeSet::new();
    for binding in EPM1_POSITIONAL_CORRESPONDENCE {
        assert!(
            ids.insert(binding.id),
            "duplicate EPM1 positional property {}",
            binding.id
        );
        assert!(
            proof.contains(&format!("Theorem {}", binding.theorem))
                || proof.contains(&format!("Corollary {}", binding.theorem)),
            "{} names absent theorem `{}`",
            binding.id,
            binding.theorem,
        );
        assert!(
            proof.contains(&format!("Print Assumptions {}.", binding.theorem)),
            "{} does not expose `{}` to the kernel-assumption gate",
            binding.id,
            binding.theorem,
        );
        assert!(
            production.contains(binding.production_marker),
            "{} names absent production marker `{}`",
            binding.id,
            binding.production_marker,
        );
        assert!(
            production.contains(binding.executable_marker),
            "{} names absent executable marker `{}`",
            binding.id,
            binding.executable_marker,
        );
    }
    assert_eq!(
        ids,
        EXPECTED_IDS.iter().copied().collect(),
        "the EPM1 positional proof-to-executable inventory changed",
    );
}

fn evidence_by_surface() -> BTreeMap<&'static str, &'static EquivalenceEvidence> {
    let mut by_surface = BTreeMap::new();
    for evidence in PDA_EQUIVALENCE_EVIDENCE {
        assert!(
            by_surface.insert(evidence.surface, evidence).is_none(),
            "duplicate formal-equivalence evidence for `{}`",
            evidence.surface
        );
    }
    by_surface
}

#[test]
fn every_recursive_surface_has_exactly_one_proof_and_oracle_binding() {
    let evidence = evidence_by_surface();
    let mut required = BTreeSet::new();

    for (_, surface, disposition) in DERIVE_DISPOSITION_REGISTRY {
        if matches!(
            disposition,
            Disposition::Converted(_) | Disposition::FollowsFrom(_)
        ) {
            required.insert(*surface);
        }
    }
    required.extend(HAND_WRITTEN_TRAVERSALS.iter().map(|(surface, _)| *surface));

    let documented: BTreeSet<_> = evidence.keys().copied().collect();
    assert_eq!(
        documented, required,
        "the formal-equivalence evidence table and traversal registries differ"
    );
}

#[test]
fn every_binding_resolves_to_a_checked_theorem_and_executable_oracle() {
    let evidence = evidence_by_surface();
    assert!(!evidence.is_empty(), "the equivalence manifest is vacuous");

    let mut proof_cache = BTreeMap::<&str, String>::new();
    let mut executable_cache = BTreeMap::<&str, String>::new();
    for item in evidence.values() {
        let proof = proof_cache
            .entry(item.proof_file)
            .or_insert_with(|| read_repository_file(item.proof_file));
        assert!(
            proof.contains(&format!("Theorem {}", item.theorem))
                || proof.contains(&format!("Corollary {}", item.theorem)),
            "{} names absent theorem `{}` in {}",
            item.surface,
            item.theorem,
            item.proof_file
        );

        let executable = executable_cache
            .entry(item.executable_file)
            .or_insert_with(|| read_repository_file(item.executable_file));
        assert!(
            executable.contains(item.executable_marker),
            "{} names absent executable marker `{}` in {}",
            item.surface,
            item.executable_marker,
            item.executable_file
        );
    }
}

#[test]
fn every_repository_boundary_pda_has_one_production_proof_and_oracle_binding() {
    const REQUIRED: &[&str] = &[
        "node::Par-to-RhoExpr conversion",
        "node::EPathMap-to-RhoExpr mode conversion",
        "node::RhoExpr::Clone",
        "node::RhoExpr::Serialize",
        "node::RhoExpr::Debug",
        "node::RhoExpr::Drop",
        "rholang::SpatialMatcher heterogeneous PDA",
        "rholang::SpatialMatcher state isolation",
        "rholang::FreeMap drive-local transaction",
        "rholang::Encoded Connective authoritative PDA",
        "rholang::Encoded Connective child metadata dispatch",
        "rholang::Encoded Connective root direct-match admission",
        "rholang::Encoded Connective root candidate metadata dispatch",
        "rholang::Encoded Connective exact-scalar refinement",
        "rholang::Encoded Expr outer-envelope routing",
        "rholang::EPathMap singleton owned-zipper matching",
        "models::RMP2 generated cursor encoder",
        "rholang::RMP2 ordered query compiler",
        "rholang::Accounting demand byte-region PDA",
    ];

    let mut evidence = BTreeMap::<&str, &BoundaryEquivalenceEvidence>::new();
    for item in BOUNDARY_PDA_EQUIVALENCE_EVIDENCE {
        assert!(
            evidence.insert(item.surface, item).is_none(),
            "duplicate boundary formal-equivalence evidence for `{}`",
            item.surface
        );
    }
    assert_eq!(
        evidence.keys().copied().collect::<BTreeSet<_>>(),
        REQUIRED.iter().copied().collect(),
        "the closed boundary-PDA inventory and its evidence table differ"
    );

    for item in evidence.values() {
        let production = read_repository_file(item.production_file);
        assert!(
            production.contains(item.production_marker),
            "{} names absent production marker `{}` in {}",
            item.surface,
            item.production_marker,
            item.production_file
        );
        let proof = read_repository_file(item.proof_file);
        assert!(
            proof.contains(&format!("Theorem {}", item.theorem))
                || proof.contains(&format!("Corollary {}", item.theorem)),
            "{} names absent theorem `{}` in {}",
            item.surface,
            item.theorem,
            item.proof_file
        );
        let executable = read_repository_file(item.executable_file);
        assert!(
            executable.contains(item.executable_marker),
            "{} names absent executable marker `{}` in {}",
            item.surface,
            item.executable_marker,
            item.executable_file
        );
    }
}

#[test]
fn every_epathmap_law_resolves_to_a_checked_theorem_and_pathmap_test() {
    assert!(
        !EPATHMAP_FORMAL_EVIDENCE.is_empty(),
        "the EPathMap formal evidence table is vacuous"
    );
    let mut surfaces = BTreeSet::new();
    for item in EPATHMAP_FORMAL_EVIDENCE {
        assert!(
            surfaces.insert(item.surface),
            "duplicate EPathMap formal evidence for {}",
            item.surface
        );
        let proof = read_repository_file(item.proof_file);
        assert!(
            proof.contains(&format!("Theorem {}", item.theorem))
                || proof.contains(&format!("Corollary {}", item.theorem)),
            "{} names absent theorem `{}`",
            item.surface,
            item.theorem
        );
        let executable = read_repository_file(item.executable_file);
        assert!(
            executable.contains(item.executable_marker),
            "{} names absent PathMap test marker `{}`",
            item.surface,
            item.executable_marker
        );
    }
}

#[test]
fn rocq_kernel_contains_no_unproved_declarations() {
    for relative in [
        "formal/rocq/stack_safe_pda/theories/StackSafePDA.v",
        "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        "formal/rocq/stack_safe_pda/theories/Bag.v",
        "formal/rocq/stack_safe_pda/theories/EPM1.v",
        "formal/rocq/stack_safe_pda/theories/RMP2.v",
        "formal/rocq/stack_safe_pda/theories/SpatialMatcher.v",
        "formal/rocq/stack_safe_pda/theories/FreeMapJournal.v",
        "formal/rocq/stack_safe_pda/theories/AccountingDemand.v",
        "formal/rocq/stack_safe_pda/theories/ChannelImage.v",
        "formal/rocq/stack_safe_pda/theories/PathMapReversePruneTeardown.v",
        "formal/rocq/stack_safe_pda/theories/PathMapReversePruneTeardownMutants.v",
        "formal/rocq/actree03_completeness/theories/Epm1CursorAuthorityV2.v",
        "formal/rocq/actree03_completeness/theories/Epm1CursorPositiveV3.v",
        "formal/rocq/actree03_completeness/theories/Epm1CursorPositiveV4.v",
        "formal/rocq/actree03_completeness/theories/Epm1CursorPositiveV4Mutants.v",
        "formal/rocq/actree03_completeness/theories/Epm1ActConcreteV5.v",
        "formal/rocq/actree03_completeness/theories/Epm1ActConcreteV5Mutants.v",
        "formal/rocq/actree03_completeness/theories/Epm1PositionalAssociationV6.v",
        "formal/rocq/actree03_completeness/theories/Epm1PositionalAssociationV6Mutants.v",
    ] {
        let source = read_repository_file(relative);
        for forbidden in ["Admitted.", "admit.", "Axiom ", "Parameter "] {
            assert!(
                !source.contains(forbidden),
                "{relative} contains an unproved declaration marker `{forbidden}`"
            );
        }
    }
}

#[test]
fn pathmap_reverse_prune_formal_first_artifacts_are_manifested() {
    let rocq =
        read_repository_file("formal/rocq/stack_safe_pda/theories/PathMapReversePruneTeardown.v");
    for marker in [
        "Theorem drain_detaches_originals_exactly_once_in_reverse_postorder",
        "Theorem drain_prunes_every_logical_edge_once",
        "Corollary topology_adversaries_are_drained_edge_by_edge",
        "Theorem immutable_wrapper_clone_does_not_clone_raw_header_or_root_value",
        "Theorem two_wrapper_arc_sequence_elects_exactly_one_final_drop",
        "Theorem make_mut_creates_one_well_formed_fresh_anchor",
        "Theorem cross_anchor_teardown_preserves_the_sibling_image",
        "Theorem every_campaign_owned_exit_drains_before_release",
        "Theorem invariant_catalog_has_exactly_twenty_five_entries",
        "Theorem campaign_branch_safe_pathmap_teardown_capstone",
    ] {
        assert!(
            rocq.contains(marker),
            "PathMap teardown Rocq model is missing `{marker}`"
        );
    }

    let mutants = read_repository_file(
        "formal/rocq/stack_safe_pda/theories/PathMapReversePruneTeardownMutants.v",
    );
    for marker in [
        "kill_value_only_walk_on_topology_only_comb",
        "kill_snapshot_only_strong_count_race",
        "kill_per_wrapper_raw_header_and_root_value_clone",
        "kill_manual_family_count_protocol",
        "kill_unregistered_make_mut_raw_header",
        "kill_early_pending_writer_or_iterator_drop",
        "kill_recursive_par_disposal",
        "every_teardown_mutant_has_a_causal_counterexample",
        "teardown_mutant_registry_has_exactly_twenty_seven_entries",
    ] {
        assert!(
            mutants.contains(marker),
            "PathMap teardown mutants are missing `{marker}`"
        );
    }

    let verus = read_repository_file("formal/verus/pathmap_reverse_prune_teardown.rs");
    for marker in [
        "pub fn executable_one_logical_edge_step(",
        "pub fn executable_root_payload_step(",
        "proof fn reverse_terminal_drive_conserves_edges_and_payloads(",
    ] {
        assert!(
            verus.contains(marker),
            "PathMap teardown Verus proof is missing `{marker}`"
        );
    }
    for forbidden in ["assume(", "admit(", "external_body"] {
        assert!(
            !verus.contains(forbidden),
            "PathMap teardown Verus proof contains `{forbidden}`"
        );
    }

    let tla = read_repository_file("formal/tlaplus/stack_safe_pda/PathMapAnchorRootTeardown.tla");
    for marker in [
        "OneRawHeaderPerActiveAnchor ==",
        "HeaderAndRootRegistrationAgree ==",
        "DeadAnchorRequiresExactlyOnePda ==",
        "NonlastDropTouchesNoRawHeader ==",
        "SnapshotMutantSpec ==",
        "PerWrapperHeaderMutantSpec ==",
        "ManualFamilyMutantSpec ==",
        "UnregisteredMakeMutSpec ==",
    ] {
        assert!(
            tla.contains(marker),
            "PathMap anchor TLA+ model is missing `{marker}`"
        );
    }

    let expected_mutants = [
        (
            "formal/tlaplus/stack_safe_pda/PathMapAnchorRootSnapshotMutant.cfg",
            "DeadAnchorRequiresExactlyOnePda",
        ),
        (
            "formal/tlaplus/stack_safe_pda/PathMapAnchorRootPerWrapperHeaderMutant.cfg",
            "OneRawHeaderPerActiveAnchor",
        ),
        (
            "formal/tlaplus/stack_safe_pda/PathMapAnchorRootManualFamilyMutant.cfg",
            "OneRawHeaderPerActiveAnchor",
        ),
        (
            "formal/tlaplus/stack_safe_pda/PathMapAnchorRootUnregisteredMakeMutMutant.cfg",
            "HeaderAndRootRegistrationAgree",
        ),
    ];
    for (config, invariant) in expected_mutants {
        assert!(
            read_repository_file(config).contains(invariant),
            "PathMap anchor mutant config `{config}` is missing `{invariant}`",
        );
    }

    let coq_project = read_repository_file("formal/rocq/stack_safe_pda/_CoqProject");
    assert!(coq_project.contains("theories/PathMapReversePruneTeardown.v"));
    assert!(coq_project.contains("theories/PathMapReversePruneTeardownMutants.v"));

    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "theories/PathMapReversePruneTeardown.v",
        "theories/PathMapReversePruneTeardownMutants.v",
        "formal/verus/pathmap_reverse_prune_teardown.rs",
        "PathMapAnchorRootTeardown.cfg",
        "DeadAnchorRequiresExactlyOnePda",
        "OneRawHeaderPerActiveAnchor",
        "HeaderAndRootRegistrationAgree",
        "StackSafePDA.PathMapReversePruneTeardown",
        "StackSafePDA.PathMapReversePruneTeardownMutants",
    ] {
        assert!(
            runner.contains(marker),
            "PathMap teardown formal runner is missing `{marker}`"
        );
    }
    for (theory, count) in [
        ("theories/PathMapReversePruneTeardown.v", "32"),
        ("theories/PathMapReversePruneTeardownMutants.v", "13"),
    ] {
        assert!(
            has_runner_command(&runner, &["rocq_compile_with_closed_count", theory, count]),
            "PathMap teardown runner must compile {theory} with {count} closed reports",
        );
    }
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.PathMapReversePruneTeardown",
        "StackSafePDA.PathMapReversePruneTeardownMutants",
    ],));
}

#[test]
fn pathmap_live_write_gateway_contract_is_bound_to_executable_witnesses() {
    let model =
        read_repository_file("formal/rocq/stack_safe_pda/theories/PathMapPhysicalOwnership.v");
    for theorem in [
        "live_normalization_requires_closed_cursor",
        "live_normalization_is_idempotent",
        "live_write_gateway_capstone",
        "live_write_empty_and_nonempty_images_are_exact",
    ] {
        assert!(model.contains(&format!("Print Assumptions {theorem}.")));
    }
    let controls = read_repository_file(
        "formal/rocq/stack_safe_pda/theories/PathMapPhysicalOwnershipMutants.v",
    );
    assert!(controls.contains("Print Assumptions live_write_negative_controls_are_nonvacuous."));
    let tests =
        read_repository_file("rspace++/tests/guarded_pathmap/two_phase_teardown_properties.rs");
    for witness in [
        "live_write_gateways_preserve_normal_and_unwind_outcomes",
        "live_write_gateways_preserve_nonempty_topology_and_siblings",
        "live_write_generated_gateway_observations",
    ] {
        assert!(tests.contains(witness));
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "theories/PathMapPhysicalOwnership.v 12",
        "theories/PathMapPhysicalOwnershipMutants.v 3",
        "-p rspace_plus_plus --lib live_write_",
        "expected exactly three executed live-write gateway witnesses",
    ] {
        assert!(runner.contains(marker));
    }
}

#[test]
fn channel_image_v2_codec_proof_and_executable_boundaries_are_manifested() {
    let rocq = read_repository_file("formal/rocq/stack_safe_pda/theories/ChannelImage.v");
    for marker in [
        "Theorem channel_prefix_parser_is_left_inverse",
        "Theorem channel_exact_parser_is_left_inverse",
        "Theorem canonical_channel_image_is_unique_iff_semantic_par_equality",
        "Theorem root_locally_free_is_quotiented_out",
        "Theorem connective_flag_separates_otherwise_equal_channels",
        "Theorem endpoint_multiplicities_are_preserved_exactly",
        "Theorem every_successfully_decoded_channel_has_canonical_endpoints",
        "Theorem duplicate_endpoint_fails_strict_order_check",
        "Theorem permuted_endpoint_fails_strict_order_check",
        "Theorem noncanonical_rpb3_endpoint_fails_admission_check",
        "Theorem zero_multiplicity_fails_admission_check",
        "Print Assumptions channel_prefix_parser_is_left_inverse.",
        "Print Assumptions channel_exact_parser_is_left_inverse.",
        "Print Assumptions canonical_channel_image_is_unique_iff_semantic_par_equality.",
        "Print Assumptions root_locally_free_is_quotiented_out.",
        "Print Assumptions connective_flag_separates_otherwise_equal_channels.",
        "Print Assumptions endpoint_multiplicities_are_preserved_exactly.",
        "Print Assumptions every_successfully_decoded_channel_has_canonical_endpoints.",
        "Print Assumptions duplicate_endpoint_fails_strict_order_check.",
        "Print Assumptions permuted_endpoint_fails_strict_order_check.",
        "Print Assumptions noncanonical_rpb3_endpoint_fails_admission_check.",
        "Print Assumptions zero_multiplicity_fails_admission_check.",
    ] {
        assert!(
            rocq.contains(marker),
            "ChannelImageV2 Rocq proof is missing `{marker}`"
        );
    }

    let verus = read_repository_file("formal/verus/channel_image_cursor.rs");
    for marker in [
        "pub fn checked_cursor_add(",
        "pub fn checked_bounded_advance(",
        "pub fn checked_cursor_increment(",
        "pub fn executable_u64_from_be_bytes(",
        "pub fn checked_fixed_u64_read(",
        "pub fn checked_escaped_frame_step(",
        "pub fn checked_distinct_increment(",
        "pub fn exact_distinct_count(",
        "proof fn admitted_endpoint_encoder_decoder_refinement(",
        "proof fn admitted_endpoints_encoder_decoder_refinement(",
        "proof fn admitted_channel_encoder_decoder_refinement(",
        "proof fn admitted_channel_boundary_is_non_vacuous(",
    ] {
        assert!(
            verus.contains(marker),
            "ChannelImageV2 Verus proof is missing `{marker}`"
        );
    }
    for forbidden in ["assume(", "admit(", "external_body"] {
        assert!(
            !verus.contains(forbidden),
            "ChannelImageV2 Verus proof contains forbidden escape `{forbidden}`"
        );
    }

    let codec = read_repository_file("models/src/rust/channel_image.rs");
    for marker in [
        "try_visit_canonical_key_parts",
        "take_escaped_frame",
        "validate_canonical_key",
        "DistinctCountMismatch",
        "NonAscendingEndpoint",
    ] {
        assert!(
            codec.contains(marker),
            "ChannelImageV2 codec is missing executable boundary `{marker}`"
        );
    }
    for forbidden in [
        "visit_decoded",
        "decode_occurrences",
        ".sort(",
        ".sort_by",
        "compact_snapshot(",
        "trie_snapshot(",
    ] {
        assert!(
            !codec.contains(forbidden),
            "ChannelImageV2 codec contains forbidden projection `{forbidden}`"
        );
    }

    let oracle = read_repository_file("models/tests/channel_image_v2.rs");
    for marker in [
        "image_is_exactly_a_function_of_par_equality",
        "raw_image_order_matches_par_order_over_the_complete_schema_corpus",
        "decode_reconstructs_the_identity_normalized_par_without_entry_projection",
        "malformed_images_fail_closed_at_every_scalar_and_trie_boundary",
        "deep_and_wide_images_use_the_same_iterative_cursor_on_a_small_stack",
        "arbitrary_bytes_never_escape_the_checked_cursor",
    ] {
        assert!(
            oracle.contains(marker),
            "ChannelImageV2 executable oracle is missing `{marker}`"
        );
    }

    let coq_project = read_repository_file("formal/rocq/stack_safe_pda/_CoqProject");
    assert!(coq_project.contains("theories/ChannelImage.v"));
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "rocq_compile_with_closed_count theories/ChannelImage.v 11",
        "formal/verus/channel_image_cursor.rs",
    ] {
        assert!(
            runner.contains(marker),
            "ChannelImageV2 formal runner is missing `{marker}`"
        );
    }
}

#[test]
fn rmp2_transcript_routing_and_candidate_order_bridges_are_manifested() {
    let verus = read_repository_file("formal/verus/rmp2_par_bag_transcript.rs");
    for marker in [
        "proof fn resident_and_region_accumulators_are_equivalent(",
        "proof fn record_correspondence_precondition_is_non_vacuous(",
        "proof fn corresponding_semantic_fields_project_identically(",
        "pub fn executable_adjacent_coalescer_step(",
        "pub fn executable_finalize_adjacent_distinct(",
        "proof fn nested_par_projection_is_opaque_to_recursive_components(",
        "pub fn executable_saturating_add(",
        "pub fn executable_occurrence_step(",
        "pub fn executable_exact_run_multiplicity_increment(",
        "pub fn executable_body_length(",
    ] {
        assert!(
            verus.contains(marker),
            "RMP2 Verus proof is missing `{marker}`"
        );
    }
    for forbidden in ["assume(", "admit(", "external_body"] {
        assert!(
            !verus.contains(forbidden),
            "RMP2 Verus proof contains forbidden escape `{forbidden}`"
        );
    }

    let rocq = read_repository_file("formal/rocq/stack_safe_pda/theories/RMP2.v");
    for marker in [
        "Theorem emitted_opaque_par_token_has_exactly_49_bytes",
        "Theorem dynamic_shell_classification_is_independent_of_summary",
        "Theorem compiled_opaque_query_has_no_false_negative",
        "Theorem unsupported_opaque_shell_fails_closed",
        "Theorem canonical_candidate_order_is_independent_of_derived_rmp_bytes",
        "Theorem corresponding_semantic_fields_have_identical_projection",
        "Theorem adjacent_coalescing_preserves_exact_cardinality",
        "Theorem nested_par_projection_is_opaque_to_recursive_component_structure",
        "Print Assumptions emitted_opaque_par_token_has_exactly_49_bytes.",
        "Print Assumptions compiled_opaque_query_has_no_false_negative.",
        "Print Assumptions unsupported_opaque_shell_fails_closed.",
        "Print Assumptions canonical_candidate_order_is_independent_of_derived_rmp_bytes.",
        "Print Assumptions corresponding_semantic_fields_have_identical_projection.",
        "Print Assumptions adjacent_coalescing_preserves_exact_cardinality.",
        "Print Assumptions nested_par_projection_is_opaque_to_recursive_component_structure.",
    ] {
        assert!(
            rocq.contains(marker),
            "RMP2 Rocq proof is missing `{marker}`"
        );
    }

    // These are executable boundary witnesses, not claims that Rocq or Verus
    // verified BLAKE2, protobuf parsing, generated Rust, or allocator behavior.
    let model_oracles = read_repository_file("models/tests/match_path_cursor.rs");
    assert!(model_oracles.contains("decoded_tokens_canonically_reencode_every_schema_shape"));
    let transcript_oracles = read_repository_file("models/tests/match_path_structure.rs");
    for marker in [
        "resident_and_region_par_bag_summaries_match_for_every_canonical_family",
        "rmp2_parbag_transcript_preserves_multiplicity_without_occurrence_expansion",
        "resident_and_canonical_region_sources_have_identical_semantic_field_projection",
        "canonical_region_coalesces_only_adjacent_identical_occurrences",
        "nested_par_projection_is_one_opaque_bag_token_plus_root_metadata",
    ] {
        assert!(
            transcript_oracles.contains(marker),
            "RMP2 transcript oracle is missing `{marker}`"
        );
    }
    let matcher_oracles =
        read_repository_file("rholang/tests/support/spatial_matcher_pda_equivalence.rs");
    assert!(matcher_oracles
        .contains("compiled_rho_path_query_is_a_superset_of_every_recursive_oracle_success"));
    let ordering_oracles = read_repository_file("rspace++/src/rspace/data_index.rs");
    assert!(ordering_oracles
        .contains("a_hash_collision_is_ordered_by_identity_not_the_derived_rho_path"));

    let coq_project = read_repository_file("formal/rocq/stack_safe_pda/_CoqProject");
    assert!(coq_project.contains("theories/RMP2.v"));
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "theories/RMP2.v",
        "rmp2_closed_capstones",
        "expected all seven RMP2 Rocq capstones to be closed",
        "formal/verus/rmp2_par_bag_transcript.rs",
    ] {
        assert!(
            runner.contains(marker),
            "RMP2 formal runner is missing `{marker}`"
        );
    }
}

#[test]
fn verus_range_and_worklist_proof_is_non_vacuous_and_runs_without_cheating() {
    let proof = read_repository_file("formal/verus/epathmap_canonical_worklist.rs");
    for marker in [
        "pub fn relative_region(",
        "proof fn canonical_validation_worklist_equivalent(",
    ] {
        assert!(proof.contains(marker), "Verus proof is missing `{marker}`");
    }
    for forbidden in ["assume(", "admit(", "external_body"] {
        assert!(
            !proof.contains(forbidden),
            "Verus proof contains forbidden escape `{forbidden}`"
        );
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(runner.contains("--no-cheating --num-threads 1"));
    assert!(runner.contains("formal/verus/epathmap_canonical_worklist.rs"));
}

#[test]
fn accounting_demand_four_layer_proof_and_mutants_are_manifested() {
    let rocq = read_repository_file("formal/rocq/stack_safe_pda/theories/AccountingDemand.v");
    for marker in [
        "Theorem accounting_pda_refines_recursive_trace",
        "Theorem accounting_pda_terminates",
        "Corollary compressed_multiplicity_saturates_after_scaling",
        "Theorem accounting_pda_preserves_stateful_callback",
        "Print Assumptions accounting_pda_refines_recursive_trace.",
        "Print Assumptions accounting_pda_terminates.",
        "Print Assumptions compressed_multiplicity_saturates_after_scaling.",
        "Print Assumptions accounting_pda_preserves_stateful_callback.",
    ] {
        assert!(
            rocq.contains(marker),
            "accounting Rocq proof is missing `{marker}`"
        );
    }

    let verus = read_repository_file("formal/verus/accounting_demand.rs");
    for marker in [
        "pub fn checked_cursor_advance(",
        "pub fn saturating_scaled_comms(",
        "proof fn whole_trace_replay_successor(",
        "proof fn grouped_replay_mutant_is_detected()",
    ] {
        assert!(
            verus.contains(marker),
            "accounting Verus proof is missing `{marker}`"
        );
    }
    for forbidden in ["assume(", "admit(", "external_body"] {
        assert!(
            !verus.contains(forbidden),
            "accounting Verus proof contains forbidden escape `{forbidden}`"
        );
    }

    let tla = read_repository_file("formal/tlaplus/stack_safe_pda/AccountingDemand.tla");
    for marker in [
        "TracePrefixRefinement ==",
        "CallbackPrefixRefinement ==",
        "SummaryRefinement ==",
        "CompletedTraceEquivalent ==",
        "ActionWitnessesAreNonVacuous ==",
        "GroupedMultiplicityMutationIsDetected ==",
        "FamilyOrderMutationIsDetected ==",
        "BranchOrderMutationIsDetected ==",
        "DroppedMissingCommMutationIsDetected ==",
        "MissingChannelCallbackMutationIsDetected ==",
        "AllReceiveSourcesMutationIsDetected ==",
        "DroppedUnknownMutationIsDetected ==",
        "EventuallyComplete ==",
    ] {
        assert!(
            tla.contains(marker),
            "accounting TLA+ model is missing `{marker}`"
        );
    }
    let tla_config = read_repository_file("formal/tlaplus/stack_safe_pda/AccountingDemand.cfg");
    for marker in [
        "INVARIANT TracePrefixRefinement",
        "INVARIANT CallbackPrefixRefinement",
        "INVARIANT SummaryRefinement",
        "INVARIANT CompletedTraceEquivalent",
        "INVARIANT ActionWitnessesAreNonVacuous",
        "INVARIANT GroupedMultiplicityMutationIsDetected",
        "INVARIANT FamilyOrderMutationIsDetected",
        "INVARIANT BranchOrderMutationIsDetected",
        "INVARIANT DroppedMissingCommMutationIsDetected",
        "INVARIANT MissingChannelCallbackMutationIsDetected",
        "INVARIANT AllReceiveSourcesMutationIsDetected",
        "INVARIANT DroppedUnknownMutationIsDetected",
        "PROPERTY EventuallyComplete",
    ] {
        assert!(
            tla_config.contains(marker),
            "accounting TLC configuration is missing `{marker}`"
        );
    }

    let coq_project = read_repository_file("formal/rocq/stack_safe_pda/_CoqProject");
    assert!(coq_project.contains("theories/AccountingDemand.v"));
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "theories/AccountingDemand.v",
        "accounting_demand_closed_capstones",
        "expected all four accounting-demand Rocq capstones to be closed",
        "formal/verus/accounting_demand.rs",
        "tlc_run \"$tla_metadir\" AccountingDemand.cfg AccountingDemand.tla",
    ] {
        assert!(
            runner.contains(marker),
            "formal runner is missing `{marker}`"
        );
    }
}

#[test]
fn verus_freemap_journal_proof_is_non_vacuous_and_runs_without_cheating() {
    let proof = read_repository_file("formal/verus/free_map_transaction.rs");
    for marker in [
        "pub fn exec_write_refines_spec(",
        "pub fn exec_rollback_refines_spec(",
        "pub fn exec_take_delta_and_rollback_refines_spec(",
        "pub fn exec_finish_refines_spec(",
        "pub fn abort_guard_restores_root_spec(",
        "proof fn undo_len_tracks_completed_writes(",
        "proof fn delta_len_is_bounded_by_segment_writes(",
    ] {
        assert!(proof.contains(marker), "Verus proof is missing `{marker}`");
    }
    for forbidden in ["assume(", "admit(", "external_body"] {
        assert!(
            !proof.contains(forbidden),
            "Verus proof contains forbidden escape `{forbidden}`"
        );
    }
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    let proof_path = "formal/verus/free_map_transaction.rs";
    let proof_path_at = runner
        .find(proof_path)
        .expect("the runner invokes the FreeMap Verus proof");
    let invocation_start = runner[..proof_path_at]
        .rfind("tlc_bounded")
        .expect("the FreeMap Verus proof is resource bounded");
    let invocation = &runner[invocation_start..proof_path_at + proof_path.len()];
    assert!(invocation.contains("--no-cheating --num-threads 1"));
}

#[test]
fn freemap_journal_four_backend_bundle_and_mutants_are_manifested() {
    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");

    let rocq = read_repository_file("formal/rocq/stack_safe_pda/theories/FreeMapJournal.v");
    for marker in [
        "Theorem journal_connective_control_refines_snapshot_semantics",
        "Theorem journal_transaction_refines_snapshot_transaction",
        "Print Assumptions journal_connective_control_refines_snapshot_semantics.",
        "Print Assumptions journal_transaction_refines_snapshot_transaction.",
    ] {
        assert!(
            rocq.contains(marker),
            "FreeMap Rocq proof is missing `{marker}`"
        );
    }
    let coq_project = read_repository_file("formal/rocq/stack_safe_pda/_CoqProject");
    assert!(coq_project.contains("theories/FreeMapJournal.v"));
    for marker in [
        "theories/FreeMapJournal.v",
        "grep -c '^Closed under the global context$'",
        "expected both FreeMap Rocq capstones to be closed",
    ] {
        assert!(
            runner.contains(marker),
            "FreeMap Rocq runner is missing `{marker}`"
        );
    }

    let tla = read_repository_file("formal/tlaplus/stack_safe_pda/FreeMapJournal.tla");
    for marker in [
        "SnapshotJournalLockstep ==",
        "CheckpointSnapshotsAgree ==",
        "ValidationIsAtomic ==",
        "ActionWitnessesAreNonVacuous ==",
        "ReverseUndoMutationIsDetected ==",
        "RollbackAllFrameMutationIsDetected ==",
        "ApplyBeforeValidationMutationIsDetected ==",
        "DropWithoutUndoMutationIsDetected ==",
    ] {
        assert!(
            tla.contains(marker),
            "FreeMap TLA+ model is missing `{marker}`"
        );
    }
    let tla_config = read_repository_file("formal/tlaplus/stack_safe_pda/FreeMapJournal.cfg");
    for invariant in [
        "INVARIANT SnapshotJournalLockstep",
        "INVARIANT CheckpointSnapshotsAgree",
        "INVARIANT ValidationIsAtomic",
        "INVARIANT ActionWitnessesAreNonVacuous",
        "INVARIANT ReverseUndoMutationIsDetected",
        "INVARIANT RollbackAllFrameMutationIsDetected",
        "INVARIANT ApplyBeforeValidationMutationIsDetected",
        "INVARIANT DropWithoutUndoMutationIsDetected",
    ] {
        assert!(
            tla_config.contains(invariant),
            "FreeMap TLC configuration is missing `{invariant}`"
        );
    }
    assert!(runner.contains("tlc_run \"$tla_metadir\" FreeMapJournal.cfg FreeMapJournal.tla"));

    let implementation =
        read_repository_file("formal/mcrl2/stack_safe_pda/free_map_journal_impl.mcrl2");
    let specification =
        read_repository_file("formal/mcrl2/stack_safe_pda/free_map_journal_spec.mcrl2");
    let no_leak = read_repository_file("formal/mcrl2/stack_safe_pda/free_map_journal_no_leak.mcf");
    let or_leak =
        read_repository_file("formal/mcrl2/stack_safe_pda/free_map_journal_or_leak_mutant.mcrl2");
    let partial_apply = read_repository_file(
        "formal/mcrl2/stack_safe_pda/free_map_journal_partial_apply_mutant.mcrl2",
    );
    assert!(implementation.contains("proc Impl ="));
    assert!(specification.contains("proc Spec ="));
    assert!(no_leak.contains("(or_binding_leak + partial_apply)"));
    assert!(or_leak.contains("or_binding_leak . publish_binding"));
    assert!(partial_apply.contains("partial_apply . tau . reject"));
    for marker in [
        "free_map_journal_impl.mcrl2",
        "free_map_journal_spec.mcrl2",
        "free_map_journal_no_leak.mcf",
        "for mutant in or_leak partial_apply; do",
        "expected FreeMap ${mutant} mutant to break bisimulation",
        "expected FreeMap ${mutant} mutant leak witness",
    ] {
        assert!(
            runner.contains(marker),
            "FreeMap mCRL2 runner is missing `{marker}`"
        );
    }
}

#[test]
fn certified_egress_census_replay_depth_and_rollback_bundle_is_manifested() {
    use models::rust::rholang::egress_protobuf_schema_tables::{
        rhoapi_bridge, rholang_scala_rust_types, rspace_plus_plus_types,
        CERTIFIED_EGRESS_CUT_EDGE_DEPTH_REGISTRY,
    };

    // Bind the abstract strict-rank theorem to every generated descriptor edge
    // and to the two hand-certified edges hidden behind opaque cut vertices.
    let depth_registries = [
        rhoapi_bridge::CERTIFIED_EGRESS_EDGE_DEPTH_REGISTRY,
        rholang_scala_rust_types::CERTIFIED_EGRESS_EDGE_DEPTH_REGISTRY,
        rspace_plus_plus_types::CERTIFIED_EGRESS_EDGE_DEPTH_REGISTRY,
        CERTIFIED_EGRESS_CUT_EDGE_DEPTH_REGISTRY,
    ];
    let mut certified_edges = 0usize;
    for registry in depth_registries {
        for &(parent, parent_depth, child, child_depth) in registry {
            assert!(
                parent_depth > child_depth,
                "certified edge `{parent}` ({parent_depth}) -> `{child}` ({child_depth}) does not decrease"
            );
            certified_edges += 1;
        }
    }
    assert!(
        certified_edges > 0,
        "the generated depth binding is vacuous"
    );
    assert_eq!(
        CERTIFIED_EGRESS_CUT_EDGE_DEPTH_REGISTRY,
        &[
            (".rhoapi.EPathMap", 3, ".rhoapi.Var", 2),
            (".rhoapi.EPathMap", 3, ".rhoapi.Par", 1),
        ],
        "the formal depth-3 EPathMap certificate drifted from generated code"
    );

    let proof = read_repository_file("formal/verus/rspace_canonical_egress.rs");
    for marker in [
        "pub proof fn certified_rank_path_is_bounded(",
        "pub proof fn self_cycle_has_no_valid_depth_certificate(",
        "pub proof fn mutual_cycle_has_no_valid_depth_certificate(",
        "pub proof fn underreported_root_bound_mutant_is_rejected(",
        "EPathMap -> Var -> WildcardMsg",
        "pub enum CertifiedLengthEventKindSpec {",
        "pub proof fn reservation_appends_one_stable_slot(",
        "pub proof fn completion_updates_only_the_reserved_slot(",
        "pub proof fn duplicate_completion_mutant_is_rejected(",
        "pub proof fn wrong_kind_completion_mutant_is_rejected(",
        "pub proof fn inline_to_spill_preserves_parent_slot(",
        "pub proof fn replay_next_consumes_exact_recorded_event(",
        "pub proof fn replay_finish_iff_exactly_exhausted(",
        "pub proof fn dropped_replay_event_mutant_is_rejected(",
        "pub proof fn duplicated_replay_event_mutant_is_rejected(",
        "pub proof fn equal_length_kind_swap_mutant_is_rejected(",
        "pub proof fn census_policy_directs_empty_and_singleton_owners(",
        "pub proof fn census_policy_memoizes_only_repeated_owners(",
        "pub proof fn empty_owner_memo_mutant_is_rejected(",
        "pub proof fn singleton_owner_memo_mutant_is_rejected(",
        "pub proof fn certified_frame_error_discards_private_and_preserves_public(",
        "pub proof fn certified_frame_commit_requires_exact_length_and_replay(",
        "pub proof fn premature_replay_commit_mutant_is_rejected(",
    ] {
        assert!(
            proof.contains(marker),
            "certified-egress Verus proof is missing `{marker}`"
        );
    }
    for forbidden in ["assume(", "admit(", "external_body"] {
        assert!(
            !proof.contains(forbidden),
            "certified-egress Verus proof contains forbidden escape `{forbidden}`"
        );
    }

    let tla = read_repository_file("formal/tlaplus/stack_safe_pda/ParBagInFrameMemo.tla");
    for marker in [
        "Lanes == {\"direct\", \"record\", \"reuse\"}",
        "CensusPrefixIsExact ==",
        "CensusCompletesBeforeEmission ==",
        "ReplayConsumesExactOccurrencePrefix ==",
        "ReplayExhaustionIffEmissionComplete ==",
        "DirectLaneForEmptyAndSingleton ==",
        "MemoOnlyForCensusProvenRepeatedOwners ==",
        "OutputMatchesDirect ==",
        "CommitRequiresExactReplay ==",
        "CensusSkipMutantSpec ==",
        "ReplaySwapMutantSpec ==",
        "ReplayOverrunMutantSpec ==",
        "ReplayRemainingCommitMutantSpec ==",
        "EmptyMemoMutantSpec ==",
        "SingletonMemoMutantSpec ==",
        "EarlyEmissionMutantSpec ==",
        "SwapMutantSpec ==",
        "WholeMutantSpec ==",
        "WrongOwnerMutantSpec ==",
        "OffByOneMutantSpec ==",
    ] {
        assert!(
            tla.contains(marker),
            "certified-egress TLA+ model is missing `{marker}`"
        );
    }
    let main_config = read_repository_file("formal/tlaplus/stack_safe_pda/ParBagInFrameMemo.cfg");
    for invariant in [
        "INVARIANT CensusPrefixIsExact",
        "INVARIANT CensusCompletesBeforeEmission",
        "INVARIANT ReplayConsumesExactOccurrencePrefix",
        "INVARIANT ReplayExhaustionIffEmissionComplete",
        "INVARIANT DirectLaneForEmptyAndSingleton",
        "INVARIANT MemoOnlyForCensusProvenRepeatedOwners",
        "INVARIANT OutputMatchesDirect",
        "INVARIANT CommitRequiresExactReplay",
    ] {
        assert!(
            main_config.contains(invariant),
            "certified-egress TLC configuration is missing `{invariant}`"
        );
    }

    let transcript =
        read_repository_file("formal/tlaplus/stack_safe_pda/CertifiedEgressTranscript.tla");
    for marker in [
        "GeneratedKinds ==",
        "AppendEvent(event, nextPc) ==",
        "CompleteSlot(slot, bodyLen, nextPc) ==",
        "ReservedPreorderIsGeneratedPrefix ==",
        "CompletionAfterDescendants ==",
        "CompletionExactlyOnce ==",
        "SealIffWalkDoneAndAllComplete ==",
        "StorageRefinesLogicalTranscript ==",
        "TypedReplayConsumesExactPrefix ==",
        "FinishIffExactExhaustion ==",
        "AtomicPublication ==",
        "EqualLengthKindSwapMutantSpec ==",
        "SpillSlotCorruptionMutantSpec ==",
    ] {
        assert!(
            transcript.contains(marker),
            "structural-transcript TLA+ model is missing `{marker}`"
        );
    }
    let transcript_config =
        read_repository_file("formal/tlaplus/stack_safe_pda/CertifiedEgressTranscript.cfg");
    for invariant in [
        "INVARIANT ReservedPreorderIsGeneratedPrefix",
        "INVARIANT CompletionAfterDescendants",
        "INVARIANT CompletionExactlyOnce",
        "INVARIANT SealIffWalkDoneAndAllComplete",
        "INVARIANT StorageRefinesLogicalTranscript",
        "INVARIANT TypedReplayConsumesExactPrefix",
        "INVARIANT FinishIffExactExhaustion",
        "INVARIANT AtomicPublication",
    ] {
        assert!(
            transcript_config.contains(invariant),
            "structural-transcript TLC configuration is missing `{invariant}`"
        );
    }

    let sink =
        read_repository_file("formal/tlaplus/stack_safe_pda/ParBagProtobufSinkPublication.tla");
    assert!(sink.contains("AbortedPrivateFrameIsDiscarded =="));
    assert!(sink.contains("RollbackMutantSpec =="));
    let sink_config =
        read_repository_file("formal/tlaplus/stack_safe_pda/ParBagProtobufSinkPublication.cfg");
    assert!(sink_config.contains("INVARIANT AbortedPrivateFrameIsDiscarded"));

    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "formal/verus/rspace_canonical_egress.rs",
        "--no-cheating --num-threads 1",
        "ParBagProtobufSinkRollbackMutant.cfg",
        "Invariant AbortedPrivateFrameIsDiscarded is violated",
        "ParBagInFrameMemoCensusSkipMutant.cfg:CensusPrefixIsExact",
        "ParBagInFrameMemoReplaySwapMutant.cfg:ReplayConsumesExactOccurrencePrefix",
        "ParBagInFrameMemoReplayOverrunMutant.cfg:ReplayConsumesExactOccurrencePrefix",
        "ParBagInFrameMemoReplayRemainingCommitMutant.cfg:CommitRequiresExactReplay",
        "ParBagInFrameMemoEmptyMemoMutant.cfg:MemoOnlyForCensusProvenRepeatedOwners",
        "ParBagInFrameMemoSingletonMemoMutant.cfg:MemoOnlyForCensusProvenRepeatedOwners",
        "ParBagInFrameMemoEarlyEmissionMutant.cfg:CensusCompletesBeforeEmission",
        "CertifiedEgressTranscript.cfg CertifiedEgressTranscript.tla",
        "CertifiedEgressTranscriptIncompleteSealMutant.cfg:SealIffWalkDoneAndAllComplete",
        "CertifiedEgressTranscriptDuplicateCompletionMutant.cfg:CompletionExactlyOnce",
        "CertifiedEgressTranscriptKindSwapMutant.cfg:TypedReplayConsumesExactPrefix",
        "CertifiedEgressTranscriptReplayOverrunMutant.cfg:TypeInvariant",
        "CertifiedEgressTranscriptRemainingCommitMutant.cfg:AtomicPublication",
        "CertifiedEgressTranscriptSpillCorruptionMutant.cfg:StorageRefinesLogicalTranscript",
        "CertifiedEgressTranscriptPartialPublicationMutant.cfg:AtomicPublication",
    ] {
        assert!(
            runner.contains(marker),
            "certified-egress formal runner is missing `{marker}`"
        );
    }

    for config in [
        "ParBagInFrameMemoCensusSkipMutant.cfg",
        "ParBagInFrameMemoReplaySwapMutant.cfg",
        "ParBagInFrameMemoReplayOverrunMutant.cfg",
        "ParBagInFrameMemoReplayRemainingCommitMutant.cfg",
        "ParBagInFrameMemoEmptyMemoMutant.cfg",
        "ParBagInFrameMemoSingletonMemoMutant.cfg",
        "ParBagInFrameMemoEarlyEmissionMutant.cfg",
        "ParBagProtobufSinkRollbackMutant.cfg",
        "CertifiedEgressTranscriptIncompleteSealMutant.cfg",
        "CertifiedEgressTranscriptDuplicateCompletionMutant.cfg",
        "CertifiedEgressTranscriptKindSwapMutant.cfg",
        "CertifiedEgressTranscriptReplayOverrunMutant.cfg",
        "CertifiedEgressTranscriptRemainingCommitMutant.cfg",
        "CertifiedEgressTranscriptSpillCorruptionMutant.cfg",
        "CertifiedEgressTranscriptPartialPublicationMutant.cfg",
    ] {
        let path = format!("formal/tlaplus/stack_safe_pda/{config}");
        assert!(
            repository_root().join(&path).is_file(),
            "certified-egress mutant configuration is absent: {path}"
        );
    }
}

#[test]
fn rspace_split_index_authority_proofs_and_mutants_are_manifested() {
    let proof = read_repository_file("formal/verus/rspace_canonical_egress.rs");
    for marker in [
        "pub open spec fn repeat_canonical_record(",
        "pub struct CanonicalRecordCellSpec {",
        "pub struct SplitIndexCountersSpec {",
        "pub proof fn continuation_identity_binding_is_first_wins(",
        "pub proof fn last_wins_continuation_identity_mutant_is_rejected(",
        "pub proof fn canonical_record_insert_has_exact_counts_and_length(",
        "pub proof fn canonical_record_insert_preserves_split_index_coherence(",
        "pub proof fn join_multiplicity_is_accounted_exactly(",
        "pub proof fn collapsed_join_multiplicity_mutant_is_rejected(",
        "pub proof fn owned_deletion_has_exact_counts_and_length(",
        "pub proof fn owned_deletion_preserves_split_index_coherence(",
        "pub proof fn cross_group_deletion_mutant_is_rejected(",
        "pub proof fn deletion_length_mutant_is_rejected(",
    ] {
        assert!(
            proof.contains(marker),
            "split-index Verus proof is missing `{marker}`"
        );
    }
    for stale_term in [
        "pub open spec fn repeat_identity(",
        "one preflighted canonical identity",
        "same identity sequence",
    ] {
        assert!(
            !proof.contains(stale_term),
            "canonical-record egress still uses stale identity-as-record term `{stale_term}`"
        );
    }

    let model = read_repository_file("formal/tlaplus/stack_safe_pda/RSpaceSplitIndexAuthority.tla");
    for marker in [
        "Roots == {\"groups\", \"records\", \"identities\", \"counters\"}",
        "StageRoot(root) ==",
        "SealCompleteAuthority ==",
        "PublishCompleteAuthority ==",
        "PrivateStagingIsInvisible ==",
        "VisibleRootsAreAtomic ==",
        "PublicationRequiresSealedAuthority ==",
        "PublishPartialRootsMutant ==",
        "SealIncompleteAuthorityMutant ==",
        "PublishBeforeSealMutant ==",
    ] {
        assert!(
            model.contains(marker),
            "split-index TLA+ model is missing `{marker}`"
        );
    }

    let main_config =
        read_repository_file("formal/tlaplus/stack_safe_pda/RSpaceSplitIndexAuthority.cfg");
    for invariant in [
        "INVARIANT PrivateStagingIsInvisible",
        "INVARIANT SealRequiresCompleteStaging",
        "INVARIANT PublishedAuthorityIsComplete",
        "INVARIANT VisibleRootsAreAtomic",
        "INVARIANT PublicationRequiresSealedAuthority",
        "INVARIANT PublicationIsExactlyOnce",
        "INVARIANT CountersCoPublishWithCanonicalRecords",
        "INVARIANT IdentityBindingsCoPublishWithGroups",
    ] {
        assert!(
            main_config.contains(invariant),
            "split-index TLC configuration is missing `{invariant}`"
        );
    }

    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "RSpaceSplitIndexAuthority.cfg RSpaceSplitIndexAuthority.tla",
        "RSpaceSplitIndexAuthorityPartialRootsMutant.cfg:VisibleRootsAreAtomic",
        "RSpaceSplitIndexAuthorityIncompleteSealMutant.cfg:SealRequiresCompleteStaging",
        "RSpaceSplitIndexAuthorityPublishBeforeSealMutant.cfg:PublicationRequiresSealedAuthority",
    ] {
        assert!(
            runner.contains(marker),
            "split-index formal runner is missing `{marker}`"
        );
    }

    for config in [
        "RSpaceSplitIndexAuthorityPartialRootsMutant.cfg",
        "RSpaceSplitIndexAuthorityIncompleteSealMutant.cfg",
        "RSpaceSplitIndexAuthorityPublishBeforeSealMutant.cfg",
    ] {
        let path = format!("formal/tlaplus/stack_safe_pda/{config}");
        assert!(
            repository_root().join(&path).is_file(),
            "split-index authority mutant configuration is absent: {path}"
        );
    }
}

#[test]
fn rspace_affine_import_lifecycle_and_negative_controls_are_manifested() {
    let model =
        read_repository_file("formal/tlaplus/stack_safe_pda/RSpaceAffineImportLifecycle.tla");
    for marker in [
        "StageTerminalPage(session) ==",
        "SealComplete(session) ==",
        "PublishPreservingCurrent(session) ==",
        "PublishActivation(session) ==",
        "RejectStaleActivation(session) ==",
        "PublicationRequiresConsumedSeal ==",
        "AtMostOneRacingActivationPublishes ==",
        "ActivationGenerationIsCASProtected ==",
        "PreserveCurrentNeverMovesAuthority ==",
        "PublishIncompleteMutant(session) ==",
        "PublishStaleActivationMutant(session) ==",
        "MoveCurrentOnPreserveMutant(session) ==",
    ] {
        assert!(
            model.contains(marker),
            "affine-import lifecycle model is missing `{marker}`"
        );
    }

    let main_config =
        read_repository_file("formal/tlaplus/stack_safe_pda/RSpaceAffineImportLifecycle.cfg");
    for invariant in [
        "INVARIANT SealRequiresCompleteClosure",
        "INVARIANT PublicationRequiresConsumedSeal",
        "INVARIANT CurrentRootIsVerified",
        "INVARIANT AtMostOneRacingActivationPublishes",
        "INVARIANT ActivationGenerationIsCASProtected",
        "INVARIANT PreserveCurrentNeverMovesAuthority",
    ] {
        assert!(
            main_config.contains(invariant),
            "affine-import TLC configuration is missing `{invariant}`"
        );
    }

    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "RSpaceAffineImportLifecycle.cfg RSpaceAffineImportLifecycle.tla",
        "RSpaceAffineImportLifecycleIncompleteMutant.cfg:PublicationRequiresConsumedSeal",
        "RSpaceAffineImportLifecycleStaleActivationMutant.cfg:AtMostOneRacingActivationPublishes",
        "RSpaceAffineImportLifecyclePreserveMutationMutant.cfg:PreserveCurrentNeverMovesAuthority",
    ] {
        assert!(
            runner.contains(marker),
            "affine-import formal runner is missing `{marker}`"
        );
    }

    for config in [
        "RSpaceAffineImportLifecycleIncompleteMutant.cfg",
        "RSpaceAffineImportLifecycleStaleActivationMutant.cfg",
        "RSpaceAffineImportLifecyclePreserveMutationMutant.cfg",
    ] {
        let path = format!("formal/tlaplus/stack_safe_pda/{config}");
        assert!(
            repository_root().join(&path).is_file(),
            "affine-import mutant configuration is absent: {path}"
        );
    }
}

#[test]
fn rspace_atomic_checkpoint_authority_and_negative_controls_are_manifested() {
    let model =
        read_repository_file("formal/tlaplus/stack_safe_pda/RSpaceAtomicCheckpointAuthority.tla");
    for marker in [
        "CommitPrepared(plan) ==",
        "CommitNoOp ==",
        "RejectPlan(plan) ==",
        "ReactivateOld ==",
        "CurrentRootIsVerified ==",
        "VerifiedRootsHaveCompleteClosure ==",
        "ContentKeysAreImmutable ==",
        "NoUnpublishedCheckpointObjects ==",
        "GenerationAdvancesExactlyOnActivation ==",
        "CommitUsesExactBase ==",
        "FailedCommitLeavesAuthorityUnchanged ==",
        "NoOpLeavesAuthorityUnchanged ==",
        "SplitObjectWriteMutant(plan) ==",
        "RootOnlyCompareMutant(plan) ==",
        "TrustCallerTargetMutant ==",
    ] {
        assert!(
            model.contains(marker),
            "atomic-checkpoint authority TLA+ model is missing `{marker}`"
        );
    }

    let main_config =
        read_repository_file("formal/tlaplus/stack_safe_pda/RSpaceAtomicCheckpointAuthority.cfg");
    for invariant in [
        "INVARIANT CurrentRootIsVerified",
        "INVARIANT VerifiedRootsHaveCompleteClosure",
        "INVARIANT ContentKeysAreImmutable",
        "INVARIANT NoUnpublishedCheckpointObjects",
        "INVARIANT GenerationAdvancesExactlyOnActivation",
        "INVARIANT CommitUsesExactBase",
        "INVARIANT FailedCommitLeavesAuthorityUnchanged",
        "INVARIANT NoOpLeavesAuthorityUnchanged",
    ] {
        assert!(
            main_config.contains(invariant),
            "atomic-checkpoint TLC configuration is missing `{invariant}`"
        );
    }

    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "RSpaceAtomicCheckpointAuthority.cfg RSpaceAtomicCheckpointAuthority.tla",
        "RSpaceAtomicCheckpointAuthoritySplitMutant.cfg:NoUnpublishedCheckpointObjects",
        "RSpaceAtomicCheckpointAuthorityRootOnlyMutant.cfg:CommitUsesExactBase",
        "RSpaceAtomicCheckpointAuthorityTrustTargetMutant.cfg:VerifiedRootsHaveCompleteClosure",
    ] {
        assert!(
            runner.contains(marker),
            "atomic-checkpoint formal runner is missing `{marker}`"
        );
    }

    for config in [
        "RSpaceAtomicCheckpointAuthoritySplitMutant.cfg",
        "RSpaceAtomicCheckpointAuthorityRootOnlyMutant.cfg",
        "RSpaceAtomicCheckpointAuthorityTrustTargetMutant.cfg",
    ] {
        let path = format!("formal/tlaplus/stack_safe_pda/{config}");
        assert!(
            repository_root().join(&path).is_file(),
            "atomic-checkpoint authority mutant configuration is absent: {path}"
        );
    }
}

#[test]
fn rho_path_v2_lifecycle_formal_artifacts_are_manifested() {
    let model = read_repository_file("formal/tlaplus/stack_safe_pda/RhoPathV2Lifecycle.tla");
    for marker in [
        "Produce(nextLane) ==",
        "Install ==",
        "Consume ==",
        "Commit ==",
        "Checkpoint ==",
        "Reset ==",
        "Rebuild ==",
        "Export ==",
        "Import ==",
        "Replay ==",
        "COMM ==",
        "FullImageCollisionSafety ==",
        "DistinctCollisionRepresentativesPreserved ==",
        "NoPartialPublication ==",
    ] {
        assert!(
            model.contains(marker),
            "RhoPath V2 lifecycle TLA+ model is missing `{marker}`"
        );
    }

    let config = read_repository_file("formal/tlaplus/stack_safe_pda/RhoPathV2Lifecycle.cfg");
    for invariant in [
        "INVARIANT TypeInvariant",
        "INVARIANT FullImageCollisionSafety",
        "INVARIANT AuthoritativeImageIsAdmitted",
        "INVARIANT SemanticRootMatchesImage",
        "INVARIANT LaneMatchesImageAfterCommit",
        "INVARIANT NoPartialPublication",
        "INVARIANT ReplayedImageWasExported",
    ] {
        assert!(
            config.contains(invariant),
            "RhoPath V2 lifecycle TLC configuration is missing `{invariant}`"
        );
    }

    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(runner.contains("RhoPathV2Lifecycle.cfg"));
    assert!(runner.contains("RhoPathV2Lifecycle.tla"));
    assert!(runner.contains("RhoPathV2LifecycleDigestOnlyMutant.cfg"));
    assert!(runner.contains("RhoPathV2LifecycleDigestOnlyMutant.tla"));
    assert!(runner.contains("RhoPathV2LifecycleOverwriteMutant.cfg"));
    assert!(runner.contains("RhoPathV2LifecycleDecodeLegacyMutant.cfg"));
    assert!(repository_root()
        .join("formal/tlaplus/stack_safe_pda/RhoPathV2LifecycleDigestOnlyMutant.cfg")
        .is_file());
    assert!(repository_root()
        .join("formal/tlaplus/stack_safe_pda/RhoPathV2LifecycleOverwriteMutant.cfg")
        .is_file());
    assert!(repository_root()
        .join("formal/tlaplus/stack_safe_pda/RhoPathV2LifecycleDecodeLegacyMutant.cfg")
        .is_file());
    let mcrl2_runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "rho_path_v2_lifecycle_impl.mcrl2",
        "rho_path_v2_lifecycle_spec.mcrl2",
        "RhoPath V2 branching bisimulation",
        "continuation_join_lifecycle_equivalence::legacy_and_pathmap_continuation_join_lifecycles_are_identical",
    ] {
        assert!(
            mcrl2_runner.contains(marker),
            "RhoPath V2 mCRL2 runner is missing `{marker}`"
        );
    }

    // The formal lifecycle model is only useful as an implementation
    // refinement when the normal Rust differential lifecycle suite is part of
    // the same evidence surface.  Keep this binding explicit so a model can
    // never silently drift away from the production continuation/join path.
    let lifecycle = read_repository_file(
        "rspace++/tests/record_index/continuation_join_lifecycle_equivalence.rs",
    );
    for marker in [
        "legacy_and_pathmap_continuation_join_lifecycles_are_identical",
        "RhoPathV2",
        "checkpoint",
        "replay",
    ] {
        assert!(
            lifecycle.contains(marker),
            "RhoPath V2 lifecycle differential binding is missing `{marker}`"
        );
    }
}

#[test]
fn rpb3_admission_formal_artifacts_are_manifested() {
    let rocq = read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3Admission.v");
    for marker in [
        "rpb3_pda_equivalent_to_recursive_validation",
        "rpb3_compiled_program_evaluates_to_result",
        "Print Assumptions rpb3_pda_equivalent_to_recursive_validation",
        "generated_descriptor_raw_wire_source_indexed_summary_equivalence",
        "generated_descriptor_raw_wire_source_indexed_verdict_equivalence",
        "Print Assumptions generated_descriptor_raw_wire_source_indexed_summary_equivalence",
        "Print Assumptions generated_descriptor_raw_wire_source_indexed_verdict_equivalence",
    ] {
        assert!(
            rocq.contains(marker),
            "RPB3 Rocq theory is missing `{marker}`"
        );
    }

    let source_frames =
        read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3SourceIndexedFrames.v");
    for marker in [
        "source_indexed_frames_refine_recursive_reference",
        "source_indexed_root_has_universal_recursive_summary",
        "source_indexed_root_has_universal_recursive_verdict",
        "source_indexed_root_full_result_refines_recursive_reference",
        "source_indexed_root_claimed_score_refines_recursive_reference",
        "Print Assumptions source_indexed_root_full_result_refines_recursive_reference",
        "Print Assumptions source_indexed_root_claimed_score_refines_recursive_reference",
    ] {
        assert!(
            source_frames.contains(marker),
            "RPB3 source-frame theory is missing `{marker}`"
        );
    }

    let recursive_source = read_repository_file(
        "formal/rocq/stack_safe_pda/theories/RPB3RecursiveAuxiliarySourceReference.v",
    );
    for marker in [
        "ordered_mixed_new_map_returnsb_iff_source_order",
        "accepted_mixed_new_has_strict_typed_map_key_order",
        "ordered_two_absent_value_maps_have_mixed_typed_reference",
        "reversed_two_absent_value_maps_have_no_mixed_typed_reference",
        "empty_pathmap_source_independent_typed_recursive_bridge",
        "pathmap_source_independent_typed_recursive_bridge",
        "present_string_map_source_independent_typed_recursive_bridge",
        "empty_pathmap_generated_roots_have_related_results",
        "malformed_pathmap_is_refused_by_both_mixed_roots",
        "Print Assumptions ordered_mixed_new_map_returnsb_iff_source_order",
        "Print Assumptions reversed_two_absent_value_maps_have_no_mixed_typed_reference",
        "Print Assumptions empty_pathmap_source_independent_typed_recursive_bridge",
        "Print Assumptions pathmap_source_independent_typed_recursive_bridge",
        "Print Assumptions present_string_map_source_independent_typed_recursive_bridge",
    ] {
        assert!(
            recursive_source.contains(marker),
            "RPB3 recursive source theory is missing `{marker}`"
        );
    }

    let mutants =
        read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3AdmissionMutants.v");
    for marker in [
        "mutant_omit_locality_is_refuted",
        "mutant_trailing_acceptance_is_refuted",
        "source_indexed_ordered_generated_new_is_nonvacuous",
        "source_indexed_reversed_generated_new_is_refused",
        "source_indexed_nested_pathmap_is_nonvacuous",
        "source_indexed_ordered_new_derives_exact_score",
        "source_indexed_explicit_scalar_default_is_refused",
        "source_indexed_trailing_root_byte_is_refused",
        "source_indexed_ordered_new_accepts_exact_claimed_score",
        "source_indexed_ordered_new_rejects_forged_claimed_score",
        "source_indexed_private_unforgeable_derives_exact_score",
        "source_indexed_private_unforgeable_accepts_exact_claimed_score",
        "source_indexed_private_unforgeable_rejects_forged_claimed_score",
        "source_indexed_truncated_private_unforgeable_is_refused",
        "mutant_scalar_visit_skips_checked_source_is_refuted",
        "mutant_auxiliary_visit_uses_raw_leaf_is_refuted",
        "mutant_forest_swaps_first_two_is_refuted",
        "mutant_close_omits_child_reverse_is_refuted",
        "Print Assumptions mutant_omit_locality_is_refuted",
        "Print Assumptions source_indexed_ordered_new_derives_exact_score",
        "Print Assumptions source_indexed_explicit_scalar_default_is_refused",
        "Print Assumptions source_indexed_trailing_root_byte_is_refused",
        "Print Assumptions source_indexed_ordered_new_accepts_exact_claimed_score",
        "Print Assumptions source_indexed_ordered_new_rejects_forged_claimed_score",
        "Print Assumptions source_indexed_private_unforgeable_derives_exact_score",
        "Print Assumptions source_indexed_private_unforgeable_accepts_exact_claimed_score",
        "Print Assumptions source_indexed_private_unforgeable_rejects_forged_claimed_score",
        "Print Assumptions source_indexed_truncated_private_unforgeable_is_refused",
        "Print Assumptions mutant_close_omits_child_reverse_is_refuted",
    ] {
        assert!(
            mutants.contains(marker),
            "RPB3 mutant theory is missing `{marker}`"
        );
    }

    let projection =
        read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3DescriptorProjectionData.v");
    assert!(projection.contains("Definition generated_programs"));
    assert!(projection.contains("Definition raw_par_program"));
    let descriptor_laws =
        read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3DescriptorLaws.v");
    for marker in [
        "descriptor_table_well_formed",
        "raw_par_substituted",
        "descriptor_table_nonvacuous",
        "resolved_tag_is_the_only_binding",
        "Print Assumptions resolved_tag_is_the_only_binding",
    ] {
        assert!(
            descriptor_laws.contains(marker),
            "RPB3 descriptor laws are missing `{marker}`"
        );
    }
    let refinement =
        read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3GeneratedFieldRefinement.v");
    for marker in [
        "explicit_worklist_equals_independent_typed_reference",
        "explicit_frames_finish_within_the_structural_budget",
        "Print Assumptions explicit_worklist_equals_independent_typed_reference",
    ] {
        assert!(
            refinement.contains(marker),
            "RPB3 field refinement is missing `{marker}`"
        );
    }
    let wire_cursor = read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3WireCursor.v");
    for marker in [
        "worklist_and_reference_agree_on_every_finite_region",
        "finite_region_scan_terminates_without_budget_exhaustion",
        "Print Assumptions truncated_or_nonminimal_varints_refuse",
    ] {
        assert!(
            wire_cursor.contains(marker),
            "RPB3 wire cursor is missing `{marker}`"
        );
    }
    let semantic_reducer =
        read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3SemanticReducer.v");
    for marker in [
        "generated_par_component_type_ids_are_exact",
        "required_component_score_contract_is_exact",
        "generated_cost_token_stack_requires_component_score",
        "generated_connective_scalar_rules_are_boolean",
        "connective_scalar_constructor_domain_is_generated",
        "absent_connective_has_exact_integer_score",
        "connective_scalar_score_uses_checked_source_boolean",
        "connective_not_score_uses_checked_par_child_evidence",
        "connective_not_without_child_score_remains_underived",
        "connective_not_empty_par_source_derives_exact_score",
        "connective_not_source_mutants_are_refused_or_underived",
        "connective_invalid_scalar_source_remains_underived",
        "connective_wrong_wire_remains_underived",
        "connective_refused_child_remains_underived",
        "generated_unforgeable_arms_are_exact",
        "unforgeable_arm_contract_uses_generated_child_type",
        "absent_unforgeable_has_exact_constructor_score",
        "unforgeable_bytes_arm_uses_checked_nested_source",
        "unforgeable_malformed_nested_source_remains_underived",
        "unforgeable_wrong_child_type_remains_underived",
        "generated_match_case_free_count_is_checked_pbint32",
        "match_case_exact_score_uses_source_checked_free_count",
        "match_case_without_pattern_score_remains_underived",
        "receive_bind_wrong_source_wire_remains_underived",
        "receive_bind_wrong_source_type_remains_underived",
        "receive_bind_wrong_pattern_type_remains_underived",
        "receive_bind_wrong_remainder_type_remains_underived",
        "receive_bind_wrong_cost_type_remains_underived",
        "match_case_wrong_pattern_wire_remains_underived",
        "match_case_wrong_pattern_type_remains_underived",
        "match_case_wrong_source_type_remains_underived",
        "match_case_wrong_guard_type_remains_underived",
        "source_default_match_case_guard_is_detected",
        "generated_receive_parent_scalar_rules_are_exact",
        "generated_match_parent_scalar_rule_is_exact",
        "complete_receive_score_retains_checked_source_values",
        "complete_match_score_retains_checked_source_values",
        "complete_receive_default_condition_has_no_claimed_score",
        "complete_match_default_guard_has_no_claimed_score",
        "complete_receive_without_optional_fields_has_exact_score",
        "complete_match_without_connective_has_exact_score",
        "complete_receive_refuses_present_default_condition",
        "complete_match_default_guard_blocks_claimed_score",
        "cost_token_stack_retains_all_layer_scores",
        "cost_region_retains_scalar_coordinates_signer_and_ordinal",
        "source_cost_region_retains_signer_and_big_endian_coordinate",
        "omitting_a_cost_layer_changes_the_stack_score",
        "Print Assumptions required_component_score_contract_is_exact",
        "Print Assumptions generated_connective_scalar_rules_are_boolean",
        "Print Assumptions connective_scalar_score_uses_checked_source_boolean",
        "Print Assumptions connective_not_score_uses_checked_par_child_evidence",
        "Print Assumptions connective_not_empty_par_source_derives_exact_score",
        "Print Assumptions unforgeable_bytes_arm_uses_checked_nested_source",
        "Print Assumptions unforgeable_malformed_nested_source_remains_underived",
        "Print Assumptions generated_match_case_free_count_is_checked_pbint32",
        "Print Assumptions receive_bind_wrong_source_wire_remains_underived",
        "Print Assumptions match_case_wrong_pattern_wire_remains_underived",
        "Print Assumptions source_default_match_case_guard_is_detected",
        "Print Assumptions generated_receive_parent_scalar_rules_are_exact",
        "Print Assumptions complete_receive_score_retains_checked_source_values",
        "Print Assumptions complete_match_score_retains_checked_source_values",
        "Print Assumptions complete_receive_without_optional_fields_has_exact_score",
        "Print Assumptions complete_match_without_connective_has_exact_score",
        "Print Assumptions complete_receive_refuses_present_default_condition",
        "Print Assumptions cost_token_stack_retains_all_layer_scores",
        "Print Assumptions omitting_a_cost_layer_changes_the_stack_score",
        "reversed_incomplete_sends_are_definitely_refused",
        "outer_send_completeness_ignores_nested_incomplete_component",
        "empty_epathmap_is_definitely_refused",
        "Print Assumptions empty_epathmap_is_definitely_refused",
    ] {
        assert!(
            semantic_reducer.contains(marker),
            "RPB3 semantic reducer is missing `{marker}`"
        );
    }

    let verus = read_repository_file("formal/verus/rpb3_admission_cursor.rs");
    for marker in [
        "checked_region",
        "worklist_step",
        "child_frame_step",
        "root_consumption_authorized",
        "rejects_unbalanced_root",
        "rejects_duplicate_slot",
    ] {
        assert!(
            verus.contains(marker),
            "RPB3 Verus refinement is missing `{marker}`"
        );
    }

    let coq_project = read_repository_file("formal/rocq/stack_safe_pda/_CoqProject");
    assert!(coq_project.contains("theories/RPB3Admission.v"));
    assert!(coq_project.contains("theories/RPB3DescriptorProjectionData.v"));
    assert!(coq_project.contains("theories/RPB3DescriptorLaws.v"));
    assert!(coq_project.contains("theories/RPB3WireCursor.v"));
    assert!(coq_project.contains("theories/RPB3GeneratedFieldRefinement.v"));
    assert!(coq_project.contains("theories/RPB3RawRegionTree.v"));
    assert!(coq_project.contains("theories/RPB3SourceScalarReduction.v"));
    assert!(coq_project.contains("theories/RPB3SourceReferenceIntegration.v"));
    assert!(coq_project.contains("theories/RPB3AuxiliaryTypedReference.v"));
    assert!(coq_project.contains("theories/RPB3SemanticReducer.v"));
    assert!(coq_project.contains("theories/RPB3SemanticWorklist.v"));
    assert!(coq_project.contains("theories/RPB3SemanticWorklistMutants.v"));
    assert!(coq_project.contains("theories/RPB3RecursiveAuxiliarySourceReference.v"));
    for theory in [
        "RPB3IndependentOracleCore",
        "RPB3IndependentOracleAdmission",
        "RPB3IndependentOracleCost",
        "RPB3IndependentOracleProcess",
        "RPB3IndependentOracleName",
        "RPB3IndependentOracleRecursive",
        "RPB3IndependentOracleFuel",
        "RPB3IndependentOracleAdequacy",
        "RPB3IndependentOracleProjection",
        "RPB3IndependentOracleProcessProjection",
        "RPB3IndependentOracleNameProjection",
        "RPB3IndependentOracleNameDecoderBridge",
        "RPB3IndependentOracleMutants",
        "RPB3IndependentOracleRecursiveProjection",
        "RPB3IndependentOracleProcessScalarDecoderBridge",
        "RPB3IndependentOracleCostScalarDecoderBridge",
        "RPB3IndependentOracleCostChildCorrespondence",
        "RPB3IndependentOracleCostVerdictBridge",
        "RPB3IndependentOracleGeneratedResolverVerdict",
        "RPB3IndependentOracleCostScalarTypeIdMutant",
        "RPB3IndependentOracleProcessChildProvenance",
        "RPB3IndependentOracleParScalarScoreBridge",
        "RPB3IndependentOracleParBytesChildBridge",
        "RPB3IndependentOracleParMessageChildBridge",
        "RPB3IndependentOracleRegionRebaseBridge",
        "RPB3IndependentOracleParMessageBodyDecodeBridge",
        "RPB3IndependentOracleParMessageRawRebaseBridge",
        "RPB3IndependentOracleStringMapRegionRebaseBridge",
        "RPB3IndependentOracleStringMapTypedRebaseBridge",
        "RPB3IndependentOracleParMessageEvaluationRebaseBridge",
        "RPB3IndependentOracleExprGroundScoreControl",
        "RPB3IndependentOracleExprFixed64BitwordBridge",
        "RPB3IndependentOracleExprNestedGroundControl",
        "RPB3IndependentOracleExprNestedGroundChildBridge",
    ] {
        assert!(coq_project.contains(&format!("theories/{theory}.v")));
    }
    assert!(coq_project.contains("theories/RPB3SourceIndexedFrames.v"));
    assert!(coq_project.contains("theories/RPB3ResidentParAdmissionBridge.v"));
    assert!(coq_project.contains("theories/RPB3AdmissionMutants.v"));

    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    assert!(runner.lines().any(|line| {
        line.trim() == "if [[ \"$rpb3_admission_mutants_closed_capstones\" != \"40\" ]]; then"
    }));
    for marker in [
        "theories/RPB3Admission.v",
        "theories/RPB3DescriptorProjectionData.v",
        "theories/RPB3DescriptorLaws.v",
        "theories/RPB3WireCursor.v",
        "theories/RPB3GeneratedFieldRefinement.v",
        "theories/RPB3RawRegionTree.v",
        "theories/RPB3SourceScalarReduction.v",
        "theories/RPB3SourceReferenceIntegration.v",
        "theories/RPB3AuxiliaryTypedReference.v",
        "theories/RPB3SemanticReducer.v",
        "theories/RPB3SemanticWorklist.v",
        "theories/RPB3SemanticWorklistMutants.v",
        "theories/RPB3RecursiveAuxiliarySourceReference.v",
        "theories/RPB3IndependentOracleCore.v",
        "theories/RPB3IndependentOracleAdmission.v",
        "theories/RPB3IndependentOracleCost.v",
        "theories/RPB3IndependentOracleProcess.v",
        "theories/RPB3IndependentOracleName.v",
        "theories/RPB3IndependentOracleRecursive.v",
        "theories/RPB3IndependentOracleFuel.v",
        "theories/RPB3IndependentOracleAdequacy.v",
        "theories/RPB3IndependentOracleProjection.v",
        "theories/RPB3IndependentOracleProcessProjection.v",
        "theories/RPB3IndependentOracleNameProjection.v",
        "theories/RPB3IndependentOracleNameDecoderBridge.v",
        "theories/RPB3IndependentOracleMutants.v",
        "theories/RPB3IndependentOracleRecursiveProjection.v",
        "theories/RPB3IndependentOracleProcessScalarDecoderBridge.v",
        "theories/RPB3IndependentOracleCostScalarDecoderBridge.v",
        "theories/RPB3IndependentOracleCostChildCorrespondence.v",
        "theories/RPB3IndependentOracleCostVerdictBridge.v",
        "theories/RPB3IndependentOracleGeneratedResolverVerdict.v",
        "theories/RPB3IndependentOracleCostScalarTypeIdMutant.v",
        "theories/RPB3IndependentOracleProcessChildProvenance.v",
        "theories/RPB3IndependentOracleParScalarScoreBridge.v",
        "theories/RPB3IndependentOracleParBytesChildBridge.v",
        "theories/RPB3IndependentOracleParMessageChildBridge.v",
        "theories/RPB3IndependentOracleRegionRebaseBridge.v",
        "theories/RPB3IndependentOracleParMessageBodyDecodeBridge.v",
        "theories/RPB3IndependentOracleParMessageRawRebaseBridge.v",
        "theories/RPB3IndependentOracleStringMapRegionRebaseBridge.v",
        "theories/RPB3IndependentOracleStringMapTypedRebaseBridge.v",
        "theories/RPB3IndependentOracleParMessageEvaluationRebaseBridge.v",
        "theories/RPB3IndependentOracleExprGroundScoreControl.v",
        "theories/RPB3IndependentOracleExprFixed64BitwordBridge.v",
        "theories/RPB3IndependentOracleExprNestedGroundControl.v",
        "theories/RPB3IndependentOracleExprNestedGroundChildBridge.v",
        "theories/RPB3SourceIndexedFrames.v",
        "theories/RPB3ResidentParAdmissionBridge.v",
        "theories/RPB3AdmissionMutants.v",
        "rpb3_admission_mutants_closed_capstones",
        "formal/verus/rpb3_admission_cursor.rs",
        "StackSafePDA.RPB3AdmissionMutants",
        "StackSafePDA.RPB3SourceIndexedFrames",
        "StackSafePDA.RPB3GeneratedFieldRefinement",
        "StackSafePDA.RPB3DescriptorProjectionData",
        "StackSafePDA.RPB3DescriptorLaws",
        "StackSafePDA.RPB3WireCursor",
        "StackSafePDA.RPB3SemanticReducer",
        "rpb3_run_tests 22 --lib protobuf_admission::tests",
        "production_component_admission_is_differentially_equivalent_for_all_ten_families",
        "rpb3_run_tests 3 --test par_bag admitted_",
    ] {
        assert!(
            runner.contains(marker),
            "RPB3 formal runner is missing `{marker}`"
        );
    }
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/RPB3Admission.v",
        "371"
    ],));
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/RPB3DescriptorProjectionData.v",
        "0"
    ],));
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/RPB3DescriptorLaws.v",
        "5"
    ],));
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/RPB3WireCursor.v",
        "5"
    ],));
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/RPB3SemanticReducer.v",
        "125"
    ],));
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/RPB3RecursiveAuxiliarySourceReference.v",
        "40"
    ],));
    for (theory, count) in [
        ("RPB3IndependentOracleCore", "3"),
        ("RPB3IndependentOracleAdmission", "2"),
        ("RPB3IndependentOracleCost", "9"),
        ("RPB3IndependentOracleProcess", "10"),
        ("RPB3IndependentOracleName", "9"),
        ("RPB3IndependentOracleRecursive", "6"),
        ("RPB3IndependentOracleFuel", "9"),
        ("RPB3IndependentOracleAdequacy", "4"),
        ("RPB3IndependentOracleProjection", "19"),
        ("RPB3IndependentOracleProcessProjection", "19"),
        ("RPB3IndependentOracleNameProjection", "15"),
        ("RPB3IndependentOracleNameDecoderBridge", "6"),
        ("RPB3IndependentOracleMutants", "3"),
        ("RPB3IndependentOracleRecursiveProjection", "38"),
        ("RPB3IndependentOracleProcessScalarDecoderBridge", "16"),
        ("RPB3IndependentOracleCostScalarDecoderBridge", "13"),
        ("RPB3IndependentOracleCostChildCorrespondence", "15"),
        ("RPB3IndependentOracleCostVerdictBridge", "59"),
        ("RPB3IndependentOracleGeneratedResolverVerdict", "11"),
        ("RPB3IndependentOracleCostScalarTypeIdMutant", "3"),
        ("RPB3IndependentOracleProcessChildProvenance", "20"),
        ("RPB3IndependentOracleParScalarScoreBridge", "15"),
        ("RPB3IndependentOracleParBytesChildBridge", "7"),
        ("RPB3IndependentOracleParMessageChildBridge", "4"),
        ("RPB3IndependentOracleRegionRebaseBridge", "21"),
        ("RPB3IndependentOracleParMessageBodyDecodeBridge", "4"),
        ("RPB3IndependentOracleParMessageRawRebaseBridge", "3"),
        ("RPB3IndependentOracleStringMapRegionRebaseBridge", "4"),
        ("RPB3IndependentOracleStringMapTypedRebaseBridge", "6"),
        ("RPB3IndependentOracleParMessageEvaluationRebaseBridge", "1"),
        ("RPB3IndependentOracleExprGroundScoreControl", "1"),
        ("RPB3IndependentOracleExprFixed64BitwordBridge", "4"),
        ("RPB3IndependentOracleExprNestedGroundControl", "16"),
        ("RPB3IndependentOracleExprNestedGroundChildBridge", "7"),
    ] {
        assert!(has_runner_command(&runner, &[
            "rocq_compile_with_closed_count",
            &format!("theories/{theory}.v"),
            count
        ]));
    }
    assert!(has_runner_command(&runner, &[
        "rocq_compile_with_closed_count",
        "theories/RPB3GeneratedFieldRefinement.v",
        "10"
    ],));
    assert!(
        runner
            .lines()
            .any(|line| line == "canonical_key_protocol_proofs"),
        "the default full proof gate must invoke canonical_key_protocol_proofs",
    );
    assert!(has_runner_kernel_check(&runner, &[
        "StackSafePDA.RPB3Admission",
        "StackSafePDA.RPB3DescriptorProjectionData",
        "StackSafePDA.RPB3DescriptorLaws",
        "StackSafePDA.RPB3ProjectionResolutionExact",
        "StackSafePDA.RPB3WireCursor",
        "StackSafePDA.RPB3GeneratedFieldRefinement",
        "StackSafePDA.RPB3RawRegionTree",
        "StackSafePDA.RPB3SourceScalarReduction",
        "StackSafePDA.RPB3SourceReferenceIntegration",
        "StackSafePDA.RPB3AuxiliaryTypedReference",
        "StackSafePDA.RPB3SemanticReducer",
        "StackSafePDA.RPB3SemanticWorklist",
        "StackSafePDA.RPB3SemanticWorklistMutants",
        "StackSafePDA.RPB3RecursiveAuxiliarySourceReference",
        "StackSafePDA.RPB3IndependentOracleCore",
        "StackSafePDA.RPB3IndependentOracleAdmission",
        "StackSafePDA.RPB3IndependentOracleCost",
        "StackSafePDA.RPB3IndependentOracleProcess",
        "StackSafePDA.RPB3IndependentOracleName",
        "StackSafePDA.RPB3IndependentOracleRecursive",
        "StackSafePDA.RPB3IndependentOracleFuel",
        "StackSafePDA.RPB3IndependentOracleAdequacy",
        "StackSafePDA.RPB3IndependentOracleProjection",
        "StackSafePDA.RPB3IndependentOracleProcessProjection",
        "StackSafePDA.RPB3IndependentOracleNameProjection",
        "StackSafePDA.RPB3IndependentOracleNameDecoderBridge",
        "StackSafePDA.RPB3IndependentOracleMutants",
        "StackSafePDA.RPB3IndependentOracleRecursiveProjection",
        "StackSafePDA.RPB3IndependentOracleProcessScalarDecoderBridge",
        "StackSafePDA.RPB3IndependentOracleCostScalarDecoderBridge",
        "StackSafePDA.RPB3IndependentOracleCostChildCorrespondence",
        "StackSafePDA.RPB3IndependentOracleCostVerdictBridge",
        "StackSafePDA.RPB3IndependentOracleGeneratedResolverVerdict",
        "StackSafePDA.RPB3IndependentOracleCostScalarTypeIdMutant",
        "StackSafePDA.RPB3IndependentOracleProcessChildProvenance",
        "StackSafePDA.RPB3IndependentOracleParScalarScoreBridge",
        "StackSafePDA.RPB3IndependentOracleParBytesChildBridge",
        "StackSafePDA.RPB3IndependentOracleParMessageChildBridge",
        "StackSafePDA.RPB3IndependentOracleRegionRebaseBridge",
        "StackSafePDA.RPB3IndependentOracleParMessageBodyDecodeBridge",
        "StackSafePDA.RPB3IndependentOracleParMessageRawRebaseBridge",
        "StackSafePDA.RPB3IndependentOracleStringMapRegionRebaseBridge",
        "StackSafePDA.RPB3IndependentOracleStringMapTypedRebaseBridge",
        "StackSafePDA.RPB3IndependentOracleParMessageEvaluationRebaseBridge",
        "StackSafePDA.RPB3IndependentOracleExprGroundScoreControl",
        "StackSafePDA.RPB3IndependentOracleExprFixed64BitwordBridge",
        "StackSafePDA.RPB3IndependentOracleExprNestedGroundControl",
        "StackSafePDA.RPB3IndependentOracleExprNestedGroundChildBridge",
        "StackSafePDA.RPB3IndependentOracleExprNestedGroundScalarBridge",
        "StackSafePDA.RPB3IndependentOracleExprNestedGroundScoreBridge",
        "StackSafePDA.RPB3SourceIndexedFrames",
        "StackSafePDA.RPB3SourceIndexedTermination",
        "StackSafePDA.RPB3ResidentParAdmissionBridge",
        "StackSafePDA.RPB3AdmissionMutants",
        "StackSafePDA.RPB3SourceIndexedTerminationWitnesses",
    ],));
}

#[test]
fn generated_rpb3_registry_is_contiguous_and_descriptor_aligned() {
    use models::rust::rholang::match_schema_tables::{
        PAR_PROTOBUF_REGION_PROGRAM, RHO_MATCH_REGISTRY,
    };

    assert!(
        !RHO_MATCH_REGISTRY.is_empty(),
        "generated RPB3 registry is empty"
    );
    for (index, (name, program)) in RHO_MATCH_REGISTRY.iter().enumerate() {
        let expected_type_id = u16::try_from(index + 1)
            .expect("generated RPB3 registry exceeds the u16 type-id domain");
        assert_eq!(
            program.type_id, expected_type_id,
            "{name}: registry type id drifted"
        );
        assert_eq!(program.type_name, *name, "{name}: registry name drifted");
        if *name == "Par" {
            assert_eq!(
                program.protobuf_fields.len(),
                0,
                "Par's resident row must not be used as its raw protobuf program"
            );
        } else {
            assert_eq!(
                program.fields.len(),
                program.protobuf_fields.len(),
                "{name}: generated semantic and protobuf descriptors disagree"
            );
        }
        assert!(
            program.type_id != 0,
            "{name}: zero is reserved for invalid type ids"
        );
    }
    assert_eq!(
        PAR_PROTOBUF_REGION_PROGRAM.type_name, "Par",
        "the separate raw protobuf Par program must retain the Par descriptor identity"
    );
}

#[test]
fn rpb3_admission_observable_spec_is_gated_and_defined_once() {
    let cursor_runner = read_repository_file("scripts/check-rpb3-admission-cursor-verus.sh");
    for marker in [
        "formal/verified_score_store/src/admission_observable_spec.rs",
        "verification results:: 139 verified, 0 errors",
    ] {
        assert!(
            cursor_runner.contains(marker),
            "RPB3 admission cursor gate is missing `{marker}`"
        );
    }
    let verified_store = read_repository_file("formal/verified_score_store/src/lib.rs");
    assert!(verified_store.contains("pub mod admission_observable_spec;"));
    let observable_spec =
        read_repository_file("formal/verified_score_store/src/admission_observable_spec.rs");
    for marker in [
        "pub open spec fn observable_next(",
        "pub open spec fn observable_step(",
        "pub open spec fn observable_trace(",
        "pub open spec fn observable_wf(",
        "pub proof fn lemma_observable_next_preserves_wf(",
        "pub proof fn lemma_observable_trace_preserves_wf(",
        "pub proof fn lemma_observable_event_determined(",
        "pub proof fn lemma_observable_next_repeated_irrelevant(",
        "pub proof fn lemma_observable_accept_appends_child_summary(",
        "pub proof fn lemma_observable_slot_record_counts_children(",
        "pub proof fn lemma_observable_children_never_dropped(",
        "pub proof fn lemma_observable_witness_message_accepts(",
        "pub proof fn lemma_observable_witness_map_accepts(",
        "pub proof fn lemma_observable_witness_canonical_path_accepts(",
        "pub proof fn lemma_observable_refusal_witnesses(",
    ] {
        assert!(
            observable_spec.contains(marker),
            "observable admission spec is missing `{marker}`"
        );
    }
    for forbidden in ["external_body", "assume(", "admit()", "#[cfg"] {
        assert!(
            !observable_spec.contains(forbidden),
            "observable admission spec contains `{forbidden}`"
        );
    }
    let mut pending = vec![repository_root().join("formal/verus")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("formal/verus must be listable") {
            let path = entry.expect("formal/verus entry must be readable").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = std::fs::read_to_string(&path)
                    .expect("formal/verus source must be readable as UTF-8");
                assert!(
                    !source.contains("fn observable_step("),
                    "{} defines observable_step outside the shared spec",
                    path.display()
                );
            }
        }
    }
}
