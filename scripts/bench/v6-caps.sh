#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <v6-throughput summary file> <time budget in milliseconds>" >&2
    exit 2
fi

summary="$1"
budget_ms="$2"

if ! [[ "$budget_ms" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
    echo "the time budget must be a positive number of milliseconds" >&2
    exit 2
fi

cap() {
    local key="$1"
    awk -v key="$key" -v budget="$budget_ms" '
        $1 == "CAP" {
            for (i = 2; i <= NF; i++) {
                split($i, pair, "=")
                if (pair[1] == key && pair[2] + 0 > worst) worst = pair[2] + 0
            }
        }
        END {
            if (worst <= 0) { print "missing"; exit }
            limit = budget * 1000000 / worst
            value = 1
            while (value * 2 <= limit) value *= 2
            if (value > limit) { print "below-one"; exit }
            printf "%d %.2f\n", value, worst
        }
    ' "$summary"
}

report() {
    local name="$1" key="$2" unit="$3"
    local result
    result="$(cap "$key")"
    case "$result" in
        missing)
            echo "$summary has no $key measurements" >&2
            exit 1
            ;;
        below-one)
            echo "a $budget_ms ms budget is below one $unit at the measured cost" >&2
            exit 1
            ;;
    esac
    local value worst
    read -r value worst <<<"$result"
    echo "$name = $value"
    echo "    worst cost ${worst} ns per $unit, worst search $(awk -v v="$value" -v w="$worst" 'BEGIN { printf "%.2f", v * w / 1000000 }') ms" >&2
}

echo "protocol-caps {"
report "  funding-search-scans" nanoseconds_per_scan scan
report "  authority-search-nodes" nanoseconds_per_node node
echo "}"
