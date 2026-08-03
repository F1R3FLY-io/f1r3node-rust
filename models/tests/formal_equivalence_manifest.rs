//! Mechanical closure gate for the generated PDA equivalence argument.
//!
//! The schema generator owns the complete traversal registry. The hand-written
//! evidence table owns the theorem and executable-oracle binding. This test
//! compares those independent inventories in both directions and then verifies
//! that every named file, theorem, and oracle marker exists.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use models::rust::rholang::schema_meta::{
    BOUNDARY_PDA_EQUIVALENCE_EVIDENCE, BoundaryEquivalenceEvidence, Disposition,
    EPATHMAP_FORMAL_EVIDENCE, EquivalenceEvidence, PDA_EQUIVALENCE_EVIDENCE,
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
