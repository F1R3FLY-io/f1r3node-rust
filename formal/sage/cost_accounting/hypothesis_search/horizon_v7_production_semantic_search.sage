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
    "semantic_eval",
    "play_replay",
    "source_corpus",
    "error_boundary",
    "state_root",
    "auth_composition",
]


def objective_enabled(selected, objective):
    selected_items = [item.strip() for item in str(selected).split(",") if item.strip()]
    return "all" in selected_items or objective in selected_items


def hypothesis_settings(profile, search_mode):
    if profile == "quick":
        max_examples = 128
    elif profile == "corpus":
        max_examples = 1024
    else:
        max_examples = 4096
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
                "path": "synthetic/v7-inline.rho",
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


def semantic_event(identifier, weight=1, deploy=0, path=None):
    return canonical_event("source", int(weight), descriptor="v7/{}".format(identifier), deploy=deploy, path=path or [0])


def semantic_eval_records(profile, search_mode, objectives):
    if not (objective_enabled(objectives, "semantic_eval") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    cfg = hypothesis_settings(profile, search_mode)
    weights = find_or_none(
        st.lists(st.integers(min_value=1, max_value=3), min_size=int(2), max_size=int(3)),
        lambda xs: sum(xs) >= 2,
        cfg,
    ) or [1, 2]
    source = '@0!(1) | @1!("v7-semantic")'
    scenario = canonical_scenario(
        "horizon_v7_semantic_eval_parallel_source",
        events=[
            canonical_event("source", int(weight), descriptor="v7/semantic/{}".format(index), deploy=0, path=[index])
            for index, weight in enumerate(weights)
        ],
        initial_budget=sum(int(weight) for weight in weights) + 4,
        rho_source=source,
        production_oracle="rholang_eval",
        expected_outcome="accept",
        differential_axes=["cost", "digest", "count", "errors"],
        eval_phlo=100000,
        expected_error_kind="none",
        eval_result_axes=["cost", "digest", "count", "errors"],
        rho_source_digest=source_digest(source),
        replay_mode="eval_only",
        attack_campaign="v7_semantic_eval_parallel_source",
        oracle_kind="production_rholang_semantic_eval",
        production_path="RhoRuntime::evaluate_with_phlo",
        campaign_steps=["evaluate_source", "check_eval_result_axes"],
        minimized_input_digest=digest_value(weights),
        reproducer_command=command_for_fixture("generated_frontier_semantic_eval_fixtures_hold"),
        threat_family="production_eval_replay",
        expected_invariants=["production_eval_result_carries_cost_digest_count_and_errors"],
        rust_reproducer={"test": "generated_frontier_semantic_eval_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="bisimilar",
    )
    return [
        record(
            "horizon_v7_semantic_eval",
            "bisimilar",
            "horizon_v7_semantic_eval_parallel_source",
            "A non-Nil production Rholang source evaluates with cost, digest, count, and error axes before promotion.",
            scenario,
            {"production_eval_replay": "parallel_source", "rho_source_digest": scenario["rho_source_digest"]},
            ["Rust: generated_frontier_semantic_eval_fixtures_hold", "Rocq: reflected execution cost preservation"],
        )
    ]


def play_replay_records(objectives):
    if not (objective_enabled(objectives, "play_replay") or objective_enabled(objectives, "state_root") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    source = "new x in { x!(1) | for (_ <- x) { Nil } }"
    scenario = canonical_scenario(
        "horizon_v7_play_replay_state_root",
        events=[semantic_event("play-replay/state-root", 2, path=[0])],
        initial_budget=8,
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count", "state_root"]},
        replay_mutations=["cost_trace_digest", "cost_trace_event_count"],
        rho_source=source,
        production_oracle="rholang_play_replay",
        expected_outcome="state_root_replayed",
        differential_axes=["cost", "digest", "count", "state_root"],
        eval_phlo=100000,
        expected_error_kind="none",
        eval_result_axes=["cost", "digest", "count", "errors", "state_root"],
        rho_source_digest=source_digest(source),
        state_root_axis="checkpoint_root",
        replay_mode="play_replay",
        attack_campaign="v7_play_replay_state_root",
        oracle_kind="production_play_replay_state_root",
        production_path="RhoRuntime::take_event_log + replay rig/check_replay_data",
        campaign_steps=["soft_checkpoint", "play", "take_event_log", "rig_replay", "replay", "check_replay_data"],
        minimized_input_digest=digest_value("v7-play-replay-state-root"),
        reproducer_command=command_for_fixture("generated_frontier_play_replay_fixtures_hold"),
        threat_family="production_state_root",
        expected_invariants=["play_replay_preserves_cost_digest_count_and_error_class"],
        rust_reproducer={"test": "generated_frontier_play_replay_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v7_play_replay",
            "confirmed_safe",
            "horizon_v7_play_replay_state_root",
            "Production play/replay preserves cost trace evidence and consumes replay data for a communicating source.",
            scenario,
            {"state_root_replay": True, "replay_mode": "play_replay"},
            ["Rust: generated_frontier_play_replay_fixtures_hold", "TLA+: RuntimeBudgetReplay"],
        )
    ]


def source_corpus_records(seed_roots, source_limit, objectives):
    if not (objective_enabled(objectives, "source_corpus") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    seeds = discover_source_seeds(seed_roots, source_limit)
    first_seed = seeds[0]
    source = '@0!(1 + 2) | @1!("v7-source-corpus")'
    scenario = canonical_scenario(
        "horizon_v7_source_corpus_semantics",
        events=[semantic_event("source-corpus/semantics", 3, path=[0, int(first_seed.get("line_count", 1)) % 512])],
        initial_budget=8,
        source_seed={"seeds": seeds},
        rho_source=source,
        production_oracle="rholang_eval",
        expected_outcome="accept",
        differential_axes=["cost", "digest", "count", "source_shape"],
        eval_phlo=100000,
        expected_error_kind="none",
        eval_result_axes=["cost", "digest", "count", "errors"],
        rho_source_digest=source_digest(source),
        replay_mode="eval_only",
        attack_campaign="v7_source_corpus_semantics",
        oracle_kind="production_source_corpus_semantics",
        production_path="RhoRuntime::evaluate_with_phlo",
        campaign_steps=["load_source_seed_metadata", "evaluate_inline_source_shape", "compare_eval_axes"],
        minimized_input_digest=digest_value(first_seed),
        reproducer_command=command_for_fixture("generated_frontier_semantic_eval_fixtures_hold"),
        threat_family="production_source_corpus",
        expected_invariants=["source_shape_semantic_fixture_is_not_nil_only"],
        rust_reproducer={"test": "generated_frontier_semantic_eval_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="bisimilar",
    )
    return [
        record(
            "horizon_v7_source_corpus",
            "bisimilar",
            "horizon_v7_source_corpus_semantics",
            "Source-corpus search carries real source metadata while evaluating a non-Nil production-shaped source.",
            scenario,
            {"production_source_corpus": first_seed.get("path", "synthetic"), "rho_source_digest": scenario["rho_source_digest"]},
            ["Rust: generated_frontier_semantic_eval_fixtures_hold", "Sage: source-corpus seed metadata"],
        )
    ]


def error_boundary_records(objectives):
    if not (objective_enabled(objectives, "error_boundary") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    oop_source = '@0!("v7-oop")'
    abort_source = 'new abort(`rho:execution:abort`) in { abort!("v7 abort") }'
    parser_source = "@"
    cases = [
        (
            "horizon_v7_finite_phlo_oop_boundary",
            oop_source,
            "oop",
            "OutOfPhlogistonsError",
            1,
            "v7_finite_phlo_oop_boundary",
            "A finite-phlo production source crosses the OOP boundary without being accepted as cost-valid.",
        ),
        (
            "horizon_v7_user_abort_error_boundary",
            abort_source,
            "user_abort",
            "UserAbortError",
            100000,
            "v7_user_abort_error_boundary",
            "A user-abort production source remains an explicit error result with cost evidence.",
        ),
        (
            "horizon_v7_parser_error_boundary",
            parser_source,
            "parser_error",
            "parse_error",
            100000,
            "v7_parser_error_boundary",
            "A malformed production source remains an explicit parse error with cost evidence.",
        ),
    ]
    records = []
    for index, (name, source, outcome, error_kind, eval_phlo, campaign, statement) in enumerate(cases):
        scenario = canonical_scenario(
            name,
            events=[semantic_event("error/{}".format(outcome), 1, path=[index])],
            initial_budget=4,
            rho_source=source,
            production_oracle="rholang_eval",
            expected_outcome=outcome,
            differential_axes=["cost", "digest", "count", "errors"],
            eval_phlo=eval_phlo,
            expected_error_kind=error_kind,
            eval_result_axes=["cost", "digest", "count", "errors"],
            rho_source_digest=source_digest(source),
            replay_mode="eval_only",
            attack_campaign=campaign,
            oracle_kind="production_error_boundary",
            production_path="RhoRuntime::evaluate_with_phlo",
            campaign_steps=["evaluate_source", "classify_error", "preserve_cost_trace_evidence"],
            minimized_input_digest=digest_value(campaign),
            reproducer_command=command_for_fixture("generated_frontier_phlo_boundary_fixtures_hold"),
            threat_family="production_error_boundary",
            expected_invariants=["production_error_result_is_not_cost_valid_success"],
            rust_reproducer={"test": "generated_frontier_phlo_boundary_fixtures_hold"},
            promotion_target="rust:test",
            expected_classification="confirmed_safe",
        )
        records.append(
            record(
                "horizon_v7_error_boundary",
                "confirmed_safe",
                name,
                statement,
                scenario,
                {"production_error_boundary": outcome, "expected_error_kind": error_kind},
                ["Rust: generated_frontier_phlo_boundary_fixtures_hold", "Rocq: error boundary cost evidence"],
            )
        )
    return records


def auth_composition_records(objectives):
    if not (objective_enabled(objectives, "auth_composition") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security")):
        return []
    source = '@0!("v7-auth-composition")'
    scenario = canonical_scenario(
        "horizon_v7_auth_composition_cost_trace_payload",
        events=[semantic_event("auth-composition/source", 2, deploy=0, path=[0])],
        initial_budget=6,
        deploy_count=2,
        settlement={"authority": "casper", "escrow": 12, "token_cost": 4, "refund": 8, "auth_composition": True},
        replay_fields={"fields": ["cost", "cost_trace_digest", "cost_trace_event_count", "signature", "block_hash", "cost_trace_present"]},
        negative_mutations=["cost", "cost_trace_digest", "cost_trace_event_count", "signature", "block_hash", "cost_trace_present"],
        rho_source=source,
        production_oracle="casper_auth_composition",
        expected_outcome="auth_composed",
        differential_axes=["cost", "digest", "count", "signature", "block_hash", "trace_presence", "refund"],
        eval_phlo=100000,
        expected_error_kind="none",
        eval_result_axes=["cost", "digest", "count", "errors"],
        rho_source_digest=source_digest(source),
        replay_mode="eval_only",
        attack_campaign="v7_auth_composition_cost_trace_payload",
        oracle_kind="production_auth_composition",
        production_path="RhoRuntime eval result + Casper replay payload authentication",
        campaign_steps=["evaluate_source", "project_cost_trace", "mutate_authenticated_axes", "assert_settlement_is_post_eval"],
        minimized_input_digest=digest_value("v7-auth-composition"),
        reproducer_command=command_for_fixture("generated_frontier_auth_composition_fixtures_hold"),
        threat_family="production_auth_composition",
        expected_invariants=["auth_payload_axes_cover_eval_cost_trace_and_settlement_projection"],
        rust_reproducer={"test": "generated_frontier_auth_composition_fixtures_hold"},
        promotion_target="rust:test",
        expected_classification="confirmed_safe",
    )
    return [
        record(
            "horizon_v7_auth_composition",
            "confirmed_safe",
            "horizon_v7_auth_composition_cost_trace_payload",
            "Production eval cost evidence remains tied to replay-authenticated Casper payload axes and settlement isolation.",
            scenario,
            {"auth_composition": True, "casper_boundary": "replay_payload", "refund": 8},
            ["Rust: generated_frontier_auth_composition_fixtures_hold", "Casper: replay/block hash authentication tests"],
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
        },
        assertions=[
            "classification != unexpected",
            "rho_source != empty",
            "rho_source_digest != empty",
            "eval_phlo > 0",
            "expected_error_kind != empty",
            "eval_result_axes != empty",
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
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
    }


def frontier_records(args):
    records = []
    records.extend(semantic_eval_records(args.profile, args.search_mode, args.objectives))
    records.extend(play_replay_records(args.objectives))
    records.extend(source_corpus_records(args.source_root, args.source_limit, args.objectives))
    records.extend(error_boundary_records(args.objectives))
    records.extend(auth_composition_records(args.objectives))
    return records


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
