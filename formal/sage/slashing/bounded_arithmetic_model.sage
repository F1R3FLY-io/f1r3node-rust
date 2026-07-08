import argparse
import json
import sys

from sage.all import Integer


def bounds(bits, signed):
    if signed:
        return -(Integer(2) ** Integer(bits - 1)), Integer(2) ** Integer(bits - 1) - Integer(1)
    return Integer(0), Integer(2) ** Integer(bits) - Integer(1)


def project(value, bits, signed, mode):
    low, high = bounds(bits, signed)
    value = Integer(value)
    if low <= value <= high:
        return {"ok": True, "value": int(value)}
    if mode == "checked":
        return {"ok": False, "value": None}
    if mode == "saturating":
        return {"ok": True, "value": int(low if value < low else high)}
    if mode == "wrapping":
        modulus = Integer(2) ** Integer(bits)
        wrapped = value % modulus
        if signed and wrapped > high:
            wrapped = wrapped - modulus
        return {"ok": True, "value": int(wrapped)}
    raise ValueError(mode)


def analyze_boundary(bits, signed, mode):
    low, high = bounds(bits, signed)
    exact = high + Integer(1)
    projected = project(exact, bits, signed, mode)
    return {
        "model": "sage_bounded_arithmetic_projection",
        "bits": bits,
        "signed": signed,
        "mode": mode,
        "low": int(low),
        "high": int(high),
        "vault": int(high),
        "bond": 1,
        "exact_sum": int(exact),
        "projected": projected,
        "property": "bounded_projection_matches_exact_arithmetic",
        "holds": projected["ok"] and projected["value"] == int(exact),
    }


def run_analysis(bits, signed):
    records = [analyze_boundary(bits, signed, mode) for mode in ["checked", "wrapping", "saturating"]]
    return {"summaries": [{"bits": bits, "signed": signed, "failures": len([record for record in records if not record["holds"]])}], "records": records}


def self_test():
    signed_result = run_analysis(64, True)
    unsigned_result = run_analysis(64, False)
    if not all(not record["holds"] for record in signed_result["records"]):
        raise AssertionError("signed overflow boundary did not diverge")
    if not all(not record["holds"] for record in unsigned_result["records"]):
        raise AssertionError("unsigned overflow boundary did not diverge")
    return signed_result


def print_summary(result):
    for summary in result["summaries"]:
        print("bits={bits} signed={signed} failures={failures}".format(**summary))
    for record in result["records"]:
        print(
            "mode={mode} high={high} exact_sum={exact_sum} projected={projected} holds={holds}".format(
                **record
            )
        )


def main(argv):
    parser = argparse.ArgumentParser(description="Sage model for bounded arithmetic projections")
    parser.add_argument("--bits", type=int, default=64)
    parser.add_argument("--signed", action="store_true")
    parser.add_argument("--unsigned", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    if args.bits < 2:
        parser.error("--bits must be at least 2")
    signed = args.signed and not args.unsigned
    result = self_test() if args.self_test else run_analysis(args.bits, signed)
    print_summary(result)
    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as handle:
            json.dump(result, handle, indent=2, sort_keys=True)
            handle.write("\n")


argv = sys.argv[1:]
if argv and argv[0] == "--":
    argv = argv[1:]
main(argv)
