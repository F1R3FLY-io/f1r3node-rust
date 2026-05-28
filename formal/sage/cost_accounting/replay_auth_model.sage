import argparse
import json
import os
import sys

load(os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "scenario_schema.sage"))


# ─────────────────────────────────────────────────────────────────────────────
# TM-CA-151 — DIAGNOSTIC-REFINEMENT LEVEL (not the production consensus surface).
# This model authenticates a *digest-inclusive* replay payload: FIELDS includes
# `digest`, `digest_present`, and `event_count`. Per TM-CA-151
# (docs/theory/cost-accounting-threat-model.md) those per-operation cost-trace
# quantities are DIAGNOSTIC/TELEMETRY ONLY and were removed from production
# consensus (the replay comparison and the signed block-hash preimage). The
# production consensus surface is total_cost (clamped to initial on OOP) +
# status + post-state hash (modeled here by the non-digest fields: cost,
# signature, failed, system_error, genesis, system_kind). The digest-inclusive
# scenarios below remain valid statements about a strictly-finer refinement
# level; they are NOT claims that the per-operation digest is consensus.
# ─────────────────────────────────────────────────────────────────────────────


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
            "Each field participates in the authenticated payload abstraction at the digest-inclusive diagnostic-refinement level (TM-CA-151: the digest/digest_present/event_count fields are diagnostic, not part of the production consensus replay fingerprint).",
            canonical_scenario("replay_mutations", replay_fields=base, expected_classification="confirmed_safe"),
            {"base": base, "mutations": mutation_rows, "all_changed": all(row["changed"] for row in mutation_rows)},
            ["Rust: replay_payload_hash field-sensitivity tests", "TLA+: CostAccountingThreats"],
        ),
        record(
            "replay_authentication",
            "confirmed_safe",
            "sage_cost_missing_digest_rejected_after_activation",
            "A post-activation payload with no cost-trace digest remains replay-invalid at the digest-inclusive diagnostic-refinement level even when the event count is zero (TM-CA-151: this is NOT a production consensus rejection; production replay does not reject on digest presence).",
            canonical_scenario("missing_digest", replay_fields=missing, projection={"activation": "cost_accounted"}, expected_classification="confirmed_safe"),
            {"payload": missing, "replay_valid": False},
            ["Rocq: rb_cost_accounted_replay_rejects_absent_commitment (diagnostic-refinement)", "Rust: consensus commitment test removed with TM-CA-151"],
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
