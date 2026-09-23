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
    deploy_storage/MC_DeployStorageBound
    soak_disk/MC_SoakDiskAdmission
    soak_disk/MC_SoakDiskGuardian
    soak_disk/MC_SoakStorageBudget
    soak_disk/MC_MetricBaseline
    soak_disk/MC_MetricBaseline_available
    soak_disk/MC_MetricMonotonicity
    soak_disk/MC_MetricSampleValidity
    soak_disk/MC_MetricSummary
    soak_disk/MC_ReserveBound
    soak_disk/MC_RetentionReserve
    node_observation/MC_ObserverSession
    node_observation/MC_BoundedCapture
)

TLC_WORKERS=auto
if [[ "$SOAK_PR" == true ]]; then
    POST_FIX_CONFIGS=(
        replay_liveness/MC_ReplayHotLoop
        carrier_index/MC_CarrierIndex
        deploy_storage/MC_DeployStorageBound
        soak_disk/MC_SoakDiskAdmission
        soak_disk/MC_SoakDiskGuardian
        soak_disk/MC_SoakStorageBudget
        soak_disk/MC_MetricBaseline
        soak_disk/MC_MetricBaseline_available
        soak_disk/MC_MetricMonotonicity
        soak_disk/MC_MetricSampleValidity
        soak_disk/MC_MetricSummary
        soak_disk/MC_ReserveBound
        soak_disk/MC_RetentionReserve
        node_observation/MC_ObserverSession
        node_observation/MC_BoundedCapture
    )
    TLC_WORKERS=2
fi

POST_FIX_CONFIGS+=(casper_soak/MC_CasperSoakHarness)

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
if [[ -z "$TIMEOUT_CMD" ]]; then
    echo "ERROR: The registered Casper controls require timeout or gtimeout." >&2
    exit 3
fi

# Registered entries are hand-maintained above: a malformed entry or a
# missing config file is a broken registration, not a skippable condition —
# a silent SKIP here would let a renamed or deleted model quietly leave CI.
#
# NEGATIVE_CONTROLS is the single registry of expected-violation configs:
# <subdir>/<config>:<invariant>. Each must exit 12 with exactly that invariant;
# scripts/ci/test-check-tla-invariants.sh reads this list rather than copying it.
NEGATIVE_CONTROLS=(
    node_observation/MC_ObserverSession_freshness_pre_fix:FreshChallenges
    node_observation/MC_ObserverSession_challenge_unsafe:FreshChallenge
    node_observation/MC_ObserverSession_identity_unsafe:BoundIdentity
    node_observation/MC_ObserverSession_frame_unsafe:BoundFrame
    node_observation/MC_ObserverSession_deadline_unsafe:BoundDeadline
    node_observation/MC_ObserverSession_repeat_unsafe:OneRequest
    node_observation/MC_ObserverSession_budget_unsafe:SessionBudget
    node_observation/MC_BoundedCapture_admission_unsafe:ValidAdmission
    node_observation/MC_BoundedCapture_deadline_unsafe:BoundLockWait
    node_observation/MC_BoundedCapture_order_unsafe:GuardOrder
    node_observation/MC_BoundedCapture_open_unsafe:OpenIdentity
    node_observation/MC_BoundedCapture_validation_unsafe:ValidatedIdentity
    node_observation/MC_BoundedCapture_generation_unsafe:GenerationStable
    node_observation/MC_BoundedCapture_incomplete_unsafe:CompleteRows
    node_observation/MC_BoundedCapture_bytes_unsafe:BoundBytes
    node_observation/MC_BoundedCapture_release_unsafe:Detached
    node_observation/MC_BoundedCapture_write_unsafe:ReadOnly
    carrier_index/MC_CarrierIndex_dag_first_pre_fix:IndexCompleteForWindow
    carrier_index/MC_CarrierIndex_read_failure_pre_fix:AbsenceProofSound
    soak_disk/MC_SoakDiskAdmission_floor_only_pre_fix:AdmissionRequiresBand
    soak_disk/MC_SoakDiskAdmission_missing_sample_pre_fix:AdmissionRequiresSample
    soak_disk/MC_SoakDiskAdmission_numeric_prefix_pre_fix:AdmissionRequiresValidSample
    soak_disk/MC_SoakDiskAdmission_unchecked_guardian_pre_fix:AdmissionRequiresGuardian
    soak_disk/MC_SoakDiskAdmission_retained_breach_pre_fix:RetainedBreachPreventsBenchmark
    soak_disk/MC_SoakDiskAdmission_benchmark_band_pre_fix:BenchmarkRequiresBand
    soak_disk/MC_SoakDiskAdmission_late_guardian_pre_fix:BenchmarkBreachObserved
    soak_disk/MC_SoakDiskAdmission_unwatched_death_pre_fix:BenchmarkCancellationObserved
    soak_disk/MC_SoakDiskAdmission_unwatched_breach_pre_fix:BenchmarkCancellationObserved
    soak_disk/MC_SoakDiskAdmission_unchecked_progress_pre_fix:StaleProgressPreventsAdmission
    soak_disk/MC_SoakDiskAdmission_unbounded_hygiene_pre_fix:HygieneWithinBudget
    soak_disk/MC_SoakDiskAdmission_unchecked_range_pre_fix:AdmissionRequiresValidDiskSettings
    soak_disk/MC_SoakDiskAdmission_age_only_pre_fix:UnownedSessionPreserved
    soak_disk/MC_SoakDiskAdmission_ignore_errors_pre_fix:CleanupFailurePreventsAdmission
    soak_disk/MC_SoakDiskAdmission_global_prune_pre_fix:UnownedDockerResourcesPreserved
    soak_disk/MC_SoakDiskAdmission_unrecorded_pre_fix:CrashRequiresRefusal
    soak_disk/MC_SoakDiskAdmission_unrecorded_benchmark_pre_fix:BenchmarkCrashRequiresRefusal
    soak_disk/MC_SoakDiskAdmission_iteration_only_pre_fix:BenchmarkMonitorDeathObserved
    soak_disk/MC_SoakDiskAdmission_unchecked_monitor_pre_fix:MonitorDeathPreventsAdmission
    soak_disk/MC_SoakDiskAdmission_unchecked_placement_pre_fix:UnverifiedPlacementPreventsAdmission
    soak_disk/MC_SoakDiskAdmission_pathname_pre_fix:UntrustedRecordPreventsAdmission
    soak_disk/MC_SoakDiskAdmission_attribute_first_pre_fix:AttributionRequiresRecord
    soak_disk/MC_SoakDiskAdmission_in_place_pre_fix:VisibleImpliesComplete
    soak_disk/MC_SoakDiskAdmission_unchecked_producer_pre_fix:FailedProducerPreservesRecord
    soak_disk/MC_SoakDiskGuardian_unwatched_pre_fix:DeadGuardianRequiresInterrupt
    soak_disk/MC_SoakDiskGuardian_unavailable_sample_pre_fix:InvalidSampleRequiresInterrupt
    soak_disk/MC_SoakDiskGuardian_unbounded_probe_pre_fix:ProbeWithinDeadline
    soak_disk/MC_SoakDiskGuardian_stop_first_pre_fix:StopRequiresRecord
    soak_disk/MC_SoakDiskGuardian_per_root_deadline_pre_fix:AttributionWithinBudget
    soak_disk/MC_SoakDiskGuardian_cleared_breach_pre_fix:RetainedBreachStopsRestart
    soak_disk/MC_SoakDiskGuardian_unbounded_stop_pre_fix:StopWithinBudget
    soak_disk/MC_SoakDiskGuardian_alive_only_pre_fix:StaleGuardianRequiresInterrupt
    soak_disk/MC_SoakDiskGuardian_client_only_pre_fix:ExitStopsWriters
    soak_disk/MC_SoakDiskGuardian_rate_exceeds_floor_pre_fix:NoOverrun
    soak_disk/MC_SoakDiskGuardian_unconfirmed_stop_pre_fix:NoOverrun
    soak_disk/MC_SoakDiskGuardian_ignored_pre_fix:FailedStopRetained
    soak_disk/MC_SoakDiskGuardian_name_only_pre_fix:UnownedWritersPreserved
    soak_disk/MC_SoakDiskGuardian_host_pattern_only_pre_fix:UnownedHostWritersPreserved
    soak_disk/MC_SoakDiskGuardian_oom_pattern_only_pre_fix:UnownedPreferencesPreserved
    soak_disk/MC_SoakDiskGuardian_periodic_name_pre_fix:UnownedContainerPreferencesPreserved
    soak_disk/MC_SoakDiskGuardian_parent_group_pre_fix:CrashStopsOwnedWriters
    soak_disk/MC_SoakDiskGuardian_unconditional_pre_fix:HandledExitHasNoExtraStop
    soak_disk/MC_SoakDiskGuardian_startup_only_pre_fix:DeadMonitorRequiresInterrupt
    soak_disk/MC_SoakDiskGuardian_drain_first_pre_fix:DrainRequiresOwnedStop
    soak_disk/MC_SoakDiskGuardian_unmanaged_pre_fix:ControllerLossStopsOwnedWriters
    soak_disk/MC_SoakDiskGuardian_start_first_pre_fix:UnavailableQueryPreventsRelease
    soak_disk/MC_SoakDiskGuardian_unbounded_pre_fix:ResponseWithinDeadline
    soak_disk/MC_SoakDiskGuardian_parent_only_pre_fix:UntrustedControlPreventsRelease
    soak_disk/MC_SoakDiskGuardian_blocking_pre_fix:ShutdownIndependentOfSync
    soak_disk/MC_SoakStorageBudget_uncapped_blocks_pre_fix:WithinBudget
    soak_disk/MC_SoakStorageBudget_uncapped_logs_pre_fix:WithinBudget
    soak_disk/MC_SoakStorageBudget_uncapped_history_pre_fix:WithinBudget
    soak_disk/MC_MetricBaseline_zero_pre_fix:MissingBaselineUnavailable
    soak_disk/MC_MetricMonotonicity_unchecked_pre_fix:DecreasingSamplesUnavailable
    soak_disk/MC_MetricSampleValidity_unchecked_pre_fix:InvalidSamplesRejected
    soak_disk/MC_MetricSummary_omitted_pre_fix:CollectedRequiredPublished
    soak_disk/MC_ReserveBound_unbounded_pre_fix:OperatingReserveHeld
    soak_disk/MC_RetentionReserve_unskipped_pre_fix:ReserveHeld
    deploy_storage/MC_DeployStorageBound_unmetered_pre_fix:RetainedWithinPhlo
    casper_soak/MC_CasperSoakHarness_identity_unsafe:IdentityPinned
    casper_soak/MC_CasperSoakHarness_resume_unsafe:ResumePreservesHistory
    casper_soak/MC_CasperSoakHarness_failure_unsafe:ProductFailureMonotone
    casper_soak/MC_CasperSoakHarness_evidence_unsafe:PassRequiresEvidence
    casper_soak/MC_CasperSoakHarness_stop_unsafe:StopPreventsLaunch
    casper_soak/MC_CasperSoakHarness_cleanup_unsafe:EvidenceBeforeCleanup
    casper_soak/MC_CasperSoakHarness_policy_unsafe:PolicyIsolation
    casper_soak/MC_CasperSoakHarness_samples_unsafe:MissingIsUnknown
    casper_soak/MC_CasperSoakHarness_merge_unsafe:PostMergeGate
    casper_soak/MC_CasperSoakHarness_control_unsafe:ControlVerdictExact
)

# Areas whose expected-violation configurations are all registered. A
# MC_<Model>_*_pre_fix.cfg beside a registered positive in one of these
# directories that is absent from NEGATIVE_CONTROLS is a broken registration,
# not a manual control. Other areas keep manual controls until they opt in
# (docs/formal-verification.md).
REGISTERED_CONTROL_AREAS=(carrier_index deploy_storage soak_disk casper_soak node_observation)
for entry in "${POST_FIX_CONFIGS[@]}"; do
    area="${entry%%/*}"
    printf '%s\n' "${REGISTERED_CONTROL_AREAS[@]}" | grep -Fx "$area" >/dev/null || continue
    for cfg in "$TLA_ROOT/$entry"_*_pre_fix.cfg "$TLA_ROOT/$entry"_*_unsafe.cfg; do
        [[ -f "$cfg" ]] || continue
        control="$area/$(basename "$cfg" .cfg)"
        if ! printf '%s\n' "${NEGATIVE_CONTROLS[@]}" | grep "^$control:" >/dev/null; then
            echo "ERROR: $control exists but is not registered in NEGATIVE_CONTROLS" >&2
            exit 2
        fi
    done
done

plan="$TLA_ROOT/node_observation/verification-plan.json"
plan_entries=$(jq -er '
    .models as $models |
    if ($models | length) == 19 and
       ([$models[].module] | unique | length) == 19 and
       ([$models[] | select(.expected_exit == 0) | .module] | sort) ==
         ["MC_BoundedCapture", "MC_ObserverSession"] and
       all($models[]; (.module | test("^MC_[A-Za-z_]+$")) and
         .configuration == (.module + ".cfg") and
         ((.expected_exit == 0 and .invariant == null) or
          (.expected_exit == 12 and (.invariant | type) == "string" and
           (.invariant | test("^[A-Za-z]+$")))))
    then $models[] | .module + ":" + (.invariant // "")
    else error("Invalid node model inventory") end' "$plan" | sort)
registered_entries=$(
    for entry in "${POST_FIX_CONFIGS[@]}"; do
        [[ "$entry" != node_observation/* ]] || printf '%s:\n' "${entry#*/}"
    done
    for entry in "${NEGATIVE_CONTROLS[@]}"; do
        [[ "$entry" != node_observation/* ]] || printf '%s\n' "${entry#*/}"
    done
)
if [[ "$plan_entries" != "$(printf '%s\n' "$registered_entries" | sort)" ]]; then
    echo 'ERROR: The node model plan and gate registrations differ.' >&2
    exit 2
fi
plan_configs=$(printf '%s\n' "$plan_entries" | cut -d: -f1 | sort)
actual_configs=$(for cfg in "$TLA_ROOT"/node_observation/MC_*.cfg; do basename "$cfg" .cfg; done | sort)
if [[ "$plan_configs" != "$actual_configs" ]]; then
    echo 'ERROR: The node model plan and configuration files differ.' >&2
    exit 2
fi

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
    model="$cfg"
    workers="$TLC_WORKERS"
    check_timeout="$TLC_PER_CONFIG_TIMEOUT"
    check_timeout_cmd="$TIMEOUT_CMD"
    java_options="${JAVA_TOOL_OPTIONS:-}"
    tlc_args=()
    if [[ "${entry%%/*}" == casper_soak ]]; then
        model=CasperSoakHarness
        workers=1
        check_timeout=2m
        check_timeout_cmd="${TIMEOUT_CMD%"$TLC_PER_CONFIG_TIMEOUT"}2m"
        java_options+=" -Xmx512m"
        tlc_args=(-seed 1)
    fi
    if [[ ! -f "$dir/$model.tla" || ! -f "$dir/$cfg.cfg" ]]; then
        echo "FAIL   $entry (missing $model.tla or $cfg.cfg in $dir — registered config not found)"
        failed=$((failed + 1))
        violations=$((violations + 1))
        continue
    fi
    started_epoch="$(date +%s)"
    echo "CHECK  $entry (started $(date -u +%H:%M:%SZ), cap $check_timeout)"
    set +e
    (cd "$dir" && JAVA_TOOL_OPTIONS="$java_options" $check_timeout_cmd $TLC_CMD -workers "$workers" "${tlc_args[@]}" -config "$cfg.cfg" "$model.tla") >"$log" 2>&1
    status=$?
    set -e
    elapsed="$(($(date +%s) - started_epoch))s"
    if ((status == 0)) && [[ -z "$expected_invariant" ]] &&
        grep -Fxq 'Model checking completed. No error has been found.' "$log" &&
        awk '/^Error:/ { invalid = 1 } END { exit invalid }' "$log"; then
        echo "OK     $entry ($elapsed)"
    elif ((status == 12)) && [[ -n "$expected_invariant" ]] &&
        awk -v expected="Error: Invariant $expected_invariant is violated." '
            $0 == expected { violations++ }
            /^Error:/ && $0 != expected && $0 != "Error: The behavior up to this point is:" { invalid = 1 }
            /^Model checking completed\. No error has been found\.$/ { invalid = 1 }
            /^Error: The behavior up to this point is:$/ { trace_header = 1; next }
            trace_header && /^[[:space:]]*$/ { next }
            trace_header && /^State 1:/ { found = 1 }
            { trace_header = 0 }
            END { exit !(found && violations == 1 && !invalid) }
        ' "$log"; then
        echo "EXPECTED-FAIL $entry ($expected_invariant, $elapsed)"
    elif ((status == 124)); then
        echo "TIMEOUT $entry after $elapsed (cap $check_timeout) — treat as failure; profile or split the config"
        failed=$((failed + 1))
        timeouts=$((timeouts + 1))
    else
        echo "FAIL   $entry ($elapsed)"
        if [[ -n "$expected_invariant" ]]; then
            echo "Expected invariant $expected_invariant with TLC exit 12 and a counterexample trace, received exit $status."
        else
            echo "Expected TLC exit 0 and a completed-search marker, received exit $status."
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
