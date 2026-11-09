#!/usr/bin/env bash
# scripts/check-reified-rspaces-proofs.sh
#
# Rocq gate for the Reified RSpaces formal development
# (formal/rocq/reified_rspaces). P1 has two modules: EventCorrelation.v
# (H1-H12, H17, H19-H21, and H23 of decision 36) and VolatileSweep.v
# (H13-H16, H18, H22). They include the round-2 rules FR-1', FR-5' and FR-9',
# the round-3 rule FR-10 (prompts/p1-fix-rules.md), Rule E of decision 45
# (H11) and decision 36 (replay reproduces play: H4 and H23). P2 adds
# Encoding.v (12 headlines, and 3 more for plan revision I-20). P4 adds
# Namespace.v (11), RecordEnvelope.v (8, 10 in P5), CounterConflict.v (9, 15 in P5),
# HeadOnlyDisciplines.v (9, 10 of decision 43 and 1 of decision 45),
# SpaceRefs.v (10, and 1 of decision 46) and SpaceFactory.v (8). Decision 42
# adds AgentReadCoverage.v (8), and decision 96 deletes it with the host-work
# metering (decision 89, DR-131). Option P of decision 45 adds
# LegacyCharges.v (2: Q79 and Q80), and decision 90 removes it with Option P
# (DR-134 deletes the legacy meter). P6 step 2 adds Similarity.v (20: S1-S20).
# P6 step 8 adds headline 14 of SpaceFactory.v (vectordb_params_validated) and
# headline 16 of CounterConflict.v (capped_writes_soft_bound, decision 71).
# P6 step 8b adds VectorDbDiscipline.v (5: S21-S25), and step 8d adds
# VectorOps.v (2: S27 and S28). P7 step 2 adds headline 12 of Namespace.v
# (seq_name_iff, SQ-1). P7 step 4 adds TheoryAdmission.v (6: TA1 to TA6),
# headline 4 of Match.v (theory_refusal_is_guard_refusal, TH-6) and
# headline 15 of SpaceFactory.v (theory_gate_validated, TH-4). P8 step 1 adds
# headline 13 of SpaceRefs.v (agent_ref_requires_write_flag, AG-4). P9 step 8
# adds H28 to H30 of EventCorrelation.v (coverage_commit,
# two_block_replay_preserves_roots and two_block_native_replay_preserves_roots,
# amendment A10). P10 step S7 adds Checkpoint.v (8: C1 to C8), the successor
# of the legacy Checkpoint.v, and leg 8, the axiom control. Decision 112 takes
# Encoding.v (15 headlines) out of the gate, and the file stays on disk as the
# frozen evidence of the marker encoding. Decision 112 adds SyntaxFields.v
# (15), the model of the Reified syntax as protobuf fields, so the gate still
# has 28 modules.
#
# The gate copies the formal tree into a scratch directory and builds it
# there. It never builds in the source tree. Legs:
#   1. hygiene  - on a comment-stripped copy of each module: no Axiom,
#                 Parameter, Conjecture, Admitted, admit, Abort, no
#                 Hypothesis or Variable outside a Section, no checker
#                 bypass, and imports from Stdlib only
#   2. standalone - each module compiles alone with `coqc -q -noglob` in a
#                 fresh directory, with no -Q and no -R
#   3. headlines - each module prints "Closed under the global context"
#                 once for each "Print Assumptions" line, and the count per
#                 module is the expected count
#   4. rocqchk  - `rocqchk -o` checks each standalone .vo and all of its
#                 dependencies: Axioms <none>, no type-in-type, no unsafe
#                 (co)fixpoints, no assumed positivity
#   5. project  - `make` with the shipped Makefile and _CoqProject builds the
#                 ReifiedRSpaces library, and rocqchk checks it
#   6. total    - the total number of closed headlines is EXPECTED_CLOSED
#   7. negative - scripts/verify-reified/rocq-negative-controls.py on the
#                 scratch copy: each mutant of a P1 rule (FR-1, FR-1',
#                 FR-5', FR-9', FR-10), of decision 36 (a staging failure,
#                 a skipped observe_comm call, a committed unpaired fire),
#                 of a P4 rule (counter bumps, head-only plans, routing,
#                 factory and envelope checks, the I-20 refresh, the
#                 metering rule of decision 42) and of decision 45 (Rule
#                 E) must fail to compile inside its expected lemma, after
#                 the pristine modules compile. Changed by decision 90: the
#                 controls of the legacy charges of Option P (the tail, the
#                 carry, the place of the pre-charge, the replay premises)
#                 go with LegacyCharges.v. Changed by decision 112: the
#                 controls of the I-20 refresh go with Encoding.v, and the
#                 controls of the fields of SyntaxFields.v join them
#   8. axiom    - the fixture formal/rocq/reified_rspaces/controls/
#                 AxiomControl.v states one Axiom on purpose: the hygiene
#                 check must report an axiom declaration, and rocqchk must
#                 list the axiom. So the two checks that keep the theories
#                 axiom-free can fail.
#
# A missing tool is a FAIL, never a SKIP.
#
# Environment:
# Changed by decision 103 (decision 118): the default is 293. The text before:
# #   RRS_EXPECTED_CLOSED  total closed headlines (default 300: 23 for P1 with
#   RRS_EXPECTED_CLOSED  total closed headlines (default 293: 23 for P1 with
#                        H23 of decision 36, the 15 headlines of Encoding.v
#                        for P2 and I-20, 67 for the six P4 modules with
#                        the native headline of decision 46 in SpaceRefs.v,
#                        the 10 headlines of decision 43 and the headline
#                        of decision 45 in HeadOnlyDisciplines.v, 8 for
#                        AgentReadCoverage.v of decision 42, 2 for
#                        LegacyCharges.v of decision 45, Option P, and 62
#                        for P5 wave W2: the query headline of SpaceRefs.v,
#                        5 family headlines of SpaceFactory.v, the engine
#                        headline of AgentReadCoverage.v, and the modules
#                        CellDiscipline (3), DataCollection (7),
#                        ContinuationCollection (5), Match (3),
#                        GenericRSpace (5), OuterStorage (8), VectorStore
#                        (3), HashMapStore (2), PathMapPrefix (6) and
#                        StackDiscipline (13), and 32 for the second wave
#                        of W2: the F23 headline and the crossed stacks
#                        of CounterConflict.v, SetDiscipline (8),
#                        PriorityQueue (20) and the 2 PQ slot headlines
#                        of RecordEnvelope.v, and 32 for P5 wave W3: the
#                        11 PathMap engine headlines of PathMapPrefix.v,
#                        the 17 headlines of JoinCascade.v, the v2 join
#                        record of RecordEnvelope.v, and the join race and
#                        the two set merges (F24, F25) of CounterConflict.v,
#                        and 1 for the PathMap source of W3 Phase 2: the
#                        token race pathmap_token_race of
#                        CounterConflict.v, 4 for decision 47: H24 to
#                        H27 of EventCorrelation.v, 20 for P6 step 2:
#                        S1 to S20 of Similarity.v, and 2 for P6 step 8:
#                        vectordb_params_validated of SpaceFactory.v and
#                        capped_writes_soft_bound of CounterConflict.v,
#                        5 for P6 step 8b: the headlines of
#                        VectorDbDiscipline.v, 2 for P6 step 8d:
#                        the headlines of VectorOps.v, 2 for P7 step 1:
#                        sweep_complete and dead_ephemeral_dangling of
#                        VolatileSweep.v, 1 for P7 step 2:
#                        seq_name_iff of Namespace.v, 1 for P7 step
#                        3: hashset_unit_rules of HashMapStore.v, and 8
#                        for P7 step 4: the 6 headlines of
#                        TheoryAdmission.v, theory_refusal_is_guard_refusal
#                        of Match.v and theory_gate_validated of
#                        SpaceFactory.v, 1 for P8 step 1:
#                        agent_ref_requires_write_flag of SpaceRefs.v, and
#                        1 for P8 step 2: r7_agent_reply_bundle_exact of
#                        SpaceRefs.v, and 6 for P8 step 3: the headlines
#                        of AgentTranslation.v, and 2 for P8 step 4:
#                        tag_untag_id of SpaceRefs.v and
#                        resume_binds_innermost of AgentTranslation.v,
#                        and 3 for P9 step 8: H28 to H30 of
#                        EventCorrelation.v, and 8 for P10 step S7: C1
#                        to C8 of Checkpoint.v). Changed by decision 90:
#                        the default is 306, because the 2 headlines of
#                        LegacyCharges.v go with Option P. Changed by
#                        decisions 89 and 96: the default is 296,
#                        because the 9 headlines of AgentReadCoverage.v
#                        and H27 of EventCorrelation.v go with the
#                        host-work metering (DR-131). Changed by
#                        decision 96 (decision 89): the default is 290.
#                        The six metering headlines of DataCollection.v,
#                        OuterStorage.v, PathMapPrefix.v,
#                        PriorityQueue.v, SetDiscipline.v and
#                        StackDiscipline.v go with the host-work
#                        metering (DR-131). Changed by decision 103
#                        (decision 118): the default is 293, because
#                        JoinCascade.v adds join_incoming_held,
#                        join_nonpersistent_bound_once and
#                        cascade_restores_inv_pers. Decision 112 keeps the
#                        default at 293, because SyntaxFields.v (15) takes
#                        the place of Encoding.v (15). The sum of
#                        EXPECTED_PER_MODULE below is the default.
#   RRS_SANDBOX=1        the job runs inside the verifier scope: no
#                        systemd-run, logs under $RRS_SCRATCH/logs
#   RRS_SCRATCH          scratch root (default <repo>/target/verification/
#                        reified-rspaces/scratch)
#   RRS_MEMORY_MAX       cgroup memory ceiling (default 6G)
#   RRS_CPU_QUOTA        cgroup CPU quota (default 400%)
#   RRS_ROCQ_SRC         the Rocq tree to check (default formal/rocq/
#                        reified_rspaces); negative controls use a copy
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${RRS_ROCQ_SRC:-$ROOT/formal/rocq/reified_rspaces}"
SCRATCH_ROOT="${RRS_SCRATCH:-$ROOT/target/verification/reified-rspaces/scratch}"
if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
  LOG_DIR="$SCRATCH_ROOT/logs/proofs"
else
  LOG_DIR="$ROOT/target/verification/reified-rspaces/logs/proofs"
fi
# Changed by decision 90: LegacyCharges.v (2 headlines) is removed with Option P.
# EXPECTED_CLOSED="${RRS_EXPECTED_CLOSED:-308}"
# Changed by decisions 89 and 96: AgentReadCoverage.v (9) and H27 go with the host-work metering.
# Changed by decision 96 (decision 89): six P5 metering headlines go with the host-work metering.
# Changed by decision 103 (decision 118): JoinCascade.v adds join_incoming_held,
# join_nonpersistent_bound_once and cascade_restores_inv_pers.
# Decision 112 keeps 293: SyntaxFields.v (15) replaces Encoding.v (15), so 293 - 15 + 15 = 293.
EXPECTED_CLOSED="${RRS_EXPECTED_CLOSED:-293}"
RRS_MEMORY_MAX="${RRS_MEMORY_MAX:-6G}"
RRS_CPU_QUOTA="${RRS_CPU_QUOTA:-400%}"

# The headline count of each module. A module in _CoqProject without an
# entry here fails the gate.
declare -A EXPECTED_PER_MODULE=(
  # Changed by decision 89 (DR-131, step M2): H27 is disabled, because no view load is metered.
  # [EventCorrelation]=24
  [EventCorrelation]=23
  [VolatileSweep]=8
  # Retired from the gate by decision 112: SyntaxFields.v models the fields, and the file stays on disk.
  # [Encoding]=15
  [SyntaxFields]=15
  [Namespace]=12
  [RecordEnvelope]=11
  [CounterConflict]=16
  [HeadOnlyDisciplines]=20
  [SpaceRefs]=15
  [SpaceFactory]=15
  # Removed by decision 96 (decision 89): AgentReadCoverage.v is deleted with the host-work metering.
  # [AgentReadCoverage]=9
  # Removed by decision 90 (decision 69): Option P is gone with the legacy meter (DR-134).
  # [LegacyCharges]=2
  [CellDiscipline]=3
  # Changed by decision 96 (decision 89): scan_trace_covered goes with the host-work metering.
  # [DataCollection]=7
  [DataCollection]=6
  [ContinuationCollection]=5
  [Match]=4
  [GenericRSpace]=5
  # Changed by decision 96 (decision 89): alloc_trace_covered goes with the host-work metering.
  # [OuterStorage]=8
  [OuterStorage]=7
  [VectorStore]=3
  [HashMapStore]=3
  # Changed by decision 96 (decision 89): prefix_scan_trace_covered goes with the host-work metering.
  # [PathMapPrefix]=17
  [PathMapPrefix]=16
  # Changed by decision 96 (decision 89): settle_trace_covered goes with the host-work metering.
  # [StackDiscipline]=13
  [StackDiscipline]=12
  # Changed by decision 96 (decision 89): set_key_trace_covered goes with the host-work metering.
  # [SetDiscipline]=8
  [SetDiscipline]=7
  # Changed by decision 96 (decision 89): level_scan_trace_covered goes with the host-work metering.
  # [PriorityQueue]=20
  [PriorityQueue]=19
  # Changed by decision 103 (decision 118): three headlines over the claim rule of decision 118.
  # [JoinCascade]=17
  [JoinCascade]=20
  [Similarity]=20
  [VectorDbDiscipline]=5
  [VectorOps]=2
  [TheoryAdmission]=6
  [AgentTranslation]=7
  [Checkpoint]=8
)

PASS=0
FAIL=0
SKIP=0
pass() { PASS=$((PASS + 1)); echo "  PASS $*"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL $*"; }

finish() {
  echo "reified-rspaces proofs: PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
  if [[ $FAIL -gt 0 ]]; then
    exit 1
  fi
  exit 0
}

if [[ ! "$EXPECTED_CLOSED" =~ ^[0-9]+$ ]]; then
  echo "error: RRS_EXPECTED_CLOSED must be a number (got $EXPECTED_CLOSED)" >&2
  exit 2
fi

echo "== Reified RSpaces Rocq gate (expected closed headlines: $EXPECTED_CLOSED)"

# ── Tools ───────────────────────────────────────────────────────────────────
tools_ok=true
for tool in coqc rocqchk make python3; do
  if command -v "$tool" >/dev/null 2>&1; then
    pass "tool $tool: $(command -v "$tool")"
  else
    fail "tool $tool is missing"
    tools_ok=false
  fi
done
if [[ "${RRS_SANDBOX:-}" != "1" ]]; then
  if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope --quiet true >/dev/null 2>&1; then
    pass "tool systemd-run: user scopes are available"
  else
    fail "tool systemd-run: no user scope (set RRS_SANDBOX=1 inside a verifier scope)"
    tools_ok=false
  fi
fi
if [[ "$tools_ok" != true ]]; then
  finish
fi

# Heavy legs run under a memory ceiling unless the verifier scope bounds them.
bounded() {
  if [[ "${RRS_SANDBOX:-}" == "1" ]]; then
    "$@"
  else
    systemd-run --user --scope --quiet -p "MemoryMax=$RRS_MEMORY_MAX" -p MemorySwapMax=0 \
      -p "CPUQuota=$RRS_CPU_QUOTA" -- "$@"
  fi
}

# ── Scratch copy ────────────────────────────────────────────────────────────
mkdir -p "$SCRATCH_ROOT" "$LOG_DIR" || { fail "cannot create $SCRATCH_ROOT or $LOG_DIR"; finish; }
WORK="$(mktemp -d "$SCRATCH_ROOT/proofs.XXXXXX")" || { fail "cannot create a scratch directory"; finish; }
trap 'rm -rf -- "$WORK"' EXIT
export TMPDIR="$WORK/tmp"
mkdir -p "$TMPDIR" "$WORK/tree/theories"

if [[ ! -f "$SRC/_CoqProject" ]]; then
  fail "missing $SRC/_CoqProject"
  finish
fi
cp -- "$SRC/_CoqProject" "$WORK/tree/"
[[ -f "$SRC/Makefile" ]] && cp -- "$SRC/Makefile" "$WORK/tree/"
mapfile -t MODULE_PATHS < <(sed -e 's/#.*//' "$SRC/_CoqProject" | grep -oE '[^[:space:]]+\.v' )
if [[ ${#MODULE_PATHS[@]} -eq 0 ]]; then
  fail "_CoqProject lists no module"
  finish
fi
for rel in "${MODULE_PATHS[@]}"; do
  if [[ ! -f "$SRC/$rel" ]]; then
    fail "_CoqProject lists $rel, which does not exist"
    continue
  fi
  mkdir -p "$WORK/tree/$(dirname "$rel")"
  cp -- "$SRC/$rel" "$WORK/tree/$rel"
done
echo "   scratch: $WORK"
echo "   modules: ${MODULE_PATHS[*]}"

# ── 1. Hygiene ──────────────────────────────────────────────────────────────
# The checker strips nested (* ... *) comments and string literals, then
# looks for forbidden vernacular. A Hypothesis or Variable must be inside a
# Section: at the top level it is an axiom.
hygiene_py="$WORK/hygiene.py"
cat > "$hygiene_py" <<'PY'
import re
import sys

def strip(text):
    out, depth, i, in_string = [], 0, 0, False
    while i < len(text):
        ch, pair = text[i], text[i:i + 2]
        if depth == 0 and ch == '"':
            in_string = not in_string
            out.append(" ")
            i += 1
            continue
        if in_string:
            out.append("\n" if ch == "\n" else " ")
            i += 1
            continue
        if pair == "(*":
            depth += 1
            i += 2
            continue
        if pair == "*)" and depth > 0:
            depth -= 1
            i += 2
            continue
        out.append(ch if depth == 0 or ch == "\n" else (" " if ch != "\n" else ch))
        i += 1
    return "".join(out)

FORBIDDEN = [
    (r"\b(Axiom|Axioms|Parameter|Parameters|Conjecture|Conjectures)\b", "an axiom declaration"),
    (r"\bAdmitted\b", "Admitted"),
    (r"\badmit\b", "admit"),
    (r"\bAbort\b", "Abort"),
    (r"\bAdmit\s+Obligations\b", "Admit Obligations"),
    (r"\bUnset\s+(Guard|Positivity|Universe)\s+Checking\b", "a disabled kernel check"),
    (r"#\[\s*bypass_check", "a bypass_check attribute"),
    (r"\bTypeInType\b|-type-in-type", "type-in-type"),
]

failed = False
path = sys.argv[-1]
text = strip(open(path, encoding="utf-8").read())
lines = text.split("\n")
if sys.argv[1] == "--count-print-assumptions":
    print(len(re.findall(r"^\s*Print\s+Assumptions\s+\S+\s*\.", text, re.M)))
    sys.exit(0)
for pattern, label in FORBIDDEN:
    for number, line in enumerate(lines, start=1):
        if re.search(pattern, line):
            print(f"{path}:{number}: {label}: {line.strip()}")
            failed = True

depth = 0
for number, line in enumerate(lines, start=1):
    stripped = line.strip()
    if re.match(r"^Section\s+\w+\s*\.", stripped):
        depth += 1
    elif re.match(r"^End\s+\w+\s*\.", stripped) and depth > 0:
        depth -= 1
    elif re.match(r"^(Hypothesis|Hypotheses|Variable|Variables|Context)\b", stripped) and depth == 0:
        print(f"{path}:{number}: a section assumption outside a Section: {stripped}")
        failed = True

for number, line in enumerate(lines, start=1):
    if re.search(r"\bRequire\b", line) and not re.match(r"^\s*From\s+Stdlib\s+Require\b", line):
        print(f"{path}:{number}: an import outside Stdlib: {line.strip()}")
        failed = True
    if re.search(r"\b(Load|Declare\s+ML\s+Module|Add\s+LoadPath|Add\s+Rec\s+LoadPath)\b", line):
        print(f"{path}:{number}: a load-path or plugin command: {line.strip()}")
        failed = True

sys.exit(1 if failed else 0)
PY

for rel in "${MODULE_PATHS[@]}"; do
  module="$(basename "$rel" .v)"
  [[ -f "$WORK/tree/$rel" ]] || continue
  if out="$(python3 "$hygiene_py" "$WORK/tree/$rel" 2>&1)"; then
    pass "hygiene $module: no axiom, admission, top-level hypothesis or foreign import"
  else
    fail "hygiene $module:"
    printf '%s\n' "$out" | sed 's/^/        /'
  fi
done

# ── 2-4. Standalone compile, headlines, rocqchk ─────────────────────────────
total_closed=0
for rel in "${MODULE_PATHS[@]}"; do
  module="$(basename "$rel" .v)"
  [[ -f "$WORK/tree/$rel" ]] || continue
  dir="$WORK/standalone/$module"
  mkdir -p "$dir"
  cp -- "$WORK/tree/$rel" "$dir/$module.v"
  log="$LOG_DIR/$module.coqc.log"
  if (cd "$dir" && bounded coqc -q -noglob "$module.v") > "$log" 2>&1; then
    pass "standalone $module: coqc -q -noglob $module.v (no -Q, no -R)"
  else
    fail "standalone $module: coqc failed; see $log"
    tail -n 20 "$log" | sed 's/^/        /'
    continue
  fi

  printed="$(python3 "$hygiene_py" --count-print-assumptions "$dir/$module.v")"
  closed="$(grep -c 'Closed under the global context' "$log")"
  if grep -qE '^(Axioms|Section Variables|Opaque Constants|Transparent Constants):' "$log"; then
    fail "headlines $module: Print Assumptions reports assumptions; see $log"
  elif [[ -z "${EXPECTED_PER_MODULE[$module]:-}" ]]; then
    fail "headlines $module: the gate has no expected count for this module"
  elif [[ "$closed" -ne "$printed" ]]; then
    fail "headlines $module: $closed closed of $printed Print Assumptions lines"
  elif [[ "$closed" -ne "${EXPECTED_PER_MODULE[$module]}" ]]; then
    fail "headlines $module: $closed closed, expected ${EXPECTED_PER_MODULE[$module]}"
  else
    pass "headlines $module: $closed x 'Closed under the global context'"
  fi
  total_closed=$((total_closed + closed))

  chk_log="$LOG_DIR/$module.rocqchk.log"
  if (cd "$dir" && bounded rocqchk -o -R . '' "$module") > "$chk_log" 2>&1 \
     && grep -q '^\* Axioms: <none>' "$chk_log" \
     && grep -q 'relying on type-in-type: <none>' "$chk_log" \
     && grep -q 'relying on unsafe (co)fixpoints: <none>' "$chk_log" \
     && grep -q 'positivity is assumed: <none>' "$chk_log"; then
    pass "rocqchk $module: Axioms <none>, no type-in-type, no unsafe fixpoints, no assumed positivity"
  else
    fail "rocqchk $module: see $chk_log"
    tail -n 20 "$chk_log" | sed 's/^/        /'
  fi
done

# ── 5. Project build ────────────────────────────────────────────────────────
project_log="$LOG_DIR/project.make.log"
if [[ ! -f "$WORK/tree/Makefile" ]]; then
  fail "project: the shipped Makefile is missing"
elif (cd "$WORK/tree" && bounded make -j1) > "$project_log" 2>&1; then
  pass "project: make builds the ReifiedRSpaces library from _CoqProject"
  mapfile -t libs < <(for rel in "${MODULE_PATHS[@]}"; do echo "ReifiedRSpaces.$(basename "$rel" .v)"; done)
  chk_log="$LOG_DIR/project.rocqchk.log"
  if (cd "$WORK/tree" && bounded rocqchk -o -Q theories ReifiedRSpaces "${libs[@]}") > "$chk_log" 2>&1 \
     && grep -q '^\* Axioms: <none>' "$chk_log" \
     && grep -q 'relying on type-in-type: <none>' "$chk_log" \
     && grep -q 'relying on unsafe (co)fixpoints: <none>' "$chk_log" \
     && grep -q 'positivity is assumed: <none>' "$chk_log"; then
    pass "project rocqchk: ${libs[*]}: Axioms <none>"
  else
    fail "project rocqchk: see $chk_log"
    tail -n 20 "$chk_log" | sed 's/^/        /'
  fi
else
  fail "project: make failed; see $project_log"
  tail -n 20 "$project_log" | sed 's/^/        /'
fi

# ── 6. Total ────────────────────────────────────────────────────────────────
if [[ "$total_closed" -eq "$EXPECTED_CLOSED" ]]; then
  pass "total: $total_closed closed headlines (expected $EXPECTED_CLOSED)"
else
  fail "total: $total_closed closed headlines, expected $EXPECTED_CLOSED"
fi

# ── 7. Negative controls ────────────────────────────────────────────────────
# The harness bounds each coqc run itself, so it runs outside bounded().
negative_log="$LOG_DIR/negative-controls.log"
if RRS_ROCQ_SRC="$WORK/tree" RRS_SCRATCH="$SCRATCH_ROOT" \
     python3 "$ROOT/scripts/verify-reified/rocq-negative-controls.py" > "$negative_log" 2>&1; then
  pass "negative: $(grep -c '^  PASS ' "$negative_log") checks (pristine baselines and mutants); see $negative_log"
else
  fail "negative: see $negative_log"
  grep -E '^  FAIL ' "$negative_log" | sed 's/^/      /'
fi

# ── 8. Axiom control ────────────────────────────────────────────────────────
# The fixture lies outside theories/ and outside _CoqProject. It states one
# axiom on purpose, and the leg passes only when both checks report it.
fixture="$ROOT/formal/rocq/reified_rspaces/controls/AxiomControl.v"
if [[ ! -f "$fixture" ]]; then
  fail "axiom control: $fixture is missing"
else
  dir="$WORK/controls/AxiomControl"
  mkdir -p "$dir"
  cp -- "$fixture" "$dir/AxiomControl.v"
  if out="$(python3 "$hygiene_py" "$dir/AxiomControl.v" 2>&1)"; then
    fail "axiom control: the hygiene check accepts the fixture"
  elif grep -q 'an axiom declaration' <<<"$out"; then
    pass "axiom control: the hygiene check reports an axiom declaration"
  else
    fail "axiom control: the hygiene check fails without naming the axiom: $out"
  fi
  log="$LOG_DIR/AxiomControl.coqc.log"
  chk_log="$LOG_DIR/AxiomControl.rocqchk.log"
  if ! (cd "$dir" && bounded coqc -q -noglob AxiomControl.v) > "$log" 2>&1; then
    fail "axiom control: the fixture does not compile; see $log"
  elif ! (cd "$dir" && bounded rocqchk -o -R . '' AxiomControl) > "$chk_log" 2>&1; then
    fail "axiom control: rocqchk fails on the fixture; see $chk_log"
  elif grep -q '^\* Axioms: <none>' "$chk_log"; then
    fail "axiom control: rocqchk reports Axioms <none> for the fixture"
  elif grep -A1 '^\* Axioms:' "$chk_log" | grep -q 'AxiomControl.control_axiom'; then
    pass "axiom control: rocqchk lists the axiom AxiomControl.control_axiom"
  else
    fail "axiom control: rocqchk does not list the axiom; see $chk_log"
  fi
fi

echo "   logs: $LOG_DIR"
finish
