#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="${1:-$root/target/v6-throughput-$(date +%Y%m%d-%H%M%S).txt}"
mkdir -p "$(dirname "$out")"

run() {
    local title="$1"
    shift
    echo "== $title" | tee -a "$out"
    (cd "$root" && cargo test --release "$@" -- --ignored --nocapture --test-threads 1 2>&1) |
        grep -oE '(envelopes=|CAP ).*' | tee -a "$out"
}

{
    echo "host: $(uname -srm)"
    if command -v sysctl >/dev/null 2>&1 && sysctl -n machdep.cpu.brand_string >/dev/null 2>&1; then
        echo "cpu: $(sysctl -n machdep.cpu.brand_string), $(sysctl -n hw.ncpu) cores"
    elif [ -r /proc/cpuinfo ]; then
        echo "cpu: $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ //'), $(nproc) cores"
    fi
    echo "commit: $(git -C "$root" rev-parse --short HEAD)"
    echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} | tee "$out"

run "executor phase timings" -p casper --test v6_executor_spec phase_timings_by_block_size
run "funding search scans" -p rholang --lib funding_search_scans_and_time_by_size
run "authority search nodes" -p rholang --lib authority_search_nodes_and_time_by_size

echo "summary: $out"
