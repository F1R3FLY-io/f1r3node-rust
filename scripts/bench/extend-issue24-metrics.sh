#!/usr/bin/env bash
set -euo pipefail

metrics_file="${1:?metrics.py path is required}"
test -f "$metrics_file"

histograms='dag_merge_relation_items|dag_merge_relation_branches|dag_merge_conflict_edges|dag_merge_rejection_options|dag_merge_state_application_actions|dag_merge_rejection_selection_time|dag_merge_compute_trie_actions_time|dag_merge_apply_trie_actions_time|block_replay_phase_user_deploys_work|block_replay_phase_system_deploys_work|runtime_spawn_replay_time|block_replay_phase_reset_time|block_replay_phase_user_deploys_time|block_replay_phase_system_deploys_time|block_replay_phase_create_checkpoint_time'
counters='block_validation_repeat_deploy_carrier_watermark_engaged|block_validation_repeat_deploy_carrier_watermark_not_ready|block_validation_repeat_deploy_carrier_index_absence|block_validation_repeat_deploy_carrier_index_hit|block_validation_repeat_deploy_carrier_index_read_failure|block_validation_repeat_deploy_carrier_fallback_scan|block_validation_repeat_deploy_carrier_row_reads|block_validation_repeat_deploy_ancestor_metadata_visits|block_validation_repeat_deploy_ancestor_body_reads|runtime_spawn_replay_calls|block_replay_phase_reset_calls|block_replay_phase_create_checkpoint_calls'
tmp="$(mktemp "${metrics_file}.XXXXXX")"
helper_tmp=""
trap 'rm -f "$tmp" "$helper_tmp"' EXIT

awk -v histograms="$histograms" -v counters="$counters" '
BEGIN {
    histogram_count = split(histograms, histogram, "|")
    counter_count = split(counters, counter, "|")
}
/^METRICS_TO_SCRAPE = \[$/ {
    mode = "histogram"
    found_histograms = 1
}
/^COUNTERS_TO_SCRAPE = \[$/ {
    mode = "counter"
    found_counters = 1
}
{
    if (mode == "histogram") {
        for (i = 1; i <= histogram_count; i++) {
            if (index($0, "\"" histogram[i] "\"") != 0) {
                seen_histogram[histogram[i]] = 1
            }
        }
    } else if (mode == "counter") {
        for (i = 1; i <= counter_count; i++) {
            if (index($0, "\"" counter[i] "\"") != 0) {
                seen_counter[counter[i]] = 1
            }
        }
    }
    if ($0 == "]" && mode == "histogram") {
        for (i = 1; i <= histogram_count; i++) {
            if (!seen_histogram[histogram[i]]) {
                print "    \"" histogram[i] "\","
            }
        }
        mode = ""
        closed_histograms = 1
    } else if ($0 == "]" && mode == "counter") {
        for (i = 1; i <= counter_count; i++) {
            if (!seen_counter[counter[i]]) {
                print "    \"" counter[i] "\","
            }
        }
        mode = ""
        closed_counters = 1
    }
    print
}
END {
    if (!found_histograms || !closed_histograms || !found_counters || !closed_counters) {
        exit 2
    }
}
' "$metrics_file" >"$tmp"

python3 -I - "$tmp" <<'PY'
import ast
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
tree = ast.parse(text)
functions = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "format_node_metrics"]
if len(functions) != 1:
    raise SystemExit("The metrics module must define one node formatter.")
formatter = functions[0]
last = formatter.body[-1]
original = ast.parse('return "\\n".join(lines)').body[0]
corrected = ast.parse('return format_soak_node_metrics(metrics, "\\n".join(lines))').body[0]
imports = [node for node in tree.body if isinstance(node, ast.ImportFrom) and node.level == 1 and node.module == "soak_telemetry" and any(alias.name == "format_soak_node_metrics" for alias in node.names)]
if ast.dump(last) == ast.dump(original) and not imports:
    lines = text.splitlines(keepends=True)
    lines[last.lineno - 1:last.end_lineno] = ['    return format_soak_node_metrics(metrics, "\\n".join(lines))\n']
    lines.insert(formatter.lineno - 1, 'from .soak_telemetry import format_soak_node_metrics\n\n\n')
    text = "".join(lines)
    compile(text, str(path), "exec")
    path.write_text(text)
elif ast.dump(last) != ast.dump(corrected) or len(imports) != 1 or [(alias.name, alias.asname) for alias in imports[0].names] != [("format_soak_node_metrics", None)]:
    raise SystemExit("The node formatter does not match the supported overlay contract.")
PY

python3 -I - "$tmp" <<'PY'
import ast
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
tree = ast.parse(text)
functions = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "compute_metric_deltas"]
if len(functions) != 1:
    raise SystemExit("The metrics module must define one interval function.")
compute = functions[0]
expected = ast.parse('checked_metric_deltas(METRICS_TO_SCRAPE, COUNTERS_TO_SCRAPE)').body[0].value
changes = []
if not compute.decorator_list:
    changes.append((compute.lineno - 1, compute.lineno - 1,
                    'from .soak_telemetry import checked_metric_deltas\n\n\n'
                    '@checked_metric_deltas(METRICS_TO_SCRAPE, COUNTERS_TO_SCRAPE)\n'))
elif len(compute.decorator_list) != 1 or ast.dump(compute.decorator_list[0]) != ast.dump(expected):
    raise SystemExit("The interval function has an unsupported decorator.")
else:
    imports = [node for node in tree.body if isinstance(node, ast.ImportFrom) and node.level == 1 and node.module == "soak_telemetry" and [(alias.name, alias.asname) for alias in node.names] == [("checked_metric_deltas", None)]]
    if len(imports) != 1:
        raise SystemExit("The interval decorator import is invalid.")
formatter = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "format_node_metrics")
for node in ast.walk(formatter):
    if isinstance(node, ast.Return) and isinstance(node.value, ast.Constant) and isinstance(node.value.value, str):
        changes.append((node.lineno - 1, node.end_lineno,
                        ' ' * node.col_offset + f'return format_soak_node_metrics(metrics, {ast.unparse(node.value)})\n'))
lines = text.splitlines(keepends=True)
for start, end, replacement in sorted(changes, reverse=True):
    lines[start:end] = [replacement]
text = ''.join(lines)
compile(text, str(path), 'exec')
path.write_text(text)
PY

helper_source="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/soak_telemetry.py"
helper_tmp="$(mktemp "$(dirname "$metrics_file")/soak_telemetry.py.XXXXXX")"
cp "$helper_source" "$helper_tmp"
mv "$helper_tmp" "$(dirname "$metrics_file")/soak_telemetry.py"
mv "$tmp" "$metrics_file"
trap - EXIT
