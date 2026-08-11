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
    "generative_semantic",
    "semantic_metamorphic",
    "external_service_replay",
    "coverage_adequacy",
    "semantic_cross_product",
    "auth_composition",
    "state_root",
]


def objective_enabled(selected, objective):
    selected_items = [item.strip() for item in str(selected).split(",") if item.strip()]
    return "all" in selected_items or objective in selected_items


def hypothesis_settings(profile, search_mode):
    if profile == "quick":
        max_examples = 192
    elif profile == "corpus":
        max_examples = 1536
    else:
        max_examples = 6144
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


def discover_source_seed(roots):
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
                return {
                    "path": os.path.relpath(path, root),
                    "root": root,
                    "bytes_sampled": int(len(content)),
                    "size_bytes": int(size),
                    "line_count": int(content.count(b"\n") + 1 if content else 0),
                    "sha256_prefix": hashlib.sha256(content).hexdigest()[:16],
                }
    return {
        "path": "synthetic/v8-inline.rho",
        "root": "synthetic",
        "bytes_sampled": 7,
        "size_bytes": 7,
        "line_count": 1,
        "sha256_prefix": source_digest("@0!(1)"),
    }


def command_for_fixture(test_name):
    return "COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang {}".format(test_name)


def v8_event(identifier, weight=1, deploy=0, path=None):
    return canonical_event("source", int(weight), descriptor="v8/{}".format(identifier), deploy=deploy, path=path or [0])


def v8_runtime_scenario(
    name,
    source,
    family,
    statement,
    events,
    initial_budget,
    classification="bisimilar",
    threat_family="generative_semantic",
    expected_outcome="accept",
    expected_error_kind="none",
    replay_mode="eval_only",
    production_oracle="rholang_eval",
    eval_phlo=100000,
    differential_axes=None,
    term_parameters=None,
    metamorphic_relation="",
    external_service_mode="",
    expected_play_replay_relation="",
    settlement=None,
    replay_mutations=None,
    negative_mutations=None,
    source_seed=None,
    rust_test="generated_frontier_generative_semantic_fixtures_hold",
    adequacy_requirements=None,
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
        metamorphic_relation=metamorphic_relation,
        external_service_mode=external_service_mode,
        adequacy_requirements=adequacy_requirements or [
            "term_family",
            "production_oracle",
            "replay_mode",
            "cost_trace_axes",
        ],
        expected_play_replay_relation=expected_play_replay_relation,
        attack_campaign="v8_{}".format(name),
        oracle_kind="v8_{}".format(production_oracle),
        production_path="RhoRuntime::evaluate_with_phlo",
        campaign_steps=["generate_bounded_source", "evaluate_source", "check_cost_trace_axes"],
        minimized_input_digest=digest_value({"name": name, "family": family, "source": source}),
        reproducer_command=command_for_fixture(rust_test),
        threat_family=threat_family,
        expected_invariants=["v8_generated_frontier_preserves_cost_accounting_axes"],
        rust_reproducer={"test": rust_test},
        promotion_target="rust:test",
        expected_classification=classification,
    )
    return record(
        "horizon_v8_generative_semantic",
        classification,
        name,
        statement,
        scenario,
        {
            "generative_semantic": family,
            "rho_source_digest": scenario["rho_source_digest"],
            "expected_play_replay_relation": expected_play_replay_relation,
            "external_service_mode": external_service_mode,
            "term_parameters": scenario.get("term_parameters", {}),
            "adequacy_requirements": scenario.get("adequacy_requirements", []),
        },
        ["Rust: {}".format(rust_test), "Sage: v8 generative semantic adequacy"],
    )


def generative_semantic_records(profile, search_mode, objectives, source_root):
    if not (objective_enabled(objectives, "generative_semantic") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    weights = find_or_none(
        st.lists(
            st.integers(min_value=int(1), max_value=int(3)),
            min_size=int(2),
            max_size=int(3),
        ),
        lambda xs: sum(xs) >= 2,
        cfg,
    ) or [1, 1]
    seed = discover_source_seed(source_root)
    return [
        v8_runtime_scenario(
            "horizon_v8_generated_send_parallel",
            '@0!(1) | @1!("v8-send")',
            "send_parallel",
            "Generated send-only parallel terms exercise production semantic evaluation with independent cost events.",
            [
                v8_event("send_parallel/left", int(weights[0]), path=[0]),
                v8_event("send_parallel/right", int(weights[1]), path=[1]),
            ],
            sum(int(weight) for weight in weights) + 4,
            classification="bisimilar",
            term_parameters={"grammar": ["send", "parallel"], "arity": 2},
        ),
        v8_runtime_scenario(
            "horizon_v8_generated_receive_join",
            'new x in { x!(1) | for (_ <- x) { @0!("v8-receive") } }',
            "receive_join",
            "Generated send/receive joins exercise replay-sensitive communication without serializing process bodies.",
            [v8_event("receive_join", 3, path=[0, 1])],
            8,
            classification="confirmed_safe",
            threat_family="semantic_cross_product",
            term_parameters={"grammar": ["new", "send", "receive", "parallel"], "bound_names": 1},
            source_seed={"seed": seed},
        ),
        v8_runtime_scenario(
            "horizon_v8_generated_arithmetic_source",
            '@0!(1 + 2) | @1!(3)',
            "arithmetic_source",
            "Generated arithmetic source shapes exercise expression normalization under cost accounting.",
            [v8_event("arithmetic_source", 2, path=[0, 2])],
            6,
            classification="bisimilar",
            threat_family="semantic_cross_product",
            term_parameters={"grammar": ["send", "arithmetic", "parallel"], "operators": ["+"]},
            source_seed={"seed": seed},
        ),
    ]


def metamorphic_records(objectives):
    if not (objective_enabled(objectives, "semantic_metamorphic") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    return [
        v8_runtime_scenario(
            "horizon_v8_parallel_permutation_metamorphic",
            '@0!("left") | @1!("right")',
            "metamorphic_parallel_permutation",
            "Parallel source permutation preserves error classification while canonical event digests are order-insensitive.",
            [
                v8_event("metamorphic/left", 1, path=[0]),
                v8_event("metamorphic/right", 2, path=[1]),
            ],
            8,
            classification="confirmed_safe",
            threat_family="semantic_metamorphic",
            expected_outcome="metamorphic_equivalent",
            term_parameters={"variant_rho_source": '@1!("right") | @0!("left")', "relation_scope": ["eval_errors", "event_digest"]},
            metamorphic_relation="independent_event_permutation_digest_invariant",
            rust_test="generated_frontier_semantic_metamorphic_fixtures_hold",
            adequacy_requirements=["metamorphic_relation", "variant_rho_source", "event_digest"],
        ),
        v8_runtime_scenario(
            "horizon_v8_nil_identity_metamorphic",
            '@0!("v8-nil")',
            "metamorphic_nil_identity",
            "Adding Nil to an independent source must not change the semantic success/error classification.",
            [v8_event("metamorphic/nil_identity", 1, path=[0])],
            4,
            classification="confirmed_safe",
            threat_family="semantic_metamorphic",
            expected_outcome="metamorphic_equivalent",
            term_parameters={"variant_rho_source": '@0!("v8-nil") | Nil', "relation_scope": ["eval_errors"]},
            metamorphic_relation="same_error_class",
            rust_test="generated_frontier_semantic_metamorphic_fixtures_hold",
            adequacy_requirements=["metamorphic_relation", "variant_rho_source"],
        ),
    ]


def external_service_records(objectives):
    if not (objective_enabled(objectives, "external_service_replay") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    return [
        v8_runtime_scenario(
            "horizon_v8_mock_gpt4_replay_success",
            'new output, gpt4(`rho:ai:gpt4`) in { gpt4!("v8 prompt", *output) }',
            "mocked_external_service",
            "Mocked GPT replay preserves cost, digest, count, and error classification without network authority.",
            [v8_event("external/gpt4_success", 2, path=[0])],
            8,
            classification="confirmed_safe",
            threat_family="external_service_replay",
            expected_outcome="external_mock_success",
            replay_mode="play_replay",
            production_oracle="external_mock_service",
            external_service_mode="mock_gpt4_success",
            expected_play_replay_relation="same_cost_digest_count_error_class",
            rust_test="generated_frontier_external_service_replay_fixtures_hold",
            adequacy_requirements=["mock_external_service", "play_replay", "success"],
        ),
        v8_runtime_scenario(
            "horizon_v8_mock_grpc_replay_error",
            'new grpcTell(`rho:io:grpcTell`) in { grpcTell!("localhost", 8080, "payload") }',
            "mocked_external_service",
            "Mocked gRPC replay preserves cost and error classification when the service boundary rejects.",
            [v8_event("external/grpc_error", 2, path=[1])],
            8,
            classification="confirmed_safe",
            threat_family="external_service_replay",
            expected_outcome="external_mock_error",
            expected_error_kind="external_service_error",
            replay_mode="play_replay",
            production_oracle="external_mock_service",
            external_service_mode="mock_grpc_error",
            expected_play_replay_relation="same_cost_digest_count_error_class",
            rust_test="generated_frontier_external_service_replay_fixtures_hold",
            adequacy_requirements=["mock_external_service", "play_replay", "error"],
        ),
    ]


def cross_product_records(objectives):
    if not (objective_enabled(objectives, "semantic_cross_product") or objective_enabled(objectives, "auth_composition") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    return [
        v8_runtime_scenario(
            "horizon_v8_auth_settlement_slashing_cross_product",
            '@0!("v8-auth-settlement-slashing")',
            "auth_settlement_slashing",
            "Auth, settlement, and slashing metadata stay outside runtime metering while remaining bound to cost evidence.",
            [v8_event("auth_settlement_slashing/source", 2, deploy=0, path=[0])],
            6,
            classification="confirmed_safe",
            threat_family="semantic_cross_product",
            expected_outcome="auth_settlement_composed",
            production_oracle="casper_auth_composition",
            differential_axes=["cost", "digest", "count", "signature", "block_hash", "refund", "slashing"],
            settlement={"authority": "casper", "escrow": 12, "token_cost": 4, "refund": 8, "slashing_scope": "post_eval"},
            negative_mutations=["cost", "cost_trace_digest", "cost_trace_event_count", "signature", "block_hash", "slash_fields"],
            term_parameters={"composition_axes": ["auth", "settlement", "slashing"], "runtime_budget_mutation": "forbidden"},
            rust_test="generated_frontier_generative_semantic_fixtures_hold",
            adequacy_requirements=["auth_composition", "settlement", "slashing", "negative_auth"],
        ),
        v8_runtime_scenario(
            "horizon_v8_runtime_budget_oop_boundary",
            '@0!("v8-runtime-oop")',
            "runtime_budget_boundary",
            "Generated event sequences keep the first OOP boundary as explicit evidence and do not admit later work.",
            [
                canonical_event("source", 2, descriptor="v8/runtime/oop-first", deploy=0, path=[0]),
                canonical_event("primitive", 4, descriptor="v8/runtime/oop-boundary", deploy=0, path=[1]),
            ],
            3,
            classification="confirmed_safe",
            threat_family="production_mutation",
            expected_outcome="oop",
            expected_error_kind="OutOfPhlogistonsError",
            eval_phlo=1,
            term_parameters={"event_sequence": ["success", "oop"], "post_oop_admission": "blocked"},
            rust_test="generated_frontier_generative_semantic_fixtures_hold",
            adequacy_requirements=["oop_boundary", "runtime_budget", "trace_count"],
        ),
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
            "metamorphic_relation": scenario.get("metamorphic_relation", ""),
            "external_service_mode": scenario.get("external_service_mode", ""),
        },
        assertions=[
            "classification != unexpected",
            "rho_source != empty",
            "term_family != empty",
            "adequacy_requirements != empty",
            "production_path_or_oracle_named",
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
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
    }


def frontier_records(args):
    records = []
    records.extend(generative_semantic_records(args.profile, args.search_mode, args.objectives, args.source_root))
    records.extend(metamorphic_records(args.objectives))
    records.extend(external_service_records(args.objectives))
    records.extend(cross_product_records(args.objectives))
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
        "send_parallel",
        "receive_join",
        "arithmetic_source",
        "metamorphic_parallel_permutation",
        "metamorphic_nil_identity",
        "mocked_external_service",
        "auth_settlement_slashing",
        "runtime_budget_boundary",
    ])
    required_features = set([
        "generative_semantic",
        "semantic_metamorphic",
        "mock_external_service",
        "coverage_adequacy",
        "auth_composition",
        "settlement",
        "slashing",
        "external_service_replay",
        "replay_mode",
    ])
    missing_families = sorted(required_families - families)
    missing_features = sorted(required_features - features)
    if missing_families or missing_features or not {"confirmed_safe", "bisimilar"}.issubset(classes):
        raise SystemExit(
            "v8 adequacy failure: missing_families={} missing_features={} classes={}".format(
                missing_families, missing_features, sorted(classes)
            )
        )


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", choices=["quick", "corpus", "deep"], default="quick")
    parser.add_argument("--search-mode", choices=["frontier", "all"], default="frontier")
    parser.add_argument("--objectives", default="all")
    parser.add_argument("--source-root", action="append", default=[])
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
