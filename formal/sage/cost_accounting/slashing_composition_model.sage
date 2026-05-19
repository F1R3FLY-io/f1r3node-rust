import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


def records():
    settlement_inputs = {"limit": 10, "price": 3, "token_cost": 4}
    preserved = dict(settlement_inputs)

    return [
        record(
            "slashing_composition",
            "confirmed_safe",
            "sage_slashing_preserves_settlement_inputs",
            "Cost-invalid slashing evidence does not rewrite deploy settlement inputs.",
            canonical_scenario(
                "slashing_preserves_settlement_inputs",
                settlement={"kind": "slash_after_evaluation"},
                threat_family="slashing_composition",
                expected_invariants=["slash_preserves_fee_settlement_inputs"],
                promotion_target="rocq:uc_ca_073",
                expected_classification="confirmed_safe",
            ),
            {"slashing": "post_evaluation", "before": settlement_inputs, "after": preserved},
            ["Rocq: uc_ca_073_slashing_composition_frontier", "Rust: slashing replay/hash tests"],
        ),
        record(
            "slashing_composition",
            "confirmed_safe",
            "sage_slashing_cannot_add_runtime_fuel",
            "Slashing system effects are unmetered system evidence and cannot replenish user runtime fuel.",
            canonical_scenario(
                "slashing_no_runtime_fuel",
                initial_budget=4,
                settlement={"kind": "slash_after_evaluation"},
                threat_family="slashing_composition",
                expected_invariants=["slash_system_effect_is_unmetered_for_user_budget"],
                promotion_target="rocq:uc_ca_073",
                expected_classification="confirmed_safe",
            ),
            {"slashing": "system_effect", "fuel_before": 0, "fuel_after": 0, "fuel_added": False},
            ["Rocq: slash_system_effect_is_unmetered_for_user_budget", "Rust: slashing integration tests"],
        ),
        record(
            "slashing_composition",
            "projection_risk",
            "sage_slashing_stale_evidence_requires_boundary_check",
            "Stale cost-invalid evidence is a projection risk unless the production slashing boundary rejects it.",
            canonical_scenario(
                "slashing_stale_cost_evidence",
                rust_replay={"evidence_epoch": "stale"},
                threat_family="slashing_composition",
                expected_invariants=["stale_cost_evidence_rejected"],
                promotion_target="rust:slashing-boundary",
                expected_classification="projection_risk",
            ),
            {"slashing": "stale_evidence", "accepted": False, "requires_boundary_guard": True},
            ["Rocq: stale_cost_evidence_sound", "Rust: stale evidence slashing tests"],
        ),
    ]


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)
    output = {"records": records()}
    output["coverage_summary"] = coverage_summary(output["records"])
    text = json.dumps(output, indent=2, sort_keys=True, default=schema_json_default)
    if args.json_out:
        with open(args.json_out, "w") as handle:
            handle.write(text + "\n")
    else:
        print(text)


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
