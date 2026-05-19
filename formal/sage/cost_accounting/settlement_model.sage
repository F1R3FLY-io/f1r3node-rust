import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


I64_MAX = 2**63 - 1


def checked_mul(left, right):
    value = int(left) * int(right)
    if value < 0 or value > I64_MAX:
        return None
    return value


def refund(phlo_limit, phlo_price, token_cost):
    if phlo_limit < 0 or phlo_price < 0 or token_cost < 0:
        return {"valid": False, "reason": "negative_input"}
    escrow = checked_mul(phlo_limit, phlo_price)
    if escrow is None:
        return {"valid": False, "reason": "escrow_overflow"}
    refundable_tokens = max(0, phlo_limit - token_cost)
    refund_value = checked_mul(refundable_tokens, phlo_price)
    if refund_value is None:
        return {"valid": False, "reason": "refund_overflow"}
    return {
        "valid": True,
        "escrow": int(escrow),
        "charged": int(min(token_cost, phlo_limit) * phlo_price),
        "refund": int(refund_value),
        "fuel_after_settlement": int(max(0, phlo_limit - token_cost)),
    }


def records():
    bounded = refund(10, 3, 4)
    exhausted = refund(10, 3, 12)
    overflow = refund(I64_MAX, 2, 1)
    multi = [refund(10, 2, 3), refund(5, 4, 5), refund(7, 0, 3)]
    multi_refund = sum(item.get("refund", 0) for item in multi if item["valid"])
    multi_escrow = sum(item.get("escrow", 0) for item in multi if item["valid"])

    return [
        record(
            "settlement",
            "confirmed_safe",
            "sage_cost_refund_is_bounded_by_escrow",
            "For valid deploy terms, refund is non-negative and never exceeds escrow.",
            canonical_scenario("bounded_refund", phlo_limit=10, phlo_price=3, token_cost=4, settlement={"kind": "refund"}, expected_classification="confirmed_safe"),
            bounded,
            ["Rocq: uc_ca_009_refund_is_bounded_by_escrow", "Rust: refund_amount_property_is_bounded_by_valid_escrow"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_cost_exhaustion_refunds_zero",
            "A deploy that exhausts or exceeds its token limit refunds zero settlement value.",
            canonical_scenario("exhausted_refund", phlo_limit=10, phlo_price=3, token_cost=12, settlement={"kind": "refund"}, expected_classification="confirmed_safe"),
            exhausted,
            ["Rocq: refund_zero_when_exhausted", "Rust: settlement_edge_cases_are_total_and_deterministic"],
        ),
        record(
            "settlement",
            "confirmed_safe",
            "sage_cost_overflowing_settlement_rejected",
            "Settlement values outside i64 are invalid before fee settlement.",
            canonical_scenario("overflow_refund", phlo_limit=I64_MAX, phlo_price=2, token_cost=1, settlement={"kind": "overflow"}, expected_classification="confirmed_safe"),
            overflow,
            ["Rust/Kani: checked_total_phlo_charge and refund_amount_for_token_cost"],
        ),
        record(
            "settlement",
            "proof_or_model_strengthening",
            "sage_cost_multi_deploy_settlement_adds_independently",
            "Block settlement is the sum of independent deploy-local settlement results.",
            canonical_scenario("multi_deploy_settlement", settlement={"kind": "multi_deploy"}, projection={"deploys": len(multi)}, expected_classification="proof_or_model_strengthening"),
            {"deploys": multi, "total_refund": int(multi_refund), "total_escrow": int(multi_escrow)},
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
