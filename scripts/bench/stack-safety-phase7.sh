#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT/scripts/bench/stack-safety-phase7.sh"
MODE="${1:-all}"
MEMORY_MAX="${PHASE7_MEMORY_MAX:-4G}"

# This harness runs many instrumented Rust processes. Refuse an accidental
# uncapped invocation: the outer process re-execs the whole campaign in one
# zero-swap cgroup, including the build.
if [[ "${PHASE7_CAPPED:-0}" != "1" ]]; then
  systemd_args=(
    --user --wait --collect --pipe
    --working-directory="$ROOT"
    -p "MemoryMax=$MEMORY_MAX"
    -p MemorySwapMax=0
    -p TasksMax=256
    --setenv=PHASE7_CAPPED=1
    --setenv=CARGO_BUILD_JOBS=1
  )
  for variable in \
    PHASE7_SUBJECTS PHASE7_DEPTH_RUNGS PHASE7_WIDTH_RUNGS \
    PHASE7_HEAP_RUNGS PHASE7_MAX_EXPONENT PHASE7_OUT_DIR; do
    if [[ -n "${!variable+x}" ]]; then
      systemd_args+=(--setenv="$variable=${!variable}")
    fi
  done
  exec systemd-run "${systemd_args[@]}" "$SCRIPT" "$@"
fi

case "$MODE" in
  all|time|heap|manifest) ;;
  *)
    echo "usage: $0 [all|time|heap|manifest]" >&2
    exit 2
    ;;
esac

for tool in cargo jq valgrind awk; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "error: $tool is required" >&2
    exit 2
  }
done

ulimit -c 0
TMP="$(mktemp -d /tmp/f1r3node-phase7.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT
OUT_DIR="${PHASE7_OUT_DIR:-$ROOT/target/phase7-stack-safety}"
mkdir -p "$OUT_DIR"

BUILD_JSON="$TMP/build.json"
echo "Building the release stack-depth instrument (one cargo job) ..." >&2
cargo test --release -p rholang --test stack_depth_gate --no-run \
  --message-format=json >"$BUILD_JSON"
BIN="$(jq -r '
  select(.reason == "compiler-artifact")
  | select(.target.name == "stack_depth_gate")
  | select(.profile.test == true)
  | .executable // empty
' "$BUILD_JSON" | tail -n 1)"
[[ -n "$BIN" && -x "$BIN" ]] || {
  echo "error: cargo did not report the stack_depth_gate executable" >&2
  exit 2
}

MANIFEST_RAW="$TMP/manifest.raw"
MANIFEST="$OUT_DIR/phase7-manifest.tsv"
"$BIN" --ignored --exact phase7_measurement_manifest --nocapture --test-threads=1 \
  >"$MANIFEST_RAW"
awk -F '\t' '$1 == "PHASE7_MANIFEST" { print $2 "\t" $3 }' \
  "$MANIFEST_RAW" >"$MANIFEST"
EXPECTED_MANIFEST_COUNT="$(awk -F '\t' '$1 == "PHASE7_MANIFEST_COUNT" { print $2 }' "$MANIFEST_RAW")"
ACTUAL_MANIFEST_COUNT="$(wc -l <"$MANIFEST")"
[[ -n "$EXPECTED_MANIFEST_COUNT" && "$ACTUAL_MANIFEST_COUNT" -eq "$EXPECTED_MANIFEST_COUNT" ]] || {
  echo "error: gate exported $ACTUAL_MANIFEST_COUNT Phase 7 rows; expected $EXPECTED_MANIFEST_COUNT" >&2
  exit 2
}

if [[ "$MODE" == "manifest" ]]; then
  cat "$MANIFEST"
  exit 0
fi

selected() {
  local subject="$1"
  local filter="${PHASE7_SUBJECTS:-}"
  local requested=" ${filter//,/ } "
  [[ -z "$filter" || "$requested" == *" $subject "* ]]
}

run_gate_child() {
  local subject="$1" parameter="$2"
  shift 2
  GATE_SUBJECT="$subject" GATE_DEPTH="$parameter" GATE_STACK=67108864 \
    "$@" "$BIN" --ignored --exact gate_child --test-threads=1
}

cache_metric() {
  local file="$1" wanted="$2"
  awk -v wanted="$wanted" '
    /^events:/ {
      for (i = 2; i <= NF; i++) if ($i == wanted) column = i
    }
    /^summary:/ {
      if (!column) exit 2
      print $column
      found = 1
    }
    END { if (!found) exit 2 }
  ' "$file"
}

run_cachegrind() {
  local subject="$1" parameter="$2" key="$3"
  local raw="$TMP/cachegrind.$key.out" log="$TMP/cachegrind.$key.log"
  if ! run_gate_child "$subject" "$parameter" \
      valgrind --quiet --tool=cachegrind --cache-sim=yes --branch-sim=no \
      --cachegrind-out-file="$raw" >"$log" 2>&1; then
    cat "$log" >&2
    echo "error: Cachegrind cell failed: $subject @ $parameter" >&2
    exit 1
  fi
  printf '%s\t%s\t%s\n' \
    "$(cache_metric "$raw" Ir)" \
    "$(cache_metric "$raw" Dr)" \
    "$(cache_metric "$raw" Dw)"
}

run_time_axis() {
  local rows="$OUT_DIR/phase7-cachegrind.tsv"
  local controls="$OUT_DIR/phase7-cachegrind-controls.tsv"
  local fits="$OUT_DIR/phase7-cachegrind-fits.tsv"
  local depth_rungs="${PHASE7_DEPTH_RUNGS:-128 256 512 768}"
  local width_rungs="${PHASE7_WIDTH_RUNGS:-128 256 512 768}"
  local threshold="${PHASE7_MAX_EXPONENT:-1.80}"
  declare -A control_ir control_dr control_dw

  printf 'axis\tsubject\tparameter\tIr\tDr\tDw\tcontrol_Ir\tcontrol_Dr\tcontrol_Dw\tnet_Ir\tnet_Dr\tnet_Dw\n' >"$rows"
  printf 'parameter\tIr\tDr\tDw\n' >"$controls"

  while IFS=$'\t' read -r axis subject; do
    selected "$subject" || continue
    local rungs="$depth_rungs"
    [[ "$axis" == "width" ]] && rungs="$width_rungs"
    echo "Cachegrind: $axis/$subject" >&2
    for parameter in $rungs; do
      if [[ -z "${control_ir[$parameter]+x}" ]]; then
        IFS=$'\t' read -r control_ir[$parameter] control_dr[$parameter] control_dw[$parameter] \
          < <(run_cachegrind phase7_invariant "$parameter" "control.$parameter")
        printf '%s\t%s\t%s\t%s\n' "$parameter" \
          "${control_ir[$parameter]}" "${control_dr[$parameter]}" "${control_dw[$parameter]}" \
          >>"$controls"
      fi
      local ir dr dw
      IFS=$'\t' read -r ir dr dw \
        < <(run_cachegrind "$subject" "$parameter" "$axis.$subject.$parameter")
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$axis" "$subject" "$parameter" "$ir" "$dr" "$dw" \
        "${control_ir[$parameter]}" "${control_dr[$parameter]}" "${control_dw[$parameter]}" \
        "$((ir - control_ir[$parameter]))" \
        "$((dr - control_dr[$parameter]))" \
        "$((dw - control_dw[$parameter]))" >>"$rows"
    done
  done <"$MANIFEST"

  # A parameter-invariant arm must remain invariant. All default rungs have the
  # same decimal width, so even environment parsing is identical.
  awk -F '\t' '
    NR == 1 { next }
    {
      for (i = 2; i <= 4; i++) {
        if (!(i in min) || $i < min[i]) min[i] = $i
        if (!(i in max) || $i > max[i]) max[i] = $i
      }
    }
    END {
      names[2] = "Ir"; names[3] = "Dr"; names[4] = "Dw"
      for (i = 2; i <= 4; i++) {
        span = max[i] - min[i]
        rel = max[i] ? span / max[i] : 0
        printf "control_%s_span\t%d\t%.8f\n", names[i], span, rel
        if (rel > 0.001) bad = 1
      }
      exit bad
    }
  ' "$controls" >"$OUT_DIR/phase7-cachegrind-control-verdict.tsv" || {
    echo "error: Cachegrind invariant control moved by more than 0.1%" >&2
    return 1
  }

  # Log-log least-squares exponent for fixture/control-subtracted Ir/Dr/Dw.
  # Exponent >= 1.80 is treated as evidence of quadratic-or-worse growth and
  # makes the campaign red; it is not rounded down to a linear label.
  if ! awk -F '\t' -v threshold="$threshold" '
    function reset() {
      n = sx = sxx = sy_ir = sxy_ir = sy_dr = sxy_dr = sy_dw = sxy_dw = 0
    }
    function emit(den, p_ir, p_dr, p_dw, maxp, verdict) {
      if (!n) return
      den = n * sxx - sx * sx
      p_ir = den ? (n * sxy_ir - sx * sy_ir) / den : 0
      p_dr = den ? (n * sxy_dr - sx * sy_dr) / den : 0
      p_dw = den ? (n * sxy_dw - sx * sy_dw) / den : 0
      maxp = p_ir
      if (p_dr > maxp) maxp = p_dr
      if (p_dw > maxp) maxp = p_dw
      verdict = maxp >= threshold ? "RED_QUADRATIC_OR_WORSE" : "PASS_SUBQUADRATIC"
      printf "%s\t%s\t%d\t%.4f\t%.4f\t%.4f\t%s\n", axis, subject, n, p_ir, p_dr, p_dw, verdict
      if (maxp >= threshold) bad = 1
    }
    BEGIN { print "axis\tsubject\tpoints\tIr_exponent\tDr_exponent\tDw_exponent\tverdict" }
    NR == 1 { next }
    {
      key = $1 SUBSEP $2
      if (last != "" && key != last) { emit(); reset() }
      axis = $1; subject = $2; last = key
      x = log($3)
      ir = $10 > 0 ? $10 : 1
      dr = $11 > 0 ? $11 : 1
      dw = $12 > 0 ? $12 : 1
      n++; sx += x; sxx += x * x
      sy_ir += log(ir); sxy_ir += x * log(ir)
      sy_dr += log(dr); sxy_dr += x * log(dr)
      sy_dw += log(dw); sxy_dw += x * log(dw)
    }
    END { emit(); exit bad }
  ' "$rows" >"$fits"; then
    echo "error: at least one converted traversal fitted exponent >= $threshold" >&2
    return 1
  fi

  echo "Cachegrind rows: $rows" >&2
  echo "Cachegrind fits: $fits" >&2
}

massif_peak() {
  awk -F= '
    /^mem_heap_B=/ { heap = $2 }
    /^mem_heap_extra_B=/ { extra = $2 }
    /^heap_tree=/ {
      total = heap + extra
      if (total > peak) peak = total
    }
    END { print peak + 0 }
  ' "$1"
}

run_massif() {
  local subject="$1" parameter="$2" key="$3"
  local raw="$TMP/massif.$key.out" log="$TMP/massif.$key.log"
  if ! run_gate_child "$subject" "$parameter" \
      valgrind --quiet --tool=massif --time-unit=B --stacks=no \
      --pages-as-heap=no --max-snapshots=1000 --detailed-freq=1 \
      --peak-inaccuracy=0 --massif-out-file="$raw" >"$log" 2>&1; then
    cat "$log" >&2
    echo "error: Massif cell failed: $subject @ $parameter" >&2
    exit 1
  fi
  massif_peak "$raw"
}

run_heap_axis() {
  local rows="$OUT_DIR/phase7-massif.tsv"
  local summary="$OUT_DIR/phase7-massif-growth.tsv"
  local rungs="${PHASE7_HEAP_RUNGS:-256 1024}"
  local subjects=(substitute sort normalize eval_with_nots)
  local controls=(heap_fixture_par heap_fixture_par heap_fixture_normalize heap_fixture_eval)
  declare -A control_peak

  printf 'subject\tparameter\tpeak_heap_bytes\tcontrol\tcontrol_peak_bytes\tnet_peak_bytes\n' >"$rows"
  for i in "${!subjects[@]}"; do
    local subject="${subjects[$i]}" control="${controls[$i]}"
    selected "$subject" || continue
    echo "Massif: $subject (control $control)" >&2
    for parameter in $rungs; do
      local control_key="$control.$parameter"
      if [[ -z "${control_peak[$control_key]+x}" ]]; then
        control_peak[$control_key]="$(run_massif "$control" "$parameter" "control.$control_key")"
      fi
      local peak
      peak="$(run_massif "$subject" "$parameter" "$subject.$parameter")"
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$subject" "$parameter" "$peak" "$control" "${control_peak[$control_key]}" \
        "$((peak - control_peak[$control_key]))" >>"$rows"
    done
  done

  awk -F '\t' '
    BEGIN { print "subject\tlo\thi\tnet_lo_bytes\tnet_hi_bytes\tbytes_per_level" }
    NR == 1 { next }
    {
      if ($1 != subject && subject != "") emit()
      if ($1 != subject) {
        subject = $1; lo = $2; net_lo = $6
      }
      hi = $2; net_hi = $6
    }
    END { emit() }
    function emit() {
      span = hi - lo
      slope = span ? (net_hi - net_lo) / span : 0
      printf "%s\t%d\t%d\t%d\t%d\t%.2f\n", subject, lo, hi, net_lo, net_hi, slope
    }
  ' "$rows" >"$summary"

  echo "Massif rows: $rows" >&2
  echo "Massif growth: $summary" >&2
}

case "$MODE" in
  time) run_time_axis ;;
  heap) run_heap_axis ;;
  all)
    run_time_axis
    run_heap_axis
    ;;
esac
