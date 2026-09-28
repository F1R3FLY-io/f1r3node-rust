#!/usr/bin/env bash
set -euo pipefail

metrics_file="${1:?metrics.py path is required}"
test -f "$metrics_file"

histograms='dag_merge_relation_items|dag_merge_relation_branches|dag_merge_conflict_edges|dag_merge_rejection_options|dag_merge_state_application_actions|dag_merge_rejection_selection_time|dag_merge_compute_trie_actions_time|dag_merge_apply_trie_actions_time|block_replay_phase_user_deploys_work|block_replay_phase_system_deploys_work|runtime_spawn_replay_time|block_replay_phase_reset_time|block_replay_phase_user_deploys_time|block_replay_phase_system_deploys_time|block_replay_phase_create_checkpoint_time'
counters='block_validation_repeat_deploy_carrier_watermark_engaged|block_validation_repeat_deploy_carrier_watermark_not_ready|block_validation_repeat_deploy_carrier_index_absence|block_validation_repeat_deploy_carrier_index_hit|block_validation_repeat_deploy_carrier_index_read_failure|block_validation_repeat_deploy_carrier_fallback_scan|block_validation_repeat_deploy_carrier_row_reads|block_validation_repeat_deploy_ancestor_metadata_visits|block_validation_repeat_deploy_ancestor_body_reads|runtime_spawn_replay_calls|block_replay_phase_reset_calls|block_replay_phase_create_checkpoint_calls'
tmp="$(mktemp "${metrics_file}.XXXXXX")"
trap 'rm -f "$tmp"' EXIT

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
' "$metrics_file" > "$tmp"

mv "$tmp" "$metrics_file"
trap - EXIT
