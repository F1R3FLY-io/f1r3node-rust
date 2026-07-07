#!/usr/bin/env bash
# scripts/check-merge-algebra-ALL.sh
#
# LOCAL-ONLY verification gate for the F1R3FLY block-merger DETERMINISM ("merge
# algebra"). Structural clone of scripts/check-fork-choice-ALL.sh. Runs every
# formal layer for the feature under a bounded memory envelope:
#
#   1. Rocq  (AUTHORITATIVE) — builds formal/rocq/merge_algebra and asserts the
#      four capstones (merge_algebra_{keeporder,netting,conflict,split}_correct)
#      are axiom-free ("Closed under the global context"), then re-checks them
#      with the TRUSTED kernel (coqchk). Any failure here fails the gate.
#      SKIPPED only while no theories/*.v exist yet (scaffold phase).
#   2. Z3    (fail-soft) — keep_one_total_order (the 5-key strict total order +
#      argmax uniqueness + the "without key 5, distinct chains tie" fork probe)
#      and channel_netting_monoid (max-union commutes but is NON-associative =
#      Finding A; the sum-union FIX is associative). SKIPPED if no python3 z3.
#   3. Rust  (fail-soft) — the modality companions: rspace_plus_plus merger tests
#      (GAP-1 combine commutativity + the Finding-A non-associativity pin +
#      the sum-union order-independence; GAP-3 removed-predicate subsumption +
#      the §3c produce-only over-fill ESCAPE of the retained detector;
#      P2 split-hides-no-conflict), the casper merging P3 proptest
#      (cmp is a strict total order whose Equal-class is the injective key), and
#      the rholang merging::rholang_merging_logic §3c guard tests
#      (produce-only over-fill rejected IFF the cell would hold > 1 value).
#      SKIPPED if cargo is absent.
#   4. Diagrams (fail-soft) — renders the dossier's PlantUML set and asserts a
#      populated SVG (closing </svg>) with no stderr. SKIPPED if plantuml is
#      absent OR docs/theory/merge-algebra/diagrams/*.puml do not exist yet (the
#      dossier is built separately).
#
# POLICY: this script is for LOCAL use only. Do NOT wire it (or any Rocq/TLA+/Wolfram
# step) into .github/workflows/* — an earlier formal-CI workflow was deliberately
# removed. See docs/theory/merge-algebra/merge-algebra-verification.md.
#
# Env knobs:
#   ROCQ_MEMMAX=16G   systemd MemoryMax for the Rocq build (default 16G)
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROCQ_DIR="$REPO_ROOT/formal/rocq/merge_algebra"
Z3_DIR="$REPO_ROOT/formal/z3/merge_algebra"
DIAG_DIR="$REPO_ROOT/docs/theory/merge-algebra/diagrams"
ROCQ_MEMMAX="${ROCQ_MEMMAX:-16G}"

rc=0
pass() { printf '  \033[32mPASS\033[0m %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; rc=1; }
skip() { printf '  \033[33mSKIP\033[0m %s\n' "$1"; }

# --- memory-capped runner (systemd-run scope, else bare) ------------------------
capped() {
  if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope true >/dev/null 2>&1; then
    systemd-run --user --scope -p "MemoryMax=$ROCQ_MEMMAX" -p CPUQuota=1800% -p TasksMax=200 "$@"
  else
    "$@"
  fi
}

echo "== [1/4] Rocq (authoritative) =="
if ! ls "$ROCQ_DIR"/theories/*.v >/dev/null 2>&1; then
  skip "no Rocq theories yet (scaffold phase) — becomes AUTHORITATIVE once modules land"
elif command -v coqc >/dev/null 2>&1 || [[ -x "$HOME/.opam/default/bin/coqc" ]]; then
  # shellcheck disable=SC1090
  eval "$(opam env 2>/dev/null)" 2>/dev/null || true
  ( cd "$ROCQ_DIR" && coq_makefile -f _CoqProject -o Makefile ) >/dev/null 2>&1
  if capped make -C "$ROCQ_DIR" -j1 >/tmp/ma_rocq_build.log 2>&1; then
    pass "Rocq build (KeepOneOrder, ChannelNetting, ConflictSoundness, EventLogSplit, MainTheorem)"
    tmpd=$(mktemp -d)
    chk="$tmpd/GateCheck.v"
    # The 4 capstones must all be axiom-free (Closed under the global context).
    cat > "$chk" <<'EOF'
From MergeAlgebra Require Import MainTheorem.
Print Assumptions merge_algebra_keeporder_correct.
Print Assumptions merge_algebra_netting_correct.
Print Assumptions merge_algebra_conflict_correct.
Print Assumptions merge_algebra_split_correct.
EOF
    out=$(coqc -Q "$ROCQ_DIR/theories" MergeAlgebra "$chk" 2>&1)
    rm -rf "$tmpd"
    n_closed=$(grep -c "Closed under the global context" <<<"$out")
    if [[ "$n_closed" == "4" ]]; then
      pass "all 4 capstones axiom-free (keeporder, netting, conflict, split)"
    else
      fail "capstones NOT all axiom-free ($n_closed/4 Closed):"; echo "$out" | sed 's/^/      /'
    fi
    # Independent kernel re-check (coqchk) — the TRUSTED kernel re-verifies every
    # capstone + dependency `.vo`, not just the elaborator's Print Assumptions.
    if capped coqchk -Q "$ROCQ_DIR/theories" MergeAlgebra MergeAlgebra.MainTheorem \
         >/tmp/ma_coqchk.log 2>&1 && grep -q "Modules were successfully checked" /tmp/ma_coqchk.log; then
      pass "coqchk kernel re-check (MainTheorem + all deps)"
    else
      fail "coqchk kernel re-check FAILED (see /tmp/ma_coqchk.log)"; tail -10 /tmp/ma_coqchk.log | sed 's/^/      /'
    fi
  else
    fail "Rocq build failed (see /tmp/ma_rocq_build.log)"; tail -20 /tmp/ma_rocq_build.log | sed 's/^/      /'
  fi
else
  fail "coqc not found — Rocq is authoritative, cannot skip"
fi

echo "== [2/4] Z3 cross-witness (fail-soft) =="
if ! ls "$Z3_DIR"/*.py >/dev/null 2>&1; then
  skip "no Z3 scripts yet"
elif command -v python3 >/dev/null 2>&1 && python3 -c 'import z3' >/dev/null 2>&1; then
  if python3 "$Z3_DIR/keep_one_total_order.py" >/tmp/ma_z3_ko.log 2>&1; then
    pass "Z3 keep-one 5-key total order + argmax uniqueness (no fork; key-5 probe)"
  else
    fail "Z3 keep_one_total_order.py failed (see /tmp/ma_z3_ko.log)"
  fi
  if python3 "$Z3_DIR/channel_netting_monoid.py" >/tmp/ma_z3_cn.log 2>&1; then
    pass "Z3 channel-netting monoid (max-union NON-assoc / sum-union assoc)"
  else
    fail "Z3 channel_netting_monoid.py failed (see /tmp/ma_z3_cn.log)"
  fi
else
  skip "no python3 z3 module"
fi

echo "== [3/4] Rust modality companions (fail-soft) =="
if command -v cargo >/dev/null 2>&1; then
  if cargo test -p rspace_plus_plus --lib merger:: >/tmp/ma_rust_rspace.log 2>&1; then
    pass "Rust rspace_plus_plus merger:: (GAP-1 combine, Finding-A pin ignored, GAP-3 subsumption + §3c produce-only escape, P2)"
  else
    fail "Rust rspace_plus_plus merger:: tests failed (see /tmp/ma_rust_rspace.log)"; tail -20 /tmp/ma_rust_rspace.log | sed 's/^/      /'
  fi
  if cargo test -p casper --lib merging:: >/tmp/ma_rust_casper.log 2>&1; then
    pass "Rust casper merging:: (P3 cmp strict-total-order, Equal-class = injective key)"
  else
    fail "Rust casper merging:: tests failed (see /tmp/ma_rust_casper.log)"; tail -20 /tmp/ma_rust_casper.log | sed 's/^/      /'
  fi
  # §3c single-value-cell guard (RCA-asi-devnet-finality-halt): the Rocq
  # ConflictSoundness.v Section Overfill companion —
  # check_single_value_cell_not_overfilled rejects a merge IFF the post-merge
  # single-value NUMBER cell would hold > 1 value (produce-only over-fill), plus
  # Kevin's §3c unit witnesses (produce-only reject / RMW allow / registry exempt).
  if cargo test -p rholang --lib merging::rholang_merging_logic >/tmp/ma_rust_rholang.log 2>&1; then
    pass "Rust rholang merging::rholang_merging_logic (§3c guard: produce-only over-fill rejected IFF result_len>1)"
  else
    fail "Rust rholang merging::rholang_merging_logic tests failed (see /tmp/ma_rust_rholang.log)"; tail -20 /tmp/ma_rust_rholang.log | sed 's/^/      /'
  fi
  # T-RECOMPUTE — the enforcement seam that makes merge-determinism consequential:
  # validate_block_checkpoint recomputes the parents' post-state and REJECTS a block
  # whose recorded pre-state hash (:259 -> Right(None), no replay) or rejected-deploy set
  # (:269 -> InvalidRejectedDeploy) disagrees with the recompute. Integration test in the
  # `mod` binary (casper/tests/batch2/validate_test.rs).
  if cargo test -p casper --test mod -- batch2::validate_test::validate_block_checkpoint_recompute >/tmp/ma_rust_recompute.log 2>&1 \
       && grep -q "test result: ok" /tmp/ma_rust_recompute.log; then
    pass "Rust casper T-RECOMPUTE (recompute-vs-recorded reject seam: pre-state + rejected-deploy)"
  else
    fail "Rust casper T-RECOMPUTE test failed (see /tmp/ma_rust_recompute.log)"; tail -20 /tmp/ma_rust_recompute.log | sed 's/^/      /'
  fi
else
  skip "no cargo on PATH"
fi

echo "== [4/4] PlantUML diagrams (fail-soft) =="
if command -v plantuml >/dev/null 2>&1; then
  n_puml=$(find "$DIAG_DIR" -name '*.puml' 2>/dev/null | wc -l)
  if [[ "$n_puml" -gt 0 ]]; then
    diag_ok=1
    for puml in "$DIAG_DIR"/*.puml; do
      svg="${puml%.puml}.svg"
      derr=$(plantuml -tsvg "$puml" 2>&1)
      if [[ -n "$derr" ]] || [[ ! -s "$svg" ]] || ! grep -q "</svg>" "$svg" 2>/dev/null; then
        fail "diagram $(basename "$puml") did not render clean"
        [[ -n "$derr" ]] && echo "$derr" | sed 's/^/      /'
        diag_ok=0
      fi
    done
    [[ "$diag_ok" == "1" ]] && pass "all $n_puml PlantUML diagrams render clean (populated SVG, no stderr)"
  else
    skip "no .puml sources in docs/theory/merge-algebra/diagrams (dossier built separately)"
  fi
else
  skip "no plantuml on PATH"
fi

echo
if [[ $rc -eq 0 ]]; then
  printf '\033[32m== merge-algebra verification: ALL GATES OK ==\033[0m\n'
else
  printf '\033[31m== merge-algebra verification: FAILURES ABOVE ==\033[0m\n'
fi
exit $rc
