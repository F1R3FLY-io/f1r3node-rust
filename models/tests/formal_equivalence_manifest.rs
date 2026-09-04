//! Mechanical closure gate for the generated PDA equivalence argument.
//!
//! The schema generator owns the complete traversal registry. The hand-written
//! evidence table owns the theorem and executable-oracle binding. This test
//! compares those independent inventories in both directions and then verifies
//! that every named file, theorem, and oracle marker exists.

use std::collections::{BTreeMap, BTreeSet};

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

fn read_repository_file(relative: &str) -> String {
    let path = repository_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
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
        "the exact 25-primary/10-quarantine PathMap teardown authority and scoped production policies",
        "the exact 27-teardown/13-quarantine PathMap teardown controls",
    ] {
        assert!(
            runner.contains(marker),
            "PathMap teardown formal runner is missing `{marker}`"
        );
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
        "theories/ChannelImage.v",
        "channel_image_closed_capstones",
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
    let model =
        read_repository_file("formal/tlaplus/stack_safe_pda/RhoPathV2Lifecycle.tla");
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
        "NoPartialPublication ==",
    ] {
        assert!(
            model.contains(marker),
            "RhoPath V2 lifecycle TLA+ model is missing `{marker}`"
        );
    }

    let config =
        read_repository_file("formal/tlaplus/stack_safe_pda/RhoPathV2Lifecycle.cfg");
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
}

#[test]
fn rpb3_admission_formal_artifacts_are_manifested() {
    let rocq = read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3Admission.v");
    for marker in [
        "rpb3_pda_equivalent_to_recursive_validation",
        "rpb3_pda_terminates_without_call_stack_recursion",
        "Print Assumptions rpb3_pda_equivalent_to_recursive_validation",
    ] {
        assert!(rocq.contains(marker), "RPB3 Rocq theory is missing `{marker}`");
    }

    let mutants =
        read_repository_file("formal/rocq/stack_safe_pda/theories/RPB3AdmissionMutants.v");
    for marker in [
        "mutant_omit_locality_is_refuted",
        "mutant_trailing_acceptance_is_refuted",
        "Print Assumptions mutant_omit_locality_is_refuted",
    ] {
        assert!(mutants.contains(marker), "RPB3 mutant theory is missing `{marker}`");
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
        assert!(verus.contains(marker), "RPB3 Verus refinement is missing `{marker}`");
    }

    let coq_project = read_repository_file("formal/rocq/stack_safe_pda/_CoqProject");
    assert!(coq_project.contains("theories/RPB3Admission.v"));
    assert!(coq_project.contains("theories/RPB3AdmissionMutants.v"));

    let runner = read_repository_file("scripts/check-stack-safe-pda-proofs.sh");
    for marker in [
        "theories/RPB3Admission.v",
        "theories/RPB3AdmissionMutants.v",
        "rpb3_admission_closed_capstones",
        "rpb3_admission_mutants_closed_capstones",
        "generated_rpb3_table_differential_is_bound_to_admission_runner",
    ] {
        assert!(runner.contains(marker), "RPB3 formal runner is missing `{marker}`");
    }
}

#[test]
fn generated_rpb3_registry_is_contiguous_and_descriptor_aligned() {
    use models::rust::rholang::match_schema_tables::{PAR_PROTOBUF_REGION_PROGRAM, RHO_MATCH_REGISTRY};

    assert!(!RHO_MATCH_REGISTRY.is_empty(), "generated RPB3 registry is empty");
    for (index, (name, program)) in RHO_MATCH_REGISTRY.iter().enumerate() {
        let expected_type_id = u16::try_from(index + 1)
            .expect("generated RPB3 registry exceeds the u16 type-id domain");
        assert_eq!(program.type_id, expected_type_id, "{name}: registry type id drifted");
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
        assert!(program.type_id != 0, "{name}: zero is reserved for invalid type ids");
    }
    assert_eq!(
        PAR_PROTOBUF_REGION_PROGRAM.type_name,
        "Par",
        "the separate raw protobuf Par program must retain the Par descriptor identity"
    );
}
