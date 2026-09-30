#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SELF="$(basename "${BASH_SOURCE[0]}")"

heavy_re='check-cost-accounted-rho-(proofs|lean)\.sh$'

declare -a names verdicts
overall=0
strict="${CA_ENFORCE_PROOFS:-0}"
log_dir="$ROOT/target/verification/cost-accounted-rho-all"
mkdir -p "$log_dir"

if [ "$strict" = "1" ] && [ "${SKIP_HEAVY:-0}" = "1" ]; then
  echo "error: SKIP_HEAVY=1 is incompatible with CA_ENFORCE_PROOFS=1" >&2
  exit 2
fi

for gate in "$ROOT"/scripts/check-cost-accounted-rho-*.sh; do
  base="$(basename "$gate")"
  [ "$base" = "$SELF" ] && continue
  if [ "${SKIP_HEAVY:-0}" = "1" ] && [[ "$base" =~ $heavy_re ]]; then
    log_file="$log_dir/${base%.sh}.log"
    printf '%s\n' 'SKIP(heavy): SKIP_HEAVY=1' >"$log_file"
    names+=("$base"); verdicts+=("SKIP(heavy)"); continue
  fi
  log_file="$log_dir/${base%.sh}.log"
  if bash "$gate" >"$log_file" 2>&1; then
    rc=0
  else
    rc=$?
  fi
  if [ "$rc" -ne 0 ]; then
    verdict="FAIL"; overall=1
  elif grep -qE '— .*skipped|leg skipped([[:space:].(]|$)|^[[:space:]]*SKIP([[:space:]]|\()|ADVISORY' "$log_file"; then
    verdict="SKIP"
    if [ "$strict" = "1" ]; then
      overall=1
    fi
  else
    verdict="PASS"
  fi
  names+=("$base"); verdicts+=("$verdict")
done

echo ""
echo "════════ cost-accounted-rho verification matrix ════════"
for i in "${!names[@]}"; do
  printf "  %-8s %s %s\n" "${verdicts[$i]}" "${names[$i]}" "$log_dir/${names[$i]%.sh}.log"
done
echo "════════════════════════════════════════════════════════"
if [ "$overall" -eq 0 ]; then
  echo "All required gates passed."
else
  echo "error: at least one required gate failed or was skipped" >&2
fi
exit "$overall"
