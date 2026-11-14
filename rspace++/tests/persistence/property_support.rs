use std::collections::BTreeMap;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use proptest::collection::btree_map;
use proptest::prelude::*;

use crate::rspace::persistence::checkpoint::OrderedObjectInventoryDigestV1;
use crate::rspace::persistence::{ObjectDigest, ObjectRefV1};

#[derive(Clone, Debug)]
pub(super) struct InventoryCase {
    pub(super) retained: Vec<ObjectRefV1>,
    pub(super) current: Vec<ObjectRefV1>,
    pub(super) manifest: ObjectRefV1,
    pub(super) current_objects: u64,
    pub(super) current_bytes: u64,
}

fn tagged_digest(tag: u8, suffix: [u8; 31]) -> ObjectDigest {
    let mut bytes = [0_u8; 32];
    bytes[0] = tag;
    bytes[1..].copy_from_slice(&suffix);
    ObjectDigest::from_array(bytes)
}

pub(super) fn inventory_case() -> impl Strategy<Value = InventoryCase> {
    (btree_map((any::<bool>(), any::<[u8; 31]>()), 32_u64..1_048_576, 1..16), 32_u64..1_048_576)
        .prop_flat_map(|(entries, manifest_len)| {
            let retained = entries
                .into_iter()
                .map(|((upper_half, suffix), encoded_len)| {
                    ObjectRefV1::new(
                        tagged_digest(if upper_half { 2 } else { 0 }, suffix),
                        encoded_len,
                    )
                    .expect("positive generated canonical length")
                })
                .collect::<Vec<_>>();
            let count = retained.len();
            (Just(retained), Just(manifest_len), proptest::collection::vec(any::<bool>(), count))
        })
        .prop_map(|(retained, manifest_len, mut current_membership)| {
            let manifest = ObjectRefV1::new(
                ObjectDigest::from_array([
                    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0,
                ]),
                manifest_len,
            )
            .expect("positive generated manifest length");
            // A semantic-state closure is nonempty and contains its manifest.
            // Pinning the first retained reference keeps the dependent subset
            // valid without filtering generated cases.
            current_membership[0] = true;
            let current = retained
                .iter()
                .zip(current_membership)
                .filter_map(|(reference, member)| member.then_some(*reference))
                .collect::<Vec<_>>();
            let current_bytes = current
                .iter()
                .map(|reference| reference.encoded_len())
                .sum();
            InventoryCase {
                retained,
                current_objects: current.len() as u64,
                current,
                manifest,
                current_bytes,
            }
        })
}

pub(super) fn mathematical_checkpoint_union(case: &InventoryCase) -> Vec<ObjectRefV1> {
    let mut expected = case.retained.clone();
    let insertion = expected
        .binary_search_by_key(&case.manifest.digest(), |reference| reference.digest())
        .expect_err("the generated manifest digest is disjoint");
    expected.insert(insertion, case.manifest);
    expected
}

pub(super) fn independent_inventory_digest(
    references: &[ObjectRefV1],
) -> OrderedObjectInventoryDigestV1 {
    let objects = references.len() as u64;
    let bytes = references
        .iter()
        .map(|reference| reference.encoded_len())
        .sum::<u64>();
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(b"RSpaceInv.RefV1\0");
    hasher.update(1_u64.to_le_bytes());
    hasher.update(objects.to_le_bytes());
    hasher.update(bytes.to_le_bytes());
    for reference in references {
        hasher.update(reference.digest().as_bytes());
        hasher.update(reference.encoded_len().to_le_bytes());
    }
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&hasher.finalize());
    OrderedObjectInventoryDigestV1::from_array(digest)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InventoryMutation {
    Omit,
    Insert,
    Duplicate,
    Reorder,
    SubstituteDigest,
    SubstituteDeclaredLength,
}

impl InventoryMutation {
    pub(super) const ALL: [Self; 6] = [
        Self::Omit,
        Self::Insert,
        Self::Duplicate,
        Self::Reorder,
        Self::SubstituteDigest,
        Self::SubstituteDeclaredLength,
    ];
}

pub(super) fn apply_inventory_mutation(
    baseline: &[ObjectRefV1],
    mutation: InventoryMutation,
) -> Vec<ObjectRefV1> {
    let mut mutated = baseline.to_vec();
    let middle = mutated.len() / 2;
    match mutation {
        InventoryMutation::Omit => {
            mutated.remove(middle);
        }
        InventoryMutation::Insert => {
            mutated.push(
                ObjectRefV1::new(ObjectDigest::from_array([3; 32]), 32)
                    .expect("fixed inserted reference has positive length"),
            );
        }
        InventoryMutation::Duplicate => {
            mutated.insert(middle, mutated[middle]);
        }
        InventoryMutation::Reorder => {
            mutated.swap(0, 1);
        }
        InventoryMutation::SubstituteDigest => {
            let encoded_len = mutated[middle].encoded_len();
            mutated[middle] = ObjectRefV1::new(ObjectDigest::from_array([4; 32]), encoded_len)
                .expect("fixed substituted reference has positive length");
        }
        InventoryMutation::SubstituteDeclaredLength => {
            let original = mutated[middle];
            let changed = if original.encoded_len() == u64::MAX {
                original.encoded_len() - 1
            } else {
                original.encoded_len() + 1
            };
            mutated[middle] = ObjectRefV1::new(original.digest(), changed)
                .expect("substituted reference remains nonempty");
        }
    }
    mutated
}

pub(super) const PROPERTY_BINDINGS: &[(&str, &str)] = &[
    ("IR-01", "cursor_edge_and_zero_crossing_mutants_violate_conservation"),
    ("IR-02", "cursor_execution_preserves_phase_invariants_and_matches_cold_dag"),
    ("IR-03", "phase_handoff_requires_both_events_and_cursors_drained"),
    ("IR-04", "cursor_execution_preserves_phase_invariants_and_matches_cold_dag"),
    ("IR-05", "zero_count_decrement_is_not_justified_by_coherence"),
    ("IR-06", "cursor_execution_preserves_phase_invariants_and_matches_cold_dag"),
    ("IR-07", "repeated_root_events_cannot_be_deduplicated"),
    ("IR-08", "root_removal_requires_root_authority_not_incoming_counts"),
    ("IR-09", "add_before_remove_preserves_shared_roots_and_releases_last_references"),
    ("IR-10", "authenticated_suffix_does_not_prove_processed_history"),
    ("IR-11", "cursor_execution_preserves_phase_invariants_and_matches_cold_dag"),
    ("IR-12", "cursor_execution_preserves_phase_invariants_and_matches_cold_dag"),
    ("IR-13", "cursor_execution_preserves_phase_invariants_and_matches_cold_dag"),
    ("IR-14", "rootless_cycle_witness_requires_acyclicity"),
    ("IR-15", "cursor_execution_preserves_phase_invariants_and_matches_cold_dag"),
    ("IR-16", "cursor_work_counts_edges_and_frame_disposal_exactly"),
    ("IR-17", "unrelated_graph_extension_does_not_change_cursor_work"),
    ("IR-18", "suspended_cursor_work_uses_remaining_suffix_not_root_count"),
    ("IR-19", "cyclic_cursor_execution_terminates_without_rooted_support"),
    ("IR-20", "zero_pending_work_does_not_require_zero_unspent_credit"),
    ("IR-21", "wrong_credit_and_cursor_costs_are_detected"),
    ("CR-01", "checked_reference_and_unique_inventory_refine_natural_arithmetic"),
    ("CR-02", "checked_inventory_boundary_controls_preserve_predecessor"),
    ("CR-03", "checked_reference_and_unique_inventory_refine_natural_arithmetic"),
    ("CR-04", "checked_inventory_boundary_controls_preserve_predecessor"),
    ("LT-01", "checked_trial_refines_cursor_steps_and_matches_cold_oracle"),
    ("LT-02", "checked_trial_budget_boundaries_are_exact_and_atomic"),
    ("LT-03", "refused_trial_retry_matches_unbounded_evidence"),
    ("LT-04", "checked_trial_refines_cursor_steps_and_matches_cold_oracle"),
    ("LT-05", "budget_domains_are_not_conflated"),
    ("LT-06", "budget_domains_are_not_conflated"),
    ("LT-07", "budget_domains_are_not_conflated"),
    ("LT-08", "external_quotes_bound_consumption_only_when_sound"),
    ("LT-09", "checked_trial_budget_boundaries_are_exact_and_atomic"),
    ("LT-10", "refused_trial_retry_matches_unbounded_evidence"),
    ("LT-11", "trial_mutants_violate_atomicity_or_cold_oracle"),
    ("DC-01", "composed_domain_budgets_are_separate_and_atomic"),
    ("DC-02", "composed_domain_histories_match_independent_cold_oracles"),
    ("DC-03", "composed_domain_histories_match_independent_cold_oracles"),
    ("DC-04", "composed_domain_histories_match_independent_cold_oracles"),
    ("DC-05", "composed_domain_histories_match_independent_cold_oracles"),
    ("DC-06", "composed_domain_histories_match_independent_cold_oracles"),
    ("DC-07", "composed_domain_histories_match_independent_cold_oracles"),
    ("DC-08", "domain_mutants_violate_the_cold_retained_union"),
    ("CL-01", "credited_cold_build_matches_independent_domain_oracles"),
    ("CL-02", "credited_cold_build_matches_independent_domain_oracles"),
    ("CL-03", "production_ledger_trial_refines_the_model_trial"),
    ("CL-04", "production_ledger_trial_refines_the_model_trial"),
    ("CL-05", "production_trial_limits_are_exact_per_axis_and_domain"),
    ("CL-06", "composed_production_trials_match_cold_domains_and_checkpoint_inventory"),
    ("CL-07", "production_trial_work_is_independent_of_unchanged_history"),
    ("CL-08", "twenty_thousand_level_chain_is_credited_and_traded_on_a_small_stack"),
    ("CL-09", "cold_oracle_differential_detects_credit_trial_and_domain_mutants"),
    ("RV-01", "retention_keys_are_exact_48_byte_bijections"),
    ("RV-02", "retention_key_bytes_sort_like_v1_identities_but_little_endian_keys_do_not"),
    ("RV-03", "generation_and_reference_aliases_keep_distinct_keys"),
    ("RV-04", "retention_v2_encodings_are_frozen"),
    ("RV-05", "radix_nodes_reuse_the_dag_body_under_the_metadata_domain"),
    ("RV-06", "radix_count_is_entry_occurrences_under_leaf_sharing"),
    ("RV-07", "metadata_closure_is_complete_closed_and_domain_separated"),
    ("RV-08", "v2_checkpoint_data_is_the_disjoint_union_of_retained_and_metadata"),
    ("RV-09", "v1_object_readers_refuse_every_frozen_v2_object"),
    ("RV-10", "v1_checkpoint_records_refuse_the_frozen_v2_descriptor"),
    ("RV-11", "catalog_selection_is_blind_to_the_appointed_retention_format"),
    ("RV-12", "every_catalog_envelope_change_makes_old_readers_select_the_stale_slot"),
    ("RV-13", "selected_successor_damage_never_falls_back_and_repairs_safe_index"),
    ("RV-14", "metadata_and_semantic_universes_are_disjoint_and_collisions_fail_closed"),
    ("RV-15", "v1_migration_preserves_entries_order_count_aliases_and_roots"),
    ("RV-16", "resume_binding_separates_v1_and_v2_representations_of_one_state"),
    ("RV-17", "v1_migration_preserves_entries_order_count_aliases_and_roots"),
    ("RV-18", "arbitrary_retention_v2_bytes_are_total"),
    ("RV-19", "every_observably_torn_inactive_slot_preserves_the_previous_authority"),
    ("RV-20", "dual_object_readers_extend_v1_readers_only_on_metadata_domains"),
    ("RV-21", "versioned_checkpoint_codec_accepts_v2_inventory_only_under_the_dual_reader"),
    ("RV-22", "production_radix_admission_refines_the_reference_decoder"),
    ("RV-23", "radix_memo_is_keyed_by_depth_and_shared_graphs_cannot_force_enumeration"),
    ("RV-24", "v2_admission_authenticates_metadata_separately_from_the_retained_union"),
    ("RV-25", "v2_cold_admission_matches_v1_admission_and_charges_metadata_reads_exactly"),
    ("RV-26", "every_v2_cold_admission_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("RV-27", "v2_restart_fixtures_are_refused_by_old_readers_and_adopted_by_dual_readers"),
    ("RV-28", "v2_admitted_sources_refuse_every_successor_before_durable_mutation"),
    ("RV-29", "dual_retention_readers_are_total_on_arbitrary_bytes_and_graphs"),
    ("RV-30", "canonical_radix_images_are_determined_by_their_entries"),
    ("RV-31", "radix_edits_equal_the_independent_rebuild"),
    ("RV-32", "radix_edit_work_is_the_changed_path_under_geometric_growth"),
    ("RV-33", "anchor_steps_realize_retention_entry_steps"),
    ("RV-34", "editor_images_are_admitted_by_the_cold_reader"),
    ("RV-35", "metadata_trial_work_is_the_changed_objects_under_geometric_growth"),
    ("RV-36", "seeded_metadata_ledger_equals_the_cold_ledger"),
    ("RV-37", "v1_migration_is_the_canonical_rebuild_and_refused_by_old_readers"),
    ("TS-01", "trusted_successor_matches_cold_admission_at_every_history_step"),
    ("TS-02", "trusted_successor_index_frontier_equals_cold_recovery"),
    ("TS-03", "trusted_bind_runs_domains_in_order_and_keeps_the_predecessor_on_first_refusal"),
    ("TS-04", "trusted_policy_accepts_exact_and_refuses_one_below_on_every_axis"),
    ("TS-05", "trusted_policy_permits_budgeted_union_scratch_above_final_state_size"),
    ("TS-06", "trusted_policy_and_mode_cannot_change_after_preparation"),
    ("TS-07", "trusted_binding_refuses_foreign_stale_and_resequenced_authority_before_mutation"),
    ("TS-08", "fresh_checkpoint_from_a_trusted_core_measures_new_cold_demand"),
    ("TS-09", "trusted_publication_performs_no_physical_recovery_or_cold_admission"),
    ("TS-10", "trusted_and_cold_checked_publication_write_identical_bytes"),
    ("TS-11", "trusted_incremental_publication_recovers_and_retries_every_phase"),
    ("TS-12", "owner_empty_checkpoint_under_tightened_cold_limits_performs_no_cold_check"),
    (
        "TS-13",
        "owner_awaiting_ack_retry_under_tightened_cold_limits_acknowledges_once_without_cold_check",
    ),
    ("TS-14", "trusted_success_does_not_imply_cold_admission_under_incoming_limits"),
    ("TS-15", "owner_trusted_policy_refusal_leaves_files_journals_and_authority_unchanged"),
    ("TS-16", "trusted_retained_state_maxima_equal_cold_requirements"),
    ("TS-17", "trusted_shape_summary_matches_cold_semantic_walk"),
    ("TS-18", "trusted_validity_is_bound_to_the_exact_predecessor_and_receipt"),
    ("TS-19", "trusted_successor_migrates_a_v1_source_once_and_matches_the_cold_seed"),
    ("TH-01", "faulted_trusted_histories_match_independent_cold_admission_at_every_step"),
    ("TH-02", "faulted_trusted_histories_match_independent_cold_admission_at_every_step"),
    ("TH-03", "every_failpoint_recovers_to_the_appointed_prefix_at_later_history_steps"),
    ("TH-04", "every_failpoint_recovers_to_the_appointed_prefix_at_later_history_steps"),
    ("TH-05", "faulted_trusted_histories_match_independent_cold_admission_at_every_step"),
    ("TH-06", "every_failpoint_recovers_to_the_appointed_prefix_at_later_history_steps"),
    ("TH-07", "faulted_trusted_histories_match_independent_cold_admission_at_every_step"),
    ("TH-08", "owner_histories_resolve_ambiguity_acknowledge_once_and_never_replay_the_journal"),
    ("PX-01", "a_staged_node_equal_to_an_inherited_subtree_stops_at_its_exact_certificate"),
    ("PX-02", "an_inexact_boundary_does_not_stop_a_coincident_staged_node"),
    ("PX-03", "an_object_staged_only_where_it_is_inherited_is_outside_the_delta"),
    ("PX-04", "shrunk_coincident_leaf_transition_seals_and_the_staged_first_control_refuses"),
    ("PX-05", "generated_transitions_with_shared_subtrees_seal_to_the_independent_rebuild"),
    ("RI-01", "every_reclamation_intent_tear_recovers_pre_cas_authority_and_retries"),
    ("RI-02", "generated_reclamation_intent_tears_never_publish_a_torn_record"),
    ("RI-03", "interrupted_reclamation_intent_write_recovers_pre_cas_authority"),
    ("RI-04", "reclamation_intent_publication_never_replaces_an_existing_record"),
    ("SW-01", "trusted_commit_work_is_the_changed_frontier_under_geometric_current_growth"),
    ("SW-02", "trusted_commit_work_is_unchanged_by_geometric_history_growth"),
    ("SW-03", "last_reference_deletion_expands_exactly_the_deleted_closure"),
    ("SW-04", "scaling_harness_is_deterministic_and_fails_closed"),
    ("OG-06", "close_revokes_old_attachments_without_mutation"),
    ("OG-07", "reopened_owners_refuse_every_earlier_incarnation"),
    ("OG-08", "pinned_reads_survive_owner_close_until_the_last_reader_releases"),
    ("OG-09", "a_foreign_gate_permit_is_refused_before_capture_without_mutation"),
    ("OG-10", "a_stale_base_is_refused_like_a_legacy_root_conflict_without_mutation"),
    ("OG-11", "checkpointing_under_the_held_view_gate_and_owner_mutex_completes"),
    ("OG-12", "legacy_facet_mismatch_conversions_keep_their_typed_source"),
    ("OG-39", "rspace_constructors_propagate_a_failing_store_as_an_error"),
    ("VR-01", "read_handle_pins_its_generation_across_a_successor_and_owner_teardown"),
    ("VR-02", "lazy_reads_equal_the_eager_enumerations_of_every_key"),
    ("VR-03", "lazy_reads_return_the_pinned_state_after_a_successor_is_published"),
    ("VR-04", "lazy_and_eager_stores_agree_on_contents_reads_and_journals"),
    ("VR-05", "lazy_bootstrap_makes_no_reader_call_during_construction"),
    ("VR-06", "a_bound_lazy_store_checkpoints_to_the_eager_successor_slot"),
    ("VR-07", "a_receive_group_is_read_only_at_its_literal_ordered_prefix"),
    ("VR-08", "detached_journals_cannot_be_published_and_mismatched_binds_change_nothing"),
    ("DI-01", "durable_state_root_is_a_function_of_contents_across_insertion_orders_and_reopen"),
    ("DI-02", "every_retained_state_resolves_to_its_own_roots_and_nothing_else_resolves"),
    ("DI-03", "states_that_differ_in_one_endpoint_have_different_roots"),
    ("DI-04", "a_manifest_with_a_legacy_history_root_does_not_resolve"),
    ("HR-01", "every_view_resets_to_every_retained_root_and_reads_that_states_contents"),
    ("HR-02", "an_unknown_root_is_refused_and_changes_no_file"),
    ("HR-03", "reset_performs_no_write_and_no_sync_under_every_directory_failpoint"),
    ("HR-04", "a_reset_completes_while_a_publication_holds_the_owner_mutex"),
    (
        "HR-05",
        "a_read_retries_on_the_published_successor_when_its_core_is_reclaimed_before_its_lease",
    ),
    ("HR-06", "a_read_refused_on_a_reclaimed_core_without_a_successor_reloads_once"),
    ("JR-01", "constructor_refuses_foreign_or_invalidated_hot_authority_without_losing_its_view"),
    ("JR-02", "invalidated_base_cannot_release_old_recovery_or_report_ready"),
    ("JR-03", "a_stale_capture_is_rejected_before_publication_even_when_its_journal_bytes_match"),
    ("HJ-01", "acknowledgement_clears_five_populated_journals_and_preserves_live_records"),
    ("HJ-02", "acknowledgement_clears_five_populated_journals_and_preserves_live_records"),
    ("HJ-03", "mismatch_or_missing_capability_in_any_journal_rejects_without_partial_clear"),
    ("HJ-04", "stale_journal_cannot_clear_a_newer_revision"),
    ("HJ-05", "journal_count_inversion_matches_live_multiplicities"),
    ("HJ-06", "journal_count_inversion_matches_live_multiplicities"),
    ("HJ-07", "journal_count_inversion_matches_live_multiplicities"),
    ("HJ-08", "absent_count_evidence_is_not_a_captured_zero"),
    ("HJ-09", "join_journal_uses_native_key_and_repeated_channel_positions"),
    ("HJ-10", "join_journal_uses_native_key_and_repeated_channel_positions"),
    ("JV-01", "journal_version_histories_never_reuse_a_retained_version"),
    ("JV-02", "journal_version_histories_never_reuse_a_retained_version"),
    ("JV-03", "journal_version_rollover_retains_fresh_identity_while_old_receipts_live"),
    ("SI-01", "incremental_inventory_tracks_replacement_removal_sharing_and_reuse"),
    ("SI-02", "incremental_inventory_matches_cold_admission_at_every_history_step"),
    ("SI-03", "incremental_inventory_tracks_replacement_removal_sharing_and_reuse"),
    ("SI-04", "incremental_inventory_is_bound_to_its_opened_authority_and_exact_sequence"),
    ("SI-05", "incremental_inventory_limits_refuse_before_any_durable_mutation"),
    ("IP-01", "borrowed_incremental_publication_recovers_and_retries_every_phase"),
    ("IP-02", "borrowed_incremental_publication_recovers_and_retries_every_phase"),
    ("INV-01", "checkpoint_union_is_exact_disjoint_ordered_merge"),
    ("INV-02", "checkpoint_union_is_exact_disjoint_ordered_merge"),
    ("INV-03", "checkpoint_union_is_exact_disjoint_ordered_merge"),
    ("INV-04", "checkpoint_union_is_exact_disjoint_ordered_merge"),
    ("INV-05", "checkpoint_union_is_exact_disjoint_ordered_merge"),
    ("INV-06", "checkpoint_union_is_exact_disjoint_ordered_merge"),
    ("INV-07", "manifest_digest_alias_reports_both_declared_lengths"),
    ("INV-08", "current_closure_is_a_native_subset_of_retained_union"),
    ("DES-01", "descriptor_roundtrip_preserves_all_six_inventory_scalars"),
    ("DES-02", "descriptor_roundtrip_preserves_all_six_inventory_scalars"),
    ("DES-03", "checkpoint_commit_rejects_truncation_corruption_and_rebinding"),
    ("DES-04", "descriptor_roundtrip_preserves_all_six_inventory_scalars"),
    ("DES-05", "parser_rejects_current_inventory_outside_the_retained_union"),
    ("DES-06", "ordered_inventory_digest_matches_independent_fold"),
    ("DES-07", "every_inventory_mutation_changes_or_rejects_authority"),
    ("DES-08", "descriptor_arithmetic_overflow_fails_closed"),
    ("WIR-01", "descriptor_roundtrip_preserves_all_six_inventory_scalars"),
    ("WIR-02", "generated_checkpoint_wire_extents_follow_the_formal_length_equations"),
    ("WIR-03", "generated_checkpoint_wire_extents_follow_the_formal_length_equations"),
    ("WIR-04", "generated_checkpoint_wire_extents_follow_the_formal_length_equations"),
    ("WIR-05", "generated_checkpoint_wire_extents_follow_the_formal_length_equations"),
    ("WIR-06", "canonical_v1_prepare_and_commit_images_are_frozen"),
    ("WAL-01", "checkpoint_commit_and_fresh_index_share_one_preflighted_layout"),
    ("WAL-02", "checkpoint_commit_and_fresh_index_share_one_preflighted_layout"),
    ("WAL-03", "checkpoint_commit_rejects_truncation_corruption_and_rebinding"),
    ("WAL-04", "generation_checkpoint_reuse_and_short_physical_pack_fail_closed"),
    ("WAL-05", "arbitrary_checkpoint_wire_bytes_are_total"),
    ("CAT-01", "every_observably_torn_inactive_slot_preserves_the_previous_authority"),
    ("CAT-02", "catalog_uncertainty_classifies_both_old_and_new_observable_outcomes"),
    ("CAT-03", "selected_successor_damage_never_falls_back_and_repairs_safe_index"),
    ("CAT-04", "authority_token_rejects_every_catalog_image_byte_mutation"),
    ("CAT-05", "every_injected_torn_catalog_prefix_recovers_only_old_or_exact_new_authority"),
    ("CAT-06", "successful_publication_executes_the_exact_formal_phase_order"),
    ("PUB-00", "checkpoint_publication_is_byte_exact_restart_equivalent_and_keeps_one_lease"),
    ("PUB-01", "catalog_sync_to_recovery_has_no_writer_lock_gap"),
    ("PUB-02", "successful_publication_executes_the_exact_formal_phase_order"),
    ("PUB-03", "successful_publication_executes_the_exact_formal_phase_order"),
    ("PUB-04", "every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("PUB-05", "every_pre_catalog_crash_shape_recovers_old_authority_and_retries_exactly"),
    ("PUB-06", "exact_catalog_change_before_cas_causes_no_publisher_slot_write"),
    ("PUB-07", "every_pre_catalog_crash_shape_recovers_old_authority_and_retries_exactly"),
    ("PUB-08", "every_pre_catalog_crash_shape_recovers_old_authority_and_retries_exactly"),
    ("PUB-09", "every_injected_torn_catalog_prefix_recovers_only_old_or_exact_new_authority"),
    ("PUB-10", "catalog_uncertainty_classifies_both_old_and_new_observable_outcomes"),
    ("PUB-11", "successful_publication_executes_the_exact_formal_phase_order"),
    ("PUB-12", "post_catalog_directory_failure_is_published_and_restart_selects_new"),
    ("PUB-13", "checkpoint_publication_is_byte_exact_restart_equivalent_and_keeps_one_lease"),
    ("PUB-14", "published_but_recovery_failure_is_typed_and_restartable"),
    ("PUB-15", "published_but_semantic_admission_failure_is_typed_and_restartable"),
    ("PUB-16", "publication_outcomes_are_total_and_mutually_exclusive"),
    ("PUB-17", "exact_catalog_change_before_cas_causes_no_publisher_slot_write"),
    ("PUB-18", "checkpoint_publication_is_byte_exact_restart_equivalent_and_keeps_one_lease"),
    ("PUB-19", "catalog_uncertainty_classifies_both_old_and_new_observable_outcomes"),
    ("PUB-20", "catalog_uncertainty_classifies_both_old_and_new_observable_outcomes"),
    ("SEM-01", "dormant_checkpoint_publisher_has_no_runtime_accounting_capability"),
    ("SEM-02", "physical_layout_variation_preserves_semantics_and_exact_successor"),
    ("SEM-03", "checkpoint_publication_is_byte_exact_restart_equivalent_and_keeps_one_lease"),
    ("RES-01", "arbitrary_checkpoint_wire_bytes_are_total"),
    ("RES-02", "every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("RES-03", "checkpoint_cardinality_stack_rss_gate"),
    ("NV-01", "checkpoint_publication_is_byte_exact_restart_equivalent_and_keeps_one_lease"),
    ("NV-02", "every_injected_torn_catalog_prefix_recovers_only_old_or_exact_new_authority"),
    ("NV-03", "every_injected_torn_catalog_prefix_recovers_only_old_or_exact_new_authority"),
    ("NV-04", "published_but_recovery_failure_is_typed_and_restartable"),
    ("NV-05", "catalog_uncertainty_classifies_both_old_and_new_observable_outcomes"),
    ("NV-06", "every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("NV-07", "every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("NV-08", "every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("NV-09", "every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("NV-10", "every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("NV-11", "every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("NV-12", "every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly"),
    ("NV-13", "selected_successor_damage_never_falls_back_and_repairs_safe_index"),
    ("NV-14", "selected_successor_damage_never_falls_back_and_repairs_safe_index"),
    ("SG-01", "an_empty_store_reopens_at_generation_one_with_empty_namespaces"),
    ("SG-02", "independently_created_empty_stores_share_the_empty_state_manifest_digest"),
    ("SG-03", "a_crash_at_any_genesis_write_or_sync_point_never_opens_a_different_state"),
    ("GC-01", "catalog_selection_and_generation_pins_stay_typed_through_publication_and_crash"),
    ("GC-02", "old_generation_is_reclaimed_only_after_the_successor_is_published"),
    ("GC-03", "exact_old_generation_pins_defer_reclamation_until_released"),
    ("GC-04", "crash_before_publication_keeps_the_old_generation_for_retry"),
    ("GC-05", "reader_export_and_import_roots_share_one_protection_quotient"),
    (
        "GC-06",
        "a_known_lease_active_at_publication_is_carried_by_the_successor_or_refuses_the_compaction",
    ),
    ("GC-07", "a_known_lease_keeps_its_exact_generation_and_is_refused_for_roots_outside_it"),
    ("GC-08", "the_overlap_witness_history_of_the_runtime_is_a_history_of_the_model"),
    ("GC-09", "only_a_checkpoint_writes_and_no_replay_or_rollback_obligation_survives_a_crash"),
    ("GC-10", "crash_before_publication_keeps_the_old_generation_for_retry"),
    ("GC-11", "only_a_checkpoint_writes_and_no_replay_or_rollback_obligation_survives_a_crash"),
    ("TX-01", "pages_carry_exactly_the_closure_once_parent_first_for_deep_wide_and_shared_states"),
    ("DF-01", "visited_differences_turn_the_from_state_into_the_to_state"),
    ("DF-02", "a_difference_reads_only_the_nodes_the_two_states_do_not_share"),
    ("DF-03", "both_orders_of_a_receive_group_are_visited_as_distinct_endpoints"),
    (
        "TX-02",
        "duplicate_reordered_truncated_and_corrupted_pages_are_refused_and_leave_the_frontier_unchanged",
    ),
    ("TX-03", "a_complete_closure_is_admitted_and_one_with_a_dangling_join_is_refused"),
    ("TX-07", "pages_served_through_the_trait_equal_the_pages_of_one_session"),
    ("TX-08", "a_position_no_session_has_served_is_reached_and_sessions_stay_bounded"),
    ("TX-09", "a_cursor_of_another_shape_is_refused"),
    ("CB-01", "a_cross_base_capture_is_acknowledged_only_at_the_successor_of_its_publication_base"),
    ("CB-02", "a_wrong_origin_or_registry_is_refused_and_a_changed_journal_fails_validation"),
    ("CB-03", "a_cross_base_capture_observes_what_a_same_base_capture_at_the_origin_observes"),
    ("OG-13", "create_history_repo_calls_share_one_durable_owner_slot"),
    ("OG-14", "a_second_semantic_profile_is_refused_by_the_owner_slot"),
    ("OG-15", "legacy_only_authority_methods_refuse_the_durable_backend"),
    ("OG-41", "only_an_empty_directory_becomes_a_store_at_the_store_genesis_root"),
    ("OG-46", "a_task_waiting_for_the_owner_mutex_yields_its_thread"),
    ("OG-16", "a_durable_prefix_has_one_authority_and_one_owner_slot_per_manager"),
    ("OG-17", "the_backend_of_a_prefix_is_fixed_at_its_first_use"),
    (
        "OG-18",
        "a_durable_selection_is_refused_for_eval_and_for_a_prefix_that_is_not_one_path_component",
    ),
    ("OG-01", "rebased_journals_applied_to_the_receiving_base_give_the_overlay"),
    ("OG-02", "a_same_base_rebase_reproduces_the_journals_of_the_soft_checkpoint"),
    ("OG-03", "rebased_count_sides_are_disjoint_and_stay_within_u64"),
    ("OG-04", "a_rebase_reads_the_receiving_base_once_per_domain_member"),
    ("OG-05", "an_ffi_shaped_payload_rebases_to_the_overlay_difference"),
    ("XB-01", "a_view_checkpoint_from_any_retained_base_returns_the_twin_stores_root"),
    ("XB-02", "a_cross_base_checkpoint_appends_one_record_pair_and_swaps_the_catalog_once"),
    ("XB-03", "an_empty_journal_and_an_unchanged_current_state_write_and_sync_nothing"),
    ("XB-04", "every_refusal_precedes_the_first_write_and_changes_nothing"),
    ("XB-05", "a_crash_at_every_owner_hook_point_recovers_the_old_state_or_the_successor"),
    ("KG-01", "v1_builders_equal_the_rhopathv1_functions_byte_for_byte"),
    ("TM-01", "a_memoised_transition_is_returned_without_a_write_a_sync_or_a_walk"),
    ("TM-02", "every_checkpoint_result_equals_that_of_an_owner_without_a_memo"),
    (
        "TM-03",
        "the_memo_is_bounded_evicts_its_oldest_transition_and_is_empty_after_close_and_a_reopen",
    ),
];

pub(super) fn binding_map() -> BTreeMap<&'static str, &'static str> {
    PROPERTY_BINDINGS.iter().copied().collect()
}
