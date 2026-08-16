#!/usr/bin/env bash
# Umbrella gate (LOCAL-ONLY): run every cost-accounted-rho verification gate and
# report a per-gate PASS / SKIP / FAIL matrix. Every discovered gate is
# mandatory by default; CA_ENFORCE_PROOFS=0 is an explicit local diagnostic mode.
#
# Env: SKIP_HEAVY=1 omits the slow legs (Rocq proofs, Lean) for a quick
# cross-witness sweep. Default runs every gate found.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SELF="$(basename "${BASH_SOURCE[0]}")"
source "$ROOT/scripts/lib/verification-tmpdir.sh"
verification_tmpdir_install "$ROOT/target/verification/cost-accounted-rho"

heavy_re='check-cost-accounted-rho-(proofs|lean)\.sh$'

declare -a names verdicts
overall=0
strict="${CA_ENFORCE_PROOFS:-1}"
export CA_ENFORCE_PROOFS="$strict"

gate_reported_skip() {
  grep -qiE '(^|[[:space:]])ADVISORY([[:space:]:()]|$)|(^|[[:space:]])SKIP[[:space:]]*\(|—[^[:cntrl:]]*skipped|leg skipped' <<<"$1"
}

if gate_reported_skip $'Summary: 1319 tests run: 1319 passed, 8 skipped\n--skip configured-test'; then
  echo "error: gate skip classifier matched an ordinary test summary" >&2
  exit 2
fi
for marker in 'ADVISORY (relaxed)' 'SKIP (tool absent)' 'tool — skipped (fail-soft)' 'termination leg skipped.'; do
  if ! gate_reported_skip "$marker"; then
    echo "error: gate skip classifier missed: $marker" >&2
    exit 2
  fi
done

for gate in "$ROOT"/scripts/check-cost-accounted-rho-*.sh; do
  base="$(basename "$gate")"
  [ "$base" = "$SELF" ] && continue
  if [ "${SKIP_HEAVY:-0}" = "1" ] && [[ "$base" =~ $heavy_re ]]; then
    names+=("$base")
    if [ "$strict" = "1" ]; then
      verdicts+=("FAIL(SKIP-heavy)")
      overall=1
    else
      verdicts+=("SKIP(heavy)")
    fi
    continue
  fi
  out="$(bash "$gate" 2>&1)"; rc=$?
  if [ "$rc" -ne 0 ]; then
    verdict="FAIL"; overall=1
  elif gate_reported_skip "$out"; then
    if [ "$strict" = "1" ]; then
      verdict="FAIL(SKIP)"; overall=1
    else
      verdict="SKIP"
    fi
  else
    verdict="PASS"
  fi
  names+=("$base"); verdicts+=("$verdict")
done

echo ""
echo "════════ cost-accounted-rho verification matrix ════════"
for i in "${!names[@]}"; do
  printf "  %-8s %s\n" "${verdicts[$i]}" "${names[$i]}"
done
echo "════════════════════════════════════════════════════════"
if [ "$overall" -eq 0 ]; then
  if [ "$strict" = "1" ]; then
    echo "All discovered gates completed without skips."
  else
    echo "All present gates passed (skips are tool-absent cross-witnesses)."
  fi
else
  echo "error: at least one gate FAILED" >&2
fi
exit "$overall"
