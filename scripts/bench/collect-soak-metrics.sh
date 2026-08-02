#!/usr/bin/env bash
# Generic soak metric collector.
#
# Scans harness and node logs written since an iteration's start marker for
# structured metric lines, and emits a JSON object of per-metric summaries:
#
#   {"lfb_spread": {"p50": 1, "p95": 3, "max": 4, "min": 0, "samples": 42}}
#
# Adding a tracked metric requires no change to this script: emit the line
# from the integration harness and declare the metric in soak-metrics.json.
#
# Line format:
#   SOAK_METRIC name=<key> value=<number> [phase=<label>] [node=<name>]
#
# Contract, matching the rest of the soak's metric collection: additive and
# fail-soft. Missing jq, missing logs, an unreadable registry or unparseable
# lines all yield "{}" on stdout and exit 0. A metric must never fail the soak
# that measures it.

set -uo pipefail

MARKER="${1:?marker file (iteration .started) is required}"
LOG_ROOT="${2:?log root directory is required}"
REGISTRY="${3:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/soak-metrics.json}"

emit_empty() { printf '{}\n'; exit 0; }

command -v jq >/dev/null 2>&1 || emit_empty
[ -r "$REGISTRY" ] || emit_empty
[ -e "$MARKER" ] || emit_empty
[ -d "$LOG_ROOT" ] || emit_empty

keys="$(jq -r '.metrics[]?.key // empty' "$REGISTRY" 2>/dev/null)" || emit_empty
[ -n "$keys" ] || emit_empty

# One pass over the logs; keep only metric lines so the per-key greps below
# scan a small buffer rather than the full log set again.
lines="$(find "$LOG_ROOT" -type f \( -name '*.log' -o -name '*.txt' \) \
  -newer "$MARKER" 2>/dev/null \
  | xargs -r grep -h -o 'SOAK_METRIC [^"]*' 2>/dev/null)" || true
[ -n "${lines:-}" ] || emit_empty

out='{}'
while IFS= read -r key; do
  [ -n "$key" ] || continue
  # value= may be integer or float, optionally negative.
  stats="$(printf '%s\n' "$lines" \
    | grep -E "(^|[[:space:]])name=${key}([[:space:]]|$)" 2>/dev/null \
    | grep -oE 'value=-?[0-9]+(\.[0-9]+)?' \
    | cut -d= -f2 \
    | sort -g \
    | awk '{ a[NR] = $1 }
           END { if (NR == 0) exit
                 p50 = a[int((NR + 1) * 0.5)]
                 p95 = a[int((NR + 1) * 0.95)]
                 if (p50 == "") p50 = a[NR]
                 if (p95 == "") p95 = a[NR]
                 printf "%s %s %s %s %d\n", p50, p95, a[NR], a[1], NR }')" || continue
  [ -n "$stats" ] || continue
  # -c: callers embed this in a shell variable, so keep it one line.
  out="$(printf '%s' "$out" | jq -c \
    --arg key "$key" \
    --argjson p50 "$(printf '%s' "$stats" | awk '{print $1}')" \
    --argjson p95 "$(printf '%s' "$stats" | awk '{print $2}')" \
    --argjson max "$(printf '%s' "$stats" | awk '{print $3}')" \
    --argjson min "$(printf '%s' "$stats" | awk '{print $4}')" \
    --argjson samples "$(printf '%s' "$stats" | awk '{print $5}')" \
    '. + {($key): {p50: $p50, p95: $p95, max: $max, min: $min, samples: $samples}}' \
    2>/dev/null)" || continue
done <<EOF
$keys
EOF

printf '%s\n' "${out:-{\}}"
