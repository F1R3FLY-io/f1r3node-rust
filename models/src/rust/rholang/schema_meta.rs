//! # `schema_meta` — what the schema IS, as data the program can read
//!
//! The hand-written half of the schema-meta table. Its generated twin —
//! `SCHEMA_CHILDREN`, `SCHEMA_SCC`, `RECURSIVE_TYPES`,
//! `DERIVE_DISPOSITION_REGISTRY`, `HAND_WRITTEN_TRAVERSALS` and
//! `DISPOSITIONED_DERIVES` — is emitted by `models/codegen/schema.rs` into
//! `OUT_DIR/rhoapi_schema_meta.rs` and included by
//! [`crate::rust::rholang::schema_meta_tables`].
//!
//! ---
//!
//! ## Why the schema's own shape is a first-class artifact
//!
//! Every driver this campaign writes replaces a recursive walk. Two questions
//! decide whether such a driver is correct and whether it is complete:
//!
//! 1. **Which types can contain themselves?** That is the set a walk can be
//!    unbounded over, and it is [`SCHEMA_SCC`] / `RECURSIVE_TYPES` — Tarjan's
//!    decomposition of the child relation, computed from the SAME resolved
//!    fields the wire tables are generated from. Recomputing it from the
//!    `.proto` by hand would be a second reading, and a second reading of one
//!    truth does not stay equal to the first: the Θ(depth) audit's converted and
//!    tripwired sets existed twice, and both prose copies went stale *within the
//!    hour* of being reconciled, twice.
//!
//! 2. **Which walks are there at all?** That is
//!    [`DERIVE_DISPOSITION_REGISTRY`], and it is the question this module exists
//!    to stop anybody answering from memory.
//!
//! ## ★★ The derive-disposition registry, and the defect it replaces
//!
//! The campaign's driver list was, at one point, hand-picked: four traits
//! somebody named. **It missed `Hash` entirely**, and the enumeration that
//! replaced it additionally found `Ord`/`PartialOrd`, which nobody had named.
//! Neither `hash` nor `ord` appeared anywhere in `rholang/tests/
//! stack_depth_gate.rs` — not in `CONVERTED_DEPTH`, not in `TRIPWIRE_DEPTH`, and
//! in no `assert_slope_below` call. Their absence meant UNMEASURED, and absence
//! reads exactly like flatness from outside.
//!
//! So the list is **derived**: `models/codegen/schema.rs` holds a closed
//! table mapping every `#[derive]` token that reaches `OUT_DIR/rhoapi.rs` to the
//! run-time surfaces it expands to, each with a [`Disposition`]; the generator
//! emits the cross product with the descriptor's items; and `models/build.rs`
//! **cross-checks the closed table against a textual scan of the generated
//! file**, exactly as it already cross-checks the `locally_free` rewrite. A
//! seventh trait fails the build, naming itself.
//!
//! ## ⚠ The registry is a LOWER BOUND, and says so
//!
//! `models/build.rs` STRIPS `PartialEq`, `Eq` and `Hash` from prost's output and
//! `models/src/lib.rs` writes them **by hand** — `<Par as PartialEq>::eq`
//! deliberately ignores `locally_free`, which no derive would do. They are
//! recursive walks all the same, and no `#[derive]` scan can see them.
//! `HAND_WRITTEN_TRAVERSALS` carries them, and the implicit `Drop` glue with
//! them.
//!
//! ★ That distinction is the whole method: the registry is complete *for what it
//! covers*, and the boundary of what it covers is stated rather than left for a
//! reader to discover by being wrong.

/// What the four-quadrant campaign has decided about one run-time surface of one
/// `#[derive]`.
///
/// ⚠ There is no "unknown" variant. A trait token that reaches
/// `OUT_DIR/rhoapi.rs` without a row in the generator's closed
/// `DERIVE_DISPOSITIONS` table **fails the build**; it cannot be carried as an
/// unclassified entry that nobody notices.
///
/// The payload string is never decorative — it is the part a reader needs and a
/// bare enum would not carry:
///
/// * [`Disposition::NotATraversal`] says **why** it does not recurse, because
///   "it does not recurse" is a claim about code;
/// * [`Disposition::Converted`] names the **driver** that replaced it, so the
///   claim is checkable;
/// * [`Disposition::FollowsFrom`] names the driver that subsumes it, so
///   "no driver of its own" is an argument rather than an omission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Not a walk over term structure at all — a marker trait, a type-level
    /// artifact, or a constructor that fills defaults without descending.
    NotATraversal(&'static str),
    /// A recursive walk that an explicit-worklist driver has already replaced.
    Converted(&'static str),
    /// A recursive walk that needs no driver of its own because a driver listed
    /// elsewhere subsumes it.
    FollowsFrom(&'static str),
}

impl Disposition {
    /// Whether this surface is a recursive walk over the term — i.e. whether it
    /// is in scope for the campaign at all.
    #[inline]
    pub const fn is_traversal(self) -> bool { !matches!(self, Disposition::NotATraversal(_)) }

    /// The payload — the driver or the reason.
    #[inline]
    pub const fn detail(self) -> &'static str {
        match self {
            Disposition::NotATraversal(s)
            | Disposition::Converted(s)
            | Disposition::FollowsFrom(s) => s,
        }
    }
}

/// A mechanically checked bridge from one concrete Rust traversal surface to
/// its parametric Rocq theorem and executable semantic oracle.
///
/// The proof establishes the post-order PDA/recursive-fold equivalence for all
/// finite trees. The executable artifact instantiates that theorem at the
/// generated rhoapi schema and pins format-specific details (bytes, ordering,
/// ignored metadata, error extent, or teardown state) that are intentionally
/// outside the generic tree algebra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EquivalenceEvidence {
    pub surface: &'static str,
    pub proof_file: &'static str,
    pub theorem: &'static str,
    pub executable_file: &'static str,
    pub executable_marker: &'static str,
}

/// Formal and executable binding for a hand-written PDA outside the generated
/// rhoapi schema. `production_file` and `production_marker` prevent the generic
/// theorem and a passing oracle from being cited for an implementation that is
/// no longer present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryEquivalenceEvidence {
    pub surface: &'static str,
    pub production_file: &'static str,
    pub production_marker: &'static str,
    pub proof_file: &'static str,
    pub theorem: &'static str,
    pub executable_file: &'static str,
    pub executable_marker: &'static str,
}

const PDA_PROOF: &str = "formal/rocq/stack_safe_pda/theories/StackSafePDA.v";

/// Complete evidence map for every generated or hand-written recursive
/// traversal in [`DERIVE_DISPOSITION_REGISTRY`] and
/// [`HAND_WRITTEN_TRAVERSALS`].
///
/// `models/tests/formal_equivalence_manifest.rs` compares this table with the
/// generated registries in both directions. Adding a recursive surface without
/// adding proof-and-oracle evidence therefore fails the test by surface name.
pub static PDA_EQUIVALENCE_EVIDENCE: &[EquivalenceEvidence] = &[
    EquivalenceEvidence {
        surface: "Serialize::serialize",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/tests/bincode_encoder_differential.rs",
        executable_marker: "every_root_and_shape_satisfies_all_four_properties",
    },
    EquivalenceEvidence {
        surface: "Deserialize::deserialize",
        proof_file: PDA_PROOF,
        theorem: "decoder_machine_equivalent_to_recursive_rebuild",
        executable_file: "models/tests/bincode_decoder_differential.rs",
        executable_marker: "par_corpus_agrees_with_the_derived_oracle",
    },
    EquivalenceEvidence {
        surface: "Clone::clone",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/tests/clone_equivalence_corpus.rs",
        executable_marker: "every_enumerated_shape_clones_identically_on_every_axis",
    },
    EquivalenceEvidence {
        surface: "Ord::cmp",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/codegen/schema.rs",
        executable_marker: "generated_ord_matches_the_recursive_oracle",
    },
    EquivalenceEvidence {
        surface: "PartialOrd::partial_cmp",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/codegen/schema.rs",
        executable_marker: "generated_ord_matches_the_recursive_oracle",
    },
    EquivalenceEvidence {
        surface: "Debug::fmt",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/codegen/schema.rs",
        executable_marker: "generated_debug_matches_the_recursive_builder_oracle",
    },
    EquivalenceEvidence {
        surface: "Message::encode_raw",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/tests/protobuf_encoder_differential.rs",
        executable_marker: "bounded_recursive_oracle_terms_encode_identically",
    },
    EquivalenceEvidence {
        surface: "Message::encoded_len",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/tests/protobuf_encoder_differential.rs",
        executable_marker: "the_length_table_holds_exactly_one_entry_per_message_node",
    },
    EquivalenceEvidence {
        surface: "Message::merge_field",
        proof_file: PDA_PROOF,
        theorem: "decoder_machine_equivalent_to_recursive_rebuild",
        executable_file: "models/tests/protobuf_decoder_differential.rs",
        executable_marker: "exhaustive_valid_corpora_decode_identically",
    },
    EquivalenceEvidence {
        surface: "Message::clear",
        proof_file: PDA_PROOF,
        theorem: "drop_machine_reaches_one_completion",
        executable_file: "models/tests/par_protobuf_stack_safety.rs",
        executable_marker: "message_clear_matches_prost_default_semantics",
    },
    EquivalenceEvidence {
        surface: "Oneof::encode",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/tests/protobuf_encoder_differential.rs",
        executable_marker: "every_expr_instance_encodes_identically",
    },
    EquivalenceEvidence {
        surface: "Oneof::encoded_len",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/tests/protobuf_encoder_differential.rs",
        executable_marker: "every_expr_instance_encodes_identically",
    },
    EquivalenceEvidence {
        surface: "Oneof::merge",
        proof_file: PDA_PROOF,
        theorem: "decoder_machine_equivalent_to_recursive_rebuild",
        executable_file: "models/tests/protobuf_decoder_differential.rs",
        executable_marker: "exhaustive_valid_corpora_decode_identically",
    },
    EquivalenceEvidence {
        surface: "PartialEq::eq",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/codegen/schema.rs",
        executable_marker: "generated_eq_matches_the_recursive_oracle",
    },
    EquivalenceEvidence {
        surface: "Hash::hash",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "models/codegen/schema.rs",
        executable_marker: "generated_hash_matches_the_recursive_oracle",
    },
    EquivalenceEvidence {
        surface: "Drop::drop",
        proof_file: PDA_PROOF,
        theorem: "drop_machine_reaches_one_completion",
        executable_file: "models/tests/par_protobuf_stack_safety.rs",
        executable_marker: "message_clear_tears_down_a_deep_term_iteratively",
    },
];

/// Repository-boundary PDAs that are not generated rhoapi traversals. The
/// formal manifest checks this table against an independent closed surface set,
/// resolves every production marker, theorem, and executable marker, and
/// rejects duplicate rows.
pub static BOUNDARY_PDA_EQUIVALENCE_EVIDENCE: &[BoundaryEquivalenceEvidence] = &[
    BoundaryEquivalenceEvidence {
        surface: "node::Par-to-RhoExpr conversion",
        production_file: "node/src/rust/api/rho_expr_pda.rs",
        production_marker: "pub(super) fn from_par(par: Par) -> Option<RhoExpr>",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "node/tests/support/rho_expr_pda_tests.rs",
        executable_marker: "conversion_pda_matches_recursive_oracle_for_every_expr_variant",
    },
    BoundaryEquivalenceEvidence {
        surface: "node::EPathMap-to-RhoExpr mode conversion",
        production_file: "node/src/rust/api/rho_expr_pda.rs",
        production_marker: "Work::PathMap(pathmap) => match pathmap.mode()",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "node/tests/support/rho_expr_pda_tests.rs",
        executable_marker: "conversion_pda_matches_recursive_oracle_for_every_epathmap_mode",
    },
    BoundaryEquivalenceEvidence {
        surface: "node::RhoExpr::Clone",
        production_file: "node/src/rust/api/rho_expr_pda.rs",
        production_marker: "impl Clone for RhoExpr",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "node/tests/support/rho_expr_pda_tests.rs",
        executable_marker: "stack_safe_traits_preserve_derived_json_shapes",
    },
    BoundaryEquivalenceEvidence {
        surface: "node::RhoExpr::Serialize",
        production_file: "node/src/rust/api/rho_expr_pda.rs",
        production_marker: "impl serde::Serialize for RhoExpr",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "node/tests/support/rho_expr_pda_tests.rs",
        executable_marker: "stack_safe_traits_preserve_derived_json_shapes",
    },
    BoundaryEquivalenceEvidence {
        surface: "node::RhoExpr::Debug",
        production_file: "node/src/rust/api/rho_expr_pda.rs",
        production_marker: "impl std::fmt::Debug for RhoExpr",
        proof_file: PDA_PROOF,
        theorem: "pda_fold_equivalent_to_recursive_fold",
        executable_file: "node/tests/support/rho_expr_pda_tests.rs",
        executable_marker: "stack_safe_traits_preserve_derived_json_shapes",
    },
    BoundaryEquivalenceEvidence {
        surface: "node::RhoExpr::Drop",
        production_file: "node/src/rust/api/rho_expr_pda.rs",
        production_marker: "impl Drop for RhoExpr",
        proof_file: PDA_PROOF,
        theorem: "drop_machine_reaches_one_completion",
        executable_file: "node/tests/support/rho_expr_pda_tests.rs",
        executable_marker: "deep_conversion_clone_json_debug_and_drop_fit_small_stack",
    },
    BoundaryEquivalenceEvidence {
        surface: "rholang::SpatialMatcher heterogeneous PDA",
        production_file: "rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs",
        production_marker: "fn drive(context: &mut SpatialMatcherContext, root: MatchPair)",
        proof_file: "formal/rocq/stack_safe_pda/theories/SpatialMatcher.v",
        theorem: "spatial_match_pda_equivalent_to_recursive_match",
        executable_file: "rholang/tests/support/spatial_matcher_pda_equivalence.rs",
        executable_marker: "recursive_oracle_and_pda_agree_on_the_semantic_corpus",
    },
    BoundaryEquivalenceEvidence {
        surface: "rholang::SpatialMatcher state isolation",
        production_file: "rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs",
        production_marker: "Frame::RestoreOnFailure { snapshot }",
        proof_file: "formal/rocq/stack_safe_pda/theories/SpatialMatcher.v",
        theorem: "failed_disjunct_retries_from_the_original_snapshot",
        executable_file: "rholang/tests/support/spatial_matcher_pda_equivalence.rs",
        executable_marker: "recursive_oracle_and_pda_agree_on_the_semantic_corpus",
    },
    BoundaryEquivalenceEvidence {
        surface: "rholang::EPathMap singleton owned-zipper matching",
        production_file: "rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs",
        production_marker: "fn init_single_path(",
        proof_file: "formal/rocq/stack_safe_pda/theories/SpatialMatcher.v",
        theorem: "owned_singleton_path_move_preserves_match_semantics",
        executable_file: "rholang/tests/epathmap_spatial_match.rs",
        executable_marker: "map_match_subtracts_exact_pairs_and_binds_key_and_value",
    },
];

/// A concrete EPathMap law proved in Rocq and exercised against the PathMap
/// implementation. These rows cover neutral mode selection, specialized set
/// and value-map algebra, mixed-mode rejection, prefix restriction, and the
/// framed EPM1 topology/value-table contract.
pub static EPATHMAP_FORMAL_EVIDENCE: &[EquivalenceEvidence] = &[
    EquivalenceEvidence {
        surface: "EPathMap::empty/join mode",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "neutral_empty_is_a_two_sided_identity",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "empty_is_mode_neutral_for_the_partial_algebra",
    },
    EquivalenceEvidence {
        surface: "EPathMap::empty/meet/restrict mode",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "neutral_empty_is_meet_and_restrict_absorbing",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "empty_is_mode_neutral_for_the_partial_algebra",
    },
    EquivalenceEvidence {
        surface: "EPathMap::empty/subtract mode",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "neutral_empty_is_subtract_left_zero_and_right_identity",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "empty_is_mode_neutral_for_the_partial_algebra",
    },
    EquivalenceEvidence {
        surface: "EPathMap::mixed modes",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "every_binary_operation_rejects_mixed_nonempty_modes",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "nonempty_set_and_map_modes_cannot_mix",
    },
    EquivalenceEvidence {
        surface: "EPathMap::set join lattice",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "set_join_associative",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "set_algebra_uses_pathmap_lattice_operations",
    },
    EquivalenceEvidence {
        surface: "EPathMap::set meet lattice",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "set_meet_associative",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "set_algebra_uses_pathmap_lattice_operations",
    },
    EquivalenceEvidence {
        surface: "EPathMap::set subtract",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "set_subtract_self_is_empty",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "set_algebra_uses_pathmap_lattice_operations",
    },
    EquivalenceEvidence {
        surface: "EPathMap::map join",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "map_join_commutative",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "map_algebra_preserves_associations_and_rejects_unequal_overlaps",
    },
    EquivalenceEvidence {
        surface: "EPathMap::map meet",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "map_meet_commutative",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "map_algebra_preserves_associations_and_rejects_unequal_overlaps",
    },
    EquivalenceEvidence {
        surface: "EPathMap::map subtract key mask",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "subtract_overlap_is_value_independent_key_mask",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "map_algebra_preserves_associations_and_rejects_unequal_overlaps",
    },
    EquivalenceEvidence {
        surface: "EPathMap::member-prefix restriction",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPathMap.v",
        theorem: "prefix_restriction_never_invents_members",
        executable_file: "models/tests/epathmap_algebra.rs",
        executable_marker: "member_prefix_restriction_is_specialized_for_both_modes",
    },
    EquivalenceEvidence {
        surface: "EPathMap::EPM1 framed round trip",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPM1.v",
        theorem: "epm1_framed_decode_encode_identity",
        executable_file: "models/tests/epathmap_epm1_snapshot.rs",
        executable_marker: "bincode_copies_the_same_epm1_snapshot_and_both_readers_agree",
    },
    EquivalenceEvidence {
        surface: "EPathMap::EPM1 topology and ordered values",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPM1.v",
        theorem: "epm1_preserves_topology_and_ordered_value_table",
        executable_file: "models/tests/epathmap_epm1_snapshot.rs",
        executable_marker: "value_bearing_map_is_a_lossless_nested_canonical_path_segment",
    },
    EquivalenceEvidence {
        surface: "EPathMap::EPM1 topology/value ordinal association",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPM1.v",
        theorem: "epm1_round_trip_preserves_topology_value_association",
        executable_file: "models/tests/epathmap_epm1_snapshot.rs",
        executable_marker:
            "live_store_preserves_map_mode_and_key_value_associations_on_both_surfaces",
    },
    EquivalenceEvidence {
        surface: "EPathMap::EPM1 ordinal uniqueness and range",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPM1.v",
        theorem: "associated_map_ordinals_are_in_range_and_unique",
        executable_file: "models/tests/epathmap_epm1_snapshot.rs",
        executable_marker:
            "map_snapshot_preserves_values_across_compact_line_branch_and_dense_shapes",
    },
    EquivalenceEvidence {
        surface: "EPathMap::EPM1 generated-PDA value equivalence",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPM1.v",
        theorem: "epm1_generated_pda_values_equal_recursive_encoding",
        executable_file: "models/tests/epathmap_epm1_snapshot.rs",
        executable_marker: "nested_map_value_snapshots_stream_on_a_256_kib_stack",
    },
    EquivalenceEvidence {
        surface: "EPathMap::EPM1 malformed canonical varint rejection",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPM1.v",
        theorem: "canonical_varint_rejects_redundant_continuation",
        executable_file: "models/src/rust/epathmap_trie_codec.rs",
        executable_marker: "decoder_rejects_noncanonical_and_malformed_snapshots",
    },
    EquivalenceEvidence {
        surface: "EPathMap::EPM1 truncated frame rejection",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPM1.v",
        theorem: "length_frame_rejects_truncated_payload",
        executable_file: "models/src/rust/epathmap_trie_codec.rs",
        executable_marker: "decoder_rejects_noncanonical_and_malformed_snapshots",
    },
    EquivalenceEvidence {
        surface: "EPathMap::EPM1 trailing byte rejection",
        proof_file: "formal/rocq/stack_safe_pda/theories/EPM1.v",
        theorem: "epm1_rejects_trailing_bytes",
        executable_file: "models/src/rust/epathmap_trie_codec.rs",
        executable_marker: "decoder_rejects_noncanonical_and_malformed_snapshots",
    },
];
