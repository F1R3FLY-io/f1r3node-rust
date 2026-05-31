import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


I64_MAX = 2**63 - 1


# D3 (DR-9, OD-2/OD-3): the singular-phlo escrow refund model (escrow =
# limit * price, refund = (limit - token_cost) * price) is REMOVED. A deploy's
# cost is the per-COMM token count `demand` (= Delta_s); funding is the
# per-signature supply pool `supply` (= Sigma_s); the block-assembly gate admits
# iff the EFFECTIVE supply meets the demand plus the genesis safety `margin`, and
# the SINGLE consensus decrement is the settlement debit `post = supply - demand`
# (applied once at block close), which must never underflow for an admitted
# deploy. There is NO op-budget-exhaustion surface and NO per-deploy refund.
def is_funded(demand, supply, margin):
    # The pure Def-19/Thm-20 funding inequality (i128 in Rust; unbounded here).
    return int(supply) >= int(demand) + int(margin)


def settle(demand, supply, margin):
    if demand < 0 or supply < 0 or margin < 0:
        return {"valid": False, "reason": "negative_input"}
    funded = is_funded(demand, supply, margin)
    # The per-COMM settlement debit is the demand (COMM count) for an admitted
    # deploy; an unfunded deploy is rejected and debits nothing.
    debit = int(demand) if funded else 0
    post = int(supply) - debit
    return {
        "valid": True,
        "demand": int(demand),
        "supply": int(supply),
        "margin": int(margin),
        "funded": bool(funded),
        # The single consensus decrement: post = pre - debit (>= 0 for admitted).
        "settlement_debit": debit,
        "supply_after": int(post),
    }


def records():
    # Funded boundary: Sigma = Delta + margin admits, and the debit (= Delta)
    # leaves a non-negative pool (no underflow).
    funded = settle(demand=8, supply=10, margin=2)
    # Just below the margin: Sigma = Delta + margin - 1 is REJECTED (no debit).
    rejected = settle(demand=8, supply=9, margin=2)
    # Drained pool: a present-but-zero supply rejects a further per-COMM demand
    # (the §7.7 duplicate-deploy double-spend shape).
    drained = settle(demand=3, supply=0, margin=0)
    # Block settlement is the sum of independent per-signature pool debits.
    multi = [settle(8, 10, 0), settle(5, 4, 0), settle(3, 3, 0)]
    multi_debit = sum(item.get("settlement_debit", 0) for item in multi if item["valid"])
    multi_supply_after = sum(item.get("supply_after", 0) for item in multi if item["valid"])

    return [
        record(
            "settlement",
            "confirmed_safe",
            "sage_per_comm_funding_admits_when_supply_meets_demand_plus_margin",
            "A deploy is admitted iff Sigma_s >= Delta_s + margin; its settlement debit (= the per-COMM demand) never underflows the supply pool.",
            canonical_scenario("funded_admission", settlement={"kind": "per_comm_settle", "demand": 8, "supply": 10, "margin": 2}, expected_classification="confirmed_safe"),
            funded,
            ["Rocq: consumed_fuel_count_eq_token_drop / funded_settlement_debit_never_underflows_supply (kani)", "Rust: settlement_debit_equals_comm_count"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_per_comm_reject_below_demand_plus_margin",
            "Sigma_s strictly below Delta_s + margin is rejected and debits nothing (§7.7 reject direction).",
            canonical_scenario("rejected_admission", settlement={"kind": "per_comm_settle", "demand": 8, "supply": 9, "margin": 2}, expected_classification="confirmed_safe"),
            rejected,
            ["Rocq: reject_below_demand_plus_margin (kani)", "Rust: funded_unfunded_boundary_at_margin"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_per_comm_drained_pool_rejects_double_spend",
            "A present-but-drained supply (Sigma = 0) rejects a further per-COMM demand — the §7.7 duplicate-deploy double-spend shape.",
            canonical_scenario("drained_pool", settlement={"kind": "per_comm_settle", "demand": 3, "supply": 0, "margin": 0}, expected_classification="confirmed_safe"),
            drained,
            ["Rust: drained_present_pool_rejects"],
        ),
        record(
            "settlement",
            "proof_or_model_strengthening",
            "sage_per_comm_block_settlement_adds_independently",
            "Block settlement is the sum of independent per-signature pool debits (each = the admitted deploy's per-COMM demand).",
            canonical_scenario("multi_pool_settlement", settlement={"kind": "multi_pool"}, projection={"pools": len(multi)}, expected_classification="proof_or_model_strengthening"),
            {"pools": multi, "total_settlement_debit": int(multi_debit), "total_supply_after": int(multi_supply_after)},
            ["Rust: generated cost frontier replay fixtures", "Sage: objective frontier"],
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
