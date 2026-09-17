import hashlib
import json
import os
from pathlib import Path
import sys


try:
    request = json.loads(Path(sys.argv[1]).read_text())["request"]
except (OSError, ValueError, KeyError, IndexError):
    print("The fixture request is unavailable or invalid.", file=sys.stderr)
    sys.exit(2)
output = Path(sys.argv[2])
mode = request.get("mode", "complete")
value = request["expected"]
if mode == "mismatch" or (mode == "failure-then-stop" and request["iteration"] == 1):
    value = "planted-mismatch"
raw = (json.dumps(value) + "\n").encode()
(output / "sample.json").write_bytes(raw)
context = request["required_observations"][0]
record = {**context, "schema_version": 1, "manifest_digest": request["manifest_digest"], "run_id": request["run_id"], "scenario_id": request["scenario_id"], "pair_id": request["pair_id"], "segment": request["segment"], "iteration": request["iteration"], "record_id": "record-1", "producer": "fixture-executor", "producer_sequence": "1", "event_id": "event-1", "presence": "observed", "payload": value, "observation_time": {"clock_id": "fixture-monotonic", "monotonic_ns": "1", "utc": "2026-01-01T00:00:00Z"}, "raw_reference": {"path": "sample.json", "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(), "producer": "fixture-executor", "capture_state": "complete", "observation_ids": ["record-1"]}}
records = [record]
if mode == "missing":
    records = []
elif mode == "wrong-identity":
    record["incarnation"] = "unrelated-incarnation"
elif mode == "missing-zero":
    record.update(presence="missing", payload=0, reason="unavailable")
elif mode == "missing-artifact":
    (output / "sample.json").unlink()
elif mode == "duplicate":
    record["raw_reference"]["observation_ids"].append("record-2")
    records.append({**record, "record_id": "record-2", "producer_sequence": "2"})
(output / "transport.json").write_text(json.dumps({"observations": records}) + "\n")
if mode == "failure-then-stop" and request["iteration"] == 2:
    (Path(os.environ["SOAK_OUTPUT_DIR"]) / "host-guardian-breach.txt").write_text("Synthetic resource stop.\n")
