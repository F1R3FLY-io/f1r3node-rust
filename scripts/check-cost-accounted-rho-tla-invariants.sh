#!/usr/bin/env bash
set -euo pipefail

# Advisory by default per Greg's compile-time-shapes design: external-proof
# certificates (Rocq/Lean/TLA+) are NOT a required gate. Set CA_ENFORCE_PROOFS=1
# to run the full strict TLA+ invariant gate.
if [ "${CA_ENFORCE_PROOFS:-0}" != "1" ]; then
  echo "cost-accounted-rho TLA+ invariant gate: ADVISORY (relaxed; external-proof certificates not required). CA_ENFORCE_PROOFS=1 to run the full gate."
  exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TLA_DIR="$REPO_ROOT/formal/tlaplus/cost_accounted_rho"

FILTER=""
if [[ $# -gt 0 ]]; then
    if [[ $# -ne 2 || "$1" != "--filter" || -z "$2" ]]; then
        echo "ERROR: usage: $0 [--filter SUBSTRING]" >&2
        exit 2
    fi
    FILTER="$2"
fi

if [[ ! -d "$TLA_DIR" ]]; then
    echo "ERROR: TLA+ cost_accounted_rho directory not found at $TLA_DIR" >&2
    exit 2
fi

# Shared memory-bounded TLC launcher: on-disk metadir, capped -Xmx heap,
# capped workers, hard systemd MemoryMax ceiling. See scripts/lib/tlc-run.sh.
export TLC_REPO_ROOT="$REPO_ROOT"
source "$REPO_ROOT/scripts/lib/tlc-run.sh"
if [[ ! -f "$TLC_JAR" ]] && ! command -v tlc >/dev/null 2>&1; then
    echo "ERROR: neither TLC jar nor tlc binary is available" >&2
    exit 2
fi

cd "$TLA_DIR"

# Some protocol .cfg files were authored for use ONLY through their
# MC wrapper module (they reference MC_-prefixed identifiers that
# only resolve when the MC*.tla module is the spec root). The MC
# wrappers have non-trivial naming (e.g., CompoundProtocol.cfg is
# wrapped by MCCompound.tla, NOT MCCompoundProtocol.tla). This
# explicit map records which protocol .cfgs depend on which wrapper.
# The mapping is used to invoke TLC as:
#   tlc -config <base>.cfg <wrapper>.tla
declare -A WRAPPED_BY
WRAPPED_BY[CompoundProtocol]=MCCompound
WRAPPED_BY[CompoundSettlement]=MCCompoundSettlement
WRAPPED_BY[CostAccountedRho]=MC
WRAPPED_BY[CostAccountingSearchFrontier]=MCCostAccountingSearchFrontier
WRAPPED_BY[CostAccountingThreats]=MCCostAccountingThreats
WRAPPED_BY[EvalScheduling]=MCEval
# #13b: focused strict reject-when-absent instance (PoolSupply = 0).
WRAPPED_BY[EvalStrictAbsent]=MCEvalStrictAbsent
WRAPPED_BY[FullProtocol]=MCFull
WRAPPED_BY[MergeableChannelAccounting]=MCMergeableChannelAccounting
WRAPPED_BY[RuntimeBudgetReplay]=MCRuntimeBudgetReplay
WRAPPED_BY[SignedRegionCoreErased]=SignedRegionCore
WRAPPED_BY[SignedRegionCorePartialJoin]=SignedRegionCore
WRAPPED_BY[SignedRegionCoreSpanAlias]=SignedRegionCore
WRAPPED_BY[TokenGatedJoin]=MCTokenGatedJoin
WRAPPED_BY[TokenGatedJoinM2Grief]=MCTokenGatedJoin
WRAPPED_BY[EvalNoLockNeg]=MCEvalNoLockNeg

declare -A EXPECTED_INVARIANT_REFUTATIONS
EXPECTED_INVARIANT_REFUTATIONS[SignedRegionCoreErased]=Inv_ContinuationSealPreserved
EXPECTED_INVARIANT_REFUTATIONS[SignedRegionCorePartialJoin]=Inv_AtomicJoinDebit
EXPECTED_INVARIANT_REFUTATIONS[SignedRegionCoreSpanAlias]=Inv_FreshBinderIsolation
EXPECTED_INVARIANT_REFUTATIONS[TokenGatedJoinM2Grief]=Inv_M2_NoGrieferDrainOfVictim

specs=()
spec_roots=()
for cfg in *.cfg; do
    [[ -e "$cfg" ]] || continue
    base="${cfg%.cfg}"
    if [[ -z "$FILTER" || "$base" == *"$FILTER"* ]]; then
        if [[ -n "${WRAPPED_BY[$base]:-}" ]]; then
            wrapper="${WRAPPED_BY[$base]}"
            if [[ ! -f "${wrapper}.tla" ]]; then
                echo "ERROR: no wrapper module ${wrapper}.tla for $cfg" >&2
                exit 2
            fi
            specs+=("$base")
            spec_roots+=("${wrapper}.tla")
        elif [[ -f "${base}.tla" ]]; then
            specs+=("$base")
            spec_roots+=("${base}.tla")
        else
            echo "ERROR: no module or classified wrapper for $cfg" >&2
            exit 2
        fi
    fi
done

validator_selected=0
if [[ -z "$FILTER" || "Validator" == *"$FILTER"* ]]; then
    validator_selected=1
fi

if [[ ${#specs[@]} -eq 0 ]] \
   && { [[ -z "$FILTER" ]] || [[ $validator_selected -eq 0 ]]; }; then
    echo "No matching specs found" >&2
    exit 2
fi

echo "Running TLC against ${#specs[@]} cost_accounted_rho specs"
echo "Memory envelope: -Xmx${TLC_HEAP}, ${TLC_WORKERS} workers, on-disk metadir, MemoryMax=${TLC_RSS} (MemorySwapMax=0)"
echo

passes=0
failures=0
failed_specs=()
# Per-spec metadirs under an ON-DISK root. NOT mktemp -d, which lands in
# TMPDIR=/tmp — tmpfs (RAM) on this host, so TLC's multi-GB state graph
# would spill into RAM instead of onto the NVMe. Cleared up front (a prior
# SIGKILL'd run leaks its metadir because the EXIT trap never fires) and
# again on exit.
METADIR_ROOT="$TLC_METADIR_ROOT/cost-accounted-gate"
rm -rf "$METADIR_ROOT"
mkdir -p "$METADIR_ROOT"
trap 'rm -rf "$METADIR_ROOT"' EXIT
log_dir="$REPO_ROOT/target/verification/cost-accounted-rho-tla"
mkdir -p "$log_dir"

for i in "${!specs[@]}"; do
    base="${specs[$i]}"
    spec_root="${spec_roots[$i]}"
    printf "  %-40s " "${base} (${spec_root%.tla})"
    metadir="$METADIR_ROOT/$base"
    log_file="$log_dir/$base.log"
    if tlc_run "$metadir" "${base}.cfg" "$spec_root" -deadlock >"$log_file" 2>&1; then
        rc=0
    else
        rc=$?
    fi
    if [[ -n "${EXPECTED_INVARIANT_REFUTATIONS[$base]:-}" ]] \
       && [[ $rc -eq 12 ]] \
       && grep -Fxq "Error: Invariant ${EXPECTED_INVARIANT_REFUTATIONS[$base]} is violated." "$log_file" \
       && grep -Fxq 'Error: The behavior up to this point is:' "$log_file"; then
        echo "PASS (expected refutation)"
        passes=$((passes + 1))
    elif [[ "$base" == EvalNoLockNeg ]] \
         && [[ $rc -eq 13 ]] \
         && [[ "$(awk '$1 == "PROPERTY" {n++; property=$2} END {if (n == 1) print property}' "${base}.cfg")" == DisjointPoolsAdmitConcurrentlyNoGlobalLock ]] \
         && grep -Fxq 'Error: Temporal properties were violated.' "$log_file" \
         && grep -Fxq 'Error: The following behavior constitutes a counter-example:' "$log_file"; then
        echo "PASS (expected refutation)"
        passes=$((passes + 1))
    elif [[ -z "${EXPECTED_INVARIANT_REFUTATIONS[$base]:-}" && "$base" != EvalNoLockNeg ]] \
         && [[ $rc -eq 0 ]] \
         && grep -Fq 'Model checking completed. No error has been found.' "$log_file" \
         && ! grep -Fq 'Error:' "$log_file"; then
        echo "PASS"
        passes=$((passes + 1))
    elif grep -Fq 'Error:' "$log_file"; then
        echo "FAIL"
        failures=$((failures + 1))
        failed_specs+=("$base")
        tail -10 "$log_file" | sed 's/^/    /'
    else
        echo "INCONCLUSIVE"
        failures=$((failures + 1))
        failed_specs+=("$base")
        tail -5 "$log_file" | sed 's/^/    /'
    fi
done

# ─────────────────────────────────────────────────────────────────────────
# Validator behavioral contract (Workstream E, stage E5): the arithmetic
# obligations of the built-in validator's contract, discharged DEDUCTIVELY
# by TLAPS (not bounded model-checking) in formal/tlaplus/validator/
# Validator.tla. The state-machine obligations stay TLC-checked above
# (RuntimeBudgetReplay). Local-only, like the rest of this script.
VALIDATOR_TLA_DIR="$REPO_ROOT/formal/tlaplus/validator"
if [[ $validator_selected -eq 1 && -f "$VALIDATOR_TLA_DIR/Validator.tla" ]]; then
    printf "  %-40s " "Validator (TLAPS contract proofs)"
    # TLAPS and its zenon backend install under ~/.local; make them findable
    # without disturbing the TLC PATH used above (scoped to this subshell).
    VALIDATOR_PATH="$HOME/.local/tlaps/bin:$HOME/.local/bin:/usr/bin:$PATH"
    if PATH="$VALIDATOR_PATH" command -v tlapm >/dev/null 2>&1; then
        # Run TLAPS from the validator dir so its .tlacache lands locally.
        if ( cd "$VALIDATOR_TLA_DIR" \
            && PATH="$VALIDATOR_PATH" tlapm Validator.tla ) >"$log_dir/Validator.log" 2>&1; then
            tlaps_rc=0
        else
            tlaps_rc=$?
        fi
        # tlapm prints one "All N obligations proved." per module root; the
        # imported TLAPS.tla reports "All 0 obligation proved", so success is
        # a non-zero-obligation "All N obligations proved." for Validator.tla
        # with no "failed"/"omitted" anywhere.
        if [[ $tlaps_rc -eq 0 ]] \
           && grep -Eq "All [1-9][0-9]* obligations? proved\." "$log_dir/Validator.log" \
           && ! grep -Eiq "obligation.*(failed|omitted)|[1-9][0-9]* (failed|omitted)" "$log_dir/Validator.log"; then
            echo "PASS"
            passes=$((passes + 1))
        else
            echo "FAIL"
            failures=$((failures + 1))
            failed_specs+=("Validator(TLAPS)")
            tail -10 "$log_dir/Validator.log" | sed 's/^/    /'
        fi
    else
        echo "FAIL (tlapm not on PATH)"
        printf '%s\n' 'tlapm not on PATH' >"$log_dir/Validator.log"
        failures=$((failures + 1))
        failed_specs+=("Validator(TLAPS)")
    fi
elif [[ $validator_selected -eq 1 ]]; then
    echo "  Validator (TLAPS contract proofs)        FAIL (model absent)"
    printf '%s\n' 'Validator.tla model absent' >"$log_dir/Validator.log"
    failures=$((failures + 1))
    failed_specs+=("Validator(TLAPS)")
fi

echo
echo "Summary: $passes passed, $failures failed (total $((passes + failures)))"
if [[ $failures -gt 0 ]]; then
    echo "Failed specs:"
    printf '  - %s\n' "${failed_specs[@]}"
    exit 1
fi
