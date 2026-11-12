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
        "plans_from_one_base_both_publish_without_partial_visibility",
    ),
    (
        "RR-06",
        "GenerationAdvancesExactlyOnActivation",
        "no_op_writes_nothing_and_does_not_advance_generation",
    ),
    (
        "RR-07",
        "SkipResetIsRejectedWithoutAuthorityMutation",
        "target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure",
    ),
    (
        "RR-08",
        "ReplayCommitRequiresReactivation",
        "target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure",
    ),
    (
        "RR-09",
        "ReplayPreparationUsesFreshExactBase",
        "target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure",
    ),
    (
        "RR-10",
        "ReplayPreparationLeavesAuthorityUnchanged",
        "target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure",
    ),
    (
        "RR-11",
        "ReplayTargetMatchesPlay",
        "epathmap_heavy_record_then_replay_reproduces_the_log",
    ),
    (
        "RR-12",
        "ReplayReusesAuthenticatedPlayClosure",
        "target_equal_replay_publishes_from_its_recorded_base_and_reuses_exact_object_closure",
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

const PT_BINDINGS: [Binding; 9] = [
    (
        "PT-01",
        "arbitrary_checkpoint_descriptor_bytes_are_total",
        "arbitrary_checkpoint_descriptor_bytes_are_total",
    ),
    (
        "PT-02",
        "arbitrary_generation_prepare_preflight_bytes_are_total",
        "arbitrary_generation_prepare_preflight_bytes_are_total",
    ),
    (
        "PT-03",
        "arbitrary_generation_prepare_bytes_are_total",
        "arbitrary_generation_prepare_bytes_are_total",
    ),
    (
        "PT-04",
        "arbitrary_generation_commit_bytes_are_total",
        "arbitrary_generation_commit_bytes_are_total",
    ),
    (
        "PT-05",
        "arbitrary_object_index_bytes_are_total",
        "arbitrary_object_index_bytes_are_total",
    ),
    (
        "PT-06",
        "arbitrary_pack_header_bytes_are_total",
        "arbitrary_pack_header_bytes_are_total",
    ),
    (
        "PT-07",
        "arbitrary_retention_manifest_bytes_are_total",
        "arbitrary_retention_manifest_bytes_are_total",
    ),
    (
        "PT-08",
        "arbitrary_exact_catalog_images_are_total",
        "arbitrary_exact_catalog_images_are_total",
    ),
    (
        "PT-09",
        "arbitrary_checkpoint_wire_bytes_are_total",
        "arbitrary_checkpoint_wire_bytes_are_total",
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

fn verify_executable_only_bindings(bindings: &[Binding], sources: &str, catalog: &str) {
    let ids = bindings
        .iter()
        .map(|(id, _, _)| *id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids.len(),
        bindings.len(),
        "executable-only IDs must be unique"
    );

    let targets = bindings
        .iter()
        .map(|(_, _, target)| *target)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        targets.len(),
        bindings.len(),
        "executable-only parser targets must be unique",
    );

    for (id, symbol, target) in bindings {
        assert_eq!(
            catalog.matches(&format!("| {id} |")).count(),
            1,
            "catalog row {id} must exist exactly once",
        );
        assert!(
            catalog.contains(&format!("`{symbol}`")),
            "catalog row {id} must cite executable property {symbol}",
        );
        let marker = format!("fn {target}(");
        let offset = sources
            .find(&marker)
            .unwrap_or_else(|| panic!("parser row {id} names missing target {target}"));
        let prefix = &sources[..offset];
        let attribute = prefix
            .rfind("#[test]")
            .unwrap_or_else(|| panic!("parser row {id} target {target} is not a test"));
        assert!(
            !prefix[attribute..].contains("#[ignore]"),
            "parser row {id} target {target} is ignored",
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

    verify_executable_only_bindings(
        &PT_BINDINGS,
        include_str!("../../rspace++/tests/persistence/generation_checkpoint_properties.rs"),
        catalog,
    );
}
