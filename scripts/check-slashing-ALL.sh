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
#   2. TLA+  — TLC (BOUNDED, explicit-state) on the slashing safety cfgs
#      (equivocation detector, concurrent tracker, slash flow) + the pre-fix
#      counterexample cfgs (must reproduce their violation). Exhaustive only at the
#      cfg bounds (detector eager: 2v/2s/2b). SKIPPED if no TLC jar.
#   3. Apalache — UNBOUNDED symbolic (SMT) inductive-invariant check on the
#      EAGER equivocation detector, COMPLEMENTING the bounded TLC run: proves IndInv ==
#      TypeOK /\ the five state-dependent safety invariants (Inv_DetectionSound,
#      Inv_TaxonomyCorrect, Inv_NeglectedHasDetectableView, Inv_RecordHasWitness,
#      Inv_LivenessAsSafety) is INDUCTIVE (holds on ALL reachable states, NO finite
#      horizon) via BASE (Init |= IndInv) + STEP (Next preserves IndInv) on the
#      type-annotated wrapper EquivocationDetectorEager_apalache.tla. Runs at 3v/3s/3b,
#      strictly beyond the gate's 2v/2s/2b TLC run on every dimension (the inductive
#      check is O(1) in trajectory length, so it scales where TLC's explicit 3-validator
#      enumeration would blow up). Apalache ignores fairness, so the EAGER model
#      (liveness-as-safety) is the right target.
#   4. Deep tiers — points at scripts/ci/slashing-search-horizon.sh for
#      the Kani / Miri / fuzz / Sage economic-safety tiers (heavier; run separately).
#
# POLICY: LOCAL-ONLY. Do NOT wire this (or any Rocq/TLA+/Apalache step) into
# .github/workflows/* — an earlier formal-CI workflow was deliberately removed.
#
# Env knobs: ROCQ_MEMMAX=16G (systemd MemoryMax for the Rocq build).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SL_DIR="$REPO_ROOT/formal/rocq/slashing"
TLA_DIR="$REPO_ROOT/formal/tlaplus/slashing"
ROCQ_MEMMAX="${ROCQ_MEMMAX:-16G}"
LOG_DIR="$REPO_ROOT/target/verification/slashing"
mkdir -p "$LOG_DIR"

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

echo "== [1/5] Rocq (authoritative) =="
if command -v coqc >/dev/null 2>&1 || [[ -x "$HOME/.opam/default/bin/coqc" ]]; then
  # shellcheck disable=SC1090
  eval "$(opam env 2>/dev/null)" 2>/dev/null || true
  ( cd "$SL_DIR" && coq_makefile -f _CoqProject -o Makefile ) >/dev/null 2>&1
  if capped make -C "$SL_DIR" -j1 >"$LOG_DIR/sl_rocq_build.log" 2>&1; then
    pass "Rocq build (slashing: $(grep -c '^theories/' "$SL_DIR/_CoqProject") modules)"
    # Every `main_*` capstone in MainTheorem.v must be axiom-free. The list is
    # generated from the source so the gate never drifts from the theorem count.
    tmpd=$(mktemp -d "$LOG_DIR/gate-check.XXXXXX"); chk="$tmpd/GateCheck.v"
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
         >"$LOG_DIR/sl_coqchk.log" 2>&1 && grep -q "Modules were successfully checked" "$LOG_DIR/sl_coqchk.log"; then
      pass "coqchk kernel re-check (MainTheorem + all deps)"
    else
      fail "coqchk kernel re-check FAILED (see $LOG_DIR/sl_coqchk.log)"; tail -10 "$LOG_DIR/sl_coqchk.log" | sed 's/^/      /'
    fi
  else
    fail "Rocq build failed (see $LOG_DIR/sl_rocq_build.log)"; tail -20 "$LOG_DIR/sl_rocq_build.log" | sed 's/^/      /'
  fi
else
  fail "coqc not found — Rocq is authoritative, cannot skip"
fi

echo "== [2/5] TLA+ TLC bounded =="
TLC_JAR="${TLC_JAR:-/usr/share/java/tla2tools.jar}"
if [[ -f "$TLC_JAR" ]] || command -v tlc >/dev/null 2>&1; then
  # Match the bounded envelope used by check-tla-invariants.sh. SlashFlow's
  # universal safety result is discharged below by TLAPS; the finite
  # MC_SlashFlowRedeem instance is its exhaustive executable cross-check.
  : "${TLC_HEAP:=16g}"
  : "${TLC_RSS:=24G}"
  : "${TLC_WORKERS:=8}"
  # shellcheck disable=SC1091
  source "$REPO_ROOT/scripts/lib/tlc-run.sh"
  # Fast safety cfgs (seconds; exhaustive at their bounds) — MUST pass. Each
  # MC_*.cfg substitutes MC_-prefixed identifiers (MC_Validators, …) defined in
  # the MC_*.tla WRAPPER (`EXTENDS <base>, TLC`), so TLC runs against the wrapper,
  # not the base. The 1v detector-liveness model checks ALL 20 detector safety
  # invariants PLUS the Live_DetectionComplete temporal property on a tiny
  # (69-state) space. The local-safety model checks the complete 2s/2b state
  # machine for one validator. DetectorProduct.v proves that every pointwise
  # invariant preserved by this local transition lifts through arbitrary
  # interleavings of any validator product. The eager model independently checks
  # multi-validator behavior and folds immediate production detection into
  # Inv_LivenessAsSafety.
  # FV audit #6 (unbonded-window record pollution fork): the POST-FIX pollution
  # model (EnableStampWitness=FALSE) must PASS Inv_NoStampAgainstUnbonded AND
  # Inv_NeglectNotFromUnbondedPollution over its bond-toggling state space
  # (exhaustive at 2v/1s/2b). Its PRE-FIX twin is checked in the VIOLATE block
  # below. Both share MC_EquivocationDetector_unbonded_pollution.tla.
  declare -A SAFE=(
    [slash_authority]="MC_AuthorizedSlashFlow.cfg:MC_AuthorizedSlashFlow.tla"
    [slash_evidence_dependency]="MC_SlashEvidenceDependency.cfg:MC_SlashEvidenceDependency.tla"
    [detector_local_safety]="MC_EquivocationDetector_safety.cfg:MC_EquivocationDetector_local_safety.tla"
    [detector_liveness]="MC_EquivocationDetector_liveness.cfg:MC_EquivocationDetector_liveness.tla"
    [detector_eager]="MC_EquivocationDetectorEager.cfg:MC_EquivocationDetectorEager.tla"
    [tracker]="MC_ConcurrentTracker.cfg:MC_ConcurrentTracker.tla"
    [slashflow_redeem]="MC_SlashFlowRedeem.cfg:MC_SlashFlowRedeem.tla"
    [unbonded_pollution]="MC_EquivocationDetector_unbonded_pollution.cfg:MC_EquivocationDetector_unbonded_pollution.tla"
  )
  for k in "${!SAFE[@]}"; do
    cfg="${SAFE[$k]%%:*}"; mod="${SAFE[$k]##*:}"
    if [[ -f "$TLA_DIR/$cfg" && -f "$TLA_DIR/$mod" ]]; then
      safe_meta="$(tlc_metadir "sl_$k")"
      rm -rf "${safe_meta:?}/"* 2>/dev/null || true
      if tlc_run "$safe_meta" "$TLA_DIR/$cfg" "$TLA_DIR/$mod" >"$LOG_DIR/sl_tlc_$k.log" 2>&1; then
        pass "TLA+ slashing $k ($cfg)"
      else
        fail "TLA+ slashing $k did NOT pass ($cfg; see $LOG_DIR/sl_tlc_$k.log)"
      fi
    else
      skip "TLA+ slashing $k: $cfg / $mod not present"
    fi
  done
  SLASHING_TLAPS_PATH="$HOME/.local/tlaps/bin:$HOME/.local/bin:/usr/bin:$PATH"
  if PATH="$SLASHING_TLAPS_PATH" command -v tlapm >/dev/null 2>&1; then
    if ( cd "$TLA_DIR" && PATH="$SLASHING_TLAPS_PATH" tlapm SlashFlowProofs.tla ) >"$LOG_DIR/sl_tlaps_slashflow.log" 2>&1 \
         && grep -Eq "All [1-9][0-9]* obligations? proved\." "$LOG_DIR/sl_tlaps_slashflow.log" \
         && ! grep -Eiq "obligation.*(failed|omitted)|[1-9][0-9]* (failed|omitted)" "$LOG_DIR/sl_tlaps_slashflow.log"; then
      pass "TLAPS SlashFlow inductive proof for all well-typed parameter values"
    else
      fail "TLAPS SlashFlow proof failed (see $LOG_DIR/sl_tlaps_slashflow.log)"; tail -20 "$LOG_DIR/sl_tlaps_slashflow.log" | sed 's/^/      /'
    fi
  else
    fail "tlapm not found — universal SlashFlow proof cannot be skipped"
  fi
  # Pre-fix counterexample — must reproduce its violation (inverted sense).
  if [[ -f "$TLA_DIR/MC_ConcurrentTracker_pre_fix.cfg" ]]; then
    if tlc_run "$(tlc_metadir sl_tracker_pre)" "$TLA_DIR/MC_ConcurrentTracker_pre_fix.cfg" "$TLA_DIR/MC_ConcurrentTracker.tla" >"$LOG_DIR/sl_tlc_tracker_pre.log" 2>&1; then
      fail "TLA+ concurrent-tracker pre-fix should VIOLATE its invariant but passed"
    elif grep -qi "is violated" "$LOG_DIR/sl_tlc_tracker_pre.log"; then
      pass "TLA+ concurrent-tracker pre-fix reproduces the race counterexample"
    else
      fail "TLA+ concurrent-tracker pre-fix failed for the wrong reason (see $LOG_DIR/sl_tlc_tracker_pre.log)"
    fi
  fi
  if [[ -f "$TLA_DIR/MC_AuthorizedSlashFlow_receiver_ambient_unsafe.cfg" ]]; then
    authority_unsafe_meta="$(tlc_metadir sl_authority_unsafe)"
    rm -rf "${authority_unsafe_meta:?}/"* 2>/dev/null || true
    if tlc_run "$authority_unsafe_meta" \
         "$TLA_DIR/MC_AuthorizedSlashFlow_receiver_ambient_unsafe.cfg" \
         "$TLA_DIR/MC_AuthorizedSlashFlow.tla" \
         >"$LOG_DIR/sl_tlc_authority_unsafe.log" 2>&1; then
      fail "TLA+ receiver-ambient authority control should VIOLATE proposer/receiver parity but passed"
    elif grep -qi "is violated" "$LOG_DIR/sl_tlc_authority_unsafe.log"; then
      pass "TLA+ receiver-ambient authority control reproduces proposer/receiver disagreement"
    else
      fail "TLA+ receiver-ambient authority control failed for the wrong reason (see $LOG_DIR/sl_tlc_authority_unsafe.log)"
    fi
  fi
  if [[ -f "$TLA_DIR/MC_SlashEvidenceDependency_omitted_unsafe.cfg" ]]; then
    evidence_dependency_unsafe_meta="$(tlc_metadir sl_evidence_dependency_unsafe)"
    rm -rf "${evidence_dependency_unsafe_meta:?}/"* 2>/dev/null || true
    if tlc_run "$evidence_dependency_unsafe_meta" \
         "$TLA_DIR/MC_SlashEvidenceDependency_omitted_unsafe.cfg" \
         "$TLA_DIR/MC_SlashEvidenceDependency.tla" \
         >"$LOG_DIR/sl_tlc_evidence_dependency_unsafe.log" 2>&1; then
      fail "TLA+ omitted slash-evidence dependency control should VIOLATE local-absence safety but passed"
    elif grep -qi "is violated" "$LOG_DIR/sl_tlc_evidence_dependency_unsafe.log"; then
      pass "TLA+ omitted slash-evidence dependency control reproduces receiver-local rejection"
    else
      fail "TLA+ omitted slash-evidence dependency control failed for the wrong reason (see $LOG_DIR/sl_tlc_evidence_dependency_unsafe.log)"
    fi
  fi
  if [[ -f "$TLA_DIR/MC_SlashEvidenceDependency_tracker_unsafe.cfg" ]]; then
    evidence_tracker_unsafe_meta="$(tlc_metadir sl_evidence_tracker_unsafe)"
    rm -rf "${evidence_tracker_unsafe_meta:?}/"* 2>/dev/null || true
    if tlc_run "$evidence_tracker_unsafe_meta" \
         "$TLA_DIR/MC_SlashEvidenceDependency_tracker_unsafe.cfg" \
         "$TLA_DIR/MC_SlashEvidenceDependency.tla" \
         >"$LOG_DIR/sl_tlc_evidence_tracker_unsafe.log" 2>&1; then
      fail "TLA+ tracker-only slash-evidence control should VIOLATE local-absence safety but passed"
    elif grep -qi "is violated" "$LOG_DIR/sl_tlc_evidence_tracker_unsafe.log"; then
      pass "TLA+ tracker-only slash-evidence control reproduces authorization after dependency bypass"
    else
      fail "TLA+ tracker-only slash-evidence control failed for the wrong reason (see $LOG_DIR/sl_tlc_evidence_tracker_unsafe.log)"
    fi
  fi
  # FV audit #6 pre-fix counterexample — the PRE-FIX pollution model
  # (EnableStampWitness=TRUE) must REPRODUCE a violation of
  # Inv_NoStampAgainstUnbonded (StampWitness stamping an unbonded offender's
  # record — the fork's root cause). Same wrapper as the post-fix SAFE run above.
  if [[ -f "$TLA_DIR/MC_EquivocationDetector_unbonded_pollution_pre_fix.cfg" ]]; then
    if tlc_run "$(tlc_metadir sl_unbonded_pre)" "$TLA_DIR/MC_EquivocationDetector_unbonded_pollution_pre_fix.cfg" "$TLA_DIR/MC_EquivocationDetector_unbonded_pollution.tla" >"$LOG_DIR/sl_tlc_unbonded_pre.log" 2>&1; then
      fail "TLA+ unbonded-pollution pre-fix should VIOLATE Inv_NoStampAgainstUnbonded but passed"
    elif grep -qi "is violated" "$LOG_DIR/sl_tlc_unbonded_pre.log"; then
      pass "TLA+ unbonded-pollution pre-fix reproduces the record-pollution counterexample (Inv_NoStampAgainstUnbonded)"
    else
      fail "TLA+ unbonded-pollution pre-fix failed for the wrong reason (see $LOG_DIR/sl_tlc_unbonded_pre.log)"
    fi
  fi
else
  fail "no TLC jar (\$TLC_JAR) or 'tlc' on PATH"
fi

echo "== [3/5] Apalache unbounded symbolic =="
# UNBOUNDED (horizon-free) inductive-invariant check on the EAGER equivocation detector,
# COMPLEMENTING the bounded TLC run above. On the type-annotated wrapper
# EquivocationDetectorEager_apalache.tla (the TLC base module + MC_*.cfg are left intact),
# Apalache proves IndInv == TypeOK /\ Inv_DetectionSound /\ Inv_TaxonomyCorrect /\
# Inv_NeglectedHasDetectableView /\ Inv_RecordHasWitness /\ Inv_LivenessAsSafety is
# INDUCTIVE — holds on ALL reachable states, NO finite horizon:
#   BASE: --init=Init  --inv=IndInv --length=0   (every Init state |= IndInv)
#   STEP: --init=IndInv --inv=IndInv --length=1   (Next preserves IndInv)
# Runs at 3v/3s/3b, strictly beyond the gate's 2v/2s/2b TLC run on every dimension.
# Apalache ignores fairness, so the EAGER model (liveness folded into the
# Inv_LivenessAsSafety safety invariant) is the correct target. PASSES iff BOTH report
# NoError. Runs
# under the shared memory cap (`capped`); SMT scratch lands on-disk under target/ (NVMe).
APALACHE_WRAP="$TLA_DIR/EquivocationDetectorEager_apalache.tla"
if ! command -v apalache-mc >/dev/null 2>&1; then
  fail "no apalache-mc on PATH — the unbounded symbolic IndInv gate is required"
elif [[ ! -f "$APALACHE_WRAP" ]]; then
  fail "no EquivocationDetectorEager_apalache.tla wrapper present"
else
  aout="$REPO_ROOT/target/apalache-slashing"; rm -rf "$aout" 2>/dev/null || true; mkdir -p "$aout"
  a_base=0; a_step=0
  if capped apalache-mc check --init=Init --inv=IndInv --length=0 --cinit=CInit \
       --out-dir="$aout" "$APALACHE_WRAP" >"$LOG_DIR/sl_apalache_base.log" 2>&1 \
       && grep -qE "The outcome is: NoError|No error found" "$LOG_DIR/sl_apalache_base.log"; then
    a_base=1
  fi
  if capped apalache-mc check --init=IndInv --inv=IndInv --length=1 --cinit=CInit \
       --out-dir="$aout" "$APALACHE_WRAP" >"$LOG_DIR/sl_apalache_step.log" 2>&1 \
       && grep -qE "The outcome is: NoError|No error found" "$LOG_DIR/sl_apalache_step.log"; then
    a_step=1
  fi
  if [[ "$a_base" == "1" && "$a_step" == "1" ]]; then
    pass "Apalache UNBOUNDED IndInv inductive — BASE+STEP clean at 3v/3s/3b: detector safety + Inv_LivenessAsSafety hold on ALL reachable states (horizon-free; strictly beyond TLC's 2v/2s/2b)"
  else
    [[ "$a_base" == "1" ]] || fail "Apalache BASE (Init |= IndInv) did NOT report NoError (see $LOG_DIR/sl_apalache_base.log)"
    [[ "$a_step" == "1" ]] || fail "Apalache STEP (Next preserves IndInv) did NOT report NoError (see $LOG_DIR/sl_apalache_step.log)"
  fi
fi

echo "== [4/5] Rust slashing lib + authorization tests =="
# The slashing seams realized in casper/src that the Rocq theorems pin:
#   - T-Slash seed-wiring (MainTheorem.v:302): build_slash_deploy derives the SlashDeploy's
#     initial_rand from the OFFENDER's invalid_block_hash, so replay recomputes it — plus the
#     T-9.8 candidate filtering (current evidence + canonical positive bond). Lib unit tests in
#     blocks::proposer::block_creator.
#   - Received-slash authorization (BugFixSlashAuthorization.v): the §9.8 receive rules incl.
#     the exact canonical merged-pre-state bond (T-9.13 canonical-zero rejects /
#     canonical-positive authorizes) and proposer/receiver parity. Integration tests in
#     casper/tests/slashing/slash_authorization_regressions.rs.
if command -v cargo >/dev/null 2>&1; then
  if cargo test -p casper --lib blocks::proposer::block_creator >"$LOG_DIR/sl_rust_bc.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/sl_rust_bc.log"; then
    n_bc=$(grep -oE 'result: ok\. [0-9]+ passed' "$LOG_DIR/sl_rust_bc.log" | grep -oE '[0-9]+' | head -1)
    pass "Rust slashing lib tests (${n_bc:-?} passed: T-Slash seed-wiring + T-9.8 candidate filtering)"
  else
    fail "Rust slashing lib tests failed (see $LOG_DIR/sl_rust_bc.log)"; tail -20 "$LOG_DIR/sl_rust_bc.log" | sed 's/^/      /'
  fi
  if cargo test -p casper --test mod -- slashing::slash_authorization_regressions >"$LOG_DIR/sl_rust_auth.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/sl_rust_auth.log"; then
    n_auth=$(grep -oE 'result: ok\. [0-9]+ passed' "$LOG_DIR/sl_rust_auth.log" | grep -oE '[0-9]+' | head -1)
    pass "Rust slash-authorization regressions (${n_auth:-?} passed: §9.8 seven-rule receive gate incl. T-9.13 parent-bond)"
  else
    fail "Rust slash-authorization regressions failed (see $LOG_DIR/sl_rust_auth.log)"; tail -20 "$LOG_DIR/sl_rust_auth.log" | sed 's/^/      /'
  fi
  if cargo test -p casper --test mod -- util::proto_util_test >"$LOG_DIR/sl_rust_dependencies.log" 2>&1 \
       && grep -qE "test result: ok\. [1-9][0-9]* passed" "$LOG_DIR/sl_rust_dependencies.log"; then
    n_dependencies=$(grep -oE 'result: ok\. [0-9]+ passed' "$LOG_DIR/sl_rust_dependencies.log" | grep -oE '[0-9]+' | head -1)
    pass "Rust slash dependency projection (${n_dependencies:-?} passed: exact set, deduplication, and complete readiness truth table)"
  else
    fail "Rust slash dependency projection failed (see $LOG_DIR/sl_rust_dependencies.log)"; tail -20 "$LOG_DIR/sl_rust_dependencies.log" | sed 's/^/      /'
  fi
  if cargo test -p casper --test mod -- blocks::block_processor_test::slash_evidence_is_fetched_before_block_validation --exact >"$LOG_DIR/sl_rust_dependency_fetch.log" 2>&1 \
       && grep -qE "test result: ok\. 1 passed" "$LOG_DIR/sl_rust_dependency_fetch.log"; then
    pass "Rust slash-evidence fetch-before-validation regression"
  else
    fail "Rust slash-evidence fetch-before-validation regression failed (see $LOG_DIR/sl_rust_dependency_fetch.log)"; tail -20 "$LOG_DIR/sl_rust_dependency_fetch.log" | sed 's/^/      /'
  fi
  if cargo test -p casper --test mod -- blocks::block_processor_test::tracker_witness_alone_does_not_suppress_block_admission --exact >"$LOG_DIR/sl_rust_tracker_admission.log" 2>&1 \
       && grep -qE "test result: ok\. 1 passed" "$LOG_DIR/sl_rust_tracker_admission.log"; then
    pass "Rust tracker-witness/admission separation regression"
  else
    fail "Rust tracker-witness/admission separation regression failed (see $LOG_DIR/sl_rust_tracker_admission.log)"; tail -20 "$LOG_DIR/sl_rust_tracker_admission.log" | sed 's/^/      /'
  fi
else
  fail "no cargo on PATH"
fi

echo "== [5/5] Deep tiers =="
if [[ -x "$REPO_ROOT/scripts/ci/slashing-search-horizon.sh" ]]; then
  pass "deep-tier executable present; exhaustive campaign is a separate required completion gate"
else
  fail "no scripts/ci/slashing-search-horizon.sh"
fi

echo
if [[ $rc -eq 0 ]]; then
  printf '\033[32m== slashing verification: ALL GATES OK ==\033[0m\n'
else
  printf '\033[31m== slashing verification: FAILURES ABOVE ==\033[0m\n'
fi
exit $rc
