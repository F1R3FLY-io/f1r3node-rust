// Slashing-subsystem test suite.
//
// This module hosts the test catalogue prescribed by
// docs/theory/slashing/design/14-test-plan.md:
//   • 54 example-based use-case tests (uc_<NN>_*.rs)
//   • 27 property-based theorem tests (prop_t_*.rs)
//   •  1 cross-implementation bisimilarity test (uc_39_*.rs) running
//      every harness operation against the hand-translated Rocq
//      oracle in `oracle.rs`
//
// Pre-fix regression coverage is provided out-of-band: the bug-fix
// commits land sequentially, so reverting to the parent commit and
// re-running the post-fix UC tests reproduces the bug. No Cargo
// feature gating is used.
//
// Submodules:
//   • `types`       — local DagState / PoSState / Status enums
//   • `harness`     — SlashingTestHarness state-machine API (spec §14.2.1)
//   • `generators`  — proptest strategies (spec §14.2.2)         [pending]
//   • `oracle`      — Rust mirror of the Rocq definitions (§14.2.3) [pending]
//
// Per-bug regression tests and per-UC example tests are added as
// `pre_fix_bug_<N>.rs` / `uc_<NN>_*.rs` siblings as each lands.

mod generators;
mod harness;
mod integration_helpers;
mod integration_t_admissible_equivocation;
mod integration_t_contains_future_deploy;
mod integration_t_ignorable_equivocation;
mod integration_t_invalid_block_hash_records;
mod integration_t_invalid_block_number;
mod integration_t_invalid_bonds_cache;
mod integration_t_invalid_follows;
mod integration_t_invalid_parents;
mod integration_t_invalid_repeat_deploy;
mod integration_t_invalid_sequence_number;
mod integration_t_invalid_shard_id;
mod integration_t_invalid_transaction;
mod integration_t_neglected_invalid_block;
mod integration_t_valid_no_record;
mod loom_t_9_2_atomic_record;
mod loom_t_9_2_n_threads_3;
mod loom_t_9_2_n_threads_4;
mod observer;
mod oracle;
mod oracle_adapter;
mod production_adapter;
mod tla_projection;
mod tla_trace_replay;
mod triple_bisim_driver;
mod types;

mod pre_fix_bug_1;
mod pre_fix_bug_2;
mod pre_fix_bug_3;
mod pre_fix_bug_4;
mod pre_fix_bug_5;
mod pre_fix_bug_6;
mod pre_fix_bug_7;
mod pre_fix_bug_8;
mod pre_fix_bug_9;
mod prop_t_1_detection_sound;
mod prop_t_11_neglect_closure;
mod prop_t_12_quorum_preservation;
mod prop_t_13a_bonds_bisim;
mod prop_t_13b_records_bisim;
mod prop_t_13c_forkchoice_bisim;
mod prop_t_14_weak_barbed_equiv;
mod prop_t_15_bisim_under_workload;
mod prop_t_2_detection_complete;
mod prop_t_3_slashable_taxonomy;
mod prop_t_4_record_uniqueness;
mod prop_t_5_record_monotonicity;
mod prop_t_6_neglect_detection;
mod prop_t_7_slash_zeros_bond;
mod prop_t_9_1_ignorable_safety;
mod prop_t_9_3_catchall_records;
mod prop_t_9_4_transfer_failure;
mod prop_t_9_5_active_has_positive_bond;
mod prop_t_9_6_self_regression;
mod prop_t_9_7_seqnum_density;
mod prop_t_9_8_unbonded_proposer;
mod prop_t_9_10_withdraw_safety;
mod prop_t_9_9_self_correcting;
mod prop_t_auth_check;
mod prop_t_idem_slash_idempotence;
mod prop_t_invariants_under_workload;
mod prop_t_triple_bisim_dispatch;
mod prop_t_triple_bisim_forkchoice;
mod prop_t_triple_bisim_records;
mod uc_01_admissible_single;
mod uc_02_concurrent_admissible;
mod uc_03_ignorable_unrequested;
mod uc_04_multiple_equivocations;
mod uc_05_neglect_at_genesis;
mod uc_06_self_regression;
mod uc_07_invalid_repeat_deploy;
mod uc_08_deploy_not_signed;
mod uc_09_contains_time_expired_deploy;
mod uc_10_invalid_format_admitted;
mod uc_11_stake_zero_protocol_unreachable;
mod uc_12_tracker_race;
mod uc_13_replay_determinism;
mod uc_14_replay_after_crash;
mod uc_15_neglect_two_level;
mod uc_16_neglect_chain;
mod uc_17_forkchoice_mixed;
mod uc_18_simultaneous_independent_equivocations;
mod uc_19_two_level_bond_zero;
mod uc_20_slash_during_propose;
mod uc_21_auth_token_check;
mod uc_22_unbonded_proposer;
mod uc_23_self_correcting;
mod uc_24_slash_idempotence_trace;
mod uc_25_slash_idempotent;
mod uc_26_quorum_drop;
mod uc_27_neglected_invalid_block;
mod uc_37_self_regression_dag_level;
mod uc_38_neglected_detection;
mod uc_39_cross_impl_bisim;
mod uc_40_vault_accounting_failure;
mod uc_41_ignorable_pre_fix_alias;
mod uc_42_dispatcher_pre_fix_alias;
mod uc_43_seqnum_pre_fix_alias;
mod uc_44_operational_halt;
mod uc_45_slash_replay_attack;
mod uc_46_partition_merge_equivocations;
mod uc_47_48_validator_set_changes;
mod uc_49_genesis_edge_cases;
mod uc_50_multi_slash_in_one_block;
mod uc_51_53_dag_topologies;
mod uc_54_record_invariants;

// UC-55 through UC-72: Sage-finding-derived tests. Per
// formal/sage/slashing/FINDINGS.md and design §14.3.3 the canonical
// paths are short (no `uc_NN_` prefix), matching the spec's §12
// table.
mod bounded_arithmetic_projection;
mod closure_fixed_point_certificate;
mod disconnected_neglect_cycle;
mod divergence_class;
mod duplicate_neglect_edges;
mod epoch_evidence_rollover;
mod evidence_view_divergence;
mod hypothesis_adversarial_scheduler;
mod hypothesis_arithmetic_projection_stress;
mod hypothesis_assumption_minimization;
mod hypothesis_assumption_weakening;
mod hypothesis_bundle_evidence_state_machine;
mod hypothesis_feature_combination_coverage;
mod hypothesis_liveness_as_safety;
mod hypothesis_multi_epoch_state_machine;
mod hypothesis_rust_differential_corpus;
mod metamorphic_graph_record_frontier;
mod partial_batch_failure_atomicity;
mod projection_risk_regressions;
mod quorum_intersection_after_slash;
mod rebonded_identity_boundary;
mod record_normalization;
mod report_time_closure_shrinkage;
mod semantic_attack_campaign_classification;
mod theorem_assumption_counterexamples;
mod weighted_amplification_boundary;
mod evidence_visibility_gap;
mod stale_evidence_filtered;
mod weighted_neglect_chain;
mod zero_stake_direct_offender;
