#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin" "$WORK/repo/scripts/ci" "$WORK/repo/formal"
cp "$ROOT/scripts/ci/check-tla-invariants.sh" "$WORK/repo/scripts/ci/"
cp -R "$ROOT/formal/tlaplus" "$WORK/repo/formal/"

cat >"$WORK/bin/tlc" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
config=""
while (($#)); do
    if [[ "$1" == "-config" ]]; then
        config="$2"
        break
    fi
    shift
done
case "$config" in
    MC_CarrierIndex_dag_first_pre_fix.cfg) invariant=IndexCompleteForWindow ;;
    MC_CarrierIndex_read_failure_pre_fix.cfg) invariant=AbsenceProofSound ;;
    MC_SoakDisk_floor_only_pre_fix.cfg) invariant=AdmissionRequiresBand ;;
    MC_DiskProbeAdmission_missing_sample_pre_fix.cfg) invariant=AdmissionRequiresSample ;;
    MC_DiskSampleValidation_numeric_prefix_pre_fix.cfg) invariant=AdmissionRequiresValidSample ;;
    MC_ActiveDiskProbe_missing_pre_fix.cfg) invariant=InvalidSampleRequiresInterrupt ;;
    MC_DiskEmergencyRecord_stop_first_pre_fix.cfg) invariant=StopRequiresRecord ;;
    MC_GuardianSupervision_unwatched_pre_fix.cfg) invariant=DeadGuardianRequiresInterrupt ;;
    MC_DiskProbeDeadline_unbounded_pre_fix.cfg) invariant=ProbeWithinDeadline ;;
    MC_DiskDiagnosticDeadline_per_root_pre_fix.cfg) invariant=AttributionWithinBudget ;;
    MC_DiskBreachRestart_clear_pre_fix.cfg) invariant=RetainedBreachStopsRestart ;;
    MC_DiskStopDeadline_unbounded_pre_fix.cfg) invariant=StopWithinBudget ;;
    MC_GuardianAdmission_unchecked_pre_fix.cfg) invariant=AdmissionRequiresGuardian ;;
    MC_BenchmarkBreachAdmission_unchecked_pre_fix.cfg) invariant=RetainedBreachPreventsBenchmark ;;
    MC_BenchmarkDiskAdmission_unchecked_pre_fix.cfg) invariant=BenchmarkRequiresBand ;;
    MC_BenchmarkDiskMonitor_late_pre_fix.cfg) invariant=BenchmarkBreachObserved ;;
    MC_BenchmarkCancellation_unwatched_death_pre_fix.cfg) invariant=BenchmarkCancellationObserved ;;
    MC_BenchmarkCancellation_unwatched_breach_pre_fix.cfg) invariant=BenchmarkCancellationObserved ;;
    MC_GuardianProgress_alive_only_pre_fix.cfg) invariant=StaleGuardianRequiresInterrupt ;;
    MC_GuardianProgressAdmission_unchecked_pre_fix.cfg) invariant=StaleProgressPreventsAdmission ;;
    MC_DiskHygieneDeadline_unbounded_pre_fix.cfg) invariant=HygieneWithinBudget ;;
    MC_DiskSettingsAdmission_unchecked_pre_fix.cfg) invariant=AdmissionRequiresValidDiskSettings ;;
    MC_CleanupOwnership_age_only_pre_fix.cfg) invariant=UnownedSessionPreserved ;;
    MC_CleanupOutcome_ignore_errors_pre_fix.cfg) invariant=CleanupFailurePreventsAdmission ;;
    MC_DockerCleanupOwnership_global_prune_pre_fix.cfg) invariant=UnownedDockerResourcesPreserved ;;
    MC_DockerExitStop_client_only_pre_fix.cfg) invariant=ParentExitStopsFixtureWriter ;;
    MC_IterationCrashRecovery_unrecorded_pre_fix.cfg) invariant=CrashRequiresRefusal ;;
    MC_DockerStopOwnership_name_only_pre_fix.cfg) invariant=UnownedWritersPreserved ;;
    MC_DockerStopFailure_ignored_pre_fix.cfg) invariant=FailedStopRetained ;;
    MC_HostStopOwnership_pattern_only_pre_fix.cfg | MC_HostStopOwnership_memory_pattern_pre_fix.cfg) invariant=UnownedHostWritersPreserved ;;
    MC_HostOomOwnership_pattern_only_pre_fix.cfg) invariant=UnownedPreferencesPreserved ;;
    MC_DockerOomOwnership_periodic_name_pre_fix.cfg) invariant=UnownedContainerPreferencesPreserved ;;
    MC_BenchmarkCrashRecovery_unrecorded_pre_fix.cfg) invariant=BenchmarkCrashRequiresRefusal ;;
    MC_DriverCrashStop_parent_group_pre_fix.cfg) invariant=DriverCrashStopsOwnedWriter ;;
    MC_CrashMonitorExit_unconditional_pre_fix.cfg) invariant=HandledExitHasNoExtraStop ;;
    MC_CrashMonitorDeath_startup_only_pre_fix.cfg) invariant=MonitorDeathStopsOwnedWriter ;;
    MC_BenchmarkMonitorDeath_iteration_only_pre_fix.cfg) invariant=BenchmarkMonitorDeathStopsOwnedWriter ;;
    MC_MonitorAdmission_benchmark_unchecked_pre_fix.cfg | MC_MonitorAdmission_iteration_unchecked_pre_fix.cfg) invariant=MonitorDeathPreventsAdmission ;;
    MC_InterruptedOutputDrain_drain_first_pre_fix.cfg) invariant=DrainRequiresOwnedStop ;;
    *) printf 'Model checking completed. No error has been found.\n'; exit 0 ;;
esac
if [[ "$config" == "$TEST_TLC_TARGET" ]]; then
    case "$TEST_TLC_RESULT" in
        clean) printf 'Model checking completed. No error has been found.\n'; exit 0 ;;
        wrong-invariant) invariant=TypeOK ;;
        tool-error) printf 'Error: The configuration could not be parsed.\n'; exit 1 ;;
        wrong-exit) printf 'Error: Invariant %s is violated.\n' "$invariant"; exit 1 ;;
        timeout) exit 124 ;;
    esac
fi
printf 'Error: Invariant %s is violated.\n' "$invariant"
printf 'Error: The behavior up to this point is:\n'
exit 12
SH
chmod +x "$WORK/bin/tlc"

for target in carrier_index/MC_CarrierIndex_dag_first_pre_fix \
    carrier_index/MC_CarrierIndex_read_failure_pre_fix soak_disk/MC_SoakDisk_floor_only_pre_fix \
    soak_disk/MC_DiskProbeAdmission_missing_sample_pre_fix \
    soak_disk/MC_DiskSampleValidation_numeric_prefix_pre_fix \
    soak_disk/MC_ActiveDiskProbe_missing_pre_fix \
    soak_disk/MC_DiskEmergencyRecord_stop_first_pre_fix \
    soak_disk/MC_GuardianSupervision_unwatched_pre_fix \
    soak_disk/MC_DiskProbeDeadline_unbounded_pre_fix \
    soak_disk/MC_DiskDiagnosticDeadline_per_root_pre_fix \
    soak_disk/MC_DiskBreachRestart_clear_pre_fix \
    soak_disk/MC_DiskStopDeadline_unbounded_pre_fix \
    soak_disk/MC_GuardianAdmission_unchecked_pre_fix \
    soak_disk/MC_BenchmarkBreachAdmission_unchecked_pre_fix \
    soak_disk/MC_BenchmarkDiskAdmission_unchecked_pre_fix \
    soak_disk/MC_BenchmarkDiskMonitor_late_pre_fix \
    soak_disk/MC_BenchmarkCancellation_unwatched_death_pre_fix \
    soak_disk/MC_BenchmarkCancellation_unwatched_breach_pre_fix \
    soak_disk/MC_GuardianProgress_alive_only_pre_fix \
    soak_disk/MC_GuardianProgressAdmission_unchecked_pre_fix \
    soak_disk/MC_DiskHygieneDeadline_unbounded_pre_fix \
    soak_disk/MC_DiskSettingsAdmission_unchecked_pre_fix \
    soak_disk/MC_CleanupOwnership_age_only_pre_fix \
    soak_disk/MC_CleanupOutcome_ignore_errors_pre_fix \
    soak_disk/MC_DockerCleanupOwnership_global_prune_pre_fix \
    soak_disk/MC_DockerExitStop_client_only_pre_fix \
    soak_disk/MC_IterationCrashRecovery_unrecorded_pre_fix \
    soak_disk/MC_DockerStopOwnership_name_only_pre_fix \
    soak_disk/MC_DockerStopFailure_ignored_pre_fix \
    soak_disk/MC_HostStopOwnership_pattern_only_pre_fix \
    soak_disk/MC_HostStopOwnership_memory_pattern_pre_fix \
    soak_disk/MC_HostOomOwnership_pattern_only_pre_fix \
    soak_disk/MC_DockerOomOwnership_periodic_name_pre_fix \
    soak_disk/MC_BenchmarkCrashRecovery_unrecorded_pre_fix \
    soak_disk/MC_DriverCrashStop_parent_group_pre_fix \
    soak_disk/MC_CrashMonitorExit_unconditional_pre_fix \
    soak_disk/MC_CrashMonitorDeath_startup_only_pre_fix \
    soak_disk/MC_BenchmarkMonitorDeath_iteration_only_pre_fix \
    soak_disk/MC_MonitorAdmission_benchmark_unchecked_pre_fix \
    soak_disk/MC_MonitorAdmission_iteration_unchecked_pre_fix \
    soak_disk/MC_InterruptedOutputDrain_drain_first_pre_fix; do
    for result in clean wrong-invariant tool-error wrong-exit timeout missing expected; do
        config="$WORK/repo/formal/tlaplus/$target.cfg"
        if [[ "$result" == missing ]]; then
            mv "$config" "$config.saved"
        fi
        status=0
        PATH="$WORK/bin:$PATH" TLA_TOOLS_JAR="$WORK/no-jar" RUN_EXHAUSTIVE_TLA=0 \
            TEST_TLC_TARGET="${target##*/}.cfg" TEST_TLC_RESULT="$result" \
            bash "$WORK/repo/scripts/ci/check-tla-invariants.sh" >"$WORK/gate.log" 2>&1 || status=$?
        if [[ "$result" == missing ]]; then
            mv "$config.saved" "$config"
        fi
        if [[ "$result" == expected ]]; then
            if ((status != 0)); then
                printf 'FAIL: The gate rejected the expected invariant violations.\n' >&2
                exit 1
            fi
        elif ((status == 0)); then
            printf 'FAIL: The formal gate accepted %s with result %s.\n' "$target" "$result" >&2
            exit 1
        fi
    done
done
printf 'PASS: The formal gate accepts only the expected invariant violations.\n'
