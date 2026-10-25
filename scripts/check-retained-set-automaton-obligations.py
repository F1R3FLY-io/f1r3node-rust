import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ROCQ = Path("formal/rocq/stack_safe_pda/theories/SpatialMatcher.v")
RUNNER = Path("scripts/check-stack-safe-pda-proofs.sh")
PACKAGE = Path("rspace++/Cargo.toml")
AUTOMATON = Path("rspace++/src/rspace/retained_set_automaton.rs")
SPEC = Path("rspace++/tests/retained_set_automaton_spec.rs")
ALLOCATION = Path("rspace++/tests/retained_set_automaton_allocation_spec.rs")
OUTPUT = Path("formal/retained_set_automaton_obligations.generated.json")

SECTIONS = {
    "RetainedPathQuery": {
        "hypotheses": (
            "successful_match_has_retained_prefix",
            "successful_match_passes_residual_query",
        ),
        "obligations": (
            (
                "Theorem",
                "retained_candidate_has_no_false_negative",
                "executable_refinement",
            ),
            ("Theorem", "retained_filter_preserves_first_spatial_match", "executable_refinement"),
        ),
    },
    "RetainedBagProduct": {
        "hypotheses": (
            "successful_match_has_product_prefix",
            "successful_fresh_match_passes_residual",
        ),
        "obligations": (
            ("Theorem", "retained_bag_product_has_no_false_negative", "executable_refinement"),
            (
                "Corollary",
                "retained_bag_product_rejection_proves_no_pool_match",
                "executable_refinement",
            ),
        ),
    },
}

EXPECTED_TESTS = {
    SPEC: (
        "exact_terminals_and_root_fallbacks_are_selected_by_bind_position",
        "automaton_selection_is_a_superset_of_direct_matches",
        "every_candidate_in_a_large_shared_terminal_chain_is_selected",
        "structural_queries_use_exact_trie_prefixes_and_filter_after_wildcards",
        "retained_segments_product_walk_the_authoritative_resting_bag",
        "compressed_product_runs_visit_prefix_terminals_and_bag_branches",
        "generated_resting_bag_product_preserves_direct_matches_and_first_order",
        "singleton_bag_path_selects_a_terminal_below_diverse_and_shared_branches",
        "singleton_bag_with_multiple_terminal_depths_uses_the_linear_product",
        "representation_stats_expose_eligibility_and_terminal_sharing",
        "immutable_rebuilds_never_accumulate_dead_terminals",
        "deep_and_wide_automata_compile_select_and_drop_on_a_tiny_stack",
    ),
    ALLOCATION: ("doubling_live_terminals_has_a_linear_construction_allocation_slope",),
}

WITNESSES = {
    "retained_candidate_has_no_false_negative": (
        "automaton_selection_is_a_superset_of_direct_matches",
    ),
    "retained_filter_preserves_first_spatial_match": (
        "generated_resting_bag_product_preserves_direct_matches_and_first_order",
    ),
    "retained_bag_product_has_no_false_negative": (
        "retained_segments_product_walk_the_authoritative_resting_bag",
        "compressed_product_runs_visit_prefix_terminals_and_bag_branches",
        "generated_resting_bag_product_preserves_direct_matches_and_first_order",
    ),
    "retained_bag_product_rejection_proves_no_pool_match": (
        "retained_segments_product_walk_the_authoritative_resting_bag",
        "compressed_product_runs_visit_prefix_terminals_and_bag_branches",
        "generated_resting_bag_product_preserves_direct_matches_and_first_order",
    ),
}

WITNESS_ASSERTIONS = {
    "automaton_selection_is_a_superset_of_direct_matches": (
        "Matcher.get(pattern, &datum.to_vec()).is_some()",
        "!directly_matches || selected[candidate_index]",
    ),
    "retained_segments_product_walk_the_authoritative_resting_bag": (
        'RhoPathV1::relative_datum_key(b"resting/alpha", b"identity/alpha")',
        'matching_candidates_with_resting(b"unrelated/fresh", &[0], Some(&resting))',
        "assert_eq!(selected, vec![true, false, true])",
    ),
    "compressed_product_runs_visit_prefix_terminals_and_bag_branches": (
        'RhoPathV1::relative_datum_key(&alpha_deeper, b"identity/alpha")',
        'RhoPathV1::relative_datum_key(&beta, b"identity/beta")',
        "assert_eq!(selected, vec![true, true, true, false, true])",
    ),
    "singleton_bag_with_multiple_terminal_depths_uses_the_linear_product": (
        "assert_eq!(selected, vec![true, true, true, false])",
    ),
    "generated_resting_bag_product_preserves_direct_matches_and_first_order": (
        "resting_only.push(0xff)",
        "Pattern::Exact(fresh.clone())",
        "Pattern::Exact(vec![0xa5; longest_datum + 1])",
        "direct.iter().filter(|accepted| **accepted).count() >= 2",
        "!selected[negative_index]",
        "!*accepted || selected[index]",
        "first_selected_direct, first_direct",
        "selected.iter().any(|retained| *retained) || first_direct.is_none()",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def digest(source):
    return hashlib.sha256(source.encode()).hexdigest()


def symbols(source, pattern):
    return re.findall(pattern, source, re.MULTILINE)


def section(source, name):
    matches = list(
        re.finditer(
            rf"(?ms)^Section {re.escape(name)}\.\n(.*?)^End {re.escape(name)}\.$",
            source,
        )
    )
    require(len(matches) == 1, f"missing or duplicate Rocq section: {name}")
    return matches[0].group(1)


def test_bodies(source, path):
    pattern = re.compile(
        r"(?m)^(?P<attrs>(?:#\[[^\n]+\]\n)+)fn (?P<name>[A-Za-z_][A-Za-z_0-9]*)\("
    )
    matches = [
        match for match in pattern.finditer(source) if "#[test]" in match.group("attrs")
    ]
    require(
        len(matches) == len(symbols(source, r"^#\[test\]$")),
        f"malformed normal-suite test in {path}",
    )
    names = [match.group("name") for match in matches]
    require(len(names) == len(set(names)), f"duplicate normal-suite target in {path}")
    require(
        tuple(names) == EXPECTED_TESTS[path],
        f"missing, extra, or reordered normal-suite target in {path}",
    )
    bodies = {}
    for index, match in enumerate(matches):
        attrs = match.group("attrs")
        require("#[ignore" not in attrs, f"ignored-only target: {match.group('name')}")
        require("#[cfg" not in attrs, f"conditional target: {match.group('name')}")
        end = matches[index + 1].start() if index + 1 < len(matches) else len(source)
        bodies[match.group("name")] = re.sub(r"\s+", "", source[match.start() : end])
    return bodies


def extract(sources):
    rocq = sources[ROCQ]
    runner = re.sub(r"\\\n", " ", sources[RUNNER])
    require(
        "rocq_compile_with_closed_count theories/SpatialMatcher.v 10" in runner
        and "matcher_count_proofs\n" in runner,
        "retained Rocq source absent from normal proof runner",
    )
    require(
        'name = "rspace_plus_plus"' in sources[PACKAGE]
        and not re.search(r"(?m)^autotests\s*=\s*false\b", sources[PACKAGE]),
        "Cargo integration-test discovery disabled",
    )
    automaton = sources[AUTOMATON]
    for seam in (
        "pub fn compile<",
        "pub fn matching_candidates_with_resting(",
        "fn collect_product_prefix_terminals(",
        "fn mark_without_residual(",
        "fn mark(",
    ):
        require(seam in automaton, f"retained runtime seam missing: {seam}")
    bodies = {}
    for path in EXPECTED_TESTS:
        registered = test_bodies(sources[path], path)
        require(
            not (bodies.keys() & registered.keys()),
            "duplicate target across test files",
        )
        bodies.update(registered)
    require(
        set(WITNESSES)
        == {
            name
            for definition in SECTIONS.values()
            for _, name, _ in definition["obligations"]
        },
        "missing or extra Rocq-to-test binding",
    )
    for witness, assertions in WITNESS_ASSERTIONS.items():
        require(witness in bodies, f"missing normal-suite control: {witness}")
        for assertion in assertions:
            require(
                re.sub(r"\s+", "", assertion) in bodies[witness],
                f"control assertion changed: {witness}",
            )
    obligations = []
    for section_name, definition in SECTIONS.items():
        body = section(rocq, section_name)
        hypotheses = symbols(body, r"^Hypothesis ([A-Za-z_][A-Za-z_0-9]*)\s*:")
        require(
            tuple(hypotheses) == definition["hypotheses"],
            f"missing, extra, or reordered Rocq hypothesis: {section_name}",
        )
        targets = symbols(body, r"^(Theorem|Corollary) ([A-Za-z_][A-Za-z_0-9]*)\s*:")
        require(
            tuple(targets)
            == tuple((kind, name) for kind, name, _ in definition["obligations"]),
            f"missing, extra, or reordered Rocq obligation: {section_name}",
        )
        require(
            len(symbols(body, r"^Qed\.$")) == len(targets),
            f"Rocq proof terminator census changed: {section_name}",
        )
        require(
            not symbols(body, r"^Print Assumptions\b"),
            f"unregistered Rocq assumptions target: {section_name}",
        )
        for kind, name, mode in definition["obligations"]:
            witnesses = WITNESSES[name]
            for witness in witnesses:
                require(witness in bodies, f"missing normal-suite witness: {witness}")
            obligations.append(
                {
                    "id": f"RSA-ROCQ-{len(obligations) + 1:02d}",
                    "section": section_name,
                    "kind": kind.lower(),
                    "symbol": name,
                    "mode": mode,
                    "conditional_hypotheses": list(hypotheses),
                    "normal_suite_witnesses": list(witnesses),
                }
            )
    return {
        "scope": "retained RSpace set automaton and authoritative resting-Bag product",
        "rocq_source": str(ROCQ),
        "rocq_sha256": digest(rocq),
        "runtime_source": str(AUTOMATON),
        "runtime_sha256": digest(automaton),
        "normal_suite": {
            str(path): {"sha256": digest(sources[path]), "tests": list(names)}
            for path, names in EXPECTED_TESTS.items()
        },
        "normal_suite_boundary": (
            "These are Cargo auto-discovered integration-test targets. The existing "
            "stack-safe-pda proof script compiles SpatialMatcher.v but does not "
            "explicitly run either retained-automaton Rust test target."
        ),
        "obligations": obligations,
        "negative_controls": [
            {"test": witness, "assertions": list(WITNESS_ASSERTIONS[witness])}
            for witness in (
                "retained_segments_product_walk_the_authoritative_resting_bag",
                "compressed_product_runs_visit_prefix_terminals_and_bag_branches",
                "singleton_bag_with_multiple_terminal_depths_uses_the_linear_product",
            )
        ],
        "dedicated_tla_configurations": [],
        "dedicated_verus_sources": [],
        "causal_formal_mutants": [],
        "proof_boundary": (
            "Rocq theorems are conditional on prefix and residual-query soundness. "
            "The generated fresh/resting-Bag property extracts no-false-negative, "
            "rejection, and first-order invariants for exact and fallback patterns. "
            "Fixed product controls exercise compressed and singleton trie paths; "
            "neither establishes universal prefix-terminal refinement for every "
            "generated RhoPathQuery arm."
        ),
    }


def reject(call):
    try:
        call()
    except RegistryError:
        return
    raise RegistryError("mutated retained-automaton registry input was accepted")


def self_test(sources):
    def changed(path, old, new):
        require(old in sources[path], f"self-test fixture missing: {old}")
        return {**sources, path: sources[path].replace(old, new, 1)}

    reject(
        lambda: extract(
            changed(
                ROCQ,
                "Hypothesis successful_match_has_product_prefix",
                "Hypothesis removed_prefix",
            )
        )
    )
    reject(
        lambda: extract(
            changed(
                ROCQ,
                "Theorem retained_bag_product_has_no_false_negative",
                "Theorem removed_product",
            )
        )
    )
    reject(
        lambda: extract(changed(ROCQ, "End RetainedBagProduct.", "End RemovedProduct."))
    )
    reject(
        lambda: extract(
            changed(
                SPEC,
                "#[test]\nfn automaton_selection_is_a_superset",
                "#[ignore]\n#[test]\nfn automaton_selection_is_a_superset",
            )
        )
    )
    reject(
        lambda: extract(
            changed(
                SPEC,
                "#[test]\nfn automaton_selection_is_a_superset",
                "#[test]\nfn duplicate_target() {}\n#[test]\nfn automaton_selection_is_a_superset",
            )
        )
    )
    reject(
        lambda: extract(
            changed(
                SPEC,
                "#[test]\nfn automaton_selection_is_a_superset",
                "#[test]\nfn exact_terminals_and_root_fallbacks_are_selected_by_bind_position() {}\n#[test]\nfn automaton_selection_is_a_superset",
            )
        )
    )
    reject(
        lambda: extract(
            changed(
                SPEC,
                "!directly_matches || selected[candidate_index]",
                "directly_matches || selected[candidate_index]",
            )
        )
    )
    reject(
        lambda: extract(
            changed(
                SPEC,
                "assert_eq!(selected, vec![true, false, true])",
                "assert_eq!(selected, vec![true, true, true])",
            )
        )
    )
    reject(
        lambda: extract(
            changed(
                RUNNER,
                "rocq_compile_with_closed_count theories/SpatialMatcher.v 10",
                "rocq_compile_with_closed_count theories/Removed.v 10",
            )
        )
    )
    reject(
        lambda: extract(
            changed(PACKAGE, 'name = "rspace_plus_plus"', 'name = "removed"')
        )
    )
    reject(
        lambda: extract(
            changed(
                AUTOMATON, "fn collect_product_prefix_terminals(", "fn removed_product("
            )
        )
    )


def check_manifest(expected, current):
    require(current == expected, "retained automaton registry is stale")


def main():
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    require(
        not list(
            (ROOT / "formal/tlaplus/stack_safe_pda").glob("*Retained*Automaton*.cfg")
        )
        and not list((ROOT / "formal/verus").glob("*retained*automaton*.rs")),
        "new dedicated formal source needs a binding",
    )
    sources = {
        path: (ROOT / path).read_text()
        for path in (ROCQ, RUNNER, PACKAGE, AUTOMATON, *EXPECTED_TESTS)
    }
    payload = json.dumps(extract(sources), indent=2, sort_keys=True) + "\n"
    if args.self_test:
        self_test(sources)
        reject(lambda: check_manifest(payload, payload + "stale"))
        print("retained automaton registry mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload)
    else:
        check_manifest(payload, (ROOT / OUTPUT).read_text())
        print("retained automaton registry current")


if __name__ == "__main__":
    main()
