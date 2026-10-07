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
PY

bash "$SCRIPT_DIR/extend-issue24-metrics.sh" "$tmp/metrics.py"
bash "$SCRIPT_DIR/extend-issue24-metrics.sh" "$tmp/metrics.py"

for metric in \
    dag_merge_relation_items \
    dag_merge_rejection_selection_time \
    dag_merge_apply_trie_actions_time \
    block_replay_phase_user_deploys_work \
    runtime_spawn_replay_time \
    block_validation_repeat_deploy_carrier_watermark_engaged \
    block_validation_repeat_deploy_ancestor_body_reads \
    runtime_spawn_replay_calls \
    block_replay_phase_create_checkpoint_calls; do
    test "$(grep -Fc "\"$metric\"" "$tmp/metrics.py")" -eq 1
done

cp "$tmp/metrics.py" "$tmp/malformed.py"
sed -i '/COUNTERS_TO_SCRAPE = \[/d' "$tmp/malformed.py"
if bash "$SCRIPT_DIR/extend-issue24-metrics.sh" "$tmp/malformed.py"; then
    exit 1
fi
