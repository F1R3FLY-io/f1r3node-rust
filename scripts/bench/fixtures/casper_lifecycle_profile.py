def generate(request):
    return {"workloads": [{"scenario_id": request["scenario_id"], "action": "fixture-observation", "seed": request["seed"]}], "faults": request["fault_schedule"]}


def collect(manifest, artifacts):
    if manifest["evidence_kind"] != "synthetic_fixture":
        raise ValueError("The lifecycle adapter requires synthetic evidence.")
    return artifacts


def classify(request, observations, acknowledgments):
    observed = [item for item in observations if item["presence"] == "observed"]
    failures = [{"event_id": item["event_id"], "expected": request["expected"], "observed": item["payload"]} for item in observed if item["payload"] != request["expected"]]
    return {"scenario_verdict": "product_failure" if failures else "passed" if observed else "incomplete", "product_failures": failures}
