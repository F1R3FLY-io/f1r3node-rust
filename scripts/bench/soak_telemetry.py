from collections.abc import Mapping
from functools import wraps
from math import isfinite


HISTOGRAM_UNITS = {
    "dag_merge_relation_items": "items",
    "dag_merge_relation_branches": "branches",
    "dag_merge_conflict_edges": "adjacency entries",
    "dag_merge_rejection_options": "options",
    "dag_merge_state_application_actions": "prepared actions",
    "dag_merge_rejection_selection_time": "seconds",
    "dag_merge_compute_trie_actions_time": "seconds",
    "dag_merge_apply_trie_actions_time": "seconds",
    "block_replay_phase_user_deploys_work": "input deploys",
    "block_replay_phase_system_deploys_work": "input deploys",
    "runtime_spawn_replay_time": "seconds",
    "block_replay_phase_reset_time": "seconds",
    "block_replay_phase_user_deploys_time": "seconds",
    "block_replay_phase_system_deploys_time": "seconds",
    "block_replay_phase_create_checkpoint_time": "seconds",
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


class MetricDeltas(dict):
    def __init__(self, values, invalid):
        super().__init__(values)
        self.invalid = invalid


def checked_metric_deltas(histograms, counters):
    def decorate(compute):
        @wraps(compute)
        def checked(before, after):
            invalid = {}
            filtered = dict(after)
            for name in histograms:
                keys = (name + "_sum", name + "_count")
                if all(key in after for key in keys):
                    if not all(key in before for key in keys):
                        invalid[name] = "missing_baseline"
                    elif any(not isfinite(snapshot[key]) for snapshot in (before, after) for key in keys):
                        invalid[name] = "nonfinite_sample"
                    elif name in HISTOGRAM_UNITS and any(snapshot[key] < 0 for snapshot in (before, after) for key in keys):
                        invalid[name] = "negative_sample"
                    elif any(after[key] < before[key] for key in keys):
                        invalid[name] = "cumulative_decrease"
                if name in invalid:
                    for key in keys:
                        filtered.pop(key, None)
            for name in counters:
                if name in after:
                    if name not in before:
                        invalid[name] = "missing_baseline"
                    elif not all(isfinite(snapshot[name]) for snapshot in (before, after)):
                        invalid[name] = "nonfinite_sample"
                    elif before[name] < 0 or after[name] < 0:
                        invalid[name] = "negative_sample"
                    elif after[name] < before[name]:
                        invalid[name] = "cumulative_decrease"
                if name in invalid:
                    filtered.pop(name, None)
            return MetricDeltas(compute(before, filtered), invalid)
        return checked
    return decorate


def format_soak_node_metrics(metrics: Mapping[str, float], original: str) -> str:
    lines = []
    invalid = getattr(metrics, "invalid", {})
    for name, unit in HISTOGRAM_UNITS.items():
        if name in invalid:
            lines.append(f"    {name}: interval=unavailable reason={invalid[name]}")
            continue
        count = metrics.get(name + ".count")
        if count is None:
            continue
        if name in metrics:
            mean = f"{metrics[name]:.6g} {unit}"
        else:
            mean = "unavailable"
        lines.append(f"    {name}: mean={mean}, observations={count:.6g}")
    for name in COUNTERS:
        if name in invalid:
            lines.append(f"    {name}: interval=unavailable reason={invalid[name]}")
            continue
        count = metrics.get(name + ".count")
        if count is not None:
            lines.append(f"    {name}: delta={count:.6g} events")
    if not lines:
        return original
    return "\n".join([original, "  Soak metric intervals:", *lines])
