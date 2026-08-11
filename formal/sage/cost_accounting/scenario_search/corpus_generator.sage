import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(sys.argv[0]))), "scenario_schema.sage"))


def corpus_records():
    cases = [
        (
            "corpus_zero_weight_rejection",
            "confirmed_safe",
            "producer_routing",
            canonical_scenario(
                "corpus_zero_weight_rejection",
                events=[canonical_event("primitive", 0, descriptor="empty-variable-work")],
                initial_budget=4,
                threat_family="producer_routing",
                expected_invariants=["zero_weight_rejected_before_mutation"],
                promotion_target="rust:fuzz",
                expected_classification="confirmed_safe",
            ),
            {"accepted": False, "trace_mutated": False},
        ),
        (
            "corpus_oop_boundary",
            "proof_or_model_strengthening",
            "concurrency_schedule",
            canonical_scenario(
                "corpus_oop_boundary",
                events=[canonical_event("source", 5, descriptor="oop", path=[0])],
                initial_budget=3,
                concurrency={"oop": "single_boundary"},
                threat_family="concurrency_schedule",
                expected_invariants=["oop_boundary_singleton"],
                promotion_target="tla:RuntimeBudgetReplay",
                expected_classification="proof_or_model_strengthening",
            ),
            {"oop": "single_boundary", "event_count": 1},
        ),
        (
            "corpus_replay_digest_mutation",
            "confirmed_safe",
            "replay_authentication",
            canonical_scenario(
                "corpus_replay_digest_mutation",
                replay_fields={"cost": 2, "digest": "a", "event_count": 1},
                replay_mutations=["cost_trace_digest"],
                threat_family="replay_authentication",
                expected_invariants=["replay_digest_sensitivity"],
                promotion_target="rust:fuzz",
                expected_classification="confirmed_safe",
            ),
            {"replay_mutation": ["cost_trace_digest"], "accepted": False},
        ),
        (
            "corpus_multi_deploy_settlement",
            "proof_or_model_strengthening",
            "settlement",
            canonical_scenario(
                "corpus_multi_deploy_settlement",
                deploy_count=2,
                settlement={
                    "deploys": [
                        {"escrow": 10, "token_cost": 3, "refund": 7},
                        {"escrow": 6, "token_cost": 6, "refund": 0},
                    ]
                },
                threat_family="settlement",
                expected_invariants=["multi_deploy_settlement_additive"],
                promotion_target="rocq:uc_ca_072",
                expected_classification="proof_or_model_strengthening",
            ),
            {"refund_sum": 7, "escrow_sum": 16},
        ),
        (
            "corpus_descriptor_bound",
            "confirmed_safe",
            "resource_exhaustion",
            canonical_scenario(
                "corpus_descriptor_bound",
                events=[canonical_event("primitive", 1, descriptor="x" * 513)],
                initial_budget=4,
                resource_bounds={"max_descriptor_bytes": 512},
                threat_family="resource_exhaustion",
                expected_invariants=["reject_before_mutation"],
                promotion_target="rust:fuzz",
                expected_classification="confirmed_safe",
            ),
            {"descriptor_bytes": 513, "trace_mutated": False},
        ),
    ]
    records = []
    for name, classification, axis, scenario, witness in cases:
        records.append(
            record(
                "scenario_corpus",
                classification,
                name,
                "Persistent deterministic corpus entry for {}".format(axis),
                scenario,
                witness,
                ["Rust: generated frontier replay", "docs: cost-accounting-search-horizon"],
            )
        )
    return records


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-out")
    parser.add_argument("--fixture-out")
    args = parser.parse_args(argv)
    records = corpus_records()
    fixtures = [
        scenario_fixture(
            item["name"],
            item["classification"],
            item["scenario"],
            item["deterministic_witness"],
            {
                "classification": item["classification"],
                "promotion_target": item["scenario"].get("promotion_target", "record"),
                "threat_family": item["scenario"].get("threat_family", "search_governance"),
            },
        )
        for item in records
    ]
    output = {
        "records": records,
        "fixtures": fixtures,
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


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
