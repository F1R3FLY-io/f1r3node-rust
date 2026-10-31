"""Check the exact ordered receive byte view proof, mutant and executable witness inventory."""

import argparse
import ast
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = "formal/ordered_receive_view_obligations.generated.json"
ROCQ = "formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceContinuationAuthority.v"
MUTANTS = "formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceContinuationAuthorityMutants.v"
VIEW = "rspace++/src/rspace/ordered_receive.rs"
PATHS = "rspace++/src/rspace/rho_path.rs"
SPEC = "rspace++/tests/ordered_receive_view_spec.rs"
MODELS = "models/tests/cold_store_records.rs"
RHOLANG = "rholang/tests/ordered_receive_view_rholang.rs"
GENERATOR = "scripts/generate-ordered-receive-view-mutant.py"
RSPACE_CARGO = "rspace++/Cargo.toml"
MODELS_CARGO = "models/Cargo.toml"
RHOLANG_CARGO = "rholang/Cargo.toml"
RUNNER = "scripts/check-stack-safe-pda-proofs.sh"
UNHASHED = (RSPACE_CARGO, MODELS_CARGO, RHOLANG_CARGO, RUNNER)
FLAG = "--ordered-receive-view"
ROCQ_MODULE = "OrderedReceiveByteView"
MUTANT_MODULE = "OrderedReceiveByteViewMutants"
BRIDGE = "production_checks_are_admit_record"
CLOSED_REPORTS = {ROCQ: 54, MUTANTS: 47}
MODELS_FILTER = "ordered_receive"
SUITE_COUNTS = {VIEW: 2, SPEC: 13, MODELS: 6, RHOLANG: 4}

THEOREMS = re.findall(
    r"\S+",
    """
encoded_record_admits_exact_fields admitted_receive_refuses_mismatched_arity
admitted_bounds_are_ordered_and_inside_record admitted_peek_tail_is_checked_exactly
admitted_peeks_are_canonical_signed admitted_persist_is_boolean
admitted_view_binds_refine_bind_at view_positions_are_exact_image_positions
prefix_related_images_are_not_aliases cross_profile_query_is_refused
empty_aligned_view_has_no_bind_or_position
source_suffix_is_exact_and_identity_is_source_free
first_view_identity_retains_head_source_suffix free_count_is_trailing_signed_word
wide_view_position_does_not_wrap
""",
)
MUTANT_EXAMPLES = re.findall(
    r"\S+",
    """
zip_truncating_arity_is_detected wrapping_peek_tail_is_detected
narrowed_count_alias_is_detected shifted_capture_bound_is_detected
source_bearing_identity_is_detected misplaced_source_split_is_detected
normalized_peek_tail_is_detected wrapping_signed_position_is_detected
unframed_prefix_match_is_detected profile_blind_query_is_detected
universal_nonempty_rule_is_detected noncanonical_persist_is_detected
""",
)

DIRECT = {
    "encoded_record_admits_exact_fields": (SPEC, "prepared_record_bytes_equal_the_frozen_encoders"),
    "admitted_receive_refuses_mismatched_arity": (
        SPEC,
        "admission_refuses_mismatched_arity_before_any_position",
    ),
    "admitted_bounds_are_ordered_and_inside_record": (
        SPEC,
        "malformed_field_bounds_and_peek_tails_are_refused",
    ),
    "admitted_peek_tail_is_checked_exactly": (SPEC, "peek_count_overflow_is_checked_without_wrap"),
    "admitted_peeks_are_canonical_signed": (
        SPEC,
        "malformed_field_bounds_and_peek_tails_are_refused",
    ),
    "admitted_persist_is_boolean": (SPEC, "malformed_field_bounds_and_peek_tails_are_refused"),
    "admitted_view_binds_refine_bind_at": (
        SPEC,
        "view_binds_align_channel_pattern_and_effective_peek_by_position",
    ),
    "view_positions_are_exact_image_positions": (
        SPEC,
        "rci2_views_return_exact_positions_over_explicit_images",
    ),
    "prefix_related_images_are_not_aliases": (
        SPEC,
        "legacy_store_views_return_exact_ordered_positions_for_repeated_and_prefix_related_channels",
    ),
    "cross_profile_query_is_refused": (SPEC, "profile_queries_are_explicit"),
    "empty_aligned_view_has_no_bind_or_position": (
        SPEC,
        "empty_aligned_view_has_no_bind_and_v2_empty_group_stays_refused",
    ),
    "source_suffix_is_exact_and_identity_is_source_free": (
        SPEC,
        "prepared_record_bytes_equal_the_frozen_encoders",
    ),
    "first_view_identity_retains_head_source_suffix": (
        SPEC,
        "first_representative_and_exact_source_suffix_are_retained",
    ),
    "free_count_is_trailing_signed_word": (
        MODELS,
        "ordered_receive_view_admits_every_corpus_pattern_and_continuation",
    ),
    "wide_view_position_does_not_wrap": (VIEW, "wide_positions_convert_without_wrap"),
}
EVIDENCE = {
    "encoded_record_admits_exact_fields": (
        "prop_assert_eq!(prepared.field_bounds().ends().len(), patterns.len() + 2);",
        "prop_assert_eq!(view.body(), body_bytes.as_slice());",
        "prop_assert_eq!(view.persist(), persist);",
    ),
    "admitted_receive_refuses_mismatched_arity": (
        "assert_eq!(put_admissions, vec![Err(OrderedReceiveError::ArityMismatch {",
        "assert_eq!(hydrated_admissions, vec![Err(OrderedReceiveError::ArityMismatch {",
    ),
    "admitted_bounds_are_ordered_and_inside_record": (
        "(vec![ends[1], ends[0], body_end, identity_end], OrderedReceiveError::MalformedFieldBounds),",
        "(canonical[..identity_end - 1].to_vec(), Err(OrderedReceiveError::TruncatedRecord)),",
    ),
    "admitted_peek_tail_is_checked_exactly": (
        "(1u64 << 62, OrderedReceiveError::PeekCountOverflow),",
        "(largest_fitting, OrderedReceiveError::PeekTailMismatch),",
    ),
    "admitted_peeks_are_canonical_signed": (
        "(rewritten(peeks_start, &signed_words(3, 3)), Err(OrderedReceiveError::NonCanonicalPeeks)),",
        "(rewritten(peeks_start, &signed_words(-1, 1)), Ok(())),",
    ),
    "admitted_persist_is_boolean": (
        "(rewritten(body_end, &[2]), Err(OrderedReceiveError::InvalidPersist(2))),",
    ),
    "admitted_view_binds_refine_bind_at": (
        "assert_eq!(view.bind_at(bind.position()), Some(*bind));",
        "assert_eq!(bind.effective_peek(), index == 2);",
        "assert_eq!(bind.free_count::<TrailingFreeCount>(), Some(patterns[index].free_count));",
    ),
    "view_positions_are_exact_image_positions": (
        "for arity in [65usize, 257, 4097] {",
        "oracle_positions(&channels, query),",
    ),
    "prefix_related_images_are_not_aliases": (
        "for group in [[first, second, first], [first, first, second]] {",
        "oracle_positions(&group, query),",
    ),
    "cross_profile_query_is_refused": (
        'assert_eq!(legacy_positions, expected, "legacy query {query:?}");',
        'assert_eq!(rci2_positions, expected, "RCI2 query {query:?}");',
        "Err(OrderedReceiveError::ProfileMismatch)",
    ),
    "empty_aligned_view_has_no_bind_or_position": (
        "assert_eq!(view.binds().len(), 0);",
        "assert_eq!(view.record().pattern_at(0), None);",
    ),
    "source_suffix_is_exact_and_identity_is_source_free": (
        "prop_assert_eq!(view.source_suffix(), source.as_slice());",
        "prop_assert_eq!(view.identity(), &canonical[..view.identity().len()]);",
    ),
    "first_view_identity_retains_head_source_suffix": (
        "assert_eq!(hot_store.put_continuation(&channels, alias), Some(false));",
        "assert_eq!(retained, vec![(first_prepared.record_view().identity().to_vec(), frozen_source)]);",
    ),
    "free_count_is_trailing_signed_word": (
        "let free_counts = vec![Some(pattern.free_count); channels.len()];",
        'assert_eq!(observed.free_counts, free_counts, "{name}");',
    ),
    "wide_view_position_does_not_wrap": (
        "assert_eq!(position.signed_peek_key(), None);",
        "assert!(!view.effective_peek(position));",
    ),
}
SOURCE_MUTANT_EVIDENCE = {
    "zip_truncating_arity": ("vec![Err(OrderedReceiveError::ArityMismatch {",),
    "wrapping_peek_tail": ("(1u64 << 62, OrderedReceiveError::PeekCountOverflow),",),
    "narrowed_count_alias": (
        "let aliased_count = ((1u64 << 32) + 2).to_le_bytes();",
        "(rewritten(0, &aliased_count), Err(OrderedReceiveError::MalformedFieldBounds)),",
    ),
    "shifted_capture_bound": (
        "assert_eq!(bind.pattern(), patterns[index].stable_hash_bytes().as_slice());",
    ),
    "source_bearing_identity": (
        "assert_eq!(first_prepared.record_view().identity(), alias_prepared.record_view().identity());",
    ),
    "misplaced_source_split": ("prop_assert_eq!(view.source_suffix(), source.as_slice());",),
    "normalized_peek_tail": (
        "(rewritten(peeks_start, &signed_words(5, 1)), Err(OrderedReceiveError::NonCanonicalPeeks)),",
    ),
    "wrapping_signed_position": ("assert_eq!(position.signed_peek_key(), None);",),
    "unframed_prefix_match": ("oracle_positions(&group, query),",),
    "profile_blind_query": ("Err(OrderedReceiveError::ProfileMismatch)",),
    "universal_nonempty_rule": ("assert_eq!(view.record().arity(), 0);",),
    "noncanonical_persist": (
        "(rewritten(body_end, &[0xFF]), Err(OrderedReceiveError::InvalidPersist(0xFF))),",
    ),
}
TEST_ATTRIBUTE = re.compile(r"^#\[(?:tokio::)?test\b")
TEST_FUNCTION = re.compile(r"^(?:async[ \t]+)?fn[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*\(")


class RegistryError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RegistryError(message)


def module_text(source, module, label):
    start = re.search(rf"(?m)^Module {re.escape(module)}\.$", source)
    if start is None:
        raise RegistryError(f"{label} lacks module {module}")
    end = re.search(rf"(?m)^End {re.escape(module)}\.$", source[start.end() :])
    if end is None:
        raise RegistryError(f"{label} module {module} is not closed")
    return source[start.end() : start.end() + end.start()]


def names(source, pattern, label):
    found = re.findall(pattern, source, flags=re.MULTILINE)
    require(len(found) == len(set(found)), f"duplicate {label}")
    return found


def test_scope(path, source):
    if path != VIEW:
        return source
    start = re.search(r"(?m)^#\[cfg\(test\)\]\nmod tests \{$", source)
    if start is None:
        raise RegistryError("ordered_receive.rs lacks its unit-test module")
    return source[start.end() :]


def tests_in(source, label):
    found = []
    attributes = []
    for line in source.splitlines():
        stripped = line.strip()
        if stripped.startswith("#["):
            attributes.append(stripped)
            continue
        function = TEST_FUNCTION.match(stripped)
        if function and any(TEST_ATTRIBUTE.match(attribute) for attribute in attributes):
            require(
                not any(attribute.startswith("#[ignore") for attribute in attributes),
                f"ignored-only test {function.group(1)} in {label}",
            )
            require(
                not any(attribute.startswith(("#[cfg(", "#[cfg_attr(")) for attribute in attributes),
                f"conditionally absent test {function.group(1)} in {label}",
            )
            found.append(function.group(1))
        if stripped:
            attributes = []
    require(len(found) == len(set(found)), f"duplicate test name in {label}")
    return found


def test_body(source, name):
    match = re.search(
        rf"(?m)^[ \t]*(?:async[ \t]+)?fn[ \t]+{re.escape(name)}[ \t]*\(", source
    )
    if match is None:
        raise RegistryError(f"missing executable witness {name}")
    following = re.search(r"(?m)^[ \t]*#\[(?:tokio::)?test\b", source[match.end() :])
    end = match.end() + following.start() if following else len(source)
    return source[match.start() : end]


def generator_mutations(text):
    module = ast.parse(text)
    constants = {}
    table = None
    for node in module.body:
        if not (
            isinstance(node, ast.Assign)
            and len(node.targets) == 1
            and isinstance(node.targets[0], ast.Name)
        ):
            continue
        name = node.targets[0].id
        if name == "MUTATIONS":
            table = node.value
        elif isinstance(node.value, ast.Constant) and isinstance(node.value.value, str):
            constants[name] = node.value.value
    if table is None:
        raise RegistryError("mutant generator has no MUTATIONS table")

    class Resolve(ast.NodeTransformer):
        def visit_Name(self, node):
            require(node.id in constants, f"mutant generator names unknown constant {node.id}")
            return ast.copy_location(ast.Constant(constants[node.id]), node)

    return ast.literal_eval(Resolve().visit(table))


def paths():
    return [ROCQ, MUTANTS, VIEW, PATHS, SPEC, MODELS, RHOLANG, GENERATOR, *UNHASHED]


def runner_test_line(runner, helper, count, *arguments):
    tail = r"[ \t]+".join(re.escape(argument) for argument in arguments)
    return re.search(rf"{helper}[ \t]+{count}[ \t]+(?:[^\n]*?[ \t]+)?{tail}(?:[ \t]|$)", runner, re.M)


def extract(sources):
    rocq = sources[ROCQ]
    mutants = sources[MUTANTS]
    runner = re.sub(r"\\\n", " ", sources[RUNNER])

    for path in (ROCQ, MUTANTS):
        require(
            not re.search(r"\b(?:Admitted|admit|Axiom|Axioms|Parameter|Parameters)\b", sources[path]),
            f"{path} contains an admitted or axiomatic declaration",
        )
    view_module = module_text(rocq, ROCQ_MODULE, ROCQ)
    theorems = names(
        view_module,
        r"^Theorem[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*:",
        f"{ROCQ_MODULE} theorem",
    )
    require(theorems == THEOREMS, f"missing, added, or reordered {ROCQ_MODULE} theorem")
    printed = names(
        view_module,
        r"^Print Assumptions[ \t]+([A-Za-z_][A-Za-z_0-9.]*)\.$",
        f"{ROCQ_MODULE} closed-assumption target",
    )
    require(printed == THEOREMS, f"missing, added, or reordered {ROCQ_MODULE} closure")

    mutant_module = module_text(mutants, MUTANT_MODULE, MUTANTS)
    examples = names(
        mutant_module,
        r"^Example[ \t]+([A-Za-z_][A-Za-z_0-9]*)[ \t]*:",
        f"{MUTANT_MODULE} example",
    )
    require(examples == MUTANT_EXAMPLES, f"missing, added, or reordered {MUTANT_MODULE} example")
    require(
        len(re.findall(rf"(?m)^Lemma[ \t]+{BRIDGE}[ \t]*:", mutant_module)) == 1,
        f"{MUTANT_MODULE} lacks its production-check bridge",
    )
    mutant_prints = names(
        mutant_module,
        r"^Print Assumptions[ \t]+([A-Za-z_][A-Za-z_0-9.]*)\.$",
        f"{MUTANT_MODULE} closed-assumption target",
    )
    require(
        mutant_prints == [BRIDGE, *MUTANT_EXAMPLES],
        f"missing, added, or reordered {MUTANT_MODULE} closure",
    )

    for path, expected in CLOSED_REPORTS.items():
        theory = path.split("stack_safe_pda/", 1)[1]
        require(
            len(
                re.findall(
                    rf"rocq_compile_with_closed_count[ \t]+{re.escape(theory)}[ \t]+{expected}[ \t]",
                    runner,
                )
            )
            == 2,
            f"{theory} {expected}-closure compilation absent from the normal or focused proof runner",
        )

    start = runner.find(f'if [[ "${{1:-}}" == "{FLAG}" ]]; then')
    require(start >= 0, f"focused {FLAG} gate absent from the proof runner")
    finish = runner.find("\nfi\n", start)
    require(finish > start, f"focused {FLAG} gate is not closed")
    focused = runner[start:finish]
    for theory in ("TwoLevelRSpaceContinuationAuthority", "TwoLevelRSpaceContinuationAuthorityMutants"):
        require(
            re.search(rf"rocq_reject_placeholders[^\n]*theories/{theory}\.v", focused),
            f"focused gate does not reject placeholders in {theory}.v",
        )
        require(
            re.search(rf"rocq_check_modules[^\n]*StackSafePDA\.{theory}(?:[ \t]|$)", focused, re.M),
            f"focused gate does not kernel-check {theory}",
        )

    for path, cargo in ((SPEC, RSPACE_CARGO), (RHOLANG, RHOLANG_CARGO)):
        require(
            not re.search(r"(?m)^autotests[ \t]*=[ \t]*false\b", sources[cargo]),
            f"{path} absent from Cargo's normal test discovery",
        )
    require(
        re.search(
            r'(?m)^\[\[test\]\]\nname = "cold_store_records"\npath = "tests/cold_store_records\.rs"$',
            sources[MODELS_CARGO],
        ),
        "cold_store_records is not a registered models test target",
    )

    suites = {}
    for path in (VIEW, SPEC, MODELS, RHOLANG):
        scope = test_scope(path, sources[path])
        found = tests_in(scope, path)
        if path == MODELS:
            found = [name for name in found if MODELS_FILTER in name]
        require(len(found) == SUITE_COUNTS[path], f"{path} normal-suite count changed")
        suites[path] = found
    require(
        runner_test_line(focused, "rspace_preparation_run_tests", SUITE_COUNTS[VIEW], "--lib", "rspace::ordered_receive::"),
        "focused gate does not run the view's unit tests with their count",
    )
    require(
        runner_test_line(focused, "rspace_preparation_run_tests", SUITE_COUNTS[SPEC], "--test", "ordered_receive_view_spec"),
        "focused gate does not run ordered_receive_view_spec with its count",
    )
    require(
        runner_test_line(focused, "rpb3_run_tests", SUITE_COUNTS[MODELS], "--test", "cold_store_records", MODELS_FILTER),
        "focused gate does not run the models ordered-receive tests with their count",
    )
    require(
        runner_test_line(focused, "rholang_run_tests", SUITE_COUNTS[RHOLANG], "--test", "ordered_receive_view_rholang"),
        "focused gate does not run ordered_receive_view_rholang with its count",
    )
    aggregate = runner.find('python3 "$repo_root/scripts/build-pathmap-obligation-registry.py" --check')
    require(0 <= aggregate < start, "the aggregate registry check does not precede the focused gate")
    invocation = 'python3 "$repo_root/scripts/check-ordered-receive-view-obligations.py"'
    require(
        runner.count(f"{invocation} --check") == 1 and f"{invocation} --check" not in focused,
        "ordered receive view registry check absent from, or duplicated in, the normal proof runner",
    )
    require(
        runner.count(f"{invocation} --self-test") == 2 and f"{invocation} --self-test" in focused,
        "ordered receive view registry mutations absent from the normal or focused proof runner",
    )

    require(set(DIRECT) == set(THEOREMS), "every capstone needs exactly one executable witness")
    require(set(EVIDENCE) == set(DIRECT), "unbound ordered receive witness evidence")
    for theorem, (path, target) in DIRECT.items():
        require(target in suites[path], f"{theorem} witness {target} is not a normal-suite test")
        body = test_body(test_scope(path, sources[path]), target)
        require(
            all(fragment in body for fragment in EVIDENCE[theorem]),
            f"{theorem} no longer has its concrete assertion in {target}",
        )

    mutations = generator_mutations(sources[GENERATOR])
    require(
        [mutation["rocq"] for mutation in mutations.values()] == MUTANT_EXAMPLES,
        "source mutants do not correspond one-to-one, in order, to the Rocq mutants",
    )
    require(set(SOURCE_MUTANT_EVIDENCE) == set(mutations), "unbound source-mutant evidence")
    source_rows = []
    for index, (name, mutation) in enumerate(mutations.items(), 1):
        require(mutation["source"] in (VIEW, PATHS), f"{name} mutates an unregistered source")
        require(
            sources[mutation["source"]].count(mutation["before"]) == 1,
            f"{name} source anchor is absent or ambiguous in {mutation['source']}",
        )
        require(mutation["before"] != mutation["after"], f"{name} is not a mutation")
        require(mutation["test_file"] in suites, f"{name} names an unregistered test file")
        require(
            mutation["test"] in suites[mutation["test_file"]],
            f"{name} killing test {mutation['test']} is not a normal-suite test",
        )
        body = test_body(test_scope(mutation["test_file"], sources[mutation["test_file"]]), mutation["test"])
        require(
            all(fragment in body for fragment in SOURCE_MUTANT_EVIDENCE[name]),
            f"{name} killing test {mutation['test']} lost its assertion",
        )
        source_rows.append(
            {
                "id": f"ORV-RS-{index:02d}",
                "mutation": name,
                "rocq_mutant": mutation["rocq"],
                "source": mutation["source"],
                "description": mutation["description"],
                "mode": "executable_refinement",
                "test_source": mutation["test_file"],
                "test_target": mutation["test"],
            }
        )
    by_rocq = {row["rocq_mutant"]: row for row in source_rows}

    return {
        "scope": "Ordered receive byte view admission, positions, binds and resting-record store views (design 8009)",
        "boundary": "Concrete witnesses assert each capstone on fixtures and generated records; the source mutants show the production checks observe each property. Neither is a formal Rust refinement proof, and the RCI2 profile is admitted only from explicit keys.",
        "sources_sha256": {
            path: hashlib.sha256(sources[path].encode()).hexdigest()
            for path in paths()
            if path not in UNHASHED
        },
        "rocq": {
            "source": ROCQ,
            "module": ROCQ_MODULE,
            "closed_reports": CLOSED_REPORTS[ROCQ],
            "print_assumptions": printed,
            "obligations": [
                {
                    "id": f"ORV-RC-{index:02d}",
                    "obligation": name,
                    "mode": "concrete_witness",
                    "test_source": DIRECT[name][0],
                    "test_target": DIRECT[name][1],
                }
                for index, name in enumerate(theorems, 1)
            ],
        },
        "mutants": {
            "source": MUTANTS,
            "module": MUTANT_MODULE,
            "closed_reports": CLOSED_REPORTS[MUTANTS],
            "bridge": BRIDGE,
            "print_assumptions": mutant_prints,
            "obligations": [
                {
                    "id": f"ORV-MUT-{index:02d}",
                    "obligation": name,
                    "mode": "concrete_witness",
                    "source_mutant": by_rocq[name]["id"],
                    "test_source": by_rocq[name]["test_source"],
                    "test_target": by_rocq[name]["test_target"],
                }
                for index, name in enumerate(examples, 1)
            ],
        },
        "source_mutants": {"generator": GENERATOR, "obligations": source_rows},
        "normal_suite": {
            "runner_flag": FLAG,
            "models_filter": MODELS_FILTER,
            "tests": {path: suites[path] for path in (VIEW, SPEC, MODELS, RHOLANG)},
        },
    }


def self_test(sources, baseline):
    def rejected(path, replacement, label, reason):
        require(replacement != sources[path], f"self-test mutation did not apply: {label}")
        changed = dict(sources)
        changed[path] = replacement
        try:
            extract(changed)
        except RegistryError as error:
            require(reason in str(error), f"{label} was refused for another reason: {error}")
            return
        raise RegistryError(f"mutation unexpectedly accepted: {label}")

    view_anchor = "        let finish = *self.pattern_ends.get(index)?;\n"
    invocation = 'python3 "$repo_root/scripts/check-ordered-receive-view-obligations.py"'
    rejected(
        ROCQ,
        sources[ROCQ].replace(
            "Theorem cross_profile_query_is_refused :",
            "Theorem stale_cross_profile_query_is_refused :",
            1,
        ),
        "renamed capstone",
        f"reordered {ROCQ_MODULE} theorem",
    )
    rejected(
        ROCQ,
        sources[ROCQ].replace("Print Assumptions wide_view_position_does_not_wrap.\n", "", 1),
        "omitted capstone closure",
        f"reordered {ROCQ_MODULE} closure",
    )
    rejected(
        ROCQ,
        sources[ROCQ].replace(
            "Print Assumptions wide_view_position_does_not_wrap.\n",
            "Axiom wide_view_shortcut : True.\nPrint Assumptions wide_view_position_does_not_wrap.\n",
            1,
        ),
        "axiomatic capstone dependency",
        "admitted or axiomatic declaration",
    )
    rejected(
        MUTANTS,
        sources[MUTANTS].replace("Print Assumptions noncanonical_persist_is_detected.\n", "", 1),
        "omitted mutant closure",
        f"reordered {MUTANT_MODULE} closure",
    )
    rejected(
        MUTANTS,
        sources[MUTANTS].replace(
            "Example profile_blind_query_is_detected :", "Example profile_query_is_detected :", 1
        ),
        "renamed Rocq mutant",
        f"reordered {MUTANT_MODULE} example",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            "theories/TwoLevelRSpaceContinuationAuthorityMutants.v 47",
            "theories/TwoLevelRSpaceContinuationAuthorityMutants.v 46",
            1,
        ),
        "wrong runner closed count",
        "TwoLevelRSpaceContinuationAuthorityMutants.v 47-closure compilation absent",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(f"{invocation} --check\n", "", 1),
        "missing normal registry check",
        "registry check absent",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(f"{invocation} --self-test\n", "", 1),
        "missing focused registry mutations",
        "registry mutations absent",
    )
    rejected(
        RUNNER,
        sources[RUNNER].replace(
            "rspace_preparation_run_tests 13 --test ordered_receive_view_spec",
            "rspace_preparation_run_tests 12 --test ordered_receive_view_spec",
            1,
        ),
        "wrong focused test count",
        "does not run ordered_receive_view_spec with its count",
    )
    rejected(
        SPEC,
        sources[SPEC].replace(
            "fn profile_queries_are_explicit(", "#[ignore]\nfn profile_queries_are_explicit(", 1
        ),
        "ignored-only Rust witness",
        "ignored-only test profile_queries_are_explicit",
    )
    rejected(
        SPEC,
        sources[SPEC].replace("(1u64 << 62, OrderedReceiveError::PeekCountOverflow),", "", 1),
        "removed concrete overflow assertion",
        "admitted_peek_tail_is_checked_exactly no longer has its concrete assertion",
    )
    rejected(
        SPEC,
        sources[SPEC].replace(
            "(rewritten(0, &aliased_count), Err(OrderedReceiveError::MalformedFieldBounds)),", "", 1
        ),
        "removed source-mutant killing assertion",
        "narrowed_count_alias killing test",
    )
    rejected(
        VIEW,
        sources[VIEW].replace(
            view_anchor,
            "        let finish = *self.pattern_ends.get(index).expect(\"bounded\");\n",
            1,
        ),
        "stale source-mutant anchor",
        "shifted_capture_bound source anchor is absent or ambiguous",
    )
    rejected(
        VIEW,
        sources[VIEW].replace(view_anchor, view_anchor + view_anchor, 1),
        "duplicated source-mutant anchor",
        "shifted_capture_bound source anchor is absent or ambiguous",
    )
    rejected(
        GENERATOR,
        sources[GENERATOR].replace(
            '"rocq": "wrapping_peek_tail_is_detected"', '"rocq": "wrapping_tail_is_detected"', 1
        ),
        "source mutant bound to a missing Rocq mutant",
        "do not correspond one-to-one",
    )
    require(extract(sources) == baseline, "nondeterministic ordered receive view registry")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    group.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    sources = {path: (ROOT / path).read_text() for path in paths()}
    inventory = extract(sources)
    rendered = json.dumps(inventory, indent=2, sort_keys=True) + "\n"
    if args.write:
        (ROOT / OUTPUT).write_text(rendered)
    elif args.check:
        require(
            (ROOT / OUTPUT).read_text() == rendered,
            "generated ordered receive view inventory is stale",
        )
    else:
        self_test(sources, inventory)
    print(
        f"ordered receive view registry {'mutations passed' if args.self_test else 'current'}: "
        f"{len(THEOREMS)} capstones, {len(MUTANT_EXAMPLES)} Rocq mutants, "
        f"{len(inventory['source_mutants']['obligations'])} source mutants, "
        f"{sum(SUITE_COUNTS.values())} executable tests"
    )


if __name__ == "__main__":
    try:
        main()
    except (RegistryError, KeyError, json.JSONDecodeError, SyntaxError, ValueError) as error:
        raise SystemExit(f"ordered receive view registry: {error}") from error
