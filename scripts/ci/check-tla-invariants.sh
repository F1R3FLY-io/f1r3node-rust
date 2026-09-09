#!/usr/bin/env bash
# scripts/ci/check-tla-invariants.sh — run TLC against the bounded
# post-fix MC configs under formal/tlaplus/ and assert clean.
#
# Reference: docs/casper/theory/slashing/design/14-test-plan.md §14.6 / §14.9.
# Invokes the TLA+ model checker (TLC) against each MC instance:
#   • slashing/MC_EquivocationDetector_liveness{,_2v}.tla / .cfg
#   • slashing/MC_EquivocationDetectorEager{,_3v2s}.tla / .cfg
#   • slashing/MC_ConcurrentTracker{,_pre_fix}.tla / .cfg
#   • slashing/MC_SlashFlow.tla / .cfg
#   • slashing/MC_TwoLevelSlashing.tla / .cfg
#   • slashing/MC_AuthorizedSlashFlow.tla / .cfg
#   • slashing/MC_JustificationProjection.tla / .cfg
#   • slashing/MC_WithdrawFlow.tla / .cfg
#   • block_admission/MC_BlockAdmission.tla / .cfg
#   • deploy_lifecycle/MC_DeployLifecycle.tla / .cfg
#   • fork_choice/MC_ForkChoice.tla / .cfg
#   • fork_choice/MC_PromotionConvergence.tla / .cfg
#   • recovery_leader/MC_RecoveryLeader.tla / .cfg
#   • replay_liveness/MC_ReplayHotLoop.tla / .cfg
#   • carrier_index/MC_CarrierIndex.tla / .cfg
#
# The exhaustive tier (RUN_EXHAUSTIVE_TLA=1) holds the configs whose state
# spaces exceed the per-config wall-clock cap: MC_EquivocationDetector and
# MC_EquivocationDetectorEager_3v hit the 45m cap on every nightly since
# the schedule began (2026-07-25) — their interleaved liveness passes go
# superlinear past ~100M states — alongside MC_EquivocationDetector_safety,
# which can run for many hours. The nightly tier therefore gates on the
# fast configs only; the heavy pair runs opt-in until it gets the
# liveness/safety split that rescued MC_EquivocationDetector_liveness.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TLA_ROOT="$REPO_ROOT/formal/tlaplus"
SOAK_PR=false
if (($#)); then
    if (($# != 1)) || [[ "$1" != --soak-pr ]]; then
        echo "Usage: scripts/ci/check-tla-invariants.sh [--soak-pr]" >&2
        exit 2
    fi
    SOAK_PR=true
    if [[ "${RUN_EXHAUSTIVE_TLA:-0}" == 1 ]]; then
        echo "ERROR: The soak PR tier cannot include exhaustive configurations." >&2
        exit 2
    fi
fi

if [[ ! -d "$TLA_ROOT/slashing" ]]; then
    echo "ERROR: TLA+ slashing directory not found at $TLA_ROOT/slashing" >&2
    exit 2
fi

# Locate TLC. Common installation paths:
#   • $TLA_TOOLS_JAR pointing at tla2tools.jar (preferred, explicit)
#   • Java + tla2tools.jar in /usr/share/tla / /opt/tlaplus / ~/.tla
#   • `tlc` wrapper script on PATH
TLC_CMD=""
if [[ -n "${TLA_TOOLS_JAR:-}" && -f "$TLA_TOOLS_JAR" ]]; then
    TLC_CMD="java -XX:+UseParallelGC -jar $TLA_TOOLS_JAR"
elif command -v tlc >/dev/null 2>&1; then
    TLC_CMD="tlc"
else
    for candidate in \
        /usr/share/tla/tla2tools.jar \
        /opt/tlaplus/tla2tools.jar \
        "$HOME/.tla/tla2tools.jar"; do
        if [[ -f "$candidate" ]]; then
            TLC_CMD="java -XX:+UseParallelGC -jar $candidate"
            break
        fi
    done
fi

if [[ -z "$TLC_CMD" ]]; then
    echo "ERROR: TLC not found. Set TLA_TOOLS_JAR=/path/to/tla2tools.jar," >&2
    echo "       install tlaplus, or place the jar at one of: " >&2
    echo "         /usr/share/tla/tla2tools.jar" >&2
    echo "         /opt/tlaplus/tla2tools.jar" >&2
    echo "         ~/.tla/tla2tools.jar" >&2
    exit 3
fi

# Post-fix configs: each must TLC-clean. Entries are
# <subdir under formal/tlaplus>/<config basename>.
POST_FIX_CONFIGS=(
    slashing/MC_EquivocationDetector_liveness
    slashing/MC_EquivocationDetector_liveness_2v
    slashing/MC_EquivocationDetectorEager
    slashing/MC_EquivocationDetectorEager_3v2s
    slashing/MC_ConcurrentTracker
    slashing/MC_SlashFlow
    slashing/MC_TwoLevelSlashing
    slashing/MC_AuthorizedSlashFlow
    slashing/MC_JustificationProjection
    slashing/MC_WithdrawFlow
    block_admission/MC_BlockAdmission
    deploy_lifecycle/MC_DeployLifecycle
    fork_choice/MC_ForkChoice
    fork_choice/MC_PromotionConvergence
    recovery_leader/MC_RecoveryLeader
    replay_liveness/MC_ReplayHotLoop
    carrier_index/MC_CarrierIndex
    soak_disk/MC_SoakDisk
    soak_disk/MC_DiskProbeAdmission
    soak_disk/MC_DiskSampleValidation
    soak_disk/MC_ActiveDiskProbe
    soak_disk/MC_DiskEmergencyRecord
    soak_disk/MC_GuardianSupervision
    soak_disk/MC_DiskProbeDeadline
    soak_disk/MC_DiskDiagnosticDeadline
    soak_disk/MC_DiskBreachRestart
    soak_disk/MC_DiskStopDeadline
    soak_disk/MC_GuardianAdmission
)

TLC_WORKERS=auto
if [[ "$SOAK_PR" == true ]]; then
    POST_FIX_CONFIGS=(
        replay_liveness/MC_ReplayHotLoop
        carrier_index/MC_CarrierIndex
        soak_disk/MC_SoakDisk
        soak_disk/MC_DiskProbeAdmission
        soak_disk/MC_DiskSampleValidation
        soak_disk/MC_ActiveDiskProbe
        soak_disk/MC_DiskEmergencyRecord
        soak_disk/MC_GuardianSupervision
        soak_disk/MC_DiskProbeDeadline
        soak_disk/MC_DiskDiagnosticDeadline
        soak_disk/MC_DiskBreachRestart
        soak_disk/MC_DiskStopDeadline
        soak_disk/MC_GuardianAdmission
    )
    TLC_WORKERS=2
fi

if [[ "${RUN_EXHAUSTIVE_TLA:-0}" == "1" ]]; then
    POST_FIX_CONFIGS+=(
        slashing/MC_EquivocationDetector
        slashing/MC_EquivocationDetectorEager_3v
        slashing/MC_EquivocationDetector_safety
    )
fi

# Per-config wall-clock cap: one wedged or state-exploded config must not
# consume the whole job silently (observed: the first config alone exceeded
# a 60-minute CI job with no output). Timeouts are reported distinctly from
# invariant violations. Override via TLC_PER_CONFIG_TIMEOUT (GNU timeout
# duration syntax); the cap is skipped when `timeout` is unavailable.
TLC_PER_CONFIG_TIMEOUT="${TLC_PER_CONFIG_TIMEOUT:-45m}"
if [[ "$SOAK_PR" == true ]]; then
    TLC_PER_CONFIG_TIMEOUT=2m
fi
TIMEOUT_CMD=""
if command -v timeout >/dev/null 2>&1; then
    TIMEOUT_CMD="timeout --signal=TERM --kill-after=60 $TLC_PER_CONFIG_TIMEOUT"
elif command -v gtimeout >/dev/null 2>&1; then
    TIMEOUT_CMD="gtimeout --signal=TERM --kill-after=60 $TLC_PER_CONFIG_TIMEOUT"
fi
if [[ "$SOAK_PR" == true && -z "$TIMEOUT_CMD" ]]; then
    echo "ERROR: The soak PR tier requires timeout or gtimeout." >&2
    exit 3
fi

# Registered entries are hand-maintained above: a malformed entry or a
# missing config file is a broken registration, not a skippable condition —
# a silent SKIP here would let a renamed or deleted model quietly leave CI.
NEGATIVE_CONTROLS=(
    carrier_index/MC_CarrierIndex_dag_first_pre_fix:IndexCompleteForWindow
    carrier_index/MC_CarrierIndex_read_failure_pre_fix:AbsenceProofSound
    soak_disk/MC_SoakDisk_floor_only_pre_fix:AdmissionRequiresBand
    soak_disk/MC_DiskProbeAdmission_missing_sample_pre_fix:AdmissionRequiresSample
    soak_disk/MC_DiskSampleValidation_numeric_prefix_pre_fix:AdmissionRequiresValidSample
    soak_disk/MC_ActiveDiskProbe_missing_pre_fix:InvalidSampleRequiresInterrupt
    soak_disk/MC_DiskEmergencyRecord_stop_first_pre_fix:StopRequiresRecord
    soak_disk/MC_GuardianSupervision_unwatched_pre_fix:DeadGuardianRequiresInterrupt
    soak_disk/MC_DiskProbeDeadline_unbounded_pre_fix:ProbeWithinDeadline
    soak_disk/MC_DiskDiagnosticDeadline_per_root_pre_fix:AttributionWithinBudget
    soak_disk/MC_DiskBreachRestart_clear_pre_fix:RetainedBreachStopsRestart
    soak_disk/MC_DiskStopDeadline_unbounded_pre_fix:StopWithinBudget
    soak_disk/MC_GuardianAdmission_unchecked_pre_fix:AdmissionRequiresGuardian
)

failed=0
timeouts=0
violations=0
for check in "${POST_FIX_CONFIGS[@]}" "${NEGATIVE_CONTROLS[@]}"; do
    entry="${check%%:*}"
    expected_invariant=""
    if [[ "$check" == *:* ]]; then
        expected_invariant="${check#*:}"
    fi
    if [[ "$entry" != */* ]]; then
        echo "ERROR: malformed POST_FIX_CONFIGS entry '$entry' (expected <subdir>/<config>)" >&2
        exit 2
    fi
    dir="$TLA_ROOT/${entry%/*}"
    cfg="${entry##*/}"
    log="/tmp/tlc-${entry//\//-}.log"
    if [[ ! -f "$dir/$cfg.tla" || ! -f "$dir/$cfg.cfg" ]]; then
        echo "FAIL   $entry (missing $cfg.tla or $cfg.cfg in $dir — registered config not found)"
        failed=$((failed + 1))
        violations=$((violations + 1))
        continue
    fi
    started_epoch="$(date +%s)"
    echo "CHECK  $entry (started $(date -u +%H:%M:%SZ), cap $TLC_PER_CONFIG_TIMEOUT)"
    set +e
    (cd "$dir" && $TIMEOUT_CMD $TLC_CMD -workers "$TLC_WORKERS" -config "$cfg.cfg" "$cfg.tla") >"$log" 2>&1
    status=$?
    set -e
    elapsed="$(($(date +%s) - started_epoch))s"
    if ((status == 0)) && [[ -z "$expected_invariant" ]]; then
        echo "OK     $entry ($elapsed)"
    elif ((status == 12)) && [[ -n "$expected_invariant" ]] &&
        grep -Fxq "Error: Invariant $expected_invariant is violated." "$log"; then
        echo "EXPECTED-FAIL $entry ($expected_invariant, $elapsed)"
    elif ((status == 124)); then
        echo "TIMEOUT $entry after $elapsed (cap $TLC_PER_CONFIG_TIMEOUT) — treat as failure; profile or split the config"
        failed=$((failed + 1))
        timeouts=$((timeouts + 1))
    else
        echo "FAIL   $entry ($elapsed)"
        if [[ -n "$expected_invariant" ]]; then
            echo "Expected invariant $expected_invariant with TLC exit 12, received exit $status."
        fi
        echo "--- last 40 lines of $log ---"
        tail -40 "$log"
        echo "--- end log ---"
        failed=$((failed + 1))
        violations=$((violations + 1))
    fi
done

if ((failed > 0)); then
    echo "FAILED: $failed config(s) did not verify — $timeouts cap timeout(s), $violations violation-or-error(s)."
    exit 1
fi

echo "All $((${#POST_FIX_CONFIGS[@]})) post-fix TLA+ configurations clean."
echo "All ${#NEGATIVE_CONTROLS[@]} negative controls violated their expected invariants."
