import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/rmp2_obligations.generated.json"
ROCQ = "formal/rocq/stack_safe_pda/theories/RMP2.v"
VERUS = "formal/verus/rmp2_par_bag_transcript.rs"
STRUCTURE = "models/tests/match_path_structure.rs"
CURSOR = "models/tests/match_path_cursor.rs"
ORDER = "rspace++/src/rspace/data_index.rs"
CARGO = "models/Cargo.toml"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
ROCQ_CAPSTONES = [
    "emitted_opaque_par_token_has_exactly_49_bytes",
    "compiled_opaque_query_has_no_false_negative",
    "unsupported_opaque_shell_fails_closed",
    "canonical_candidate_order_is_independent_of_derived_rmp_bytes",
    "corresponding_semantic_fields_have_identical_projection",
    "adjacent_coalescing_preserves_exact_cardinality",
    "nested_par_projection_is_opaque_to_recursive_component_structure",
]
VERUS_DECLARATIONS = [
    ("project_resident_field", "open_spec"),
    ("project_region_field", "open_spec"),
    ("semantic_fields_correspond", "open_spec"),
    ("corresponding_semantic_fields_project_identically", "proof"),
    ("same_pending_occurrence", "open_spec"),
    ("executable_adjacent_coalescer_step", "verified_exec"),
    ("executable_finalize_adjacent_distinct", "verified_exec"),
    ("project_nested_par", "open_spec"),
    ("nested_par_projection_is_opaque_to_recursive_components", "proof"),
    ("frame_of", "open_spec"),
    ("framed_transcript", "open_spec"),
    ("records_correspond", "open_spec"),
    ("saturating_add_spec", "open_spec"),
    ("saturated_cardinality", "open_spec"),
    ("corresponding_sequences_are_extentionally_equal", "proof"),
    ("resident_and_region_accumulators_are_equivalent", "proof"),
    ("record_correspondence_precondition_is_non_vacuous", "proof"),
    ("executable_saturating_add", "verified_exec"),
    ("executable_occurrence_step", "verified_exec"),
    ("executable_exact_run_multiplicity_increment", "verified_exec"),
    ("executable_body_length", "verified_exec"),
]
WITNESSES = {
    "emitted_opaque_par_token_has_exactly_49_bytes": (
        CURSOR,
        "par_bag_token_is_the_fixed_49_byte_rmp2_summary",
        "assert_eq!(token.end - token.start, 1 + 8 + 8 + 32);",
    ),
    "canonical_candidate_order_is_independent_of_derived_rmp_bytes": (
        ORDER,
        "a_hash_collision_is_ordered_by_identity_not_the_derived_rho_path",
        "sort_encoded_candidates(&mut entries);",
    ),
    "corresponding_semantic_fields_have_identical_projection": (
        STRUCTURE,
        "resident_and_canonical_region_sources_have_identical_semantic_field_projection",
        "resident,\n            region.expect",
    ),
    "adjacent_coalescing_preserves_exact_cardinality": (
        STRUCTURE,
        "canonical_region_coalesces_only_adjacent_identical_occurrences",
        "vec![2, 3],",
    ),
    "nested_par_projection_is_opaque_to_recursive_component_structure": (
        STRUCTURE,
        "nested_par_projection_is_one_opaque_bag_token_plus_root_metadata",
        "assert_eq!(opaque.len(), 1);",
    ),
    "project_resident_field": (
        STRUCTURE,
        "resident_and_canonical_region_sources_have_identical_semantic_field_projection",
        "resident_semantic_projection(ParCursor::from(&stored))",
    ),
    "project_region_field": (
        STRUCTURE,
        "resident_and_canonical_region_sources_have_identical_semantic_field_projection",
        "region_semantic_projection(",
    ),
    "semantic_fields_correspond": (
        STRUCTURE,
        "resident_and_canonical_region_sources_have_identical_semantic_field_projection",
        "resident,\n            region.expect",
    ),
    "same_pending_occurrence": (
        STRUCTURE,
        "canonical_region_coalesces_only_adjacent_identical_occurrences",
        "vec![2, 3],",
    ),
    "executable_adjacent_coalescer_step": (
        STRUCTURE,
        "canonical_region_coalesces_only_adjacent_identical_occurrences",
        "vec![2, 3],",
    ),
    "executable_finalize_adjacent_distinct": (
        STRUCTURE,
        "canonical_region_coalesces_only_adjacent_identical_occurrences",
        "assert_eq!(resident_records.len(), 2);",
    ),
    "project_nested_par": (
        STRUCTURE,
        "nested_par_projection_is_one_opaque_bag_token_plus_root_metadata",
        'vec!["Expr", "EList", "Par"],',
    ),
    "executable_occurrence_step": (
        STRUCTURE,
        "canonical_region_coalesces_only_adjacent_identical_occurrences",
        "region_occurrences, expanded_resident,",
    ),
    "executable_exact_run_multiplicity_increment": (
        STRUCTURE,
        "canonical_region_coalesces_only_adjacent_identical_occurrences",
        "vec![2, 3],",
    ),
}
REQUIRED_TEST_FRAGMENTS = {
    "a_hash_collision_is_ordered_by_identity_not_the_derived_rho_path": (
        "entries[0].1.path < entries[1].1.path",
        'b"a-identity");',
        'b"z-identity");',
    ),
    "resident_and_canonical_region_sources_have_identical_semantic_field_projection": (
        "par_corpus::every_expr_instance().len()",
        "resident_semantic_projection(ParCursor::from(&stored))",
        "region_semantic_projection(",
    ),
    "canonical_region_coalesces_only_adjacent_identical_occurrences": (
        "region_occurrences, expanded_resident,",
        "par_bag_tokens(&encode_par_match_atom(&carrier))",
        "assert_eq!(resident_records.len(), 2);",
    ),
    "nested_par_projection_is_one_opaque_bag_token_plus_root_metadata": (
        "assert_eq!(resident, region);",
        'vec!["Expr", "EList", "Par"],',
        "par_bag_tokens(&changed),",
        "assert_ne!(",
    ),
}


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def test_body(source, name):
    matches = list(re.finditer(rf"(?m)^[ \t]*fn {re.escape(name)}\(\)", source))
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
    next_test = re.search(r"(?m)^[ \t]*#\[test\]", source[start + 1 :])
    end = start + 1 + next_test.start() if next_test else len(source)
    return source[start:end]


def extract(sources, mutant_names, tla_names):
    require(not mutant_names, "unregistered RMP2 formal mutant")
    require(not tla_names, "unregistered RMP2 TLC source")
    rocq = sources[ROCQ]
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
    runner = re.sub(r"\\\n", " ", sources[RUNNER])
    require(
        "theories/RMP2.v" in runner and "rmp2_closed_capstones" in runner,
        "RMP2 Rocq proof absent from runner",
    )
    require(
        'if [[ "$rmp2_closed_capstones" != "7" ]]' in runner,
        "RMP2 seven-closed-capstone gate absent",
    )
    require(
        "--no-cheating --num-threads 1" in runner and VERUS in runner,
        "RMP2 Verus no-cheating run absent",
    )
    verus = sources[VERUS]
    body = verus.split("\n}\n\nfn main()", 1)
    require(len(body) == 2, "Verus proof block or main changed")
    declarations = re.findall(
        r"(?m)^\s*(?:pub[ \t]+)?(?:(open spec|proof)[ \t]+)?fn[ \t]+([A-Za-z][A-Za-z0-9_]*)[ \t]*\(",
        body[0],
    )
    observed = [
        (name, kind.replace(" ", "_") if kind else "verified_exec")
        for kind, name in declarations
    ]
    require(
        observed == VERUS_DECLARATIONS,
        "missing, extra, reordered, or stale Verus declaration",
    )
    for name in ("match_path_structure", "match_path_cursor"):
        require(
            re.search(
                rf'(?s)\[\[test\]\]\s*name = "{name}"\s*path = "tests/{name}\.rs"',
                sources[CARGO],
            )
            is not None,
            f"normal models test target absent: {name}",
        )
    require(
        len(set(WITNESSES) - ({name for name, _ in observed} | set(prints))) == 0,
        "witness bound to unconfigured obligation",
    )
    for symbol, (path, test, fragment) in WITNESSES.items():
        require(
            fragment in test_body(sources[path], test),
            f"stale executable assertion: {symbol}",
        )
        for required in REQUIRED_TEST_FRAGMENTS.get(test, ()):
            require(
                required in test_body(sources[path], test),
                f"stale executable assertion: {symbol}: {required}",
            )
    require(
        "#[cfg(test)]\nmod candidate_identity_order_tests {" in sources[ORDER],
        "candidate identity unit-test module absent",
    )
    require(
        'assert_eq!(&entries[0].1.path[entries[0].1.identity_offset..], b"a-identity");'
        in test_body(
            sources[ORDER],
            "a_hash_collision_is_ordered_by_identity_not_the_derived_rho_path",
        ),
        "stale identity-order assertion",
    )
    return {
        "tla_obligations": [],
        "causal_mutants": [],
        "rocq_print_assumptions": [
            {
                "id": f"RMP2-ROCQ-{index:02d}",
                "symbol": symbol,
                "classification": "executable_refinement"
                if symbol in WITNESSES
                else "proof_only",
                "target_source": WITNESSES[symbol][0] if symbol in WITNESSES else None,
                "target": WITNESSES[symbol][1] if symbol in WITNESSES else None,
                "assertion": WITNESSES[symbol][2] if symbol in WITNESSES else None,
            }
            for index, symbol in enumerate(prints, 1)
        ],
        "verus_obligations": [
            {
                "id": f"RMP2-VERUS-{index:02d}",
                "symbol": symbol,
                "kind": kind,
                "classification": "executable_refinement"
                if symbol in WITNESSES
                else "proof_only",
                "target_source": WITNESSES[symbol][0] if symbol in WITNESSES else None,
                "target": WITNESSES[symbol][1] if symbol in WITNESSES else None,
                "assertion": WITNESSES[symbol][2] if symbol in WITNESSES else None,
            }
            for index, (symbol, kind) in enumerate(observed, 1)
        ],
        "scope_limit": "No RMP2-specific TLA config or formal mutant is accepted in this family. Direct executable rows bind normal Rust assertions for concrete projection, coalescing, token size, opacity, and candidate order; they do not verify BLAKE2b, protobuf parsing, or symbolic Verus proof steps. Other obligations are proof-only. Source wiring is checked, not Rocq/Verus/Cargo execution.",
        "source_sha256": {
            path: hashlib.sha256(source.encode()).hexdigest()
            for path, source in sorted(sources.items())
            if path not in (RUNNER, CARGO)
        },
    }


def reject(sources, mutants, tla, path, before, after, expected):
    changed = dict(sources)
    require(before in changed[path], f"self-test setup absent: {before}")
    changed[path] = changed[path].replace(before, after, 1)
    try:
        extract(changed, mutants, tla)
    except RegistryError as error:
        require(expected in str(error), f"wrong mutation rejection: {error}")
        return
    raise RegistryError(f"mutation unexpectedly passed: {expected}")


def self_test(sources, mutants, tla):
    reject(
        sources,
        mutants,
        tla,
        ROCQ,
        "Print Assumptions emitted_opaque_par_token_has_exactly_49_bytes.\n",
        "",
        "missing, extra, or reordered Rocq capstone",
    )
    reject(
        sources,
        mutants,
        tla,
        ROCQ,
        "Print Assumptions emitted_opaque_par_token_has_exactly_49_bytes.\n",
        "Print Assumptions emitted_opaque_par_token_has_exactly_49_bytes.\nPrint Assumptions emitted_opaque_par_token_has_exactly_49_bytes.\n",
        "missing, extra, or reordered Rocq capstone",
    )
    reject(
        sources,
        mutants,
        tla,
        VERUS,
        "pub fn executable_occurrence_step(",
        "pub fn stale_occurrence_step(",
        "missing, extra, reordered, or stale Verus declaration",
    )
    reject(
        sources,
        mutants,
        tla,
        STRUCTURE,
        "#[test]\nfn canonical_region_coalesces_only_adjacent_identical_occurrences",
        "#[test]\n#[ignore]\nfn canonical_region_coalesces_only_adjacent_identical_occurrences",
        "ignored-only target",
    )
    reject(
        sources,
        mutants,
        tla,
        CURSOR,
        "assert_eq!(token.end - token.start, 1 + 8 + 8 + 32);",
        "assert_eq!(token.end - token.start, 0);",
        "stale executable assertion",
    )
    reject(
        sources,
        mutants,
        tla,
        ORDER,
        'b"a-identity");',
        'b"wrong-identity");',
        "stale executable assertion",
    )
    reject(
        sources,
        mutants,
        tla,
        CARGO,
        'name = "match_path_structure"',
        'name = "stale_path_structure"',
        "normal models test target absent",
    )
    reject(
        sources,
        mutants,
        tla,
        RUNNER,
        'if [[ "$rmp2_closed_capstones" != "7" ]]',
        'if [[ "$rmp2_closed_capstones" != "0" ]]',
        "seven-closed-capstone gate absent",
    )
    try:
        extract(sources, mutants | {"RMP2UnexpectedMutant.cfg"}, tla)
    except RegistryError as error:
        require(
            "unregistered RMP2 formal mutant" in str(error),
            f"wrong mutant inventory rejection: {error}",
        )
    else:
        raise RegistryError("unregistered RMP2 formal mutant passed")


def main():
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    paths = {ROCQ, VERUS, STRUCTURE, CURSOR, ORDER, CARGO, RUNNER}
    sources = {
        path: (ROOT / path).read_text(encoding="utf-8") for path in sorted(paths)
    }
    formal = ROOT / "formal"
    mutants = {path.name for path in formal.rglob("*RMP2*Mutant*")}
    tla = {path.name for path in formal.rglob("*RMP2*.cfg")} | {
        path.name for path in formal.rglob("*RMP2*.tla")
    }
    payload = (
        json.dumps(extract(sources, mutants, tla), indent=2, sort_keys=True) + "\n"
    )
    if args.self_test:
        self_test(sources, mutants, tla)
        print("RMP2 registry: mutation controls passed")
    elif args.write:
        (ROOT / OUTPUT).write_text(payload, encoding="utf-8")
        print("RMP2 registry: written")
    else:
        require(
            (ROOT / OUTPUT).read_text(encoding="utf-8") == payload,
            "RMP2 registry is stale",
        )
        print("RMP2 registry: current")


if __name__ == "__main__":
    main()
