#!/usr/bin/env python3
import csv
import hashlib
import http.server
import importlib
import json
from math import inf
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import threading
import time
from types import SimpleNamespace
import urllib.request


HISTOGRAMS = {
    "dag_merge_relation_items": ("items", None),
    "dag_merge_relation_branches": ("branches", None),
    "dag_merge_conflict_edges": ("adjacency entries", None),
    "dag_merge_rejection_options": ("options", None),
    "dag_merge_state_application_actions": ("prepared actions", None),
    "dag_merge_rejection_selection_time": ("seconds", None),
    "dag_merge_compute_trie_actions_time": ("seconds", "compute_trie_actions"),
    "dag_merge_apply_trie_actions_time": ("seconds", "apply_trie_actions"),
    "block_replay_phase_user_deploys_work": ("input deploys", None),
    "block_replay_phase_system_deploys_work": ("input deploys", None),
    "runtime_spawn_replay_time": ("seconds", "spawn_replay_runtime"),
    "block_replay_phase_reset_time": ("seconds", "reset"),
    "block_replay_phase_user_deploys_time": ("seconds", "user_deploys"),
    "block_replay_phase_system_deploys_time": ("seconds", "system_deploys"),
    "block_replay_phase_create_checkpoint_time": ("seconds", "checkpoint"),
}
COUNTERS = (
    "block_validation_repeat_deploy_carrier_watermark_engaged",
    "block_validation_repeat_deploy_carrier_watermark_not_ready",
    "block_validation_repeat_deploy_carrier_index_absence",
    "block_validation_repeat_deploy_carrier_index_hit",
    "block_validation_repeat_deploy_carrier_index_read_failure",
    "block_validation_repeat_deploy_carrier_fallback_scan",
    "block_validation_repeat_deploy_carrier_row_reads",
    "block_validation_repeat_deploy_ancestor_metadata_visits",
    "block_validation_repeat_deploy_ancestor_body_reads",
    "runtime_spawn_replay_calls",
    "block_replay_phase_reset_calls",
    "block_replay_phase_create_checkpoint_calls",
)


def workload(harness, case):
    sys.path.insert(0, str(harness / "integration-tests/test"))
    metrics = importlib.import_module("infra.metrics")
    ResourceMonitor = importlib.import_module("infra.resource_monitor").ResourceMonitor

    served = threading.Event()
    advanced = threading.Event()

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, format: str, *args):
            pass

        def do_GET(self):
            if self.path != "/metrics":
                self.send_error(404)
                return
            after = advanced.is_set()
            if case == "missing-baseline" and not after:
                self.send_error(503)
                return
            if case == "decreasing-sums":
                total, count, counter = (2, 9, 4) if after else (10, 8, 12)
            elif case == "decreasing-counts":
                total, count, counter = (1.5, 2, 4) if after else (0.5, 6, 12)
            elif case == "zero-deltas":
                total, count, counter = 0.5, 2, 4
            else:
                total, count, counter = (1.5, 6, 12) if after else (0.5, 2, 4)
            if case in ("nonfinite-before", "nonfinite-after") and after == (case == "nonfinite-after"):
                total = counter = "1e309"
            if case in ("negative-before", "negative-after") and after == (case == "negative-after"):
                total = counter = -1
            lines = []
            for name in HISTOGRAMS:
                lines.extend((
                    f'{name}_sum{{source="fixture"}} {total}',
                    f'{name}_count{{source="fixture"}} {count}',
                    f'{name}_bucket{{source="fixture",le="+Inf"}} {count}',
                ))
            lines.extend(f'{name}{{source="fixture"}} {counter}' for name in COUNTERS)
            body = ("\n".join(lines) + "\n").encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            served.set()

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    class Node:
        name = "rnode.fixture.validator1"
        grpc_host = "127.0.0.1"
        ports = SimpleNamespace(http=server.server_port)

        def http_get(self, path, timeout):
            with urllib.request.urlopen(f"http://127.0.0.1:{server.server_port}{path}", timeout=timeout) as response:
                return SimpleNamespace(text=response.read().decode())

        def resource_usage(self):
            return {"memory_mb": 42.0, "cpu_percent": 3.0, "memory_limit_mb": 128.0}

    node = Node()
    monitor = ResourceMonitor(
        interval=0.1,
        provider=SimpleNamespace(active_handles=[node]),
        output_dir=harness / "integration-tests/data/telemetry-session",
    )
    try:
        before = metrics.scrape_metrics(node)
        advanced.set()
        after = metrics.scrape_metrics(node)
        print(metrics.format_node_metrics(metrics.compute_metric_deltas(before, after)), flush=True)
        served.clear()
        monitor.start()
        if not served.wait(5):
            raise RuntimeError("The monitor did not reach the HTTP observation boundary.")
        time.sleep(1.1)
    finally:
        monitor.stop()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    print("TELEMETRY_WORKLOAD_COMPLETED", flush=True)


def run(source, pinned_harness, evidence, case):
    evidence.mkdir(parents=True, exist_ok=False)
    harness = evidence / "harness"
    shutil.copytree(pinned_harness / "integration-tests/test", harness / "integration-tests/test")
    bindings = {}
    for base, paths in (
        (source, ["scripts/run-merge-recovery-soak.sh", "scripts/bench/write-soak-summary.sh",
                  "scripts/bench/collect-soak-metrics.sh", "scripts/bench/soak-metrics.json",
                  "scripts/bench/extend-issue24-metrics.sh"]),
        (pinned_harness, ["integration-tests/test/infra/metrics.py", "integration-tests/test/infra/resource_monitor.py"]),
    ):
        for path in paths:
            bindings[str(base / path)] = hashlib.sha256((base / path).read_bytes()).hexdigest()
    bindings[str(Path(__file__).resolve())] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    helper = source / "scripts/bench/soak_telemetry.py"
    if helper.exists():
        bindings[str(helper)] = hashlib.sha256(helper.read_bytes()).hexdigest()
    (evidence / "source-bindings.json").write_text(json.dumps(bindings, indent=2) + "\n")
    target = harness / "integration-tests/test/infra/metrics.py"
    with (evidence / "overlay.log").open("w") as log:
        subprocess.run(["bash", str(source / "scripts/bench/extend-issue24-metrics.sh"), str(target)],
                       stdout=log, stderr=subprocess.STDOUT, check=True)
        first = target.read_bytes()
        subprocess.run(["bash", str(source / "scripts/bench/extend-issue24-metrics.sh"), str(target)],
                       stdout=log, stderr=subprocess.STDOUT, check=True)
        if target.read_bytes() != first:
            raise RuntimeError("The overlay is not idempotent.")
    bin_dir = evidence / "bin"
    bin_dir.mkdir()
    commands = {
        "poetry": "exec " + " ".join(shlex.quote(value) for value in
                  [sys.executable, "-I", str(Path(__file__).resolve()), "workload", str(harness), case]) + "\n",
        "docker": "exit 0\n",
        "curl": "exit 22\n",
        "git": "printf 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb refs/heads/fixture\\n'\n",
    }
    for name, command in commands.items():
        path = bin_dir / name
        path.write_text("#!/usr/bin/env bash\nset -eu\n" + command)
        path.chmod(0o755)
    output = evidence / "output"
    environment = {
        "PATH": f"{bin_dir}:/usr/bin:/bin",
        "HOME": str(evidence),
        "LANG": "C.UTF-8",
        "SOAK_DURATION_SECONDS": "120",
        "SYSTEM_INTEGRATION_DIR": str(harness),
        "SOAK_OUTPUT_DIR": str(output),
        "SOAK_NODE_REPO_DIR": str(source),
        "SOAK_TARGET_REF": "refs/heads/fixture",
        "SOAK_TARGET_SHA": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "SOAK_MERGE_EXIT_MIN_SECONDS": "1",
        "SOAK_RUN_BENCHMARKS": "false",
        "SOAK_RSS_CEILING_MB": "0",
        "SOAK_HOST_FREE_FLOOR_MB": "0",
        "SOAK_DISK_FREE_FLOOR_MB": "0",
        "SOAK_GUARDIAN_POLL_SECONDS": "0.1",
        "SOAK_MONITOR_SNAPSHOT_SECONDS": "0.1",
    }
    with (evidence / "driver.log").open("w") as log:
        result = subprocess.run(["bash", str(source / "scripts/run-merge-recovery-soak.sh")],
                                env=environment, stdout=log, stderr=subprocess.STDOUT, timeout=90)
    (evidence / "driver-exit.txt").write_text(f"{result.returncode}\n")
    try:
        summary = json.loads((output / "summary.json").read_text())
    except (OSError, ValueError) as error:
        raise RuntimeError("The driver summary cannot be read.") from error
    if result.returncode != 0 or [summary["iterations"], summary["failures"]] != [1, 0]:
        raise RuntimeError("The public driver did not complete one successful workload.")
    iteration = output / "iteration-00001-docker"
    phase_log = (iteration / "pytest.log").read_text()
    if "TELEMETRY_WORKLOAD_COMPLETED" not in phase_log:
        raise RuntimeError("The workload did not complete its collector lifecycle.")
    try:
        with (iteration / "node-metrics-timeseries.csv").open(newline="") as stream:
            rows = list(csv.DictReader(stream))
        raw = {(row["metric"], float(row["value"])) for row in rows if row["node"] == "rnode.fixture.validator1"}
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise RuntimeError("The raw telemetry artifact cannot be read.") from error
    failures = []
    if case == "decreasing-sums":
        expected_sum, expected_count, expected_counter = 2, 9, 4
    elif case == "decreasing-counts":
        expected_sum, expected_count, expected_counter = 1.5, 2, 4
    elif case == "zero-deltas":
        expected_sum, expected_count, expected_counter = 0.5, 2, 4
    elif case == "nonfinite-after":
        expected_sum, expected_count, expected_counter = inf, 6, inf
    elif case == "negative-after":
        expected_sum, expected_count, expected_counter = -1, 6, -1
    else:
        expected_sum, expected_count, expected_counter = 1.5, 6, 12
    invalid_reason = {
        "missing-baseline": "missing_baseline",
        "decreasing-sums": "cumulative_decrease",
        "decreasing-counts": "cumulative_decrease",
        "nonfinite-before": "nonfinite_sample",
        "nonfinite-after": "nonfinite_sample",
        "negative-before": "negative_sample",
        "negative-after": "negative_sample",
    }.get(case)
    for name, (unit, legacy) in HISTOGRAMS.items():
        for suffix, value in (("_sum", expected_sum), ("_count", expected_count), ("_bucket", expected_count)):
            labels = 'source="fixture",le="+Inf"' if suffix == "_bucket" else 'source="fixture"'
            if (f"{name}{suffix}{{{labels}}}", value) not in raw:
                failures.append(f"The retained raw artifact omits {name}{suffix}.")
        if invalid_reason:
            expected = f"{name}: interval=unavailable reason={invalid_reason}"
            if expected not in phase_log or f"{name}: mean=" in phase_log:
                failures.append(f"The phase summary does not reject {invalid_reason} for {name}.")
        elif case == "zero-deltas":
            if f"{name}: mean=unavailable, observations=0" not in phase_log:
                failures.append(f"The phase summary changes the zero observation count for {name}.")
        else:
            expected = f"{name}: mean=0.25 {unit}, observations=4"
            old = legacy and re.search(rf"^\s+{re.escape(legacy)}: 250(?:\.0)?ms \(4 (?:merges|blocks|calls)\)$", phase_log, re.M)
            if expected not in phase_log and not old:
                failures.append(f"The retained phase summary omits {name} or its correct unit.")
    for name in COUNTERS:
        if (f'{name}{{source="fixture"}}', expected_counter) not in raw:
            failures.append(f"The retained raw artifact omits {name}.")
        if invalid_reason:
            expected = f"{name}: interval=unavailable reason={invalid_reason}"
            if expected not in phase_log or f"{name}: delta=" in phase_log:
                failures.append(f"The phase summary does not reject {invalid_reason} for {name}.")
        else:
            expected_delta = 0 if case == "zero-deltas" else 8
            if f"{name}: delta={expected_delta} events" not in phase_log:
                failures.append(f"The retained phase summary omits {name}.")
    (evidence / "verdict.json").write_text(json.dumps({"required_families": 27, "raw_rows": len(rows), "failures": failures}, indent=2) + "\n")
    if failures:
        print("\n".join("FAIL: " + failure for failure in failures))
        return 1
    if case == "missing-baseline":
        print("PASS: The public driver retains unavailable intervals after a failed baseline scrape without assuming zero.")
    elif case in ("decreasing-sums", "decreasing-counts"):
        print("PASS: The public driver rejects decreasing cumulative values without publishing numeric intervals.")
    elif case == "zero-deltas":
        print("PASS: The public driver retains genuine zero counts without inventing histogram means.")
    elif case in ("nonfinite-before", "nonfinite-after", "negative-before", "negative-after"):
        print("PASS: The public driver rejects invalid cumulative samples with their specific reason.")
    else:
        print("PASS: The public driver retains all 27 metric families in raw telemetry and phase summaries with their declared units.")
    return 0


if __name__ == "__main__":
    try:
        if len(sys.argv) == 4 and sys.argv[1] == "workload":
            workload(Path(sys.argv[2]).resolve(), sys.argv[3])
            status = 0
        elif len(sys.argv) in (4, 5):
            case = sys.argv[4] if len(sys.argv) == 5 else "summary"
            if case not in ("summary", "missing-baseline", "decreasing-sums", "decreasing-counts", "zero-deltas",
                            "nonfinite-before", "nonfinite-after", "negative-before", "negative-after"):
                raise ValueError("The telemetry fixture case is invalid.")
            source, harness, evidence = [Path(value).resolve() for value in sys.argv[1:4]]
            status = run(source, harness, evidence, case)
        else:
            raise ValueError("Expected SOURCE PINNED_HARNESS NEW_EVIDENCE_DIRECTORY [CASE].")
    except Exception as error:
        print(f"SETUP ERROR: {type(error).__name__}: {error}", file=sys.stderr)
        status = 2
    raise SystemExit(status)
