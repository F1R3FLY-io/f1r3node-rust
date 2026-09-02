use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RefinementStatus {
    Gap,
}

#[derive(Clone, Copy, Debug)]
struct RecoveryPropertyBinding {
    id: &'static str,
    capstones: &'static [&'static str],
    executable_target: &'static str,
    status: RefinementStatus,
}

const RECOVERY_PROPERTIES: &[RecoveryPropertyBinding] = &[
    RecoveryPropertyBinding {
        id: "RBA1-REC-01",
        capstones: &[
            "canonical_checkpoint_validation_recovers_exact_image",
            "exact_recovery_is_sound",
            "selected_new_implies_complete_valid_exactly_bound_successor",
            "invalid_current_rba1_never_falls_through_to_legacy",
        ],
        executable_target: "rba1_exact_validation_rejects_every_subject_mutation",
        status: RefinementStatus::Gap,
    },
    RecoveryPropertyBinding {
        id: "RBA1-REC-02",
        capstones: &[
            "constructed_rba1_object_is_exactly_inventory_bound",
            "rba1_object_construction_and_inventory_binding_are_inhabited",
        ],
        executable_target: "rba1_object_inventory_binding_is_exact",
        status: RefinementStatus::Gap,
    },
    RecoveryPropertyBinding {
        id: "RBA1-REC-03",
        capstones: &[
            "checkpoint_binding_eqb_reflects_exact_receipt",
            "selected_new_implies_complete_valid_exactly_bound_successor",
        ],
        executable_target: "rba1_physical_and_semantic_generations_bind_independently",
        status: RefinementStatus::Gap,
    },
    RecoveryPropertyBinding {
        id: "RBA1-REC-04",
        capstones: &[
            "accepted_match_binds_artifact_to_exact_successor_state_generation",
            "refused_match_cannot_publish_successor_semantic_state",
        ],
        executable_target: "rba1_match_publication_uses_exact_semantic_successor",
        status: RefinementStatus::Gap,
    },
    RecoveryPropertyBinding {
        id: "RBA1-REC-05",
        capstones: &[
            "runtime_new_authority_requires_acknowledged_exact_readmission",
            "acknowledged_exact_rba1_readmission_is_reachable_and_inhabited",
        ],
        executable_target: "rba1_authority_is_unobservable_before_exact_readmission",
        status: RefinementStatus::Gap,
    },
    RecoveryPropertyBinding {
        id: "RBA1-REC-06",
        capstones: &[
            "loaded_empty_rba1_survives_restart_exactly",
            "accepted_migration_can_become_restart_authority_only_after_validation",
            "rba1_recovery_invariant_catalog_is_duplicate_free_and_exact",
        ],
        executable_target: "rba1_restart_matrix_preserves_loaded_empty_exactly",
        status: RefinementStatus::Gap,
    },
];

const RECOVERY_MUTANTS: &[(&str, &str)] = &[
    ("RBA1-REC-M01", "exact_subject_validation_kills_same_cardinality_substitution"),
    ("RBA1-REC-M02", "exact_inventory_binding_kills_detached_object"),
    ("RBA1-REC-M03", "semantic_successor_kills_pre_match_state_generation_substitution"),
    ("RBA1-REC-M04", "restart_exactness_kills_loaded_empty_erasure"),
];

fn catalog_ids<'a>(catalog: &'a str, prefix: &str) -> BTreeSet<&'a str> {
    catalog
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
        .filter(|token| {
            token.strip_prefix(prefix).is_some_and(|index| {
                index.len() == 2 && index.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        .collect()
}

#[test]
fn rba1_recovery_registry_is_unique_exhaustive_and_explicitly_unimplemented() {
    let catalog = include_str!(
        "../../../docs/design/pathmap/rba1-checkpoint-recovery-invariant-catalog-2026-09-02.md"
    );
    let recovery = RECOVERY_PROPERTIES
        .iter()
        .map(|binding| (binding.id, binding))
        .collect::<BTreeMap<_, _>>();
    let mutants = RECOVERY_MUTANTS.iter().copied().collect::<BTreeMap<_, _>>();

    assert_eq!(recovery.len(), 6, "the formal recovery catalog has six invariants");
    assert_eq!(recovery.len(), RECOVERY_PROPERTIES.len(), "duplicate recovery ID");
    assert_eq!(mutants.len(), 4, "the formal recovery seam has four mutants");
    assert_eq!(mutants.len(), RECOVERY_MUTANTS.len(), "duplicate recovery mutant ID");
    assert_eq!(
        recovery.keys().copied().collect::<BTreeSet<_>>(),
        catalog_ids(catalog, "RBA1-REC-"),
        "documentation and recovery property IDs diverged",
    );
    assert_eq!(
        mutants.keys().copied().collect::<BTreeSet<_>>(),
        catalog_ids(catalog, "RBA1-REC-M"),
        "documentation and recovery mutant IDs diverged",
    );

    for binding in RECOVERY_PROPERTIES {
        assert_eq!(binding.status, RefinementStatus::Gap);
        assert!(catalog.contains(&format!("`{}`", binding.executable_target)));
        assert!(
            !binding.capstones.is_empty(),
            "recovery property {} has no formal derivation",
            binding.id,
        );
    }
    assert!(catalog.contains("production RBA1 codec and recovery adapter open"));
    assert!(catalog.contains("RBA1 remains a formally specified implementation target"));
}

#[test]
fn every_reported_recovery_capstone_and_mutant_is_catalogued_and_gated() {
    let catalog = include_str!(
        "../../../docs/design/pathmap/rba1-checkpoint-recovery-invariant-catalog-2026-09-02.md"
    );
    let recovery_source = include_str!(
        "../../../formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceCheckpointRecovery.v"
    );
    let mutant_source = include_str!(
        "../../../formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceCheckpointRecoveryMutants.v"
    );
    let formal_gate = include_str!("../../../scripts/check-stack-safe-pda-proofs.sh");

    for capstone in recovery_source.lines().filter_map(|line| {
        line.trim()
            .strip_prefix("Print Assumptions ")
            .map(|symbol| symbol.trim_end_matches('.'))
    }) {
        assert!(
            catalog.contains(&format!("`{capstone}`")),
            "Rocq recovery capstone {capstone} is absent from the catalog",
        );
    }
    for binding in RECOVERY_PROPERTIES {
        for capstone in binding.capstones {
            assert!(
                recovery_source.contains(&format!("Print Assumptions {capstone}.")),
                "{} cites unreported recovery capstone {capstone}",
                binding.id,
            );
        }
    }
    for (id, witness) in RECOVERY_MUTANTS {
        assert!(catalog.contains(&format!("`{witness}`")));
        assert!(
            mutant_source.contains(&format!("Print Assumptions {witness}.")),
            "{id} cites unreported mutant witness {witness}",
        );
    }
    for metatheorem in [
        "every_recovery_seam_mutant_has_a_causal_witness",
        "recovery_seam_mutant_catalog_is_unique_complete_and_exact",
    ] {
        assert!(catalog.contains(&format!("`{metatheorem}`")));
        assert!(mutant_source.contains(&format!("Print Assumptions {metatheorem}.")));
    }
    assert!(formal_gate.contains("theories/TwoLevelRSpaceCheckpointRecovery.v 14"));
    assert!(formal_gate.contains("theories/TwoLevelRSpaceCheckpointRecoveryMutants.v 6"));
    assert!(formal_gate.contains("StackSafePDA.TwoLevelRSpaceCheckpointRecovery"));
    assert!(formal_gate.contains("StackSafePDA.TwoLevelRSpaceCheckpointRecoveryMutants"));
}
