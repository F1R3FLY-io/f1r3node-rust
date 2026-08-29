use std::collections::BTreeSet;

type Binding = (&'static str, &'static str, &'static str);

const REPLAY_BINDINGS: [Binding; 13] = [
    (
        "RR-01",
        "TypeInvariant",
        "memory_checkpoint_publishes_all_namespaces_together",
    ),
    (
        "RR-02",
        "CurrentRootIsVerified",
        "memory_checkpoint_publishes_all_namespaces_together",
    ),
    (
        "RR-03",
        "VerifiedRootsHaveCompleteClosure",
        "memory_checkpoint_publishes_all_namespaces_together",
    ),
    (
        "RR-04",
        "ContentKeysAreImmutable",
        "immutable_collision_leaves_exact_pre_state",
    ),
    (
        "RR-05",
        "NoUnpublishedCheckpointObjects",
        "same_generation_plans_conflict_without_partial_visibility",
    ),
    (
        "RR-06",
        "GenerationAdvancesExactlyOnActivation",
        "no_op_writes_nothing_and_does_not_advance_generation",
    ),
    (
        "RR-07",
        "SkipResetIsRejectedWithoutAuthorityMutation",
        "target_equal_replay_requires_reactivation_and_reuses_exact_object_closure",
    ),
    (
        "RR-08",
        "ReplayCommitRequiresReactivation",
        "target_equal_replay_requires_reactivation_and_reuses_exact_object_closure",
    ),
    (
        "RR-09",
        "ReplayPreparationUsesFreshExactBase",
        "target_equal_replay_requires_reactivation_and_reuses_exact_object_closure",
    ),
    (
        "RR-10",
        "ReplayPreparationLeavesAuthorityUnchanged",
        "target_equal_replay_requires_reactivation_and_reuses_exact_object_closure",
    ),
    (
        "RR-11",
        "ReplayTargetMatchesPlay",
        "epathmap_heavy_record_then_replay_reproduces_the_log",
    ),
    (
        "RR-12",
        "ReplayReusesAuthenticatedPlayClosure",
        "target_equal_replay_requires_reactivation_and_reuses_exact_object_closure",
    ),
    (
        "RR-13",
        "ReplayLifecycleEventuallyCompletes",
        "epathmap_heavy_record_then_replay_reproduces_the_log",
    ),
];

const FFI_BINDINGS: [Binding; 13] = [
    (
        "FFI-01",
        "TypeInvariant",
        "all_six_operations_satisfy_the_exhaustive_boundary_fault_matrix",
    ),
    (
        "FFI-02",
        "AtMostOneAffineCapability",
        "all_six_operations_satisfy_the_exhaustive_boundary_fault_matrix",
    ),
    (
        "FFI-03",
        "PreparedCapabilityIsPrivate",
        "begin_primary_encoding_failure_exposes_no_open_capability",
    ),
    (
        "FFI-04",
        "CapabilityOutputRequiresEncodedSuccess",
        "seal_primary_encoding_failure_exposes_no_sealed_capability",
    ),
    (
        "FFI-05",
        "FailureLeavesNoNewCapabilityOutput",
        "all_six_operations_satisfy_the_exhaustive_boundary_fault_matrix",
    ),
    (
        "FFI-06",
        "NoOrdinaryUnwindEscapesBoundary",
        "all_six_operations_satisfy_the_exhaustive_boundary_fault_matrix",
    ),
    (
        "FFI-07",
        "PublicationIsMonotoneAndUnique",
        "two_sequential_publications_are_distinct_and_each_exactly_once",
    ),
    (
        "FFI-08",
        "PublicationResponseIsTruthful",
        "publish_primary_encoding_failure_returns_null_after_authority_effect",
    ),
    (
        "FFI-09",
        "PrePublicationPanicIsStructured",
        "pre_publication_operation_panic_is_structured_and_does_not_invent_publication",
    ),
    (
        "FFI-10",
        "PublishSuccessRequiresPublication",
        "two_sequential_publications_are_distinct_and_each_exactly_once",
    ),
    (
        "FFI-11",
        "StageEncodingFailurePoisonsMutatedOpen",
        "stage_primary_encoding_failure_poisons_the_mutated_open_session",
    ),
    (
        "FFI-12",
        "AbortConsumesWithoutPublishing",
        "all_six_operations_satisfy_the_exhaustive_boundary_fault_matrix",
    ),
    (
        "FFI-13",
        "EveryStartedCallReturns",
        "all_six_operations_satisfy_the_exhaustive_boundary_fault_matrix",
    ),
];

fn configured_obligations(config: &str) -> BTreeSet<&str> {
    config
        .lines()
        .filter_map(|line| {
            line.strip_prefix("INVARIANT ")
                .or_else(|| line.strip_prefix("PROPERTY "))
        })
        .collect()
}

fn verify_bindings(config: &str, bindings: &[Binding], sources: &str, catalog: &str) {
    let configured = configured_obligations(config);
    let mapped = bindings
        .iter()
        .map(|(_, symbol, _)| *symbol)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        mapped, configured,
        "every configured formal obligation must be mapped exactly once"
    );

    let ids = bindings
        .iter()
        .map(|(id, _, _)| *id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids.len(),
        bindings.len(),
        "formal binding IDs must be unique"
    );
    assert_eq!(
        mapped.len(),
        bindings.len(),
        "formal symbols must be mapped exactly once"
    );

    for (id, symbol, target) in bindings {
        assert_eq!(
            catalog.matches(&format!("| {id} |")).count(),
            1,
            "catalog row {id} must exist exactly once",
        );
        assert!(
            catalog.contains(&format!("`{symbol}`")),
            "catalog row {id} must cite formal symbol {symbol}",
        );
        assert!(
            sources.contains(&format!("fn {target}(")),
            "formal row {id} names missing executable property {target}",
        );
    }
}

#[test]
fn every_workspace_regression_formal_obligation_has_an_executable_property() {
    let catalog = include_str!(
        "../../docs/design/pathmap/workspace-regression-formal-invariant-catalog-2026-08-28.md"
    );
    let replay_sources = [
        include_str!("../../rspace++/tests/state/atomic_state_authority.rs"),
        include_str!("../../rholang/tests/epathmap_replay_equivalence_spec.rs"),
    ]
    .concat();
    let ffi_sources =
        include_str!("../../rspace++/libs/rspace_rhotypes/tests/affine_state_import_ffi/mod.rs");

    verify_bindings(
        include_str!(
            "../../formal/tlaplus/stack_safe_pda/RSpaceRecordReplayAuthorityLifecycle.cfg"
        ),
        &REPLAY_BINDINGS,
        &replay_sources,
        catalog,
    );
    verify_bindings(
        include_str!("../../formal/tlaplus/stack_safe_pda/RSpaceStateImportFfiBoundary.cfg"),
        &FFI_BINDINGS,
        ffi_sources,
        catalog,
    );
}
