#!/usr/bin/env bash
set -euo pipefail

reference=""
candidate=""
revision=""
out=""
counts="${DAG_MEMORY_COUNTS:-120000 240000 480000}"
apply=0
mode=checkpoints
while (($#)); do
    case "$1" in
        --reference-bin) reference="$2"; shift 2 ;;
        --candidate-bin) candidate="$2"; shift 2 ;;
        --reference-revision) revision="$2"; shift 2 ;;
        --out-dir) out="$2"; shift 2 ;;
        --apply) apply=1; shift ;;
        --independent-sizes) mode=independent; shift ;;
        --help|-h)
            printf '%s\n' 'Usage: run-dag-memory-comparison.sh --reference-bin PATH --candidate-bin PATH --reference-revision SHA --out-dir NEW_DIRECTORY [--apply] [--independent-sizes]' 'Default checkpoint mode runs three independent processes per implementation, each growing through 120k/240k/480k, closing and copying its LMDB fixture at each exact size. Latencies exclude warmup separately in each interval. --independent-sizes instead creates three fresh processes at each size. Both binaries must be built from the same harness source. Default plan only. DAG_MEMORY_COUNTS overrides sizes for smoke runs, which do not meet acceptance. Records all JSONL, binary hashes, CPU/memory information and wall-clock labels; no statistical acceptance is inferred from completion. Use a disk filesystem with at least 30 GiB free, not tmpfs.'
            printf '%s\n' 'Checkpoint copies are taken with writers quiescent; the process-global LMDB environment remains open. In-process reconstruction is not a restart measurement. Use run-dag-restart-comparison.sh afterward for fresh-process restart and legacy migration.'
            exit 0 ;;
        *) printf 'Unknown argument: %s\n' "$1" >&2; exit 2 ;;
    esac
done
[[ -n "$reference" && -n "$candidate" && -n "$out" && "$revision" =~ ^[a-f0-9]{40}$ ]] || { printf 'Both binary paths, full reference revision and output directory are required.\n' >&2; exit 2; }
read -r -a sizes <<<"$counts"
((${#sizes[@]} > 0)) || { printf 'At least one history size is required.\n' >&2; exit 2; }
previous=0
for count in "${sizes[@]}"; do
    [[ "$count" =~ ^[1-9][0-9]*$ && "$count" -ge 2 && "$count" -gt "$previous" ]]
    previous="$count"
done
printf 'Compare actual reference %s with candidate: sizes [%s], mode %s, three processes per implementation, serial execution; output %s\n' "$revision" "$counts" "$mode" "$out"
((apply)) || exit 0
[[ -x "$reference" && -x "$candidate" && ! -e "$out" ]]
ancestor="$out"
while [[ ! -e "$ancestor" ]]; do ancestor="$(dirname "$ancestor")"; done
available="$(df -Pk "$ancestor" | awk 'NR==2 {print $4}')"
((available >= 30 * 1024 * 1024)) || { printf 'Comparison needs 30 GiB available disk space.\n' >&2; exit 2; }
mkdir -p "$out"
sha256sum "$reference" "$candidate" >"$out/binary-sha256.txt"
printf '%s\n' "$revision" >"$out/reference-revision.txt"
printf '%s\n' "$mode" >"$out/workload-mode.txt"
uname -a >"$out/kernel.txt"
cat /proc/cpuinfo >"$out/cpuinfo.txt"
cat /proc/meminfo >"$out/meminfo.txt"
printf 'run,blocks,implementation,start,end\n' >"$out/runs.csv"
run_sizes=("${sizes[@]}")
extra_args=()
if [[ "$mode" == checkpoints ]]; then
    run_sizes=("${sizes[-1]}")
    checkpoint_counts="$(IFS=,; printf '%s' "${sizes[*]}")"
    extra_args=(--checkpoints "$checkpoint_counts")
fi
for count in "${run_sizes[@]}"; do
    for run in 1 2 3; do
        if ((run % 2)); then order=(reference candidate); else order=(candidate reference); fi
        for implementation in "${order[@]}"; do
            binary="$candidate"
            [[ "$implementation" == candidate ]] || binary="$reference"
            name="$implementation-$count-$run"
            start="$(date -u +%s)"
            "$binary" --blocks "$count" --window 1024 --data-dir "$out/$name.data" "${extra_args[@]}" >"$out/$name.jsonl" 2>"$out/$name.stderr"
            end="$(date -u +%s)"
            printf '%s,%s,%s,%s,%s\n' "$run" "$count" "$implementation" "$start" "$end" >>"$out/runs.csv"
        done
    done
done
printf 'All comparisons completed. Inspect all runs; completion does not certify the 10%% latency or heap thresholds.\n'
