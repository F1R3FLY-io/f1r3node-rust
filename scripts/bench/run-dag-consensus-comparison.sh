#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
reference=${1:?usage: run-dag-consensus-comparison.sh REFERENCE_ARCHIVE OUTPUT_DIRECTORY}
output=${2:?usage: run-dag-consensus-comparison.sh REFERENCE_ARCHIVE OUTPUT_DIRECTORY}
if [[ -e $output ]]; then
    printf '%s\n' 'OUTPUT_DIRECTORY must be a new path so previous runs cannot enter the comparison.' >&2
    exit 2
fi
mkdir -p "$output"
output=$(cd "$output" && pwd)
reference=$(cd "$reference" && pwd)
mode=${DAG_CONSENSUS_MODE:-performance}
if [[ $mode == performance ]]; then
    runs=${DAG_CONSENSUS_RUNS:-3}
    samples=${DAG_CONSENSUS_SAMPLES:-100}
    warmup=${DAG_CONSENSUS_WARMUP:-1088}
    depths=${DAG_CONSENSUS_DEPTHS:-15}
    windows=${DAG_CONSENSUS_WINDOWS:-1024}
elif [[ $mode == correctness ]]; then
    runs=${DAG_CONSENSUS_RUNS:-1}
    samples=${DAG_CONSENSUS_SAMPLES:-4}
    warmup=${DAG_CONSENSUS_WARMUP:-24}
    depths=${DAG_CONSENSUS_DEPTHS:-"15 64 2147483647"}
    windows=${DAG_CONSENSUS_WINDOWS:-"1 2 16 1024"}
else
    printf '%s\n' 'DAG_CONSENSUS_MODE must be performance or correctness.' >&2
    exit 2
fi
if (( runs < 1 || samples < 1 || warmup < 1 )) || { [[ $mode == performance ]] && (( runs < 3 || samples < 100 )); }; then
    printf '%s\n' 'Counts must be positive; performance needs at least 3 processes and 100 measured proposals.' >&2
    exit 2
fi

build() {
    local source=$1 variant=$2
    local target="$source/target"
    if [[ $variant == reference && -f $source/../reference-revision.txt ]]; then
        target="$source/../build"
        cp "$source/../reference-revision.txt" "$output/reference-revision.txt"
    fi
    (
        cd "$source"
        CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2} cargo test --release --locked -p casper --test dag_consensus_bench --target-dir "$target" --no-run --message-format=json > "$output/$variant-build.jsonl" 2> "$output/$variant-build.log"
        sha256sum casper/tests/dag_consensus_bench.rs casper/tests/dag_consensus_bench/backend.rs casper/src/rust/blocks/proposer/block_creator.rs casper/tests/helper/test_node.rs casper/tests/util/rholang/resources.rs > "$output/$variant-source-sha256.txt"
    )
    jq -r 'select(.reason == "compiler-artifact" and .target.name == "dag_consensus_bench" and .executable != null) | .executable' "$output/$variant-build.jsonl" | tail -n1
}

current_binary=$(build "$repo" windowed)
reference_binary=$(build "$reference" reference)
test -x "$current_binary"
test -x "$reference_binary"
sha256sum "$current_binary" "$reference_binary" > "$output/binary-sha256.txt"

run_one() {
    local binary=$1 variant=$2 depth=$3 window=$4 iteration=$5
    local prefix="$output/$variant-depth$depth-window$window-run$iteration"
    RUST_LOG=error DAG_CONSENSUS_MODE="$mode" DAG_CONSENSUS_VARIANT="$variant" DAG_CONSENSUS_DEPTH="$depth" DAG_CONSENSUS_WINDOW="$window" DAG_CONSENSUS_WARMUP="$warmup" DAG_CONSENSUS_SAMPLES="$samples" DAG_CONSENSUS_OUTPUT="$prefix.json" "$binary" --ignored --exact deterministic_consensus_latency_matrix --nocapture --test-threads=1 > "$prefix.log" 2>&1
}

for depth in $depths; do
    for ((iteration=1; iteration<=runs; iteration++)); do
        run_one "$reference_binary" reference "$depth" 1024 "$iteration"
        for window in $windows; do
            run_one "$current_binary" windowed "$depth" "$window" "$iteration"
        done
    done
done

jq -s --arg mode "$mode" '
    def median: sort | .[(length / 2 | floor)];
    map(select(has("results"))) as $artifacts |
    ($mode == "correctness" or ($artifacts | all(.latency_release_process_shape_met))) as $process_shape |
    [$artifacts[] | .variant as $variant | .results[] + {variant:$variant}] as $rows |
    [$rows[] | select(.variant == "windowed")] | group_by([.parent_depth,.window]) |
    map(. as $candidate |
        [$rows[] | select(.variant == "reference" and .parent_depth == $candidate[0].parent_depth)] as $baseline |
        ($baseline | map(.proposal.p95_ns) | median) as $proposal_baseline |
        ($baseline | map(.peer_validation.p95_ns) | median) as $validation_baseline |
        ($candidate | map(.proposal.p95_ns) | median) as $proposal_current |
        ($candidate | map(.peer_validation.p95_ns) | median) as $validation_current |
        ($baseline | map(.proposal.p50_ns) | median) as $proposal_p50_baseline |
        ($baseline | map(.peer_validation.p50_ns) | median) as $validation_p50_baseline |
        ($candidate | map(.proposal.p50_ns) | median) as $proposal_p50_current |
        ($candidate | map(.peer_validation.p50_ns) | median) as $validation_p50_current |
        {
            parent_depth:$candidate[0].parent_depth, window:$candidate[0].window,
            reference_runs:($baseline|length), windowed_runs:($candidate|length),
            proposal_p95_ratio:($proposal_current/$proposal_baseline),
            peer_validation_p95_ratio:($validation_current/$validation_baseline),
            proposal_p50_ratio:($proposal_p50_current/$proposal_p50_baseline),
            peer_validation_p50_ratio:($validation_p50_current/$validation_p50_baseline),
            identical_consensus:((($baseline+$candidate)|map(.trace_digest)|unique|length)==1),
            cold_window_exercised_in_every_sample:($candidate|all(.cold_window_exercised_in_every_sample)),
            resident_blocks:($candidate|map(.resident_blocks)),
            cold_blocks:($candidate|map(.cold_blocks)),
            proposal_gate:($proposal_current <= $proposal_baseline*1.1 and $proposal_p50_current <= $proposal_p50_baseline*1.1),
            peer_validation_gate:($validation_current <= $validation_baseline*1.1 and $validation_p50_current <= $validation_p50_baseline*1.1)
        }
    ) |
    {mode:$mode, process_shape_pass:$process_shape, comparisons:., accepted:($process_shape and all(.identical_consensus and ($mode == "correctness" or (.proposal_gate and .peer_validation_gate and .cold_window_exercised_in_every_sample))))}
' "$output"/reference-depth*-run*.json "$output"/windowed-depth*-run*.json > "$output/comparison.json"
jq . "$output/comparison.json"
jq -e '.accepted' "$output/comparison.json" > /dev/null
