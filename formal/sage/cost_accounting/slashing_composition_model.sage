import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


def current_authorized(parent_bond, evidence_epoch, target_epoch, current_epoch):
    return int(parent_bond) > 0 and int(evidence_epoch) == int(current_epoch) and int(target_epoch) == int(current_epoch)


def records():
    settlement_inputs = {"limit": 10, "price": 3, "token_cost": 4}
    preserved = dict(settlement_inputs)
    current_parent = {
        "parent_pre_state_bond": 1,
        "ambient_bond": 0,
        "execution_bond": 1,
        "evidence_epoch": 2,
        "target_activation_epoch": 2,
        "current_epoch": 2,
    }
    ambient_only = {
        "parent_pre_state_bond": 0,
        "ambient_bond": 1,
        "execution_bond": 1,
        "evidence_epoch": 2,
        "target_activation_epoch": 2,
        "current_epoch": 2,
    }
    stale_recovered = {
        "parent_pre_state_bond": 1,
        "ambient_bond": 1,
        "execution_bond": 1,
        "evidence_epoch": 1,
        "target_activation_epoch": 2,
        "current_epoch": 2,
    }
    zero_bond_noop = {
        "parent_pre_state_bond": 1,
        "ambient_bond": 0,
        "execution_bond": 0,
        "evidence_epoch": 2,
        "target_activation_epoch": 2,
        "current_epoch": 2,
    }

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
            "confirmed_safe",
            "sage_slashing_stale_evidence_requires_boundary_check",
            "Stale cost-invalid evidence is rejected at the recovered slash boundary.",
            canonical_scenario(
                "slashing_stale_cost_evidence",
                rust_replay={"evidence_epoch": "stale", "target_activation_epoch": "current"},
                slashing_authorization=stale_recovered,
                threat_family="slashing_composition",
                expected_invariants=["stale_recovered_slash_not_authorized"],
                promotion_target="rocq:uc_ca_146",
                expected_classification="confirmed_safe",
            ),
            {
                "slashing": "stale_evidence",
                "accepted": current_authorized(
                    stale_recovered["parent_pre_state_bond"],
                    stale_recovered["evidence_epoch"],
                    stale_recovered["target_activation_epoch"],
                    stale_recovered["current_epoch"],
                ),
                "requires_current_evidence": True,
            },
            ["Rocq: stale_recovered_slash_not_authorized", "Rust: non_current_rejected_slash_is_not_recovered"],
        ),
        record(
            "slashing_composition",
            "confirmed_safe",
            "sage_slashing_current_parent_pre_state_authorizes_without_user_cost_mutation",
            "Current cost-invalid evidence with a parent pre-state bond authorizes a slash without mutating user cost.",
            canonical_scenario(
                "slashing_current_parent_pre_state_authorization",
                settlement={"kind": "slash_after_evaluation"},
                slashing_authorization=current_parent,
                threat_family="slashing_composition",
                expected_invariants=[
                    "current_cost_evidence_epoch_sound",
                    "parent_pre_state_authorized_slash_preserves_cost_boundary",
                ],
                promotion_target="rocq:uc_ca_147",
                expected_classification="confirmed_safe",
            ),
            {
                "slashing": "parent_pre_state_authorization",
                "authorized": current_authorized(
                    current_parent["parent_pre_state_bond"],
                    current_parent["evidence_epoch"],
                    current_parent["target_activation_epoch"],
                    current_parent["current_epoch"],
                ),
                "cost_boundary_preserved": True,
            },
            ["Rocq: uc_ca_147_parent_pre_state_slash_authorization_preserves_cost_boundary", "Rust: slash_authorization_regressions"],
        ),
        record(
            "slashing_composition",
            "confirmed_safe",
            "sage_slashing_parent_zero_rejects_ambient_positive",
            "An ambient positive bond cannot authorize a slash when the parent pre-state bond is zero.",
            canonical_scenario(
                "slashing_parent_zero_ambient_positive",
                rust_replay={"ambient_bond": "positive", "parent_pre_state_bond": 0},
                slashing_authorization=ambient_only,
                threat_family="slashing_composition",
                expected_invariants=["ambient_bond_does_not_authorize_without_parent_pre_state"],
                promotion_target="rocq:uc_ca_147",
                expected_classification="confirmed_safe",
            ),
            {
                "slashing": "ambient_only_rejection",
                "authorized": current_authorized(
                    ambient_only["parent_pre_state_bond"],
                    ambient_only["evidence_epoch"],
                    ambient_only["target_activation_epoch"],
                    ambient_only["current_epoch"],
                ),
                "ambient_bond_positive": True,
            },
            ["Rocq: ambient_bond_does_not_authorize_without_parent_pre_state", "Rust: slash_authorization_regressions"],
        ),
        record(
            "slashing_composition",
            "confirmed_safe",
            "sage_recovered_rejected_slash_requires_current_evidence",
            "Recovered rejected slashes are recoverable only when their evidence and target activation epochs are current.",
            canonical_scenario(
                "recovered_rejected_slash_current_evidence",
                rust_replay={"recovered_rejected": True, "evidence_epoch": "current"},
                slashing_authorization=current_parent,
                threat_family="slashing_composition",
                expected_invariants=["recovered_rejected_slash_requires_current_cost_evidence"],
                promotion_target="rocq:uc_ca_146",
                expected_classification="confirmed_safe",
            ),
            {"slashing": "recovered_rejected", "recovered": True, "current_evidence": True},
            ["Rocq: uc_ca_146_recovered_slash_requires_current_cost_evidence", "Rust: rejected_slash::non_current_rejected_slash_is_not_recovered"],
        ),
        record(
            "slashing_composition",
            "confirmed_safe",
            "sage_slash_target_epoch_is_replay_authenticated",
            "Changing a slash target activation epoch changes the authenticated replay payload.",
            canonical_scenario(
                "slash_target_epoch_replay_authentication",
                replay_mutations=["slash_fields", "target_activation_epoch", "evidence_epoch", "cost_trace_digest"],
                slashing_authorization=current_parent,
                threat_family="slashing_composition",
                expected_invariants=["rb_full_replay_payload_slash_target_epoch_change_detected"],
                promotion_target="rocq:uc_ca_148",
                expected_classification="confirmed_safe",
            ),
            {"slashing": "replay_authentication", "target_epoch_mutation_detected": True},
            ["Rocq: uc_ca_148_slash_target_epoch_is_replay_authenticated", "Rust: cost_accounting_v14_replay_slashing_oracles_hold"],
        ),
        record(
            "slashing_composition",
            "confirmed_safe",
            "sage_zero_bond_slash_noop_preserves_cost_boundary",
            "A zero-bond slash represented as a no-op preserves the cost-accounting boundary.",
            canonical_scenario(
                "zero_bond_slash_noop",
                settlement={"kind": "slash_after_evaluation"},
                slashing_authorization=zero_bond_noop,
                threat_family="slashing_composition",
                expected_invariants=["zero_bond_slash_noop_preserves_cost_boundary"],
                promotion_target="rocq:uc_ca_149",
                expected_classification="confirmed_safe",
            ),
            {"slashing": "zero_bond_noop", "execution_bond": 0, "cost_boundary_preserved": True},
            ["Rocq: uc_ca_149_zero_bond_slash_noop_preserves_cost_boundary", "Rust: slash_authorization_regressions"],
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
