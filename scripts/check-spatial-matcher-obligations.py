import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/spatial_matcher_obligations.generated.json"
CONFIG = "formal/tlaplus/stack_safe_pda/SpatialMatcherEquivalence.cfg"
MODEL = "formal/tlaplus/stack_safe_pda/SpatialMatcherEquivalence.tla"
ROCQ = "formal/rocq/stack_safe_pda/theories/SpatialMatcher.v"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
MODULE = "rholang/src/rust/interpreter/matcher/mod.rs"
PDA = "rholang/tests/support/spatial_matcher_pda_equivalence.rs"
ISOLATION = "rholang/tests/matcher_connective_isolation.rs"
OWNER_MODEL = "formal/verus/canonical_connective_owner_cursor.rs"
OWNER_MUTANTS = "scripts/generate-canonical-connective-owner-mutant.py"
CONNECTIVE_SOURCE = "models/src/rust/par_bag.rs"
CONNECTIVE_PROPERTY = "models/tests/par_bag.rs"
MATCHER_SOURCE = "rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs"
PROJECTION_TABLE = "rholang/src/rust/interpreter/matcher/projection_rules.rs"
PROJECTION_THEORY = "formal/rocq/stack_safe_pda/theories/SchemaProjectionCorrespondence.v"
POOL = "rspace++/tests/data_index/candidate_product_continuation.rs"
RETAINED = "rholang/tests/matcher/retained_path_index_test.rs"
OWNER_INVARIANTS = [
    (
        "mismatched_owner_refused_before_position_transfer",
        [
            "first_children.bind_owned(second_children.message())",
            "first_children.bind_owned(first_children.message().to_vec())",
            "first_children.bind_owned(truncated)",
            "exhausted.bind_owned(second_children.message())",
        ],
    ),
    (
        "exact_owner_preserves_validated_position",
        [
            "let consumed = split % (expected.len() + 1);",
            "proptest::prop_assert_eq!(partial.len(), expected.len() - consumed);",
            "proptest::prop_assert_eq!(retained.len(), expected.len() - consumed);",
        ],
    ),
    (
        "next_returns_exact_validated_region",
        [
            "proptest::prop_assert_eq!(next_header, first_children.message().len());",
            "proptest::prop_assert_eq!(&range, expected_range);",
            "prop_assert_eq!(&owned.body()[range], *expected_child);",
            "prop_assert_eq!(&retained.body()[range], *expected_child);",
        ],
    ),
    (
        "exhaustion_iff_all_validated_children_consumed",
        [
            "prop_assert_eq!(owned.next(), None);",
            "prop_assert_eq!(retained.next(), None);",
            "prop_assert!(exhausted.is_empty());",
        ],
    ),
]
OWNER_MUTATION_NAMES = (
    "length_only",
    "pointer_only",
    "skip_child",
    "wrong_range",
    "early_exhaustion",
)
NESTED_IF_PROPERTY = "generated_nested_if_equation_order_and_rollback_match_recursive_oracle"
NESTED_IF_ASSERTIONS = (
    "depth in 1usize..17,",
    "failure_level in 0usize..16,",
    "prop_assert_eq!(success_oracle.free_map.get(&0), Some(&leaf));",
    "prop_assert_eq!(success_oracle.free_map.get(&41), entry_map.get(&41));",
    '"generated nested If equation order: success",',
    "prop_assert_eq!(failure_oracle.free_map, entry_map.clone());",
    '"generated nested If equation order: rollback",',
)
TLA_INVARIANTS = [
    "TypeInvariant",
    "NoInstructionUnderflow",
    "CompletedEquivalent",
    "FailedBranchesCarryNoBindings",
    "OuterEnvelopeRoutingEquivalent",
    "SameOrUnprovedDelegatesExactly",
    "OnlyProvenDifferentMayShortCircuit",
    "DelegatedInvalidIsNotCollapsedAbsentDifferentTags",
    "CollapsedDeepFailureMutationIsDetected",
    "EncodedParserSoundOnCanonicalCorpus",
    "EncodedConnectiveRoutingEquivalent",
    "EncodedUnprovedDelegatesExactly",
    "MalformedNeverEntersEncodedMachine",
    "SuccessfulDisjunctionCarriesNoBindings",
    "AndFailureRestoresWholeFoldSnapshot",
    "NegationAlwaysRestoresSnapshot",
    "EncodedCursorEnumeratesChildrenOnceInOrder",
    "LeakySuccessfulOrMutationIsDetected",
    "PerChildAndRestoreMutationIsDetected",
    "NotBindingLeakMutationIsDetected",
    "CursorMutationsAreDetected",
    "ParserNoneAsRejectionMutationIsDetected",
    "ShallowTagOnlyMalformedMutationIsDetected",
    "PredicateFamilyMappingIsExactAndPayloadIndependent",
    "PredicatePayloadMutationIsDetected",
    "VarRefAndAbsentReject",
    "AbsentAsEmptyAndMutationIsDetected",
    "ChildMetadataDispatchPreservesResidentAuthority",
    "FalseSummarySingletonDelegatesToConcreteMatch",
    "UnprovedSummarySingletonDelegatesToConcreteMatch",
    "FalseSummaryChildDispatchMutationIsDetected",
    "UnprovedSummaryChildDispatchMutationIsDetected",
    "RootMetadataDispatchPreservesResidentAuthority",
    "FalseSummaryRootDelegatesToConcreteMatch",
    "UnsafeActiveRootDelegatesToCandidateSelectingMatch",
    "SafeActiveRootEntersEncodedMatch",
    "FalseSummaryRootDoesNotCompileConnectiveCandidate",
    "FalseSummaryRootCandidateMutationIsDetected",
    "FalseSummaryRootMatchMutationIsDetected",
    "UnsafeActiveRootMatchMutationIsDetected",
    "CanonicalScalarByteEqualityIsDecodedEquality",
    "ExactScalarRoutePreservesAuthority",
    "UnprovedScalarBytesDelegateExactly",
    "ExactScalarMismatchMutationIsDetected",
    "UnprovedScalarRejectionMutationIsDetected",
]
ROCQ_CAPSTONES = [
    "exact_family_total_is_endpoint_mass",
    "finite_cap_quotient_preserves_target_constraints",
    "bounded_interval_fold_preserves_existing_equations",
    "concrete_expression_minimum_is_exact_nonfree_mass",
    "exact_feasible_range_preserves_both_constraints",
    "impossible_lower_bounds_refuse_finite_targets",
    "zero_complement_requires_the_entire_exact_target",
    "endpointwise_partitions_conserve_exact_mass",
    "feasible_filter_preserves_reference_order_and_uniqueness",
    "partition_scratch_size_depends_only_on_distinct_endpoints",
]
PROJECTED_DYNAMIC_ROOTS = ("Match", "MatchCase", "New")
PENDING_DYNAMIC_ROOTS = ("Receive", "ReceiveBind")
PROJECTION_CAPSTONES = [
    "projection_rules_respect_authority",
    "table_projection_preserves_spatial_success",
    "ignored_field_changes_never_change_acceptance",
    "declined_root_never_prunes",
    "index_filters_preserve_first_admissible_binding",
    "table_query_filters_preserve_first_admissible_binding",
    "locally_free_exact_mutant_is_a_false_negative",
    "cost_region_exact_mutant_is_a_false_negative",
    "extended_retained_prefix_mutant_is_a_false_negative",
    "dynamic_projection_rules_respect_authority",
    "dynamic_table_projection_preserves_spatial_success",
    "dynamic_ignored_field_changes_never_change_acceptance",
    "dynamic_declined_root_never_prunes",
    "dynamic_table_query_filters_preserve_first_admissible_binding",
    "dynamic_projection_preserves_ground_lanes",
    "dynamic_projection_still_declines",
    "new_uri_exact_mutant_is_a_false_negative",
    "new_injections_exact_mutant_is_a_false_negative",
    "receive_peek_exact_mutant_is_a_false_negative",
    "receive_condition_exact_mutant_is_a_false_negative",
    "match_case_free_count_exact_mutant_is_a_false_negative",
    "receive_bind_remainder_exact_mutant_is_a_false_negative",
    "receive_binds_ordered_mutant_is_a_false_negative",
]
PROJECTION_PROPERTIES = [
    {
        "id": "SM-PROJ-PROP-01",
        "theorem": "table_projection_preserves_spatial_success",
        "target_source": PDA,
        "target": "compiled_rho_path_query_is_a_superset_of_every_recursive_oracle_success",
        "assertions": ["compare(label, target, pattern);"],
        "helper_source": PDA,
        "helper": "compare_with_entry_map",
        "helper_assertions": [
            '"{label}: a compiled RhoPath query rejected a recursive-oracle success"',
            'assert_eq!(pda_verdict, recursive_verdict, "{label}: verdict");',
        ],
    },
    {
        "id": "SM-PROJ-PROP-02",
        "theorem": "ignored_field_changes_never_change_acceptance",
        "target_source": PDA,
        "target": "ignored_projection_fields_are_inert_and_exact_fields_change_the_query",
        "assertions": ["assert_projection_rows_are_respected(PROJECTION_RULES, &regions);"],
        "helper_source": PDA,
        "helper": "assert_projection_rows_are_respected",
        "helper_assertions": [
            "assert_projection_root_is_normalized(fixture.label, &fixture.target);",
            "assert_projection_root_is_normalized(fixture.label, &fixture.pattern);",
            "\"{}: the pattern's {owner}.{field} = {value:?} changed the projection\"",
            "\"{}: the target's {owner}.{field} = {value:?} changed the projection\"",
            '"{owner}.{field} is marked Exact, but no generated value changes its query"',
        ],
    },
    {
        "id": "SM-PROJ-PROP-03",
        "theorem": "index_filters_preserve_first_admissible_binding",
        "target_source": POOL,
        "target": "continuation_arena_matches_the_legacy_pda_and_byte_oracle",
        "assertions": ["fixture.assert_matches(query);"],
        "helper_source": POOL,
        "helper": "assert_matches",
        "helper_assertions": [
            "(*available && query.matches_structure(structure)).then_some(position)",
            'assert_eq!(actual, expected, "arena traversal must equal the byte oracle");',
        ],
    },
    {
        "id": "SM-PROJ-PROP-04",
        "theorem": "index_filters_preserve_first_admissible_binding",
        "target_source": PDA,
        "target": "list_target_group_mask_equals_the_structure_filter_over_every_group",
        "assertions": [
            ".filter(|(_, structure)| query.matches_structure(structure))",
            '"the group mask must keep exactly the groups the structure filter keeps"',
            '"the group mask dropped a group the structure filter keeps"',
            '"only a query without a retained prefix may skip the group mask"',
        ],
        "helper_source": MATCHER_SOURCE,
        "helper": "send_target_group_mask",
        "helper_assertions": ["(groups, index.group_mask_matching(query))"],
    },
    {
        "id": "SM-PROJ-PROP-05",
        "theorem": None,
        "target_source": PDA,
        "target": "ground_send_query_equals_the_committed_golden_bytes",
        "assertions": [
            "RhoPathQueryAtom::Match(RhoMatchQuery::new(parts)),",
            '"the ground Send query drifted from the committed golden bytes"',
        ],
        "helper_source": None,
        "helper": None,
        "helper_assertions": [],
    },
    {
        "id": "SM-PROJ-PROP-06",
        "theorem": "declined_root_never_prunes",
        "target_source": PDA,
        "target": "every_decline_rule_yields_no_query_and_the_oracle_verdict",
        "assertions": [
            '"{owner}.{field} is a Decline rule, but its pattern compiled a query"',
            'compare(\n            &format!("declined {owner}.{field}"),\n            target.clone(),\n'
            "            pattern.clone(),\n        );",
            "Some(RhoPathQueryAtom::Exact(encode_par_match_atom(empty))),",
        ],
        "helper_source": PDA,
        "helper": "compare_with_entry_map",
        "helper_assertions": ['assert_eq!(pda_verdict, recursive_verdict, "{label}: verdict");'],
    },
    {
        "id": "SM-PROJ-PROP-07",
        "theorem": None,
        "target_source": RETAINED,
        "target": "depth_4096_pattern_compiles_and_matches_on_a_one_mebibyte_thread",
        "assertions": [
            ".stack_size(1024 * 1024)",
            "const DEPTH: usize = 4_096;",
            ".get(&pattern, &candidate)",
        ],
        "helper_source": None,
        "helper": None,
        "helper_assertions": [],
    },
]
WITNESSES = {
    "CompletedEquivalent": (
        PDA,
        "recursive_oracle_and_pda_agree_on_the_semantic_corpus",
        'compare("ground equality"',
    ),
    "FailedBranchesCarryNoBindings": (
        ISOLATION,
        "control_the_disjunction_arm_is_behaviourally_untouched",
        'assert_eq!(ctx.free_map, new_free_map(), "and the first left nothing");',
    ),
    "SameOrUnprovedDelegatesExactly": (
        PDA,
        "encoded_expr_outer_route_rejects_only_proven_different_tags",
        "_ => Delegate,",
    ),
    "OnlyProvenDifferentMayShortCircuit": (
        PDA,
        "encoded_expr_outer_route_rejects_only_proven_different_tags",
        "if target != pattern => RejectProvenDifferent,",
    ),
    "SuccessfulDisjunctionCarriesNoBindings": (
        ISOLATION,
        "control_the_disjunction_arm_is_behaviourally_untouched",
        'assert_eq!(ctx.free_map, new_free_map(), "and the first left nothing");',
    ),
    "AndFailureRestoresWholeFoldSnapshot": (
        ISOLATION,
        "a_refused_conjunction_reverts_the_conjuncts_that_already_bound",
        'assert_eq!(ctx.free_map, new_free_map(), "and nothing else either");',
    ),
    "NegationAlwaysRestoresSnapshot": (
        ISOLATION,
        "a_negation_binds_nothing_whatever_its_body_is_made_of",
        "with.free_map, without.free_map,",
    ),
    "PredicateFamilyMappingIsExactAndPayloadIndependent": (
        PDA,
        "byte_native_connective_authority_covers_every_terminal_shape",
        '"encoded predicate rejects a different scalar family"',
    ),
    "VarRefAndAbsentReject": (
        PDA,
        "byte_native_connective_authority_covers_every_terminal_shape",
        '"encoded VarRef remains unconditional refusal"',
    ),
    "ChildMetadataDispatchPreservesResidentAuthority": (
        PDA,
        "byte_native_connective_respects_child_and_root_dispatch_metadata",
        '"unary spine stops before an inactive Connective child"',
    ),
    "RootMetadataDispatchPreservesResidentAuthority": (
        PDA,
        "byte_native_connective_respects_child_and_root_dispatch_metadata",
        '"inactive root Connective remains concrete data"',
    ),
    "ExactScalarRoutePreservesAuthority": (
        PDA,
        "byte_native_connective_exact_scalar_route_is_complete_and_non_vacuous",
        "assert_eq!(stats.authoritative_exact_scalar_equal, 2);",
    ),
    "UnprovedScalarBytesDelegateExactly": (
        PDA,
        "byte_native_connective_exact_scalar_route_is_complete_and_non_vacuous",
        "assert_eq!(stats.authoritative_exact_scalar_unproved, 2);",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def directives(source, directive):
    found = re.findall(rf"(?m)^{directive}[ \t]+([A-Za-z][A-Za-z0-9_]*)$", source)
    require(len(found) == len(set(found)), f"duplicate {directive}")
    require(
        len(found) == len(re.findall(rf"(?m)^{directive}\b", source)),
        f"malformed {directive}",
    )
    return found


def test_body(source, name):
    matches = list(re.finditer(rf"(?m)^fn {re.escape(name)}\(\)", source))
    require(len(matches) == 1, f"missing or duplicate normal-suite test: {name}")
    attrs = []
    for line in reversed(source[: matches[0].start()].splitlines()):
        if not line.strip().startswith("#["):
            break
        attrs.append(line.strip())
    require("#[test]" in attrs, f"non-test target: {name}")
    require(
        not any(attr.startswith("#[ignore") for attr in attrs),
        f"ignored-only target: {name}",
    )
    require(
        not any(attr.startswith("#[cfg") for attr in attrs),
        f"conditional target: {name}",
    )
    start = matches[0].start()
    following = source.find("\n#[test]", start)
    return source[start : following if following >= 0 else len(source)]


def generated_property_body(source, name):
    marker = f"    fn {name}("
    require(source.count(marker) == 1, f"missing or duplicate generated property: {name}")
    start = source.index(marker)
    before = source[:start]
    require(
        "proptest::proptest! {" in before
        and "proptest::collection::vec" in source[start:],
        f"generated property lacks proptest corpus: {name}",
    )
    end = source.find("\n    }\n}", start)
    require(end >= 0, f"generated property boundary missing: {name}")
    return source[start:end]


def nested_if_property_body(source):
    marker = f"    fn {NESTED_IF_PROPERTY}("
    require(source.count(marker) == 1, "missing or duplicate nested-If property")
    start = source.index(marker)
    macro = source.rfind("proptest! {", 0, start)
    require(macro >= 0, "nested-If property is outside proptest")
    end = source.find("\n    }\n}", start)
    require(end >= 0, "nested-If property boundary missing")
    declaration = source[macro:start]
    require(
        "#[test]" in declaration and "#[ignore" not in declaration,
        "nested-If property is not an active normal-suite test",
    )
    body = source[start:end]
    for assertion in NESTED_IF_ASSERTIONS:
        require(assertion in body, f"stale nested-If oracle property: {assertion}")
    require(
        ".spatial_match(target.clone(), success_pattern.clone())\n            .is_some()"
        in body
        and ".spatial_match(target.clone(), failure_pattern.clone())\n            .is_none()"
        in body,
        "nested-If property lacks anti-vacuous success and failure oracles",
    )
    return body


def projection_property_body(source, name):
    if re.search(rf"(?m)^fn {re.escape(name)}\(\)", source):
        return test_body(source, name)
    marker = f"    fn {name}("
    require(source.count(marker) == 1, f"missing or duplicate projection property: {name}")
    start = source.index(marker)
    macro = source.rfind("proptest! {", 0, start)
    require(macro >= 0, f"projection property is outside proptest: {name}")
    declaration = source[macro:start]
    require(
        "#[test]" in declaration and "#[ignore" not in declaration,
        f"projection property is not an active normal-suite test: {name}",
    )
    end = source.find("\n    }\n}", start)
    require(end >= 0, f"projection property boundary missing: {name}")
    return source[start:end]


def helper_body(source, name):
    matches = list(
        re.finditer(rf"(?m)^([ \t]*)(?:pub(?:\([a-z]+\))? )?fn {re.escape(name)}\(", source)
    )
    require(len(matches) == 1, f"missing or duplicate projection property helper: {name}")
    start = matches[0].start()
    end = source.find(f"\n{matches[0].group(1)}}}\n", start)
    require(end >= 0, f"projection property helper boundary missing: {name}")
    return source[start:end]


def extract(sources):
    config = sources[CONFIG]
    model = sources[MODEL]
    rocq = sources[ROCQ]
    runner = re.sub(r"\\\n", " ", sources[RUNNER])
    require(directives(config, "SPECIFICATION") == ["Spec"], "stale TLC specification")
    invariants = directives(config, "INVARIANT")
    require(invariants == TLA_INVARIANTS, "missing, extra, or reordered TLC invariant")
    require(
        not re.search(r"(?m)^PROPERT(?:Y|IES)\b|^INVARIANTS\b", config),
        "unregistered TLC directive",
    )
    for symbol in ["Spec", *invariants]:
        require(
            len(re.findall(rf"(?m)^{re.escape(symbol)}[ \t]*==", model)) == 1,
            f"missing or duplicate TLC definition: {symbol}",
        )
    require(
        'tlc_run "$tla_metadir" SpatialMatcherEquivalence.cfg SpatialMatcherEquivalence.tla'
        in runner,
        "SpatialMatcher TLC baseline absent from normal runner",
    )
    prints = re.findall(r"(?m)^Print Assumptions ([A-Za-z][A-Za-z0-9_]*)\.$", rocq)
    require(prints == ROCQ_CAPSTONES, "missing, extra, or reordered Rocq capstone")
    require(
        len(prints) == len(re.findall(r"(?m)^Print Assumptions\b", rocq)),
        "malformed Rocq capstone",
    )
    for symbol in prints:
        require(
            len(re.findall(rf"(?m)^Theorem {re.escape(symbol)}[ \t]*:", rocq)) == 1,
            f"missing or duplicate Rocq theorem: {symbol}",
        )
    require(
        "rocq_compile_with_closed_count theories/SpatialMatcher.v 10" in runner,
        "Rocq ten-closed-capstone gate absent",
    )
    require("matcher_count_proofs\n" in runner, "Rocq proof gate not invoked")
    projection = sources[PROJECTION_THEORY]
    table_rows = re.findall(
        r'\(\s*"([A-Za-z][A-Za-z0-9]*)",\s*"([a-z][a-z0-9_]*)",\s*ProjectionRule::([A-Za-z]+),?\s*\)',
        sources[PROJECTION_TABLE],
    )
    theory_rows = re.findall(
        r'ProjectionRow "([A-Za-z][A-Za-z0-9]*)" "([a-z][a-z0-9_]*)" ([A-Za-z]+)',
        projection,
    )
    dynamic_table = re.search(
        r"Definition dynamic_root_rows : list projection_row := \[(.*?)\]\.",
        projection,
        re.S,
    )
    require(dynamic_table is not None, "Rocq dynamic-root table absent")
    dynamic_rows = re.findall(
        r'DynamicRootRow "([A-Za-z][A-Za-z0-9]*)" "([a-z][a-z0-9_]*)" ([A-Za-z]+)',
        dynamic_table.group(1),
    )
    dynamic_rules = {(owner, field): rule for owner, field, rule in dynamic_rows}
    dynamic_owners = {owner for owner, _, _ in dynamic_rows}
    require(
        len(dynamic_rules) == len(dynamic_rows)
        and not set(PROJECTED_DYNAMIC_ROOTS) & set(PENDING_DYNAMIC_ROOTS)
        and dynamic_owners == set(PROJECTED_DYNAMIC_ROOTS) | set(PENDING_DYNAMIC_ROOTS),
        "Rocq dynamic-root table does not name exactly the dynamic roots once per field",
    )
    require(
        set(dynamic_rules)
        == {(owner, field) for owner, field, _ in theory_rows if owner in dynamic_owners},
        "Rocq dynamic-root table does not cover exactly the dynamic roots' fields",
    )
    implemented_rows = [
        (owner, field, dynamic_rules[owner, field] if owner in PROJECTED_DYNAMIC_ROOTS else rule)
        for owner, field, rule in theory_rows
    ]
    require(
        bool(table_rows) and implemented_rows == table_rows,
        "Rocq projection table differs from projection_rules.rs",
    )
    projection_prints = re.findall(
        r"(?m)^Print Assumptions ([A-Za-z][A-Za-z0-9_]*)\.$", projection
    )
    require(
        projection_prints == PROJECTION_CAPSTONES,
        "missing, extra, or reordered projection capstone",
    )
    require(
        len(projection_prints) == len(re.findall(r"(?m)^Print Assumptions\b", projection)),
        "malformed projection capstone",
    )
    for symbol in projection_prints:
        require(
            len(re.findall(rf"(?m)^Theorem {re.escape(symbol)}[ \t]*:", projection)) == 1,
            f"missing or duplicate projection theorem: {symbol}",
        )
    require(
        "rocq_compile_with_closed_count theories/SchemaProjectionCorrespondence.v "
        f"{len(PROJECTION_CAPSTONES)}" in runner,
        "projection closed-capstone gate absent",
    )
    property_ids = [entry["id"] for entry in PROJECTION_PROPERTIES]
    require(len(property_ids) == len(set(property_ids)), "duplicate projection property id")
    for entry in PROJECTION_PROPERTIES:
        require(
            entry["theorem"] is None or entry["theorem"] in projection_prints,
            f"projection property bound to an unregistered theorem: {entry['id']}",
        )
        body = projection_property_body(sources[entry["target_source"]], entry["target"])
        for fragment in entry["assertions"]:
            require(fragment in body, f"stale projection property assertion: {entry['id']}")
        if entry["helper"] is not None:
            helper = helper_body(sources[entry["helper_source"]], entry["helper"])
            for fragment in entry["helper_assertions"]:
                require(fragment in helper, f"stale projection property assertion: {entry['id']}")
    require(
        "mod spatial_matcher_pda_equivalence;" in sources[MODULE],
        "PDA normal-suite module unregistered",
    )
    require(
        "rholang_run_tests 66 --lib rust::interpreter::matcher::spatial_matcher_pda_equivalence::"
        in runner,
        "PDA normal-suite lane absent",
    )
    require(
        "rholang_run_tests 16 --test matcher_connective_isolation" in runner,
        "connective-isolation normal-suite lane absent",
    )
    require(
        len(re.findall(r"(?m)^[ \t]*#\[test\]$", sources[PDA])) == 66,
        "PDA normal-suite census changed",
    )
    require(
        len(re.findall(r"(?m)^#\[test\]$", sources[ISOLATION])) == 16,
        "connective-isolation normal-suite census changed",
    )
    require(
        len(set(WITNESSES) - set(invariants)) == 0,
        "witness bound to unconfigured TLC invariant",
    )
    for symbol, (path, test, fragment) in WITNESSES.items():
        require(
            fragment in test_body(sources[path], test),
            f"stale executable assertion: {symbol}",
        )
    compare = sources[PDA].split("fn compare_with_entry_map(", 1)
    require(len(compare) == 2, "missing recursive/PDA comparison helper")
    comparison = compare[1].split("fn assert_binding_bytes(", 1)[0]
    for fragment in (
        'assert_eq!(pda_verdict, recursive_verdict, "{label}: verdict");',
        'assert_eq!(pda.free_map, recursive_map, "{label}: FreeMap");',
    ):
        require(fragment in comparison, f"stale PDA differential assertion: {fragment}")
    nested_if_property_body(sources[PDA])
    owner_model = sources[OWNER_MODEL]
    owner_mutants = sources[OWNER_MUTANTS]
    owner_source = sources[CONNECTIVE_SOURCE]
    property_body = generated_property_body(
        sources[CONNECTIVE_PROPERTY],
        "generated_connective_cursor_owner_pairing_and_exhaustion",
    )
    require(
        'canonical_connective_owner_verus\n' in runner,
        "canonical Connective owner proof absent from normal runner",
    )
    require(
        "verification results:: 10 verified, 0 errors" in runner,
        "canonical Connective owner proof count absent",
    )
    require(
        "rpb3_run_tests 1 --test par_bag generated_connective_cursor_owner_pairing_and_exhaustion"
        in runner
        and "rpb3_run_tests 1 --release --test par_bag generated_connective_cursor_owner_pairing_and_exhaustion"
        in runner,
        "generated connective owner property absent from debug/release normal runner",
    )
    require(
        "rpb3_run_tests 4 --test par_bag canonical_connective_cursor_" in runner
        and "rpb3_run_tests 4 --release --test par_bag canonical_connective_cursor_"
        in runner,
        "concrete connective owner tests absent from debug/release normal runner",
    )
    for symbol, fragments in OWNER_INVARIANTS:
        require(
            len(re.findall(rf"(?m)^pub proof fn {re.escape(symbol)}\(", owner_model)) == 1,
            f"missing or duplicate canonical Connective owner theorem: {symbol}",
        )
        for fragment in fragments:
            require(fragment in property_body, f"stale owner property extraction: {symbol}")
    require(
        property_body.count("proptest::prop_assert_eq!(&range, expected_range);") == 2,
        "owned and resumed Connective ranges must both match independent positions",
    )
    require(
        "pub unsafe trait StableConnectiveBodyOwner: AsRef<[u8]> {}" in owner_source,
        "stable Connective owner contract absent",
    )
    for implementation in (
        "unsafe impl StableConnectiveBodyOwner for &[u8] {}",
        "unsafe impl StableConnectiveBodyOwner for Vec<u8> {}",
        "unsafe impl StableConnectiveBodyOwner for crate::rust::rholang::protobuf_cursor::ProtobufRegion {}",
    ):
        require(implementation in owner_source, f"stable owner implementation absent: {implementation}")
    require(
        "unsafe impl StableConnectiveBodyOwner for EncodedRegion {}" in sources[MATCHER_SOURCE],
        "encoded region owner implementation absent",
    )
    for mutant in OWNER_MUTATION_NAMES:
        require(
            f'"{mutant}": (' in owner_mutants,
            f"canonical Connective owner mutant generator absent: {mutant}",
        )
        require(
            mutant in runner,
            f"canonical Connective owner mutant gate absent: {mutant}",
        )
    return {
        "tla_invariants": [
            {
                "id": f"SM-TLA-{index:02d}",
                "symbol": symbol,
                "classification": "executable_refinement"
                if symbol in WITNESSES
                else "proof_only",
                "target_source": WITNESSES[symbol][0] if symbol in WITNESSES else None,
                "target": WITNESSES[symbol][1] if symbol in WITNESSES else None,
                "assertion": WITNESSES[symbol][2] if symbol in WITNESSES else None,
            }
            for index, symbol in enumerate(invariants, 1)
        ],
        "rocq_print_assumptions": [
            {
                "id": f"SM-ROCQ-{index:02d}",
                "symbol": symbol,
                "classification": "proof_only",
            }
            for index, symbol in enumerate(prints, 1)
        ],
        "verus_obligations": [
            {
                "id": f"SM-VERUS-{index:02d}",
                "symbol": symbol,
                "classification": "executable_refinement",
                "method": "source_model_with_generated_property",
                "source_model": OWNER_MODEL,
                "source_contract": CONNECTIVE_SOURCE,
                "target_source": CONNECTIVE_PROPERTY,
                "target": "generated_connective_cursor_owner_pairing_and_exhaustion",
                "assertions": fragments,
            }
            for index, (symbol, fragments) in enumerate(OWNER_INVARIANTS, 1)
        ],
        "generated_properties": [
            {
                "id": "SM-RUST-01",
                "classification": "executable_refinement",
                "method": "bounded_recursive_oracle_differential",
                "target_source": PDA,
                "target": NESTED_IF_PROPERTY,
                "assertions": list(NESTED_IF_ASSERTIONS),
            }
        ],
        "mcrl2_obligations": [],
        "projection_rules": {
            "table": PROJECTION_TABLE,
            "theory": PROJECTION_THEORY,
            "rows": len(table_rows),
            "capstones": [
                {
                    "id": f"SM-PROJ-{index:02d}",
                    "symbol": symbol,
                    "classification": "executable_refinement"
                    if any(entry["theorem"] == symbol for entry in PROJECTION_PROPERTIES)
                    else "proof_only",
                    "normal_suite_witnesses": [
                        entry["target"]
                        for entry in PROJECTION_PROPERTIES
                        if entry["theorem"] == symbol
                    ],
                }
                for index, symbol in enumerate(projection_prints, 1)
            ],
            "properties": [
                {
                    "id": entry["id"],
                    "theorem": entry["theorem"],
                    "classification": "executable_refinement"
                    if entry["theorem"] is not None
                    else "executable_only",
                    "target_source": entry["target_source"],
                    "target": entry["target"],
                    "assertions": list(entry["assertions"]),
                    "helper_source": entry["helper_source"],
                    "helper": entry["helper"],
                    "helper_assertions": list(entry["helper_assertions"]),
                }
                for entry in PROJECTION_PROPERTIES
            ],
        },
        "scope_limit": "The canonical Connective owner Verus model constrains validated field-1 ranges by complete canonical source bytes, then proves binding, exact next range, and exhaustion under the audited StableConnectiveBodyOwner premise. The model does not verify the production Rust parser, pointer provenance, or unsafe-trait implementations; the generated debug/release property and concrete source audit bridge those premises. Its five Verus mutants test the proof gate. Finite TLC invariants and abstract Rocq theorems still require their proof engines. The registry checks source and runner wiring, not proof execution.",
        "source_sha256": {
            path: hashlib.sha256(source.encode()).hexdigest()
            for path, source in sorted(sources.items())
            if path not in (RUNNER, MODULE)
        },
    }


def reject(sources, path, before, after, expected):
    changed = dict(sources)
    require(before in changed[path], f"self-test setup absent: {before}")
    changed[path] = changed[path].replace(before, after, 1)
    try:
        extract(changed)
    except RegistryError as error:
        require(expected in str(error), f"wrong mutation rejection: {error}")
        return
    raise RegistryError(f"mutation unexpectedly passed: {expected}")


def self_test(sources):
    reject(
        sources,
        CONFIG,
        "INVARIANT CompletedEquivalent\n",
        "",
        "missing, extra, or reordered TLC invariant",
    )
    reject(
        sources,
        CONFIG,
        "INVARIANT CompletedEquivalent\n",
        "INVARIANT CompletedEquivalent\nINVARIANT CompletedEquivalent\n",
        "duplicate INVARIANT",
    )
    reject(
        sources,
        MODEL,
        "CompletedEquivalent ==",
        "RemovedEquivalent ==",
        "missing or duplicate TLC definition",
    )
    reject(
        sources,
        ROCQ,
        "Print Assumptions exact_family_total_is_endpoint_mass.\n",
        "",
        "missing, extra, or reordered Rocq capstone",
    )
    reject(
        sources,
        ROCQ,
        "Print Assumptions exact_family_total_is_endpoint_mass.\n",
        "Print Assumptions exact_family_total_is_endpoint_mass.\nPrint Assumptions exact_family_total_is_endpoint_mass.\n",
        "missing, extra, or reordered Rocq capstone",
    )
    reject(
        sources,
        PDA,
        "#[test]\nfn recursive_oracle_and_pda_agree_on_the_semantic_corpus",
        "#[test]\n#[ignore]\nfn recursive_oracle_and_pda_agree_on_the_semantic_corpus",
        "ignored-only target",
    )
    reject(
        sources,
        ISOLATION,
        'assert_eq!(ctx.free_map, new_free_map(), "and the first left nothing");',
        'assert_eq!(ctx.free_map, wrong_map(), "and the first left nothing");',
        "stale executable assertion",
    )
    reject(
        sources,
        PDA,
        'assert_eq!(pda.free_map, recursive_map, "{label}: FreeMap");',
        'assert_eq!(pda.free_map, wrong_map, "{label}: FreeMap");',
        "stale PDA differential assertion",
    )
    reject(
        sources,
        PDA,
        "prop_assert_eq!(failure_oracle.free_map, entry_map.clone());",
        "prop_assert_eq!(failure_oracle.free_map, wrong_map);",
        "stale nested-If oracle property",
    )
    reject(
        sources,
        RUNNER,
        'tlc_run "$tla_metadir" SpatialMatcherEquivalence.cfg SpatialMatcherEquivalence.tla',
        'tlc_run "$tla_metadir" Other.cfg Other.tla',
        "TLC baseline absent",
    )
    reject(
        sources,
        OWNER_MODEL,
        "pub proof fn exact_owner_preserves_validated_position(",
        "pub proof fn removed_owner_position_theorem(",
        "missing or duplicate canonical Connective owner theorem",
    )
    reject(
        sources,
        CONNECTIVE_PROPERTY,
        "prop_assert_eq!(&owned.body()[range], *expected_child);",
        "prop_assert_eq!(&owned.body()[range], wrong_child);",
        "stale owner property extraction",
    )
    reject(
        sources,
        OWNER_MUTANTS,
        '"length_only": (',
        '"unregistered_length_only": (',
        "canonical Connective owner mutant generator absent",
    )
    reject(
        sources,
        PROJECTION_THEORY,
        'ProjectionRow "Par" "locally_free" Ignored',
        'ProjectionRow "Par" "locally_free" Exact',
        "Rocq projection table differs from projection_rules.rs",
    )
    reject(
        sources,
        PROJECTION_TABLE,
        '("Send", "cost_region", ProjectionRule::Ignored),',
        '("Send", "cost_region", ProjectionRule::Exact),',
        "Rocq projection table differs from projection_rules.rs",
    )
    reject(
        sources,
        PROJECTION_TABLE,
        '("New", "uri", ProjectionRule::Ignored),',
        '("New", "uri", ProjectionRule::Exact),',
        "Rocq projection table differs from projection_rules.rs",
    )
    reject(
        sources,
        PROJECTION_THEORY,
        'DynamicRootRow "New" "uri" Ignored',
        'DynamicRootRow "New" "uri" Exact',
        "Rocq projection table differs from projection_rules.rs",
    )
    reject(
        sources,
        PROJECTION_TABLE,
        '("Receive", "binds", ProjectionRule::Decline),',
        '("Receive", "binds", ProjectionRule::UnorderedSeq),',
        "Rocq projection table differs from projection_rules.rs",
    )
    reject(
        sources,
        PROJECTION_THEORY,
        "Definition dynamic_root_rows : list projection_row := [",
        "Definition dynamic_root_rows : list projection_row := (",
        "Rocq dynamic-root table absent",
    )
    reject(
        sources,
        PROJECTION_THEORY,
        'DynamicRootRow "New" "uri" Ignored',
        'DynamicRootRow "New" "p" Ignored',
        "Rocq dynamic-root table does not name exactly the dynamic roots once per field",
    )
    reject(
        sources,
        PROJECTION_THEORY,
        'DynamicRootRow "New" "uri" Ignored',
        'DynamicRootRow "Nova" "uri" Ignored',
        "Rocq dynamic-root table does not name exactly the dynamic roots once per field",
    )
    reject(
        sources,
        PROJECTION_THEORY,
        'DynamicRootRow "New" "uri" Ignored',
        'DynamicRootRow "New" "url" Ignored',
        "Rocq dynamic-root table does not cover exactly the dynamic roots' fields",
    )
    reject(
        sources,
        PROJECTION_THEORY,
        "Print Assumptions declined_root_never_prunes.\n",
        "",
        "missing, extra, or reordered projection capstone",
    )
    reject(
        sources,
        RUNNER,
        "rocq_compile_with_closed_count theories/SchemaProjectionCorrespondence.v 23",
        "rocq_compile_with_closed_count theories/SchemaProjectionCorrespondence.v 22",
        "projection closed-capstone gate absent",
    )
    reject(
        sources,
        PDA,
        "    fn ignored_projection_fields_are_inert_and_exact_fields_change_the_query(",
        "    fn renamed_projection_property(",
        "missing or duplicate projection property",
    )
    reject(
        sources,
        RETAINED,
        "#[test]\nfn depth_4096_pattern_compiles_and_matches_on_a_one_mebibyte_thread",
        "#[test]\n#[ignore]\nfn depth_4096_pattern_compiles_and_matches_on_a_one_mebibyte_thread",
        "ignored-only target",
    )
    reject(
        sources,
        POOL,
        'assert_eq!(actual, expected, "arena traversal must equal the byte oracle");',
        'assert_eq!(actual, actual, "arena traversal must equal the byte oracle");',
        "stale projection property assertion",
    )
    reject(
        sources,
        PDA,
        "assert_projection_rows_are_respected(PROJECTION_RULES, &regions);",
        "assert_projection_rows_are_respected(&[], &regions);",
        "stale projection property assertion",
    )


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    paths = {
        CONFIG, MODEL, ROCQ, RUNNER, MODULE, PDA, ISOLATION,
        OWNER_MODEL, OWNER_MUTANTS, CONNECTIVE_SOURCE, CONNECTIVE_PROPERTY,
        MATCHER_SOURCE,
        PROJECTION_TABLE, PROJECTION_THEORY, POOL, RETAINED,
    }
    sources = {
        path: (ROOT / path).read_text(encoding="utf-8") for path in sorted(paths)
    }
    payload = json.dumps(extract(sources), indent=2, sort_keys=True) + "\n"
    if args.self_test:
        self_test(sources)
        print("SpatialMatcher registry: mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload, encoding="utf-8")
        print("SpatialMatcher registry: written")
    else:
        require(
            (ROOT / OUTPUT).read_text(encoding="utf-8") == payload,
            "SpatialMatcher registry is stale",
        )
        print("SpatialMatcher registry: current")


if __name__ == "__main__":
    main()
