import argparse
import hashlib
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(sys.argv[0]))), "scenario_schema.sage"))

try:
    from hypothesis import HealthCheck, Phase, find, settings
    from hypothesis import strategies as st
    from hypothesis.database import DirectoryBasedExampleDatabase
    from hypothesis.errors import NoSuchExample
except Exception as exc:
    raise SystemExit("Hypothesis is required in the Sage Python environment: {}".format(exc))


OBJECTIVES = [
    "all",
    "security",
    "production",
    "corpus_semantic",
    "grammar_mutation",
    "differential_oracle",
    "external_service_matrix",
    "casper_security_matrix",
    "runtime_trace_interleaving",
    "coverage_adequacy",
]


def objective_enabled(selected, objective):
    selected_items = [item.strip() for item in str(selected).split(",") if item.strip()]
    return "all" in selected_items or objective in selected_items


def hypothesis_settings(profile, search_mode):
    if profile == "quick":
        max_examples = 256
    elif profile == "corpus":
        max_examples = 2048
    else:
        max_examples = 8192
    database = None
    if search_mode != "frontier":
        database_dir = os.environ.get("COST_ACCOUNTING_HYPOTHESIS_DB")
        if database_dir:
            os.makedirs(database_dir, exist_ok=True)
            database = DirectoryBasedExampleDatabase(database_dir)
    return settings(
        max_examples=int(max_examples),
        database=database,
        derandomize=database is None,
        deadline=None,
        phases=[Phase.generate, Phase.shrink],
        suppress_health_check=[HealthCheck.too_slow, HealthCheck.filter_too_much],
    )


def find_or_none(strategy, predicate, cfg):
    try:
        return find(strategy, predicate, settings=cfg)
    except NoSuchExample:
        return None


def digest_value(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, default=schema_json_default).encode("utf-8")).hexdigest()[:16]


def source_digest(source):
    return hashlib.sha256(source.encode("utf-8")).hexdigest()[:16]


def expected_fixture_values(scenario):
    consumed = 0
    count = 0
    budget = int(scenario.get("initial_budget", 0))
    for event in scenario.get("events", []):
        weight = int(event.get("weight", 0))
        primitive_descriptor = str(event.get("primitive_descriptor", event.get("descriptor", "")))
        invalid_descriptor = str(event.get("kind", "")) == "primitive" and len(primitive_descriptor) > 512
        invalid_source_path = len(event.get("path", [])) > 1024
        if weight <= 0 or invalid_descriptor or invalid_source_path:
            return (0, 0, True, False)
        if consumed + weight > budget:
            return (budget, count + 1, False, True)
        consumed += weight
        count += 1
    return (consumed, count, False, False)


def discover_source_seeds(roots, limit):
    seeds = []
    for root in roots:
        if not root:
            continue
        root = os.path.abspath(root)
        if not os.path.exists(root):
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [
                name
                for name in dirnames
                if name not in [".git", "target", "fuzz", "node_modules"] and not name.startswith(".")
            ]
            for filename in sorted(filenames):
                if not filename.endswith(".rho"):
                    continue
                path = os.path.join(dirpath, filename)
                try:
                    with open(path, "rb") as handle:
                        content = handle.read(4096)
                    size = os.path.getsize(path)
                except OSError:
                    continue
                seeds.append(
                    {
                        "path": os.path.relpath(path, root),
                        "root": root,
                        "bytes_sampled": int(len(content)),
                        "size_bytes": int(size),
                        "line_count": int(content.count(b"\n") + 1 if content else 0),
                        "sha256_prefix": hashlib.sha256(content).hexdigest()[:16],
                    }
                )
                if len(seeds) >= int(limit):
                    return seeds
    if not seeds:
        seeds.append(
            {
                "path": "synthetic/v9-inline.rho",
                "root": "synthetic",
                "bytes_sampled": 7,
                "size_bytes": 7,
                "line_count": 1,
                "sha256_prefix": source_digest("@0!(1)"),
            }
        )
    return seeds


def command_for_fixture(test_name):
    return "COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang {}".format(test_name)


def v9_event(identifier, weight=1, kind="source", deploy=0, path=None):
    return canonical_event(kind, int(weight), descriptor="v9/{}".format(identifier), deploy=deploy, path=path or [0])


def v9_scenario(
    name,
    source,
    family,
    statement,
    events,
    initial_budget,
    classification="confirmed_safe",
    threat_family="corpus_semantic",
    expected_outcome="accept",
    expected_error_kind="none",
    replay_mode="eval_only",
    production_oracle="rholang_eval",
    eval_phlo=100000,
    differential_axes=None,
    term_parameters=None,
    source_seed=None,
    source_corpus_case=None,
    mutation_operator="",
    differential_oracle="",
    service_case="",
    external_service_mode="",
    security_axis="",
    adequacy_budget=None,
    expected_play_replay_relation="",
    settlement=None,
    replay_mutations=None,
    negative_mutations=None,
    rust_test="generated_frontier_corpus_semantic_fixtures_hold",
):
    scenario = canonical_scenario(
        name,
        events=events,
        initial_budget=int(initial_budget),
        settlement=settlement or {},
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count"]} if replay_mode == "play_replay" else {},
        replay_mutations=replay_mutations or [],
        negative_mutations=negative_mutations or [],
        source_seed=source_seed or {},
        rho_source=source,
        production_oracle=production_oracle,
        expected_outcome=expected_outcome,
        differential_axes=differential_axes or ["cost", "digest", "count", "errors"],
        eval_phlo=int(eval_phlo),
        expected_error_kind=expected_error_kind,
        eval_result_axes=["cost", "digest", "count", "errors"],
        rho_source_digest=source_digest(source),
        replay_mode=replay_mode,
        term_family=family,
        term_parameters=term_parameters or {},
        source_corpus_case=source_corpus_case or {},
        mutation_operator=mutation_operator,
        differential_oracle=differential_oracle,
        service_case=service_case,
        external_service_mode=external_service_mode,
        security_axis=security_axis,
        adequacy_budget=adequacy_budget or {"profile": "bounded", "max_family_examples": 1},
        adequacy_requirements=[
            "source_corpus_case" if source_corpus_case else "term_family",
            "mutation_operator" if mutation_operator else "production_oracle",
            "differential_oracle" if differential_oracle else "cost_trace_axes",
            "service_case" if service_case else "replay_mode",
            "security_axis" if security_axis else "classification",
        ],
        expected_play_replay_relation=expected_play_replay_relation,
        attack_campaign="v9_{}".format(name),
        oracle_kind="v9_{}".format(production_oracle),
        production_path="RhoRuntime production eval/replay + RuntimeBudget projection",
        campaign_steps=["generate_case", "evaluate_or_project", "check_cost_security_axes"],
        minimized_input_digest=digest_value({"name": name, "family": family, "source": source}),
        reproducer_command=command_for_fixture(rust_test),
        threat_family=threat_family,
        expected_invariants=["v9_frontier_preserves_cost_and_security_axes"],
        rust_reproducer={"test": rust_test},
        promotion_target="rust:test",
        expected_classification=classification,
    )
    witness = {
        "corpus_semantic": source_corpus_case,
        "grammar_mutation": mutation_operator,
        "differential_oracle": differential_oracle,
        "external_service_matrix": service_case,
        "casper_security_matrix": security_axis,
        "runtime_trace_interleaving": family if threat_family == "runtime_trace_interleaving" else "",
        "adequacy_budget": scenario.get("adequacy_budget", {}),
        "rho_source_digest": scenario["rho_source_digest"],
    }
    return record(
        "horizon_v9_differential_corpus_security",
        classification,
        name,
        statement,
        scenario,
        witness,
        ["Rust: {}".format(rust_test), "Sage: v9 differential corpus/security frontier"],
    )


def corpus_records(profile, search_mode, objectives, roots, limit):
    if not (objective_enabled(objectives, "corpus_semantic") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    seeds = discover_source_seeds(roots, limit)
    sample_index = find_or_none(st.integers(min_value=int(0), max_value=int(max(0, len(seeds) - 1))), lambda value: value >= 0, cfg) or 0
    seed = seeds[int(sample_index) % len(seeds)]
    return [
        v9_scenario(
            "horizon_v9_corpus_seed_semantic_eval",
            '@0!("v9-corpus") | @1!(1)',
            "corpus_seed_eval",
            "Corpus-derived source metadata must promote only with an executable semantic fixture.",
            [v9_event("corpus/source", 2, path=[0, int(seed.get("line_count", 1)) % 1024])],
            8,
            classification="bisimilar",
            threat_family="corpus_semantic",
            source_seed={"seeds": seeds},
            source_corpus_case={"selected": seed, "mode": "metadata_plus_executable_fixture"},
            rust_test="generated_frontier_corpus_semantic_fixtures_hold",
        )
    ]


def grammar_mutation_records(objectives):
    if not (objective_enabled(objectives, "grammar_mutation") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    return [
        v9_scenario(
            "horizon_v9_parallel_regrouping_mutation",
            '@0!("a") | (@1!("b") | @2!("c"))',
            "grammar_parallel_regroup",
            "Parallel regrouping preserves the cost-accounting error class and independent event digest.",
            [
                v9_event("grammar/regroup/a", 1, path=[0]),
                v9_event("grammar/regroup/b", 1, path=[1]),
                v9_event("grammar/regroup/c", 1, path=[2]),
            ],
            8,
            threat_family="grammar_mutation",
            expected_outcome="mutation_equivalent",
            term_parameters={"variant_rho_source": '(@0!("a") | @1!("b")) | @2!("c")'},
            mutation_operator="parallel_regroup",
            rust_test="generated_frontier_grammar_mutation_fixtures_hold",
        ),
        v9_scenario(
            "horizon_v9_nil_injection_mutation",
            '@0!("v9-nil-injection")',
            "grammar_nil_injection",
            "Independent Nil injection preserves semantic success/error classification.",
            [v9_event("grammar/nil_injection", 1, path=[0])],
            4,
            threat_family="grammar_mutation",
            expected_outcome="mutation_equivalent",
            term_parameters={"variant_rho_source": '@0!("v9-nil-injection") | Nil'},
            mutation_operator="nil_injection",
            rust_test="generated_frontier_grammar_mutation_fixtures_hold",
        ),
    ]


def differential_oracle_records(objectives):
    if not (objective_enabled(objectives, "differential_oracle") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    return [
        v9_scenario(
            "horizon_v9_play_replay_differential_oracle",
            'new x in { x!(1) | for (_ <- x) { @0!("v9-diff") } }',
            "differential_play_replay",
            "Production play/replay differential oracle preserves cost, digest, count, and error class.",
            [v9_event("differential/play_replay", 3, path=[0])],
            8,
            threat_family="differential_oracle",
            production_oracle="rholang_play_replay",
            replay_mode="play_replay",
            differential_oracle="play_replay_cost_digest_count_error",
            expected_play_replay_relation="same_cost_digest_count_error_class",
            rust_test="generated_frontier_differential_oracle_fixtures_hold",
        ),
        v9_scenario(
            "horizon_v9_parser_error_differential_oracle",
            "@",
            "differential_eval_error",
            "Parser-error differential oracle preserves explicit error classification and cost-trace axes.",
            [v9_event("differential/parser_error", 1, path=[1])],
            4,
            threat_family="differential_oracle",
            expected_outcome="parser_error",
            expected_error_kind="parse_error",
            differential_oracle="eval_error_classification",
            rust_test="generated_frontier_differential_oracle_fixtures_hold",
        ),
    ]


def external_service_matrix_records(objectives):
    if not (objective_enabled(objectives, "external_service_matrix") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    return [
        v9_scenario(
            "horizon_v9_external_gpt4_success",
            'new output, gpt4(`rho:ai:gpt4`) in { gpt4!("v9 prompt", *output) }',
            "external_gpt4_success",
            "GPT mock success replays with stable cost evidence.",
            [v9_event("external/gpt4_success", 2, path=[0])],
            8,
            threat_family="external_service_matrix",
            expected_outcome="external_mock_success",
            production_oracle="external_mock_service",
            replay_mode="play_replay",
            service_case="mock_gpt4_success",
            external_service_mode="mock_gpt4_success",
            expected_play_replay_relation="same_cost_digest_count_error_class",
            rust_test="generated_frontier_external_service_matrix_fixtures_hold",
        ),
        v9_scenario(
            "horizon_v9_external_dalle_error",
            'new output, dalle3(`rho:ai:dalle3`) in { dalle3!("v9 image", *output) }',
            "external_dalle_error",
            "DALL-E mock error replays with stable cost and error classification.",
            [v9_event("external/dalle_error", 2, path=[1])],
            8,
            threat_family="external_service_matrix",
            expected_outcome="external_mock_error",
            expected_error_kind="external_service_error",
            production_oracle="external_mock_service",
            replay_mode="play_replay",
            service_case="mock_dalle_error",
            external_service_mode="mock_dalle_error",
            expected_play_replay_relation="same_cost_digest_count_error_class",
            rust_test="generated_frontier_external_service_matrix_fixtures_hold",
        ),
        v9_scenario(
            "horizon_v9_external_tts_success",
            'new output, tts(`rho:ai:textToAudio`) in { tts!("v9 audio", *output) }',
            "external_tts_success",
            "TTS mock success replays with stable cost evidence.",
            [v9_event("external/tts_success", 2, path=[2])],
            8,
            threat_family="external_service_matrix",
            expected_outcome="external_mock_success",
            production_oracle="external_mock_service",
            replay_mode="play_replay",
            service_case="mock_tts_success",
            external_service_mode="mock_tts_success",
            expected_play_replay_relation="same_cost_digest_count_error_class",
            rust_test="generated_frontier_external_service_matrix_fixtures_hold",
        ),
        v9_scenario(
            "horizon_v9_external_grpc_success",
            'new grpcTell(`rho:io:grpcTell`) in { grpcTell!("localhost", 8080, "payload") }',
            "external_grpc_success",
            "gRPC mock success replays with stable cost evidence.",
            [v9_event("external/grpc_success", 2, path=[3])],
            8,
            threat_family="external_service_matrix",
            expected_outcome="external_mock_success",
            production_oracle="external_mock_service",
            replay_mode="play_replay",
            service_case="mock_grpc_success",
            external_service_mode="mock_grpc_success",
            expected_play_replay_relation="same_cost_digest_count_error_class",
            rust_test="generated_frontier_external_service_matrix_fixtures_hold",
        ),
    ]


def casper_security_records(objectives):
    if not (objective_enabled(objectives, "casper_security_matrix") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    return [
        v9_scenario(
            "horizon_v9_casper_auth_payload_security_matrix",
            '@0!("v9-casper-auth")',
            "casper_auth_payload",
            "Casper authenticated payload mutations cover cost, digest, count, signature, block hash, and trace presence.",
            [v9_event("casper/auth_payload", 2, deploy=0, path=[0])],
            6,
            threat_family="casper_security_matrix",
            expected_outcome="auth_payload_rejects_mutation",
            production_oracle="casper_auth_composition",
            differential_axes=["cost", "digest", "count", "signature", "block_hash", "trace_presence"],
            negative_mutations=["cost", "cost_trace_digest", "cost_trace_event_count", "signature", "block_hash", "cost_trace_present"],
            security_axis="auth_payload_mutation",
            rust_test="generated_frontier_casper_security_matrix_fixtures_hold",
        ),
        v9_scenario(
            "horizon_v9_casper_refund_slashing_security_matrix",
            '@0!("v9-refund-slash")',
            "casper_refund_slashing",
            "Refund and slashing evidence remain post-evaluation security axes and cannot replenish runtime fuel.",
            [v9_event("casper/refund_slashing", 2, deploy=0, path=[1])],
            6,
            threat_family="casper_security_matrix",
            expected_outcome="settlement_slashing_isolated",
            production_oracle="casper_auth_composition",
            differential_axes=["cost", "digest", "count", "refund", "slashing", "block_hash"],
            settlement={"authority": "casper", "escrow": 12, "token_cost": 4, "refund": 8, "slashing_scope": "post_eval"},
            negative_mutations=["slash_fields", "block_hash", "signature"],
            security_axis="refund_slashing_isolation",
            rust_test="generated_frontier_casper_security_matrix_fixtures_hold",
        ),
    ]


def runtime_trace_records(objectives):
    if not (objective_enabled(objectives, "runtime_trace_interleaving") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    return [
        v9_scenario(
            "horizon_v9_trace_interleaving_multi_deploy",
            '@0!("v9-trace-interleave") | @1!("v9-trace-interleave")',
            "trace_interleaving_multi_deploy",
            "Runtime trace interleaving keeps deploy/path identity and canonical digest stability across independent workers.",
            [
                v9_event("trace/deploy0/path0", 1, deploy=0, path=[0]),
                v9_event("trace/deploy1/path0", 1, deploy=1, path=[0]),
                v9_event("trace/deploy0/path1", 2, deploy=0, path=[1]),
            ],
            8,
            threat_family="runtime_trace_interleaving",
            expected_outcome="trace_interleaving_safe",
            differential_oracle="runtime_budget_trace_digest",
            adequacy_budget={"profile": "bounded", "max_trace_events": 3, "max_deploys": 2},
            rust_test="generated_frontier_runtime_trace_interleaving_properties_hold",
        )
    ]


def fixture_from_record(item):
    scenario = item["scenario"]
    return scenario_fixture(
        item["name"],
        item["classification"],
        scenario,
        item["deterministic_witness"],
        {
            "classification": item["classification"],
            "promotion_target": scenario.get("promotion_target", "record"),
            "threat_family": scenario.get("threat_family", "search_governance"),
            "production_oracle": scenario.get("production_oracle", ""),
            "expected_outcome": scenario.get("expected_outcome", ""),
            "expected_error_kind": scenario.get("expected_error_kind", ""),
            "replay_mode": scenario.get("replay_mode", ""),
            "term_family": scenario.get("term_family", ""),
            "source_corpus_case": scenario.get("source_corpus_case", {}),
            "mutation_operator": scenario.get("mutation_operator", ""),
            "differential_oracle": scenario.get("differential_oracle", ""),
            "service_case": scenario.get("service_case", ""),
            "security_axis": scenario.get("security_axis", ""),
        },
        assertions=[
            "classification != unexpected",
            "rho_source != empty",
            "v9_metadata != empty",
            "adequacy_budget != empty",
        ],
    )


def rust_fixture_from_record(item):
    scenario = item["scenario"]
    total, count, invalid, oop = expected_fixture_values(scenario)
    return {
        "id": item["name"],
        "classification": item["classification"],
        "threat_family": scenario.get("threat_family", "search_governance"),
        "promotion_target": scenario.get("promotion_target", "record"),
        "initial_budget": int(scenario.get("initial_budget", 0)),
        "events": scenario.get("events", []),
        "expected_total_cost": int(total),
        "expected_event_count": int(count),
        "expects_invalid_admission": bool(invalid),
        "expects_oop": bool(oop),
        "settlement": scenario.get("settlement", {}),
        "replay_mutations": scenario.get("replay_mutations", []),
        "negative_mutations": scenario.get("negative_mutations", []),
        "coverage_features": item.get("coverage_features", []),
        "source_seed": scenario.get("source_seed", {}),
        "attack_campaign": scenario.get("attack_campaign", ""),
        "oracle_kind": scenario.get("oracle_kind", ""),
        "production_path": scenario.get("production_path", ""),
        "campaign_steps": scenario.get("campaign_steps", []),
        "minimized_input_digest": scenario.get("minimized_input_digest", ""),
        "reproducer_command": scenario.get("reproducer_command", ""),
        "candidate_property": scenario.get("candidate_property", ""),
        "oracle_strength": scenario.get("oracle_strength", ""),
        "rho_source": scenario.get("rho_source", ""),
        "production_oracle": scenario.get("production_oracle", ""),
        "expected_outcome": scenario.get("expected_outcome", ""),
        "differential_axes": scenario.get("differential_axes", []),
        "eval_phlo": int(scenario.get("eval_phlo", 0)),
        "expected_error_kind": scenario.get("expected_error_kind", ""),
        "eval_result_axes": scenario.get("eval_result_axes", []),
        "rho_source_digest": scenario.get("rho_source_digest", ""),
        "state_root_axis": scenario.get("state_root_axis", ""),
        "replay_mode": scenario.get("replay_mode", ""),
        "term_family": scenario.get("term_family", ""),
        "term_parameters": scenario.get("term_parameters", {}),
        "metamorphic_relation": scenario.get("metamorphic_relation", ""),
        "external_service_mode": scenario.get("external_service_mode", ""),
        "adequacy_requirements": scenario.get("adequacy_requirements", []),
        "expected_play_replay_relation": scenario.get("expected_play_replay_relation", ""),
        "source_corpus_case": scenario.get("source_corpus_case", {}),
        "mutation_operator": scenario.get("mutation_operator", ""),
        "differential_oracle": scenario.get("differential_oracle", ""),
        "service_case": scenario.get("service_case", ""),
        "security_axis": scenario.get("security_axis", ""),
        "adequacy_budget": scenario.get("adequacy_budget", {}),
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
    }


def frontier_records(args):
    records = []
    records.extend(corpus_records(args.profile, args.search_mode, args.objectives, args.source_root, args.source_limit))
    records.extend(grammar_mutation_records(args.objectives))
    records.extend(differential_oracle_records(args.objectives))
    records.extend(external_service_matrix_records(args.objectives))
    records.extend(casper_security_records(args.objectives))
    records.extend(runtime_trace_records(args.objectives))
    return records


def assert_adequacy(records):
    features = set()
    families = set()
    classes = set()
    for item in records:
        classes.add(item["classification"])
        for feature in item.get("coverage_features", []):
            features.add(feature)
        family = item["scenario"].get("term_family", "")
        if family:
            families.add(family)
    required_families = set([
        "corpus_seed_eval",
        "grammar_parallel_regroup",
        "grammar_nil_injection",
        "differential_play_replay",
        "differential_eval_error",
        "external_gpt4_success",
        "external_dalle_error",
        "external_tts_success",
        "external_grpc_success",
        "casper_auth_payload",
        "casper_refund_slashing",
        "trace_interleaving_multi_deploy",
    ])
    required_features = set([
        "corpus_semantic",
        "grammar_mutation",
        "differential_oracle",
        "external_service_matrix",
        "casper_security_matrix",
        "runtime_trace_interleaving",
        "coverage_adequacy",
        "adequacy_budget",
    ])
    missing_families = sorted(required_families - families)
    missing_features = sorted(required_features - features)
    if missing_families or missing_features or not {"confirmed_safe", "bisimilar"}.issubset(classes):
        raise SystemExit(
            "v9 adequacy failure: missing_families={} missing_features={} classes={}".format(
                missing_families, missing_features, sorted(classes)
            )
        )


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=["quick", "corpus", "deep"], default="quick")
    parser.add_argument("--search-mode", choices=["frontier", "all"], default="frontier")
    parser.add_argument("--objectives", default="all")
    parser.add_argument("--source-root", action="append", default=[])
    parser.add_argument("--source-limit", type=int, default=8)
    parser.add_argument("--json-out")
    parser.add_argument("--fixture-out")
    parser.add_argument("--coverage-out")
    parser.add_argument("--rust-fixtures-out")
    args = parser.parse_args(argv)

    records = frontier_records(args)
    assert_adequacy(records)
    fixtures = [fixture_from_record(item) for item in records]
    rust_fixtures = [rust_fixture_from_record(item) for item in records]
    output = {
        "profile": args.profile,
        "search_mode": args.search_mode,
        "objectives": args.objectives,
        "source_roots": args.source_root,
        "records": records,
        "fixtures": fixtures,
        "rust_fixtures": rust_fixtures,
        "coverage_summary": coverage_summary(records),
        "frontier": pareto_frontier(records),
    }
    text = json.dumps(output, indent=2, sort_keys=True, default=schema_json_default)
    if args.json_out:
        with open(args.json_out, "w") as handle:
            handle.write(text + "\n")
    else:
        print(text)
    if args.fixture_out:
        with open(args.fixture_out, "w") as handle:
            handle.write(json.dumps({"fixtures": fixtures}, indent=2, sort_keys=True, default=schema_json_default) + "\n")
    if args.coverage_out:
        with open(args.coverage_out, "w") as handle:
            handle.write(json.dumps(output["coverage_summary"], indent=2, sort_keys=True, default=schema_json_default) + "\n")
    if args.rust_fixtures_out:
        with open(args.rust_fixtures_out, "w") as handle:
            handle.write(json.dumps({"fixtures": rust_fixtures}, indent=2, sort_keys=True, default=schema_json_default) + "\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
