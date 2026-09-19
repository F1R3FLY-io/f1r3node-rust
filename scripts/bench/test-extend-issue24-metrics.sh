#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/metrics.py" <<'PY'
METRICS_TO_SCRAPE = [
    "dag_merge_apply_trie_actions_time",
]

COUNTERS_TO_SCRAPE = [
    "is_mergeable_channel_calls",
]


def compute_metric_deltas(before, after):
    result = {"existing_metric": 1}
    return result


def format_node_metrics(metrics):
    if not metrics:
        return "  (no node metrics available)"
    lines = ["existing report"]
    return "\n".join(lines)
PY

bash "$SCRIPT_DIR/extend-issue24-metrics.sh" "$tmp/metrics.py"
cp "$tmp/metrics.py" "$tmp/once.py"
bash "$SCRIPT_DIR/extend-issue24-metrics.sh" "$tmp/metrics.py"
cmp "$tmp/once.py" "$tmp/metrics.py"

for metric in \
    dag_merge_relation_items \
    dag_merge_rejection_selection_time \
    dag_merge_apply_trie_actions_time \
    block_replay_phase_user_deploys_work \
    runtime_spawn_replay_time \
    block_validation_repeat_deploy_carrier_watermark_engaged \
    block_validation_repeat_deploy_ancestor_body_reads \
    runtime_spawn_replay_calls \
    block_replay_phase_create_checkpoint_calls \
    block_arrival_depth \
    block_arrived_uncitable \
    history_checkpoint_time \
    history_checkpoint_storage_actions_time \
    history_checkpoint_partition_time \
    history_checkpoint_serialize_time \
    history_checkpoint_leaf_write_time \
    history_checkpoint_history_lock_wait_time \
    history_checkpoint_history_process_time \
    history_checkpoint_roots_lock_wait_time \
    history_checkpoint_root_commit_time \
    history_checkpoint_actions \
    history_checkpoint_serialized_bytes \
    history_repository_current_history_lock_wait_ns \
    history_repository_roots_repository_lock_wait_ns \
    block_replay_runtime_lock_wait_time \
    block_replay_runtime_execute_time \
    block_replay_runtime_save_mergeable_time \
    block_validation_repeat_deploy_parents_time \
    block_validation_repeat_deploy_rejected_sigs_time \
    block_validation_repeat_deploy_retry_gate_time \
    block_validation_repeat_deploy_carrier_watermark_time \
    block_validation_repeat_deploy_carrier_probes_time \
    block_validation_repeat_deploy_ancestor_scan_time; do
    test "$(grep -Fxc "    \"$metric\"," "$tmp/metrics.py")" -eq 1
done

python3 - "$tmp/metrics.py" "$SCRIPT_DIR/../.." <<'PY'
import ast
import json
from pathlib import Path
import re
import runpy
import sys

registries = {
    node.targets[0].id: ast.literal_eval(node.value)
    for node in ast.parse(Path(sys.argv[1]).read_text()).body
    if isinstance(node, ast.Assign) and isinstance(node.targets[0], ast.Name)
    and node.targets[0].id in {"METRICS_TO_SCRAPE", "COUNTERS_TO_SCRAPE"}
}
histograms = registries["METRICS_TO_SCRAPE"]
counters = registries["COUNTERS_TO_SCRAPE"]
assert len(histograms) == len(set(histograms))
assert len(counters) == len(set(counters))
assert not set(histograms) & set(counters)
root = Path(sys.argv[2])
for filename in ["casper/src/rust/metrics_constants.rs", "rspace++/src/rspace/metrics_constants.rs"]:
    for constant, name in re.findall(r'pub const (\w+): &str =\s*"([^"]+)";', (root / filename).read_text()):
        exported = re.sub(r"[.-]", "_", name)
        if (constant.startswith(("HISTORY_CHECKPOINT_", "BLOCK_REPLAY_RUNTIME_")) and constant.endswith("_METRIC")) or (constant.startswith("REPEAT_DEPLOY_") and constant.endswith("_TIME_METRIC")) or constant == "BLOCK_ARRIVAL_DEPTH_METRIC":
            assert exported in histograms, constant
        if constant.startswith("HISTORY_REPO_") or constant == "BLOCK_ARRIVED_UNCITABLE_METRIC":
            assert exported in counters, constant
assert "is_mergeable_channel_calls" in counters
module = runpy.run_path(sys.argv[1])
compute = module["compute_metric_deltas"]
report = module["format_node_metrics"]


def payload(metrics):
    return json.loads(report(metrics).split("ISSUE24_METRICS ", 1)[1])


name = "history_checkpoint_serialize_time"
counter = "block_arrived_uncitable"
missing = payload({})
assert missing["histograms"][name]["mean"] is None
assert missing["histograms"][name]["samples"] is None
assert missing["counters"][counter]["delta"] is None
before = {name + "_sum": 2.0, name + "_count": 4.0, counter: 2.0}
after = {name + "_sum": 3.0, name + "_count": 6.0, counter: 2.0}
deltas = compute(before, after)
assert deltas["existing_metric"] == 1
assert deltas[name] == 0.5
assert deltas[name + ".count"] == 2
observed = payload(deltas)
assert observed["histograms"][name] == {"mean": 0.5, "samples": 2, "unit": "seconds"}
assert observed["counters"][counter]["delta"] == 0
assert report(deltas).startswith("existing report\n")
zero = payload(compute(before, before))
assert zero["histograms"][name]["samples"] == 0
assert zero["histograms"][name]["mean"] is None
assert zero["counters"][counter]["delta"] == 0
for start, end in [({}, after), (before, {}), (after, {**before, counter: 1.0}), (before, {**after, name + "_sum": float("inf"), counter: float("nan")})]:
    unknown = payload(compute(start, end))
    assert unknown["histograms"][name]["samples"] is None
    assert unknown["histograms"][name]["mean"] is None
    assert unknown["counters"][counter]["delta"] is None
PY

cp "$tmp/metrics.py" "$tmp/malformed.py"
sed -i '/COUNTERS_TO_SCRAPE = \[/d' "$tmp/malformed.py"
cp "$tmp/malformed.py" "$tmp/malformed-before.py"
if bash "$SCRIPT_DIR/extend-issue24-metrics.sh" "$tmp/malformed.py"; then
    exit 1
fi
cmp "$tmp/malformed-before.py" "$tmp/malformed.py"

cp "$tmp/once.py" "$tmp/missing-report.py"
sed -i 's/def format_node_metrics(/def renamed_format_node_metrics(/' "$tmp/missing-report.py"
cp "$tmp/missing-report.py" "$tmp/missing-report-before.py"
if bash "$SCRIPT_DIR/extend-issue24-metrics.sh" "$tmp/missing-report.py"; then
    exit 1
fi
cmp "$tmp/missing-report-before.py" "$tmp/missing-report.py"
