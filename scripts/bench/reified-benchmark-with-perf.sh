#!/usr/bin/env bash
# scripts/bench/reified-benchmark-with-perf.sh
#
# Runs the benchmarks of Reified RSpaces (P10 step S8, design v2 section 2.8),
# the port of scripts/benchmark-with-perf.sh of the legacy branch at ff479042.
#
# Each run writes one directory with:
#   system.txt              the host, the CPU, the frequency driver, the
#                           governor and the boost state of the pinned CPUs,
#                           the load average at the start, the toolchain and
#                           the revision of the tree
#   BENCH.log               the Criterion output of each bench
#   BENCH-records.jsonl     one record for each case (RRS_BENCH_RECORD): the
#                           phlo, the root, the result and the host work
#   criterion/              the Criterion estimates and samples (CRITERION_HOME)
#   BENCH.uprof/            with --profile: the AMD uProf session of the bench
#   BENCH.uprof-report.csv  with --profile: the uProf report with the self and
#                           the deep (with callees) samples of each function
#   examples-hyperfine.json with --examples: hyperfine over every example of
#                           rholang/tests/reified_examples_execute.rs
#
# Usage:
#   reified-benchmark-with-perf.sh [--bench NAME]... [--filter PATTERN]
#       [--profile] [--profile-time S] [--examples] [--runs N] [--cpus LIST]
#       [--max-size N] [--out DIR]
#
#   --bench NAME     a bench of rholang/Cargo.toml (default: reified_spaces and
#                    reified_persistence); repeat it for several benches
#   --filter PATTERN the Criterion filter of the runs
#   --profile        record each bench run with AMD uProf, the profiler of the
#                    host CPU (AMD Zen 3): the hotspots configuration, which
#                    samples the CPU time with call stacks. Criterion runs each
#                    selected case for the profile time without its analysis
#                    (--profile-time), so the profile holds the deploys and
#                    not the statistics. The report of one bench runs while
#                    the next bench runs. The DWARF unwinders of uProf and of
#                    perf give no call stacks on these binaries, so a profiled
#                    run builds the bench with frame pointers in target/fp:
#                    the rustflags that cargo takes from .cargo/config.toml
#                    for the host target, and -C force-frame-pointers=yes.
#   --profile-time S the seconds of each case in a profiled run (default 10)
#   --examples       also time every example with hyperfine, one command each
#   --runs N         the hyperfine runs of each example (default 10)
#   --cpus LIST      the CPUs of taskset (default 2,3)
#   --max-size N     RRS_BENCH_MAX_SIZE of the size series
#   --out DIR        the run directory (default target/verification/
#                    reified-rspaces/benches/<UTC time>)
#
# The script never sets the CPU governor. It records the governor of each
# pinned CPU in system.txt, and the run should use the performance governor.
# Builds and runs run under systemd-run caps, unless RRS_SANDBOX=1.
#
# Exit status: 0 success, 1 a build or a run failed, 2 a usage error.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BENCHES=()
FILTER=""
PROFILE=false
EXAMPLES=false
RUNS=10
CPUS="${BENCHMARK_CPUS:-2,3}"
MAX_SIZE=""
OUT=""
RRS_MEMORY_MAX="${RRS_MEMORY_MAX:-32G}"
RRS_CPU_QUOTA="${RRS_CPU_QUOTA:-1800%}"

PROFILE_TIME=10

usage() {
  sed -n '/^# Usage:/,/^# The script never/p' "${BASH_SOURCE[0]}" | sed -e 's/^# \{0,1\}//' -e '$d'
}

while (($# > 0)); do
  case "$1" in
    --bench) BENCHES+=("${2:?--bench needs a name}"); shift 2 ;;
    --filter) FILTER="${2:?--filter needs a pattern}"; shift 2 ;;
    --profile) PROFILE=true; shift ;;
    --profile-time) PROFILE_TIME="${2:?--profile-time needs seconds}"; shift 2 ;;
    --examples) EXAMPLES=true; shift ;;
    --runs) RUNS="${2:?--runs needs a count}"; shift 2 ;;
    --cpus) CPUS="${2:?--cpus needs a list}"; shift 2 ;;
    --max-size) MAX_SIZE="${2:?--max-size needs a size}"; shift 2 ;;
    --out) OUT="${2:?--out needs a directory}"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "reified-benchmark-with-perf: unknown option $1" >&2; usage >&2; exit 2 ;;
  esac
done
((${#BENCHES[@]} > 0)) || BENCHES=(reified_spaces reified_persistence)
OUT="${OUT:-$ROOT/target/verification/reified-rspaces/benches/$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$OUT/criterion" "$OUT/tmp" || { echo "cannot create $OUT" >&2; exit 1; }
export TMPDIR="$OUT/tmp"
export CRITERION_HOME="$OUT/criterion"
[[ -n "$MAX_SIZE" ]] && export RRS_BENCH_MAX_SIZE="$MAX_SIZE"
cd "$ROOT" || exit 1

for tool in cargo taskset; do
  command -v "$tool" > /dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
$PROFILE && { command -v AMDuProfCLI > /dev/null || { echo "missing tool: AMDuProfCLI" >&2; exit 1; }; }
$EXAMPLES && { command -v hyperfine > /dev/null || { echo "missing tool: hyperfine" >&2; exit 1; }; }

bounded() {
  if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
    "$@"
  else
    systemd-run --user --scope --quiet -p "MemoryMax=$RRS_MEMORY_MAX" -p MemorySwapMax=0 \
      -p "CPUQuota=$RRS_CPU_QUOTA" -p IOWeight=30 -p TasksMax=400 -- "$@"
  fi
}

# The rustflags of a profiled build: the flags that cargo takes from
# .cargo/config.toml for the host target, where a target table wins over the
# [build] table, and frame pointers for the call stacks of uProf.
profile_rustflags() {
  python3 -I - "$ROOT/.cargo/config.toml" "$(rustc -vV | sed -n 's/^host: //p')" <<'EOF'
import sys
import tomllib

with open(sys.argv[1], "rb") as handle:
    config = tomllib.load(handle)
flags = config.get("target", {}).get(sys.argv[2], {}).get("rustflags")
if flags is None:
    flags = config.get("build", {}).get("rustflags", [])
print(" ".join([*flags, "-C", "force-frame-pointers=yes"]))
EOF
}
if $PROFILE; then
  RUSTFLAGS="$(profile_rustflags)" || { echo "cannot read the rustflags of .cargo/config.toml" >&2; exit 1; }
  export RUSTFLAGS
  export CARGO_TARGET_DIR="$ROOT/target/fp"
fi

# The CPUs that the profile reports use: CPUs that the benchmarks do not use.
report_cpus() {
  local all last
  last=$(( $(nproc) - 1 ))
  all="$(seq -s, 0 "$last")"
  python3 -I -c '
import sys
pinned = set()
for part in sys.argv[1].split(","):
    a, _, b = part.partition("-")
    pinned.update(range(int(a), int(b or a) + 1))
free = [c for c in map(int, sys.argv[2].split(",")) if c not in pinned]
print(",".join(map(str, free[-4:])))
' "$CPUS" "$all"
}

{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "uname: $(uname -srvm)"
  lscpu | grep -E '^(Model name|CPU\(s\)|Thread|Core|Socket|CPU max MHz|L3 cache)' || true
  for cpu in $(echo "$CPUS" | tr ',' ' '); do
    echo "governor of cpu$cpu: $(cat "/sys/devices/system/cpu/cpu$cpu/cpufreq/scaling_governor" 2>/dev/null || echo unknown)"
    echo "frequency driver of cpu$cpu: $(cat "/sys/devices/system/cpu/cpu$cpu/cpufreq/scaling_driver" 2>/dev/null || echo unknown)"
  done
  echo "boost: $(cat /sys/devices/system/cpu/cpufreq/boost 2>/dev/null || echo unknown)"
  echo "pinned cpus: $CPUS"
  echo "load average at the start: $(cut -d' ' -f1-3 /proc/loadavg)"
  echo "rustc: $(rustc -V)"
  echo "cargo: $(cargo -V)"
  echo "revision: $(git rev-parse HEAD)"
  echo "tree changes: $(git status --porcelain | wc -l) paths"
  echo "RRS_BENCH_MAX_SIZE: ${RRS_BENCH_MAX_SIZE:-default}"
  $PROFILE && echo "profile: AMD uProf $(AMDuProfCLI --version 2>/dev/null | sed -n 's/.*Version //p' | head -1), hotspots with frame-pointer stacks, ${PROFILE_TIME} s for each case"
  echo "RUSTFLAGS: ${RUSTFLAGS:-the flags of .cargo/config.toml}"
  echo "CARGO_TARGET_DIR: ${CARGO_TARGET_DIR:-target}"
} > "$OUT/system.txt"

# bench_executable NAME builds the bench NAME and prints its executable.
bench_executable() {
  local name=$1 log="$OUT/$1.build.log"
  bounded cargo bench -p rholang --bench "$name" --no-run > "$log" 2>&1 || return 1
  sed -n 's/.*Executable .*(\(.*\))$/\1/p' "$log" | tail -1
}

STATUS=0
REPORT_PIDS=()
for bench in "${BENCHES[@]}"; do
  executable="$(bench_executable "$bench")" || { echo "build of $bench failed: $OUT/$bench.build.log" >&2; STATUS=1; continue; }
  [[ -x "$ROOT/$executable" ]] && executable="$ROOT/$executable"
  args=(--bench)
  # The benches read the first argument that is not an option as the filter,
  # so an option with a value is written as --option=value.
  $PROFILE && args+=("--profile-time=$PROFILE_TIME")
  [[ -n "$FILTER" ]] && args+=("$FILTER")
  export RRS_BENCH_RECORD="$OUT/$bench-records.jsonl"
  echo "running $bench on cpus $CPUS"
  if $PROFILE; then
    # uProf sets the CPU affinity of the bench that it launches.
    bounded AMDuProfCLI collect --config hotspots --call-graph-depth 128 --affinity "$CPUS" \
      -o "$OUT/$bench.uprof" "$executable" "${args[@]}" > "$OUT/$bench.log" 2>&1 || STATUS=1
    # The report of this bench runs on other CPUs while the next bench runs.
    taskset -c "$(report_cpus)" AMDuProfCLI report -i "$OUT/$bench.uprof" --detail -g --cutoff 200 \
      --report-output "$OUT/$bench.uprof-report.csv" > "$OUT/$bench.uprof-report.log" 2>&1 &
    REPORT_PIDS+=("$!")
  else
    bounded taskset -c "$CPUS" "$executable" "${args[@]}" > "$OUT/$bench.log" 2>&1 || STATUS=1
  fi
done

if $EXAMPLES; then
  log="$OUT/examples.build.log"
  if bounded cargo test --release -p rholang --test reified_examples_execute --no-run > "$log" 2>&1; then
    harness="$(sed -n 's/.*Executable .*(\(.*\))$/\1/p' "$log" | tail -1)"
    [[ -x "$ROOT/$harness" ]] && harness="$ROOT/$harness"
    commands=()
    while IFS= read -r test; do
      commands+=(-n "$test" "taskset -c $CPUS $harness --exact $test --test-threads=1 -q")
    done < <("$harness" --list 2>/dev/null | sed -n 's/^\(example_[a-z0-9_]*\): test$/\1/p')
    hyperfine --warmup 1 --runs "$RUNS" --export-json "$OUT/examples-hyperfine.json" \
      "${commands[@]}" > "$OUT/examples-hyperfine.log" 2>&1 || STATUS=1
  else
    echo "build of the examples harness failed: $log" >&2
    STATUS=1
  fi
fi

for pid in "${REPORT_PIDS[@]}"; do
  wait "$pid" || STATUS=1
done
echo "reified-benchmark-with-perf: run directory $OUT, status $STATUS"
exit "$STATUS"
