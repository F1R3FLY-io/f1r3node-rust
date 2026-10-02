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
    require(
        "mod spatial_matcher_pda_equivalence;" in sources[MODULE],
        "PDA normal-suite module unregistered",
    )
    require(
        "rholang_run_tests 57 --lib rust::interpreter::matcher::spatial_matcher_pda_equivalence::"
        in runner,
        "PDA normal-suite lane absent",
    )
    require(
        "rholang_run_tests 16 --test matcher_connective_isolation" in runner,
        "connective-isolation normal-suite lane absent",
    )
    require(
        len(re.findall(r"(?m)^#\[test\]$", sources[PDA])) == 57,
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
        "verus_obligations": [],
        "mcrl2_obligations": [],
        "scope_limit": "This SpatialMatcher source family has no dedicated Verus or mCRL2 source. Direct executable rows check concrete normal-suite assertions; finite TLC invariants and abstract Rocq theorems require their proof engines. Mutation-detection and parser/model-only predicates have no claimed executable refinement target. The registry checks source and runner wiring, not proof execution.",
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
        RUNNER,
        'tlc_run "$tla_metadir" SpatialMatcherEquivalence.cfg SpatialMatcherEquivalence.tla',
        'tlc_run "$tla_metadir" Other.cfg Other.tla',
        "TLC baseline absent",
    )


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    paths = {CONFIG, MODEL, ROCQ, RUNNER, MODULE, PDA, ISOLATION}
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
