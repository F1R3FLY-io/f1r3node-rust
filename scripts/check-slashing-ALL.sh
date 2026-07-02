#!/usr/bin/env bash
# scripts/check-slashing-ALL.sh
#
# LOCAL-ONLY verification gate for the slashing subsystem — the parity gate that
# finalized-floor and fork-choice already have, so the slashing FV cannot rot
# silently (its axiom-free claim previously lived only in doc §14, run by hand).
#
#   1. Rocq  (AUTHORITATIVE) — builds formal/rocq/slashing and asserts EVERY
#      `main_*` capstone in MainTheorem.v is axiom-free (`Print Assumptions` ⇒
#      "Closed under the global context"), then re-checks the whole library through
#      the trusted kernel (`coqchk`). Any failure fails the gate.
#   2. TLA+  (fail-soft) — TLC on the slashing safety cfgs (equivocation detector,
#      concurrent tracker, slash flow) + the pre-fix counterexample cfgs (must
#      reproduce their violation). SKIPPED if no TLC jar.
#   3. Deep tiers (fail-soft) — points at scripts/ci/slashing-search-horizon.sh for
#      the Kani / Miri / fuzz / Sage economic-safety tiers (heavier; run separately).
#
# POLICY: LOCAL-ONLY. Do NOT wire this (or any Rocq/TLA+ step) into
# .github/workflows/* — an earlier formal-CI workflow was deliberately removed.
#
# Env knobs: ROCQ_MEMMAX=16G (systemd MemoryMax for the Rocq build).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SL_DIR="$REPO_ROOT/formal/rocq/slashing"
TLA_DIR="$REPO_ROOT/formal/tlaplus/slashing"
ROCQ_MEMMAX="${ROCQ_MEMMAX:-16G}"

rc=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; rc=1; }
skip() { printf '  \033[33mSKIP\033[0m %s\n' "$1"; }

capped() {
  if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope true >/dev/null 2>&1; then
    systemd-run --user --scope -p "MemoryMax=$ROCQ_MEMMAX" -p CPUQuota=1800% -p TasksMax=200 "$@"
  else
    "$@"
  fi
}

echo "== [1/3] Rocq (authoritative) =="
if command -v coqc >/dev/null 2>&1 || [[ -x "$HOME/.opam/default/bin/coqc" ]]; then
  # shellcheck disable=SC1090
  eval "$(opam env 2>/dev/null)" 2>/dev/null || true
  ( cd "$SL_DIR" && coq_makefile -f _CoqProject -o Makefile ) >/dev/null 2>&1
  if capped make -C "$SL_DIR" -j1 >/tmp/sl_rocq_build.log 2>&1; then
    pass "Rocq build (slashing: $(grep -c '^theories/' "$SL_DIR/_CoqProject") modules)"
    # Every `main_*` capstone in MainTheorem.v must be axiom-free. The list is
    # generated from the source so the gate never drifts from the theorem count.
    tmpd=$(mktemp -d); chk="$tmpd/GateCheck.v"
    thms=$(grep -oE '^Theorem main_[A-Za-z0-9_]+' "$SL_DIR/theories/MainTheorem.v" | sed 's/^Theorem //')
    n_thm=$(wc -l <<<"$thms")
    { echo "From Slashing Require Import MainTheorem."
      while IFS= read -r t; do echo "Print Assumptions $t."; done <<<"$thms"; } > "$chk"
    out=$(coqc -Q "$SL_DIR/theories" Slashing "$chk" 2>&1); rm -rf "$tmpd"
    n_closed=$(grep -c "Closed under the global context" <<<"$out")
    if [[ "$n_closed" == "$n_thm" ]]; then
      pass "all $n_thm slashing capstones axiom-free (main_T1..T10, T9_*, TAuth, TSlash, TIdem)"
    else
      fail "slashing capstones NOT all axiom-free ($n_closed/$n_thm Closed):"; echo "$out" | grep -iE "axiom|assumption" | head | sed 's/^/      /'
    fi
    # Independent kernel re-check.
    if capped coqchk -Q "$SL_DIR/theories" Slashing Slashing.MainTheorem \
         >/tmp/sl_coqchk.log 2>&1 && grep -q "Modules were successfully checked" /tmp/sl_coqchk.log; then
      pass "coqchk kernel re-check (MainTheorem + all deps)"
    else
      fail "coqchk kernel re-check FAILED (see /tmp/sl_coqchk.log)"; tail -10 /tmp/sl_coqchk.log | sed 's/^/      /'
    fi
  else
    fail "Rocq build failed (see /tmp/sl_rocq_build.log)"; tail -20 /tmp/sl_rocq_build.log | sed 's/^/      /'
  fi
else
  fail "coqc not found — Rocq is authoritative, cannot skip"
fi

echo "== [2/3] TLA+ (fail-soft) =="
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  # shellcheck disable=SC1091
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"
  # Fast safety cfgs (seconds; exhaustive at their bounds) — MUST pass. Each
  # MC_*.cfg substitutes MC_-prefixed identifiers (MC_Validators, …) defined in
  # the MC_*.tla WRAPPER (`EXTENDS <base>, TLC`), so TLC runs against the wrapper,
  # not the base. The 1v detector-liveness model checks ALL 20 detector safety
  # invariants PLUS the Live_DetectionComplete temporal property on a tiny
  # (69-state) space — full detector-invariant coverage without the 2v blow-up;
  # the exhaustive 2v safety run is budgeted separately below.
  declare -A SAFE=(
    [detector_liveness]="MC_EquivocationDetector_liveness.cfg:MC_EquivocationDetector_liveness.tla"
    [tracker]="MC_ConcurrentTracker.cfg:MC_ConcurrentTracker.tla"
    [slashflow]="MC_SlashFlow.cfg:MC_SlashFlow.tla"
  )
  for k in "${!SAFE[@]}"; do
    cfg="${SAFE[$k]%%:*}"; mod="${SAFE[$k]##*:}"
    if [[ -f "$TLA_DIR/$cfg" && -f "$TLA_DIR/$mod" ]]; then
      if tlc_run "$(tlc_metadir "sl_$k")" "$TLA_DIR/$cfg" "$TLA_DIR/$mod" >/tmp/sl_tlc_$k.log 2>&1; then
        pass "TLA+ slashing $k ($cfg)"
      else
        fail "TLA+ slashing $k did NOT pass ($cfg; see /tmp/sl_tlc_$k.log)"
      fi
    else
      skip "TLA+ slashing $k: $cfg / $mod not present"
    fi
  done
  # Pre-fix counterexample — must reproduce its violation (inverted sense).
  if [[ -f "$TLA_DIR/MC_ConcurrentTracker_pre_fix.cfg" ]]; then
    if tlc_run "$(tlc_metadir sl_tracker_pre)" "$TLA_DIR/MC_ConcurrentTracker_pre_fix.cfg" "$TLA_DIR/MC_ConcurrentTracker.tla" >/tmp/sl_tlc_tracker_pre.log 2>&1; then
      fail "TLA+ concurrent-tracker pre-fix should VIOLATE its invariant but passed"
    elif grep -qi "is violated" /tmp/sl_tlc_tracker_pre.log; then
      pass "TLA+ concurrent-tracker pre-fix reproduces the race counterexample"
    else
      fail "TLA+ concurrent-tracker pre-fix failed for the wrong reason (see /tmp/sl_tlc_tracker_pre.log)"
    fi
  fi
  # Heavy exhaustive detector SAFETY (2v × 2s × 2b, no symmetry) — the full safety
  # proof over tens of millions of distinct states (doc §14 records it ✅-exhausted,
  # 0 violations). BUDGETED so it never gates the fast path: PASS on clean
  # exhaustion, fail on a real violation, SKIP-with-note past SL_DETECTOR_SAFETY_BUDGET
  # seconds (default 300; set 0 to remove the cap and run to completion). Rocq is
  # AUTHORITATIVE for detection soundness (T-1/T-6, axiom-free); this is defense-in-
  # depth. NB: run `java` as timeout's DIRECT child (not via tlc-run.sh's
  # `systemd-run --scope`, which detaches into its own unit and would ORPHAN the JVM
  # when timeout kills the wrapper); heap is bounded by -Xmx and the fingerprint
  # graph lands on-disk (NVMe target/), so RAM stays bounded without the cgroup.
  dsafe_cfg="MC_EquivocationDetector_safety.cfg"; dsafe_mod="MC_EquivocationDetector_safety.tla"
  budget="${SL_DETECTOR_SAFETY_BUDGET:-300}"
  if [[ -f "$TLA_DIR/$dsafe_cfg" && -f "$TLA_DIR/$dsafe_mod" ]]; then
    dmeta="$(tlc_metadir sl_detector)"; rm -rf "${dmeta:?}/"* 2>/dev/null || true
    tocmd=(); [[ "$budget" != "0" ]] && tocmd=(timeout --signal=TERM --kill-after=15 "$budget")
    "${tocmd[@]}" java "-Xmx${TLC_HEAP:-4g}" -XX:+UseParallelGC -cp "$TLC_JAR" tlc2.TLC \
      -workers "${TLC_WORKERS:-4}" -metadir "$dmeta" \
      -config "$TLA_DIR/$dsafe_cfg" "$TLA_DIR/$dsafe_mod" >/tmp/sl_tlc_detector.log 2>&1
    ec=$?
    dstates="$(grep -oE '[0-9,]+ distinct states found' /tmp/sl_tlc_detector.log | tail -1)"
    if grep -qi "is violated" /tmp/sl_tlc_detector.log; then
      fail "TLA+ detector SAFETY invariant VIOLATED ($dsafe_cfg; see /tmp/sl_tlc_detector.log)"
    elif [[ $ec -eq 0 ]] && grep -qi "Model checking completed" /tmp/sl_tlc_detector.log; then
      pass "TLA+ detector SAFETY exhausted, 0 violations ($dsafe_cfg; ${dstates:-?})"
    elif [[ $ec -eq 124 || $ec -eq 137 ]]; then
      skip "TLA+ detector SAFETY exceeded ${budget}s budget (defense-in-depth; Rocq T-1/T-6 authoritative; doc §14 ✅-exhausted; got ${dstates:-0}). Full run: SL_DETECTOR_SAFETY_BUDGET=0 bash $0"
    else
      fail "TLA+ detector SAFETY failed for another reason (exit $ec; see /tmp/sl_tlc_detector.log)"
    fi
  else
    skip "TLA+ detector SAFETY: $dsafe_cfg / $dsafe_mod not present"
  fi
else
  skip "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo "== [3/3] Deep tiers (fail-soft, run separately) =="
if [[ -x "$REPO_ROOT/scripts/ci/slashing-search-horizon.sh" ]]; then
  skip "Kani / Miri / fuzz / Sage economic-safety tiers: run scripts/ci/slashing-search-horizon.sh (heavier; not part of this fast gate)"
else
  skip "no scripts/ci/slashing-search-horizon.sh"
fi

echo
if [[ $rc -eq 0 ]]; then
  printf '\033[32m== slashing verification: ALL GATES OK ==\033[0m\n'
else
  printf '\033[31m== slashing verification: FAILURES ABOVE ==\033[0m\n'
fi
exit $rc
