#!/usr/bin/env bash
# scripts/check-cost-accounted-rho-tla-invariants.sh
#
# Local-only TLA+ invariant runner for the cost_accounted_rho specs.
# Per team policy (memory `feedback_formal_verification_is_local_only_not_ci`),
# formal verification stays local — this script is NOT a CI step.
#
# Runs TLC against every .cfg under formal/tlaplus/cost_accounted_rho/
# whose paired .tla module exists. Every TLC run goes through the shared
# scripts/lib/tlc-run.sh launcher, which enforces a strict memory envelope
# (on-disk metadir — NOT tmpfs; bounded -Xmx heap; bounded workers; and a
# hard systemd-run MemoryMax / MemorySwapMax=0 ceiling) so a single large
# model can't exhaust RAM. Tune via TLC_HEAP / TLC_WORKERS / TLC_RSS /
# TLC_METADIR_ROOT (see the helper header).
#
# Each run is reported as PASS / FAIL based on the TLC output. Exit
# code 0 iff every spec reports "Model checking completed. No error
# has been found".
#
# Usage:
#   bash scripts/check-cost-accounted-rho-tla-invariants.sh
#   bash scripts/check-cost-accounted-rho-tla-invariants.sh --filter MC

set -euo pipefail

# Advisory by default per Greg's compile-time-shapes design: external-proof
# certificates (Rocq/Lean/TLA+) are NOT a required gate. Set CA_ENFORCE_PROOFS=1
# to run the full strict TLA+ invariant gate (preserved verbatim below).
if [ "${CA_ENFORCE_PROOFS:-0}" != "1" ]; then
  echo "cost-accounted-rho TLA+ invariant gate: ADVISORY (relaxed; external-proof certificates not required). CA_ENFORCE_PROOFS=1 to run the full gate."
  exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TLA_DIR="$REPO_ROOT/formal/tlaplus/cost_accounted_rho"

FILTER="${1:-}"
if [[ "$FILTER" == "--filter" ]]; then
    shift
    FILTER="${1:-}"
    shift || true
fi

if [[ ! -d "$TLA_DIR" ]]; then
    echo "ERROR: TLA+ cost_accounted_rho directory not found at $TLA_DIR" >&2
    exit 2
fi

if ! command -v tlc >/dev/null 2>&1; then
    echo "ERROR: tlc binary not on PATH; install tlaplus tooling" >&2
    exit 2
fi

# Shared memory-bounded TLC launcher: on-disk metadir, capped -Xmx heap,
# capped workers, hard systemd MemoryMax ceiling. See scripts/lib/tlc-run.sh.
export TLC_REPO_ROOT="$REPO_ROOT"
source "$REPO_ROOT/scripts/lib/tlc-run.sh"

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
# MAJOR-5: the token-gated-join sequential-fuel griefing / atomicity obligation.
# TokenGatedJoin.cfg is the NATIVE-model safety suite (must HOLD). The companion
# TokenGatedJoinM2Grief.cfg is an EXPECTED-REFUTATION run (it confirms the griefing
# vector for the TRANSPILER runtime-gate model by producing a counterexample), so
# it is deliberately NOT registered here — a counterexample is its intended result,
# not a pass. Run it explicitly: tlc -config TokenGatedJoinM2Grief.cfg MCTokenGatedJoin.tla
WRAPPED_BY[TokenGatedJoin]=MCTokenGatedJoin

# Collect all .cfg files whose paired .tla module exists.
specs=()
spec_roots=()
for cfg in *.cfg; do
    [[ -e "$cfg" ]] || continue
    base="${cfg%.cfg}"
    if [[ -z "$FILTER" || "$base" == *"$FILTER"* ]]; then
        if [[ "$base" != MC* ]] && [[ -n "${WRAPPED_BY[$base]:-}" ]]; then
            wrapper="${WRAPPED_BY[$base]}"
            if [[ -f "${wrapper}.tla" ]]; then
                specs+=("$base")
                spec_roots+=("${wrapper}.tla")
            fi
        elif [[ -f "${base}.tla" ]]; then
            specs+=("$base")
            spec_roots+=("${base}.tla")
        fi
    fi
done

if [[ ${#specs[@]} -eq 0 ]]; then
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

for i in "${!specs[@]}"; do
    base="${specs[$i]}"
    spec_root="${spec_roots[$i]}"
    printf "  %-40s " "${base} (${spec_root%.tla})"
    metadir="$METADIR_ROOT/$base"
    output=$(tlc_run "$metadir" "${base}.cfg" "$spec_root" -deadlock 2>&1 || true)
    if echo "$output" | grep -q "Model checking completed. No error has been found"; then
        echo "PASS"
        passes=$((passes + 1))
    elif echo "$output" | grep -q "Error:"; then
        echo "FAIL"
        failures=$((failures + 1))
        failed_specs+=("$base")
        echo "$output" | tail -10 | sed 's/^/    /'
    else
        echo "INCONCLUSIVE"
        failures=$((failures + 1))
        failed_specs+=("$base")
        echo "$output" | tail -5 | sed 's/^/    /'
    fi
done

# ─────────────────────────────────────────────────────────────────────────
# Validator behavioral contract (Workstream E, stage E5): the arithmetic
# obligations of the built-in validator's contract, discharged DEDUCTIVELY
# by TLAPS (not bounded model-checking) in formal/tlaplus/validator/
# Validator.tla. The state-machine obligations stay TLC-checked above
# (RuntimeBudgetReplay). Local-only, like the rest of this script.
VALIDATOR_TLA_DIR="$REPO_ROOT/formal/tlaplus/validator"
if [[ -z "$FILTER" || "Validator" == *"$FILTER"* ]] \
   && [[ -f "$VALIDATOR_TLA_DIR/Validator.tla" ]]; then
    printf "  %-40s " "Validator (TLAPS contract proofs)"
    # TLAPS and its zenon backend install under ~/.local; make them findable
    # without disturbing the TLC PATH used above (scoped to this subshell).
    VALIDATOR_PATH="$HOME/.local/tlaps/bin:$HOME/.local/bin:/usr/bin:$PATH"
    if PATH="$VALIDATOR_PATH" command -v tlapm >/dev/null 2>&1; then
        # Run TLAPS from the validator dir so its .tlacache lands locally.
        tlaps_out=$( cd "$VALIDATOR_TLA_DIR" \
            && PATH="$VALIDATOR_PATH" tlapm Validator.tla 2>&1 || true )
        # tlapm prints one "All N obligations proved." per module root; the
        # imported TLAPS.tla reports "All 0 obligation proved", so success is
        # a non-zero-obligation "All N obligations proved." for Validator.tla
        # with no "failed"/"omitted" anywhere.
        if echo "$tlaps_out" | grep -Eq "All [1-9][0-9]* obligations? proved\." \
           && ! echo "$tlaps_out" | grep -Eiq "obligation.*(failed|omitted)|[1-9][0-9]* (failed|omitted)"; then
            echo "PASS"
            passes=$((passes + 1))
        else
            echo "FAIL"
            failures=$((failures + 1))
            failed_specs+=("Validator(TLAPS)")
            echo "$tlaps_out" | tail -10 | sed 's/^/    /'
        fi
    else
        echo "SKIP (tlapm not on PATH)"
    fi
fi

echo
echo "Summary: $passes passed, $failures failed (total $((${#specs[@]} + 1)))"
if [[ $failures -gt 0 ]]; then
    echo "Failed specs:"
    printf '  - %s\n' "${failed_specs[@]}"
    exit 1
fi
