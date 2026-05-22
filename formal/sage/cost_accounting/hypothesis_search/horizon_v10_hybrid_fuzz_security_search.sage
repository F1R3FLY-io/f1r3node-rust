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
    "fuzz_runtime_budget",
    "fuzz_replay_payload",
    "fuzz_lifecycle_trace",
    "fuzz_casper_block_auth",
    "fuzz_external_service_error",
    "fuzz_rholang_corpus_mutation",
    "kani_budget",
    "parallel_schedule_stress",
    "settlement_refund",
    "slashing_isolation",
    "legacy_downgrade",
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


def source_digest(source):
    return hashlib.sha256(source.encode("utf-8")).hexdigest()[:16]


def digest_value(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, default=schema_json_default).encode("utf-8")).hexdigest()[:16]


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
            return (consumed, count, True, False)
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
                "path": "synthetic/v10-inline.rho",
                "root": "synthetic",
                "bytes_sampled": 15,
                "size_bytes": 15,
                "line_count": 1,
                "sha256_prefix": source_digest('@0!("v10")'),
            }
        )
    return seeds


def command_for_fixture(test_name):
    return "COST_ACCOUNTING_FRONTIER_FIXTURES_JSON=<fixtures> cargo nextest run -p rholang {}".format(test_name)


def v10_event(identifier, weight=1, kind="source", deploy=0, path=None):
    return canonical_event(kind, int(weight), descriptor="v10/{}".format(identifier), deploy=deploy, path=path or [0])


def v10_scenario(
    name,
    source,
    family,
    statement,
    events,
    initial_budget,
    classification="confirmed_safe",
    threat_family="hybrid_fuzz_security",
    expected_outcome="accept",
    expected_error_kind="none",
    replay_mode="eval_only",
    production_oracle="runtime_budget",
    eval_phlo=100000,
    differential_axes=None,
    term_parameters=None,
    settlement=None,
    replay_mutations=None,
    negative_mutations=None,
    fuzz_target="",
    fuzz_seed_kind="",
    kani_harness="",
    bounded_depth=4,
    mutator_family="",
    production_replay_target="generated_frontier_v10_fuzz_seed_fixtures_hold",
    promotion_gate="rust_replay_before_source_action",
    rust_test="generated_frontier_v10_fuzz_seed_fixtures_hold",
    source_seed=None,
    source_corpus_case=None,
    external_service_mode="",
    expected_play_replay_relation="",
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
        external_service_mode=external_service_mode,
        adequacy_requirements=[
            "fuzz_target" if fuzz_target else "production_oracle",
            "kani_harness" if kani_harness else "production_replay_target",
            "promotion_gate",
            "bounded_depth",
        ],
        expected_play_replay_relation=expected_play_replay_relation,
        attack_campaign="v10_{}".format(name),
        oracle_kind="v10_{}".format(production_oracle),
        production_path="Hybrid fuzz/Kani seed projection + production Rust replay",
        campaign_steps=["generate_hybrid_seed", "project_to_fixture", "replay_on_production_gate"],
        minimized_input_digest=digest_value({"name": name, "family": family, "events": events}),
        reproducer_command=command_for_fixture(rust_test),
        fuzz_target=fuzz_target,
        fuzz_seed_kind=fuzz_seed_kind,
        kani_harness=kani_harness,
        bounded_depth=int(bounded_depth),
        mutator_family=mutator_family,
        production_replay_target=production_replay_target,
        promotion_gate=promotion_gate,
        threat_family=threat_family,
        expected_invariants=["v10_hybrid_frontier_requires_production_replay_before_source_action"],
        rust_reproducer={"test": rust_test, "fuzz_target": fuzz_target, "kani_harness": kani_harness},
        promotion_target="rust:test",
        expected_classification=classification,
    )
    witness = {
        "hybrid_fuzz_security": threat_family,
        "fuzz_target": fuzz_target,
        "fuzz_seed_kind": fuzz_seed_kind,
        "kani_harness": kani_harness,
        "bounded_depth": bounded_depth,
        "mutator_family": mutator_family,
        "production_replay_target": production_replay_target,
        "promotion_gate": promotion_gate,
    }
    return record(
        "horizon_v10_hybrid_fuzz_security",
        classification,
        name,
        statement,
        scenario,
        witness,
        ["Rust: {}".format(rust_test), "Sage: v10 hybrid fuzz/Kani/security frontier"],
    )


def fuzz_records(profile, search_mode, objectives, roots, limit):
    cfg = hypothesis_settings(profile, search_mode)
    seeds = discover_source_seeds(roots, limit)
    sample_index = find_or_none(st.integers(min_value=int(0), max_value=int(max(0, len(seeds) - 1))), lambda value: value >= 0, cfg) or 0
    seed = seeds[int(sample_index) % len(seeds)]
    records = []
    if objective_enabled(objectives, "fuzz_runtime_budget") or objective_enabled(objectives, "production") or objective_enabled(objectives, "security"):
        records.append(v10_scenario(
            "horizon_v10_fuzz_runtime_budget_admission_boundaries",
            '@0!("v10-oop")',
            "fuzz_runtime_budget_admission_boundaries",
            "Runtime-budget fuzz seeds must include an OOP boundary that commits exactly the budget and one boundary event.",
            [v10_event("runtime/oop_boundary", 5, path=[0])],
            3,
            threat_family="hybrid_fuzz_runtime",
            expected_outcome="oop",
            expected_error_kind="none",
            fuzz_target="runtime_budget_admission",
            fuzz_seed_kind="oop_boundary",
            mutator_family="weight_boundary",
            production_replay_target="generated_frontier_v10_fuzz_seed_fixtures_hold",
            rust_test="generated_frontier_v10_fuzz_seed_fixtures_hold",
        ))
    if objective_enabled(objectives, "fuzz_replay_payload") or objective_enabled(objectives, "security"):
        records.append(v10_scenario(
            "horizon_v10_fuzz_replay_payload_cost_fields",
            '@0!("v10-replay")',
            "fuzz_replay_payload_cost_fields",
            "Replay-payload fuzz seeds must mutate every cost-trace payload field before promotion.",
            [v10_event("replay/cost_digest", 1, deploy=0, path=[0]), v10_event("replay/cost_count", 2, deploy=1, path=[0])],
            8,
            threat_family="hybrid_fuzz_replay",
            expected_outcome="replay_mutation_rejected",
            replay_mutations=["cost", "cost_trace_digest", "cost_trace_event_count", "signature", "block_hash", "cost_trace_present"],
            negative_mutations=["cost", "cost_trace_digest", "cost_trace_event_count", "signature", "block_hash", "cost_trace_present"],
            fuzz_target="replay_payload_cost_fields",
            fuzz_seed_kind="authenticated_payload_mutation",
            mutator_family="replay_payload_cost_fields",
            production_replay_target="generated_frontier_v10_replay_payload_matrix_fixtures_hold",
            rust_test="generated_frontier_v10_replay_payload_matrix_fixtures_hold",
        ))
    if objective_enabled(objectives, "fuzz_lifecycle_trace") or objective_enabled(objectives, "security"):
        records.append(v10_scenario(
            "horizon_v10_fuzz_lifecycle_trace_sequence",
            '@0!("v10-lifecycle") | @1!("trace")',
            "fuzz_lifecycle_trace_sequence",
            "Lifecycle fuzz seeds must preserve rollback/finalization trace evidence and bounded replay promotion metadata.",
            [v10_event("lifecycle/reserve", 1, path=[0]), v10_event("lifecycle/finalize", 1, path=[1])],
            6,
            threat_family="hybrid_fuzz_lifecycle",
            expected_outcome="lifecycle_trace_safe",
            fuzz_target="cost_accounting_lifecycle_trace",
            fuzz_seed_kind="reserve_finalize_replay",
            mutator_family="lifecycle_operation_sequence",
            production_replay_target="generated_frontier_v10_lifecycle_trace_fixtures_hold",
            rust_test="generated_frontier_v10_lifecycle_trace_fixtures_hold",
        ))
    if objective_enabled(objectives, "fuzz_casper_block_auth") or objective_enabled(objectives, "security"):
        records.append(v10_scenario(
            "horizon_v10_fuzz_casper_block_auth_fields",
            '@0!("v10-block-auth")',
            "fuzz_casper_block_auth_fields",
            "Block-auth fuzz seeds must cover cost trace digest/count, signature, block hash, and trace presence.",
            [v10_event("casper/block_auth", 2, path=[0])],
            6,
            threat_family="hybrid_fuzz_casper",
            expected_outcome="block_auth_rejects_mutation",
            production_oracle="casper_auth_composition",
            differential_axes=["cost", "digest", "count", "signature", "block_hash", "trace_presence"],
            negative_mutations=["cost_trace_digest", "cost_trace_event_count", "signature", "block_hash", "cost_trace_present"],
            fuzz_target="replay_payload_cost_fields",
            fuzz_seed_kind="block_auth_payload_mutation",
            mutator_family="casper_block_auth_fields",
            production_replay_target="generated_frontier_v10_casper_block_auth_fixtures_hold",
            rust_test="generated_frontier_v10_casper_block_auth_fixtures_hold",
        ))
    if objective_enabled(objectives, "fuzz_external_service_error") or objective_enabled(objectives, "production"):
        records.append(v10_scenario(
            "horizon_v10_fuzz_external_service_error_replay",
            'new output, dalle3(`rho:ai:dalle3`) in { dalle3!("v10 image", *output) }',
            "fuzz_external_service_error_replay",
            "External-service fuzz seeds must replay error outcomes with stable cost evidence and no network authority.",
            [v10_event("external/dalle_error", 2, path=[1])],
            8,
            threat_family="hybrid_fuzz_external",
            expected_outcome="external_mock_error",
            expected_error_kind="external_service_error",
            production_oracle="external_mock_service",
            replay_mode="play_replay",
            external_service_mode="mock_dalle_error",
            expected_play_replay_relation="same_cost_digest_count_error_class",
            fuzz_target="external_service_replay",
            fuzz_seed_kind="mock_error_replay",
            mutator_family="external_error_surface",
            production_replay_target="generated_frontier_v10_fuzz_seed_fixtures_hold",
            rust_test="generated_frontier_v10_fuzz_seed_fixtures_hold",
        ))
    if objective_enabled(objectives, "fuzz_rholang_corpus_mutation") or objective_enabled(objectives, "production"):
        records.append(v10_scenario(
            "horizon_v10_fuzz_rholang_corpus_mutation",
            '@0!("v10-corpus") | @1!(1)',
            "fuzz_rholang_corpus_mutation",
            "Rholang corpus fuzz seeds must keep real source metadata attached to executable and mutated source shapes.",
            [v10_event("corpus/mutation", 2, path=[0, int(seed.get("line_count", 1)) % 1024])],
            8,
            classification="bisimilar",
            threat_family="hybrid_fuzz_corpus",
            term_parameters={"variant_rho_source": '@0!("v10-corpus") | @1!(1) | Nil'},
            source_seed={"seeds": seeds},
            source_corpus_case={"selected": seed, "mode": "v10_mutated_executable_fixture"},
            fuzz_target="rholang_corpus_mutation",
            fuzz_seed_kind="source_corpus_mutation",
            mutator_family="rho_source_nil_injection",
            production_replay_target="generated_frontier_v10_semantic_corpus_mutation_fixtures_hold",
            rust_test="generated_frontier_v10_semantic_corpus_mutation_fixtures_hold",
        ))
    return records


def kani_records(objectives):
    if not (objective_enabled(objectives, "kani_budget") or objective_enabled(objectives, "security") or objective_enabled(objectives, "production")):
        return []
    return [
        v10_scenario(
            "horizon_v10_kani_budget_conservation_bound",
            '@0!("v10-kani-budget")',
            "kani_budget_conservation_bound",
            "Kani-bound metadata must target RuntimeBudget conservation over successful reservations.",
            [v10_event("kani/budget/a", 1, path=[0]), v10_event("kani/budget/b", 2, path=[1])],
            8,
            threat_family="hybrid_kani_bound",
            kani_harness="kani_runtime_budget_conservation",
            bounded_depth=6,
            production_replay_target="generated_frontier_v10_fuzz_seed_fixtures_hold",
            rust_test="generated_frontier_v10_fuzz_seed_fixtures_hold",
        ),
        v10_scenario(
            "horizon_v10_kani_invalid_admission_no_mutation",
            '@0!("v10-kani-invalid")',
            "kani_invalid_admission_no_mutation",
            "Kani-bound metadata must target invalid admission before budget or trace mutation.",
            [v10_event("kani/invalid", 0, kind="primitive", path=[0])],
            4,
            threat_family="hybrid_kani_bound",
            expected_outcome="invalid_admission",
            kani_harness="kani_invalid_admission_no_mutation",
            bounded_depth=4,
            production_replay_target="generated_frontier_v10_fuzz_seed_fixtures_hold",
            rust_test="generated_frontier_v10_fuzz_seed_fixtures_hold",
        ),
        v10_scenario(
            "horizon_v10_kani_oop_single_boundary",
            '@0!("v10-kani-oop")',
            "kani_oop_single_boundary",
            "Kani-bound metadata must target exactly one OOP boundary event under bounded reservation depth.",
            [v10_event("kani/oop/success", 3, path=[0]), v10_event("kani/oop/boundary", 3, path=[1])],
            4,
            threat_family="hybrid_kani_bound",
            expected_outcome="oop",
            kani_harness="kani_oop_single_boundary",
            bounded_depth=4,
            production_replay_target="generated_frontier_v10_fuzz_seed_fixtures_hold",
            rust_test="generated_frontier_v10_fuzz_seed_fixtures_hold",
        ),
    ]


def composition_records(objectives):
    records = []
    if objective_enabled(objectives, "parallel_schedule_stress") or objective_enabled(objectives, "security"):
        records.append(v10_scenario(
            "horizon_v10_parallel_multi_deploy_permutation_stress",
            '@0!("v10-parallel") | @1!("v10-parallel")',
            "parallel_multi_deploy_permutation_stress",
            "Parallel multi-deploy stress fixtures must preserve canonical digest under order changes and detect deploy mutation.",
            [
                v10_event("parallel/deploy0/path0", 1, deploy=0, path=[0]),
                v10_event("parallel/deploy1/path0", 1, deploy=1, path=[0]),
                v10_event("parallel/deploy0/path1", 2, deploy=0, path=[1]),
                v10_event("parallel/deploy1/path1", 2, deploy=1, path=[1]),
            ],
            10,
            threat_family="hybrid_parallel_stress",
            expected_outcome="parallel_schedule_safe",
            fuzz_target="runtime_budget_admission",
            fuzz_seed_kind="multi_deploy_permutation",
            mutator_family="deploy_domain_mutation",
            production_replay_target="generated_frontier_v10_parallel_schedule_stress_fixtures_hold",
            rust_test="generated_frontier_v10_parallel_schedule_stress_fixtures_hold",
        ))
    if objective_enabled(objectives, "settlement_refund") or objective_enabled(objectives, "security"):
        records.append(v10_scenario(
            "horizon_v10_settlement_refund_no_fuel_matrix",
            '@0!("v10-settlement")',
            "settlement_refund_no_fuel_matrix",
            "Settlement refund matrix fixtures must preserve escrow/refund arithmetic without replenishing runtime fuel.",
            [v10_event("settlement/refund", 2, path=[0])],
            6,
            threat_family="hybrid_settlement_matrix",
            expected_outcome="settlement_refund_isolated",
            production_oracle="casper_auth_composition",
            differential_axes=["cost", "digest", "count", "refund", "block_hash"],
            settlement={"authority": "casper", "escrow": 15, "token_cost": 5, "refund": 10},
            fuzz_target="settlement_refund_no_fuel",
            fuzz_seed_kind="escrow_refund_matrix",
            mutator_family="settlement_refund",
            production_replay_target="generated_frontier_v10_casper_block_auth_fixtures_hold",
            rust_test="generated_frontier_v10_casper_block_auth_fixtures_hold",
        ))
    if objective_enabled(objectives, "slashing_isolation") or objective_enabled(objectives, "security"):
        records.append(v10_scenario(
            "horizon_v10_slashing_cost_invalid_isolation_matrix",
            '@0!("v10-slashing")',
            "slashing_cost_invalid_isolation_matrix",
            "Slashing isolation fixtures must keep cost-invalid evidence post-evaluation and user runtime cost unchanged.",
            [v10_event("slashing/cost_invalid", 2, path=[0])],
            6,
            threat_family="hybrid_slashing_matrix",
            expected_outcome="slashing_cost_invalid_isolated",
            production_oracle="casper_auth_composition",
            differential_axes=["cost", "digest", "count", "slashing", "block_hash"],
            settlement={"kind": "slash_after_evaluation", "authority": "casper", "escrow": 12, "token_cost": 4, "refund": 8, "slashing_scope": "post_eval"},
            negative_mutations=["slash_fields", "genesis", "block_hash", "signature"],
            fuzz_target="slashing_cost_invalid_isolation",
            fuzz_seed_kind="slashing_evidence_matrix",
            mutator_family="slashing_cost_invalid",
            production_replay_target="generated_frontier_v10_casper_block_auth_fixtures_hold",
            rust_test="generated_frontier_v10_casper_block_auth_fixtures_hold",
        ))
    if objective_enabled(objectives, "legacy_downgrade") or objective_enabled(objectives, "security"):
        records.append(v10_scenario(
            "horizon_v10_legacy_downgrade_quarantine_matrix",
            '@0!("v10-legacy")',
            "legacy_downgrade_quarantine_matrix",
            "Legacy downgrade fixtures must keep absent cost traces quarantined from post-activation replay.",
            [v10_event("legacy/downgrade", 1, path=[0])],
            4,
            threat_family="hybrid_legacy_quarantine",
            expected_outcome="legacy_downgrade_quarantined",
            replay_mutations=["cost_trace_present", "cost_trace_digest", "cost_trace_event_count"],
            negative_mutations=["cost_trace_present", "cost_trace_digest", "cost_trace_event_count", "block_hash"],
            fuzz_target="replay_payload_cost_fields",
            fuzz_seed_kind="legacy_absent_trace_downgrade",
            mutator_family="legacy_replay_downgrade",
            production_replay_target="generated_frontier_v10_replay_payload_matrix_fixtures_hold",
            rust_test="generated_frontier_v10_replay_payload_matrix_fixtures_hold",
        ))
    records.append(v10_scenario(
        "horizon_v10_coverage_adequacy",
        "Nil",
        "v10_coverage_adequacy",
        "The V10 frontier must fail closed if required hybrid fuzz/Kani/security families disappear.",
        [],
        0,
        threat_family="coverage_adequacy",
        expected_outcome="coverage_adequacy",
        production_oracle="runtime_budget",
        eval_phlo=1,
        bounded_depth=1,
        production_replay_target="generated_frontier_v10_coverage_adequacy_holds",
        promotion_gate="v10_adequacy_gate",
        rust_test="generated_frontier_v10_coverage_adequacy_holds",
    ))
    return records


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
            "fuzz_target": scenario.get("fuzz_target", ""),
            "fuzz_seed_kind": scenario.get("fuzz_seed_kind", ""),
            "kani_harness": scenario.get("kani_harness", ""),
            "production_replay_target": scenario.get("production_replay_target", ""),
            "promotion_gate": scenario.get("promotion_gate", ""),
        },
        assertions=[
            "classification != unexpected",
            "promotion_gate != empty",
            "production_replay_target != empty",
            "rust_replay_before_source_action",
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
        "rho_source": scenario.get("rho_source", ""),
        "production_oracle": scenario.get("production_oracle", ""),
        "expected_outcome": scenario.get("expected_outcome", ""),
        "differential_axes": scenario.get("differential_axes", []),
        "eval_phlo": int(scenario.get("eval_phlo", 0)),
        "expected_error_kind": scenario.get("expected_error_kind", ""),
        "eval_result_axes": scenario.get("eval_result_axes", []),
        "rho_source_digest": scenario.get("rho_source_digest", ""),
        "replay_mode": scenario.get("replay_mode", ""),
        "term_family": scenario.get("term_family", ""),
        "term_parameters": scenario.get("term_parameters", {}),
        "external_service_mode": scenario.get("external_service_mode", ""),
        "expected_play_replay_relation": scenario.get("expected_play_replay_relation", ""),
        "source_corpus_case": scenario.get("source_corpus_case", {}),
        "adequacy_budget": scenario.get("adequacy_budget", {}),
        "fuzz_target": scenario.get("fuzz_target", ""),
        "fuzz_seed_kind": scenario.get("fuzz_seed_kind", ""),
        "kani_harness": scenario.get("kani_harness", ""),
        "bounded_depth": int(scenario.get("bounded_depth", 0)),
        "mutator_family": scenario.get("mutator_family", ""),
        "production_replay_target": scenario.get("production_replay_target", ""),
        "promotion_gate": scenario.get("promotion_gate", ""),
        "production_disposition": production_disposition(item["classification"]),
        "production_guards": production_guards(item["name"], item["classification"], scenario),
        "trace_digest_fields": scenario.get("trace_digest_fields", []),
    }


def frontier_records(args):
    records = []
    records.extend(fuzz_records(args.profile, args.search_mode, args.objectives, args.source_root, args.source_limit))
    records.extend(kani_records(args.objectives))
    records.extend(composition_records(args.objectives))
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
        "fuzz_runtime_budget_admission_boundaries",
        "fuzz_replay_payload_cost_fields",
        "fuzz_lifecycle_trace_sequence",
        "fuzz_casper_block_auth_fields",
        "fuzz_external_service_error_replay",
        "fuzz_rholang_corpus_mutation",
        "kani_budget_conservation_bound",
        "kani_invalid_admission_no_mutation",
        "kani_oop_single_boundary",
        "parallel_multi_deploy_permutation_stress",
        "settlement_refund_no_fuel_matrix",
        "slashing_cost_invalid_isolation_matrix",
        "legacy_downgrade_quarantine_matrix",
        "v10_coverage_adequacy",
    ])
    required_features = set([
        "fuzz_target",
        "fuzz_seed_kind",
        "kani_harness",
        "bounded_depth",
        "mutator_family",
        "production_replay_target",
        "promotion_gate",
        "coverage_adequacy",
    ])
    missing_families = sorted(required_families - families)
    missing_features = sorted(required_features - features)
    if missing_families or missing_features or not {"confirmed_safe", "bisimilar"}.issubset(classes):
        raise SystemExit(
            "v10 adequacy failure: missing_families={} missing_features={} classes={}".format(
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
