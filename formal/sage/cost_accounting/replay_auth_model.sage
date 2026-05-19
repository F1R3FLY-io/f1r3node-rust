import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


FIELDS = [
    "cost",
    "digest_present",
    "digest",
    "event_count",
    "signature",
    "failed",
    "system_error",
    "genesis",
    "system_kind",
]


def replay_payload(fields):
    return tuple((field, fields.get(field)) for field in FIELDS)


def mutations(base):
    out = []
    for field in FIELDS:
        changed = dict(base)
        value = changed.get(field)
        if isinstance(value, bool):
            changed[field] = not value
        elif isinstance(value, int):
            changed[field] = value + 1
        elif value is None:
            changed[field] = "present"
        else:
            changed[field] = str(value) + "-mutated"
        out.append({"field": field, "changed": replay_payload(changed) != replay_payload(base)})
    return out


def records():
    base = {
        "cost": 7,
        "digest_present": True,
        "digest": "good",
        "event_count": 2,
        "signature": "sig-a",
        "failed": False,
        "system_error": None,
        "genesis": False,
        "system_kind": "close",
    }
    mutation_rows = mutations(base)
    missing = dict(base)
    missing["digest_present"] = False
    missing["digest"] = ""
    missing["event_count"] = 0

    return [
        record(
            "replay_authentication",
            "confirmed_safe",
            "sage_cost_replay_payload_mutations_are_observable",
            "Each replay-relevant field participates in the authenticated payload abstraction.",
            canonical_scenario("replay_mutations", replay_fields=base, expected_classification="confirmed_safe"),
            {"base": base, "mutations": mutation_rows, "all_changed": all(row["changed"] for row in mutation_rows)},
            ["Rust: replay_payload_hash field-sensitivity tests", "TLA+: CostAccountingThreats"],
        ),
        record(
            "replay_authentication",
            "confirmed_safe",
            "sage_cost_missing_digest_rejected_after_activation",
            "A post-activation payload with no cost-trace digest remains replay-invalid even when the event count is zero.",
            canonical_scenario("missing_digest", replay_fields=missing, projection={"activation": "cost_accounted"}, expected_classification="confirmed_safe"),
            {"payload": missing, "replay_valid": False},
            ["Rocq: rb_cost_accounted_replay_rejects_absent_commitment", "Rust: replaycomputestate_should_require_cost_trace_after_activation"],
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
