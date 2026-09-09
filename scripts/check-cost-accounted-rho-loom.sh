#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

for gate_tool in cargo timeout; do
  if ! command -v "$gate_tool" >/dev/null 2>&1; then
    echo "Required tool is unavailable: $gate_tool" >&2
    exit 1
  fi
done

for partial_limit in LOOM_MAX_PERMUTATIONS LOOM_MAX_DURATION LOOM_CHECKPOINT_FILE; do
  if [[ -v $partial_limit ]]; then
    echo "Qualification rejects partial exploration or checkpoint resume: $partial_limit" >&2
    exit 1
  fi
done

preemptions="${LOOM_MAX_PREEMPTIONS:-3}"
branches="${LOOM_MAX_BRANCHES:-1000}"
if [[ ! $preemptions =~ ^[0-9]+$ || ! $branches =~ ^[1-9][0-9]*$ ]]; then
  echo "Loom bounds must be nonnegative preemptions and positive branches." >&2
  exit 1
fi

echo "Checking Loom models: preemption bound=$preemptions, default branch bound=$branches."
echo "A model can raise its branch bound explicitly. Early-success limits and checkpoint resume are prohibited."

out="$(cd "$ROOT" && RUSTFLAGS="--cfg loom -C target-cpu=native" \
  LOOM_MAX_PREEMPTIONS="$preemptions" LOOM_MAX_BRANCHES="$branches" \
  timeout 900 cargo test --locked -p cost-accounting-loom-models 2>&1)"
rc=$?
printf '%s\n' "$out"

if [ "$rc" -ne 0 ]; then
  echo "Loom qualification failed with exit status $rc." >&2
  exit "$rc"
fi

counts="$(printf '%s\n' "$out" | awk '
  /^test result: ok[.]/ {
    for (i = 1; i <= NF; i++) {
      if ($i == "passed;") passed += $(i - 1)
      if ($i == "failed;") failed += $(i - 1)
      if ($i == "ignored;") ignored += $(i - 1)
      if ($i == "filtered") filtered += $(i - 1)
    }
  }
  END { print passed + 0, failed + 0, ignored + 0, filtered + 0 }
')"
read -r passed failed ignored filtered <<< "$counts"
if (( passed == 0 || failed != 0 || ignored != 0 || filtered != 0 )); then
  echo "Loom qualification requires passing tests with no failures, ignored tests, or filtered tests: $counts" >&2
  exit 1
fi

echo "Loom passed: $passed tests completed within preemption bound $preemptions."
echo "This result does not establish unbounded exploration or full production refinement."
