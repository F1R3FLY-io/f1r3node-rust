#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

mapfile -d '' documentation < <(find docs/casper/theory/uptime -type f -name '*.md' -print0)

if [[ "${#documentation[@]}" -eq 0 ]]; then
  printf 'error: no uptime documentation found\n' >&2
  exit 1
fi

failures=0
for document in "${documentation[@]}"; do
  if ! perl -ne '
    if (/^```/) {
      $fenced = !$fenced;
      next;
    }
    next if $fenced;
    $line = $_;
    $line =~ s/\$`[^`]*`\$//g;
    $line =~ s/`[^`]*`//g;
    if ($line =~ /\$\$/) {
      print "$ARGV:$.: bare double-dollar math delimiter\n";
      $failed = 1;
    }
    while ($line =~ /(?<!\$)\$([^\n\$]+)\$(?!\$)/g) {
      print "$ARGV:$.: bare inline math delimiter: $&\n";
      $failed = 1;
    }
    END {
      if ($fenced) {
        print "$ARGV: unclosed fenced code block\n";
        $failed = 1;
      }
      exit($failed ? 1 : 0);
    }
  ' "$document"; then
    failures=$((failures + 1))
  fi
done

if [[ "$failures" -ne 0 ]]; then
  printf 'error: pgmcp uptime documentation syntax failed for %s file(s)\n' "$failures" >&2
  exit 1
fi

projection_register="docs/casper/theory/uptime/shard-failure-modes.md"
required_projection_markers=(
  "## Projection and regression model inventory"
  "shard reliability CTMC"
  "held-out soak model"
  "RSS guard reconstruction"
  "right-censored lifetime model"
  "calibrated competing-risk first loss"
  "correlated validator failure"
  "workload transition model"
  "memory trajectory model"
  "latency-tail model"
  "joint calibration model"
  "Bayesian hierarchical joint longitudinal"
  "engineering envelope**, not a"
)
for marker in "${required_projection_markers[@]}"; do
  if ! grep -Fq -- "$marker" "$projection_register"; then
    printf 'error: uptime projection register is missing required model marker: %s\n' "$marker" >&2
    exit 1
  fi
done

preflight_manifest="formal/storm/uptime/backtests/preflight-33099406770.json"
jq -e '
  .evidence_class == "preflight_transient_observation"
  and .github_run_id == 33099406770
  and .artifact.sha256 == "d7e448adfa3cd65d6e44df44086bbde5010010755f0bcb5fedc150b6d1ac7cb6"
  and .preflight.failure.deploys_not_finalized_within_deadline == 604
  and .preflight.failed_test_topology.validators == 4
  and .soak.completed_segments == 0
  and .soak.continuous_shard_exposure_seconds == 0
  and .statistical_treatment.shard_lifetime_observation == false
  and .statistical_treatment.current_three_validator_profile_compatible == false
  and .statistical_treatment.uptime_rate_identifiable == false
' "$preflight_manifest" >/dev/null || {
  printf 'error: run-330 preflight manifest lost its fail-closed evidence classification\n' >&2
  exit 1
}

for diagram in docs/casper/theory/uptime/diagrams/*.puml; do
  svg="${diagram%.puml}.svg"
  test -s "$svg" || {
    printf 'error: missing rendered uptime diagram: %s\n' "$svg" >&2
    exit 1
  }
  if [[ "$diagram" -nt "$svg" ]]; then
    printf 'error: stale rendered uptime diagram: %s\n' "$svg" >&2
    exit 1
  fi
done

printf 'pgmcp uptime documentation passed for %s files.\n' "${#documentation[@]}"
