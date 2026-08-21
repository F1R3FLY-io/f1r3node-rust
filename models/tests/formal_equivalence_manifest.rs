//! Mechanical closure gate for the generated PDA equivalence argument.
//!
//! The schema generator owns the complete traversal registry. The hand-written
//! evidence table owns the theorem and executable-oracle binding. This test
//! compares those independent inventories in both directions and then verifies
//! that every named file, theorem, and oracle marker exists.

use std::collections::{BTreeMap, BTreeSet};
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

fn read_repository_file(relative: &str) -> String {
    let path = repository_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
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
        "models::RMP1 generated cursor encoder",
        "rholang::RMP1 ordered query compiler",
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
        "formal/rocq/stack_safe_pda/theories/SpatialMatcher.v",
        "formal/rocq/stack_safe_pda/theories/FreeMapJournal.v",
    ] {
        let source = read_repository_file(relative);
        for forbidden in ["Admitted.", "admit.", "Axiom "] {
            assert!(
                !source.contains(forbidden),
                "{relative} contains an unproved declaration marker `{forbidden}`"
            );
        }
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
