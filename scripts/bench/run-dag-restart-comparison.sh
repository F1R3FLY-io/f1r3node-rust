#!/usr/bin/env bash
set -euo pipefail

reference=""
candidate=""
comparison=""
out=""
apply=0
counts="${DAG_MEMORY_COUNTS:-120000 240000 480000}"
while (($#)); do
    case "$1" in
        --reference-bin) reference="$2"; shift 2 ;;
        --candidate-bin) candidate="$2"; shift 2 ;;
        --comparison-dir) comparison="$2"; shift 2 ;;
        --out-dir) out="$2"; shift 2 ;;
        --apply) apply=1; shift ;;
        --help|-h)
            printf '%s\n' 'Usage: run-dag-restart-comparison.sh --reference-bin PATH --candidate-bin PATH --comparison-dir COMPLETED_COMPARISON --out-dir NEW_DIRECTORY [--apply]' 'Runs three fresh processes per size for reference restart, candidate restart, and candidate migration from actual reference checkpoints. Default sizes are 120000/240000/480000; DAG_MEMORY_COUNTS overrides them for smoke only. Sources are never opened by either binary. One disposable reflink/copy is used at a time; successful scratch copies are removed, failed copies are retained. Raw JSONL, stderr, hashes, timings and disk sizes are preserved. Default plan only. OS page cache is uncontrolled; completion does not certify acceptance.'
            exit 0 ;;
        *) printf 'Unknown argument: %s\n' "$1" >&2; exit 2 ;;
    esac
done
[[ -n "$reference" && -n "$candidate" && -n "$comparison" && -n "$out" ]] || { printf 'Both binaries, comparison directory and new output directory are required.\n' >&2; exit 2; }
read -r -a sizes <<<"$counts"
((${#sizes[@]} > 0))
previous=0
for count in "${sizes[@]}"; do
    [[ "$count" =~ ^[1-9][0-9]*$ && "$count" -ge 2 && "$count" -gt "$previous" ]]
    previous="$count"
done
printf 'Plan: sizes [%s], three reference restarts, three candidate restarts and three candidate migrations per size; sources %s; output %s\n' "$counts" "$comparison" "$out"
((apply)) || exit 0
[[ -x "$reference" && -x "$candidate" && -d "$comparison" && ! -e "$out" ]]
reference="$(realpath "$reference")"
candidate="$(realpath "$candidate")"
comparison="$(realpath "$comparison")"
mode="$(cat "$comparison/workload-mode.txt")"
[[ "$mode" == checkpoints || "$mode" == independent ]]
mkdir -p "$out"
out="$(realpath "$out")"
sha256sum "$reference" "$candidate" >"$out/binary-sha256.txt"
cp "$comparison/reference-revision.txt" "$out/reference-revision.txt"
printf '%s\n' "$comparison" >"$out/source-comparison.txt"
printf '%s\n' "$counts" >"$out/counts.txt"
uname -a >"$out/kernel.txt"
cat /proc/cpuinfo >"$out/cpuinfo.txt"
cat /proc/meminfo >"$out/meminfo.txt"
printf 'stage,run,blocks,start,end,exit_code,copy_start,copy_end,source_apparent_bytes,source_allocated_bytes,scratch_allocated_bytes\n' >"$out/runs.csv"

run_one() {
    local stage=$1 count=$2 run=$3 source_impl binary expected label
    source_impl=candidate
    binary="$candidate"
    expected=process_restart
    local args=(--reopen-existing)
    if [[ "$stage" == reference_restart ]]; then
        source_impl=reference
        binary="$reference"
    elif [[ "$stage" == candidate_migration ]]; then
        source_impl=reference
        expected=process_legacy_migration
        args+=(--legacy-fixture)
    fi
    local generated_count=$count suffix=""
    if [[ "$mode" == checkpoints ]]; then
        generated_count="${sizes[-1]}"
        suffix=".checkpoint-$count"
    fi
    local name="$source_impl-$generated_count-$run"
    local source="$comparison/$name.data$suffix"
    [[ -f "$source/dagstorage/data.mdb" ]]
    awk -F, -v run="$run" -v count="$generated_count" -v implementation="$source_impl" 'NR>1 && $1==run && $2==count && $3==implementation && $5~/^[0-9]+$/ {found=1} END {exit !found}' "$comparison/runs.csv"
    jq -s -e --argjson count "$count" 'any(.[]; (.stage=="checkpoint" or .stage=="steady") and .blocks==$count)' "$comparison/$name.jsonl" >/dev/null
    if [[ "$source_impl" == reference ]]; then
        jq -s -e 'any(.[]; .stage=="baseline" and .windowed==false)' "$comparison/$name.jsonl" >/dev/null
    else
        jq -s -e 'any(.[]; .stage=="baseline" and .windowed==true)' "$comparison/$name.jsonl" >/dev/null
    fi
    label="$stage-$count-$run"
    local apparent allocated available
    apparent="$(du -sb "$source" | cut -f1)"
    allocated="$(du -sB1 "$source" | cut -f1)"
    available="$(df -Pk "$out" | awk 'NR==2 {print $4 * 1024}')"
    awk -v available="$available" -v apparent="$apparent" 'BEGIN {exit !(available >= apparent*3 + 536870912)}' || { printf 'Insufficient scratch space for %s\n' "$label" >&2; return 2; }
    local scratch copy_start copy_end start end status scratch_allocated
    scratch="$(mktemp -d "$out/scratch-$label-XXXXXXXX")"
    printf '%s\n' "$label" >"$scratch/.runner-owned"
    printf '%s\n' "$scratch" >"$out/$label.scratch-path.txt"
    printf '%s\n' "$source" >"$out/$label.source-path.txt"
    sha256sum "$source/dagstorage/data.mdb" >"$out/$label.source-before.sha256"
    copy_start="$(date -u +%s)"
    cp -a --reflink=auto "$source" "$scratch/fixture"
    copy_end="$(date -u +%s)"
    start="$(date -u +%s)"
    status=0
    "$binary" --blocks "$count" --window 1024 --data-dir "$scratch/fixture" "${args[@]}" >"$out/$label.jsonl" 2>"$out/$label.stderr" || status=$?
    end="$(date -u +%s)"
    scratch_allocated="$(du -sB1 "$scratch" | cut -f1)"
    printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' "$stage" "$run" "$count" "$start" "$end" "$status" "$copy_start" "$copy_end" "$apparent" "$allocated" "$scratch_allocated" >>"$out/runs.csv"
    sha256sum "$source/dagstorage/data.mdb" >"$out/$label.source-after.sha256"
    cmp "$out/$label.source-before.sha256" "$out/$label.source-after.sha256"
    if ((status)); then
        printf 'Failed %s; retained scratch %s\n' "$label" "$scratch" >&2
        return "$status"
    fi
    jq -s -e --arg stage "$expected" --argjson count "$count" 'any(.[]; .stage==$stage and .blocks==$count)' "$out/$label.jsonl" >/dev/null
    [[ "$scratch" == "$out/scratch-$label-"* && "$(cat "$scratch/.runner-owned")" == "$label" ]]
    rm -rf -- "$scratch"
}

for count in "${sizes[@]}"; do
    for run in 1 2 3; do
        if ((run % 2)); then order=(reference_restart candidate_restart); else order=(candidate_restart reference_restart); fi
        for stage in "${order[@]}"; do run_one "$stage" "$count" "$run"; done
        run_one candidate_migration "$count" "$run"
    done
done
printf 'Fresh-process measurements completed; inspect all runs and heap/latency thresholds. Page-cache state was uncontrolled.\n'
