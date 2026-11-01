"""Check the receive occurrence index proof, model, mutant and executable witness inventory."""

import argparse
import ast
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/receive_accelerator_obligations.generated.json"
CHECKER = "scripts/check-receive-accelerator-obligations.py"
ROCQ = "formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceReceiveAccelerator.v"
MUTANTS = "formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceReceiveAcceleratorMutants.v"
CHANNEL_SCOPE = "formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceReceiveAcceleratorChannelScope.v"
CHANNEL_SCOPE_MUTANTS = "formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceReceiveAcceleratorChannelScopeMutants.v"
TLA_ROOT = "formal/tlaplus/stack_safe_pda"
STEM = "RSpaceReceiveAcceleratorCoherence"
MODEL = f"{TLA_ROOT}/{STEM}.tla"
CONFIG = f"{TLA_ROOT}/{STEM}.cfg"
MUTANT_MODEL = f"{TLA_ROOT}/{STEM}Mutants.tla"
REFERENCE = "rspace++/tests/support/receive_occurrence_reference.rs"
ADOPTION = "rspace++/tests/support/receive_accelerator_adoption.rs"
SPEC = "rspace++/tests/receive_accelerator_spec.rs"
RHOLANG = "rholang/tests/receive_accelerator_rholang.rs"
LIB = "rspace++/tests/persistence/receive_accelerator_manifest_binding.rs"
PRODUCTION_SPEC = "rspace++/tests/receive_accelerator_production_spec.rs"
PRODUCTION_RHOLANG = "rholang/tests/receive_accelerator_production_rholang.rs"
PRODUCTION_SUPPORT = "rspace++/tests/support/receive_accelerator_production.rs"
OCCURRENCE_TESTS = "rspace++/tests/receive_accelerator/occurrence_index.rs"
BINDING_TESTS = "rspace++/tests/hot_store/receive_accelerator_bindings.rs"
TOKEN_TESTS = "rspace++/tests/hot_store/receive_revision_tokens.rs"
GENERATOR = "scripts/generate-receive-accelerator-mutant.py"
HOT_STORE = "rspace++/src/rspace/hot_store.rs"
RSPACE = "rspace++/src/rspace/rspace.rs"
RECEIVE_ACCELERATOR = "rspace++/src/rspace/receive_accelerator.rs"
LIB_MODULES = "rspace++/tests/persistence/mod.rs"
LIB_ROOT = "rspace++/src/rspace/persistence/mod.rs"
RSPACE_CARGO = "rspace++/Cargo.toml"
RHOLANG_CARGO = "rholang/Cargo.toml"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
UNHASHED = (HOT_STORE, RSPACE, RECEIVE_ACCELERATOR, LIB_MODULES, LIB_ROOT, RSPACE_CARGO, RHOLANG_CARGO, RUNNER)
HARNESS_SOURCES = (REFERENCE, ADOPTION)
PRODUCTION_SOURCES = (HOT_STORE, RSPACE)
PATHMAP_CODEC = "rspace++/src/rspace/pathmap_codec.rs"
ISOLATED_ROOTS = ("rspace++/src/rspace/persistence", "rspace++/src/rspace/history")
ISOLATED_FILES = (
    PATHMAP_CODEC,
    "rspace++/src/rspace/data_index.rs",
    "rspace++/src/rspace/hot_store_action.rs",
    "rspace++/src/rspace/checkpoint.rs",
)
ISOLATION_PATTERN = re.compile(
    r"receive_accelerator|ReceiveAccelerator|ReceiveOccurrence|receive_occurrence|receive_index|ReceiveRevision"
    r"|ReceiveTarget|ReceiveId|hydrate_receive_scope|occurrence_index"
)
FLAG = "--receive-accelerator"
ROCQ_MODULE = "TwoLevelRSpaceReceiveAccelerator"
MUTANT_MODULE = "TwoLevelRSpaceReceiveAcceleratorMutants"
BRIDGE = "implementation_with_correct_switches_is_the_model"
CHANNEL_SCOPE_MODULE = "TwoLevelRSpaceReceiveAcceleratorChannelScope"
CHANNEL_SCOPE_MUTANT_MODULE = "TwoLevelRSpaceReceiveAcceleratorChannelScopeMutants"
CHANNEL_SCOPE_BRIDGE = "channel_scope_implementation_with_correct_switches_is_the_model"
DECLARATIONS = "Theorem|Lemma|Corollary|Proposition|Fact|Remark|Example|Definition|Fixpoint"
CLOSED_REPORTS = {ROCQ: 26, MUTANTS: 16, CHANNEL_SCOPE: 8, CHANNEL_SCOPE_MUTANTS: 5}
LIB_FILTER = "receive_accelerator_manifest_binding"
LIBRARY_FILTERS = {
    LIB: LIB_FILTER,
    OCCURRENCE_TESTS: "rspace::receive_accelerator::",
    BINDING_TESTS: "rspace::hot_store::receive_accelerator_bindings_tests::",
    TOKEN_TESTS: "rspace::hot_store::receive_revision_tokens_tests::",
}
SUITE_COUNTS = {
    SPEC: 31,
    RHOLANG: 5,
    LIB: 2,
    PRODUCTION_SPEC: 31,
    PRODUCTION_RHOLANG: 5,
    OCCURRENCE_TESTS: 14,
    BINDING_TESTS: 18,
    TOKEN_TESTS: 5,
}
CATALOG_ROW = "TLRS-RECEIVE-28"
PERSISTED_WITNESS = (LIB, "persisted_accelerator_adoption_requires_the_bound_semantic_manifest")
PERSISTED_REFINES = ("RAC-RC-20", "RAC-TLA-08")

THEOREMS = re.findall(
    r"\S+",
    """
derived_occurrence_is_sound derived_occurrence_is_complete derived_positions_factor_through_group
repeated_channel_is_one_occurrence_with_all_positions occurrence_capacity_is_position_multiplicity
prefix_aliased_channels_have_disjoint_occurrences derivation_ignores_data_and_runtime_order
join_root_projection_is_exact derivation_is_order_neutral receive_id_resolution_is_exact
distinct_records_have_distinct_receive_ids hash_bucket_resolution_is_collision_checked
stale_revision_never_resolves first_identity_representative_is_preserved
insertion_update_equals_rebuild nonfinal_removal_update_equals_rebuild
final_removal_update_equals_rebuild install_and_uninstall_update_equals_rebuild
history_load_and_byte_equal_replacement_preserve_derivation
reset_issues_fresh_generation_and_rebuilds same_revision_same_derivation
read_never_hides_an_enabled_receive failed_or_stale_update_publishes_nothing
published_artifacts_share_one_revision accelerator_candidate_commit_refines_accepted_match
semantic_successor_is_accelerator_free
""",
)
MUTANT_EXAMPLES = re.findall(
    r"\S+",
    """
hash_only_receive_id_is_refuted ordinal_receive_id_is_refuted absent_as_empty_is_refuted
stale_snapshot_answer_is_refuted foreign_accelerator_adoption_is_refuted
incomplete_scope_answer_is_refuted collapsed_repeated_positions_are_refuted
installed_resting_collapse_is_refuted order_sensitive_derivation_is_refuted
split_artifact_publication_is_refuted failed_update_leak_is_refuted
accelerator_in_semantic_identity_is_refuted last_source_wins_is_refuted
unchecked_candidate_commit_is_refuted metadata_erasing_target_is_refuted
""",
)
CHANNEL_SCOPE_CAPSTONES = {
    "RAC-CS-01": ("join_coverage_completes_the_channel_scope",),
    "RAC-CS-02": ("complete_channel_scope_lookup_is_derivation",),
    "RAC-CS-03": (
        "exact_channel_read_is_current_sound_and_group_complete",
        "covered_exact_channel_read_is_the_derivation",
    ),
    "RAC-CS-04": ("global_coherence_implies_channel_coherence",),
    "RAC-CS-05": ("exact_channel_read_contains_the_joins_derivation",),
    "RAC-CS-06": ("framed_occurrence_keys_enumerate_exactly_the_channel_groups",),
    "RAC-CS-07": ("equal_store_tokens_imply_equal_revisions",),
}
CHANNEL_SCOPE_REFUTATIONS = {
    "RAC-CSMUT-01": ("skipped_join_scope_check_is_refuted",),
    "RAC-CSMUT-02": ("skipped_lookup_scope_check_is_refuted",),
    "RAC-CSMUT-03": ("unframed_occurrence_keys_are_refuted",),
    "RAC-CSMUT-04": ("live_scope_join_check_is_refuted",),
}
INVARIANTS = re.findall(
    r"\S+",
    """
TypeInvariant PublishedArtifactsShareOneRevision CoherentAcceleratorEqualsDerivation
AnswerIsSound NoEnabledReceiveHidden StaleAcceleratorNeverAnswers
IncompleteAcceleratorNeverAnswers ForeignAcceleratorNeverAdopted StaleCandidateNeverCommits
FailedUpdateIsNonMutating SemanticIdentityIsAcceleratorFree
""",
)
TLA_MUTANTS = {
    "SplitPublication": ("split-publication", "PublishedArtifactsShareOneRevision"),
    "PartialRebuild": ("partial-rebuild", "CoherentAcceleratorEqualsDerivation"),
    "UnsoundAnswer": ("unsound-answer", "AnswerIsSound"),
    "AbsentAsEmpty": ("absent-as-empty", "NoEnabledReceiveHidden"),
    "StaleAnswer": ("stale-answer", "StaleAcceleratorNeverAnswers"),
    "IncompleteAnswer": ("incomplete-answer", "IncompleteAcceleratorNeverAnswers"),
    "ForeignAdoption": ("foreign-adoption", "ForeignAcceleratorNeverAdopted"),
    "StaleCandidateCommit": ("stale-candidate-commit", "StaleCandidateNeverCommits"),
    "FailedUpdateLeak": ("failed-update-leak", "FailedUpdateIsNonMutating"),
    "SemanticPollution": ("semantic-pollution", "SemanticIdentityIsAcceleratorFree"),
}
REFINEMENT = (
    *(f"RAC-RC-{index:02d}" for index in range(1, len(THEOREMS) + 1)),
    *(f"RAC-TLA-{index:02d}" for index in range(2, len(INVARIANTS) + 1)),
    *(f"RAC-RS-{index:02d}" for index in range(17, 25)),
)
PROOF_ONLY = (
    "RAC-TLA-01",
    *(f"RAC-TMUT-{index:02d}" for index in range(1, len(TLA_MUTANTS) + 1)),
    *CHANNEL_SCOPE_CAPSTONES,
    *CHANNEL_SCOPE_REFUTATIONS,
)

OWNERS = {
    "reference_occurrences_are_sound_over_generated_groups": (SPEC, ("RAC-RC-01",)),
    "reference_occurrences_are_complete_over_generated_groups": (SPEC, ("RAC-RC-02",)),
    "occurrence_positions_factor_through_the_group": (SPEC, ("RAC-RC-03",)),
    "repeated_channels_yield_one_occurrence_with_every_position": (SPEC, ("RAC-RC-04",)),
    "prefix_aliased_channel_images_never_share_occurrences": (SPEC, ("RAC-RC-06",)),
    "derivation_ignores_data_and_authority_orders": (SPEC, ("RAC-RC-07",)),
    "join_bag_projection_matches_derived_groups": (SPEC, ("RAC-RC-08",)),
    "construction_order_does_not_change_the_relation_or_ids": (SPEC, ("RAC-RC-09",)),
    "receive_ids_resolve_only_to_byte_identical_records": (SPEC, ("RAC-RC-10",)),
    "metadata_distinct_records_keep_distinct_ids": (SPEC, ("RAC-RC-11",)),
    "colliding_buckets_resolve_by_full_bytes": (SPEC, ("RAC-RC-12",)),
    "ids_from_a_prior_revision_or_generation_never_resolve": (SPEC, ("RAC-RC-13",)),
    "first_source_representative_survives_alias_insertion": (SPEC, ("RAC-RC-14",)),
    "incremental_insertion_equals_rebuild": (SPEC, ("RAC-RC-15",)),
    "incremental_nonfinal_removal_equals_rebuild": (SPEC, ("RAC-RC-16",)),
    "incremental_final_removal_equals_rebuild_and_clears_the_group": (SPEC, ("RAC-RC-17",)),
    "install_and_generation_uninstall_equal_rebuild": (SPEC, ("RAC-RC-18",)),
    "history_load_and_duplicate_put_are_relation_and_version_neutral": (SPEC, ("RAC-RC-19",)),
    "clear_and_set_state_invalidate_every_held_version_and_rebuild_equal": (
        SPEC,
        ("RAC-RC-20",),
    ),
    "equal_versions_imply_equal_group_relation": (SPEC, ("RAC-RC-21",)),
    "reference_reads_equal_derivation_in_every_accelerator_state": (
        SPEC,
        ("RAC-RC-22", "RAC-TLA-05"),
    ),
    "failed_update_leaves_store_and_accelerator_unchanged": (SPEC, ("RAC-RC-23", "RAC-TLA-10")),
    "catalog_occurrence_and_automaton_publish_at_one_revision": (SPEC, ("RAC-TLA-02",)),
    "same_count_group_replacement_never_reuses_a_stale_automaton": (SPEC, ("RAC-RC-24",)),
    "stale_accelerator_candidates_never_commit": (SPEC, ("RAC-RC-25", "RAC-TLA-09")),
    "enabled_and_disabled_acceleration_select_identically": (SPEC, ("RAC-RC-26", "RAC-TLA-11")),
    "incremental_maintenance_equals_rebuild_over_generated_traces": (SPEC, ("RAC-TLA-03",)),
    "reference_answers_never_include_absent_receives": (SPEC, ("RAC-TLA-04",)),
    "stale_accelerators_fall_back_after_every_mutation": (SPEC, ("RAC-TLA-06",)),
    "unloaded_history_groups_make_the_accelerator_incomplete": (SPEC, ("RAC-TLA-07",)),
    "foreign_generation_or_manifest_accelerators_are_never_adopted": (SPEC, ("RAC-TLA-08",)),
    "compiled_join_capacity_agrees_with_occurrence_multiplicity": (RHOLANG, ("RAC-RC-05",)),
}
EVIDENCE = {
    "RAC-RC-01": ("expected.contains(&occurrence),",),
    "RAC-RC-02": ("derived.contains(&occurrence),",),
    "RAC-RC-03": (
        "assert_eq!(positions, expected_positions(&group, &image));",
        'assert_eq!(occurrences, factored, "positions depend only on the group and channel");',
    ),
    "RAC-RC-04": (
        "assert_eq!(derive(&observation, a), expected_a);",
        "assert_eq!(capacity(&aba_target.group, a), 2);",
    ),
    "RAC-RC-05": (
        "assert_capacity_occurrence(&rested, &join, &a_image, &[0, 2]);",
        "assert_position_bindings(&join, &produced, &join_a, &[10, 20]);",
    ),
    "RAC-RC-06": (
        "assert_eq!(occurrence.positions, expected_positions(&occurrence.group, image));",
        "assert_eq!(derive(&observation, &[0x01, 0x00]).len(), 1);",
    ),
    "RAC-RC-07": (
        "assert_eq!(derive_all(&with_data), derive_all(&removed));",
        'assert_eq!(derive_all(&renumbered), derive_all(&removed), "renumbering keeps the relation");',
    ),
    "RAC-RC-08": ("assert_join_projection(&space, &universe, &channels);",),
    "RAC-RC-09": (
        "assert_construction_order_invariance(&fixed);",
        "assert_construction_order_invariance(&plans);",
    ),
    "RAC-RC-10": (
        'assert_eq!(target, id.target, "an id resolves to its own full bytes");',
        'assert_eq!(resolve(&world, &forged), None, "a perturbed id never resolves");',
    ),
    "RAC-RC-11": (
        'assert_eq!(targets.len(), 4, "persist, peeks and body each keep a distinct target");',
        'assert_ne!(first_target, alias_target, "a source alias keeps a distinct id");',
    ),
    "RAC-RC-12": (
        'assert_eq!(buckets.bucket_len(target), targets.len(), "every target collides");',
        "assert_eq!(buckets.resolve(target), Some(target));",
    ),
    "RAC-RC-13": (
        'assert_eq!(resolve(&mutated_world, id), None, "a prior-revision id never resolves");',
        'assert_eq!(resolve(&replaced, id), None, "a prior-generation id never resolves");',
    ),
    "RAC-RC-14": (
        "assert_eq!(store.put_continuation(&channels, alias.clone()), Some(false));",
        '"the first source stays the representative"',
    ),
    "RAC-RC-15": (
        '"an insertion replaces the group version"',
        "assert_matches_rebuild(&maintained, &after);",
    ),
    "RAC-RC-16": (
        '"a removal replaces the group version"',
        "assert!(!after.observation.contains_target(&named));",
    ),
    "RAC-RC-17": (
        'assert_ne!(occurrence.group, emptied_key, "a cleared group leaves no occurrence");',
        "assert!(!after.observation.support().contains(&emptied_key));",
    ),
    "RAC-RC-18": (
        "assert_eq!(derive(&installed.observation, image).len(), 2);",
        "assert_eq!(maintained.classify(&uninstalled), AcceleratorClass::Foreign);",
    ),
    "RAC-RC-19": (
        "assert_eq!(maintained.classify(&partial), AcceleratorClass::Incomplete);",
        "assert_eq!(neutral, before.observation);",
    ),
    "RAC-RC-20": (
        "assert_every_version_replaced(&original.observation, &cleared.observation, &universe);",
        "assert_eq!(accelerator.classify(&cleared), AcceleratorClass::Foreign);",
    ),
    "RAC-RC-21": ('"equal group versions imply equal group relations"',),
    "RAC-RC-22": (
        'assert!(absent_nonempty > 0, "the absent state is probed where the derivation is nonempty");',
    ),
    "RAC-RC-23": ('assert_eq!(accelerator, held, "a failed update publishes nothing");',),
    "RAC-RC-24": ('"a replaced group never reuses the cached automaton"',),
    "RAC-RC-25": (
        'assert!(space.process_match_found(held).is_none(), "production refuses a stale ticket");',
        "assert_same_hot_state(&store.snapshot(), &expected);",
    ),
    "RAC-RC-26": ('"receive identity is accelerator-free"',),
    "RAC-TLA-02": ("assert_single_revision(&accelerator, &initial);",),
    "RAC-TLA-03": (
        "let report = check_trace(ReferenceAccelerator::new, &*store, &groups, &probes(), &steps);",
    ),
    "RAC-TLA-04": ("let _ = assert_answers_name_present_receives(&plans, &removals);",),
    "RAC-TLA-05": ("let absent_nonempty = assert_reads_equal_derivation(",),
    "RAC-TLA-06": ("assert_stale_fallback(&accelerator, &after, name);",),
    "RAC-TLA-07": (
        "assert_eq!(accelerator.classify(&partial_world), AcceleratorClass::Incomplete);",
        "Answer::Fallback(AcceleratorClass::Incomplete)",
    ),
    "RAC-TLA-08": (
        "assert_eq!(held.classify(&regenerated), AcceleratorClass::Foreign);",
        'assert!(distinguishing > 0, "adopting the foreign accelerator would answer wrongly");',
    ),
    "RAC-TLA-09": ("assert_eq!(commit_candidate(&current_world, id), CommitOutcome::Stale);",),
    "RAC-TLA-10": (
        "assert_eq!(accelerator.apply(&operation, &before, &after), Err(Fault::Injected(stage)));",
    ),
    "RAC-TLA-11": ('"{step:?}: acceleration never changes the selection"',),
}
SOURCE_MUTANT_EVIDENCE = {
    "RAC-RS-01": ("assert_eq!(buckets.resolve(target), Some(target));",),
    "RAC-RS-02": ("assert_matches_rebuild(&maintained, &after);",),
    "RAC-RS-03": ("let absent_nonempty = assert_reads_equal_derivation(",),
    "RAC-RS-04": ("assert_stale_fallback(&accelerator, &after, name);",),
    "RAC-RS-05": ("assert_eq!(held.classify(&regenerated), AcceleratorClass::Foreign);",),
    "RAC-RS-06": ("assert_eq!(accelerator.classify(&partial_world), AcceleratorClass::Incomplete);",),
    "RAC-RS-07": ("assert_eq!(derive(&observation, a), expected_a);",),
    "RAC-RS-08": ("assert_eq!(derive(&installed.observation, image).len(), 2);",),
    "RAC-RS-09": ("assert_construction_order_invariance(&fixed);",),
    "RAC-RS-10": ("assert_single_revision(&accelerator, &after);",),
    "RAC-RS-11": ('assert_eq!(accelerator, held, "a failed update publishes nothing");',),
    "RAC-RS-12": ("assert_eq!(accelerated_read, unaccelerated_read);",),
    "RAC-RS-13": ("assert_matches_rebuild(&accelerator, &after);",),
    "RAC-RS-14": ("assert_eq!(commit_candidate(&current_world, id), CommitOutcome::Stale);",),
    "RAC-RS-15": (
        'assert_eq!(targets.len(), 4, "persist, peeks and body each keep a distinct target");',
    ),
    "RAC-RS-16": ("assert_answers_name_present_receives(&fixed, &[(0, 0), (0, 0)]) > 0,",),
    "RAC-RS-17": ('"an insertion replaces the group version"',),
    "RAC-RS-18": (
        "assert_ne!(installed.observation.version(&key), reinstalled.observation.version(&key));",
    ),
    "RAC-RS-19": ('"a removal replaces the group version"',),
    "RAC-RS-20": ('"equal group versions imply equal group relations"',),
    "RAC-RS-21": (
        "assert_every_version_replaced(&original.observation, &cleared.observation, &universe);",
    ),
    "RAC-RS-22": (
        'assert_eq!(resolve(&replaced, id), None, "a prior-generation id never resolves");',
    ),
    "RAC-RS-23": (
        'assert!(space.process_match_found(held).is_none(), "production refuses a stale ticket");',
    ),
    "RAC-RS-24": ('"a datum matching only the replacement fires it"',),
    "RAC-RS-25": (
        '"{name}: adoption requires the bound semantic manifest to equal the current one"',
        'assert_eq!(foreign, 4, "four bindings are foreign to the current manifest");',
    ),
}
PRODUCTION_WITNESSES = {
    "production_answers_equal_the_derivation_over_generated_stores": (
        PRODUCTION_SPEC,
        ("RAC-RC-01", "RAC-RC-02"),
        "prop_assert_eq!(&published.answer(&world, &image), &expected);",
    ),
    "production_positions_come_from_the_group_prefix": (
        PRODUCTION_SPEC,
        ("RAC-RC-03",),
        "\"an occurrence is its target's own projection at the probed channel\"",
    ),
    "production_repeated_channels_yield_one_occurrence_per_target": (
        PRODUCTION_SPEC,
        ("RAC-RC-04", "RAC-RC-05"),
        "assert_eq!(accelerator.answer(&world, a), Answer::Exact(expected_a.clone()));",
    ),
    "production_prefix_aliased_images_never_share_occurrences": (
        PRODUCTION_SPEC,
        ("RAC-RC-06",),
        '"an image reaches exactly the groups holding it"',
    ),
    "production_answers_ignore_data_and_authority_orders": (
        PRODUCTION_SPEC,
        ("RAC-RC-07",),
        '"a held handle answers unchanged after a datum"',
    ),
    "production_exact_groups_are_the_join_groups_with_targets": (
        PRODUCTION_SPEC,
        ("RAC-RC-08",),
        '"a fired single receive leaves no join"',
    ),
    "production_history_groups_without_joins_are_invisible_to_both_paths": (
        PRODUCTION_SPEC,
        ("RAC-RC-08",),
        '"an unjoined history group is invisible to the accelerator and to the joins"',
    ),
    "production_construction_order_does_not_change_answers_or_ids": (
        PRODUCTION_SPEC,
        ("RAC-RC-09",),
        'assert_construction_order_invariance(&fixed).expect("the fixed plan is order invariant");',
    ),
    "production_receive_ids_resolve_only_to_byte_identical_records": (
        PRODUCTION_SPEC,
        ("RAC-RC-10", "RAC-RC-12"),
        '"a forgery differs from every current target"',
    ),
    "production_metadata_distinct_records_keep_distinct_ids": (
        PRODUCTION_SPEC,
        ("RAC-RC-11",),
        '"persist, peeks and the continuation each keep a distinct identity"',
    ),
    "production_ids_and_handles_from_a_prior_revision_or_generation_never_answer": (
        PRODUCTION_SPEC,
        ("RAC-RC-13",),
        '"a current id resolves to its own target"',
    ),
    "production_alias_puts_keep_the_revision_answers_and_first_source": (
        PRODUCTION_SPEC,
        ("RAC-RC-14",),
        '"the first source stays the representative"',
    ),
    "production_insertions_maintain_the_published_accelerator": (
        PRODUCTION_SPEC,
        ("RAC-RC-15",),
        '"an insertion replaces the group version"',
    ),
    "production_nonfinal_removals_maintain_the_published_accelerator": (
        PRODUCTION_SPEC,
        ("RAC-RC-16",),
        '"a removal replaces the group version"',
    ),
    "production_final_removal_clears_the_maintained_group": (
        PRODUCTION_SPEC,
        ("RAC-RC-17",),
        '"a cleared group leaves no occurrence group, not even an empty one"',
    ),
    "production_installs_maintain_and_set_state_retires_the_published_accelerator": (
        PRODUCTION_SPEC,
        ("RAC-RC-18",),
        '"an install replaces the group version"',
    ),
    "production_history_loads_extend_the_maintained_scope": (
        PRODUCTION_SPEC,
        ("RAC-RC-19",),
        '"a history load extends the maintained scope"',
    ),
    "production_clear_and_set_state_retire_held_handles_and_rebuilds_equal_the_derivation": (
        PRODUCTION_SPEC,
        ("RAC-RC-20",),
        "assert_generation_retires_held_handles(",
    ),
    "production_equal_group_versions_imply_equal_group_answers": (
        PRODUCTION_SPEC,
        ("RAC-RC-21",),
        '"the prefix produce fires the single-channel receive"',
    ),
    "production_reads_equal_derivation_in_every_accelerator_state": (
        PRODUCTION_SPEC,
        ("RAC-RC-22", "RAC-TLA-05"),
        '"the absent state is probed where the derivation is nonempty"',
    ),
    "production_refused_updates_leave_the_store_and_accelerator_unchanged": (
        PRODUCTION_SPEC,
        ("RAC-RC-23", "RAC-TLA-10"),
        '"a refused removal changes no version or target"',
    ),
    "production_index_and_catalog_publish_at_one_revision": (
        PRODUCTION_SPEC,
        ("RAC-RC-24", "RAC-TLA-02"),
        "assert_one_revision(&store, &accelerator, &after);",
    ),
    "production_same_count_group_replacement_never_reuses_a_stale_answer_or_handle": (
        PRODUCTION_SPEC,
        ("RAC-RC-24",),
        '"the replacement keeps the candidate count"',
    ),
    "production_stale_tickets_ids_and_handles_never_commit": (
        PRODUCTION_SPEC,
        ("RAC-RC-25", "RAC-TLA-09"),
        '"the old id names a receive that is still present"',
    ),
    "production_accelerator_never_changes_semantic_identity": (
        PRODUCTION_SPEC,
        ("RAC-RC-26", "RAC-TLA-11"),
        '"{step:?}: the accelerator never changes a selection or a checkpoint"',
    ),
    "production_maintenance_equals_rebuild_over_generated_traces": (
        PRODUCTION_SPEC,
        ("RAC-TLA-03",),
        "let report = check_trace(make, &*store, &groups, &probes(), &steps);",
    ),
    "production_answers_never_name_absent_receives": (
        PRODUCTION_SPEC,
        ("RAC-TLA-04",),
        '"the fixed trace removes receives"',
    ),
    "production_held_handles_fall_back_after_every_mutation": (
        PRODUCTION_SPEC,
        ("RAC-TLA-06",),
        "assert_stale_fallback(&held, &held_answers, &after, name);",
    ),
    "production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated": (
        PRODUCTION_SPEC,
        ("RAC-TLA-07",),
        '"{gap:?}: a current handle falls back as incomplete"',
    ),
    "production_held_handles_never_answer_for_groups_loaded_after_they_were_taken": (
        PRODUCTION_SPEC,
        ("RAC-TLA-07",),
        '"a held handle never answers for a group loaded after it was taken"',
    ),
    "production_foreign_generation_or_manifest_accelerators_are_never_adopted": (
        PRODUCTION_SPEC,
        ("RAC-TLA-08",),
        '"an accelerator bound to another manifest is never adopted"',
    ),
    "occurrence_keys_are_exact_under_prefix_aliased_images": (
        OCCURRENCE_TESTS,
        ("RAC-RC-06",),
        "assert_eq!(keys.len(), distinct_occurrences);",
    ),
    "faulted_maintenance_publishes_nothing": (
        OCCURRENCE_TESTS,
        ("RAC-RC-23", "RAC-TLA-10"),
        "assert!(faulted.receive_index().is_none());",
    ),
    "forged_alias_source_targets_never_resolve_at_current_tokens": (
        BINDING_TESTS,
        ("RAC-RC-10",),
        "assert!(store.resolve_receive(&forged).is_none());",
    ),
    "publication_refuses_a_stale_base_or_a_history_load": (
        BINDING_TESTS,
        ("RAC-RC-23", "RAC-TLA-10"),
        '"{disturbance} during a build refuses its publication"',
    ),
    "held_handles_never_answer_for_groups_loaded_after_they_were_taken": (
        BINDING_TESTS,
        ("RAC-TLA-07",),
        "fallback(store.receive_occurrences(Some(&held), &receiving)),",
    ),
    "every_continuation_change_issues_the_bumped_group_epoch_as_a_fresh_store_token": (
        TOKEN_TESTS,
        ("RAC-RC-21",),
        "assert_fresh_revision(&store, &mut issued, &resting);",
    ),
    "production_adoption_requires_the_stored_bound_semantic_manifest": (
        LIB,
        ("RAC-RC-20", "RAC-TLA-08"),
        '"{name}: production adoption requires the bound manifest to equal the current one"',
    ),
    "production_compiled_join_capacity_agrees_with_occurrence_multiplicity": (
        PRODUCTION_RHOLANG,
        ("RAC-RC-05",),
        '"one datum on a cannot fill both a positions, so the join keeps resting"',
    ),
    "production_checkpoint_reset_and_soft_revert_rebuild_identical_occurrences": (
        PRODUCTION_RHOLANG,
        ("RAC-RC-20",),
        'assert_absent_everywhere(&checkpointed_store, &probed, "checkpoint");',
    ),
}
TEST_ATTRIBUTE = re.compile(r"^#\[(?:tokio::)?test\b")
TEST_FUNCTION = re.compile(r"^(?:async[ \t]+)?fn[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*\(")


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def identifiers(kind, count):
    return [f"RAC-{kind}-{index:02d}" for index in range(1, count + 1)]


def mode(identifier):
    if identifier in REFINEMENT:
        return "executable_refinement"
    if identifier in PROOF_ONLY:
        return "proof_only"
    return "concrete_witness"


def module_text(source, module, label):
    start = re.search(rf"(?m)^Module {re.escape(module)}\.$", source)
    if start is None:
        raise RegistryError(f"{label} lacks module {module}")
    end = re.search(rf"(?m)^End {re.escape(module)}\.$", source[start.end() :])
    if end is None:
        raise RegistryError(f"{label} module {module} is not closed")
    return source[start.end() : start.end() + end.start()]


def names(source, pattern, label):
    found = re.findall(pattern, source, flags=re.MULTILINE)
    require(len(found) == len(set(found)), f"duplicate {label}")
    return found


def ordered_closure(source, path, module, expected):
    text = module_text(source, module, path)
    theorems = names(text, r"^Theorem[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*:", f"{module} theorem")
    require(sorted(theorems) == sorted(expected), f"missing or added {module} theorem")
    require(theorems == expected, f"reordered {module} theorem")
    printed = names(
        text,
        r"^Print Assumptions[ \t]+([A-Za-z_][A-Za-z_0-9.]*)\.$",
        f"{module} closed-assumption target",
    )
    require(printed == expected, f"missing, added, or reordered {module} closure")
    return printed


def directives(source, name):
    found = re.findall(rf"(?m)^{name}[ \t]+([A-Za-z][A-Za-z0-9_]*)[ \t]*$", source)
    require(
        len(found) == len(re.findall(rf"(?m)^{name}\b", source)),
        f"unsupported {name} directive",
    )
    require(len(found) == len(set(found)), f"duplicate {name} directive")
    return found


def tests_in(source, label):
    found = []
    attributes = []
    for line in source.splitlines():
        stripped = line.strip()
        if stripped.startswith("#["):
            attributes.append(stripped)
            continue
        function = TEST_FUNCTION.match(stripped)
        if function and any(TEST_ATTRIBUTE.match(attribute) for attribute in attributes):
            require(
                not any(attribute.startswith("#[ignore") for attribute in attributes),
                f"ignored-only test {function.group(1)} in {label}",
            )
            require(
                not any(attribute.startswith(("#[cfg(", "#[cfg_attr(")) for attribute in attributes),
                f"conditionally absent test {function.group(1)} in {label}",
            )
            found.append(function.group(1))
        if stripped:
            attributes = []
    require(len(found) == len(set(found)), f"duplicate test name in {label}")
    return found


def test_body(source, name):
    found = list(
        re.finditer(rf"(?m)^[ \t]*(?:async[ \t]+)?fn[ \t]+{re.escape(name)}[ \t]*\(", source)
    )
    require(len(found) == 1, f"missing or duplicate executable witness {name}")
    following = re.search(r"(?m)^[ \t]*#\[(?:tokio::)?test\b", source[found[0].end() :])
    end = found[0].end() + following.start() if following else len(source)
    return source[found[0].start() : end]


def generator_mutations(text):
    module = ast.parse(text)
    constants = {}
    table = None
    for node in module.body:
        if not (
            isinstance(node, ast.Assign)
            and len(node.targets) == 1
            and isinstance(node.targets[0], ast.Name)
        ):
            continue
        name = node.targets[0].id
        if name == "MUTATIONS":
            table = node.value
        elif isinstance(node.value, ast.Constant) and isinstance(node.value.value, str):
            constants[name] = node.value.value
    if table is None:
        raise RegistryError("mutant generator has no MUTATIONS table")

    class Resolve(ast.NodeTransformer):
        def visit_Name(self, node):
            require(node.id in constants, f"mutant generator names unknown constant {node.id}")
            return ast.copy_location(ast.Constant(constants[node.id]), node)

    return ast.literal_eval(Resolve().visit(table))


def mutant_configs():
    return [f"{TLA_ROOT}/{STEM}{name}Mutant.cfg" for name in TLA_MUTANTS]


def paths():
    return [
        ROCQ,
        MUTANTS,
        CHANNEL_SCOPE,
        CHANNEL_SCOPE_MUTANTS,
        MODEL,
        CONFIG,
        MUTANT_MODEL,
        *mutant_configs(),
        REFERENCE,
        ADOPTION,
        SPEC,
        RHOLANG,
        LIB,
        PRODUCTION_SPEC,
        PRODUCTION_RHOLANG,
        PRODUCTION_SUPPORT,
        OCCURRENCE_TESTS,
        BINDING_TESTS,
        TOKEN_TESTS,
        GENERATOR,
        CHECKER,
        *UNHASHED,
    ]


def isolated_paths():
    return [
        *(
            path.relative_to(ROOT).as_posix()
            for root in ISOLATED_ROOTS
            for path in sorted((ROOT / root).rglob("*.rs"))
        ),
        *ISOLATED_FILES,
    ]


def runner_test_line(runner, helper, count, *arguments):
    tail = r"[ \t]+".join(re.escape(argument) for argument in arguments)
    return re.search(rf"{helper}[ \t]+{count}[ \t]+(?:[^\n]*?[ \t]+)?{tail}(?:[ \t]|$)", runner, re.M)


def extract(sources, configs):
    rocq = sources[ROCQ]
    mutants = sources[MUTANTS]
    runner = re.sub(r"\\\n", " ", sources[RUNNER])

    for path in (ROCQ, MUTANTS, CHANNEL_SCOPE, CHANNEL_SCOPE_MUTANTS):
        require(
            not re.search(r"\b(?:Admitted|admit|Axiom|Axioms|Parameter|Parameters)\b", sources[path]),
            f"{path} contains an admitted or axiomatic declaration",
        )
    accelerator_module = module_text(rocq, ROCQ_MODULE, ROCQ)
    theorems = names(
        accelerator_module,
        r"^Theorem[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*:",
        f"{ROCQ_MODULE} theorem",
    )
    require(sorted(theorems) == sorted(THEOREMS), f"missing or added {ROCQ_MODULE} theorem")
    printed = names(
        accelerator_module,
        r"^Print Assumptions[ \t]+([A-Za-z_][A-Za-z_0-9.]*)\.$",
        f"{ROCQ_MODULE} closed-assumption target",
    )
    require(printed == THEOREMS, f"missing, added, or reordered {ROCQ_MODULE} closure")

    mutant_module = module_text(mutants, MUTANT_MODULE, MUTANTS)
    examples = names(
        mutant_module,
        r"^Example[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*:",
        f"{MUTANT_MODULE} example",
    )
    require(examples == MUTANT_EXAMPLES, f"missing, added, or reordered {MUTANT_MODULE} example")
    require(
        len(re.findall(rf"(?m)^Theorem[ \t]+{BRIDGE}[ \t]*:", mutant_module)) == 1,
        f"{MUTANT_MODULE} lacks its implementation bridge",
    )
    mutant_prints = names(
        mutant_module,
        r"^Print Assumptions[ \t]+([A-Za-z_][A-Za-z_0-9.]*)\.$",
        f"{MUTANT_MODULE} closed-assumption target",
    )
    require(
        mutant_prints == [BRIDGE, *MUTANT_EXAMPLES],
        f"missing, added, or reordered {MUTANT_MODULE} closure",
    )
    scope_theorems = [name for held in CHANNEL_SCOPE_CAPSTONES.values() for name in held]
    scope_mutants = [
        CHANNEL_SCOPE_BRIDGE,
        *(name for held in CHANNEL_SCOPE_REFUTATIONS.values() for name in held),
    ]
    scope_prints = ordered_closure(sources[CHANNEL_SCOPE], CHANNEL_SCOPE, CHANNEL_SCOPE_MODULE, scope_theorems)
    scope_mutant_prints = ordered_closure(
        sources[CHANNEL_SCOPE_MUTANTS], CHANNEL_SCOPE_MUTANTS, CHANNEL_SCOPE_MUTANT_MODULE, scope_mutants
    )
    for path, declared in (
        (ROCQ, THEOREMS),
        (MUTANTS, [BRIDGE, *MUTANT_EXAMPLES]),
        (CHANNEL_SCOPE, scope_theorems),
        (CHANNEL_SCOPE_MUTANTS, scope_mutants),
    ):
        for name in declared:
            require(
                len(re.findall(rf"(?m)^(?:{DECLARATIONS})[ \t]+{name}\b", sources[path])) == 1,
                f"{name} is not declared exactly once in {path}",
            )

    start = runner.find(f'if [[ "${{1:-}}" == "{FLAG}" ]]; then')
    require(start >= 0, f"focused {FLAG} gate absent from the proof runner")
    finish = runner.find("\nfi\n", start)
    require(finish > start, f"focused {FLAG} gate is not closed")
    focused = runner[start:finish]
    remainder = runner[:start] + runner[finish:]
    for path, expected in CLOSED_REPORTS.items():
        theory = path.split("stack_safe_pda/", 1)[1]
        compilation = rf"rocq_compile_with_closed_count[ \t]+{re.escape(theory)}[ \t]+{expected}[ \t]"
        require(
            len(re.findall(compilation, runner)) == 2 and len(re.findall(compilation, focused)) == 1,
            f"{theory} {expected}-closure compilation absent from the normal or focused proof runner",
        )
    for theory in (ROCQ_MODULE, MUTANT_MODULE, CHANNEL_SCOPE_MODULE, CHANNEL_SCOPE_MUTANT_MODULE):
        require(
            re.search(rf"rocq_reject_placeholders[^\n]*theories/{theory}\.v", focused),
            f"focused gate does not reject placeholders in {theory}.v",
        )
        require(
            re.search(rf"rocq_check_modules[^\n]*StackSafePDA\.{theory}(?:[ \t]|$)", focused, re.M),
            f"focused gate does not kernel-check {theory}",
        )

    model = sources[MODEL]
    config = sources[CONFIG]
    require(directives(config, "SPECIFICATION") == ["Spec"], "wrong base specification")
    require(
        not re.search(r"(?m)^(?:INVARIANTS|PROPERTIES|PROPERTY)\b", config),
        "unsupported base obligation",
    )
    invariants = directives(config, "INVARIANT")
    require(invariants == INVARIANTS, "missing, extra, or reordered invariant binding")
    for symbol in invariants:
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}[ \t]*==", model)) == 1,
            f"missing or duplicate model invariant: {symbol}",
        )
    for run in (
        f"{STEM}.cfg {STEM}.tla -cleanup",
        f'"$receive_accelerator_cfg" {STEM}Mutants.tla -cleanup',
    ):
        require(
            focused.count(run) == 1 and remainder.count(run) == 1,
            f"TLC run absent from the normal or focused proof runner: {run}",
        )
    require(
        set(configs) == {path.rsplit("/", 1)[1] for path in mutant_configs()},
        "missing or extra mutant config",
    )
    require(
        [invariant for _, invariant in TLA_MUTANTS.values()] == INVARIANTS[1:],
        "TLA+ mutants do not refute RAC-TLA-02 to RAC-TLA-11 one-to-one, in order",
    )
    entries = []
    for name, (label, invariant) in TLA_MUTANTS.items():
        filename = f"{STEM}{name}Mutant.cfg"
        source = sources[f"{TLA_ROOT}/{filename}"]
        require(
            directives(source, "SPECIFICATION") == [f"{name}Spec"]
            and directives(source, "INVARIANT") == ["TypeInvariant", invariant],
            f"wrong mutant specification or invariant: {filename}",
        )
        require(
            len(re.findall(rf"(?m)^{name}[ \t]*==", sources[MUTANT_MODEL])) == 1
            and len(re.findall(rf"(?m)^{name}Spec[ \t]*==", sources[MUTANT_MODEL])) == 1,
            f"mutant action or specification is missing: {filename}",
        )
        entry = f"{label}:{filename}:{invariant}"
        require(
            focused.count(entry) == 1 and remainder.count(entry) == 1,
            f"mutant is absent from the normal or focused proof runner: {filename}",
        )
        entries.append(entry)
    loop = rf'"([a-z-]+:{STEM}[A-Za-z]+Mutant\.cfg:[A-Za-z]+)"'
    require(
        re.findall(loop, focused) == entries and re.findall(loop, remainder) == entries,
        "the normal or focused mutant loop is not exactly the registered loop",
    )

    for path, cargo in (
        (SPEC, RSPACE_CARGO),
        (RHOLANG, RHOLANG_CARGO),
        (PRODUCTION_SPEC, RSPACE_CARGO),
        (PRODUCTION_RHOLANG, RHOLANG_CARGO),
    ):
        require(
            not re.search(r"(?m)^autotests[ \t]*=[ \t]*false\b", sources[cargo]),
            f"{path} absent from Cargo's normal test discovery",
        )
    require(
        '#[cfg(test)]\n#[path = "../../../tests/persistence/mod.rs"]\nmod tests;' in sources[LIB_ROOT]
        and re.search(rf"(?m)^mod {LIB_FILTER};$", sources[LIB_MODULES]),
        f"{LIB} is not compiled into the rspace++ library tests",
    )
    require(
        '#[path = "support/receive_occurrence_reference.rs"]' in sources[SPEC]
        and '#[path = "../../rspace++/tests/support/receive_occurrence_reference.rs"]'
        in sources[RHOLANG]
        and '#[path = "receive_accelerator_adoption.rs"]' in sources[REFERENCE]
        and '#[path = "../support/receive_accelerator_adoption.rs"]' in sources[LIB],
        "the reference oracle or its adoption rule is not compiled into its suites",
    )
    require(
        '#[path = "support/receive_occurrence_reference.rs"]' in sources[PRODUCTION_SPEC]
        and '#[path = "support/receive_accelerator_production.rs"]' in sources[PRODUCTION_SPEC]
        and '#[path = "../../rspace++/tests/support/receive_occurrence_reference.rs"]'
        in sources[PRODUCTION_RHOLANG],
        "the reference oracle or the production adapter is not compiled into the production suites",
    )
    for path, host, mount in (
        (
            OCCURRENCE_TESTS,
            RECEIVE_ACCELERATOR,
            '#[cfg(test)]\n#[path = "../../tests/receive_accelerator/occurrence_index.rs"]\nmod occurrence_index_tests;',
        ),
        (
            BINDING_TESTS,
            HOT_STORE,
            '#[cfg(test)]\n#[path = "../../tests/hot_store/receive_accelerator_bindings.rs"]\n'
            "mod receive_accelerator_bindings_tests;",
        ),
        (
            TOKEN_TESTS,
            HOT_STORE,
            '#[cfg(test)]\n#[path = "../../tests/hot_store/receive_revision_tokens.rs"]\nmod receive_revision_tokens_tests;',
        ),
    ):
        require(mount in sources[host], f"{path} is not compiled into the rspace++ library tests")
    prefixes = tuple(f"{root}/" for root in ISOLATED_ROOTS)
    isolated = sorted(
        path
        for path in sources
        if path in ISOLATED_FILES or (path.endswith(".rs") and path.startswith(prefixes))
    )
    for root, prefix in zip(ISOLATED_ROOTS, prefixes):
        require(
            any(path.startswith(prefix) for path in isolated),
            f"{root} has no source checked for receive accelerator names",
        )
    require(
        all(path in sources for path in ISOLATED_FILES),
        "an isolated egress source is not checked for receive accelerator names",
    )
    for path in isolated:
        require(not ISOLATION_PATTERN.search(sources[path]), f"{path} names the receive accelerator")

    suites = {}
    for path in SUITE_COUNTS:
        found = tests_in(sources[path], path)
        require(len(found) == SUITE_COUNTS[path], f"{path} normal-suite count changed")
        suites[path] = found
    require(
        runner_test_line(focused, "rspace_preparation_run_tests", SUITE_COUNTS[SPEC], "--test", "receive_accelerator_spec"),
        "focused gate does not run receive_accelerator_spec with its count",
    )
    require(
        runner_test_line(focused, "rspace_preparation_run_tests", SUITE_COUNTS[LIB], "--lib", LIB_FILTER),
        "focused gate does not run the manifest-binding library test with its count",
    )
    require(
        runner_test_line(focused, "rholang_run_tests", SUITE_COUNTS[RHOLANG], "--test", "receive_accelerator_rholang"),
        "focused gate does not run receive_accelerator_rholang with its count",
    )
    for path, helper, kind, target in (
        (PRODUCTION_SPEC, "rspace_preparation_run_tests", "--test", "receive_accelerator_production_spec"),
        (OCCURRENCE_TESTS, "rspace_preparation_run_tests", "--lib", LIBRARY_FILTERS[OCCURRENCE_TESTS]),
        (BINDING_TESTS, "rspace_preparation_run_tests", "--lib", LIBRARY_FILTERS[BINDING_TESTS]),
        (TOKEN_TESTS, "rspace_preparation_run_tests", "--lib", LIBRARY_FILTERS[TOKEN_TESTS]),
        (PRODUCTION_RHOLANG, "rholang_run_tests", "--test", "receive_accelerator_production_rholang"),
    ):
        require(
            runner_test_line(focused, helper, SUITE_COUNTS[path], kind, target),
            f"focused gate does not run {target} with its count",
        )
    aggregate = runner.find('python3 "$repo_root/scripts/build-pathmap-obligation-registry.py" --check')
    require(0 <= aggregate < start, "the aggregate registry check does not precede the focused gate")
    invocation = 'python3 "$repo_root/scripts/check-receive-accelerator-obligations.py"'
    require(
        runner.count(f"{invocation} --check") == 1 and f"{invocation} --check" not in focused,
        "receive accelerator registry check absent from, or duplicated in, the normal proof runner",
    )
    require(
        runner.count(f"{invocation} --self-test") == 2 and f"{invocation} --self-test" in focused,
        "receive accelerator registry mutations absent from the normal or focused proof runner",
    )

    capstones = identifiers("RC", len(THEOREMS))
    refutations = identifiers("MUT", len(MUTANT_EXAMPLES))
    bindings = identifiers("TLA", len(INVARIANTS))
    controls = identifiers("TMUT", len(TLA_MUTANTS))
    owned = {
        identifier: (path, test)
        for test, (path, held) in OWNERS.items()
        for identifier in held
    }
    require(
        sorted(owned) == sorted([*capstones, *bindings[1:]])
        and len(owned) == sum(len(held) for _, held in OWNERS.values()),
        "every capstone and witnessed invariant needs exactly one owning test",
    )
    require(set(EVIDENCE) == set(owned), "unbound receive accelerator witness evidence")
    for identifier, (path, test) in owned.items():
        require(test in suites[path], f"{identifier} witness {test} is not a normal-suite test")
        body = test_body(sources[path], test)
        require(
            all(fragment in body for fragment in EVIDENCE[identifier]),
            f"{identifier} no longer has its concrete assertion in {test}",
        )
    require(
        [test for test, (path, _) in OWNERS.items() if path == SPEC] == suites[SPEC],
        f"{SPEC} tests do not own their obligations in file order",
    )
    require(
        PERSISTED_WITNESS[1] in suites[LIB],
        f"{CATALOG_ROW} witness {PERSISTED_WITNESS[1]} is not a normal-suite test",
    )
    production_owned = {}
    for test, (path, held, fragment) in PRODUCTION_WITNESSES.items():
        require(test in suites[path], f"production witness {test} is not a normal-suite test of {path}")
        require(held, f"production witness {test} witnesses no obligation")
        require(
            fragment in test_body(sources[path], test),
            f"production witness {test} lost its production evidence",
        )
        for identifier in held:
            production_owned.setdefault(identifier, []).append([path, test])
    require(
        [test for test, (path, _, _) in PRODUCTION_WITNESSES.items() if path == PRODUCTION_SPEC]
        == suites[PRODUCTION_SPEC],
        f"{PRODUCTION_SPEC} tests are not production witnesses in file order",
    )
    require(
        sorted(production_owned) == sorted([*capstones, *bindings[1:]]),
        "every capstone and witnessed invariant needs a production witness, and no witness names another id",
    )

    mutations = generator_mutations(sources[GENERATOR])
    require(
        list(mutations) == list(SOURCE_MUTANT_EVIDENCE),
        "source mutants are not RAC-RS-01 to RAC-RS-25 in order",
    )
    formals = [tuple(mutation["formal"]) for mutation in mutations.values()]
    require(
        formals[: len(refutations)] == [(identifier,) for identifier in refutations]
        and not any(
            identifier in refutations for formal in formals[len(refutations) :] for identifier in formal
        ),
        "source mutants do not correspond one-to-one, in order, to the Rocq mutants",
    )
    source_rows = []
    for (name, mutation), formal in zip(mutations.items(), formals):
        expected = mode(name)
        require(mutation["mode"] == expected, f"{name} is not classified {expected}")
        require(
            mutation["source"]
            in (PRODUCTION_SOURCES if expected == "executable_refinement" else HARNESS_SOURCES),
            f"{name} mutates an unregistered source",
        )
        require(
            sources[mutation["source"]].count(mutation["before"]) == 1,
            f"{name} source anchor is absent or ambiguous in {mutation['source']}",
        )
        require(mutation["before"] != mutation["after"], f"{name} is not a mutation")
        require(mutation["test_file"] in suites, f"{name} names an unregistered test file")
        require(
            mutation["test"] in suites[mutation["test_file"]],
            f"{name} killing test {mutation['test']} is not a normal-suite test",
        )
        killer = (mutation["test_file"], mutation["test"])
        if formal[0] not in refutations:
            require(set(formal) <= {*capstones, *bindings}, f"{name} refines an unregistered obligation")
            require(
                killer in {owned.get(identifier) for identifier in formal}
                or (formal == PERSISTED_REFINES and killer == PERSISTED_WITNESS),
                f"{name} is not killed by an owner of {', '.join(formal)}",
            )
        body = test_body(sources[mutation["test_file"]], mutation["test"])
        require(
            all(fragment in body for fragment in SOURCE_MUTANT_EVIDENCE[name]),
            f"{name} killing test {mutation['test']} lost its assertion",
        )
        source_rows.append(
            {
                "id": name,
                "formal": list(formal),
                "source": mutation["source"],
                "description": mutation["description"],
                "mode": mutation["mode"],
                "test_source": mutation["test_file"],
                "test_target": mutation["test"],
            }
        )
    persisted = [
        row["id"]
        for row in source_rows
        if (row["test_source"], row["test_target"]) == PERSISTED_WITNESS
    ]
    require(
        len(persisted) == 1 and tuple(mutations[persisted[0]]["formal"]) == PERSISTED_REFINES,
        f"{CATALOG_ROW} lacks exactly one persisted-manifest control",
    )
    by_refutation = {row["formal"][0]: row for row in source_rows if row["formal"][0] in refutations}
    refuted_by = dict(zip(INVARIANTS[1:], controls))

    return {
        "scope": "Generation-bound receive occurrence index: the receive relation derived from the semantic roots and the versioned installed input, exact receive identifiers, revision-bound accelerator classes, artifact publication, failure atomicity and candidate commit (designs 8010 and 8011)",
        "boundary": "The Rocq capstones and the bounded TLA+ invariants specify the derived index; the named tests check a test-only reference oracle over real HotStore and RSpace instances, and the production witnesses check the production receive accelerator of task 8011 against the same obligations. Harness controls RAC-RS-01 to RAC-RS-16 and RAC-RS-25 show that each named test observes its property, and production controls RAC-RS-17 to RAC-RS-24 show that the tests observe the production revision signals. None of this is a formal Rust refinement proof.",
        "sources_sha256": {
            path: hashlib.sha256(sources[path].encode()).hexdigest()
            for path in paths()
            if path not in UNHASHED
        },
        "rocq": {
            "source": ROCQ,
            "module": ROCQ_MODULE,
            "closed_reports": CLOSED_REPORTS[ROCQ],
            "print_assumptions": printed,
            "obligations": [
                {
                    "id": identifier,
                    "obligation": name,
                    "mode": mode(identifier),
                    "test_source": owned[identifier][0],
                    "test_target": owned[identifier][1],
                    "production_witnesses": production_owned[identifier],
                }
                for identifier, name in zip(capstones, printed)
            ],
        },
        "mutants": {
            "source": MUTANTS,
            "module": MUTANT_MODULE,
            "closed_reports": CLOSED_REPORTS[MUTANTS],
            "bridge": BRIDGE,
            "print_assumptions": mutant_prints,
            "obligations": [
                {
                    "id": identifier,
                    "obligation": name,
                    "mode": mode(identifier),
                    "source_mutant": by_refutation[identifier]["id"],
                    "test_source": by_refutation[identifier]["test_source"],
                    "test_target": by_refutation[identifier]["test_target"],
                }
                for identifier, name in zip(refutations, examples)
            ],
        },
        "channel_scope": {
            "source": CHANNEL_SCOPE,
            "module": CHANNEL_SCOPE_MODULE,
            "closed_reports": CLOSED_REPORTS[CHANNEL_SCOPE],
            "print_assumptions": scope_prints,
            "obligations": [
                {"id": identifier, "obligations": list(held), "mode": mode(identifier)}
                for identifier, held in CHANNEL_SCOPE_CAPSTONES.items()
            ],
        },
        "channel_scope_mutants": {
            "source": CHANNEL_SCOPE_MUTANTS,
            "module": CHANNEL_SCOPE_MUTANT_MODULE,
            "closed_reports": CLOSED_REPORTS[CHANNEL_SCOPE_MUTANTS],
            "bridge": CHANNEL_SCOPE_BRIDGE,
            "print_assumptions": scope_mutant_prints,
            "obligations": [
                {"id": identifier, "obligations": list(held), "mode": mode(identifier)}
                for identifier, held in CHANNEL_SCOPE_REFUTATIONS.items()
            ],
        },
        "tla": {
            "model": MODEL,
            "config": CONFIG,
            "specification": "Spec",
            "obligations": [
                {
                    "id": identifier,
                    "obligation": symbol,
                    "mode": mode(identifier),
                    "tla_mutant": refuted_by.get(symbol),
                    "test_source": owned.get(identifier, (None, None))[0],
                    "test_target": owned.get(identifier, (None, None))[1],
                    "production_witnesses": production_owned.get(identifier, []),
                }
                for identifier, symbol in zip(bindings, invariants)
            ],
        },
        "tla_mutants": {
            "model": MUTANT_MODEL,
            "obligations": [
                {
                    "id": identifier,
                    "obligation": f"{name}Spec",
                    "mode": mode(identifier),
                    "config": config_path,
                    "action": name,
                    "runner_label": label,
                    "expected_invariant": invariant,
                    "refutes": bindings[INVARIANTS.index(invariant)],
                }
                for identifier, config_path, (name, (label, invariant)) in zip(
                    controls, mutant_configs(), TLA_MUTANTS.items()
                )
            ],
        },
        "source_mutants": {"generator": GENERATOR, "obligations": source_rows},
        "persisted_manifest_witness": {
            "catalog_row": CATALOG_ROW,
            "refines": list(PERSISTED_REFINES),
            "source_mutant": persisted[0],
            "test_source": PERSISTED_WITNESS[0],
            "test_target": PERSISTED_WITNESS[1],
        },
        "normal_suite": {
            "runner_flag": FLAG,
            "library_filter": LIB_FILTER,
            "library_filters": LIBRARY_FILTERS,
            "tests": {path: suites[path] for path in SUITE_COUNTS},
        },
        "accelerator_isolation": {
            "roots": list(ISOLATED_ROOTS),
            "files": list(ISOLATED_FILES),
            "pattern": ISOLATION_PATTERN.pattern,
        },
    }


def self_test(sources, configs, baseline):
    def rejected(path, replacement, label, reason, mutated_configs=None):
        require(
            replacement != sources[path] or mutated_configs is not None,
            f"self-test mutation did not apply: {label}",
        )
        changed = dict(sources)
        changed[path] = replacement
        try:
            extract(changed, configs if mutated_configs is None else mutated_configs)
        except RegistryError as error:
            require(reason in str(error), f"{label} was refused for another reason: {error}")
            return
        raise RegistryError(f"mutation unexpectedly accepted: {label}")

    reference_anchor = "        bucket.iter().find(|candidate| *candidate == wanted)\n"
    ticket_anchor = (
        "            if expected.address.version.is_none() || expected.address.version != current_version {\n"
    )
    invocation = 'python3 "$repo_root/scripts/check-receive-accelerator-obligations.py"'
    stale_answer = f"{TLA_ROOT}/{STEM}StaleAnswerMutant.cfg"
    dropped = sources[GENERATOR].index('    "RAC-RS-16": {')
    retained = sources[GENERATOR].index('    "RAC-RS-17": {')
    rejected(
        ROCQ,
        sources[ROCQ].replace(
            "Theorem stale_revision_never_resolves :",
            "Theorem stale_revision_rarely_resolves :",
            1,
        ),
        "renamed capstone",
        f"missing or added {ROCQ_MODULE} theorem",
    )
    rejected(
        ROCQ,
        sources[ROCQ].replace("Print Assumptions semantic_successor_is_accelerator_free.\n", "", 1),
        "omitted capstone closure",
        f"reordered {ROCQ_MODULE} closure",
    )
    rejected(
        ROCQ,
        sources[ROCQ].replace(
            "Print Assumptions semantic_successor_is_accelerator_free.\n",
            "Axiom semantic_shortcut : True.\nPrint Assumptions semantic_successor_is_accelerator_free.\n",
            1,
        ),
        "axiomatic capstone dependency",
        "admitted or axiomatic declaration",
    )
    rejected(
        MUTANTS,
        sources[MUTANTS].replace("Print Assumptions metadata_erasing_target_is_refuted.\n", "", 1),
        "omitted mutant closure",
        f"reordered {MUTANT_MODULE} closure",
    )
    rejected(
        MUTANTS,
        sources[MUTANTS].replace(
            "Example split_artifact_publication_is_refuted :",
            "Example split_publication_is_refuted :",
            1,
        ),
        "renamed Rocq mutant",
        f"reordered {MUTANT_MODULE} example",
    )
    rejected(
        CHANNEL_SCOPE,
        sources[CHANNEL_SCOPE].replace(
            "Theorem equal_store_tokens_imply_equal_revisions :",
            "Theorem equal_store_tokens_imply_equal_generations :",
            1,
        ),
        "renamed channel-scope capstone",
        f"missing or added {CHANNEL_SCOPE_MODULE} theorem",
    )
    rejected(
        CHANNEL_SCOPE,
        sources[CHANNEL_SCOPE].replace("Print Assumptions equal_store_tokens_imply_equal_revisions.\n", "", 1),
        "omitted channel-scope closure",
        f"reordered {CHANNEL_SCOPE_MODULE} closure",
    )
    rejected(
        CHANNEL_SCOPE,
        sources[CHANNEL_SCOPE].replace(
            "Print Assumptions equal_store_tokens_imply_equal_revisions.\n",
            "Axiom channel_scope_shortcut : True.\nPrint Assumptions equal_store_tokens_imply_equal_revisions.\n",
            1,
        ),
        "axiomatic channel-scope dependency",
        "admitted or axiomatic declaration",
    )
    rejected(
        CHANNEL_SCOPE_MUTANTS,
        sources[CHANNEL_SCOPE_MUTANTS].replace(
            "Theorem live_scope_join_check_is_refuted :",
            "Theorem live_join_check_is_refuted :",
            1,
        ),
        "renamed channel-scope refutation",
        f"missing or added {CHANNEL_SCOPE_MUTANT_MODULE} theorem",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            "theories/TwoLevelRSpaceReceiveAcceleratorMutants.v 16",
            "theories/TwoLevelRSpaceReceiveAcceleratorMutants.v 15",
            1,
        ),
        "wrong runner closed count",
        "TwoLevelRSpaceReceiveAcceleratorMutants.v 16-closure compilation absent",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            "theories/TwoLevelRSpaceReceiveAcceleratorChannelScope.v 8",
            "theories/TwoLevelRSpaceReceiveAcceleratorChannelScope.v 7",
            1,
        ),
        "wrong channel-scope runner closed count",
        "TwoLevelRSpaceReceiveAcceleratorChannelScope.v 8-closure compilation absent",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(f"StackSafePDA.{CHANNEL_SCOPE_MUTANT_MODULE}", "", 1),
        "channel-scope refutations absent from the focused kernel check",
        f"focused gate does not kernel-check {CHANNEL_SCOPE_MUTANT_MODULE}",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(f"{invocation} --check\n", "", 1),
        "missing normal registry check",
        "registry check absent",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(f"{invocation} --self-test\n", "", 1),
        "missing focused registry mutations",
        "registry mutations absent",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            "rspace_preparation_run_tests 31 --test receive_accelerator_spec",
            "rspace_preparation_run_tests 30 --test receive_accelerator_spec",
            1,
        ),
        "wrong focused test count",
        "does not run receive_accelerator_spec with its count",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            "rholang_run_tests 5 --test receive_accelerator_rholang",
            "rholang_run_tests 4 --test receive_accelerator_rholang",
            1,
        ),
        "wrong focused Rholang test count",
        "does not run receive_accelerator_rholang with its count",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            "rspace_preparation_run_tests 2 --lib receive_accelerator_manifest_binding",
            "rspace_preparation_run_tests 1 --lib receive_accelerator_manifest_binding",
            1,
        ),
        "wrong focused library test count",
        "does not run the manifest-binding library test with its count",
    )
    for helper, count, kind, target in (
        ("rspace_preparation_run_tests", 31, "--test", "receive_accelerator_production_spec"),
        ("rspace_preparation_run_tests", 14, "--lib", "rspace::receive_accelerator::"),
        ("rspace_preparation_run_tests", 18, "--lib", "rspace::hot_store::receive_accelerator_bindings_tests::"),
        ("rspace_preparation_run_tests", 5, "--lib", "rspace::hot_store::receive_revision_tokens_tests::"),
        ("rholang_run_tests", 5, "--test", "receive_accelerator_production_rholang"),
    ):
        rejected(
            RUNNER,
            sources[RUNNER].replace(
                f"{helper} {count} {kind} {target}\n", f"{helper} {count - 1} {kind} {target}\n", 1
            ),
            f"wrong focused {target} count",
            f"does not run {target} with its count",
        )
    rejected(
        PRODUCTION_SPEC,
        sources[PRODUCTION_SPEC].replace(
            '#[tokio::test(flavor = "current_thread")]\n'
            "async fn production_foreign_generation_or_manifest_accelerators_are_never_adopted(",
            "async fn production_foreign_generation_or_manifest_accelerators_are_never_adopted(",
            1,
        ),
        "dropped production spec test",
        f"{PRODUCTION_SPEC} normal-suite count changed",
    )
    rejected(
        RECEIVE_ACCELERATOR,
        sources[RECEIVE_ACCELERATOR].replace("mod occurrence_index_tests;\n", "", 1),
        "unmounted occurrence index tests",
        f"{OCCURRENCE_TESTS} is not compiled into the rspace++ library tests",
    )
    rejected(
        PRODUCTION_SPEC,
        sources[PRODUCTION_SPEC].replace('#[path = "support/receive_accelerator_production.rs"]\n', "", 1),
        "unmounted production adapter",
        "the production adapter is not compiled into the production suites",
    )
    rejected(
        PRODUCTION_SPEC,
        sources[PRODUCTION_SPEC].replace(
            "fn production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated(",
            "fn production_scope_gaps_fall_back_until_the_channel_is_hydrated(",
            1,
        ),
        "renamed production witness",
        "production witness production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated is not a "
        "normal-suite test",
    )
    rejected(
        PRODUCTION_SPEC,
        sources[PRODUCTION_SPEC].replace('"a forgery differs from every current target"', '""', 1),
        "blanked production evidence",
        "production witness production_receive_ids_resolve_only_to_byte_identical_records lost its production "
        "evidence",
    )
    rejected(
        LIB_ROOT,
        "use crate::rspace::receive_accelerator::ReceiveRevision;\n" + sources[LIB_ROOT],
        "receive accelerator import in persistence",
        f"{LIB_ROOT} names the receive accelerator",
    )
    rejected(
        PATHMAP_CODEC,
        sources[PATHMAP_CODEC] + "// receive_index\n",
        "receive accelerator name in the egress codec",
        f"{PATHMAP_CODEC} names the receive accelerator",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(f"{STEM}.cfg {STEM}.tla -cleanup", "", 1),
        "base model absent from the focused gate",
        "TLC run absent from the normal or focused proof runner",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            f"stale-answer:{STEM}StaleAnswerMutant.cfg:StaleAcceleratorNeverAnswers",
            f"stale-answer:{STEM}StaleAnswerMutant.cfg:IncompleteAcceleratorNeverAnswers",
            1,
        ),
        "focused mutant loop bound to another invariant",
        f"mutant is absent from the normal or focused proof runner: {STEM}StaleAnswerMutant.cfg",
    )
    rejected(
        SPEC,
        sources[SPEC].replace(
            "fn colliding_buckets_resolve_by_full_bytes(",
            "#[ignore]\nfn colliding_buckets_resolve_by_full_bytes(",
            1,
        ),
        "ignored-only Rust witness",
        "ignored-only test colliding_buckets_resolve_by_full_bytes",
    )
    rejected(
        SPEC,
        sources[SPEC].replace(
            'assert_eq!(buckets.bucket_len(target), targets.len(), "every target collides");', "", 1
        ),
        "removed concrete collision assertion",
        "RAC-RC-12 no longer has its concrete assertion",
    )
    rejected(
        SPEC,
        sources[SPEC].replace('"a datum matching only the replacement fires it"', '""', 1),
        "removed source-mutant killing assertion",
        "RAC-RS-24 killing test same_count_group_replacement_never_reuses_a_stale_automaton",
    )
    rejected(
        SPEC,
        sources[SPEC].replace(
            "fn incremental_maintenance_equals_rebuild_over_generated_traces(",
            "fn incremental_maintenance_matches_rebuild_over_generated_traces(",
            1,
        ),
        "missing owning test",
        "RAC-TLA-03 witness incremental_maintenance_equals_rebuild_over_generated_traces is not",
    )
    rejected(
        SPEC,
        sources[SPEC] + "\nfn colliding_buckets_resolve_by_full_bytes() {}\n",
        "duplicated witness name",
        "missing or duplicate executable witness colliding_buckets_resolve_by_full_bytes",
    )
    rejected(
        RHOLANG,
        sources[RHOLANG].replace('#[tokio::test(flavor = "current_thread")]\n', "", 1),
        "dropped Rholang test",
        f"{RHOLANG} normal-suite count changed",
    )
    rejected(
        LIB,
        sources[LIB].replace(
            "#[test]\nfn persisted_accelerator_adoption_requires_the_bound_semantic_manifest(",
            "fn persisted_accelerator_adoption_requires_the_bound_semantic_manifest(",
            1,
        ),
        "dropped persisted-manifest library test",
        f"{LIB} normal-suite count changed",
    )
    rejected(
        LIB_MODULES,
        sources[LIB_MODULES].replace(f"mod {LIB_FILTER};\n", "", 1),
        "uncompiled persisted-manifest library test",
        "is not compiled into the rspace++ library tests",
    )
    rejected(
        CONFIG,
        sources[CONFIG].replace(
            "INVARIANT AnswerIsSound\nINVARIANT NoEnabledReceiveHidden\n",
            "INVARIANT NoEnabledReceiveHidden\nINVARIANT AnswerIsSound\n",
            1,
        ),
        "reordered base invariants",
        "missing, extra, or reordered invariant binding",
    )
    rejected(
        CONFIG,
        sources[CONFIG].replace("INVARIANT SemanticIdentityIsAcceleratorFree\n", "", 1),
        "dropped base invariant",
        "missing, extra, or reordered invariant binding",
    )
    rejected(
        MODEL,
        sources[MODEL].replace("FailedUpdateIsNonMutating ==", "FailedUpdateIsNonMutatingRemoved ==", 1),
        "undefined model invariant",
        "missing or duplicate model invariant: FailedUpdateIsNonMutating",
    )
    rejected(
        stale_answer,
        sources[stale_answer].replace(
            "INVARIANT StaleAcceleratorNeverAnswers", "INVARIANT IncompleteAcceleratorNeverAnswers", 1
        ),
        "mutant config bound to another invariant",
        f"wrong mutant specification or invariant: {STEM}StaleAnswerMutant.cfg",
    )
    rejected(
        MUTANT_MODEL,
        sources[MUTANT_MODEL].replace("\nSemanticPollution ==", "\nSemanticPollutionAction ==", 1),
        "undefined mutant action",
        f"mutant action or specification is missing: {STEM}SemanticPollutionMutant.cfg",
    )
    rejected(
        CONFIG,
        sources[CONFIG],
        "dropped mutant config",
        "missing or extra mutant config",
        configs[1:],
    )
    rejected(
        CONFIG,
        sources[CONFIG],
        "unregistered mutant config",
        "missing or extra mutant config",
        [*configs, f"{STEM}ShadowMutant.cfg"],
    )
    rejected(
        HOT_STORE,
        sources[HOT_STORE].replace(
            ticket_anchor,
            "            if expected.address.version.is_none() || expected.address.version.ne(&current_version) {\n",
            1,
        ),
        "stale source-mutant anchor",
        f"RAC-RS-23 source anchor is absent or ambiguous in {HOT_STORE}",
    )
    rejected(
        REFERENCE,
        sources[REFERENCE].replace(reference_anchor, reference_anchor + reference_anchor, 1),
        "duplicated source-mutant anchor",
        f"RAC-RS-01 source anchor is absent or ambiguous in {REFERENCE}",
    )
    rejected(
        GENERATOR,
        sources[GENERATOR].replace('"formal": ("RAC-MUT-02",),', '"formal": ("RAC-MUT-16",),', 1),
        "source mutant bound to a missing Rocq mutant",
        "do not correspond one-to-one",
    )
    rejected(
        GENERATOR,
        sources[GENERATOR][:dropped] + sources[GENERATOR][retained:],
        "dropped source mutant",
        "source mutants are not RAC-RS-01 to RAC-RS-25 in order",
    )
    rejected(
        GENERATOR,
        sources[GENERATOR].replace('"mode": PRODUCTION,', '"mode": HARNESS,', 1),
        "production control classified as a harness control",
        "RAC-RS-17 is not classified executable_refinement",
    )
    rejected(
        GENERATOR,
        sources[GENERATOR].replace('"source": RSPACE,', '"source": REFERENCE,', 1),
        "production control mutating the oracle",
        "RAC-RS-24 mutates an unregistered source",
    )
    rejected(
        GENERATOR,
        sources[GENERATOR].replace(
            '"test": "same_count_group_replacement_never_reuses_a_stale_automaton",',
            '"test": "catalog_occurrence_and_automaton_publish_at_one_revision",',
            1,
        ),
        "production control killed by a non-owner",
        "RAC-RS-24 is not killed by an owner of RAC-RC-24",
    )
    require(extract(sources, configs) == baseline, "nondeterministic receive accelerator registry")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    sources = {path: (ROOT / path).read_text() for path in (*paths(), *isolated_paths())}
    configs = sorted(path.name for path in (ROOT / TLA_ROOT).glob(f"{STEM}*Mutant.cfg"))
    inventory = extract(sources, configs)
    rendered = json.dumps(inventory, indent=2, sort_keys=True) + "\n"
    if args.write:
        (ROOT / OUTPUT).write_text(rendered)
    elif args.check:
        require(
            (ROOT / OUTPUT).read_text() == rendered,
            "generated receive accelerator inventory is stale",
        )
    else:
        self_test(sources, configs, inventory)
    print(
        f"receive accelerator registry {'mutations passed' if args.self_test else 'current'}: "
        f"{len(THEOREMS)} capstones, {len(MUTANT_EXAMPLES)} Rocq mutants, "
        f"{len(CHANNEL_SCOPE_CAPSTONES)} channel-scope capstones, "
        f"{len(CHANNEL_SCOPE_REFUTATIONS)} channel-scope refutations, "
        f"{len(INVARIANTS)} TLA+ invariants, {len(TLA_MUTANTS)} TLA+ mutants, "
        f"{len(inventory['source_mutants']['obligations'])} source mutants, "
        f"{sum(SUITE_COUNTS.values())} executable tests"
    )


if __name__ == "__main__":
    try:
        main()
    except (RegistryError, KeyError, json.JSONDecodeError, SyntaxError, ValueError) as error:
        raise SystemExit(f"receive accelerator registry: {error}") from error
