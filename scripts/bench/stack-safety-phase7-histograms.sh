#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out_dir="$repo_root/target/phase7-stack-safety"
raw="$out_dir/phase7-depth-events.tsv"
summary="$out_dir/phase7-depth-histograms.tsv"
corpora="$out_dir/phase7-depth-corpora.tsv"

if [[ "${PHASE7_HISTOGRAM_CAPPED:-0}" != 1 ]]; then
  exec systemd-run --user --wait --collect --pipe \
    --working-directory="$repo_root" \
    -p MemoryMax=4G \
    -p MemorySwapMax=0 \
    -p TasksMax=256 \
    --setenv=CARGO_BUILD_JOBS=1 \
    --setenv=PHASE7_HISTOGRAM_CAPPED=1 \
    "$0"
fi

mkdir -p "$out_dir"
: > "$raw"

# Every subject gets its own production-facing corpus. The subject selector is
# load-bearing: without it, codec hooks reached incidentally by another corpus
# would mix populations and recreate the inheritance error this measurement is
# meant to close.
run_rholang() {
  local subject="$1"
  local suite="$2"
  PHASE7_DEPTH_HISTOGRAM_PATH="$raw" \
  PHASE7_DEPTH_HISTOGRAM_SUITE="$suite" \
  PHASE7_DEPTH_HISTOGRAM_SUBJECT="$subject" \
    cargo test -p rholang --test "$suite" \
      --features phase7-depth-histograms \
      -- --test-threads=1
}

# Escape payloads are absent from 903cefb3's five-suite event-hash population.
# `drop_head_spec` is the ordinary evaluator route that creates, operates on,
# and round-trips shallow escaped keys. Dedicated depth-ladder fixtures are
# deliberately excluded because they would manufacture the distribution.
run_rholang escape_arm drop_head_spec

# The exact five-target production-weighted event-hash corpus from 903cefb3.
event_hash_suites=(demo_verification interpreter_spec reduce_spec where_examples_compile rholang_numeric_eval_spec)
for suite in "${event_hash_suites[@]}"; do
  run_rholang bincode_encoder "$suite"
done

# Organic cold-store reads from the same interpreter corpus, plus the
# EPathMap-heavy checkpoint/replay route. Differential and depth-ladder tests
# are excluded from the population.
for suite in "${event_hash_suites[@]}" epathmap_replay_equivalence_spec; do
  run_rholang bincode_decoder "$suite"
done

# The foreign-function boundary is the production protobuf ingress in this
# workspace. Its tests build legal parent messages, decode them through the
# real prost entry point, and therefore exercise generated Par::merge calls.
PHASE7_DEPTH_HISTOGRAM_PATH="$raw" \
PHASE7_DEPTH_HISTOGRAM_SUITE="rspace_rhotypes_ffi" \
PHASE7_DEPTH_HISTOGRAM_SUBJECT="protobuf_decoder" \
  cargo test -p rspace_plus_plus_rhotypes --tests \
    --features models/phase7-depth-histograms \
    -- --test-threads=1

printf 'subject\troot_kind\tdepth\tcount\tshare_percent\n' > "$summary"
awk -F '\t' '
  NF == 4 {
    key = $2 SUBSEP $3 SUBSEP $4
    counts[key]++
    totals[$2]++
  }
  END {
    for (key in counts) {
      split(key, fields, SUBSEP)
      printf "%s\t%s\t%s\t%d\t%.6f\n", fields[1], fields[2], fields[3], counts[key], 100 * counts[key] / totals[fields[1]]
    }
  }
' "$raw" | sort -t $'\t' -k1,1 -k2,2 -k3,3n >> "$summary"

printf 'subject\tcorpus\tobservations\n' > "$corpora"
awk -F '\t' '
  NF == 4 { counts[$2 SUBSEP $1]++ }
  END {
    for (key in counts) {
      split(key, fields, SUBSEP)
      printf "%s\t%s\t%d\n", fields[1], fields[2], counts[key]
    }
  }
' "$raw" | sort -t $'\t' -k1,1 -k2,2 >> "$corpora"

for subject in escape_arm bincode_encoder bincode_decoder protobuf_decoder; do
  if ! awk -F '\t' -v subject="$subject" 'NR > 1 && $1 == subject { found = 1 } END { exit !found }' "$summary"; then
    printf 'Phase 7 histogram is vacuous: subject %s had zero production-corpus observations\n' "$subject" >&2
    exit 1
  fi
done

printf 'raw observations: %s\nhistogram: %s\ncorpora: %s\n' "$raw" "$summary" "$corpora"
cat "$summary"
cat "$corpora"
