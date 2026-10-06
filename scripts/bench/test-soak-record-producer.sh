#!/usr/bin/env bash
set -euo pipefail

SOURCE="$(cd "${1:?source directory is required}" && pwd)"
EVIDENCE="${2:?new evidence directory is required}"
[[ "$(id -u)" != 0 && ! -e "$EVIDENCE" ]] || exit 2
mkdir -p "$EVIDENCE"
EVIDENCE="$(cd "$EVIDENCE" && pwd)"
trap 'printf "SETUP ERROR: The record-producer fixture did not reach its behavioral verdict.\n" >&2; exit 2' ERR
(cd "$SOURCE" && sha256sum scripts/run-merge-recovery-soak.sh scripts/bench/write-soak-summary.sh scripts/bench/collect-soak-metrics.sh scripts/bench/soak-metrics.json) > "$EVIDENCE/source-sha256.txt"
sha256sum "${BASH_SOURCE[0]}" > "$EVIDENCE/fixture-sha256.txt"
failed=0
for scenario in valid producer-error; do
    case_dir="$EVIDENCE/$scenario"
    mkdir -p "$case_dir/bin" "$case_dir/output" "$case_dir/harness" "$case_dir/home" "$case_dir/tmp" "$case_dir/runner"
    for command in docker poetry curl oci git; do
        printf '#!/usr/bin/env bash\nprintf "%%s\\n" "${0##*/}" >> "$EXTERNAL_CALLS_FILE"\nexit 125\n' > "$case_dir/bin/$command"
        chmod +x "$case_dir/bin/$command"
    done
    failures=0
    [[ "$scenario" == valid ]] || failures=invalid
    printf 'STARTED_AT=1\nITERATIONS=0\nINFLIGHT_ITERATION=0\nINFLIGHT_BENCHMARK=0\nFAILURES=%s\nBENCH_SEGMENTS=0\nBENCH_FAILURES=0\nSEGMENT=1\n' "$failures" > "$case_dir/output/.soak-state"
    printf '{"previous":"valid"}\n' > "$case_dir/output/.soak-checkpoint-state.json"
    cp "$case_dir/output/.soak-checkpoint-state.json" "$case_dir/checkpoint-before.json"
    status=0
    timeout --signal=TERM --kill-after=2 15 env -i \
        PATH="$case_dir/bin:/usr/bin:/bin" HOME="$case_dir/home" LANG=C \
        EXTERNAL_CALLS_FILE="$case_dir/external-calls.txt" \
        SYSTEM_INTEGRATION_DIR="$case_dir/harness" SOAK_OUTPUT_DIR="$case_dir/output" \
        SOAK_TMP_ROOT="$case_dir/tmp" SOAK_RUNNER_ROOT="$case_dir/runner" \
        SOAK_DURATION_SECONDS=1 SOAK_RUN_BENCHMARKS=false SOAK_CONTAINMENT=unmanaged \
        SOAK_RSS_CEILING_MB=0 SOAK_HOST_FREE_FLOOR_MB=0 SOAK_DISK_FREE_FLOOR_MB=0 \
        bash "$SOURCE/scripts/run-merge-recovery-soak.sh" > "$case_dir/driver.txt" 2>&1 || status=$?
    printf '%s\n' "$status" > "$case_dir/driver-exit.txt"
    [[ ! -e "$case_dir/external-calls.txt" ]]
    [[ -z "$(find "$case_dir/output" -maxdepth 1 -name 'iteration-*' -print -quit)" ]]
    if [[ "$scenario" == valid ]]; then
        [[ "$status" == 0 ]]
        jq -e '.iterations == 0 and .failures == 0 and .started_at == 1' "$case_dir/output/.soak-checkpoint-state.json" > /dev/null
        printf 'PASS: Valid record generation publishes a valid checkpoint.\n'
    else
        [[ "$status" == 2 ]]
        grep -Fq 'jq: invalid JSON text passed to --argjson' "$case_dir/driver.txt"
        trap - ERR
        if cmp -s "$case_dir/checkpoint-before.json" "$case_dir/output/.soak-checkpoint-state.json"; then
            printf 'PASS: Failed record generation preserves the previous checkpoint and admits no work.\n'
        else
            printf 'FAIL: Failed record generation replaced the previous valid checkpoint.\n' >&2
            failed=1
        fi
        stat -c '%s' "$case_dir/output/.soak-checkpoint-state.json" > "$case_dir/checkpoint-after-bytes.txt"
    fi
done
exit "$failed"
